//! sync: aigw to aigw multi-table read-only incremental sync.
//!
//! Stage 86 (`aigw-migrate sync`): one-shot CLI that copies data between two
//! aigw database instances (PG/SQLite any combination).  Unlike
//! `remote-import`/`remote-export` (litellm to aigw heterogeneous migration
//! with table-name/camelCase/column-redirect bindings), this is same-schema
//! aigw to aigw: same table names, same snake_case, same PK `call_id`.  So it
//! reuses the low-level `SourcePool` / `CursorRange` / `insert_rows_batch`
//! abstractions but with empty column overrides (direct-match) and a new
//! `build_aigw_cursor_sql` anchor (`start_time`) that does not touch the
//! litellm `build_cursor_sql`.
//!
//! ## Boundaries
//! - Read-only append: `INSERT OR IGNORE` / `ON CONFLICT DO NOTHING`.  No
//!   UPDATE/DELETE propagation.  Re-running is idempotent (PK conflicts skipped).
//! - One-shot CLI: not a daemon, not scheduled, not CDC.
//! - Encrypted tables (`credentials` / `proxy_models`) copy ciphertext
//!   directly, assuming both ends share the same `master_key`.  Cross-key
//!   sync still belongs to `remote-import`.
//! - `config` table excluded by default (contains `master_key`).  Opt in
//!   explicitly via `--tables config`; `INSERT OR IGNORE` only fills missing
//!   rows, never overwrites an existing master_key.

use crate::native::{self, CursorRange, SourcePool, UnifiedRow};
use chrono::Timelike;
use futures::StreamExt;
use std::collections::HashMap;

/// All aigw tables the sync recognizes (11 business tables + config).
/// Used to validate `--tables` names.  `config` is known but not in the
/// default set.
pub const ALL_AIGW_TABLES: &[&str] = &[
    "virtual_keys",
    "spend_logs",
    "organizations",
    "teams",
    "users",
    "projects",
    "budgets",
    "organization_memberships",
    "team_memberships",
    "credentials",
    "proxy_models",
    "config",
];

/// Tables synced by default (11 business tables).  `config` is excluded
/// because it holds `master_key`; opt in with `--tables config`.
pub const DEFAULT_TABLES: &[&str] = &[
    "virtual_keys",
    "spend_logs",
    "organizations",
    "teams",
    "users",
    "projects",
    "budgets",
    "organization_memberships",
    "team_memberships",
    "credentials",
    "proxy_models",
];

/// The spend_logs body columns skipped by `--skip-body`.
const SPEND_LOGS_BODY_COLUMNS: &[&str] =
    &["messages", "response", "proxy_server_request"];

/// Per-table result of a sync run.
#[derive(Debug, Clone, Default)]
pub struct TableSyncStats {
    pub inserted: usize,
    pub ignored: usize,
}

/// Aggregate result of a sync run, keyed by table name.
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    pub per_table: HashMap<String, TableSyncStats>,
}

impl SyncStats {
    pub fn total_inserted(&self) -> usize {
        self.per_table.values().map(|s| s.inserted).sum()
    }
    pub fn total_ignored(&self) -> usize {
        self.per_table.values().map(|s| s.ignored).sum()
    }
}

/// Validate a user-supplied `--tables` list against the known aigw tables.
///
/// Returns the parsed list on success, or an error naming the first unknown
/// table so the CLI fails loudly instead of silently skipping a typo.
pub fn parse_tables(raw: &str) -> anyhow::Result<Vec<String>> {
    let known: std::collections::HashSet<&str> = ALL_AIGW_TABLES.iter().copied().collect();
    let mut out = Vec::new();
    for part in raw.split(',') {
        let name = part.trim().to_string();
        if name.is_empty() {
            continue;
        }
        if !known.contains(name.as_str()) {
            anyhow::bail!(
                "unknown table '{}' in --tables; known aigw tables: {:?}",
                name,
                ALL_AIGW_TABLES
            );
        }
        out.push(name);
    }
    if out.is_empty() {
        anyhow::bail!("--tables parsed to an empty list");
    }
    // De-duplicate while preserving order.
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.clone()));
    Ok(out)
}

/// Resolve the effective `--tables` list: explicit list if given, else the
/// default 11 business tables (config excluded).
pub fn resolve_tables(explicit: Option<&str>) -> anyhow::Result<Vec<String>> {
    match explicit {
        Some(raw) => parse_tables(raw),
        None => Ok(DEFAULT_TABLES.iter().map(|s| s.to_string()).collect()),
    }
}

/// Resolve the effective spend_logs cursor from `--days` and explicit bounds.
///
/// `--days N` means `start_time` within the last N days (UTC):
/// `resume_after = now - N days`, `end_before = now`.  If explicit
/// `--resume-after` / `--end-before` are also given, the stricter bound wins
/// (max resume_after, min end_before) instead of erroring — the caller asked
/// for the intersection of both windows.
pub fn resolve_cursor(
    days: Option<i64>,
    explicit_resume_after: Option<String>,
    explicit_end_before: Option<String>,
) -> anyhow::Result<CursorRange> {
    let mut cursor = CursorRange {
        resume_after: explicit_resume_after,
        end_before: explicit_end_before,
    };
    if let Some(n) = days {
        if n < 0 {
            anyhow::bail!("--days must be >= 0, got {}", n);
        }
        let now = chrono::Utc::now();
        // resume = start of the day N days ago (00:00:00Z), so the SQL `>=`
        // includes all rows from that day onwards.
        let resume = (now - chrono::Duration::days(n))
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
            .unwrap_or(now - chrono::Duration::days(n));
        cursor.resume_after = Some(match cursor.resume_after {
            Some(existing) => {
                // Stricter (later) lower bound wins.
                let existing_dt = parse_iso8601(&existing)?;
                if resume > existing_dt {
                    resume.to_rfc3339()
                } else {
                    existing
                }
            }
            None => resume.to_rfc3339(),
        });
        cursor.end_before = Some(match cursor.end_before {
            Some(existing) => {
                let existing_dt = parse_iso8601(&existing)?;
                if now < existing_dt {
                    now.to_rfc3339()
                } else {
                    existing
                }
            }
            None => now.to_rfc3339(),
        });
    }
    Ok(cursor)
}

fn parse_iso8601(s: &str) -> anyhow::Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| anyhow::anyhow!("invalid ISO 8601 datetime '{}': {}", s, e))
}

/// Parse a `--test-range=min,max` string into a (lo, hi) pair.
pub fn parse_test_range(raw: &str) -> anyhow::Result<(usize, usize)> {
    let mut parts = raw.splitn(2, ',');
    let lo: usize = parts
        .next()
        .unwrap_or("")
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --test-range min: '{}'", raw))?;
    let hi: usize = parts
        .next()
        .unwrap_or("")
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid --test-range max: '{}'", raw))?;
    if lo == 0 || hi == 0 {
        anyhow::bail!("--test-range min and max must be >= 1, got {}-{}", lo, hi);
    }
    if lo > hi {
        anyhow::bail!("--test-range min ({}) must be <= max ({})", lo, hi);
    }
    Ok((lo, hi))
}

/// Test-mode sync: sample spend_logs per hour with `ORDER BY random()`.
///
/// Iterates each full clock-hour in the cursor window.  For each hour:
/// 1. Pick a random limit N in `[lo ..= hi]`.
/// 2. `SELECT … FROM spend_logs WHERE start_time >= hour AND start_time < next_hour ORDER BY random() LIMIT N`
/// 3. Batch-INSERT into the target (idempotent).
///
/// Progress is printed every 10 hours or after each batch if `debug`.
pub async fn run_sync_test(
    source_url: &str,
    target_url: &str,
    cursor: &CursorRange,
    lo: usize,
    hi: usize,
    _batch_size: usize,
    debug: bool,
) -> anyhow::Result<SyncStats> {
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;

    // Build column lists (same as sync_spend_logs).
    let (id_column, overrides) = spend_logs_id_mapping(&source).await?;
    let tgt_cols_all = target.column_types("spend_logs").await?;
    let filtered_cols: Vec<(String, String, bool)> = tgt_cols_all;
    let select_columns: Vec<String> = filtered_cols
        .iter()
        .map(|(n, _, _)| {
            if n == "call_id" {
                id_column.clone()
            } else {
                n.clone()
            }
        })
        .collect();
    let col_list = select_columns
        .iter()
        .map(|c| source.quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");

    // Parse the time window.
    let after = cursor
        .resume_after
        .clone()
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
    let before = cursor
        .end_before
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let start_dt = parse_iso8601(&after)?;
    let end_dt = parse_iso8601(&before)?;

    // Round start down to the hour.
    let hour_start = start_dt
        .date_naive()
        .and_hms_opt(start_dt.time().hour(), 0, 0)
        .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
        .unwrap_or(start_dt);

    let total_hours = ((end_dt - hour_start).num_hours().max(0) + 1) as usize;
    eprintln!(
        "  [TEST] spend_logs sampling: {} hours, {}-{} rows/hour (random), window={}..{}",
        total_hours, lo, hi, after, before
    );

    let _now = chrono::Utc::now();
    let t_start = std::time::Instant::now();
    let mut inserted_total = 0usize;
    let mut ignored_total = 0usize;

    for h in 0..total_hours {
        let hour_start_dt = hour_start + chrono::Duration::hours(h as i64);
        let hour_end_dt = hour_start_dt + chrono::Duration::hours(1);

        // Clamp to the actual window.
        let w_start = if hour_start_dt < start_dt {
            start_dt
        } else {
            hour_start_dt
        };
        let w_end = if hour_end_dt > end_dt {
            end_dt
        } else {
            hour_end_dt
        };
        if w_start >= w_end {
            continue;
        }

        // Simple deterministic per-hour "random" using hour index.
        let limit = if lo == hi {
            lo
        } else {
            lo + (h % (hi - lo + 1))
        };

        let lit_start = source.time_literal(&w_start.to_rfc3339());
        let lit_end = source.time_literal(&w_end.to_rfc3339());
        let sql = format!(
            "SELECT {} FROM {} WHERE {} >= {} AND {} < {} ORDER BY random() LIMIT {}",
            col_list,
            source.quote_ident("spend_logs"),
            source.quote_ident("start_time"),
            lit_start,
            source.quote_ident("start_time"),
            lit_end,
            limit,
        );

        let rows = source.read_rows_sql(&sql).await?;
        if !rows.is_empty() {
            let (ins, ign) =
                native::insert_rows_batch(&target, "spend_logs", &filtered_cols, &rows, &overrides)
                    .await?;
            inserted_total += ins;
            ignored_total += ign;
        }

        if h % 10 == 0 || (debug && !rows.is_empty()) {
            let rate = inserted_total as f64 / t_start.elapsed().as_secs_f64().max(0.001);
            eprintln!(
                "  [PROGRESS] test hour {}/{}: inserted={} ignored={} ({:.0} rows/s)",
                h + 1,
                total_hours,
                inserted_total,
                ignored_total,
                rate
            );
        }
    }

    let elapsed = t_start.elapsed();
    let rate = inserted_total as f64 / elapsed.as_secs_f64().max(0.001);
    eprintln!(
        "  [TIMING] spend_logs test-sync: {:?} ({} inserted, {} ignored, {:.0} rows/s)",
        elapsed, inserted_total, ignored_total, rate
    );

    let mut stats = SyncStats::default();
    stats.per_table.insert(
        "spend_logs".into(),
        TableSyncStats {
            inserted: inserted_total,
            ignored: ignored_total,
        },
    );
    Ok(stats)
}

/// Run an aigw to aigw sync.
///
/// Connects to `source_url` and `target_url` (any PG/SQLite/MySQL combo the
/// `SourcePool` supports), iterates `tables`, and appends each table's rows
/// to the target with idempotent `INSERT OR IGNORE` / `ON CONFLICT DO
/// NOTHING`.  `spend_logs` uses the `start_time` cursor for incremental
/// `--days` filtering; all other tables are full-table idempotent copies.
///
/// `skip_body` nulls the three large body columns of `spend_logs` on the
/// target (they are simply not selected from the source).  Encrypted tables
/// (`credentials` / `proxy_models`) are copied as plain rows — ciphertext
/// bytes travel verbatim, no key rotation.
pub async fn run_sync(
    source_url: &str,
    target_url: &str,
    tables: &[String],
    cursor: &CursorRange,
    skip_body: bool,
    batch_size: usize,
    debug: bool,
) -> anyhow::Result<SyncStats> {
    let batch_size = if batch_size == 0 { 10 } else { batch_size };
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;
    let empty_overrides: HashMap<String, String> = HashMap::new();

    if debug {
        eprintln!("[DEBUG] source kind: {:?}, target kind: {:?}", source.kind(), target.kind());
    }

    let mut stats = SyncStats::default();
    for table in tables {
        let t = std::time::Instant::now();
        let result = if table == "spend_logs" {
            sync_spend_logs(&source, &target, cursor, skip_body, batch_size, debug).await
        } else {
            sync_plain_table(&source, &target, table, &empty_overrides).await
        };
        match result {
            Ok(s) => {
                eprintln!(
                    "  {} -> {}: inserted={} ignored={} ({:?})",
                    table, table, s.inserted, s.ignored, t.elapsed()
                );
                stats.per_table.insert(table.clone(), s);
            }
            Err(e) => {
                eprintln!("  [SKIP] {}: {}", table, e);
                stats.per_table.insert(
                    table.clone(),
                    TableSyncStats {
                        inserted: 0,
                        ignored: 0,
                    },
                );
            }
        }
    }
    Ok(stats)
}

/// Full-table idempotent copy for plain tables (and encrypted tables, which
/// are treated as plain — ciphertext copied verbatim).
async fn sync_plain_table(
    source: &SourcePool,
    target: &SourcePool,
    table: &str,
    overrides: &HashMap<String, String>,
) -> anyhow::Result<TableSyncStats> {
    let rows = source.read_rows(table).await?;
    if rows.is_empty() {
        return Ok(TableSyncStats::default());
    }
    let tgt_cols = target.column_types(table).await?;
    if tgt_cols.is_empty() {
        anyhow::bail!("target table {} has no columns", table);
    }
    let inserted = native::insert_rows(target, table, &tgt_cols, &rows, overrides).await?;
    Ok(TableSyncStats {
        inserted,
        ignored: rows.len().saturating_sub(inserted),
    })
}

/// Returns the identity column name for the source `spend_logs` table and
/// any column overrides needed for INSERT.  Handles pre-023 and post-023
/// schema divergence:
///
/// - pre-023: the source has `request_id` as PK (no `call_id`).  Since the
///   target is always post-023 with `call_id` (PK) + `request_id` (upstream
///   provider id), we map `target.call_id ← source.request_id`.
/// - post-023: the source has `call_id` (PK) + `request_id` (upstream).  Both
///   columns match the target by name directly — same-schema, no overrides.
///
/// Returns `(id_column_name, overrides)`.
async fn spend_logs_id_mapping(source: &SourcePool) -> anyhow::Result<(String, HashMap<String, String>)> {
    let src_cols = source.column_types("spend_logs").await?;
    let has_call_id = src_cols.iter().any(|(n, _, _)| n == "call_id");
    let has_request_id = src_cols.iter().any(|(n, _, _)| n == "request_id");
    if has_call_id {
        // post-023: same schema on both ends, direct match.
        Ok(("call_id".into(), HashMap::new()))
    } else if has_request_id {
        let mut overrides = HashMap::new();
        overrides.insert("call_id".into(), "request_id".into());
        Ok(("request_id".into(), overrides))
    } else {
        anyhow::bail!("spend_logs source has neither call_id nor request_id column");
    }
}

/// Stream `spend_logs` from the source through the `start_time` cursor and
/// batch-insert into the target.  `skip_body` prunes the three body columns
/// from the source SELECT so they never leave the source DB.
async fn sync_spend_logs(
    source: &SourcePool,
    target: &SourcePool,
    cursor: &CursorRange,
    skip_body: bool,
    batch_size: usize,
    debug: bool,
) -> anyhow::Result<TableSyncStats> {
    // Detect source schema (pre-023 vs post-023) and build overrides.
    let (id_column, overrides) = spend_logs_id_mapping(source).await?;

    // Target columns (filtered if --skip-body drops body columns).
    let tgt_cols_all = target.column_types("spend_logs").await?;
    let skip_set: std::collections::HashSet<&str> = if skip_body {
        SPEND_LOGS_BODY_COLUMNS.iter().copied().collect()
    } else {
        std::collections::HashSet::new()
    };
    let filtered_cols: Vec<(String, String, bool)> = tgt_cols_all
        .iter()
        .filter(|(c, _, _)| !skip_set.contains(c.as_str()))
        .cloned()
        .collect();
    if filtered_cols.is_empty() {
        anyhow::bail!("spend_logs: all columns filtered out");
    }

    // Source SELECT projection: map target column names → actual source column
    // names.  In post-023 the mapping is identity; in pre-023 we must replace
    // `call_id` (which doesn't exist on the source) with `request_id`.
    let select_columns: Vec<String> = filtered_cols
        .iter()
        .map(|(n, _, _)| {
            if n == "call_id" {
                id_column.clone()
            } else {
                n.clone()
            }
        })
        .collect();

    if debug {
        let col_names: Vec<&str> = select_columns.iter().map(|s| s.as_str()).collect();
        eprintln!("[DEBUG] source id_column: \"{}\"", id_column);
        eprintln!(
            "[DEBUG] select_columns ({}): [{}]",
            select_columns.len(),
            col_names.join(", ")
        );
        eprintln!(
            "[DEBUG] insert target cols ({}): [{}]",
            filtered_cols.len(),
            filtered_cols.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        );
        if !overrides.is_empty() {
            eprintln!("[DEBUG] column overrides {:?}", overrides);
        }
        // Print equivalent SQL that would runnon-PG path via build_aigw_cursor_sql,
        // PG uses keyset pagination which is similar but with (start_time, id) anchor).
        let sql = source.build_aigw_cursor_sql("spend_logs", cursor, Some(batch_size), Some(&select_columns));
        eprintln!("[DEBUG] cursor SQL (SQLite/MySQL; PG uses keyset pagination):");
        eprintln!("[DEBUG]   {}", sql);
        eprintln!(
            "[DEBUG] target INSERT: {} (col_names) ON CONFLICT DO NOTHING, batch_size={}",
            target.insert_prefix(),
            batch_size
        );
    }

    // Count matching rows first for progress context.
    let quoted = source.quote_ident("spend_logs");
    let mut conditions: Vec<String> = Vec::new();
    if let Some(ref after) = cursor.resume_after {
        conditions.push(format!("{} >= {}", quoted, source.time_literal(after)));
    }
    if let Some(ref before) = cursor.end_before {
        conditions.push(format!("{} < {}", quoted, source.time_literal(before)));
    }
    let count_sql = if conditions.is_empty() {
        format!("SELECT COUNT(*) FROM {}", quoted)
    } else {
        format!("SELECT COUNT(*) FROM {} WHERE {}", quoted, conditions.join(" AND "))
    };
    let total_est = source.query_scalar_string(&count_sql)
        .await
        .ok()
        .flatten()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(-1);
    if total_est >= 0 {
        eprintln!("  [EST] spend_logs: ~{} rows matching cursor", total_est);
    }

    let mut stream = source.stream_rows_with_cursor_aigw(
        "spend_logs",
        cursor,
        Some(&select_columns),
        batch_size,
    );

    let t_start = std::time::Instant::now();
    let log_every = (batch_size * 10).max(10);
    let mut inserted_total = 0usize;
    let mut ignored_total = 0usize;
    let mut since_log = 0usize;
    let mut last_cursor: Option<String> = None;
    let mut buf: Vec<UnifiedRow> = Vec::with_capacity(batch_size);
    let mut write_acc = std::time::Duration::ZERO;
    eprintln!("  [START] spend_logs: streaming from source, batch_size={} ...", batch_size);
    while let Some(row_res) = stream.next().await {
        let row = row_res?;
        // Track cursor position for progress reporting.
        for (col_name, val) in &row {
            if col_name == "start_time" {
                if let Some(s) = val.as_str() {
                    last_cursor = Some(s.to_string());
                }
                break;
            }
        }
        buf.push(row);
        since_log += 1;
        if buf.len() >= batch_size {
            let batch = std::mem::replace(&mut buf, Vec::with_capacity(batch_size));
            let t_write = std::time::Instant::now();
            let (ins, ign) =
                native::insert_rows_batch(target, "spend_logs", &filtered_cols, &batch, &overrides)
                    .await?;
            let write_elapsed = t_write.elapsed();
            write_acc += write_elapsed;
            inserted_total += ins;
            ignored_total += ign;
            if since_log >= log_every {
                let total_elapsed = t_start.elapsed();
                let rate = inserted_total as f64 / total_elapsed.as_secs_f64().max(0.001);
                if debug {
                    let read_elapsed = total_elapsed.saturating_sub(write_acc);
                    eprintln!(
                        "  [PROGRESS] spend_logs: scanned={} inserted={} ignored={} ({:.0} rows/s, read={:?} write={:?}, cursor={})",
                        since_log,
                        inserted_total,
                        ignored_total,
                        rate,
                        read_elapsed,
                        write_acc,
                        last_cursor.as_deref().unwrap_or("<none>"),
                    );
                } else {
                    eprintln!(
                        "  [PROGRESS] spend_logs: scanned={} inserted={} ignored={} ({:.0} rows/s, cursor={})",
                        since_log,
                        inserted_total,
                        ignored_total,
                        rate,
                        last_cursor.as_deref().unwrap_or("<none>"),
                    );
                }
                since_log = 0;
            }
        }
    }
    if !buf.is_empty() {
        let (ins, ign) =
            native::insert_rows_batch(target, "spend_logs", &filtered_cols, &buf, &overrides).await?;
        inserted_total += ins;
        ignored_total += ign;
    }
    let elapsed = t_start.elapsed();
    let rate = inserted_total as f64 / elapsed.as_secs_f64().max(0.001);
    if debug {
        let read_elapsed = elapsed.saturating_sub(write_acc);
        eprintln!(
            "  [TIMING] spend_logs: {:?} ({} inserted, {} ignored, {:.0} rows/s, read={:?} write={:?})",
            elapsed, inserted_total, ignored_total, rate, read_elapsed, write_acc
        );
    } else {
        eprintln!(
            "  [TIMING] spend_logs: {:?} ({} inserted, {} ignored, {:.0} rows/s)",
            elapsed, inserted_total, ignored_total, rate
        );
    }
    if let Some(ts) = &last_cursor {
        eprintln!("  [RESUME] spend_logs last cursor: {}", ts);
    }
    Ok(TableSyncStats {
        inserted: inserted_total,
        ignored: ignored_total,
    })
}
