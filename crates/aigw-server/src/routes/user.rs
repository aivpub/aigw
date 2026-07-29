//! User management endpoints — /user/* routes
//!
//! Endpoints:
//! - POST   /user/new    — Create a new user
//! - GET    /user/info   — Get user info by user_id
//! - GET    /user/list   — List users (optional ?organization_id= filter)
//! - PUT    /user/update — Update a user
//! - DELETE /user/delete — Delete a user
//! - GET    /user/deleted — List deleted users

use aigw_core::models::User;
use aigw_core::models::DeletedUser;
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
pub struct UserInfoQuery {
    pub user_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserListQuery {
    pub organization_id: Option<String>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UserDeleteQuery {
    pub user_id: String,
}

/// POST /user/new
pub async fn user_new(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let user_id = body
        .get("user_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    let now = chrono::Utc::now();

    let user = User {
        user_id: user_id.clone(),
        user_alias: body
            .get("user_alias")
            .and_then(|v| v.as_str())
            .map(String::from),
        team_id: body.get("team_id").and_then(|v| v.as_str()).map(String::from),
        sso_user_id: body.get("sso_user_id").and_then(|v| v.as_str()).map(String::from),
        organization_id: body.get("organization_id").and_then(|v| v.as_str()).map(String::from),
        object_permission_id: body.get("object_permission_id").and_then(|v| v.as_str()).map(String::from),
        password: body.get("password").and_then(|v| v.as_str()).map(String::from),
        teams: body.get("teams").cloned().unwrap_or(json!([])),
        user_role: body.get("user_role").and_then(|v| v.as_str()).map(String::from),
        max_budget: body.get("max_budget").and_then(|v| v.as_f64()).map(|v| v.to_string()),
        spend: 0.0,
        user_email: body.get("user_email").and_then(|v| v.as_str()).map(String::from),
        models: body.get("models").cloned().unwrap_or(json!([])),
        metadata: body.get("metadata").cloned().unwrap_or(json!({})),
        max_parallel_requests: body.get("max_parallel_requests").and_then(|v| v.as_i64()).map(|v| v.to_string()),
        tpm_limit: body.get("tpm_limit").and_then(|v| v.as_i64()).map(|v| v.to_string()),
        rpm_limit: body.get("rpm_limit").and_then(|v| v.as_i64()).map(|v| v.to_string()),
        budget_duration: body.get("budget_duration").and_then(|v| v.as_str()).map(String::from),
        budget_reset_at: None,
        allowed_cache_controls: body.get("allowed_cache_controls").cloned().unwrap_or(json!([])),
        policies: body.get("policies").cloned().unwrap_or(json!([])),
        model_spend: body.get("model_spend").cloned().unwrap_or(json!({})),
        model_max_budget: body.get("model_max_budget").cloned().unwrap_or(json!({})),
        virtual_keys_count: None,
        created_at: Some(now),
        updated_at: Some(now),
    };

    state.db.insert_user(&user).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&user).unwrap_or(json!({"user_id": user_id}))))
}

/// GET /user/info?user_id=...
pub async fn user_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<UserInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let user_id = query.user_id.ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": "user_id is required", "type": "bad_request"}})),
    ))?;

    let user = state
        .db
        .get_user_by_id(&user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "User not found", "type": "not_found"}})),
        ))?;

    Ok(Json(serde_json::to_value(&user).unwrap_or(json!({}))))
}

/// GET /user/list?organization_id=...&page=...&page_size=...
pub async fn user_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<UserListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(10).max(1).min(100);

    let all_users = state.db.list_users(query.organization_id.as_deref()).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    let total_count = all_users.len() as i64;
    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    let offset = (page - 1) as usize * page_size as usize;
    let paged: Vec<&User> = all_users
        .iter()
        .skip(offset)
        .take(page_size as usize)
        .collect();

    let users: Vec<Value> = paged
        .iter()
        .map(|u| {
            let mut v = serde_json::to_value(u).unwrap_or(json!({}));
            if let Some(obj) = v.as_object_mut() {
                if let Some(ref _budget_str) = u.max_budget {
                    obj["max_budget"] = json!(u.max_budget_f64());
                }
                obj["max_parallel_requests"] = json!(u.max_parallel_requests_i32());
            }
            v
        })
        .collect();

    Ok(Json(json!({
        "data": users,
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// PUT /user/update
pub async fn user_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let user_id = body
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "user_id is required", "type": "bad_request"}})),
        ))?;

    let mut existing = state
        .db
        .get_user_by_id(user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "User not found", "type": "not_found"}})),
        ))?;

    if let Some(v) = body.get("user_alias").and_then(|v| v.as_str()) {
        existing.user_alias = Some(v.to_string());
    }
    if let Some(v) = body.get("team_id") {
        existing.team_id = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("sso_user_id") {
        existing.sso_user_id = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("organization_id") {
        existing.organization_id = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("object_permission_id") {
        existing.object_permission_id = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("password") {
        existing.password = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("teams") {
        existing.teams = v.clone();
    }
    if let Some(v) = body.get("user_role") {
        existing.user_role = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("user_email") {
        existing.user_email = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("max_budget") {
        existing.max_budget = v.as_f64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("models") {
        existing.models = v.clone();
    }
    if let Some(v) = body.get("metadata") {
        existing.metadata = v.clone();
    }
    if let Some(v) = body.get("tpm_limit") {
        existing.tpm_limit = v.as_i64().map(|v| v.to_string());
    }
    if let Some(v) = body.get("rpm_limit") {
        existing.rpm_limit = v.as_i64().map(|v| v.to_string());
    }
    if let Some(v) = body.get("max_parallel_requests") {
        existing.max_parallel_requests = v.as_i64().map(|v| v.to_string());
    }
    if let Some(v) = body.get("budget_duration") {
        existing.budget_duration = v.as_str().map(String::from);
    }
    if let Some(v) = body.get("allowed_cache_controls") {
        existing.allowed_cache_controls = v.clone();
    }
    if let Some(v) = body.get("policies") {
        existing.policies = v.clone();
    }
    if let Some(v) = body.get("model_spend") {
        existing.model_spend = v.clone();
    }
    if let Some(v) = body.get("model_max_budget") {
        existing.model_max_budget = v.clone();
    }
    existing.updated_at = Some(chrono::Utc::now());

    state.db.update_user(&existing).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&existing).unwrap_or(json!({}))))
}

/// GET /user/deleted — list deleted users (admin)
pub async fn user_deleted_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Vec<DeletedUser>>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let deleted = state.db.list_deleted_users().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;
    Ok(Json(deleted))
}

/// DELETE /user/delete
pub async fn user_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<UserDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    state.db.delete_user(&query.user_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(json!({"message": "User deleted"})))
}
