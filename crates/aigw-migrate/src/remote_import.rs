//! remote-import: Full litellm → aigw migration with encryption key rotation.
//!
//! Pipeline:
//!   1. Connect to source (litellm) and target (aigw) via native pools
//!   2. Extract litellm master_key from LiteLLM_Config or CLI arg
//!   3. Migrate plain tables (no encrypted fields)
//!   4. Migrate credentials — decrypt credential_values, re-encrypt with aigw key
//!   5. Migrate proxy_models — decrypt litellm_params, re-encrypt with aigw key
//!   6. Batch migrate spend_logs
//!
//! All cross-database type coercion is handled by [crate::native].

use crate::native::{self, CursorRange, SourcePool};
use futures::StreamExt;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            result.push(c);
        }
    }
    result
}

/// Build a column name override mapping (camelCase → snake_case) from source columns.
fn build_snake_overrides(src_columns: &[String]) -> HashMap<String, String> {
    src_columns
        .iter()
        .map(|n| (camel_to_snake(n), n.clone()))
        .collect()
}


/// Return a col-type string that forces JSON-literal coercion in
/// `value_to_target_literal`, unless the target already advertises a JSON type.
///
/// aigw-core's `proxy_models.litellm_params` / `credentials.credential_values`
/// hold JSON values decoded at runtime as `serde_json::Value`.  Their storage
/// types differ per backend:
///   * PG      → JSONB  (contains "json", passthrough)
///   * MySQL   → JSON   (contains "json", passthrough)
///   * SQLite  → TEXT   (would NOT match, so we override to "json")
fn json_column_type_for(actual: &str, kind: native::DbKind) -> String {
    let ty = actual.to_lowercase();
    if ty.contains("json") {
        return actual.to_string();
    }
    match kind {
        native::DbKind::Sqlite => "json".to_string(),
        // On PG/MySQL the actual storage IS JSON/JSONB — this branch is
        // defensive: if `column_types` ever returned something odd, we still
        // route through the JSON literal path so encrypted blobs get wrapped.
        _ => "json".to_string(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Master key extraction
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn extract_source_master_key(source: &SourcePool) -> anyhow::Result<Option<String>> {
    let tbl = source.quote_ident("LiteLLM_Config");
    // On PG, `param_value` may be `jsonb` (litellm >= v1.61) or `text` (older
    // versions).  Casting to text with `::text` handles both uniformly so
    // `query_scalar_string` can always decode it as `String`.
    let col = if source.kind() == native::DbKind::Postgres {
        "param_value::text"
    } else {
        "param_value"
    };

    // Strategy 1: legacy flat key
    let sql = format!(
        "SELECT {col} FROM {} WHERE param_name = 'litellm_master_key'",
        tbl
    );
    if let Some(val) = source.query_scalar_string(&sql).await? {
        // JSONB always wraps scalar strings in double quotes (e.g. `"sk-..."`).
        // Strip them so the caller sees the raw key regardless of source
        // column type.
        let val = strip_jsonb_quotes(&val);
        if !val.is_empty() {
            return Ok(Some(val));
        }
    }

    // Strategy 2: general_settings JSON
    let sql = format!(
        "SELECT {col} FROM {} WHERE param_name = 'general_settings'",
        tbl
    );
    if let Some(val) = source.query_scalar_string(&sql).await? {
        if let Ok(parsed) = serde_json::from_str::<Value>(&val) {
            if let Some(mk) = parsed.get("master_key").and_then(|v| v.as_str()) {
                if !mk.is_empty() {
                    return Ok(Some(mk.to_string()));
                }
            }
        }
    }

    Ok(None)
}

/// JSONB scalar strings are serialised with surrounding double quotes when cast
/// to text (e.g. `"sk-...".to_string()`).  TEXT columns don't.  Strip a single
/// pair of outer quotes if present.
fn strip_jsonb_quotes(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        // serde_json handles escape sequences like \" and \\ correctly.
        if let Ok(v) = serde_json::from_str::<String>(s) {
            return v;
        }
    }
    s.to_string()
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Plain table migration
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Tables without encrypted fields (plain copy).
const PLAIN_TABLES: &[(&str, &str)] = &[
    ("LiteLLM_OrganizationTable", "organizations"),
    ("LiteLLM_TeamTable", "teams"),
    ("LiteLLM_UserTable", "users"),
    ("LiteLLM_ProjectTable", "projects"),
    ("LiteLLM_BudgetTable", "budgets"),
    ("LiteLLM_OrganizationMembership", "organization_memberships"),
    ("LiteLLM_TeamMembership", "team_memberships"),
    ("LiteLLM_VerificationToken", "virtual_keys"),
    ("LiteLLM_Config", "config"),
];

async fn migrate_plain_table(
    source: &SourcePool,
    target: &SourcePool,
    src_table: &str,
    tgt_table: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows(src_table).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] {}: {}", src_table, e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types(tgt_table).await?;
    let overrides = build_snake_overrides(&src_col_names);

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] {}: no target columns", src_table);
        return Ok(0);
    }

    let count = native::insert_rows(target, tgt_table, &tgt_col_info, &rows, &overrides).await?;
    Ok(count)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Credentials migration (with key rotation)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_credentials(
    source: &SourcePool,
    target: &SourcePool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows("LiteLLM_CredentialsTable").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_CredentialsTable: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("credentials").await?;
    let overrides = build_snake_overrides(&src_col_names);

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] credentials: no target columns");
        return Ok(0);
    }

    let values_col = tgt_col_info
        .iter()
        .position(|(n, _)| n == "credential_values");
    // `credential_info` is also a JSON payload in aigw-core (see Credential in
    // models.rs); force JSON coercion for it too so that ports where the
    // target column is plain TEXT don't fall through to raw string storage.
    let info_col = tgt_col_info
        .iter()
        .position(|(n, _)| n == "credential_info");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();

        let values: Vec<String> = tgt_col_info
            .iter()
            .enumerate()
            .map(|(idx, (col_name, col_type))| {
                if values_col == Some(idx) {
                    // credential_values: same shape variance as litellm_params —
                    // PG jsonb → Object/Array, PG text / SQLite → String.
                    // Normalise to a JSON string before rotating.
                    let raw = row_map.get(col_name.as_str()).copied().unwrap_or(&Value::Null);
                    let encrypted_str: String = match raw {
                        Value::Null => String::new(),
                        Value::String(s) => s.clone(),
                        Value::Object(_) | Value::Array(_) => raw.to_string(),
                        other => other.to_string(),
                    };
                    let literal_type = json_column_type_for(col_type, target.kind());
                    let v = if encrypted_str.is_empty() || encrypted_str == "{}" {
                        Value::String(encrypted_str)
                    } else {
                        let rotated = rotate_field(&encrypted_str, source_key, target_key, &mut skipped);
                        Value::String(rotated.unwrap_or(encrypted_str))
                    };
                    native::value_to_target_literal(&v, &literal_type, target.kind())
                } else if info_col == Some(idx) {
                    let v = row_map
                        .get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&Value::Null);
                    let literal_type = json_column_type_for(col_type, target.kind());
                    native::value_to_target_literal(v, &literal_type, target.kind())
                } else {
                    let v = row_map
                        .get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&Value::Null);
                    native::value_to_target_literal(v, col_type, target.kind())
                }
            })
            .collect();

        let tbl_quoted = target.quote_ident("credentials");
        let quoted_cols: Vec<String> = tgt_col_info.iter().map(|(n, _)| target.quote_ident(n)).collect();
        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(),
            tbl_quoted,
            quoted_cols.join(", "),
            values.join(", "),
            target.conflict_clause(),
        );
        target.execute_raw(&sql).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} credential rows due to crypto errors", skipped);
    }
    Ok(inserted)
}

fn rotate_field(encrypted: &str, source_key: &str, target_key: &str, skipped: &mut usize) -> Option<String> {
    if encrypted.starts_with('{') {
        // JSON object — rotate individual encrypted fields.
        // `rotate_json_fields` already re-encrypts each field with target_key,
        // so we return the rotated JSON string AS-IS — no outer encryption.
        match serde_json::from_str::<Value>(encrypted) {
            Ok(json_val) => {
                match aigw_core::rotate_json_fields(&json_val, source_key, target_key) {
                    Ok(rotated) => return Some(rotated),
                    Err(_) => { *skipped += 1; }
                }
            }
            Err(_) => { *skipped += 1; }
        }
    } else {
        // Simple encrypted string — decrypt with source key, re-encrypt with target key
        if let Ok(plaintext) = aigw_core::decrypt_litellm_value(encrypted, source_key) {
            if let Ok(re_encrypted) = aigw_core::encrypt_litellm_value(&plaintext, target_key) {
                return Some(re_encrypted);
            }
        }
        *skipped += 1;
    }
    None
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Proxy models migration (with key rotation)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_proxy_models(
    source: &SourcePool,
    target: &SourcePool,
    source_key: &str,
    target_key: &str,
) -> anyhow::Result<usize> {
    let rows = match source.read_rows("LiteLLM_ProxyModelTable").await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  [SKIP] LiteLLM_ProxyModelTable: {}", e);
            return Ok(0);
        }
    };
    if rows.is_empty() {
        return Ok(0);
    }

    let src_col_names: Vec<String> = rows[0].iter().map(|(n, _)| n.clone()).collect();
    let tgt_col_info = target.column_types("proxy_models").await?;
    let overrides = build_snake_overrides(&src_col_names);

    if tgt_col_info.is_empty() {
        eprintln!("  [SKIP] proxy_models: no target columns");
        return Ok(0);
    }

    let params_col = tgt_col_info.iter().position(|(n, _)| n == "litellm_params");
    // model_info is also a JSON payload the runtime decodes as serde_json::Value.
    let info_col = tgt_col_info.iter().position(|(n, _)| n == "model_info");

    let mut inserted = 0usize;
    let mut skipped = 0usize;
    for row in &rows {
        let row_map: HashMap<&str, &Value> = row.iter().map(|(n, v)| (n.as_str(), v)).collect();

        let values: Vec<String> = tgt_col_info
            .iter()
            .enumerate()
            .map(|(idx, (col_name, col_type))| {
                if params_col == Some(idx) {
                    // litellm's `litellm_params` may arrive as:
                    //   * PG `jsonb`   → decoded as `Value::Object` / `Value::Array`
                    //   * PG `text`    → decoded as `Value::String` (full JSON text)
                    //   * SQLite TEXT  → decoded as `Value::String` or (via BLOB fallback) `Value::Object`
                    //   * whole-blob encrypted → `Value::String` not starting with `{`
                    // Normalise to a JSON string so `rotate_field` can walk it uniformly.
                    let raw = row_map.get(col_name.as_str()).copied().unwrap_or(&Value::Null);
                    let value_str: String = match raw {
                        Value::Null => String::new(),
                        Value::String(s) => s.clone(),
                        Value::Object(_) | Value::Array(_) => raw.to_string(),
                        other => other.to_string(),
                    };
                    // Force JSON coercion at the target side even when the target
                    // column type is plain TEXT (SQLite / older PG variants).  The
                    // runtime always decodes `litellm_params` as `serde_json::Value`
                    // (see `ProxyModel` in aigw-core), so a bare encrypted string
                    // like "gAAAAAB..." would fail with EOF at line 1 col 0.
                    // `value_to_target_literal` with `col_type = "json"` wraps
                    // non-JSON strings in JSON double quotes.
                    let literal_type = json_column_type_for(col_type, target.kind());
                    if value_str.is_empty() {
                        return native::value_to_target_literal(
                            &Value::String("".into()),
                            &literal_type,
                            target.kind(),
                        );
                    }
                    let rotated = rotate_field(&value_str, source_key, target_key, &mut skipped);
                    let v = Value::String(rotated.unwrap_or(value_str));
                    native::value_to_target_literal(&v, &literal_type, target.kind())
                } else if info_col == Some(idx) {
                    let v = row_map
                        .get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&Value::Null);
                    let literal_type = json_column_type_for(col_type, target.kind());
                    native::value_to_target_literal(v, &literal_type, target.kind())
                } else {
                    let v = row_map
                        .get(col_name.as_str())
                        .or_else(|| overrides.get(col_name.as_str())
                            .and_then(|m| row_map.get(m.as_str())))
                        .copied()
                        .unwrap_or(&Value::Null);
                    native::value_to_target_literal(v, col_type, target.kind())
                }
            })
            .collect();

        let tbl_quoted = target.quote_ident("proxy_models");
        let quoted_cols: Vec<String> = tgt_col_info.iter().map(|(n, _)| target.quote_ident(n)).collect();
        let sql = format!(
            "{}{} ({}) VALUES ({}){}",
            target.insert_prefix(),
            tbl_quoted,
            quoted_cols.join(", "),
            values.join(", "),
            target.conflict_clause(),
        );
        target.execute_raw(&sql).await?;
        inserted += 1;
    }

    if skipped > 0 {
        eprintln!("  [WARN] Skipped {} model rows due to crypto errors", skipped);
    }
    Ok(inserted)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Spend logs migration (batch, no crypto)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn migrate_spend_logs(
    source: &SourcePool,
    target: &SourcePool,
    limit: Option<usize>,
    cursor: &CursorRange,
    _skip_body: bool,
    skip_columns_set: &HashSet<(String, String)>,
    batch_size: usize,
) -> anyhow::Result<usize> {
    // ── Step A: figure out target columns (filter out skips) ─────────────
    let tgt_col_info_all = target.column_types("spend_logs").await?;

    let skipped_list: Vec<String> = tgt_col_info_all
        .iter()
        .filter(|(col, _)| skip_columns_set.contains(&("spend_logs".to_string(), col.clone())))
        .map(|(col, _)| format!("spend_logs.{}", col))
        .collect();
    if !skipped_list.is_empty() {
        eprintln!("  [SKIP-COLUMNS] spend_logs: {:?}", skipped_list);
    }

    let filtered_cols: Vec<(String, String)> = tgt_col_info_all
        .into_iter()
        .filter(|(col, _)| !skip_columns_set.contains(&("spend_logs".to_string(), col.clone())))
        .collect();

    if filtered_cols.is_empty() {
        eprintln!("  [SKIP] spend_logs: all columns filtered out");
        return Ok(0);
    }

    // ── Step B: figure out source columns to SELECT ─────────────────────
    //
    // Upstream `LiteLLM_SpendLogs` mixes camelCase (`startTime`, `endTime`,
    // `completionStartTime`) with snake_case.  We drop any source column
    // whose snake_case-mapped name is in `skip_columns_set` — this makes
    // `--skip-body` prune messages/response/proxy_server_request from the
    // SELECT itself, so they never leave the source DB.  `startTime` is
    // preserved unconditionally because pagination needs it.
    let src_col_info = source.column_types("LiteLLM_SpendLogs").await?;
    let src_col_names_all: Vec<String> =
        src_col_info.iter().map(|(n, _)| n.clone()).collect();
    let mut select_columns: Vec<String> = src_col_names_all
        .iter()
        .filter(|src_name| {
            let mapped = camel_to_snake(src_name);
            !skip_columns_set.contains(&("spend_logs".to_string(), mapped))
                && !skip_columns_set.contains(&("spend_logs".to_string(), src_name.to_string()))
        })
        .cloned()
        .collect();
    if !select_columns.iter().any(|c| c == "startTime")
        && src_col_names_all.iter().any(|c| c == "startTime")
    {
        select_columns.push("startTime".to_string());
    }
    eprintln!(
        "  [SELECT] spend_logs source cols: {} / {} (pruned {} skipped)",
        select_columns.len(),
        src_col_names_all.len(),
        src_col_names_all.len() - select_columns.len()
    );

    // Snake-case overrides so target-side lookup finds camelCase source cols.
    let overrides = build_snake_overrides(&select_columns);

    // ── Step C: producer/consumer pipeline ───────────────────────────────
    //
    // Producer streams rows from the source (server-side cursor) and pushes
    // Vec<UnifiedRow> batches into a bounded channel.  Consumer drains the
    // channel and runs `insert_rows_batch` — one multi-row INSERT per batch
    // inside a transaction.  Channel capacity 4 balances backpressure vs
    // throughput: producer stays a couple of batches ahead.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<native::UnifiedRow>>(4);
    let cursor_owned = cursor.clone();
    let select_cols_for_producer = select_columns.clone();
    let t_pipe = std::time::Instant::now();
    let progress_every = batch_size.saturating_mul(10).max(1);

    let producer = async {
        let mut stream = source.stream_rows_with_cursor(
            "LiteLLM_SpendLogs",
            &cursor_owned,
            limit,
            Some(&select_cols_for_producer),
        );
        let mut buf: Vec<native::UnifiedRow> = Vec::with_capacity(batch_size);
        while let Some(row_res) = stream.next().await {
            let row = row_res?;
            buf.push(row);
            if buf.len() >= batch_size {
                let batch = std::mem::replace(&mut buf, Vec::with_capacity(batch_size));
                if tx.send(batch).await.is_err() {
                    anyhow::bail!("spend_logs consumer closed the channel unexpectedly");
                }
            }
        }
        if !buf.is_empty() && tx.send(buf).await.is_err() {
            anyhow::bail!("spend_logs consumer closed the channel unexpectedly");
        }
        drop(tx);
        Ok::<(), anyhow::Error>(())
    };

    let consumer = async {
        let mut inserted_total: usize = 0;
        let mut ignored_total: usize = 0;
        let mut last_start_time: Option<String> = None;
        let mut since_last_log: usize = 0;

        while let Some(batch) = rx.recv().await {
            if let Some(last) = batch.last() {
                if let Some((_, v)) = last.iter().find(|(c, _)| c == "startTime") {
                    if let Some(s) = v.as_str() {
                        last_start_time = Some(s.to_string());
                    }
                }
            }

            let (ins, ign) = native::insert_rows_batch(
                target,
                "spend_logs",
                &filtered_cols,
                &batch,
                &overrides,
            )
            .await?;
            inserted_total += ins;
            ignored_total += ign;
            since_last_log += batch.len();

            if since_last_log >= progress_every {
                let elapsed = t_pipe.elapsed().as_secs_f64().max(0.001);
                let rate = inserted_total as f64 / elapsed;
                eprintln!(
                    "  [PROGRESS] spend_logs: inserted={} ignored={} ({:.0} rows/s, cursor={})",
                    inserted_total,
                    ignored_total,
                    rate,
                    last_start_time.as_deref().unwrap_or("<none>"),
                );
                since_last_log = 0;
            }
        }
        Ok::<(usize, usize, Option<String>), anyhow::Error>((
            inserted_total,
            ignored_total,
            last_start_time,
        ))
    };

    let (prod_res, cons_res) = tokio::join!(producer, consumer);
    prod_res?;
    let (inserted_total, ignored_total, last_start_time) = cons_res?;

    let elapsed = t_pipe.elapsed();
    let rate = inserted_total as f64 / elapsed.as_secs_f64().max(0.001);
    eprintln!(
        "  [TIMING] spend_logs pipeline: {:?} ({} inserted, {} ignored, {:.0} rows/s)",
        elapsed, inserted_total, ignored_total, rate
    );
    if let Some(ts) = last_start_time {
        eprintln!(
            "  [PROGRESS] spend_logs: {} rows migrated. resume: {}",
            inserted_total, ts
        );
    }

    Ok(inserted_total)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Main entry point
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn run_filtered(
    source_url: &str,
    target_url: &str,
    source_master_key: Option<&str>,
    target_master_key: &str,
    spend_log_limit: Option<usize>,
    spend_log_cursor: &CursorRange,
    step_filter: Option<u8>,
    skip_body: bool,
    skip_columns_set: &HashSet<(String, String)>,
    batch_size: usize,
) -> anyhow::Result<bool> {
    let total_start = std::time::Instant::now();

    let t0 = std::time::Instant::now();
    let source = SourcePool::connect(source_url).await?;
    let target = SourcePool::connect(target_url).await?;
    eprintln!("  [TIMING] connect: {:?}", t0.elapsed());

    // Step 1: Extract source master_key
    let t0 = std::time::Instant::now();
    let source_key = match source_master_key {
        Some(k) => k.to_string(),
        None => match extract_source_master_key(&source).await? {
            Some(k) => {
                eprintln!("  Extracted master_key from LiteLLM_Config");
                k
            }
            None => {
                anyhow::bail!(
                    "No source master_key found. Provide --source-master-key or \
                     ensure LiteLLM_Config has param_name='general_settings' with master_key field"
                );
            }
        },
    };
    eprintln!("Step 1: Source master_key obtained ({:?})", t0.elapsed());

    let run_step = |s: u8| step_filter.map_or(true, |f| f == s);

    // Step 2: Migrate plain tables
    if run_step(2) {
        eprintln!("Step 2: Migrating plain tables...");
        let t0 = std::time::Instant::now();
        for &(src, tgt) in PLAIN_TABLES {
            let t_tbl = std::time::Instant::now();
            let count = migrate_plain_table(&source, &target, src, tgt).await?;
            eprintln!("  {} -> {} ({} rows, {:?})", src, tgt, count, t_tbl.elapsed());
        }
        eprintln!("Step 2: plain tables done ({:?})", t0.elapsed());
    } else {
        eprintln!("Step 2: [SKIP]");
    }

    // Step 3: Migrate credentials
    if run_step(3) {
        eprintln!("Step 3: Migrating credentials (with key rotation)...");
        let t0 = std::time::Instant::now();
        let cred_count = migrate_credentials(&source, &target, &source_key, target_master_key).await?;
        eprintln!("  LiteLLM_CredentialsTable -> credentials ({} rows, {:?})", cred_count, t0.elapsed());
    } else {
        eprintln!("Step 3: [SKIP]");
    }

    // Step 4: Migrate proxy_models
    if run_step(4) {
        eprintln!("Step 4: Migrating proxy_models (with key rotation)...");
        let t0 = std::time::Instant::now();
        let model_count = migrate_proxy_models(&source, &target, &source_key, target_master_key).await?;
        eprintln!("  LiteLLM_ProxyModelTable -> proxy_models ({} rows, {:?})", model_count, t0.elapsed());
    } else {
        eprintln!("Step 4: [SKIP]");
    }

    // Step 5: Migrate spend_logs
    if run_step(5) {
        eprintln!("Step 5: Migrating spend_logs...");
        let t0 = std::time::Instant::now();
        let spend_count = migrate_spend_logs(
            &source,
            &target,
            spend_log_limit,
            spend_log_cursor,
            skip_body,
            skip_columns_set,
            batch_size,
        )
        .await?;
        eprintln!("  LiteLLM_SpendLogs -> spend_logs ({} rows, {:?})", spend_count, t0.elapsed());
    } else {
        eprintln!("Step 5: [SKIP]");
    }

    // Step 6: Verify
    eprintln!("Step 6: Verifying row counts...");
    let t0 = std::time::Instant::now();
    let mut all_match = true;
    let all_tables: &[(&str, &str)] = &[
        ("LiteLLM_OrganizationTable", "organizations"),
        ("LiteLLM_TeamTable", "teams"),
        ("LiteLLM_UserTable", "users"),
        ("LiteLLM_ProjectTable", "projects"),
        ("LiteLLM_BudgetTable", "budgets"),
        ("LiteLLM_OrganizationMembership", "organization_memberships"),
        ("LiteLLM_TeamMembership", "team_memberships"),
        ("LiteLLM_VerificationToken", "virtual_keys"),
        ("LiteLLM_Config", "config"),
        ("LiteLLM_CredentialsTable", "credentials"),
        ("LiteLLM_ProxyModelTable", "proxy_models"),
        ("LiteLLM_SpendLogs", "spend_logs"),
    ];

    for &(src, tgt) in all_tables {
        let src_count = source.count_rows(src).await.unwrap_or(0);
        let tgt_count = target.count_rows(tgt).await.unwrap_or(-1);

        let status = if src_count == tgt_count { "OK" } else { "MISMATCH" };
        if src_count != tgt_count {
            all_match = false;
        }
        eprintln!("  {} -> {}: src={} tgt={} [{}]", src, tgt, src_count, tgt_count, status);
    }
    eprintln!("Step 6: verify done ({:?})", t0.elapsed());

    eprintln!("[TIMING] total migration: {:?}", total_start.elapsed());
    Ok(all_match)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn create_pool(path: &str) -> sqlx::SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path.to_string())
                    .create_if_missing(true),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn test_remote_import_plain_tables() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        let src_str_sqlite = format!("sqlite://{}", src_str);
        let tgt_str_sqlite = format!("sqlite://{}", tgt_str);

        // Setup source DB
        let src_pool = create_pool(src_str).await;
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_OrganizationTable" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&src_pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_OrganizationTable\" (organization_id, organization_alias, spend) VALUES ('org-1', 'test', 42.0)"
        ).execute(&src_pool).await.unwrap();

        sqlx::query(
            r#"CREATE TABLE "LiteLLM_Config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#,
        ).execute(&src_pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-test-source-key-12345')"
        ).execute(&src_pool).await.unwrap();

        // Credentials
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_CredentialsTable" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT NOT NULL,
                credential_values TEXT, credential_info TEXT
            )"#,
        ).execute(&src_pool).await.unwrap();
        let source_key = "sk-test-source-key-12345";
        let plain_cred = r#"{"api_key":"sk-secret-123","api_base":"https://api.openai.com"}"#;
        let encrypted_cred = aigw_core::encrypt_litellm_value(plain_cred, source_key).unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_CredentialsTable\" (credential_id, credential_name, credential_values) VALUES ('cred-1', 'openai-key', ?)"
        ).bind(&encrypted_cred).execute(&src_pool).await.unwrap();

        // Proxy models
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_ProxyModelTable" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        ).execute(&src_pool).await.unwrap();
        let plain_params = r#"{"model":"gpt-4","api_key":"sk-model-key-456"}"#;
        let encrypted_params = aigw_core::encrypt_litellm_value(plain_params, source_key).unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_ProxyModelTable\" (model_id, model_name, litellm_params) VALUES ('model-1', 'gpt-4', ?)"
        ).bind(&encrypted_params).execute(&src_pool).await.unwrap();

        // Spend logs
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0)"#,
        ).execute(&src_pool).await.unwrap();
        src_pool.close().await;

        // Setup target DB
        let tgt_pool = create_pool(tgt_str).await;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "organizations" (
                organization_id TEXT PRIMARY KEY, organization_alias TEXT, spend REAL DEFAULT 0
            )"#,
        ).execute(&tgt_pool).await.unwrap();

        for table in &["teams", "users", "projects", "budgets", "organization_memberships",
            "team_memberships", "virtual_keys", "spend_logs"] {
            sqlx::query(&format!("CREATE TABLE IF NOT EXISTS \"{}\" (id TEXT PRIMARY KEY)", table))
                .execute(&tgt_pool).await.unwrap();
        }

        sqlx::query(r#"CREATE TABLE IF NOT EXISTS "config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#)
            .execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "credentials" (
                credential_id TEXT PRIMARY KEY, credential_name TEXT NOT NULL,
                credential_values TEXT, credential_info TEXT
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "proxy_models" (
                model_id TEXT PRIMARY KEY, model_name TEXT, litellm_params TEXT, model_info TEXT
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        tgt_pool.close().await;

        // Run
        let target_key = "sk-aigw-target-key-99999";
        let cursor = CursorRange::default();
        let result = run_filtered(
            &src_str_sqlite, &tgt_str_sqlite,
            None, target_key, None, &cursor, None, false,
            &HashSet::new(),
            1000,
        ).await;
        assert!(result.is_ok(), "remote_import failed: {:?}", result.err());

        // Verify re-encryption
        let tgt_pool = create_pool(tgt_str).await;
        let cred_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM credentials")
            .fetch_one(&tgt_pool).await.unwrap();
        assert_eq!(cred_count.0, 1);

        let cred_row: (String,) = sqlx::query_as(
            "SELECT credential_values FROM credentials WHERE credential_id = 'cred-1'",
        ).fetch_one(&tgt_pool).await.unwrap();
        // Storage format: opaque encrypted blobs are wrapped as JSON scalar
        // strings so runtime `serde_json::Value` decode round-trips as
        // `Value::String(_)` (see native.rs Postgres jsonb / SQLite json
        // literal handling).  Unwrap here to mirror what the server does at
        // read time via `.as_str()`.
        let raw = serde_json::from_str::<String>(&cred_row.0).unwrap_or(cred_row.0);
        let decrypted = aigw_core::decrypt_litellm_value(&raw, target_key).unwrap();
        assert_eq!(decrypted, plain_cred);

        let model_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM proxy_models")
            .fetch_one(&tgt_pool).await.unwrap();
        assert_eq!(model_count.0, 1);

        let model_row: (String,) = sqlx::query_as(
            "SELECT litellm_params FROM proxy_models WHERE model_id = 'model-1'",
        ).fetch_one(&tgt_pool).await.unwrap();
        let raw_params = serde_json::from_str::<String>(&model_row.0).unwrap_or(model_row.0);
        let decrypted_params = aigw_core::decrypt_litellm_value(&raw_params, target_key).unwrap();
        assert_eq!(decrypted_params, plain_params);

        tgt_pool.close().await;
    }

    #[tokio::test]
    async fn test_extract_master_key_from_config() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        let pool = create_pool(db_str).await;
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_Config" (param_name TEXT PRIMARY KEY, param_value TEXT)"#,
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO \"LiteLLM_Config\" (param_name, param_value) VALUES ('litellm_master_key', 'sk-extracted-key')"
        ).execute(&pool).await.unwrap();
        pool.close().await;

        let source = SourcePool::connect(db_str).await.unwrap();
        let key = extract_source_master_key(&source).await.unwrap();
        assert_eq!(key, Some("sk-extracted-key".to_string()));
    }

    #[tokio::test]
    async fn test_migrate_plain_table_empty() {
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = format!("sqlite://{}", src_path.to_str().unwrap());
        let tgt_str = format!("sqlite://{}", tgt_path.to_str().unwrap());

        let src_pool = create_pool(src_path.to_str().unwrap()).await;
        sqlx::query(r#"CREATE TABLE "LiteLLM_OrganizationTable" (organization_id TEXT)"#)
            .execute(&src_pool).await.unwrap();
        src_pool.close().await;

        let tgt_pool = create_pool(tgt_path.to_str().unwrap()).await;
        sqlx::query(r#"CREATE TABLE "organizations" (organization_id TEXT)"#)
            .execute(&tgt_pool).await.unwrap();
        tgt_pool.close().await;

        let source = SourcePool::connect(&src_str).await.unwrap();
        let target = SourcePool::connect(&tgt_str).await.unwrap();
        let count = migrate_plain_table(&source, &target, "LiteLLM_OrganizationTable", "organizations").await.unwrap();
        assert_eq!(count, 0, "empty table should migrate 0 rows");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Breakpoint resume tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Create an in-memory SQLite DB with `LiteLLM_SpendLogs` table and `n` rows.
    /// Each row has sequential startTime values for cursor testing.
    struct SpendLogsDb {
        db_path: String,
        _dir: tempfile::TempDir,
    }

    async fn setup_spend_logs_db(n: usize) -> SpendLogsDb {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_path_str = db_path.to_str().unwrap().to_string();

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&db_path_str)
                    .create_if_missing(true),
            )
            .await
            .unwrap();

        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (
                request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0,
                startTime TEXT NOT NULL, call_type TEXT DEFAULT '', api_key TEXT DEFAULT '',
                total_tokens INTEGER DEFAULT 0, prompt_tokens INTEGER DEFAULT 0,
                completion_tokens INTEGER DEFAULT 0, endTime TEXT DEFAULT ''
            )"#,
        ).execute(&pool).await.unwrap();

        // Insert rows with sequential timestamps (1 second apart)
        for i in 0..n {
            let rid = format!("req-{:04}", i);
            let hour = 10 + (i / 3600) % 24;
            let minute = (i / 60) % 60;
            let second = i % 60;
            let ts = format!(
                "2026-07-20 {:02}:{:02}:{:02}",
                hour, minute, second
            );
            sqlx::query(
                "INSERT INTO \"LiteLLM_SpendLogs\" (request_id, model, startTime) VALUES (?, 'gpt-4', ?)",
            )
            .bind(&rid)
            .bind(&ts)
            .execute(&pool)
            .await
            .unwrap();
        }

        pool.close().await;

        SpendLogsDb {
            db_path: db_path_str,
            _dir: dir,
        }
    }

    #[tokio::test]
    async fn test_read_rows_with_cursor_full_scan() {
        sqlx::any::install_default_drivers();
        let pool = setup_spend_logs_db(3).await;

        let source = SourcePool::connect(&format!("sqlite://{}", pool.db_path)).await.unwrap();
        let cursor = CursorRange::default();
        let rows = source
            .read_rows_with_cursor("LiteLLM_SpendLogs", &cursor, None)
            .await
            .unwrap();

        // Should return all rows, ordered by startTime ASC
        assert_eq!(rows.len(), 3);
        let times: Vec<String> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .find(|(col, _)| col == "startTime")
                    .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
                    .unwrap()
            })
            .collect();
        assert!(times[0] <= times[1] && times[1] <= times[2],
            "rows should be ordered by startTime ASC");
    }

    #[tokio::test]
    async fn test_read_rows_with_cursor_resume_after() {
        sqlx::any::install_default_drivers();
        let pool = setup_spend_logs_db(5).await;

        let source = SourcePool::connect(&format!("sqlite://{}", pool.db_path)).await.unwrap();

        // Read all rows first to find the middle timestamp
        let all = source
            .read_rows_with_cursor("LiteLLM_SpendLogs", &CursorRange::default(), None)
            .await
            .unwrap();
        assert!(all.len() >= 3);

        // Get the 3rd row's startTime as resume_after
        let mid_time = all[2]
            .iter()
            .find(|(col, _)| col == "startTime")
            .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
            .unwrap();

        let cursor = CursorRange {
            resume_after: Some(mid_time.clone()),
            end_before: None,
        };
        let resumed = source
            .read_rows_with_cursor("LiteLLM_SpendLogs", &cursor, None)
            .await
            .unwrap();

        // Should return rows from mid_time onwards (inclusive)
        assert!(!resumed.is_empty(), "should have rows >= mid_time");
        let first_time = resumed[0]
            .iter()
            .find(|(col, _)| col == "startTime")
            .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
            .unwrap();
        assert!(first_time >= mid_time);
    }

    #[tokio::test]
    async fn test_read_rows_with_cursor_time_window() {
        sqlx::any::install_default_drivers();
        let pool = setup_spend_logs_db(6).await;

        let source = SourcePool::connect(&format!("sqlite://{}", pool.db_path)).await.unwrap();

        // Read all rows to find middle timestamps
        let all = source
            .read_rows_with_cursor("LiteLLM_SpendLogs", &CursorRange::default(), None)
            .await
            .unwrap();
        assert!(all.len() >= 4);

        let start_time = all[1]
            .iter()
            .find(|(col, _)| col == "startTime")
            .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
            .unwrap();
        let end_time = all[all.len() - 2]
            .iter()
            .find(|(col, _)| col == "startTime")
            .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
            .unwrap();

        let cursor = CursorRange {
            resume_after: Some(start_time.clone()),
            end_before: Some(end_time.clone()),
        };
        let window = source
            .read_rows_with_cursor("LiteLLM_SpendLogs", &cursor, None)
            .await
            .unwrap();

        assert!(!window.is_empty());
        for row in &window {
            let ts = row
                .iter()
                .find(|(col, _)| col == "startTime")
                .and_then(|(_, v)| v.as_str().map(|s| s.to_string()))
                .unwrap();
            assert!(ts >= start_time, "row time {} < start boundary {}", ts, start_time);
            assert!(ts < end_time, "row time {} >= end boundary {}", ts, end_time);
        }
    }

    #[tokio::test]
    async fn test_read_rows_with_cursor_and_limit() {
        sqlx::any::install_default_drivers();
        let pool = setup_spend_logs_db(10).await;

        let source = SourcePool::connect(&format!("sqlite://{}", pool.db_path)).await.unwrap();

        let cursor = CursorRange::default();
        let rows = source
            .read_rows_with_cursor("LiteLLM_SpendLogs", &cursor, Some(4))
            .await
            .unwrap();

        assert_eq!(rows.len(), 4, "LIMIT should cap result count");
    }

    #[tokio::test]
    async fn test_migrate_spend_logs_resume_idempotent() {
        // Full migration then "resume" from same startTime — target count must not double.
        sqlx::any::install_default_drivers();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.db");
        let tgt_path = dir.path().join("tgt.db");
        let src_str = src_path.to_str().unwrap();
        let tgt_str = tgt_path.to_str().unwrap();

        let src_pool = create_pool(src_str).await;
        sqlx::query(
            r#"CREATE TABLE "LiteLLM_SpendLogs" (
                request_id TEXT PRIMARY KEY, model TEXT, spend REAL DEFAULT 0,
                startTime TEXT NOT NULL, call_type TEXT DEFAULT '', api_key TEXT DEFAULT '',
                total_tokens INTEGER DEFAULT 0, prompt_tokens INTEGER DEFAULT 0,
                completion_tokens INTEGER DEFAULT 0, endTime TEXT DEFAULT ''
            )"#,
        ).execute(&src_pool).await.unwrap();
        // Insert 5 rows with distinct timestamps
        for i in 0..5 {
            let rid = format!("rid-{:02}", i);
            let ts = format!("2026-07-{:02}T10:00:00Z", 15 + i); // Jul 15-19
            sqlx::query(
                "INSERT INTO \"LiteLLM_SpendLogs\" (request_id, model, startTime) VALUES (?, 'gpt-4', ?)",
            )
            .bind(&rid)
            .bind(&ts)
            .execute(&src_pool)
            .await
            .unwrap();
        }
        src_pool.close().await;

        let tgt_pool = create_pool(tgt_str).await;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS "spend_logs" (
                request_id TEXT PRIMARY KEY, call_type TEXT NOT NULL DEFAULT '',
                api_key TEXT NOT NULL DEFAULT '', spend REAL DEFAULT 0,
                total_tokens INTEGER DEFAULT 0, prompt_tokens INTEGER DEFAULT 0,
                completion_tokens INTEGER DEFAULT 0, start_time TEXT NOT NULL,
                end_time TEXT DEFAULT '', model TEXT NOT NULL DEFAULT '',
                model_id TEXT, model_group TEXT,
                custom_llm_provider TEXT, api_base TEXT, "user" TEXT,
                metadata BLOB,
                cache_hit TEXT, cache_key TEXT,
                request_tags BLOB,
                team_id TEXT, organization_id TEXT,
                end_user TEXT, requester_ip_address TEXT,
                messages BLOB, response BLOB,
                session_id TEXT, status TEXT,
                mcp_namespaced_tool_name TEXT, agent_id TEXT,
                proxy_server_request BLOB
            )"#,
        ).execute(&tgt_pool).await.unwrap();
        tgt_pool.close().await;

        let source = SourcePool::connect(src_str).await.unwrap();
        let target = SourcePool::connect(tgt_str).await.unwrap();

        let skip_set = HashSet::new();

        // First: migrate all
        let count1 = migrate_spend_logs(&source, &target, None, &CursorRange::default(), false, &skip_set, 1000)
            .await
            .unwrap();
        assert_eq!(count1, 5, "first migration should insert 5 rows");

        // Second: "resume" with the same cursor (simulating restart from earliest)
        let count2 = migrate_spend_logs(&source, &target, None, &CursorRange::default(), false, &skip_set, 1000)
            .await
            .unwrap();
        assert_eq!(count2, 0, "second migration should insert 0 rows (all conflict)");

        // Verify target still has 5 rows
        let tgt_pool2 = create_pool(tgt_str).await;
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM spend_logs")
            .fetch_one(&tgt_pool2)
            .await
            .unwrap();
        assert_eq!(row.0, 5, "target should still have 5 rows after idempotent resume");
    }
}
