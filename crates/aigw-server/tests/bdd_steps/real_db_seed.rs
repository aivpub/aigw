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
    pub call_id: String,
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
        call_id: &str,
        api_key: &str,
        spend: f64,
        total_tokens: i64,
        model: &str,
        ts_iso8601: &str,
    ) -> Self {
        Self {
            call_id: call_id.to_string(),
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

/// Delete all rows whose call_id starts with the given prefix.
/// Idempotent — safe to call before each seeding.
pub(crate) async fn cleanup_by_prefix(db_url: &str, prefix: &str) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let sql = format!("DELETE FROM spend_logs WHERE call_id LIKE '{}%'", prefix);
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Clean up virtual keys with the given key_alias prefix.
pub(crate) async fn cleanup_keys_by_alias(db_url: &str, alias_prefix: &str) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let sql = format!(
        "DELETE FROM virtual_keys WHERE key_alias LIKE '{}%'",
        alias_prefix
    );
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Insert spend_log rows directly into the test database using SourcePool.
///
/// Handles cross-DB differences: time literals use `time_literal()`,
/// request_tags serialized as proper JSON. NULL columns use NULL (not empty strings).
pub(crate) async fn seed_spend_logs(db_url: &str, rows: &[SeedRow]) -> anyhow::Result<()> {
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

        let user_ident = pool.quote_ident("user");

        let sql = format!(
            r#"INSERT INTO spend_logs
            (call_id, call_type, api_key, spend, total_tokens, prompt_tokens, completion_tokens,
             start_time, end_time, model, status, {user_ident}, team_id, organization_id,
             request_tags, custom_llm_provider, end_user)
            VALUES ('{}', 'completion', '{}', {}, {}, {}, {},
                    {}, {}, '{}', '{}',
                    {}, {}, {},
                    {}, {}, {})"#,
            row.call_id,
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
        token_hash, alias, alias, user_val, now, now,
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
    let looks_json =
        s.starts_with('{') || s.starts_with('[') || (s.starts_with('"') && s.ends_with('"'));
    let escaped = s.replace('\'', "''");
    if looks_json {
        format!("'{}'", escaped)
    } else {
        // Plain text — JSON-string encode as "\"value\""
        let json_str = serde_json::json!(s);
        format!("'{}'", json_str.to_string().replace('\'', "''"))
    }
}

// ── Multi-level budget enforcement seed helpers ──

/// Insert or update a user row in the test database.
/// Deletes any existing user with the same user_id first (idempotent).
pub(crate) async fn ensure_user(
    db_url: &str,
    user_id: &str,
    team_id: Option<&str>,
    max_budget: Option<f64>,
    spend: f64,
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let del_sql = format!("DELETE FROM users WHERE user_id = '{}'", user_id);
    let _ = pool.execute_raw(&del_sql).await;

    let team_val = match team_id {
        Some(tid) => format!("'{}'", tid.replace('\'', "''")),
        None => "NULL".to_string(),
    };
    let max_budget_val = match max_budget {
        Some(v) => format!("'{}'", v),
        None => "NULL".to_string(),
    };
    let now = pool.time_literal("2026-07-20T00:00:00");

    let sql = format!(
        r#"INSERT INTO users
        (user_id, user_alias, team_id, organization_id, object_permission_id,
         password, teams, user_role, max_budget, spend, user_email, models, metadata,
         max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at,
         allowed_cache_controls, policies, model_spend, model_max_budget,
         created_at, updated_at)
        VALUES ('{}', 'user-{}', {}, NULL, NULL,
                NULL, '[]', NULL, {}, {}, NULL, '[]', '{{}}',
                NULL, NULL, NULL, NULL, NULL,
                '[]', '[]', '{{}}', '{{}}',
                {}, {})"#,
        user_id, user_id, team_val, max_budget_val, spend, now, now,
    );
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Insert or update a team row in the test database.
/// Deletes any existing team with the same team_id first (idempotent).
pub(crate) async fn ensure_team(
    db_url: &str,
    team_id: &str,
    org_id: Option<&str>,
    max_budget: Option<f64>,
    soft_budget: Option<f64>,
    spend: f64,
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let del_sql = format!("DELETE FROM teams WHERE team_id = '{}'", team_id);
    let _ = pool.execute_raw(&del_sql).await;

    let org_val = match org_id {
        Some(oid) => format!("'{}'", oid.replace('\'', "''")),
        None => "NULL".to_string(),
    };
    let max_budget_val = match max_budget {
        Some(v) => format!("'{}'", v),
        None => "NULL".to_string(),
    };
    let soft_budget_val = match soft_budget {
        Some(v) => format!("'{}'", v),
        None => "NULL".to_string(),
    };
    let now = pool.time_literal("2026-07-20T00:00:00");

    let sql = format!(
        r#"INSERT INTO teams
        (team_id, team_alias, organization_id, object_permission_id, admins, members,
         members_with_roles, metadata, max_budget, soft_budget, spend, models,
         max_parallel_requests, tpm_limit, rpm_limit, budget_duration, budget_reset_at,
         blocked, created_at, updated_at, model_spend, model_max_budget,
         router_settings, team_member_permissions, access_group_ids, policies,
         default_team_member_models, budget_limits, model_id, allow_team_guardrail_config)
        VALUES ('{}', 'team-{}', {}, NULL, '[]', '[]', '{{}}', '{{}}', {}, {}, {}, '[]',
                NULL, NULL, NULL, NULL, NULL,
                0, {}, {}, '{{}}', '{{}}',
                NULL, '[]', '[]', '[]', '[]', NULL, NULL, 0)"#,
        team_id, team_id, org_val, max_budget_val, soft_budget_val, spend, now, now,
    );
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Insert or update an organization row in the test database.
/// Deletes any existing org with the same organization_id first (idempotent).
pub(crate) async fn ensure_organization(
    db_url: &str,
    org_id: &str,
    budget_id: &str,
    spend: f64,
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let del_sql = format!("DELETE FROM organizations WHERE organization_id = '{}'", org_id);
    let _ = pool.execute_raw(&del_sql).await;

    let now = pool.time_literal("2026-07-20T00:00:00");

    let sql = format!(
        r#"INSERT INTO organizations
        (organization_id, organization_alias, budget_id, metadata, models, spend,
         model_spend, object_permission_id, created_at, created_by, updated_at, updated_by)
        VALUES ('{}', 'org-{}', '{}', '{{}}', '[]', {},
                '{{}}', NULL, {}, 'aigw-test', {}, 'aigw-test')"#,
        org_id, org_id, budget_id, spend, now, now,
    );
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Insert or update a budget row in the test database.
/// Deletes any existing budget with the same budget_id first (idempotent).
pub(crate) async fn ensure_budget(
    db_url: &str,
    budget_id: &str,
    max_budget: f64,
    soft_budget: Option<f64>,
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let del_sql = format!("DELETE FROM budgets WHERE budget_id = '{}'", budget_id);
    let _ = pool.execute_raw(&del_sql).await;

    let soft_budget_val = match soft_budget {
        Some(v) => format!("'{}'", v),
        None => "NULL".to_string(),
    };
    let now = pool.time_literal("2026-07-20T00:00:00");

    let sql = format!(
        r#"INSERT INTO budgets
        (budget_id, max_budget, soft_budget, max_parallel_requests, tpm_limit, rpm_limit,
         model_max_budget, budget_duration, budget_reset_at, allowed_models,
         created_at, created_by, updated_at, updated_by)
        VALUES ('{}', '{}', {}, NULL, NULL, NULL,
                '{{}}', NULL, NULL, '[]',
                {}, 'aigw-test', {}, 'aigw-test')"#,
        budget_id, max_budget, soft_budget_val, now, now,
    );
    pool.execute_raw(&sql).await?;
    Ok(())
}

/// Generic cleanup by entity type + ID. Deletes the row matching the given
/// entity_id from the appropriate table. Idempotent.
pub(crate) async fn cleanup_entity(
    db_url: &str,
    entity_type: &str,
    entity_id: &str,
) -> anyhow::Result<()> {
    let pool = aigw_migrate::native::SourcePool::connect(db_url).await?;
    let (table, id_col) = match entity_type {
        "key" | "virtual_key" => ("virtual_keys", "token"),
        "user" => ("users", "user_id"),
        "team" => ("teams", "team_id"),
        "organization" => ("organizations", "organization_id"),
        "budget" => ("budgets", "budget_id"),
        _ => return Ok(()),
    };
    let sql = format!("DELETE FROM {} WHERE {} = '{}'", table, id_col, entity_id);
    pool.execute_raw(&sql).await?;
    Ok(())
}
