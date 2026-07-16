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

/// Valid router_settings top-level keys (defense-in-depth).
const VALID_ROUTER_SETTINGS_KEYS: &[&str] = &[
    "routing_strategy", "num_retries", "allowed_fails",
    "cooldown_time", "retry_after", "fallbacks",
    "model_group_alias", "routing_groups",
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
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Validate field names
    validate_router_settings_keys(&body)?;

    // Write to config table
    state.db.upsert_config("router_settings", &body.to_string()).await.map_err(|e| {
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
    Path(token): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_router_settings_keys(&body)?;

    state.db.update_key_router_settings(&token, &body.to_string()).await.map_err(|e| {
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
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    validate_router_settings_keys(&body)?;

    state.db.update_team_router_settings(&id, &body.to_string()).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
        )
    })?;

    tracing::info!(%id, "Team router_settings updated");
    Ok(Json(body))
}
