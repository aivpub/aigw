//! Step bindings for claude_oauth.feature — Stage 126 /credential/oauth/exchange
//!
//! Uses the shared in-memory SQLite test DB + a dedicated router with the
//! credential endpoints. The 3-step exchange runs against `MockUpstream`'s
//! OAuth routes (`/api/organizations`, `/v1/oauth/{org}/authorize`,
//! `/v1/oauth/token`), which are driven through the mock proxy.

use axum::http::Method;
use axum::Router;
use cucumber::{given, then, when};
use std::sync::Arc;
use tower::ServiceExt;

use super::e2e_steps::mock_upstream;
use crate::TestWorld;

fn build_credential_router(state: aigw_server::routes::keys::SharedState) -> Router {
    Router::new()
        .route(
            "/credential/oauth/exchange",
            axum::routing::post(aigw_server::routes::credentials::oauth_exchange),
        )
        .route(
            "/credential/info",
            axum::routing::get(aigw_server::routes::credentials::credential_info),
        )
        .with_state(state)
}

async fn send_request(
    world: &mut TestWorld,
    method: Method,
    uri: &str,
    auth: Option<&str>,
    body: Option<&str>,
) {
    let state = world.ensure_state().await;
    let app = build_credential_router(state);
    let mut req = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json");
    if let Some(token) = auth {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let req = req
        .body(axum::body::Body::from(body.unwrap_or("").to_string()))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    world.last_status = Some(response.status().as_u16());
    world.last_body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
}

/// Replace the world state with one carrying AIGW_MASTER_KEY so encrypt/decrypt
/// of OAuth credential fields round-trips. Distinct step text from proxy_steps'
/// "数据库已初始化且已配置 master key" to avoid a cucumber ambiguity.
#[given(expr = "数据库已初始化且已配置 master key（OAuth 场景）")]
async fn given_db_ready_with_master_key(world: &mut TestWorld) {
    let db = aigw_core::db::Database::init("sqlite::memory:")
        .await
        .expect("db init");
    let state: aigw_server::routes::keys::SharedState =
        Arc::new(aigw_server::routes::keys::AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: Some("bdd-master-key".to_string()),
            key_generate_length: aigw_server::routes::keys::DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            token_provider: std::sync::Arc::new(aigw_core::claude_token::TokenProvider::new()),
            metrics: None,
        });
    world.master_key = "sk-master-test".to_string();
    world.state = Some(state);
}

/// Configure the mock OAuth upstream by overriding the OAuth client's endpoint
/// constants. Since the exchange client uses hard-coded `https://claude.ai/...`
/// URLs, we remap the orgs/authorize/token endpoints to the mock's base URL so
/// the 3-step exchange runs entirely against MockUpstream (deterministic BDD).
#[given(expr = "mock 上游 OAuth 已配置")]
async fn given_mock_oauth_configured(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let mu = mock_upstream().lock().await;
    let mock_base = mu
        .as_ref()
        .expect("mock upstream started")
        .url()
        .to_string();
    drop(mu);

    // Store the mock base URL so the exchange step can inject it. We use a
    // OnceLock-style override via env that the OAuth client reads at request
    // time (kept in the BDD harness only — never in production code).
    std::env::set_var("AIGW_OAUTH_MOCK_BASE", &mock_base);
    let _ = state;
}

/// Set the mock upstream's `/api/organizations` route to return 401 so the
/// exchange classifies the cookie as invalid.
#[given(expr = "mock 上游 OAuth 组织接口返回 401")]
async fn given_mock_oauth_orgs_401(world: &mut TestWorld) {
    let _state = world.ensure_state().await;
    let mut mu = mock_upstream().lock().await;
    mu.as_mut().unwrap().set_response(
        "/api/organizations",
        401,
        serde_json::json!({"error": "unauthorized"}),
    );
    drop(mu);
}

/// Seed an OAuth credential with encrypted sensitive fields so the /credential/
/// info response can be asserted to redact them.
#[given(expr = "已存在 OAuth 凭证 {string}")]
async fn given_existing_oauth_credential(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let enc_access =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-access-secret", "bdd-master-key").unwrap();
    let enc_refresh =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-refresh-secret", "bdd-master-key")
            .unwrap();
    let enc_session =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-sid-secret", "bdd-master-key").unwrap();
    let cred = aigw_core::models::Credential {
        credential_id: uuid::Uuid::new_v4().to_string(),
        credential_name: name,
        credential_values: serde_json::json!({
            "type": "anthropic_oauth",
            "access_token": enc_access,
            "refresh_token": enc_refresh,
            "session_key": enc_session,
            "expires_at": 1752900000,
            "proxy_id": null,
            "org_uuid": "org-team-1",
            "status": "active",
        }),
        credential_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: None,
    };
    state
        .db
        .insert_credential(&cred)
        .await
        .expect("insert oauth credential");
}

/// Seed an OAuth credential whose access token is still valid and NOT expiring
/// (expires_at far in the future). The Stage 127 cache-hit scenario relies on
/// the refresh path: with a long-lived token, the cache miss triggers a refresh
/// which (through the mock token endpoint) returns `sk-ant-access-mock`.
/// (Distinct text from the plain `已存在 OAuth 凭证` to avoid ambiguity.)
#[given(expr = "已存在 OAuth 凭证 {string} 用于 token 获取")]
async fn given_existing_oauth_credential_cached(world: &mut TestWorld, name: String) {
    // Reuse the shared seed step for the raw credential (long-lived token).
    let state = world.ensure_state().await;
    let enc_access =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-access-secret", "bdd-master-key").unwrap();
    let enc_refresh =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-refresh-secret", "bdd-master-key")
            .unwrap();
    let enc_session =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-sid-secret", "bdd-master-key").unwrap();
    let cred = aigw_core::models::Credential {
        credential_id: uuid::Uuid::new_v4().to_string(),
        credential_name: name.clone(),
        credential_values: serde_json::json!({
            "type": "anthropic_oauth",
            "access_token": enc_access,
            "refresh_token": enc_refresh,
            "session_key": enc_session,
            "expires_at": chrono::Utc::now().timestamp() + 3600,
            "proxy_id": null,
            "org_uuid": "org-team-1",
            "status": "active",
        }),
        credential_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: None,
    };
    state
        .db
        .insert_credential(&cred)
        .await
        .expect("insert oauth credential");
    world.created_keys.insert("oauth:latest".to_string(), name);
}

/// Seed an OAuth credential whose refresh_token is invalid, forcing the
/// cookie self-heal path (refresh → invalid_grant → cookie exchange → mock
/// token endpoint returns sk-ant-access-mock).
#[given(expr = "已存在 OAuth 凭证 {string} 其 refresh_token 已失效")]
async fn given_oauth_credential_stale_refresh(world: &mut TestWorld, name: String) {
    let state = world.ensure_state().await;
    let enc_access =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-access-old", "bdd-master-key").unwrap();
    let enc_refresh =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-refresh-invalid", "bdd-master-key")
            .unwrap();
    let enc_session =
        aigw_core::crypto::encrypt_litellm_value("sk-ant-sid-heal", "bdd-master-key").unwrap();
    let cred = aigw_core::models::Credential {
        credential_id: uuid::Uuid::new_v4().to_string(),
        credential_name: name.clone(),
        credential_values: serde_json::json!({
            "type": "anthropic_oauth",
            "access_token": enc_access,
            "refresh_token": enc_refresh,
            "session_key": enc_session,
            "expires_at": chrono::Utc::now().timestamp() + 60,
            "proxy_id": null,
            "org_uuid": "org-team-1",
            "status": "active",
        }),
        credential_info: serde_json::json!({}),
        created_at: chrono::Utc::now().to_rfc3339(),
        created_by: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
        updated_by: None,
    };
    state
        .db
        .insert_credential(&cred)
        .await
        .expect("insert oauth credential");
    world.created_keys.insert("oauth:latest".to_string(), name);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送 POST \\/credential\\/oauth\\/exchange 请求")]
async fn when_oauth_exchange(world: &mut TestWorld, step: &cucumber::gherkin::Step) {
    let body = step.docstring.as_ref().expect("docstring body").to_string();
    send_request(
        world,
        Method::POST,
        "/credential/oauth/exchange",
        Some(&world.master_key.clone()),
        Some(&body),
    )
    .await;
}

#[when(expr = "发送 GET \\/credential\\/info 请求查询该凭证")]
async fn when_credential_info(world: &mut TestWorld) {
    // The most recently stored OAuth credential name.
    let name = world
        .created_keys
        .get("oauth:latest")
        .cloned()
        .unwrap_or_else(|| "oauth-cred-redact".to_string());
    let uri = format!("/credential/info?credential_name={}", name);
    send_request(
        world,
        Method::GET,
        &uri,
        Some(&world.master_key.clone()),
        None,
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 127 — Token lifecycle steps (TokenProvider through the mock OAuth)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过 TokenProvider 获取该凭证的 token")]
async fn when_get_token_via_provider(world: &mut TestWorld) {
    let state = world.ensure_state().await;
    let name = world
        .created_keys
        .get("oauth:latest")
        .cloned()
        .unwrap_or_else(|| "oauth-token-cache".to_string());
    let provider = aigw_core::claude_token::TokenProvider::new();
    let result = provider
        .get_access_token(&state.db, &name, "bdd-master-key")
        .await;
    match result {
        Ok(token) => {
            world.last_status = Some(200);
            world.last_body = Some(serde_json::json!({ "token": token }));
        }
        Err(e) => {
            world.last_status = Some(500);
            world.last_body = Some(serde_json::json!({ "error": e.to_string() }));
        }
    }
}

#[then(expr = "token 获取结果为 {string}")]
async fn then_token_result(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no token response");
    if let Some(token) = body.get("token").and_then(|v| v.as_str()) {
        assert_eq!(token, expected, "token mismatch");
        return;
    }
    panic!("token fetch failed: {}", body);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应凭证 type 为 {string}")]
async fn then_oauth_cred_type(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let cv = body
        .get("credential_values")
        .expect("credential_values in response");
    let ty = cv.get("type").and_then(|v| v.as_str()).expect("type");
    assert_eq!(ty, expected);
    // Remember the name for info lookup.
    if let Some(name) = body.get("credential_name").and_then(|v| v.as_str()) {
        world
            .created_keys
            .insert("oauth:latest".to_string(), name.to_string());
    }
}

#[then(expr = "响应凭证敏感字段已 redact")]
async fn then_oauth_sensitive_redacted(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let cv = body
        .get("credential_values")
        .or_else(|| body.get("data").and_then(|d| d.get(0)))
        .expect("credential_values in response");
    for key in ["access_token", "refresh_token", "session_key"] {
        let val = cv.get(key).and_then(|v| v.as_str()).expect(key);
        assert_eq!(val, "***", "{} must be redacted", key);
    }
    // Non-sensitive fields still visible.
    assert_eq!(cv["type"], "anthropic_oauth");
}

#[then(expr = "响应错误 kind 为 {string}")]
async fn then_oauth_error_kind(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let kind = body
        .get("error")
        .and_then(|e| e.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no error.kind in response: {}", body));
    assert_eq!(kind, expected);
}
