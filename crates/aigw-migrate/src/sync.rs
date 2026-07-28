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
        let resume = now - chrono::Duration::days(n);
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
) -> anyhow::Result<SyncStats> {
    let batch_size = if batch_size == 0 { 10 } else { batch_size };
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;
    let empty_overrides: HashMap<String, String> = HashMap::new();

    let mut stats = SyncStats::default();
    for table in tables {
        let t = std::time::Instant::now();
        let result = if table == "spend_logs" {
            sync_spend_logs(&source, &target, cursor, skip_body, batch_size, &empty_overrides)
                .await
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

/// Stream `spend_logs` from the source through the `start_time` cursor and
/// batch-insert into the target.  `skip_body` prunes the three body columns
/// from the source SELECT so they never leave the source DB.
async fn sync_spend_logs(
    source: &SourcePool,
    target: &SourcePool,
    cursor: &CursorRange,
    skip_body: bool,
    batch_size: usize,
    overrides: &HashMap<String, String>,
) -> anyhow::Result<TableSyncStats> {
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

    // Source SELECT projection: every target column we will insert.  Because
    // aigw is same-schema on both ends, the source column names equal the
    // target column names — direct match, no overrides.
    let select_columns: Vec<String> = filtered_cols.iter().map(|(n, _, _)| n.clone()).collect();

    let mut stream = source.stream_rows_with_cursor_aigw(
        "spend_logs",
        cursor,
        None,
        Some(&select_columns),
        batch_size,
    );

    let mut inserted_total = 0usize;
    let mut ignored_total = 0usize;
    let mut buf: Vec<UnifiedRow> = Vec::with_capacity(batch_size);
    while let Some(row_res) = stream.next().await {
        let row = row_res?;
        buf.push(row);
        if buf.len() >= batch_size {
            let batch = std::mem::replace(&mut buf, Vec::with_capacity(batch_size));
            let (ins, ign) =
                native::insert_rows_batch(target, "spend_logs", &filtered_cols, &batch, overrides)
                    .await?;
            inserted_total += ins;
            ignored_total += ign;
        }
    }
    if !buf.is_empty() {
        let (ins, ign) =
            native::insert_rows_batch(target, "spend_logs", &filtered_cols, &buf, overrides).await?;
        inserted_total += ins;
        ignored_total += ign;
    }
    Ok(TableSyncStats {
        inserted: inserted_total,
        ignored: ignored_total,
    })
}
