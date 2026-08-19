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

use super::spend::{require_admin, SpendAuth};
use crate::routes::keys::SharedState;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request/Response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct CredentialListQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

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
                warn!(
                    "AIGW_MASTER_KEY not configured — returning encrypted credential_values as-is"
                );
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

    state.db.insert_credential(&credential).await.map_err(|e| {
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
            let mut value = serde_json::to_value(resp).unwrap_or(json!({}));
            if let Some(cv) = value.get_mut("credential_values") {
                *cv = aigw_core::crypto::redact_oauth_credential_values(cv);
            }
            Ok(Json(value))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "credential not found"}})),
        )),
    }
}

/// GET /credential/list — list all credentials (admin, paginated)
pub async fn credential_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<CredentialListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (credentials, total_count) = tokio::try_join!(
        state.db.list_credentials_paged(limit, offset),
        state.db.count_credentials(),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    let master_key = state.aigw_master_key.as_deref();
    let data: Vec<Value> = credentials
        .into_iter()
        .map(|c| {
            let mut value =
                serde_json::to_value(CredentialResponse::from_credential(c, master_key))
                    .unwrap_or(json!({}));
            if let Some(cv) = value.get_mut("credential_values") {
                *cv = aigw_core::crypto::redact_oauth_credential_values(cv);
            }
            value
        })
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

    state.db.update_credential(&credential).await.map_err(|e| {
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Claude OAuth cookie→token exchange (Phase 51, Stage 126)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// POST /credential/oauth/exchange request body.
#[derive(Debug, Deserialize)]
pub struct OauthExchangeBody {
    /// `sk-ant-sid...` session cookie.
    pub session_key: String,
    /// Optional proxy binding (must exist + be active). None → direct exchange.
    #[serde(default)]
    pub proxy_id: Option<i64>,
    /// Optional prompt appended to the system block when the model is resolved
    /// to this OAuth credential (Stage 128 wiring).
    #[serde(default)]
    pub inject_prompt: Option<String>,
    /// Credential name (must be unique).
    pub name: String,
}

/// POST /credential/oauth/exchange — run the 3-step cookie→token exchange and
/// persist an `anthropic_oauth` credential with sensitive fields encrypted.
///
/// Returns the credential with the token trio redacted (access/refresh/session
/// masked as `***`).
pub async fn oauth_exchange(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<OauthExchangeBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use aigw_core::claude_oauth as oauth;

    require_admin(&auth)?;

    // Validate session_key shape (warn-only for sk-ant- prefix, hard-error empty).
    if body.session_key.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "session_key is required"}})),
        ));
    }
    if !body.session_key.starts_with("sk-ant-") {
        warn!("session_key does not start with sk-ant- — may not be a valid Claude cookie");
    }

    // Resolve the bound proxy URL (optional binding).
    let proxy_url = match body.proxy_id {
        Some(pid) => {
            let p = state.db.get_proxy_by_id(pid).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
                )
            })?;
            let p = p.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": {"message": "proxy_id not found"}})),
                )
            })?;
            if p.status != "active" {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": {"message": "proxy_id is not active"}})),
                ));
            }
            // proxy_url is encrypted — decrypt with the master key.
            let mk = state.aigw_master_key.as_deref().ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": "AIGW_MASTER_KEY not configured — cannot decrypt proxy_url"}})),
                )
            })?;
            let plain = aigw_core::crypto::decrypt_proxy_url(&p.proxy_url, mk).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": format!("Failed to decrypt proxy_url: {}", e)}})),
                )
            })?;
            Some(plain)
        }
        None => None,
    };

    // Run the 3-step exchange (through the proxy client when bound).
    let client = oauth::OauthClient::new(proxy_url.as_deref()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": format!("Invalid proxy_url: {}", e), "type": "invalid_request_error"}})),
        )
    })?;
    let (token, org_uuid) = match client.exchange(&body.session_key).await {
        Ok(pair) => pair,
        Err(e) => {
            let status = match e.kind.as_str() {
                "cf_challenge"
                | "account_session_invalid"
                | "account_blocked"
                | "forbidden"
                | "unauthorized" => StatusCode::FORBIDDEN,
                "rate_limited" => StatusCode::TOO_MANY_REQUESTS,
                "no_org" => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            return Err((status, Json(json!({ "error": e }))));
        }
    };

    // Persist as an OAuth credential with sensitive fields encrypted.
    let mk = state.aigw_master_key.as_deref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": "AIGW_MASTER_KEY not configured — cannot encrypt credential"}})),
        )
    })?;
    let values = aigw_core::claude_oauth::build_oauth_credential_values(
        &body.session_key,
        &token,
        &org_uuid,
        body.proxy_id,
        body.inject_prompt.as_deref(),
        mk,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Failed to encrypt credential: {}", e)}})),
        )
    })?;

    // Duplicate name guard (same as /credential/new).
    let existing = state
        .db
        .get_credential_by_name(&body.name)
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
        credential_name: body.name.clone(),
        credential_values: values,
        credential_info: json!({}),
        created_at: now.clone(),
        created_by: None,
        updated_at: now,
        updated_by: None,
    };
    state.db.insert_credential(&credential).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e)}})),
        )
    })?;

    // Response: decrypt + redact the token trio before exposing.
    let resp = CredentialResponse::from_credential(credential, Some(mk));
    let mut value = serde_json::to_value(resp).unwrap_or(json!({}));
    if let Some(cv) = value.get_mut("credential_values") {
        *cv = aigw_core::crypto::redact_oauth_credential_values(cv);
    }
    Ok(Json(value))
}
