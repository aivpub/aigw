//! Organization management endpoints — /org/* routes
//!
//! Endpoints:
//! - POST   /org/new    — Create a new organization
//! - GET    /org/info   — Get organization info by organization_id
//! - GET    /org/list   — List all organizations
//! - PUT    /org/update — Update an organization
//! - DELETE /org/delete — Delete an organization

use aigw_core::models::Organization;
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
pub struct OrgInfoQuery {
    pub organization_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrgDeleteQuery {
    pub organization_id: String,
}

/// POST /org/new
pub async fn org_new(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let org_id = body
        .get("organization_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    let now = chrono::Utc::now();

    let org = Organization {
        organization_id: org_id.clone(),
        organization_alias: body
            .get("organization_alias")
            .and_then(|v| v.as_str())
            .unwrap_or(&org_id)
            .to_string(),
        budget_id: body
            .get("budget_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string(),
        metadata: body.get("metadata").cloned().unwrap_or(json!({})),
        models: body.get("models").cloned().unwrap_or(json!([])),
        spend: 0.0,
        model_spend: body.get("model_spend").cloned().unwrap_or(json!({})),
        object_permission_id: body
            .get("object_permission_id")
            .and_then(|v| v.as_str())
            .map(String::from),
        created_at: now,
        created_by: auth.key_alias.clone().unwrap_or_default(),
        updated_at: now,
        updated_by: auth.key_alias.unwrap_or_default(),
    };

    state
        .db
        .insert_organization(&org)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?;

    Ok(Json(serde_json::to_value(&org).unwrap_or(json!({"organization_id": org_id}))))
}

/// GET /org/info?organization_id=...
pub async fn org_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<OrgInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let org_id = query.organization_id.ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": "organization_id is required", "type": "bad_request"}})),
    ))?;

    let org = state
        .db
        .get_organization_by_id(&org_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Organization not found", "type": "not_found"}})),
        ))?;

    Ok(Json(serde_json::to_value(&org).unwrap_or(json!({}))))
}

/// GET /org/list
pub async fn org_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let orgs = state.db.list_organizations().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&orgs).unwrap_or(json!([]))))
}

/// PUT /org/update
pub async fn org_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let org_id = body
        .get("organization_id")
        .and_then(|v| v.as_str())
        .ok_or((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "organization_id is required", "type": "bad_request"}})),
        ))?;

    let mut existing = state
        .db
        .get_organization_by_id(org_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Organization not found", "type": "not_found"}})),
        ))?;

    if let Some(v) = body.get("organization_alias").and_then(|v| v.as_str()) {
        existing.organization_alias = v.to_string();
    }
    if let Some(v) = body.get("budget_id").and_then(|v| v.as_str()) {
        existing.budget_id = v.to_string();
    }
    if let Some(v) = body.get("metadata") {
        existing.metadata = v.clone();
    }
    if let Some(v) = body.get("models") {
        existing.models = v.clone();
    }
    if let Some(v) = body.get("model_spend") {
        existing.model_spend = v.clone();
    }
    if let Some(v) = body.get("object_permission_id") {
        existing.object_permission_id = v.as_str().map(String::from);
    }
    existing.updated_at = chrono::Utc::now();
    existing.updated_by = auth.key_alias.unwrap_or_default();

    state
        .db
        .update_organization(&existing)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?;

    Ok(Json(serde_json::to_value(&existing).unwrap_or(json!({}))))
}

/// DELETE /org/delete
pub async fn org_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<OrgDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    state
        .db
        .delete_organization(&query.organization_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?;

    Ok(Json(json!({"message": "Organization deleted"})))
}
