//! Real DB seed helpers — reusable SourcePool-based data seeding for
//! multi-database real BDD scenarios.
//!
//! These helpers use SourcePool (from aigw_migrate) to insert test data
//! directly into the test database, bypassing the HTTP API. This keeps
//! the data layer independent of the transport layer under test.
//!
//! All insertions are idempotent (DELETE-before-INSERT by request_id prefix).

use aigw_core::crypto;
use serde_json;

/// A row of test data to insert into spend_logs.
#[derive(Debug, Clone)]
pub(crate) struct SeedRow {
    pub request_id: String,
    pub api_key: String,
    pub spend: f64,
    pub total_tokens: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub model: String,
    pub status: String,
    /// ISO 8601 datetime string (e.g. "2026-07-20T10:30:00")
    pub ts_iso8601: String,
    pub user: Option<String>,
    pub team_id: Option<String>,
    pub organization_id: Option<String>,
    /// request_tags — stored as JSON (JSONB/TEXT/BLOB). When None, NULL.
    /// When Some, should be a valid JSON value.
    pub request_tags: Option<String>,
    pub custom_llm_provider: Option<String>,
    pub end_user: Option<String>,
}

impl SeedRow {
    /// Create a minimal row with defaults. request_tags defaults to NULL.
    pub(crate) fn new(
        request_id: &str,
        api_key: &str,
        spend: f64,
        total_tokens: i64,
        model: &str,
        ts_iso8601: &str,
    ) -> Self {
        Self {
            request_id: request_id.to_string(),
            api_key: api_key.to_string(),
            spend,
            total_tokens,
            prompt_tokens: total_tokens / 2,
            completion_tokens: total_tokens - total_tokens / 2,
            model: model.to_string(),
            status: "success".to_string(),
            ts_iso8601: ts_iso8601.to_string(),
            user: None,
            team_id: None,
            organization_id: None,
            request_tags: None,
            custom_llm_provider: None,
            end_user: None,
        }
    }
}

/// Delete all rows whose request_id starts with the given prefix.
/// Idempotent — safe to call before each seeding.
pub(crate) async fn cleanup_by_prefix(db_url: &str, prefix: &str) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let sql = format!("DELETE FROM spend_logs WHERE request_id LIKE '{}%'", prefix);
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Clean up virtual keys with the given key_alias prefix.
pub(crate) async fn cleanup_keys_by_alias(db_url: &str, alias_prefix: &str) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let sql = format!("DELETE FROM virtual_keys WHERE key_alias LIKE '{}%'", alias_prefix);
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Insert spend_log rows directly into the test database using SourcePool.
///
/// Handles cross-DB differences: time literals use `time_literal()`,
/// request_tags serialized as proper JSON. NULL columns use NULL (not empty strings).
pub(crate) async fn seed_spend_logs(
    db_url: &str,
    rows: &[SeedRow],
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;

    for row in rows {
        let time_lit = pool.time_literal(&row.ts_iso8601);

        let user_val = opt_str_literal(&row.user);
        let team_id_val = opt_str_literal(&row.team_id);
        let org_id_val = opt_str_literal(&row.organization_id);
        let provider_val = opt_str_literal(&row.custom_llm_provider);
        let end_user_val = opt_str_literal(&row.end_user);

        // request_tags: must be valid JSON for sqlx deserialization.
        // When None → NULL. When Some(valid JSON string) → quoted.
        // Plain text values are JSON-string-encoded.
        let tags_val = encode_tags_literal(&row.request_tags);

        let sql = format!(
            r#"INSERT INTO spend_logs
            (request_id, call_type, api_key, spend, total_tokens, prompt_tokens, completion_tokens,
             start_time, end_time, model, status, "user", team_id, organization_id,
             request_tags, custom_llm_provider, end_user)
            VALUES ('{}', 'completion', '{}', {}, {}, {}, {},
                    {}, {}, '{}', '{}',
                    {}, {}, {},
                    {}, {}, {})"#,
            row.request_id,
            row.api_key,
            row.spend,
            row.total_tokens,
            row.prompt_tokens,
            row.completion_tokens,
            time_lit,
            time_lit,
            row.model,
            row.status,
            user_val,
            team_id_val,
            org_id_val,
            tags_val,
            provider_val,
            end_user_val,
        );
        pool.execute_raw(&sql).await?;
    }
    Ok(())
}

/// Insert (or ensure) a virtual_key row in the test database.
/// Deletes any existing key with the same alias first (idempotent).
pub(crate) async fn ensure_virtual_key(
    db_url: &str,
    raw_token: &str,
    alias: &str,
    user_id: Option<&str>,
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let token_hash = crypto::hash_token(raw_token);

    // Clean up old key first (idempotent)
    let del_sql = format!("DELETE FROM virtual_keys WHERE key_alias = '{}'", alias);
    let _ = pool.execute_raw(&del_sql).await;

    let user_val = match user_id {
        Some(uid) => format!("'{}'", uid.replace('\'', "''")),
        None => "NULL".to_string(),
    };
    let now = pool.time_literal("2026-07-20T00:00:00");

    let sql = format!(
        r#"INSERT INTO virtual_keys
        (token, key_alias, key_name, spend,
         models, aliases, config, permissions, metadata,
         allowed_cache_controls, allowed_routes, policies, access_group_ids,
         model_spend, model_max_budget,
         user_id, team_id, max_budget, budget_duration, budget_reset_at,
         soft_budget_cooldown, created_at, updated_at)
        VALUES ('{}', '{}', '{}', 0.0,
                '[]', '{{}}', '{{}}', '{{}}', '{{}}',
                '[]', '[]', '[]', '[]',
                '{{}}', '{{}}',
                {}, NULL, NULL, NULL, NULL,
                'false', {}, {})"#,
        token_hash, alias, alias,
        user_val,
        now, now,
    );

    pool.execute_raw(&sql).await?;
    Ok(())
}

// ── helpers ──

fn opt_str_literal(v: &Option<String>) -> String {
    match v {
        Some(s) if !s.is_empty() => format!("'{}'", s.replace('\'', "''")),
        _ => "NULL".to_string(),
    }
}

/// Encode request_tags as a SQL literal that sqlx can deserialize as Option<serde_json::Value>.
/// - None → NULL
/// - Some(empty) → NULL
/// - Some(valid JSON) → quoted JSON
/// - Some(plain text) → JSON-string-quoted (e.g., "\"important\"")
fn encode_tags_literal(tags: &Option<String>) -> String {
    let s = match tags {
        Some(s) if !s.is_empty() => s.as_str(),
        _ => return "NULL".to_string(),
    };
    // Already looks like JSON object/array/string → use as-is
    let looks_json = s.starts_with('{') || s.starts_with('[')
        || (s.starts_with('"') && s.ends_with('"'));
    let escaped = s.replace('\'', "''");
    if looks_json {
        format!("'{}'", escaped)
    } else {
        // Plain text — JSON-string encode as "\"value\""
        let json_str = serde_json::json!(s);
        format!("'{}'", json_str.to_string().replace('\'', "''"))
    }
}
