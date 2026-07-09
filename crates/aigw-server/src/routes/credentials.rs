//! Credential management endpoints — litellm-compatible /credential/* routes
//!
//! Endpoints:
//! - POST   /credential/new      — Create new credential
//! - GET    /credential/info      — Get credential by name
//! - GET    /credential/list      — List all credentials
//! - PUT    /credential/update    — Update existing credential
//! - DELETE /credential/delete    — Delete a credential

use aigw_core::crypto::{decrypt_json_fields, decrypt_litellm_value};
use aigw_core::models::Credential;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

use crate::routes::keys::SharedState;
use super::spend::{require_admin, SpendAuth};

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request/Response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct CredentialInfoQuery {
    pub credential_name: String,
}

#[derive(Debug, Deserialize)]
pub struct CredentialDeleteQuery {
    pub credential_name: String,
}

#[derive(Debug, Deserialize)]
pub struct NewCredentialBody {
    pub credential_name: String,
    pub credential_values: Value,
    #[serde(default)]
    pub credential_info: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCredentialBody {
    pub credential_name: String,
    #[serde(default)]
    pub credential_values: Option<Value>,
    #[serde(default)]
    pub credential_info: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct CredentialResponse {
    pub credential_id: String,
    pub credential_name: String,
    pub credential_values: Value,
    pub credential_info: Value,
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

impl CredentialResponse {
    /// Build a CredentialResponse, decrypting `credential_values` if needed.
    ///
    /// Mirrors `ModelResponse::from_model()` / `decrypt_params()`: handles both
    /// plain JSON object fields and the single-blob encryption produced by
    /// `aigw-migrate remote-import`.
    fn from_credential(c: Credential, master_key: Option<&str>) -> Self {
        let credential_values = Self::decrypt_credential_values(&c.credential_values, master_key);
        Self {
            credential_id: c.credential_id,
            credential_name: c.credential_name,
            credential_values,
            credential_info: c.credential_info,
            created_at: c.created_at,
            created_by: c.created_by,
            updated_at: c.updated_at,
            updated_by: c.updated_by,
        }
    }

    fn decrypt_credential_values(params: &Value, master_key: Option<&str>) -> Value {
        let key = match master_key {
            Some(k) => k,
            None => {
                warn!("AIGW_MASTER_KEY not configured — returning encrypted credential_values as-is");
                return params.clone();
            }
        };
        match params {
            Value::Object(_) => decrypt_json_fields(params, key),
            Value::String(s) => {
                if s.starts_with('{') {
                    return params.clone();
                }
                let decrypted = match decrypt_litellm_value(s, key) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!("Failed to decrypt credential_values: {} — returning encrypted value as-is", e);
                        return params.clone();
                    }
                };
                serde_json::from_str(&decrypted).unwrap_or_else(|_| params.clone())
            }
            _ => params.clone(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handlers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /credential/new — create a new credential (admin)
pub async fn credential_new(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<NewCredentialBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;

    // Check for duplicate credential_name
    let existing = state
        .db
        .get_credential_by_name(&body.credential_name)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;
    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": {"message": "credential_name already exists"}})),
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let credential = Credential {
        credential_id: uuid::Uuid::new_v4().to_string(),
        credential_name: body.credential_name.clone(),
        credential_values: body.credential_values.clone(),
        credential_info: body.credential_info.clone().unwrap_or(json!({})),
        created_at: now.clone(),
        created_by: None,
        updated_at: now,
        updated_by: None,
    };

    state
        .db
        .insert_credential(&credential)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;

    let resp = CredentialResponse::from_credential(credential, state.aigw_master_key.as_deref());
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// GET /credential/info?credential_name=... — get credential details (admin)
pub async fn credential_info(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(q): Query<CredentialInfoQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let credential = state
        .db
        .get_credential_by_name(&q.credential_name)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;

    match credential {
        Some(c) => {
            let resp = CredentialResponse::from_credential(c, state.aigw_master_key.as_deref());
            Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "credential not found"}})),
        )),
    }
}

/// GET /credential/list — list all credentials (admin)
pub async fn credential_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let credentials = state.db.list_credentials().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let master_key = state.aigw_master_key.as_deref();
    let data: Vec<Value> = credentials
        .into_iter()
        .map(|c| serde_json::to_value(CredentialResponse::from_credential(c, master_key)).unwrap_or(json!({})))
        .collect();

    Ok(Json(json!({"object": "list", "data": data})))
}

/// PUT /credential/update — update an existing credential (admin)
pub async fn credential_update(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<UpdateCredentialBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let existing = state
        .db
        .get_credential_by_name(&body.credential_name)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;

    let mut credential = existing.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "credential not found"}})),
        )
    })?;

    if let Some(ref values) = body.credential_values {
        credential.credential_values = values.clone();
    }
    if let Some(ref info) = body.credential_info {
        credential.credential_info = info.clone();
    }
    credential.updated_at = chrono::Utc::now().to_rfc3339();

    state
        .db
        .update_credential(&credential)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;

    let resp = CredentialResponse::from_credential(credential, state.aigw_master_key.as_deref());
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// DELETE /credential/delete?credential_name=... — delete a credential (admin)
pub async fn credential_delete(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(q): Query<CredentialDeleteQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let existing = state
        .db
        .get_credential_by_name(&q.credential_name)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;

    if existing.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "credential not found"}})),
        ));
    }

    state
        .db
        .delete_credential(&q.credential_name)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e)}})),
            )
        })?;

    Ok(Json(json!({"status": "deleted"})))
}
