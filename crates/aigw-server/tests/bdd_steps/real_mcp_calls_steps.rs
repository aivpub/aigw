//! Step bindings for real/mcp_calls_real.feature — Stage 147 real BDD.
//!
//! Drives the running aigw server over HTTP so the PostgreSQL / MySQL branches of
//! [`aigw_core::db`] actually execute: `034` adds an index in three dialects and
//! `McpCallFilter.user_id` is bound twice on MySQL (bare `?`) but once on
//! SQLite / PG (`?n` / `$n`). A mis-numbered bind is silent there — the types
//! all coerce — so the only honest check is a row written by one identity and
//! read back by two.
//!
//! The index assertions read the catalog rather than an `EXPLAIN` plan: the test
//! database holds a handful of rows, so the planner picks a sequential scan and
//! an assertion on the plan would be asserting the optimizer's mood, not the
//! schema. The MySQL `FORCE INDEX` probe is the one place a plan is inspected,
//! and it is forced precisely so it cannot pass or fail on cost estimates.

use cucumber::{given, then, when};
use serde_json::{json, Value};

use super::real_api_steps::{
    base_url, client, create_key_with_user_id, real_api_enabled, set_skip_pass,
};
use crate::TestWorld;

const SERVER_ALIAS: &str = "real-calls-scope";

/// Alias of the server the shared "创建指向 real 透传 stub 的 MCP server" step
/// registered — it stores it under this key, which keeps the two features from
/// having to re-derive one another's fixture names.
const SERVER_ALIAS_KEY: &str = "mcp-pass-real:alias";

fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD harness")
}

fn alias_of(world: &TestWorld) -> String {
    world
        .created_keys
        .get(SERVER_ALIAS_KEY)
        .cloned()
        .unwrap_or_else(|| SERVER_ALIAS.to_string())
}

fn key_of(world: &TestWorld, alias: &str) -> String {
    world
        .created_keys
        .get(alias)
        .cloned()
        .unwrap_or_else(|| panic!("no key created under {alias}"))
}

async fn read_rows(sql: &str) -> Vec<Vec<(String, Value)>> {
    let pool = aigw_migrate::native::SourcePool::connect(&test_db_url())
        .await
        .expect("connect to test db");
    pool.read_rows_sql(sql).await.expect("read rows")
}

async fn total_count_of(world: &mut TestWorld, path: &str, token: &str) {
    let resp = client()
        .get(format!("{}{}", base_url(), path))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .expect("GET call log");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "真库调用日志场景以用户 {string} 的 key {string} 为准")]
async fn given_key_for_user(world: &mut TestWorld, user_id: String, alias: String) {
    if !real_api_enabled() {
        return;
    }
    create_key_with_user_id(world, &alias, &user_id).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "用真实 key {string} 对 {string} 发送 tools\\/list")]
async fn when_tools_list(world: &mut TestWorld, key_alias: String, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, json!({"tools": [], "servers": []}));
        return;
    }
    let token = key_of(world, &key_alias);
    let resp = client()
        .post(format!("{}/mcp-rest/tools/list", base_url()))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&json!({"alias": alias}))
        .send()
        .await
        .expect("POST /mcp-rest/tools/list");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "用 master key 查询 \\/v1\\/mcp\\/calls")]
async fn when_master_key_hits_user_endpoint(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(
            world,
            400,
            json!({"error": {"message": "no user", "type": "key_without_user"}}),
        );
        return;
    }
    let mk = world.master_key.clone();
    total_count_of(world, "/v1/mcp/calls", &mk).await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "用真实 key {string} 查询 \\/v1\\/mcp\\/calls 该 alias 的 total_count 为 {int}")]
async fn then_user_scoped_count(world: &mut TestWorld, key_alias: String, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let token = key_of(world, &key_alias);
    let alias = alias_of(world);
    total_count_of(
        world,
        &format!("/v1/mcp/calls?server_alias={alias}&page_size=50"),
        &token,
    )
    .await;
    assert_eq!(
        world.last_status,
        Some(200),
        "user-scoped call log failed: {:?}",
        world.last_body
    );
    let body = world.last_body.as_ref().expect("body");
    assert_eq!(
        body["total_count"].as_i64(),
        Some(expected),
        "total_count for {alias} as {key_alias}: {body}"
    );
}

#[then(
    expr = "用真实 key {string} 带别人的 user_id 查询 \\/v1\\/mcp\\/calls 该 alias 的 total_count 仍为 {int}"
)]
async fn then_user_id_is_ignored(world: &mut TestWorld, key_alias: String, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let token = key_of(world, &key_alias);
    let alias = alias_of(world);
    total_count_of(
        world,
        &format!("/v1/mcp/calls?server_alias={alias}&user_id=real-calls-b&page_size=50"),
        &token,
    )
    .await;
    let body = world.last_body.as_ref().expect("body");
    assert_eq!(
        body["total_count"].as_i64(),
        Some(expected),
        "a foreign user_id must not widen the result: {body}"
    );
}

#[then(
    expr = "用 master key 查询 \\/admin\\/mcp\\/calls 按 user_id {string} 得到的 total_count 为 {int}"
)]
async fn then_admin_user_filter(world: &mut TestWorld, user_id: String, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let alias = alias_of(world);
    total_count_of(
        world,
        &format!("/admin/mcp/calls?server_alias={alias}&user_id={user_id}&page_size=50"),
        &mk,
    )
    .await;
    let body = world.last_body.as_ref().expect("body");
    assert_eq!(
        body["total_count"].as_i64(),
        Some(expected),
        "admin filter by user_id={user_id}: {body}"
    );
    for row in body["data"].as_array().into_iter().flatten() {
        assert_eq!(
            row["user_id"].as_str(),
            Some(user_id.as_str()),
            "admin filter returned another user's row: {row}"
        );
    }
}

#[then(expr = "用 master key 查询 \\/admin\\/mcp\\/calls 按该 alias 得到的 total_count 为 {int}")]
async fn then_admin_alias_count(world: &mut TestWorld, expected: i64) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let alias = alias_of(world);
    total_count_of(
        world,
        &format!("/admin/mcp/calls?server_alias={alias}&page_size=50"),
        &mk,
    )
    .await;
    let body = world.last_body.as_ref().expect("body");
    assert_eq!(
        body["total_count"].as_i64(),
        Some(expected),
        "admin unfiltered-by-user count for {alias}: {body}"
    );
}

#[then(expr = "真库 mcp_call_log 上存在以 user_id 为前导列、created_at 为次列的索引 {string}")]
async fn then_index_exists(_world: &mut TestWorld, index_name: String) {
    if !real_api_enabled() {
        return;
    }
    let index = index_name;
    let url = test_db_url();
    let kind = aigw_migrate::native::DbKind::from_url(&url);

    // Column order is the whole point: `user_id` must lead so `created_at` still
    // range-scans instead of forcing a sort. Reading the definition (rather than
    // only confirming the name exists) is what makes an accidental
    // `(created_at, user_id)` a failure instead of a green run.
    let definition = match kind {
        // `pg_indexes.indexdef` is the whole `CREATE INDEX ...` statement, so the
        // key parts come out in declared order without joining pg_attribute.
        aigw_migrate::native::DbKind::Postgres => {
            let rows = read_rows(&format!(
                "SELECT indexdef FROM pg_indexes WHERE tablename = 'mcp_call_log' AND indexname = '{index}'"
            ))
            .await;
            rows.first()
                .and_then(|row| row.first())
                .map(|(_, v)| v.to_string())
                .unwrap_or_default()
        }
        aigw_migrate::native::DbKind::Mysql => {
            let rows = read_rows(&format!(
                "SELECT column_name FROM information_schema.statistics \
                 WHERE table_schema = DATABASE() AND table_name = 'mcp_call_log' \
                 AND index_name = '{index}' ORDER BY seq_in_index"
            ))
            .await;
            rows.iter()
                .filter_map(|row| row.first())
                .map(|(_, v)| v.to_string().trim_matches('"').to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
        aigw_migrate::native::DbKind::Sqlite => {
            let rows = read_rows(&format!(
                "SELECT name FROM pragma_index_info('{index}') ORDER BY seqno"
            ))
            .await;
            rows.iter()
                .filter_map(|row| row.first())
                .map(|(_, v)| v.to_string().trim_matches('"').to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    };

    assert!(
        !definition.is_empty(),
        "034 must create {index}; the catalog returned nothing"
    );
    let parts: Vec<&str> = definition.split(['(', ',', ')']).map(str::trim).collect();
    let user_at = parts.iter().position(|p| *p == "user_id");
    let created_at = parts.iter().position(|p| *p == "created_at");
    assert!(
        matches!((user_at, created_at), (Some(u), Some(c)) if u < c),
        "{index} must lead with user_id then created_at, got: {definition}"
    );

    if matches!(kind, aigw_migrate::native::DbKind::Mysql) {
        // Forced so the assertion is about the index being usable for this
        // predicate, not about the optimizer's cost model on a tiny table.
        let plan = read_rows(&format!(
            "EXPLAIN SELECT user_id, created_at FROM mcp_call_log FORCE INDEX ({index}) \
             WHERE user_id = 'real-calls-a'"
        ))
        .await;
        let key = plan
            .first()
            .and_then(|row| row.iter().find(|(name, _)| name == "key"))
            .map(|(_, v)| v.to_string().trim_matches('"').to_string())
            .unwrap_or_default();
        assert_eq!(key, index, "MySQL plan must use {index}");
    }
}
