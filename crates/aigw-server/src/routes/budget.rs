//! Budget management endpoints — /budget/* routes
//!
//! Endpoints:
//! - GET    /budget/list   — List all budgets (paginated)
//! - POST   /budget/new    — Create a new budget
//! - GET    /budget/info   — Get budget info by budget_id
//! - POST   /budget/update — Update a budget
//! - POST   /budget/delete — Delete a budget

use aigw_core::models::Budget;
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
pub struct BudgetInfoQuery {
    pub budget_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BudgetListQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct BudgetDeleteBody {
    pub budget_id: Option<String>,
}

/// GET /budget/list?page=...&page_size=...
pub async fn budget_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<BudgetListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (budgets, total_count) = tokio::try_join!(
        state.db.list_budgets_paged(limit, offset),
        state.db.count_budgets(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    let data: Vec<Value> = budgets
        .iter()
        .map(|b| serde_json::to_value(b).unwrap_or(json!({})))
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

/// POST /budget/new
pub async fn budget_new(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let budget_id = body
        .get("budget_id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    let now = chrono::Utc::now();

    let budget = Budget {
        budget_id: budget_id.clone(),
        max_budget: body
            .get("max_budget")
            .and_then(|v| v.as_f64())
            .map(|v| v.to_string()),
        soft_budget: body
            .get("soft_budget")
            .and_then(|v| v.as_f64())
            .map(|v| v.to_string()),
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
        model_max_budget: body.get("model_max_budget").cloned().unwrap_or(json!({})),
        budget_duration: body
            .get("budget_duration")
            .and_then(|v| v.as_str())
            .map(String::from),
        budget_reset_at: body
            .get("budget_reset_at")
            .and_then(|v| v.as_str())
            .and_then(super::keys::parse_rfc3339),
        allowed_models: body.get("allowed_models").cloned().unwrap_or(json!([])),
        created_at: now,
        created_by: auth.key_alias.clone().unwrap_or_default(),
        updated_at: now,
        updated_by: auth.key_alias.clone().unwrap_or_default(),
    };

    state.db.insert_budget(&budget).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(
        serde_json::to_value(&budget).unwrap_or(json!({"budget_id": budget_id})),
    ))
}

/// GET /budget/info?budget_id=...
pub async fn budget_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    axum::extract::Query(query): axum::extract::Query<BudgetInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let budget_id = query.budget_id.ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": "budget_id is required", "type": "bad_request"}})),
    ))?;

    let budget = state
        .db
        .get_budget_by_id(&budget_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Budget not found", "type": "not_found"}})),
        ))?;

    Ok(Json(serde_json::to_value(&budget).unwrap_or(json!({}))))
}

/// POST /budget/update
pub async fn budget_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let budget_id = body.get("budget_id").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": "budget_id is required", "type": "bad_request"}})),
    ))?;

    let mut existing = state
        .db
        .get_budget_by_id(budget_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "Budget not found", "type": "not_found"}})),
        ))?;

    if let Some(v) = body.get("max_budget") {
        existing.max_budget = v.as_f64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("soft_budget") {
        existing.soft_budget = v.as_f64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("max_parallel_requests") {
        existing.max_parallel_requests = v.as_i64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("tpm_limit") {
        existing.tpm_limit = v.as_i64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("rpm_limit") {
        existing.rpm_limit = v.as_i64().map(|vv| vv.to_string());
    }
    if let Some(v) = body.get("model_max_budget") {
        existing.model_max_budget = v.clone();
    }
    if let Some(v) = body.get("budget_duration").and_then(|v| v.as_str()) {
        existing.budget_duration = Some(v.to_string());
    }
    if let Some(v) = body
        .get("budget_reset_at")
        .and_then(|v| v.as_str())
        .and_then(super::keys::parse_rfc3339)
    {
        existing.budget_reset_at = Some(v);
    }
    if let Some(v) = body.get("allowed_models") {
        existing.allowed_models = v.clone();
    }
    existing.updated_at = chrono::Utc::now();
    existing.updated_by = auth.key_alias.clone().unwrap_or_default();

    state.db.update_budget(&existing).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(serde_json::to_value(&existing).unwrap_or(json!({}))))
}

/// POST /budget/delete
pub async fn budget_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<BudgetDeleteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    let budget_id = body.budget_id.ok_or((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": "budget_id is required", "type": "bad_request"}})),
    ))?;

    state.db.delete_budget(&budget_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "internal"}})),
        )
    })?;

    Ok(Json(json!({"message": "Budget deleted"})))
}
