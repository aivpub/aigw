//! Team management endpoints — /team/* routes
//!
//! Endpoints:
//! - POST   /team/new    — Create a new team
//! - GET    /team/info   — Get team info by team_id
//! - GET    /team/list   — List teams (optional ?organization_id= filter)
//! - PUT    /team/update — Update a team
//! - DELETE /team/delete — Delete a team

use aigw_core::models::Team;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::keys::SharedState;
use super::spend::{require_admin, SpendAuth};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct TeamInfoQuery {
    pub team_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamListQuery {
    pub organization_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamDeleteQuery {
    pub team_id: String,
}

/// POST /team/new
pub async fn team_new(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let team_id = body
        .get("team_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    let now = chrono::Utc::now();

    let team = Team {
        team_id: team_id.clone(),
        team_alias: body
            .get("team_alias")
            .and_then(|v| v.as_str())
            .map(String::from),
        organization_id: body.get("organization_id").and_then(|v| v.as_str()).map(String::from),
        object_permission_id: body.get("object_permission_id").and_then(|v| v.as_str()).map(String::from),
        admins: body.get("admins").cloned().unwrap_or(json!([])),
        members: body.get("members").cloned().unwrap_or(json!([])),
        members_with_roles: body.get("members_with_roles").cloned().unwrap_or(json!([])),
        metadata: body.get("metadata").cloned().unwrap_or(json!({})),
        max_budget: body.get("max_budget").and_then(|v| v.as_f64()),
        soft_budget: body.get("soft_budget").and_then(|v| v.as_f64()),
        spend: 0.0,
        models: body.get("models").cloned().unwrap_or(json!([])),
        max_parallel_requests: body.get("max_parallel_requests").and_then(|v| v.as_i64()).map(|v| v as i32),
        tpm_limit: body.get("tpm_limit").and_then(|v| v.as_i64()),
        rpm_limit: body.get("rpm_limit").and_then(|v| v.as_i64()),
        budget_duration: body.get("budget_duration").and_then(|v| v.as_str()).map(String::from),
        budget_reset_at: None,
        blocked: body.get("blocked").and_then(|v| v.as_bool()).unwrap_or(false),
        created_at: now,
        updated_at: now,
        model_spend: body.get("model_spend").cloned().unwrap_or(json!({})),
        model_max_budget: body.get("model_max_budget").cloned().unwrap_or(json!({})),
        router_settings: body.get("router_settings").cloned(),
        team_member_permissions: body.get("team_member_permissions").cloned().unwrap_or(json!([])),
        access_group_ids: body.get("access_group_ids").cloned().unwrap_or(json!([])),
        policies: body.get("policies").cloned().unwrap_or(json!([])),
        default_team_member_models: body.get("default_team_member_models").cloned().unwrap_or(json!([])),
        budget_limits: body.get("budget_limits").cloned(),
        model_id: body.get("model_id").and_then(|v| v.as_i64()).map(|v| v as i32),
        allow_team_guardrail_config: body.get("allow_team_guardrail_config").and_then(|v| v.as_bool()).unwrap_or(false),
    };

    state.db.insert_team(&team).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&team).unwrap_or(json!({"team_id": team_id}))))
}

/// GET /team/info?team_id=...
pub async fn team_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<TeamInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let team_id = query.team_id.ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": "team_id is required", "type": "bad_request"}})),
    ))?;

    let team = state
        .db
        .get_team_by_id(&team_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Team not found", "type": "not_found"}})),
        ))?;

    Ok(Json(serde_json::to_value(&team).unwrap_or(json!({}))))
}

/// GET /team/list?organization_id=...
pub async fn team_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<TeamListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let teams = state.db.list_teams(query.organization_id.as_deref()).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&teams).unwrap_or(json!([]))))
}

/// PUT /team/update
pub async fn team_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let team_id = body
        .get("team_id")
        .and_then(|v| v.as_str())
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "team_id is required", "type": "bad_request"}})),
        ))?;

    let mut existing = state
        .db
        .get_team_by_id(team_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Team not found", "type": "not_found"}})),
        ))?;

    if let Some(v) = body.get("team_alias").and_then(|v| v.as_str()) {
        existing.team_alias = Some(v.to_string());
    }
    if let Some(v) = body.get("organization_id") {
        existing.organization_id = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("admins") {
        existing.admins = v.clone();
    }
    if let Some(v) = body.get("members") {
        existing.members = v.clone();
    }
    if let Some(v) = body.get("metadata") {
        existing.metadata = v.clone();
    }
    if let Some(v) = body.get("max_budget") {
        existing.max_budget = v.as_f64();
    }
    if let Some(v) = body.get("models") {
        existing.models = v.clone();
    }
    if let Some(v) = body.get("tpm_limit") {
        existing.tpm_limit = v.as_i64();
    }
    if let Some(v) = body.get("rpm_limit") {
        existing.rpm_limit = v.as_i64();
    }
    if let Some(v) = body.get("blocked") {
        existing.blocked = v.as_bool().unwrap_or(existing.blocked);
    }
    existing.updated_at = chrono::Utc::now();

    state.db.update_team(&existing).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&existing).unwrap_or(json!({}))))
}

/// DELETE /team/delete
pub async fn team_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<TeamDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    state.db.delete_team(&query.team_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(json!({"message": "Team deleted"})))
}
