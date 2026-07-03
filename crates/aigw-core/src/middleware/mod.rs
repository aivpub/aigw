//! Auth middleware — Virtual Key + Master Key authentication
//!
//! Extracts Bearer token from Authorization header, hashes it,
//! and looks up in virtual_keys table via the KeyStore trait.
//! Falls back to master key check.
//!
//! # Usage
//!
//! ```rust,ignore
//! use aigw_core::middleware::KeyIdentity;
//!
//! async fn handler(auth: KeyIdentity) -> Json<Value> {
//!     // auth contains the extracted identity
//! }
//! ```
//!
//! The master key must be provided via axum state (Arc<AppState>).

pub mod auth_gateway;
pub mod rate_limit;

use std::sync::Arc;

use axum::{
    extract::FromRef,
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
};
use sqlx::SqlitePool;

use crate::crypto::hash_token;
use crate::db::KeyStore;

/// Extracted key identity after auth
#[derive(Debug, Clone)]
pub struct KeyIdentity {
    pub token_hash: String,
    pub key_alias: Option<String>,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub organization_id: Option<String>,
    pub is_master_key: bool,
}

impl KeyIdentity {
    /// Returns true if the request was authenticated via the master key.
    pub fn is_admin(&self) -> bool {
        self.is_master_key
    }
}

/// Auth extraction error — litellm-compatible JSON responses
#[derive(Debug)]
pub enum AuthError {
    MissingHeader,
    InvalidFormat,
    TokenNotFound,
    TokenExpired,
    TokenBlocked,
}

impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AuthError::MissingHeader => (StatusCode::UNAUTHORIZED, "Missing Authorization header"),
            AuthError::InvalidFormat => (StatusCode::UNAUTHORIZED, "Invalid Authorization format"),
            AuthError::TokenNotFound => (StatusCode::UNAUTHORIZED, "Invalid API key"),
            AuthError::TokenExpired => (StatusCode::UNAUTHORIZED, "API key expired"),
            AuthError::TokenBlocked => (StatusCode::FORBIDDEN, "API key blocked"),
        };
        let body = serde_json::json!({ "error": { "message": message, "type": "auth_error" } });
        (status, axum::Json(body)).into_response()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AppState — shared application state for auth middleware
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Shared application state accessible by the auth middleware.
#[derive(Clone, FromRef)]
pub struct AppState {
    pub master_key: String,
    pub db_pool: SqlitePool,
}

impl AppState {
    pub fn new(master_key: String, db_pool: SqlitePool) -> Self {
        Self {
            master_key,
            db_pool,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// FromRequestParts implementation for KeyIdentity
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl<S> FromRequestParts<S> for KeyIdentity
where
    S: Send + Sync,
    Arc<AppState>: axum::extract::FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let app_state: Arc<AppState> = FromRef::from_ref(state);
        authenticate(&app_state, parts).await
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Authentication logic
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Extract the Bearer token from the Authorization header and validate it.
async fn authenticate(
    state: &AppState,
    parts: &Parts,
) -> std::result::Result<KeyIdentity, AuthError> {
    // 1. Extract Authorization header
    let auth_header = parts
        .headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::MissingHeader)?;

    // 2. Extract Bearer token
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(AuthError::InvalidFormat)?;

    if token.is_empty() {
        return Err(AuthError::InvalidFormat);
    }

    // 3. Check master key (raw comparison for admin access)
    if token == state.master_key {
        return Ok(KeyIdentity {
            token_hash: "*master*".to_string(),
            key_alias: Some("master".to_string()),
            user_id: None,
            team_id: None,
            organization_id: None,
            is_master_key: true,
        });
    }

    // 4. SHA256 hash the token and lookup in virtual_keys
    let token_hash = hash_token(token);
    let key = state
        .db_pool
        .get_key_by_token(&token_hash)
        .await
        .map_err(|_| AuthError::TokenNotFound)?;

    match key {
        Some(k) => Ok(KeyIdentity {
            token_hash,
            key_alias: k.key_alias,
            user_id: k.user_id,
            team_id: k.team_id,
            organization_id: k.organization_id,
            is_master_key: false,
        }),
        None => Err(AuthError::TokenNotFound),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::VirtualKey;
    use axum::{body::Body, http::Request, routing::get, Router};
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use tower::ServiceExt;

    fn make_key(
        token_hash: &str,
        blocked: Option<bool>,
        expires: Option<DateTime<Utc>>,
    ) -> VirtualKey {
        VirtualKey {
            token: token_hash.to_string(),
            key_name: Some("test-key".to_string()),
            key_alias: Some("test-alias".to_string()),
            soft_budget_cooldown: false,
            spend: 0.0,
            expires,
            models: json!([]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: Some("test-user".to_string()),
            team_id: Some("test-team".to_string()),
            agent_id: None,
            project_id: None,
            permissions: json!({}),
            max_parallel_requests: None,
            metadata: json!({}),
            blocked,
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
            created_at: Some(Utc::now()),
            created_by: None,
            updated_at: Some(Utc::now()),
            updated_by: None,
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
        }
    }

    async fn setup_test_app() -> (Router, SqlitePool, String) {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let pool = match db {
            Database::Sqlite(p) => p,
            _ => unreachable!(),
        };
        let master_key = "sk-master-test".to_string();
        let state = Arc::new(AppState::new(master_key.clone(), pool.clone()));

        async fn handler(auth: KeyIdentity) -> axum::Json<serde_json::Value> {
            axum::Json(json!({
                "authenticated": true,
                "is_master_key": auth.is_master_key,
                "key_alias": auth.key_alias,
                "user_id": auth.user_id,
            }))
        }

        let app = Router::new()
            .route("/test-auth", get(handler))
            .with_state(Arc::clone(&state));

        (app, pool, master_key)
    }

    #[tokio::test]
    async fn test_missing_header() {
        let (app, _pool, _mk) = setup_test_app().await;
        let req = Request::builder()
            .uri("/test-auth")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_format() {
        let (app, _pool, _mk) = setup_test_app().await;
        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", "Basic dGVzdDp0ZXN0")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_master_key_auth() {
        let (app, _pool, master_key) = setup_test_app().await;
        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", master_key))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_valid_key_auth() {
        let (app, pool, _mk) = setup_test_app().await;
        let raw_token = "sk-test-valid-key";
        let token_hash = hash_token(raw_token);
        let key = make_key(&token_hash, None, None);

        // Insert the key
        pool.insert_key(&key).await.expect("insert key");

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", raw_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_blocked_key_auth() {
        let (app, pool, _mk) = setup_test_app().await;
        let raw_token = "sk-blocked-test";
        let token_hash = hash_token(raw_token);
        let key = make_key(&token_hash, Some(true), None);

        pool.insert_key(&key).await.expect("insert key");

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", raw_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_expired_key_auth() {
        let (app, pool, _mk) = setup_test_app().await;
        let raw_token = "sk-expired-test";
        let token_hash = hash_token(raw_token);
        let expiry = Utc::now() - chrono::Duration::hours(1);
        let key = make_key(&token_hash, None, Some(expiry));

        pool.insert_key(&key).await.expect("insert key");

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", raw_token))
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_invalid_key_auth() {
        let (app, _pool, _mk) = setup_test_app().await;
        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", "Bearer sk-nonexistent-key-12345")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
