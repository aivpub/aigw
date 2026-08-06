//! Team management endpoints — /team/* routes
//!
//! Endpoints:
//! - POST   /team/new    — Create a new team
//! - GET    /team/info   — Get team info by team_id
//! - GET    /team/list   — List teams (optional ?organization_id= filter)
//! - PUT    /team/update — Update a team
//! - DELETE /team/delete — Delete a team
//! - GET    /team/deleted — List deleted teams

use aigw_core::db::Database;
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

/// Validate team budget against parent organization budget (via budgets table).
async fn validate_team_budget(
    db: &Database,
    team_max_budget: f64,
    organization_id: Option<&str>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(oid) = organization_id {
        // Org delegates budget to budgets table via budget_id
        if let Ok(Some(org)) = db.get_organization_by_id(oid).await {
            if let Ok(Some(budget)) = db.get_budget_by_id(&org.budget_id).await {
                if let Some(org_max) = budget.max_budget_f64() {
                    if org_max > 0.0 && team_max_budget > org_max {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(
                                json!({"error": {"message": "Team budget cannot exceed organization budget", "type": "budget_violation"}}),
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct TeamInfoQuery {
    pub team_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamListQuery {
    pub organization_id: Option<String>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
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
        organization_id: body
            .get("organization_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        object_permission_id: body
            .get("object_permission_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        admins: body.get("admins").cloned().unwrap_or(json!([])),
        members: body.get("members").cloned().unwrap_or(json!([])),
        members_with_roles: body.get("members_with_roles").cloned().unwrap_or(json!([])),
        metadata: body.get("metadata").cloned().unwrap_or(json!({})),
        max_budget: body
            .get("max_budget")
            .and_then(|v| v.as_f64())
            .map(|v| v.to_string()),
        soft_budget: body
            .get("soft_budget")
            .and_then(|v| v.as_f64())
            .map(|v| v.to_string()),
        spend: 0.0,
        models: body.get("models").cloned().unwrap_or(json!([])),
        max_parallel_requests: body
            .get("max_parallel_requests")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string()),
        tpm_limit: body
            .get("tpm_limit")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string()),
        rpm_limit: body
            .get("rpm_limit")
            .and_then(|v| v.as_i64())
            .map(|v| v.to_string()),
        budget_duration: body
            .get("budget_duration")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        budget_reset_at: None,
        blocked: body
            .get("blocked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        created_at: now,
        updated_at: now,
        model_spend: body.get("model_spend").cloned().unwrap_or(json!({})),
        model_max_budget: body.get("model_max_budget").cloned().unwrap_or(json!({})),
        router_settings: body.get("router_settings").cloned(),
        team_member_permissions: body
            .get("team_member_permissions")
            .cloned()
            .unwrap_or(json!([])),
        access_group_ids: body.get("access_group_ids").cloned().unwrap_or(json!([])),
        policies: body.get("policies").cloned().unwrap_or(json!([])),
        default_team_member_models: body
            .get("default_team_member_models")
            .cloned()
            .unwrap_or(json!([])),
        budget_limits: body.get("budget_limits").cloned(),
        model_id: body
            .get("model_id")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32),
        allow_team_guardrail_config: body
            .get("allow_team_guardrail_config")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };

    // Validate budget hierarchy: team budget cannot exceed organization budget
    if let Some(tb) = team.max_budget_f64() {
        if tb > 0.0 {
            validate_team_budget(&state.db, tb, team.organization_id.as_deref()).await?;
        }
    }

    state.db.insert_team(&team).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(
        serde_json::to_value(&team).unwrap_or(json!({"team_id": team_id})),
    ))
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

/// GET /team/list?organization_id=... (paginated)
pub async fn team_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<TeamListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (teams, total_count) = tokio::try_join!(
        state
            .db
            .list_teams_paged(query.organization_id.as_deref(), limit, offset),
        state.db.count_teams_store(query.organization_id.as_deref()),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    let teams: Vec<Value> = teams
        .iter()
        .map(|t| {
            let mut v = serde_json::to_value(t).unwrap_or(json!({}));
            if let Some(obj) = v.as_object_mut() {
                if let Some(ref _budget_str) = t.max_budget {
                    obj["max_budget"] = json!(t.max_budget_f64());
                }
                obj["max_parallel_requests"] = json!(t.max_parallel_requests_i32());
            }
            v
        })
        .collect();
    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };
    Ok(Json(json!({
        "data": teams,
        "count": teams.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// PUT /team/update
pub async fn team_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let team_id = body.get("team_id").and_then(|v| v.as_str()).ok_or((
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
        existing.max_budget = v.as_f64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("models") {
        existing.models = v.clone();
    }
    if let Some(v) = body.get("tpm_limit") {
        existing.tpm_limit = v.as_i64().map(|v| v.to_string());
    }
    if let Some(v) = body.get("rpm_limit") {
        existing.rpm_limit = v.as_i64().map(|v| v.to_string());
    }
    if let Some(v) = body.get("blocked") {
        existing.blocked = v.as_bool().unwrap_or(existing.blocked);
    }
    if let Some(v) = body.get("budget_duration") {
        existing.budget_duration = v
            .as_str()
            .map(String::from)
            .filter(|s| !s.is_empty());
    }
    if let Some(v) = body.get("soft_budget") {
        existing.soft_budget = v.as_f64().map(|vv| vv.to_string());
    }
    existing.updated_at = chrono::Utc::now();

    // Validate budget hierarchy: team budget cannot exceed organization budget
    if let Some(tb) = existing.max_budget_f64() {
        if tb > 0.0 {
            validate_team_budget(&state.db, tb, existing.organization_id.as_deref()).await?;
        }
    }

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

/// GET /team/deleted — list deleted teams (admin, paginated)
pub async fn team_deleted_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<TeamListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (deleted, total_count) = tokio::try_join!(
        state.db.list_deleted_teams_paged(limit, offset),
        state.db.count_deleted_teams(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    let data: Vec<Value> = deleted
        .into_iter()
        .map(|t| serde_json::to_value(t).unwrap_or(json!({})))
        .collect();
    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };
    Ok(Json(json!({
        "data": data,
        "count": data.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}
