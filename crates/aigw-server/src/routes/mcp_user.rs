//! `/v1/mcp/*` — the current identity's view of the MCP registry and its own
//! upstream credentials (Stage 144 §3.4).
//!
//! These handlers take their identity from [`KeyIdentity`] and nothing else:
//! there is no `owner_id` parameter anywhere in this file, so there is no
//! injection point for filling in someone else's token. That is the runtime half
//! of the guarantee; the compile-time half is the signature.
//!
//! A master key has no `user_id`, and "whose credential goes upstream" has no
//! answer for it, so those calls are a 400 rather than an implicit admin
//! identity. An admin signed in through the UI does have a `user_id`
//! (`routes/login.rs` fixes it to the admin's own id), so this only reaches
//! callers hitting the API with the master key directly.

use aigw_core::mcp::credential::{is_token_usable, seal_token};
use aigw_core::middleware::KeyIdentity;
use aigw_core::models::{CredentialScope, McpServerRow, McpServerTokenRow, MCP_OWNER_USER};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

use super::keys::SharedState;
use super::mcp::{call_filter, call_log_response};
use super::spend::SpendAuth;

fn db_error(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": {"message": format!("{e}"), "type": "db_error"}})),
    )
}

fn bad_request(message: impl Into<String>, kind: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": message.into(), "type": kind}})),
    )
}

fn not_found(message: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"message": message, "type": "not_found"}})),
    )
}

/// The identity of the caller, or the 400 that says why there is none.
fn user_id_of(auth: &KeyIdentity) -> Result<String, (StatusCode, Json<Value>)> {
    auth.user_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| bad_request("该 key 未绑定用户，无法保存个人凭据", "key_without_user"))
}

async fn fetch_enabled_server(
    state: &SharedState,
    id: i64,
) -> Result<McpServerRow, (StatusCode, Json<Value>)> {
    state
        .db
        .get_mcp_server(id)
        .await
        .map_err(db_error)?
        .filter(|s| s.enabled)
        .ok_or_else(|| not_found("MCP server not found"))
}

fn owns(row: &McpServerTokenRow, user_id: &str) -> bool {
    row.owner_type == MCP_OWNER_USER && row.owner_id == user_id
}

async fn my_tokens(
    state: &SharedState,
    server_id: i64,
    user_id: &str,
) -> Result<Vec<McpServerTokenRow>, (StatusCode, Json<Value>)> {
    let rows = state
        .db
        .list_mcp_tokens(server_id)
        .await
        .map_err(db_error)?;
    Ok(rows.into_iter().filter(|t| owns(t, user_id)).collect())
}

/// The token the gateway would actually send for this user, if any. Mirrors
/// `resolve_bearer`, which resolves the *default* row and then checks usability —
/// reporting a token here that the call path would ignore would be a lie.
async fn usable_default(
    state: &SharedState,
    server_id: i64,
    user_id: &str,
) -> Result<Option<McpServerTokenRow>, (StatusCode, Json<Value>)> {
    let token = state
        .db
        .default_mcp_token(server_id, MCP_OWNER_USER, user_id)
        .await
        .map_err(db_error)?;
    Ok(token.filter(|t| is_token_usable(t, chrono::Utc::now()) && !t.token_value.is_empty()))
}

fn my_token_wire(token: &McpServerTokenRow) -> Value {
    json!({
        "token_name": token.token_name.clone(),
        "is_default": token.is_default,
        "expires_at": token.expires_at.clone(),
        "updated_at": token.updated_at.clone(),
    })
}

pub async fn list_my_servers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = user_id_of(&auth)?;
    let servers = state.db.list_mcp_servers().await.map_err(db_error)?;

    let mut data = Vec::new();
    for server in servers.into_iter().filter(|s| s.enabled) {
        let has_my_token = usable_default(&state, server.id, &user_id).await?.is_some();
        let token_required = server.scope() == CredentialScope::UserRequired && !has_my_token;
        data.push(json!({
            "id": server.id,
            "alias": server.alias,
            "display_name": server.display_name,
            "credential_scope": server.credential_scope,
            "enabled": server.enabled,
            "has_my_token": has_my_token,
            "token_required": token_required,
        }));
    }

    Ok(Json(
        json!({"object": "list", "data": data, "count": data.len()}),
    ))
}

pub async fn get_my_token(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = user_id_of(&auth)?;
    fetch_enabled_server(&state, id).await?;

    let token = state
        .db
        .default_mcp_token(id, MCP_OWNER_USER, &user_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("no token configured for this server on your identity"))?;
    Ok(Json(my_token_wire(&token)))
}

#[derive(Debug, Deserialize)]
pub struct SetMyTokenBody {
    pub token_name: String,
    pub token_value: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

fn normalize_expiry(raw: Option<&str>) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(raw) = raw.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(None);
    };
    chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| {
        bad_request(
            format!("expires_at must be an RFC 3339 timestamp, got \"{raw}\""),
            "invalid_expires_at",
        )
    })?;
    Ok(Some(raw.to_string()))
}

pub async fn put_my_token(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<SetMyTokenBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = user_id_of(&auth)?;
    fetch_enabled_server(&state, id).await?;

    let token_name = body.token_name.trim();
    if token_name.is_empty() {
        return Err(bad_request(
            "token_name must not be empty",
            "invalid_token_name",
        ));
    }
    super::mcp::validate_len(
        token_name,
        "token_name",
        super::mcp::MAX_NAME_LEN,
        "invalid_token_name",
    )?;
    let plaintext = body.token_value.trim();
    if plaintext.is_empty() {
        return Err(bad_request(
            "token_value must not be empty",
            "invalid_token_value",
        ));
    }
    let expires_at = normalize_expiry(body.expires_at.as_deref())?;

    let master_key = state.aigw_master_key.as_deref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {
                "message": "AIGW_MASTER_KEY is not configured — cannot encrypt MCP tokens",
                "type": "master_key_missing",
            }})),
        )
    })?;
    let sealed = seal_token(plaintext, master_key).map_err(|e| {
        warn!(error = %e, "MCP personal token seal failed");
        bad_request("could not encrypt token", "encrypt_failed")
    })?;

    let now = chrono::Utc::now().to_rfc3339();
    let existing = state
        .db
        .list_mcp_tokens(id)
        .await
        .map_err(db_error)?
        .into_iter()
        .find(|t| owns(t, &user_id) && t.token_name == token_name);

    let token_id = match existing {
        Some(mut row) => {
            row.token_value = sealed;
            row.enabled = true;
            row.expires_at = expires_at;
            row.updated_at = now.clone();
            state.db.update_mcp_token(&row).await.map_err(db_error)?;
            row.id
        }
        None => {
            let row = McpServerTokenRow {
                id: 0,
                server_id: id,
                owner_type: MCP_OWNER_USER.to_string(),
                owner_id: user_id.clone(),
                token_name: token_name.to_string(),
                token_value: sealed,
                is_default: false,
                enabled: true,
                expires_at,
                created_by: user_id.clone(),
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            state.db.create_mcp_token(&row).await.map_err(db_error)?
        }
    };

    state
        .db
        .set_default_mcp_token(id, MCP_OWNER_USER, &user_id, token_id)
        .await
        .map_err(db_error)?;

    let token = state
        .db
        .get_mcp_token(token_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("MCP token not found"))?;
    Ok(Json(my_token_wire(&token)))
}

pub async fn delete_my_token(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = user_id_of(&auth)?;
    fetch_enabled_server(&state, id).await?;

    for token in my_tokens(&state, id, &user_id).await? {
        state
            .db
            .delete_mcp_token(token.id)
            .await
            .map_err(db_error)?;
    }
    Ok(Json(json!({"status": "deleted", "server_id": id})))
}

/// Query string for `GET /v1/mcp/calls`. There is deliberately no `user_id`
/// field: the identity comes from the key and nowhere else, so an attempt to
/// pass someone else's id is not "rejected", it is simply not part of the
/// request's vocabulary and has no effect.
#[derive(Debug, Deserialize)]
pub struct MyCallQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub server_alias: Option<String>,
    pub tool_name: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// The caller's own call history — the answer to "I configured a token and it
/// still fails, what happened", which until now only an admin could give.
///
/// A key with no bound user gets a 400 instead of an empty list: "we do not
/// know who you are" and "you have made no calls" need different fixes, and an
/// empty table reads as the second.
pub async fn list_my_calls(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<MyCallQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let user_id = user_id_of(&auth)?;
    let filter = call_filter(
        query.server_alias,
        query.tool_name,
        query.status.as_deref(),
        query.from,
        query.to,
        Some(user_id),
    )?;
    call_log_response(&state, &filter, query.page, query.page_size).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::{AppState, DEFAULT_KEY_TOKEN_LEN};
    use aigw_core::mcp::credential::unseal_token;
    use aigw_core::models::{McpCallLogRow, VirtualKey, MCP_OWNER_SERVER};
    use axum::http::Method;
    use axum::routing::get;
    use axum::Router;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    const MK: &str = "sk-master-test";
    const OTHER_USER: &str = "user-2";

    async fn test_state() -> SharedState {
        let db = aigw_core::db::Database::init("sqlite::memory:")
            .await
            .unwrap();
        Arc::new(AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some(MK.to_string()),
            aigw_master_key: Some(MK.to_string()),
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            token_provider: Arc::new(aigw_core::claude_token::TokenProvider::new()),
            web_search: aigw_core::websearch::WebSearchHandle::disabled(),
            mcp_disable_rest: false,
            metrics: None,
        })
    }

    fn build_router(state: SharedState) -> Router {
        Router::new()
            .route("/v1/mcp/servers", get(list_my_servers))
            .route(
                "/v1/mcp/servers/{id}/my-token",
                get(get_my_token).put(put_my_token).delete(delete_my_token),
            )
            .route("/v1/mcp/calls", get(list_my_calls))
            .with_state(state)
    }

    fn render_key(user_id: Option<&str>, raw: &str) -> VirtualKey {
        VirtualKey {
            token: aigw_core::crypto::hash_token(raw),
            key_name: Some("test-key".to_string()),
            key_alias: Some("test-key".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: json!([]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: user_id.map(str::to_string),
            team_id: None,
            agent_id: None,
            project_id: None,
            permissions: json!({}),
            max_parallel_requests: None,
            metadata: json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: None,
            soft_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: json!([]),
            allowed_routes: json!([]),
            policies: json!([]),
            access_group_ids: json!([]),
            model_spend: json!({}),
            model_max_budget: json!({}),
            budget_id: None,
            organization_id: None,
            object_permission_id: None,
            created_at: Some(chrono::Utc::now()),
            created_by: Some("test".to_string()),
            updated_at: Some(chrono::Utc::now()),
            updated_by: Some("test".to_string()),
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
            user_email: None,
            user_alias: None,
        }
    }

    async fn insert_key(state: &SharedState, user_id: Option<&str>) -> String {
        let raw = format!("sk-{}", uuid::Uuid::new_v4());
        state
            .db
            .insert_key(&render_key(user_id, &raw))
            .await
            .expect("insert key");
        raw
    }

    async fn seed_server(state: &SharedState, alias: &str, scope: &str) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        state
            .db
            .create_mcp_server(&McpServerRow {
                id: 0,
                alias: alias.to_string(),
                display_name: format!("{alias} server"),
                url: "http://127.0.0.1:1/mcp".to_string(),
                transport: "http".to_string(),
                credential_scope: scope.to_string(),
                enabled: true,
                protocol_version: None,
                auth_header_name: None,
                auth_header_value: None,
                extra_headers: None,
                tool_count: 0,
                created_at: now.clone(),
                updated_at: now,
            })
            .await
            .expect("insert server")
    }

    async fn send(
        app: &Router,
        method: Method,
        uri: &str,
        token: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(axum::body::Body::from(
                body.map(|b| b.to_string()).unwrap_or_default(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), 262_144)
            .await
            .unwrap_or_default();
        let json = serde_json::from_slice(&bytes).unwrap_or(json!({}));
        (status, json)
    }

    #[tokio::test]
    async fn a_token_name_over_the_column_width_is_a_bad_request() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;
        let server = seed_server(&state, "demo", "user_required").await;

        let (s, resp) = send(
            &app,
            Method::PUT,
            &format!("/v1/mcp/servers/{server}/my-token"),
            &raw,
            Some(json!({
                "token_name": "名".repeat(256),
                "token_value": "plain-secret-value"
            })),
        )
        .await;
        assert_eq!(s, 400, "over-length token_name: {resp}");
        assert_eq!(resp["error"]["type"], "invalid_token_name");

        let stored = state.db.list_mcp_tokens(server).await.expect("tokens");
        assert!(stored.is_empty(), "no row may be written: {}", stored.len());
    }

    #[tokio::test]
    async fn a_personal_token_is_stored_as_gcm_ciphertext() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;
        let server = seed_server(&state, "demo", "user_required").await;

        let (s, _) = send(
            &app,
            Method::PUT,
            &format!("/v1/mcp/servers/{server}/my-token"),
            &raw,
            Some(json!({"token_name": "work", "token_value": "plain-secret-value"})),
        )
        .await;
        assert_eq!(s, 200);

        let stored = state.db.list_mcp_tokens(server).await.expect("tokens");
        assert_eq!(stored.len(), 1);
        assert!(
            stored[0].token_value.starts_with("v2:gcm:"),
            "stored as {}",
            stored[0].token_value
        );
        assert!(!stored[0].token_value.contains("plain-secret-value"));
        assert_eq!(
            unseal_token(&stored[0].token_value, MK).expect("unseal"),
            "plain-secret-value"
        );
    }

    #[tokio::test]
    async fn putting_the_same_name_again_updates_in_place_and_makes_it_default() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;
        let server = seed_server(&state, "demo", "user_required").await;

        for value in ["first-value", "second-value"] {
            let (s, resp) = send(
                &app,
                Method::PUT,
                &format!("/v1/mcp/servers/{server}/my-token"),
                &raw,
                Some(json!({"token_name": "work", "token_value": value})),
            )
            .await;
            assert_eq!(s, 200, "put failed: {resp}");
        }

        let stored = state.db.list_mcp_tokens(server).await.expect("tokens");
        assert_eq!(stored.len(), 1, "same name must update, not duplicate");
        assert_eq!(stored[0].token_name, "work");
        assert!(stored[0].is_default);
        assert_eq!(
            unseal_token(&stored[0].token_value, MK).expect("unseal"),
            "second-value"
        );

        let (s, resp) = send(
            &app,
            Method::GET,
            &format!("/v1/mcp/servers/{server}/my-token"),
            &raw,
            None,
        )
        .await;
        assert_eq!(s, 200, "get failed: {resp}");
        assert_eq!(resp["token_name"], "work");
        assert_eq!(resp["is_default"], true);
    }

    #[tokio::test]
    async fn getting_a_token_that_was_never_set_is_a_404() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;
        let server = seed_server(&state, "demo", "user_required").await;

        let (s, resp) = send(
            &app,
            Method::GET,
            &format!("/v1/mcp/servers/{server}/my-token"),
            &raw,
            None,
        )
        .await;
        assert_eq!(s, 404, "got {resp}");
    }

    #[tokio::test]
    async fn deleting_only_removes_the_callers_own_rows() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;
        let server = seed_server(&state, "demo", "shared_or_user").await;
        let now = chrono::Utc::now().to_rfc3339();

        for (owner_type, owner_id, name) in [
            (MCP_OWNER_SERVER, "", "shared"),
            (MCP_OWNER_USER, OTHER_USER, "theirs"),
            (MCP_OWNER_USER, "user-1", "mine"),
        ] {
            state
                .db
                .create_mcp_token(&McpServerTokenRow {
                    id: 0,
                    server_id: server,
                    owner_type: owner_type.to_string(),
                    owner_id: owner_id.to_string(),
                    token_name: name.to_string(),
                    token_value: "v2:gcm:opaque".to_string(),
                    is_default: false,
                    enabled: true,
                    expires_at: None,
                    created_by: "test".to_string(),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                })
                .await
                .expect("insert token");
        }

        let (s, _) = send(
            &app,
            Method::DELETE,
            &format!("/v1/mcp/servers/{server}/my-token"),
            &raw,
            None,
        )
        .await;
        assert_eq!(s, 200);

        let names: Vec<String> = state
            .db
            .list_mcp_tokens(server)
            .await
            .expect("tokens")
            .into_iter()
            .map(|t| format!("{}:{}", t.owner_id, t.token_name))
            .collect();
        assert_eq!(
            names,
            vec![":shared".to_string(), format!("{OTHER_USER}:theirs")]
        );
    }

    #[tokio::test]
    async fn token_required_flips_once_a_personal_token_exists() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;
        let server = seed_server(&state, "demo", "user_required").await;

        let (s, resp) = send(&app, Method::GET, "/v1/mcp/servers", &raw, None).await;
        assert_eq!(s, 200);
        assert_eq!(resp["data"][0]["token_required"], true, "got {resp}");
        assert_eq!(resp["data"][0]["has_my_token"], false);

        let (s, _) = send(
            &app,
            Method::PUT,
            &format!("/v1/mcp/servers/{server}/my-token"),
            &raw,
            Some(json!({"token_name": "work", "token_value": "secret"})),
        )
        .await;
        assert_eq!(s, 200);

        let (s, resp) = send(&app, Method::GET, "/v1/mcp/servers", &raw, None).await;
        assert_eq!(s, 200);
        assert_eq!(resp["data"][0]["token_required"], false, "got {resp}");
        assert_eq!(resp["data"][0]["has_my_token"], true);
    }

    #[tokio::test]
    async fn a_key_without_a_user_cannot_touch_personal_tokens() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let server = seed_server(&state, "demo", "user_required").await;

        let cases = [
            (Method::GET, "/v1/mcp/servers".to_string(), None),
            (
                Method::GET,
                format!("/v1/mcp/servers/{server}/my-token"),
                None,
            ),
            (
                Method::PUT,
                format!("/v1/mcp/servers/{server}/my-token"),
                Some(json!({"token_name": "work", "token_value": "secret"})),
            ),
            (
                Method::DELETE,
                format!("/v1/mcp/servers/{server}/my-token"),
                None,
            ),
        ];
        for (method, uri, body) in cases {
            let (s, resp) = send(&app, method.clone(), &uri, MK, body).await;
            assert_eq!(s, 400, "{method} {uri} gave {resp}");
            assert_eq!(resp["error"]["type"], "key_without_user");
        }

        assert!(state
            .db
            .list_mcp_tokens(server)
            .await
            .expect("tokens")
            .is_empty());
    }

    fn record_call(
        user: Option<&str>,
        alias: &str,
        tool: &str,
        status: &str,
        at: &str,
    ) -> McpCallLogRow {
        McpCallLogRow {
            call_id: uuid::Uuid::new_v4().to_string(),
            server_alias: alias.to_string(),
            tool_name: Some(tool.to_string()),
            method: "tools/call".to_string(),
            key_id: Some("*master*".to_string()),
            user_id: user.map(str::to_string),
            token_owner: "user".to_string(),
            status: status.to_string(),
            error_code: None,
            latency_ms: 3,
            request_bytes: 7,
            response_bytes: 9,
            created_at: at.to_string(),
        }
    }

    /// Two users, same alias, same timestamp — only the identity can separate
    /// them.
    async fn seed_calls(state: &SharedState) {
        for (user, tool) in [(Some("user-1"), "search"), (Some(OTHER_USER), "read")] {
            state
                .db
                .insert_mcp_call_log(&record_call(
                    user,
                    "demo",
                    tool,
                    "success",
                    "2026-10-31T00:00:00+00:00",
                ))
                .await
                .expect("insert call");
        }
        state
            .db
            .insert_mcp_call_log(&record_call(
                None,
                "demo",
                "search",
                "failed",
                "2026-10-31T00:00:00+00:00",
            ))
            .await
            .expect("insert anonymous call");
    }

    fn user_ids(resp: &Value) -> Vec<Option<String>> {
        resp["data"]
            .as_array()
            .expect("data array")
            .iter()
            .map(|row| row["user_id"].as_str().map(str::to_string))
            .collect()
    }

    #[tokio::test]
    async fn a_user_sees_only_their_own_calls() {
        let state = test_state().await;
        let app = build_router(state.clone());
        seed_calls(&state).await;
        let raw = insert_key(&state, Some("user-1")).await;

        let (s, resp) = send(&app, Method::GET, "/v1/mcp/calls", &raw, None).await;
        assert_eq!(s, 200, "{resp}");
        assert_eq!(user_ids(&resp), vec![Some("user-1".to_string())]);
        assert_eq!(resp["total_count"], 1);
    }

    #[tokio::test]
    async fn passing_someone_elses_user_id_changes_nothing() {
        let state = test_state().await;
        let app = build_router(state.clone());
        seed_calls(&state).await;
        let raw = insert_key(&state, Some("user-1")).await;

        let (s, resp) = send(
            &app,
            Method::GET,
            &format!("/v1/mcp/calls?user_id={OTHER_USER}"),
            &raw,
            None,
        )
        .await;
        assert_eq!(s, 200, "{resp}");
        assert_eq!(
            user_ids(&resp),
            vec![Some("user-1".to_string())],
            "user_id is not part of this endpoint's vocabulary: {resp}"
        );
    }

    #[tokio::test]
    async fn a_key_without_a_user_gets_a_400_not_an_empty_list() {
        let state = test_state().await;
        let app = build_router(state.clone());
        seed_calls(&state).await;

        let (s, resp) = send(&app, Method::GET, "/v1/mcp/calls", MK, None).await;
        assert_eq!(s, 400, "{resp}");
        assert_eq!(resp["error"]["type"], "key_without_user");
    }

    #[tokio::test]
    async fn the_alias_and_status_filters_apply_to_my_calls() {
        let state = test_state().await;
        let app = build_router(state.clone());
        seed_calls(&state).await;
        state
            .db
            .insert_mcp_call_log(&record_call(
                Some("user-1"),
                "other",
                "search",
                "success",
                "2026-10-31T01:00:00+00:00",
            ))
            .await
            .expect("insert call");
        let raw = insert_key(&state, Some("user-1")).await;

        let (_, resp) = send(
            &app,
            Method::GET,
            "/v1/mcp/calls?server_alias=other",
            &raw,
            None,
        )
        .await;
        assert_eq!(resp["total_count"], 1, "{resp}");
        assert_eq!(resp["data"][0]["server_alias"], "other");

        let (_, resp) = send(&app, Method::GET, "/v1/mcp/calls?status=failed", &raw, None).await;
        assert_eq!(resp["total_count"], 0, "{resp}");

        let (_, resp) = send(&app, Method::GET, "/v1/mcp/calls?tool_name=SEA", &raw, None).await;
        assert_eq!(
            resp["total_count"], 2,
            "tool_name is a case-insensitive substring: {resp}"
        );
    }

    #[tokio::test]
    async fn an_unparseable_from_is_a_bad_request() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key(&state, Some("user-1")).await;

        let (s, resp) = send(
            &app,
            Method::GET,
            "/v1/mcp/calls?from=yesterday",
            &raw,
            None,
        )
        .await;
        assert_eq!(s, 400, "{resp}");
        assert_eq!(resp["error"]["type"], "invalid_time_range");
    }

    #[tokio::test]
    async fn the_envelope_matches_the_admin_endpoint() {
        let state = test_state().await;
        let app = build_router(state.clone());
        seed_calls(&state).await;
        let raw = insert_key(&state, Some("user-1")).await;

        let (_, resp) = send(
            &app,
            Method::GET,
            "/v1/mcp/calls?page=1&page_size=50",
            &raw,
            None,
        )
        .await;
        assert_eq!(resp["object"], "list");
        assert_eq!(resp["count"], 1);
        assert_eq!(resp["page"], 1);
        assert_eq!(resp["page_size"], 50);
        assert_eq!(resp["total_pages"], 1);
        assert!(resp["data"].is_array());
    }
}
