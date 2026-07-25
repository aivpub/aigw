//! Tenant-aware auth extractor for SaaS multi-tenant deployment
//!
//! Provides `TenantAuth` — an axum extractor that authenticates Bearer tokens
//! and returns a [`TenantIdentity`] with deployment mode awareness.
//!
//! # SaaS enforcement
//!
//! In SaaS mode, keys must belong to an organization. Keys without an `organization_id`
//! are rejected. In OnPrem mode, this restriction is not applied.
//!
//! # Usage
//!
//! ```rust,ignore
//! use crate::routes::auth::TenantAuth;
//!
//! async fn handler(auth: TenantAuth) -> impl IntoResponse {
//!     if !auth.0.can_access_org("my-org") {
//!         return StatusCode::FORBIDDEN;
//!     }
//!     // ...
//! }
//! ```

use aigw_core::crypto::hash_token;
use aigw_core::middleware::auth_gateway::{DeploymentMode, TenantIdentity};
use aigw_core::middleware::{AuthError, KeyIdentity};
use axum::{
    extract::FromRequestParts,
    http::{self, request::Parts},
};
use std::str::FromStr;

use super::keys::SharedState;

/// Tenant-aware auth extractor for SaaS mode.
///
/// Wraps a [`TenantIdentity`] so handlers can check organization access.
/// Implements `FromRequestParts` to extract and validate the Bearer token,
/// then produce a `TenantIdentity` based on the deployment mode.
#[allow(dead_code)]
pub struct TenantAuth(pub TenantIdentity);

impl std::ops::Deref for TenantAuth {
    type Target = TenantIdentity;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<S> FromRequestParts<S> for TenantAuth
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let state: SharedState = axum::extract::FromRef::from_ref(state);

        // 1. Extract Authorization header
        let auth_header = parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(AuthError::MissingHeader)?;

        // 2. Extract Bearer token
        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(AuthError::InvalidFormat)?;

        if token.is_empty() {
            return Err(AuthError::InvalidFormat);
        }

        let Ok(deployment_mode) = DeploymentMode::from_str(&state.deployment_mode);

        // 3. Check master key
        if let Some(ref mk) = state.master_key {
            if token == *mk {
                let identity = KeyIdentity {
                    token_hash: "*master*".to_string(),
                    key_alias: Some("master".to_string()),
                    user_id: None,
                    team_id: None,
                    organization_id: None,
                    is_master_key: true,
                    user_role: Some("proxy_admin".to_string()),
                };
                return Ok(TenantAuth(TenantIdentity::new(identity, deployment_mode)));
            }
        }

        // 4. SHA256 hash the token and lookup in virtual_keys
        let token_hash = hash_token(token);
        let key = state
            .db
            .get_key_by_token(&token_hash)
            .await
            .map_err(|_| AuthError::TokenNotFound)?;

        match key {
            Some(k) => {
                let identity = KeyIdentity {
                    token_hash,
                    key_alias: k.key_alias,
                    user_id: k.user_id,
                    team_id: k.team_id,
                    organization_id: k.organization_id,
                    is_master_key: false,
                    user_role: None,
                };

                // In SaaS mode, verify key belongs to an organization
                if deployment_mode.is_saas() && identity.organization_id.is_none() {
                    return Err(AuthError::TokenNotFound);
                }

                Ok(TenantAuth(TenantIdentity::new(identity, deployment_mode)))
            }
            None => Err(AuthError::TokenNotFound),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Integration tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use aigw_core::db::Database;
    use aigw_core::models::VirtualKey;
    use aigw_core::provider::ProviderRegistry;
    use aigw_core::rate_limiter::RateLimiter;
    use aigw_core::router::{Router as AigwRouter, RouterState};
use aigw_core::resolver::ModelResolver;
    use axum::{body::Body, http::Request, routing::get, Json, Router};
    use serde_json::{json, Value};
    use std::sync::Arc;
    use tower::ServiceExt;

    fn make_state(db: Database, master_key: Option<String>, deployment_mode: &str) -> SharedState {
        Arc::new(super::super::keys::AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key,
            aigw_master_key: None,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: deployment_mode.to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
  otel_active: false,
            body_archiver: None,            metrics: None,
        })
    }

    fn make_key(token_hash: &str, org_id: Option<&str>) -> VirtualKey {
        VirtualKey {
            token: token_hash.to_string(),
            key_name: Some("test-key".to_string()),
            key_alias: Some("test-alias".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
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
            organization_id: org_id.map(|s| s.to_string()),
            object_permission_id: None,
            created_at: None,
            created_by: None,
            updated_at: None,
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

    async fn handler(TenantAuth(auth): TenantAuth) -> Json<Value> {
        Json(json!({
            "authenticated": true,
            "is_master_key": auth.identity.is_master_key,
            "organization_id": auth.organization_id,
            "deployment_mode": if auth.deployment_mode.is_saas() { "saas" } else { "onprem" },
        }))
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // test_tenant_auth_master_key
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_tenant_auth_master_key() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let mk = "sk-master-test".to_string();
        let state = make_state(db, Some(mk.clone()), "saas");

        let app = Router::new()
            .route("/test-auth", get(handler))
            .with_state(state);

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", mk))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["is_master_key"], true);
        assert_eq!(body["organization_id"], Value::Null);
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // test_tenant_auth_saas_requires_org
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_tenant_auth_saas_requires_org() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Insert a key WITHOUT organization_id directly through Database
        let raw_token = "sk-no-org-key";
        let token_hash = hash_token(raw_token);
        let key = make_key(&token_hash, None); // no org_id
        db.insert_key(&key).await.expect("insert");

        let state = make_state(db, None, "saas");

        let app = Router::new()
            .route("/test-auth", get(handler))
            .with_state(state);

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", raw_token))
            .body(Body::empty())
            .unwrap();

        // In SaaS mode, keys without org_id should be rejected
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // test_tenant_auth_saas_with_org_succeeds
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_tenant_auth_saas_with_org_succeeds() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Insert a key WITH organization_id directly through Database
        let raw_token = "sk-with-org-key";
        let token_hash = hash_token(raw_token);
        let key = make_key(&token_hash, Some("org-1"));
        db.insert_key(&key).await.expect("insert");

        let state = make_state(db, None, "saas");

        let app = Router::new()
            .route("/test-auth", get(handler))
            .with_state(state);

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", raw_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["organization_id"], "org-1");
        assert_eq!(body["deployment_mode"], "saas");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // test_tenant_auth_onprem_without_org_succeeds
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_tenant_auth_onprem_without_org_succeeds() {
        let db = Database::init("sqlite::memory:").await.expect("init");

        // Insert a key WITHOUT organization_id directly through Database
        let raw_token = "sk-onprem-no-org";
        let token_hash = hash_token(raw_token);
        let key = make_key(&token_hash, None);
        db.insert_key(&key).await.expect("insert");

        let state = make_state(db, None, "onprem");

        let app = Router::new()
            .route("/test-auth", get(handler))
            .with_state(state);

        let req = Request::builder()
            .uri("/test-auth")
            .header("Authorization", format!("Bearer {}", raw_token))
            .body(Body::empty())
            .unwrap();

        // In OnPrem mode, keys without org_id should still authenticate
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["deployment_mode"], "onprem");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // test_tenant_auth_missing_header
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_tenant_auth_missing_header() {
        let db = Database::init("sqlite::memory:").await.expect("init");
        let state = make_state(db, None, "onprem");

        let app = Router::new()
            .route("/test-auth", get(handler))
            .with_state(state);

        let req = Request::builder()
            .uri("/test-auth")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }
}
