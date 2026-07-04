//! Model management endpoints — litellm-compatible /model/* routes
//!
//! Endpoints:
//! - POST   /model/new      — Create new proxy model
//! - GET    /model/info      — Get model info by ID
//! - GET    /model/list      — List all proxy models
//! - PUT    /model/update    — Update existing model
//! - DELETE /model/delete    — Delete a model

use aigw_core::models::ProxyModel;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::routes::keys::SharedState;
use super::spend::{require_admin, SpendAuth};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request/Response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct ModelInfoQuery {
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ModelDeleteQuery {
    pub model_id: String,
}

#[derive(Debug, Deserialize)]
pub struct NewModelBody {
    pub model_name: String,
    pub litellm_params: Value,
    #[serde(default)]
    pub model_info: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelBody {
    pub model_id: String,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub litellm_params: Option<Value>,
    #[serde(default)]
    pub model_info: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ModelResponse {
    pub model_id: String,
    pub model_name: String,
    pub litellm_params: Value,
    pub model_info: Value,
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

impl From<ProxyModel> for ModelResponse {
    fn from(m: ProxyModel) -> Self {
        Self {
            model_id: m.model_id,
            model_name: m.model_name,
            litellm_params: m.litellm_params,
            model_info: m.model_info,
            created_at: m.created_at,
            created_by: m.created_by,
            updated_at: m.updated_at,
            updated_by: m.updated_by,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handlers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /model/new — create a new proxy model (admin)
pub async fn model_new(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<NewModelBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let now = chrono::Utc::now().to_rfc3339();
    let model_id = uuid::Uuid::new_v4().to_string();

    let model = ProxyModel {
        model_id: model_id.clone(),
        model_name: body.model_name.clone(),
        litellm_params: body.litellm_params.clone(),
        model_info: body.model_info.clone().unwrap_or(json!({})),
        created_at: now.clone(),
        created_by: None,
        updated_at: now,
        updated_by: None,
    };

    state.db.insert_model(&model).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let resp = ModelResponse::from(model);
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// GET /model/info?model_id=... — get model details (admin)
pub async fn model_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(q): Query<ModelInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let model = state.db.get_model_by_id(&q.model_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    match model {
        Some(m) => {
            let resp = ModelResponse::from(m);
            Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "model not found"}})),
        )),
    }
}

/// GET /model/list — list all proxy models (admin)
pub async fn model_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let models = state.db.list_models().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let data: Vec<Value> = models
        .into_iter()
        .map(|m| serde_json::to_value(ModelResponse::from(m)).unwrap_or(json!({})))
        .collect();

    Ok(Json(json!({"object": "list", "data": data})))
}

/// PUT /model/update — update an existing model (admin)
pub async fn model_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<UpdateModelBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let existing = state.db.get_model_by_id(&body.model_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let mut model = existing.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "model not found"}})),
        )
    })?;

    if let Some(ref name) = body.model_name {
        model.model_name = name.clone();
    }
    if let Some(ref params) = body.litellm_params {
        model.litellm_params = params.clone();
    }
    if let Some(ref info) = body.model_info {
        model.model_info = info.clone();
    }
    model.updated_at = chrono::Utc::now().to_rfc3339();

    state.db.update_model(&model).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let resp = ModelResponse::from(model);
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// DELETE /model/delete?model_id=... — delete a model (admin)
pub async fn model_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(q): Query<ModelDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let existing = state.db.get_model_by_id(&q.model_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    if existing.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "model not found"}})),
        ));
    }

    state.db.delete_model(&q.model_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    Ok(Json(json!({"status": "deleted"})))
}
