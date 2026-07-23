//! Login/Logout endpoints — litellm-compatible /v2/login/*
//!
//! Endpoints:
//! - POST /v2/login       — Username+password login, returns HttpOnly cookie JWT
//! - POST /v2/logout      — Clear cookie + delete temp session key
//! - GET  /v2/login/check — Check if current cookie JWT is valid

use aigw_core::auth::{decode_jwt, encode_jwt, JwtClaims};
use aigw_core::crypto::hash_token;
use aigw_core::models::VirtualKey;
use aigw_core::password::verify_password;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::keys::{generate_key_token, SharedState};

/// litellm session key team_id marker — these keys are filtered from /key/list
const UI_SESSION_TEAM_ID: &str = "litellm-dashboard";

/// Default session duration: 24 hours
const DEFAULT_SESSION_DURATION_HOURS: i64 = 24;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request / Response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user_id: String,
    pub user_role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Read UI_USERNAME from env, default "admin"
fn ui_username() -> String {
    std::env::var("UI_USERNAME").unwrap_or_else(|_| "admin".to_string())
}

/// Read session duration from LITELLM_UI_SESSION_DURATION env (e.g. "24h")
fn session_duration() -> Duration {
    let default = Duration::hours(DEFAULT_SESSION_DURATION_HOURS);
    std::env::var("LITELLM_UI_SESSION_DURATION")
        .ok()
        .and_then(|s| {
            let s = s.trim();
            if let Some(hours_str) = s.strip_suffix('h') {
                hours_str.parse::<i64>().ok().map(Duration::hours)
            } else if let Some(mins_str) = s.strip_suffix('m') {
                mins_str.parse::<i64>().ok().map(Duration::minutes)
            } else {
                None
            }
        })
        .unwrap_or(default)
}

/// Build the Set-Cookie header value for a JWT
fn cookie_header(jwt: &str, max_age_secs: i64) -> String {
    format!(
        "token={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        jwt, max_age_secs
    )
}

/// Build a Set-Cookie header that clears the token cookie (logout)
fn clear_cookie_header() -> String {
    "token=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0".to_string()
}

/// Extract a cookie value from request headers
fn get_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(|s| s.trim())
        .find_map(|part| {
            let (k, v) = part.split_once('=')?;
            if k == name { Some(v.to_string()) } else { None }
        })
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handlers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /v2/login
pub async fn login(
    State(state): State<SharedState>,
    Json(req): Json<LoginRequest>,
) -> Result<(StatusCode, [(String, String); 1], Json<Value>), (StatusCode, Json<Value>)> {
    let master_key = state
        .master_key
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": "Server misconfigured", "type": "server_error"}})),
            )
        })?;

    let username = req.username.trim();
    let password = req.password;

    // Determine user_id
    let expected_username = ui_username();
    let user_id: String;

    if username == expected_username {
        // Admin login: password must match master_key or UI_PASSWORD env var
        let ui_password = std::env::var("UI_PASSWORD").unwrap_or_else(|_| master_key.clone());
        if password != *master_key && password != ui_password {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": {"message": "Invalid credentials", "type": "auth_error"}})),
            ));
        }
        user_id = "default_user_id".to_string();
    } else {
        // Database user login: look up by user_email
        match state.db.get_user_by_email(username).await {
            Ok(Some(user)) => {
                // Verify scrypt password hash
                let pw_hash = user.password.unwrap_or_default();
                let valid = verify_password(&password, &pw_hash).unwrap_or(false);
                if !valid {
                    return Err((
                        StatusCode::UNAUTHORIZED,
                        Json(json!({"error": {"message": "Invalid credentials", "type": "auth_error"}})),
                    ));
                }
                user_id = user.user_id;
            }
            Ok(None) => {
                return Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"message": "Invalid credentials", "type": "auth_error"}})),
                ));
            }
            Err(_) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": "Internal error", "type": "server_error"}})),
                ));
            }
        }
    }

    // Create temporary session key
    let raw_token = generate_key_token();
    let token_hash = hash_token(&raw_token);
    let expiry = Utc::now() + session_duration();
    let now = Utc::now();

    let session_key = VirtualKey {
        token: token_hash.clone(),
        key_name: Some(format!("ui-session-{}", &user_id)),
        key_alias: Some(format!("ui-session-{}", &user_id)),
        soft_budget_cooldown: "false".to_string(),
        spend: 0.0,
        expires: Some(expiry),
        models: json!([]),
        aliases: json!({}),
        config: json!({}),
        router_settings: None,
        user_id: Some(user_id.clone()),
        team_id: Some(UI_SESSION_TEAM_ID.to_string()),
        agent_id: None,
        project_id: None,
        permissions: json!({}),
        max_parallel_requests: None,
        metadata: json!({}),
        blocked: None,
        tpm_limit: None,
        rpm_limit: None,
        max_budget: None,
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
        created_at: Some(now),
        created_by: None,
        updated_at: Some(now),
        updated_by: None,
        last_active: None,
        rotation_count: None,
        auto_rotate: None,
        rotation_interval: None,
        last_rotation_at: None,
        key_rotation_at: None,
        budget_limits: None,
    };

    state.db.insert_key(&session_key).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    // Create JWT with session key
    let claims = JwtClaims {
        user_id: user_id.clone(),
        key: raw_token,
        user_email: if username == expected_username {
            None
        } else {
            Some(username.to_string())
        },
        user_role: "proxy_admin".to_string(),
        login_method: "username_password".to_string(),
    };

    let jwt = encode_jwt(&claims, master_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": e, "type": "server_error"}})),
        )
    })?;

    let max_age = session_duration().num_seconds();
    let cookie = cookie_header(&jwt, max_age);

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE.to_string(), cookie)],
        Json(json!(LoginResponse {
            user_id: claims.user_id,
            user_role: claims.user_role,
            user_email: claims.user_email,
        })),
    ))
}

/// POST /v2/logout
#[allow(dead_code)]
pub async fn logout(
    State(_state): State<SharedState>,
) -> Result<(StatusCode, [(String, String); 1], Json<Value>), (StatusCode, Json<Value>)> {
    // Try to extract JWT from cookie to delete the temp key
    // We read the cookie header directly from the request
    // Since axum doesn't give us access to parts here, we use an extractor pattern
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE.to_string(), clear_cookie_header())],
        Json(json!({"status": "ok", "message": "Logged out"})),
    ))
}

/// POST /v2/logout with cookie extraction and key deletion
pub async fn logout_with_cleanup(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<(StatusCode, [(String, String); 1], Json<Value>), (StatusCode, Json<Value>)> {
    let master_key = state.master_key.as_ref().map(|k| k.clone()).unwrap_or_default();

    // Extract JWT from cookie and delete the session key
    if let Some(token) = get_cookie(&headers, "token") {
        if let Ok(claims) = decode_jwt(&token, &master_key) {
            let token_hash = hash_token(&claims.key);
            let _ = state.db.delete_key(&token_hash).await;
        }
    }

    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE.to_string(), clear_cookie_header())],
        Json(json!({"status": "ok", "message": "Logged out"})),
    ))
}

/// GET /v2/login/check
pub async fn login_check(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let master_key = state.master_key.as_ref().map(|k| k.clone()).unwrap_or_default();

    let token = get_cookie(&headers, "token").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": "Not authenticated", "type": "auth_error"}})),
        )
    })?;

    let claims = decode_jwt(&token, &master_key).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": "Invalid session", "type": "auth_error"}})),
        )
    })?;

    // Verify the session key still exists and is not expired/blocked
    let token_hash = hash_token(&claims.key);
    let key = state.db.get_key_by_token(&token_hash).await.map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": "Session expired", "type": "auth_error"}})),
        )
    })?;

    match key {
        Some(_) => Ok(Json(json!({
            "user_id": claims.user_id,
            "user_role": claims.user_role,
            "user_email": claims.user_email,
        }))),
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"message": "Session expired", "type": "auth_error"}})),
        )),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use aigw_core::db::Database;
    use aigw_core::provider::ProviderRegistry;
    use aigw_core::rate_limiter::RateLimiter;
    use aigw_core::router::{Router as AigwRouter, RouterState};
use aigw_core::resolver::ModelResolver;
    use axum::{
        body::Body,
        http::{header, Method, Request},
        Router,
    };
    use std::sync::Arc;
    use tower::util::ServiceExt;

    use crate::routes::keys::AppState;

    async fn test_app() -> Router {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test".to_string()),
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            metrics: None,
        });
        Router::new()
            .route("/v2/login", axum::routing::post(login))
            .route("/v2/logout", axum::routing::post(logout_with_cleanup))
            .route("/v2/login/check", axum::routing::get(login_check))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_login_success() {
        // Set UI_USERNAME for this test
        std::env::set_var("UI_USERNAME", "admin");
        std::env::set_var("UI_PASSWORD", "sk-master-test");

        let app = test_app().await;
        let body = json!({"username": "admin", "password": "sk-master-test"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v2/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        // Check Set-Cookie header
        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(
            cookies.iter().any(|c| c.contains("token=") && c.contains("HttpOnly")),
            "Expected HttpOnly cookie, got: {:?}",
            cookies
        );

        // Check response body
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json_val: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json_val["user_role"], "proxy_admin");
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        std::env::set_var("UI_USERNAME", "admin");
        std::env::remove_var("UI_PASSWORD");

        let app = test_app().await;
        let body = json!({"username": "admin", "password": "wrong-password"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v2/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 401);
    }

    #[tokio::test]
    async fn test_login_wrong_username() {
        std::env::set_var("UI_USERNAME", "admin");

        let app = test_app().await;
        let body = json!({"username": "nobody", "password": "anything"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v2/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 401);
    }

    #[tokio::test]
    async fn test_login_check_unauthorized() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v2/login/check")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 401);
    }

    #[tokio::test]
    async fn test_login_check_after_login() {
        std::env::set_var("UI_USERNAME", "admin");
        std::env::set_var("UI_PASSWORD", "sk-master-test");

        let app = test_app().await;

        // 1. Login to get cookie
        let body = json!({"username": "admin", "password": "sk-master-test"});
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v2/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let cookie_value: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| {
                let s = v.to_str().unwrap();
                s.split(';').next().map(|c| c.to_string())
            })
            .collect();

        // 2. Check with cookie
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v2/login/check")
            .header(header::COOKIE, cookie_value.join("; "))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_logout() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v2/logout")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let cookies: Vec<_> = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_string())
            .collect();
        assert!(
            cookies
                .iter()
                .any(|c| c.contains("token=") && c.contains("Max-Age=0")),
            "Expected clear cookie, got: {:?}",
            cookies
        );
    }
}
