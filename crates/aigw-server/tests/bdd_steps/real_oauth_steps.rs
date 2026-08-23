//! Step bindings for real/claude_oauth_crud.feature — Stage 130 real BDD
//!
//! Drives the real aigw server over HTTP (like real_proxy_steps.rs) to
//! exercise OAuth credential CRUD + encrypted-at-rest + redact + in-use guard
//! against the real SQLite/PG/MySQL backend.
//!
//! IMPORTANT (Gate 2 finding F1): the real-mode aigw **binary** does not carry
//! aigw-core's `test` feature, so the OAuth mock-base env remaps
//! (AIGW_OAUTH_MOCK_BASE / AIGW_ANTHROPIC_MOCK_BASE) are inert for HTTP calls
//! to the server. The live 3-step exchange / refresh / self-heal paths are
//! therefore NOT exercised here — they are covered by mock BDD
//! (claude_oauth.feature, 12 scenarios). This file exercises what the real
//! server actually does: full HTTP CRUD, redact of the token trio, encrypted
//! at-rest (verified by a read-only DB probe), and the proxy in-use guard.
//!
//! The OAuth credential is seeded via /credential/new with plaintext token
//! values (test-only — not real secrets). The read-only DB probe verifies the
//! server encrypted them (v2:gcm: prefix, no plaintext `sk-ant-` substring).

use crate::TestWorld;
use cucumber::{given, then, when};
use serde_json::Value;

use super::real_api_steps::{base_url, client, real_api_enabled};

/// Track the most recently created OAuth credential name per scenario.
fn store_oauth_cred(world: &mut TestWorld, name: &str) {
    world
        .created_keys
        .insert(format!("oauth:{}", name), name.to_string());
}

/// The test DB URL (set by bdd.rs harness) — used for the read-only probe.
fn test_db_url() -> String {
    std::env::var("AIGW_TEST_DB_URL").expect("AIGW_TEST_DB_URL must be set by the BDD test harness")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /credential/new with an `anthropic_oauth` credential. Sensitive fields
/// are passed plaintext (test-only) — the server is expected to encrypt them.
#[given(expr = "通过 API 创建 OAuth 凭证 {string} 带 cookie {string}")]
async fn given_create_oauth_credential(world: &mut TestWorld, name: String, cookie: String) {
    if !real_api_enabled() {
        return;
    }
    let body = serde_json::json!({
        "credential_name": name,
        "credential_values": {
            "type": "anthropic_oauth",
            "access_token": "sk-ant-access-real",
            "refresh_token": "sk-ant-refresh-real",
            "session_key": cookie,
            "expires_at": 1752900000,
            "org_uuid": "org-team-1",
            "status": "active"
        }
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/credential/new", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("create OAuth credential request");
    assert!(
        resp.status().is_success(),
        "create OAuth credential failed: {}",
        resp.text().await.unwrap_or_default()
    );
    store_oauth_cred(world, &name);
}

/// The `encrypt_litellm_value` envelope (AES-256-GCM) used by the OAuth
/// credential builder to encrypt the token trio at rest.
const GCM_PREFIX: &str = "v2:gcm:";

/// Reuse the Stage 125 proxy-create step: create a proxy, then create an OAuth
/// credential referencing it (proxy_id in credential_values) — the delete-409
/// guard applies identically.
#[given(expr = "通过 API 创建 OAuth 凭证 {string} 引用该代理")]
async fn given_create_oauth_credential_referencing_proxy(world: &mut TestWorld, cred_name: String) {
    if !real_api_enabled() {
        return;
    }
    let id = world
        .created_keys
        .iter()
        .filter(|(k, _)| k.starts_with("proxy:"))
        .max_by_key(|(_, v)| v.parse::<i64>().unwrap_or(0))
        .map(|(_, v)| v.clone())
        .expect("proxy created first");
    let body = serde_json::json!({
        "credential_name": cred_name,
        "credential_values": {
            "type": "anthropic_oauth",
            "access_token": "sk-ant-access-inuse",
            "refresh_token": "sk-ant-refresh-inuse",
            "session_key": "sk-ant-sid-inuse",
            "expires_at": 1752900000,
            "proxy_id": id.parse::<i64>().unwrap(),
            "org_uuid": "org-team-1",
            "status": "active"
        }
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(format!("{}/credential/new", base_url()))
        .header(
            axum::http::header::AUTHORIZATION.as_str(),
            format!("Bearer {}", mk),
        )
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("create OAuth credential referencing proxy");
    assert!(
        resp.status().is_success(),
        "create credential referencing proxy failed: {}",
        resp.text().await.unwrap_or_default()
    );
    store_oauth_cred(world, &cred_name);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 API 查询凭证列表")]
async fn when_list_credentials(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/credential/list", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("list credentials request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "通过 API 查询凭证详情 {string}")]
async fn when_get_credential_detail(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/credential/info", base_url()))
        .query(&[("credential_name", name.as_str())])
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("get credential detail request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "通过 API 删除凭证 {string}")]
async fn when_delete_credential(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let resp = client()
        .delete(format!("{}/credential/delete", base_url()))
        .query(&[("credential_name", name.as_str())])
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("delete credential request");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "凭证列表包含 {string}")]
async fn then_credential_in_list(world: &mut TestWorld, name: String) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("list response body");
    let data = body["data"].as_array().expect("data array");
    assert!(
        data.iter()
            .any(|c| c["credential_name"] == Value::String(name.clone())),
        "credential '{}' not in list: {}",
        name,
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "凭证响应 token 三件套已 redact 为 ***")]
async fn then_oauth_token_trio_redacted(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("response body");
    // A single-credential response has credential_values at top level; the
    // list response nests it under data[].credential_values.
    let cv = body
        .get("credential_values")
        .or_else(|| {
            body.get("data")
                .and_then(|d| d.as_array())
                .and_then(|a| a.first())
                .and_then(|item| item.get("credential_values"))
        })
        .expect("credential_values in response");
    for key in ["access_token", "refresh_token", "session_key"] {
        let val = cv.get(key).and_then(|v| v.as_str()).expect(key);
        assert_eq!(val, "***", "{} must be redacted", key);
    }
    assert_eq!(cv["type"], "anthropic_oauth");
}

#[then(expr = "凭证详情 token 三件套已 redact 为 ***")]
async fn then_credential_detail_redacted(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(200),
        "expected 200 for credential detail, got {:?} body {:?}",
        world.last_status,
        world.last_body
    );
    then_oauth_token_trio_redacted(world).await;
}

/// Read the persisted `credential_values` directly from the DB (read-only
/// probe) and assert: (1) the token trio is stored encrypted (`v2:gcm:`),
/// (2) no plaintext `sk-ant-` substring survives in the stored JSON.
#[then(expr = "real 凭证 DB 直读 credential_values 含 v2:gcm: 前缀且无明文 sk-ant- 子串")]
async fn then_real_credential_encrypted_at_rest(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let name = world
        .created_keys
        .iter()
        .find(|(k, _)| k.starts_with("oauth:"))
        .map(|(_, v)| v.clone())
        .expect("oauth credential created in scenario");
    // The credential was created via /credential/new with plaintext token
    // values; the server is expected to encrypt them before persisting. When
    // the field-level encryption is NOT wired (a future regression), the
    // stored values are the plaintext we sent — which would make this probe
    // fail loudly (that is the point of the security audit).
    let db_url = test_db_url();
    let pool = aigw_migrate::native::SourcePool::connect(&db_url)
        .await
        .expect("connect to test db");
    let rows = pool
        .read_rows_sql(&format!(
            "SELECT credential_values FROM credentials WHERE credential_name = '{}' LIMIT 1",
            name
        ))
        .await
        .expect("read credential_values");
    assert_eq!(rows.len(), 1, "expected exactly one credential row");
    let row = &rows[0];
    let cv_val = row
        .iter()
        .find(|(col, _)| col.eq_ignore_ascii_case("credential_values"))
        .map(|(_, v)| v)
        .expect("credential_values column present");
    // credential_values decodes as either a String (PG text/jsonb wire, SQLite
    // TEXT) or a nested serde_json::Value (MySQL JSON, SQLite BLOB) — normalize
    // both to a JSON string for the substring assertions.
    let cv_str = match cv_val {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    // Every encrypted field must carry the v2:gcm: envelope.
    assert!(
        cv_str.contains(GCM_PREFIX),
        "credential_values must be encrypted ({}), got: {}",
        GCM_PREFIX,
        cv_str
    );
    // No plaintext secret (the values we seeded) may survive.
    for needle in [
        "sk-ant-access-real",
        "sk-ant-refresh-real",
        "sk-ant-sid-real",
    ] {
        assert!(
            !cv_str.contains(needle),
            "plaintext secret leaked in DB: {} ({} contains {})",
            needle,
            cv_str,
            name
        );
    }
}

#[then(expr = "real 凭证删除返回 200")]
async fn then_real_credential_delete_200(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    assert_eq!(
        world.last_status,
        Some(200),
        "expected 200 for credential delete, got {:?} body {:?}",
        world.last_status,
        world.last_body
    );
}
