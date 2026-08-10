//! Router settings endpoints — Phase 23
//!
//! Three-tier configuration (Key > Team > Global) for routing strategy,
//! retry, cooldown, and failure thresholds.
//!
//! Endpoints:
//! - GET  /router/settings             — Read global router_settings from config table
//! - PUT  /router/settings             — Write global router_settings (hot reload)
//! - PATCH /key/{token}/router/settings  — Write key-level router_settings
//! - PATCH /team/{id}/router/settings    — Write team-level router_settings

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use super::keys::SharedState;
use super::spend::{require_admin, SpendAuth};

/// Valid router_settings top-level keys (defense-in-depth).
const VALID_ROUTER_SETTINGS_KEYS: &[&str] = &[
    "routing_strategy",
    "num_retries",
    "allowed_fails",
    "cooldown_time",
    "retry_after",
    "fallbacks",
    "model_group_alias",
    "routing_groups",
];

fn validate_router_settings_keys(body: &Value) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(obj) = body.as_object() {
        for key in obj.keys() {
            if !VALID_ROUTER_SETTINGS_KEYS.contains(&key.as_str()) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "message": format!(
                                "Invalid router_settings key: '{}'. Valid keys: {:?}",
                                key, VALID_ROUTER_SETTINGS_KEYS
                            ),
                            "type": "invalid_request_error"
                        }
                    })),
                ));
            }
        }
    }
    Ok(())
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// GET /router/settings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn get_global(
    State(state): State<SharedState>,
    SpendAuth(_auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let val = state.db.get_config("router_settings").await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
        )
    })?;

    match val {
        Some(json_str) => {
            let parsed: Value = serde_json::from_str(&json_str).unwrap_or(json!({}));
            Ok(Json(parsed))
        }
        None => Ok(Json(json!({}))),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PUT /router/settings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn put_global(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    // Validate field names
    validate_router_settings_keys(&body)?;

    // Write to config table
    state
        .db
        .upsert_config("router_settings", &body.to_string())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
            )
        })?;

    // Hot reload — update the in-memory Router (Router is Clone but not wrapped in Arc<Mutex<>>,
    // so this is a best-effort log. Full hot reload requires Arc<Mutex<Router>> in AppState.)
    // For now: log + rely on server restart for global config changes.
    tracing::info!(settings=%body, "Router settings updated (hot reload — restart recommended for global changes)");

    // Try to build a RouterConfig from the input and log if it parses
    use aigw_core::router::RouterConfig;
    if let Ok(new_config) = serde_json::from_value::<RouterConfig>(body.clone()) {
        tracing::info!(
            strategy=%new_config.routing_strategy,
            num_retries=%new_config.num_retries,
            allowed_fails=%new_config.allowed_fails,
            cooldown_time=%new_config.cooldown_time,
            "Router config parsed successfully"
        );
    } else {
        tracing::warn!("Router settings saved but failed to parse as RouterConfig");
    }

    Ok(Json(body))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PATCH /key/{token}/router/settings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn patch_key(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(token): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    validate_router_settings_keys(&body)?;

    state
        .db
        .update_key_router_settings(&token, &body.to_string())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
            )
        })?;

    tracing::info!(%token, "Key router_settings updated");
    Ok(Json(body))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// PATCH /team/{id}/router/settings
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn patch_team(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    validate_router_settings_keys(&body)?;

    state
        .db
        .update_team_router_settings(&id, &body.to_string())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
            )
        })?;

    tracing::info!(%id, "Team router_settings updated");
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::AppState;
    use crate::routes::keys::DEFAULT_KEY_TOKEN_LEN;
    use aigw_core::db::Database;
    use aigw_core::provider::ProviderRegistry;
    use aigw_core::rate_limiter::RateLimiter;
    use aigw_core::resolver::ModelResolver;
    use aigw_core::router::{Router as AigwRouter, RouterState};
    use axum::{
        body::Body,
        http::{header, Method, Request},
        Router,
    };
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_app() -> Router {
        let db = Database::init("sqlite::memory:")
            .await
            .expect("init sqlite");
        let state = Arc::new(AppState {
            resolver: ModelResolver::new(db.clone(), None, "onprem"),
            router: AigwRouter::default(),
            db,
            master_key: Some("sk-master-test-123".to_string()),
            aigw_master_key: None,
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: ProviderRegistry::new(),
            router_state: RouterState::default(),
            rate_limiter: Arc::new(RateLimiter::new()),
            deployment_mode: "onprem".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
        });
        Router::new()
            .route(
                "/router/settings",
                axum::routing::get(get_global).put(put_global),
            )
            .route(
                "/key/{token}/router/settings",
                axum::routing::patch(patch_key),
            )
            .route(
                "/team/{id}/router/settings",
                axum::routing::patch(patch_team),
            )
            .with_state(state)
    }

    #[tokio::test]
    async fn get_global_no_auth_returns_401() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/router/settings")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn put_global_no_auth_returns_401() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::PUT)
            .uri("/router/settings")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"routing_strategy":"least_latency"}"#))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn patch_key_no_auth_returns_401() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::PATCH)
            .uri("/key/tk-test/router/settings")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"num_retries":3}"#))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn patch_team_no_auth_returns_401() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::PATCH)
            .uri("/team/team-1/router/settings")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"cooldown_time":60}"#))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_global_bad_token_returns_401() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/router/settings")
            .header(header::AUTHORIZATION, "Bearer sk-bad-token")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_global_master_key_returns_200() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::GET)
            .uri("/router/settings")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn put_global_master_key_returns_200() {
        let app = test_app().await;
        let request = Request::builder()
            .method(Method::PUT)
            .uri("/router/settings")
            .header(header::AUTHORIZATION, "Bearer sk-master-test-123")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"routing_strategy":"least_latency","num_retries":2}"#,
            ))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
