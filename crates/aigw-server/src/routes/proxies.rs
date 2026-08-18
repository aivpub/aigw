//! Proxy service management endpoints — /admin/proxies/* (Phase 50, Stage 122/123)
//!
//! Endpoints:
//! - GET    /admin/proxies          — list (paged, status/search/sort filters)
//! - GET    /admin/proxies/all      — all active proxies (credential-bind dropdown)
//! - POST   /admin/proxies          — create proxy (proxy_url encrypted)
//! - GET    /admin/proxies/{id}     — detail (decrypt + redact password)
//! - PUT    /admin/proxies/{id}     — update (whole-url re-encrypt)
//! - DELETE /admin/proxies/{id}     — delete (in-use guard → 409)
//! - POST   /admin/proxies/batch-delete — batch delete (in-use skipped)
//! - POST   /admin/proxies/{id}/test     — exit probe (IP + latency), writes snapshot
//! - POST   /admin/proxies/{id}/quality  — quality check (score + grade + items)
//! - POST   /admin/proxies/{id}/toggle   — active ↔ inactive
//!
//! Create/update trigger an async probe (`spawn_async_probe`) via the real
//! probe engine (`aigw_core::probe`) so the request path never blocks.

use aigw_core::crypto::{decrypt_proxy_url, encrypt_proxy_url, redact_proxy_url};
use aigw_core::models::{CreateProxyRequest, Proxy, UpdateProxyRequest};
use axum::{
    extract::{Path, Query, State},
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
pub struct ProxyListQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchDeleteBody {
    pub ids: Vec<i64>,
}

/// Admin-facing proxy response — `proxy_url` is **redacted** (password masked).
/// `exit_ip`/`country`/`latency_ms`/`score`/`grade` are projected out of the
/// `probe_result` JSON for the list table (Stage 124 frontend).
#[derive(Debug, Serialize)]
pub struct ProxyResponse {
    pub id: i64,
    pub name: String,
    pub proxy_url: String, // redacted: scheme://user:***@host:port
    pub status: String,
    pub expires_at: Option<String>,
    pub probe_result: Value,
    pub created_at: String,
    pub updated_at: String,
    // Projected from probe_result for convenience
    pub exit_ip: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub latency_ms: Option<u64>,
    pub score: Option<i64>,
    pub grade: Option<String>,
}

fn probe_opt(pr: &Value, key: &str) -> Option<String> {
    pr.get(key).and_then(|v| v.as_str()).map(String::from)
}

impl ProxyResponse {
    fn from_proxy(p: Proxy, master_key: Option<&str>) -> Self {
        let redacted = match master_key {
            Some(key) => match decrypt_proxy_url(&p.proxy_url, key) {
                Ok(plain) => redact_proxy_url(&plain),
                Err(e) => {
                    warn!(
                        "Failed to decrypt proxy_url for proxy {}: {} — returning as-is",
                        p.id, e
                    );
                    "[encrypted]".to_string()
                }
            },
            None => "[encrypted]".to_string(),
        };
        let pr = &p.probe_result;
        Self {
            id: p.id,
            name: p.name,
            proxy_url: redacted,
            status: p.status,
            expires_at: p.expires_at,
            probe_result: pr.clone(),
            created_at: p.created_at,
            updated_at: p.updated_at,
            exit_ip: probe_opt(pr, "exit_ip"),
            country: probe_opt(pr, "country"),
            country_code: probe_opt(pr, "country_code"),
            latency_ms: pr.get("latency_ms").and_then(|v| v.as_u64()),
            score: pr.get("score").and_then(|v| v.as_i64()),
            grade: probe_opt(pr, "grade"),
        }
    }
}

/// Encrypt + timestamp a proxy for persistence. On missing master key, returns
/// an error so we never silently store plaintext proxy credentials.
fn build_persisted_proxy(
    name: &str,
    proxy_url: &str,
    expires_at: Option<&str>,
    status: &str,
    probe_result: Value,
    id: Option<i64>,
    master_key: &str,
) -> Result<Proxy, (StatusCode, Json<Value>)> {
    let encrypted = encrypt_proxy_url(proxy_url, master_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Failed to encrypt proxy_url: {}", e)}})),
        )
    })?;
    let now = chrono::Utc::now().to_rfc3339();
    Ok(Proxy {
        id: id.unwrap_or(0),
        name: name.to_string(),
        proxy_url: encrypted,
        status: status.to_string(),
        expires_at: expires_at.map(String::from),
        probe_result,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Trigger a background exit+quality probe after create/update.
///
/// Stage 123: real probe engine — runs exit + quality via the proxy client and
/// writes the snapshot into `proxies.probe_result`. Never blocks the request
/// path (tokio::spawn). Failure is silent (logged) — the existing snapshot (or
/// `{}`) stays in place.
fn spawn_async_probe(db: aigw_core::db::Database, id: i64) {
    tokio::spawn(async move {
        let Some(proxy) = db.get_proxy_by_id(id).await.ok().flatten() else {
            return;
        };
        // `proxy_url` is encrypted — decrypt with the configured master key
        // (read from the config table at probe time; one cheap row).
        let mk = match db.get_master_key_from_db().await {
            Ok(Some(k)) => k,
            _ => return,
        };
        let Ok(plain_url) = decrypt_proxy_url(&proxy.proxy_url, &mk) else {
            return;
        };
        match aigw_core::probe::run_full_probe(&plain_url, std::time::Duration::from_secs(10)).await
        {
            Ok(snapshot) => {
                let mut updated = proxy.clone();
                updated.probe_result = snapshot;
                updated.updated_at = chrono::Utc::now().to_rfc3339();
                if let Err(e) = db.update_proxy(&updated).await {
                    tracing::warn!("Failed to persist probe_result for proxy {}: {}", id, e);
                }
            }
            Err(e) => {
                tracing::warn!("Async probe for proxy {} failed: {}", id, e);
            }
        }
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Handlers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// GET /admin/proxies — list (paged, filters)
pub async fn list_proxies(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<ProxyListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;

    let (proxies, total_count) = tokio::try_join!(
        state.db.list_proxies(
            limit,
            offset,
            query.status.as_deref(),
            query.search.as_deref(),
            query.sort_by.as_deref(),
            query.sort_order.as_deref(),
        ),
        state
            .db
            .count_proxies(query.status.as_deref(), query.search.as_deref()),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;

    let mk = state.aigw_master_key.as_deref();
    let data: Vec<Value> = proxies
        .into_iter()
        .map(|p| serde_json::to_value(ProxyResponse::from_proxy(p, mk)).unwrap_or(json!({})))
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

/// GET /admin/proxies/all — all active proxies (dropdown)
pub async fn list_all_proxies(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let proxies = state.db.list_active_proxies().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    let mk = state.aigw_master_key.as_deref();
    let data: Vec<Value> = proxies
        .into_iter()
        .map(|p| serde_json::to_value(ProxyResponse::from_proxy(p, mk)).unwrap_or(json!({})))
        .collect();
    Ok(Json(
        json!({ "object": "list", "data": data, "count": data.len() }),
    ))
}

/// POST /admin/proxies — create proxy
pub async fn create_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<CreateProxyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mk = state.aigw_master_key.as_deref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": "AIGW_MASTER_KEY not configured — cannot encrypt proxy_url"}})),
        )
    })?;

    let proxy = build_persisted_proxy(
        &body.name,
        &body.proxy_url,
        body.expires_at.as_deref(),
        "active",
        json!({}),
        None,
        mk,
    )?;
    let id = state.db.create_proxy(&proxy).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    // Stage 123: async exit + quality probe after create.
    spawn_async_probe(state.db.clone(), id);

    let created = state.db.get_proxy_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    let resp = ProxyResponse::from_proxy(created.unwrap(), Some(mk));
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// GET /admin/proxies/{id} — detail
pub async fn get_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let proxy = state.db.get_proxy_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    match proxy {
        Some(p) => {
            let resp = ProxyResponse::from_proxy(p, state.aigw_master_key.as_deref());
            Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "proxy not found"}})),
        )),
    }
}

/// PUT /admin/proxies/{id} — update (whole-url re-encrypt when proxy_url present)
pub async fn update_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProxyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let existing = state.db.get_proxy_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    let mut proxy = existing.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "proxy not found"}})),
        )
    })?;

    if let Some(ref name) = body.name {
        proxy.name = name.clone();
    }
    if let Some(ref url) = body.proxy_url {
        let mk = state.aigw_master_key.as_deref().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": "AIGW_MASTER_KEY not configured — cannot encrypt proxy_url"}})),
            )
        })?;
        proxy.proxy_url = encrypt_proxy_url(url, mk).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("Failed to encrypt proxy_url: {}", e)}})),
            )
        })?;
    }
    if let Some(ref status) = body.status {
        proxy.status = status.clone();
    }
    if let Some(ref exp) = body.expires_at {
        proxy.expires_at = Some(exp.clone());
    }
    proxy.updated_at = chrono::Utc::now().to_rfc3339();

    state.db.update_proxy(&proxy).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    // Stage 123: async re-probe after update.
    spawn_async_probe(state.db.clone(), id);

    let resp = ProxyResponse::from_proxy(proxy, state.aigw_master_key.as_deref());
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

/// DELETE /admin/proxies/{id} — delete (in-use guard → 409 PROXY_IN_USE)
pub async fn delete_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    if state
        .db
        .proxy_in_use_by_credentials(id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?
    {
        let creds = state
            .db
            .credentials_referencing_proxy(id)
            .await
            .unwrap_or_default();
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": {
                    "message": "Proxy is in use by one or more credentials",
                    "type": "PROXY_IN_USE",
                    "referenced_by": creds,
                }
            })),
        ));
    }
    state.db.delete_proxy(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    Ok(Json(json!({"status": "deleted"})))
}

/// POST /admin/proxies/batch-delete — batch delete (in-use skipped)
pub async fn batch_delete_proxies(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<BatchDeleteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut deleted_ids = Vec::new();
    let mut skipped = Vec::new();
    for id in body.ids {
        match state.db.proxy_in_use_by_credentials(id).await {
            Ok(true) => {
                let creds = state
                    .db
                    .credentials_referencing_proxy(id)
                    .await
                    .unwrap_or_default();
                skipped.push(json!({"id": id, "reason": "in_use", "referenced_by": creds}));
            }
            Ok(false) => {
                state.db.delete_proxy(id).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
                    )
                })?;
                deleted_ids.push(id);
            }
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
                ))
            }
        }
    }
    Ok(Json(json!({
        "status": "ok",
        "deleted_ids": deleted_ids,
        "skipped": skipped,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 123: exit + quality probe endpoints
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Build a proxy client for a proxy id (decrypt + redact resolved URL), or a
/// 404/500/400 error. `proxy_url` is decrypted from the ciphertext.
async fn proxy_client(
    state: &SharedState,
    id: i64,
) -> Result<reqwest::Client, (StatusCode, Json<Value>)> {
    let plain = plain_proxy_url(state, id).await?;
    aigw_core::probe::build_proxy_client(&plain, std::time::Duration::from_secs(10)).map_err(
        |e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": format!("Invalid proxy_url: {}", e), "type": "invalid_request_error"}})),
            )
        },
    )
}

/// Resolve the plaintext proxy URL for a proxy id, or a 404/500 error.
async fn plain_proxy_url(
    state: &SharedState,
    id: i64,
) -> Result<String, (StatusCode, Json<Value>)> {
    let proxy = state.db.get_proxy_by_id(id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    let proxy = proxy.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": {"message": "proxy not found"}})),
        )
    })?;
    let mk = state.aigw_master_key.as_deref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": "AIGW_MASTER_KEY not configured — cannot decrypt proxy_url"}})),
        )
    })?;
    decrypt_proxy_url(&proxy.proxy_url, mk).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Failed to decrypt proxy_url: {}", e)}})),
        )
    })
}

/// Persist a probe snapshot into `proxies.probe_result` for a proxy id.
async fn persist_snapshot(
    state: &SharedState,
    id: i64,
    snapshot: Value,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut proxy = state
        .db
        .get_proxy_by_id(id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": {"message": "proxy not found"}})),
            )
        })?;
    proxy.probe_result = snapshot;
    proxy.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.update_proxy(&proxy).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })
}

/// POST /admin/proxies/{id}/test — exit probe (IP + latency), writes snapshot.
pub async fn test_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let client = proxy_client(&state, id).await?;
    let exit = aigw_core::probe::probe_exit(&client).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Exit probe failed: {}", e)}})),
        )
    })?;
    let snapshot = json!({
        "latency_ms": exit.latency_ms,
        "exit_ip": exit.ip,
        "country": exit.country,
        "country_code": exit.country_code,
        "region": exit.region,
        "city": exit.city,
        "last_check_at": chrono::Utc::now().to_rfc3339(),
    });
    persist_snapshot(&state, id, snapshot.clone()).await?;
    let resp = json!({
        "id": id,
        "probe_result": snapshot,
    });
    Ok(Json(resp))
}

/// POST /admin/proxies/{id}/quality — quality check (score + grade + items).
pub async fn quality_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let client = proxy_client(&state, id).await?;
    let exit = aigw_core::probe::probe_exit(&client).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("Exit probe failed: {}", e)}})),
        )
    })?;
    let quality = aigw_core::probe::run_quality_check(&client, &exit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("Quality check: {}", e)}})),
            )
        })?;
    let snapshot = json!({
        "latency_ms": quality.base_latency_ms,
        "exit_ip": quality.exit_ip,
        "country": quality.country,
        "country_code": quality.country_code,
        "score": quality.score,
        "grade": quality.grade,
        "overall_status": quality.overall_status,
        "items": quality.items,
        "last_check_at": quality.last_check_at,
    });
    persist_snapshot(&state, id, snapshot.clone()).await?;
    Ok(Json(json!({ "id": id, "probe_result": snapshot })))
}

/// POST /admin/proxies/{id}/toggle — active ↔ inactive.
pub async fn toggle_proxy(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut proxy = state
        .db
        .get_proxy_by_id(id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": {"message": "proxy not found"}})),
            )
        })?;
    proxy.status = if proxy.status == "inactive" {
        "active".to_string()
    } else {
        "inactive".to_string()
    };
    proxy.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.update_proxy(&proxy).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"message": format!("{}", e), "type": "db_error"}})),
        )
    })?;
    let resp = ProxyResponse::from_proxy(proxy, state.aigw_master_key.as_deref());
    Ok(Json(serde_json::to_value(resp).unwrap_or(json!({}))))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::DEFAULT_KEY_TOKEN_LEN;
    use axum::Router;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    async fn test_state() -> SharedState {
        let db = aigw_core::db::Database::init("sqlite::memory:")
            .await
            .unwrap();
        let mk = "sk-master-test".to_string();
        Arc::new(super::super::keys::AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some(mk.clone()),
            aigw_master_key: Some(mk),
            key_generate_length: DEFAULT_KEY_TOKEN_LEN,
            disable_custom_api_keys: false,
            provider_registry: aigw_core::provider::ProviderRegistry::new(),
            router_state: aigw_core::router::RouterState::default(),
            rate_limiter: Arc::new(aigw_core::rate_limiter::RateLimiter::new()),
            deployment_mode: "test".to_string(),
            started_at: std::time::Instant::now(),
            daily_spend_queue: None,
            otel_active: false,
            body_archiver: None,
            metrics: None,
        })
    }

    fn build_proxy_router(state: SharedState) -> Router {
        use axum::routing::{get, post};
        Router::new()
            .route("/admin/proxies", get(list_proxies).post(create_proxy))
            .route("/admin/proxies/all", get(list_all_proxies))
            .route("/admin/proxies/batch-delete", post(batch_delete_proxies))
            .route(
                "/admin/proxies/{id}",
                get(get_proxy).put(update_proxy).delete(delete_proxy),
            )
            .route("/admin/proxies/{id}/test", post(test_proxy))
            .route("/admin/proxies/{id}/quality", post(quality_proxy))
            .route("/admin/proxies/{id}/toggle", post(toggle_proxy))
            .with_state(state)
    }

    async fn send(
        app: &Router,
        method: axum::http::Method,
        uri: &str,
        mk: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", mk));
        let rb = req
            .body(axum::body::Body::from(
                body.map(|b| b.to_string()).unwrap_or_default(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(rb).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), 1024)
            .await
            .unwrap_or_default();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::json!({}));
        (status, json)
    }

    async fn create_proxy_helper(app: &Router, mk: &str, name: &str) -> i64 {
        let (s, body) = send(
            app,
            axum::http::Method::POST,
            "/admin/proxies",
            mk,
            Some(serde_json::json!({
                "name": name,
                "proxy_url": format!("http://user:secret@1.2.3.4:8080"),
            })),
        )
        .await;
        assert_eq!(s, 200, "create proxy failed: {}", body);
        body["id"].as_i64().expect("proxy id")
    }

    /// Stage 122 UT-1: create response masks the password in proxy_url.
    #[tokio::test]
    async fn test_proxy_create_masks_password() {
        let state = test_state().await;
        let app = build_proxy_router(state);
        let mk = "sk-master-test";
        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/proxies",
            mk,
            Some(serde_json::json!({
                "name": "masked-proxy",
                "proxy_url": "http://user:secret@1.2.3.4:8080",
            })),
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["name"], "masked-proxy");
        // proxy_url must be redacted — no plaintext password anywhere
        let url = body["proxy_url"].as_str().unwrap();
        assert!(
            url.contains(":***@"),
            "expected masked password, got {}",
            url
        );
        assert!(
            !url.contains("secret"),
            "password leaked in response: {}",
            url
        );
        // The response's probe_result is an empty snapshot
        assert_eq!(body["probe_result"], serde_json::json!({}));
    }

    /// Stage 122 UT-2: delete an in-use proxy → 409 PROXY_IN_USE.
    #[tokio::test]
    async fn test_proxy_delete_in_use_409() {
        let state = test_state().await;
        let app = build_proxy_router(state.clone());
        let mk = "sk-master-test";
        let id = create_proxy_helper(&app, mk, "inuse").await;

        // Insert a credential referencing this proxy id in credential_values.proxy_id
        let cred = aigw_core::models::Credential {
            credential_id: uuid::Uuid::new_v4().to_string(),
            credential_name: "oauth-cred".to_string(),
            credential_values: serde_json::json!({"proxy_id": id}),
            credential_info: serde_json::json!({}),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: None,
        };
        state
            .db
            .insert_credential(&cred)
            .await
            .expect("insert credential");

        let (s, body) = send(
            &app,
            axum::http::Method::DELETE,
            &format!("/admin/proxies/{}", id),
            mk,
            None,
        )
        .await;
        assert_eq!(s, 409);
        assert_eq!(body["error"]["type"], "PROXY_IN_USE");
        assert_eq!(body["error"]["referenced_by"][0], "oauth-cred");
    }

    /// Stage 122 UT-3: non-admin key → 403 forbidden.
    #[tokio::test]
    async fn test_proxy_require_admin_403() {
        let state = test_state().await;
        let app = build_proxy_router(state.clone());
        // A regular key (non-master) is not admin
        let raw = format!("sk-{}", uuid::Uuid::new_v4());
        let key = aigw_core::models::VirtualKey {
            token: aigw_core::crypto::hash_token(&raw),
            key_name: Some("non-admin".to_string()),
            key_alias: Some("non-admin".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: serde_json::json!([]),
            aliases: serde_json::json!({}),
            config: serde_json::json!({}),
            router_settings: None,
            user_id: None,
            team_id: None,
            agent_id: None,
            project_id: None,
            permissions: serde_json::json!({}),
            max_parallel_requests: None,
            metadata: serde_json::json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: None,
            soft_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: serde_json::json!([]),
            allowed_routes: serde_json::json!([]),
            policies: serde_json::json!([]),
            access_group_ids: serde_json::json!([]),
            model_spend: serde_json::json!({}),
            model_max_budget: serde_json::json!({}),
            budget_id: None,
            organization_id: None,
            object_permission_id: None,
            created_at: Some(chrono::Utc::now()),
            created_by: Some("test".to_string()),
            updated_at: Some(chrono::Utc::now()),
            updated_by: Some("test".to_string()),
            last_active: None,
            rotation_count: None,
            auto_rotate: None,
            rotation_interval: None,
            last_rotation_at: None,
            key_rotation_at: None,
            budget_limits: None,
            user_email: None,
            user_alias: None,
        };
        state.db.insert_key(&key).await.expect("insert key");

        let (s, _) = send(&app, axum::http::Method::GET, "/admin/proxies", &raw, None).await;
        assert_eq!(s, 403);
    }

    /// Stage 123 UT-1: toggle flips status active → inactive and back.
    #[tokio::test]
    async fn test_proxy_toggle_status() {
        let state = test_state().await;
        let app = build_proxy_router(state);
        let mk = "sk-master-test";
        let id = create_proxy_helper(&app, mk, "toggle-proxy").await;

        // active → inactive
        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/proxies/{}/toggle", id),
            mk,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["status"], "inactive");

        // inactive → active
        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/proxies/{}/toggle", id),
            mk,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["status"], "active");
    }

    /// Stage 123 UT-2: test/quality return 400 for an invalid proxy_url
    /// (unreachable build — build_proxy_client rejects malformed URLs).
    #[tokio::test]
    async fn test_proxy_test_and_quality_endpoints_require_valid_url() {
        let state = test_state().await;
        let app = build_proxy_router(state.clone());
        let mk = "sk-master-test";
        // Create a proxy with a malformed URL so build_proxy_client fails → 400.
        let (s, _) = send(
            &app,
            axum::http::Method::POST,
            "/admin/proxies",
            mk,
            Some(serde_json::json!({
                "name": "bad-url",
                "proxy_url": "not-a-url",
            })),
        )
        .await;
        assert_eq!(s, 200);
        let id = create_proxy_helper(&app, mk, "bad-url-valid").await;
        // Overwrite the created proxy's url with an invalid one.
        let mut p = state.db.get_proxy_by_id(id).await.unwrap().unwrap();
        p.proxy_url = aigw_core::crypto::encrypt_proxy_url("://", mk).unwrap();
        state.db.update_proxy(&p).await.unwrap();

        let (s, _) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/proxies/{}/test", id),
            mk,
            None,
        )
        .await;
        assert_eq!(s, 400);

        let (s, _) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/proxies/{}/quality", id),
            mk,
            None,
        )
        .await;
        assert_eq!(s, 400);
    }
}
