//! Model management endpoints — litellm-compatible /model/* routes
//!
//! Endpoints:
//! - POST   /model/new      — Create new proxy model
//! - GET    /model/info      — Get model info by ID
//! - GET    /model/list      — List all proxy models
//! - PUT    /model/update    — Update existing model
//! - DELETE /model/delete    — Delete a model

use aigw_core::crypto::{decrypt_json_fields, decrypt_litellm_value};
use aigw_core::models::ProxyModel;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

use super::spend::{require_admin, SpendAuth};
use crate::routes::keys::SharedState;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request/Response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct ModelListQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

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
pub struct UpdateModelQuery {
    #[serde(default)]
    pub model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelBody {
    // model_id is optional in body — it can come from query param instead
    // (litellm upstream sends it in query, not body)
    #[serde(default)]
    pub model_id: Option<String>,
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

impl ModelResponse {
    /// Build a ModelResponse, decrypting `litellm_params` if needed.
    ///
    /// If `master_key` is provided and `litellm_params` appears encrypted (not starting with `{`),
    /// it will be decrypted and parsed as JSON.
    fn from_model(m: ProxyModel, master_key: Option<&str>) -> Self {
        let litellm_params = Self::decrypt_params(&m.litellm_params, master_key);
        Self {
            model_id: m.model_id,
            model_name: m.model_name,
            litellm_params,
            model_info: m.model_info,
            created_at: m.created_at,
            created_by: m.created_by,
            updated_at: m.updated_at,
            updated_by: m.updated_by,
        }
    }

    fn decrypt_params(params: &Value, master_key: Option<&str>) -> Value {
        let key = match master_key {
            Some(k) => k,
            None => {
                warn!("AIGW_MASTER_KEY not configured — returning encrypted litellm_params as-is");
                return params.clone();
            }
        };
        match params {
            Value::Object(_) => {
                // Walk each field: individually encrypted values like api_base,
                // api_key, custom_llm_provider live inside the JSON object.
                decrypt_json_fields(params, key)
            }
            Value::String(s) => {
                if s.starts_with('{') {
                    // Plaintext JSON string — parse and then walk fields for
                    // individually encrypted values inside.
                    let parsed: Value = serde_json::from_str(s).unwrap_or_else(|_| params.clone());
                    return decrypt_json_fields(&parsed, key);
                }
                // Encrypted blob (e.g. legacy whole-value encryption) — decrypt
                // outer layer first, then walk fields for individually encrypted
                // values inside.
                let decrypted = match decrypt_litellm_value(s, key) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("Failed to decrypt litellm_params: {} — returning encrypted value as-is", e);
                        return params.clone();
                    }
                };
                let parsed: Value =
                    serde_json::from_str(&decrypted).unwrap_or_else(|_| params.clone());
                decrypt_json_fields(&parsed, key)
            }
            _ => params.clone(),
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

    let resp = ModelResponse::from_model(model, state.aigw_master_key.as_deref());
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
            let resp = ModelResponse::from_model(m, state.aigw_master_key.as_deref());
            Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "model not found"}})),
        )),
    }
}

/// GET /model/list — list all proxy models (admin, paginated)
pub async fn model_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<ModelListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (models, total_count) = tokio::try_join!(
        state.db.list_models_paged(limit, offset),
        state.db.count_models(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let mk = state.aigw_master_key.as_deref();
    let data: Vec<Value> = models
        .into_iter()
        .map(|m| serde_json::to_value(ModelResponse::from_model(m, mk)).unwrap_or(json!({})))
        .collect();

    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    Ok(Json(json!({
        "object": "list",
        "data": data,
        "count": data.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

/// PUT /model/update — update an existing model (admin)
///
/// model_id can be passed as query param (litellm convention: PUT /model/update?model_id=...)
/// or in the JSON body field `model_id`.
pub async fn model_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(q): Query<UpdateModelQuery>,
    Json(body): Json<UpdateModelBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let model_id = body
        .model_id
        .as_deref()
        .or(q.model_id.as_deref())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "model_id required in query or body"}})),
            )
        })?;
    let existing = state.db.get_model_by_id(model_id).await.map_err(|e| {
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

    let resp = ModelResponse::from_model(model, state.aigw_master_key.as_deref());
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// GET /model/deleted — list deleted models (admin, paginated)
pub async fn model_deleted_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<ModelListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).max(1).min(100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (deleted, total_count) = tokio::try_join!(
        state.db.list_deleted_models_paged(limit, offset),
        state.db.count_deleted_models(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let data: Vec<Value> = deleted
        .into_iter()
        .map(|m| serde_json::to_value(m).unwrap_or(json!({})))
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    fn make_encrypted(value: &str) -> String {
        aigw_core::crypto::encrypt_litellm_value(value, "test-master-key").unwrap()
    }

    // ━━━━ decrypt_params — whole-value encrypted ━━━━

    #[test]
    fn decrypt_whole_object_encrypted_string() {
        let plain = r#"{"model":"gpt-4","api_base":"https://api.openai.com"}"#;
        let encrypted = make_encrypted(plain);
        let result = ModelResponse::decrypt_params(&json!(encrypted), Some("test-master-key"));
        assert_eq!(result["model"], json!("gpt-4"));
        assert_eq!(result["api_base"], json!("https://api.openai.com"));
    }

    #[test]
    fn decrypt_already_plain_json_passes_through() {
        let params = json!({"model": "gpt-4", "api_base": "https://api.openai.com"});
        let result = ModelResponse::decrypt_params(&params, Some("test-master-key"));
        assert_eq!(result, params);
    }

    #[test]
    fn decrypt_no_master_key_returns_as_is() {
        let params = json!("some-encrypted-blob");
        let result = ModelResponse::decrypt_params(&params, None);
        assert_eq!(result, params);
    }

    // ━━━━ decrypt_params — nested field decryption ━━━━

    #[test]
    fn decrypt_nested_encrypted_fields() {
        let plain_api_base = "https://api.openai.com";
        let plain_api_key = "sk-secret-key";
        let params = json!({
            "model": "gpt-4",
            "api_base": make_encrypted(plain_api_base),
            "api_key": make_encrypted(plain_api_key),
            "custom_llm_provider": "openai",
        });
        let result = ModelResponse::decrypt_params(&params, Some("test-master-key"));
        assert_eq!(result["model"], json!("gpt-4"));
        assert_eq!(result["api_base"], json!(plain_api_base));
        assert_eq!(result["api_key"], json!("sk-secret-key"));
        assert_eq!(result["custom_llm_provider"], json!("openai"));
    }

    #[test]
    fn decrypt_nested_plaintext_values_untouched() {
        let params = json!({
            "model": "gpt-4",
            "rpm": 100,
            "tpm": 2000,
            "api_base": "https://plain.example.com",
        });
        let result = ModelResponse::decrypt_params(&params, Some("test-master-key"));
        assert_eq!(result["model"], json!("gpt-4"));
        assert_eq!(result["rpm"], json!(100));
        assert_eq!(result["tpm"], json!(2000));
        assert_eq!(result["api_base"], json!("https://plain.example.com"));
    }

    #[test]
    fn decrypt_nested_empty_string_passes_through() {
        let params = json!({"model": "gpt-4", "api_base": ""});
        let result = ModelResponse::decrypt_params(&params, Some("test-master-key"));
        assert_eq!(result["api_base"], json!(""));
    }

    #[test]
    fn decrypt_nested_recursive_in_object() {
        let plain_deployment = "us-east-1";
        let params = json!({
            "model": "bedrock",
            "litellm_params": {
                "deployment": make_encrypted(plain_deployment),
                "region": "us-east-1",
            },
        });
        let result = ModelResponse::decrypt_params(&params, Some("test-master-key"));
        assert_eq!(result["model"], json!("bedrock"));
        assert_eq!(
            result["litellm_params"]["deployment"],
            json!(plain_deployment)
        );
        assert_eq!(result["litellm_params"]["region"], json!("us-east-1"));
    }

    #[test]
    fn decrypt_nested_in_arrays() {
        let plain1 = "model-a";
        let plain2 = "model-b";
        let params = json!({
            "fallbacks": [
                make_encrypted(plain1),
                make_encrypted(plain2),
            ],
        });
        let result = ModelResponse::decrypt_params(&params, Some("test-master-key"));
        assert_eq!(result["fallbacks"][0], json!(plain1));
        assert_eq!(result["fallbacks"][1], json!(plain2));
    }

    #[test]
    fn decrypt_nested_no_master_key_returns_as_is() {
        let params = json!({
            "model": "gpt-4",
            "api_base": make_encrypted("secret"),
        });
        let result = ModelResponse::decrypt_params(&params, None);
        // All values unchanged — master_key missing
        assert_eq!(result, params);
    }
}
