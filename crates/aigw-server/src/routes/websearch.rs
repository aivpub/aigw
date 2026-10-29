//! Search provider / instance administration — /admin/websearch/* (Stage 138).
//!
//! Endpoints:
//! - GET    /admin/websearch/providers                    — list (with instances + default flag)
//! - POST   /admin/websearch/providers                    — create
//! - GET    /admin/websearch/providers/{id}               — detail
//! - PUT    /admin/websearch/providers/{id}               — update
//! - DELETE /admin/websearch/providers/{id}               — delete (default guard → 409)
//! - POST   /admin/websearch/providers/batch-delete       — batch delete (default skipped)
//! - POST   /admin/websearch/providers/{id}/toggle        — enable ↔ disable
//! - GET    /admin/websearch/providers/{id}/instances     — list a provider's instances
//! - POST   /admin/websearch/providers/{id}/instances     — add an instance
//! - GET    /admin/websearch/instances/{id}               — instance detail
//! - PUT    /admin/websearch/instances/{id}               — update instance
//! - DELETE /admin/websearch/instances/{id}               — delete instance
//! - POST   /admin/websearch/instances/{id}/toggle        — enable ↔ disable
//! - POST   /admin/websearch/instances/{id}/test          — connectivity probe, writes snapshot
//! - POST   /admin/websearch/reload                       — rebuild the runtime registry
//! - POST   /admin/websearch/enable-all                   — enable every provider + instance
//!
//! Every successful write triggers a reload, so "edit in the UI, takes effect
//! now" is the default rather than something an operator has to remember. The
//! reload reads the *same* source-selection rules as boot
//! ([`aigw_core::config_loader::load_websearch_registry`]); a reload that fails
//! keeps the previous registry rather than storing `None`, because silently
//! switching search off is worse than carrying a stale price for one more edit.

use aigw_core::crypto::encrypt_litellm_value_gcm;
use aigw_core::models::{WebSearchInstanceRow, WebSearchProviderRow};
use aigw_core::websearch::{probe_instance, SUPPORTED_KINDS};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};

use super::spend::{require_admin, SpendAuth};
use crate::routes::keys::SharedState;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Request / response types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Deserialize)]
pub struct BatchDeleteBody {
    pub ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub cost_per_query: Option<f64>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub failover_order: Option<i64>,
    #[serde(default)]
    pub timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProviderRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub cost_per_query: Option<f64>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub failover_order: Option<i64>,
    #[serde(default)]
    pub timeout_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct InstanceRequest {
    pub name: String,
    pub base_url: String,
    /// Plaintext on the way in; stored as `v2:gcm:` ciphertext.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub weight: Option<i64>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct InstanceResponse {
    pub id: i64,
    pub provider_id: i64,
    pub name: String,
    pub base_url: String,
    /// `true` when a key is stored, never the key itself.
    pub has_api_key: bool,
    pub redacted_api_key: Option<String>,
    pub weight: Option<i64>,
    pub enabled: bool,
    pub probe_result: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ProviderResponse {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub cost_per_query: f64,
    pub enabled: bool,
    /// True for the row that actually serves un-named requests: the enabled
    /// provider with the smallest non-NULL `failover_order`, ties by id.
    pub is_default: bool,
    pub failover_order: Option<i64>,
    pub timeout_ms: Option<i64>,
    pub instances: Vec<InstanceResponse>,
    pub created_at: String,
    pub updated_at: String,
}

fn bad_request(message: impl Into<String>, kind: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error": {"message": message.into(), "type": kind}})),
    )
}

fn db_error(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": {"message": format!("{e}"), "type": "db_error"}})),
    )
}

fn not_found(what: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"message": format!("{what} not found"), "type": "not_found"}})),
    )
}

/// Reject anything we have no client for, and say which of the two situations it
/// is — "no such vendor" and "planned but not implemented" call for different
/// actions from the operator.
fn validate_kind(kind: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if SUPPORTED_KINDS.contains(&kind) {
        return Ok(());
    }
    let planned = ["tavily", "bocha"];
    let hint = if planned.contains(&kind) {
        "this provider is planned but has no client yet (Stage 134 §8.2); adding its client is purely additive"
    } else {
        "unknown provider kind"
    };
    Err(bad_request(
        format!(
            "unsupported provider kind \"{kind}\" — {hint}. Supported kinds: {}",
            SUPPORTED_KINDS.join(", ")
        ),
        "unsupported_provider_kind",
    ))
}

fn validate_cost(cost: f64) -> Result<(), (StatusCode, Json<Value>)> {
    if !cost.is_finite() || cost < 0.0 {
        return Err(bad_request(
            "cost_per_query must be a finite, non-negative number",
            "invalid_cost_per_query",
        ));
    }
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl InstanceResponse {
    fn from_row(mut row: WebSearchInstanceRow, master_key: Option<&str>) -> Self {
        let stored = row.api_key.take().filter(|k| !k.trim().is_empty());
        let redacted = stored.as_deref().and_then(|k| {
            master_key
                .and_then(|mk| aigw_core::crypto::decrypt_litellm_value(k, mk).ok())
                .map(|plain| redact_secret(&plain))
        });
        Self {
            id: row.id,
            provider_id: row.provider_id,
            name: row.name,
            base_url: row.base_url,
            has_api_key: stored.is_some(),
            redacted_api_key: redacted,
            weight: row.weight,
            enabled: row.enabled,
            probe_result: row.probe_result,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Never echo a stored credential. Showing the last 4 characters is enough for
/// an operator to tell two keys apart without the response becoming a place to
/// copy a secret from.
fn redact_secret(plain: &str) -> String {
    let n = plain.chars().count();
    if n <= 4 {
        return "****".to_string();
    }
    let tail: String = plain.chars().skip(n - 4).collect();
    format!("****{tail}")
}

impl ProviderResponse {
    fn from_row(
        row: &WebSearchProviderRow,
        instances: Vec<WebSearchInstanceRow>,
        default_id: Option<i64>,
        master_key: Option<&str>,
    ) -> Self {
        Self {
            id: row.id,
            name: row.name.clone(),
            kind: row.kind.clone(),
            cost_per_query: row.cost_per_query,
            enabled: row.enabled,
            is_default: default_id == Some(row.id),
            failover_order: row.failover_order,
            timeout_ms: row.timeout_ms,
            instances: instances
                .into_iter()
                .map(|i| InstanceResponse::from_row(i, master_key))
                .collect(),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Reload
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Rebuild the runtime registry from the current rows and swap it in.
///
/// On failure the **previous** registry stays in place. The writes already
/// landed, and trading a stale-but-working search layer for a dead one would
/// turn one bad row into an outage. The outcome is reported back to the caller
/// so "saved" and "saved and live" stay distinguishable.
async fn reload_registry(state: &SharedState) -> Result<Vec<String>, String> {
    let yaml = state.web_search.yaml_base();
    let mk = state.aigw_master_key.as_deref().unwrap_or_default();
    match aigw_core::config_loader::load_websearch_registry(&state.db, &yaml, mk).await {
        Ok(reg) => {
            let names: Vec<String> = reg
                .as_ref()
                .map(|r| r.provider_names().into_iter().map(str::to_string).collect())
                .unwrap_or_default();
            state.web_search.store(reg.map(std::sync::Arc::new));
            info!(providers = ?names, "web search registry reloaded");
            Ok(names)
        }
        Err(e) => {
            warn!(error = %e, "web search reload failed; keeping the previous registry");
            Err(e)
        }
    }
}

/// Attach the reload outcome to a write response.
fn with_reload(mut body: Value, outcome: Result<Vec<String>, String>) -> Value {
    body["reload"] = match outcome {
        Ok(names) => json!({"ok": true, "providers": names}),
        Err(e) => json!({"ok": false, "error": e}),
    };
    body
}

async fn fetch_provider(
    state: &SharedState,
    id: i64,
) -> Result<ProviderResponse, (StatusCode, Json<Value>)> {
    let row = state
        .db
        .get_web_search_provider(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("provider"))?;
    let instances = state
        .db
        .list_web_search_instances(id)
        .await
        .map_err(db_error)?;
    let default_id = state
        .db
        .default_web_search_provider()
        .await
        .map_err(db_error)?
        .map(|p| p.id);
    Ok(ProviderResponse::from_row(
        &row,
        instances,
        default_id,
        state.aigw_master_key.as_deref(),
    ))
}

async fn fetch_instance(
    state: &SharedState,
    id: i64,
) -> Result<InstanceResponse, (StatusCode, Json<Value>)> {
    let row = state
        .db
        .get_web_search_instance(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("instance"))?;
    Ok(InstanceResponse::from_row(
        row,
        state.aigw_master_key.as_deref(),
    ))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Providers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn list_providers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let rows = state
        .db
        .list_web_search_providers()
        .await
        .map_err(db_error)?;
    let default_id = state
        .db
        .default_web_search_provider()
        .await
        .map_err(db_error)?
        .map(|p| p.id);

    let mut data = Vec::with_capacity(rows.len());
    for row in &rows {
        let instances = state
            .db
            .list_web_search_instances(row.id)
            .await
            .map_err(db_error)?;
        data.push(ProviderResponse::from_row(
            row,
            instances,
            default_id,
            state.aigw_master_key.as_deref(),
        ));
    }
    let count = data.len();
    Ok(Json(json!({"data": data, "count": count})))
}

pub async fn create_provider(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<CreateProviderRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    validate_kind(&body.kind)?;
    if body.name.trim().is_empty() {
        return Err(bad_request("name must not be empty", "invalid_name"));
    }
    // Keep in step with the `DEFAULT 0.01` in 028_*.sql: the column default only
    // fires for rows the DB writes itself, and this path always supplies a value.
    let cost = body.cost_per_query.unwrap_or(0.01);
    validate_cost(cost)?;

    let name = body.name.trim().to_string();
    if state
        .db
        .get_web_search_provider_by_name(&name)
        .await
        .map_err(db_error)?
        .is_some()
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": {
                "message": format!("a web search provider named \"{name}\" already exists"),
                "type": "DUPLICATE_PROVIDER_NAME",
            }})),
        ));
    }

    let now = now_rfc3339();
    let row = WebSearchProviderRow {
        id: 0,
        name,
        kind: body.kind.clone(),
        cost_per_query: cost,
        enabled: body.enabled.unwrap_or(true),
        failover_order: body.failover_order,
        timeout_ms: body.timeout_ms,
        created_at: now.clone(),
        updated_at: now,
    };
    let id = state
        .db
        .create_web_search_provider(&row)
        .await
        .map_err(db_error)?;

    let outcome = reload_registry(&state).await;
    let created = fetch_provider(&state, id).await?;
    Ok(Json(with_reload(json!(created), outcome)))
}

pub async fn get_provider(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    Ok(Json(json!(fetch_provider(&state, id).await?)))
}

pub async fn update_provider(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProviderRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut row = state
        .db
        .get_web_search_provider(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("provider"))?;

    if let Some(kind) = body.kind.as_deref() {
        validate_kind(kind)?;
        row.kind = kind.to_string();
    }
    if let Some(cost) = body.cost_per_query {
        validate_cost(cost)?;
        row.cost_per_query = cost;
    }
    if let Some(name) = body.name.as_deref() {
        let name = name.trim();
        if name.is_empty() {
            return Err(bad_request("name must not be empty", "invalid_name"));
        }
        // `UNIQUE(name)` would reject this anyway, but a 409 that names the
        // conflict beats a raw constraint violation surfaced as a 500.
        if let Some(existing) = state
            .db
            .get_web_search_provider_by_name(name)
            .await
            .map_err(db_error)?
        {
            if existing.id != id {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({"error": {
                        "message": format!("a web search provider named \"{name}\" already exists"),
                        "type": "DUPLICATE_PROVIDER_NAME",
                    }})),
                ));
            }
        }
        row.name = name.to_string();
    }
    if let Some(on) = body.enabled {
        row.enabled = on;
    }
    if let Some(order) = body.failover_order {
        row.failover_order = Some(order);
    }
    if let Some(t) = body.timeout_ms {
        row.timeout_ms = Some(t);
    }
    row.updated_at = now_rfc3339();

    state
        .db
        .update_web_search_provider(&row)
        .await
        .map_err(db_error)?;

    let outcome = reload_registry(&state).await;
    let updated = fetch_provider(&state, id).await?;
    Ok(Json(with_reload(json!(updated), outcome)))
}

/// The provider that serves un-named requests cannot be deleted: there would be
/// nothing left to fall back to, and the failure would only show up on the next
/// search. This replaces the `proxies` in-use guard, which here has no cross-table
/// referent — after DB-ification "the default" is a property of this table.
async fn guard_default(state: &SharedState, id: i64) -> Result<(), (StatusCode, Json<Value>)> {
    let default_id = state
        .db
        .default_web_search_provider()
        .await
        .map_err(db_error)?
        .map(|p| p.id);
    if default_id == Some(id) {
        let name = state
            .db
            .get_web_search_provider(id)
            .await
            .ok()
            .flatten()
            .map(|p| p.name)
            .unwrap_or_default();
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": {
                "message": "this is the default web search provider; promote another provider first",
                "type": "WEB_SEARCH_PROVIDER_IN_USE",
                "referenced_by": [name],
            }})),
        ));
    }
    Ok(())
}

pub async fn delete_provider(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    if state
        .db
        .get_web_search_provider(id)
        .await
        .map_err(db_error)?
        .is_none()
    {
        return Err(not_found("provider"));
    }
    guard_default(&state, id).await?;

    state
        .db
        .delete_web_search_provider(id)
        .await
        .map_err(db_error)?;
    let outcome = reload_registry(&state).await;
    Ok(Json(with_reload(
        json!({"status": "deleted", "id": id}),
        outcome,
    )))
}

pub async fn batch_delete_providers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<BatchDeleteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    for id in body.ids {
        if let Err(_conflict) = guard_default(&state, id).await {
            skipped.push(json!({"id": id, "reason": "default_provider"}));
            continue;
        }
        match state.db.delete_web_search_provider(id).await {
            Ok(()) => deleted.push(id),
            Err(e) => skipped.push(json!({"id": id, "reason": e.to_string()})),
        }
    }
    let outcome = reload_registry(&state).await;
    Ok(Json(with_reload(
        json!({"status": "ok", "deleted_ids": deleted, "skipped": skipped}),
        outcome,
    )))
}

pub async fn toggle_provider(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut row = state
        .db
        .get_web_search_provider(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("provider"))?;
    row.enabled = !row.enabled;
    row.updated_at = now_rfc3339();
    state
        .db
        .update_web_search_provider(&row)
        .await
        .map_err(db_error)?;

    let outcome = reload_registry(&state).await;
    let updated = fetch_provider(&state, id).await?;
    Ok(Json(with_reload(json!(updated), outcome)))
}

/// Enable every provider and instance that a previous "disable all" turned off.
///
/// There is no global on/off flag: "the layer is on" means "some provider is
/// enabled and has an enabled instance", because the provider list has exactly
/// one source and a second switch would be a second source of truth. This is
/// the bulk counterpart of `toggle_provider`, so the UI's single switch has
/// something to call.
///
/// It enables **everything**, including rows that were individually disabled
/// before the bulk-off. "Restore" is not "undo" — there is no record of which
/// flag was set by which action, and inventing one would mean a per-row
/// previous-state column that nothing else needs.
pub async fn enable_all(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut enabled = 0usize;
    for p in state
        .db
        .list_web_search_providers()
        .await
        .map_err(db_error)?
    {
        let mut row = p;
        if !row.enabled {
            row.enabled = true;
            row.updated_at = now_rfc3339();
            state
                .db
                .update_web_search_provider(&row)
                .await
                .map_err(db_error)?;
            enabled += 1;
        }
        for mut i in state
            .db
            .list_web_search_instances(row.id)
            .await
            .map_err(db_error)?
        {
            if !i.enabled {
                i.enabled = true;
                i.updated_at = now_rfc3339();
                state
                    .db
                    .update_web_search_instance(&i)
                    .await
                    .map_err(db_error)?;
                enabled += 1;
            }
        }
    }
    let outcome = reload_registry(&state).await;
    Ok(Json(with_reload(
        json!({"status": "ok", "enabled_rows": enabled}),
        outcome,
    )))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Instances
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Encrypt an incoming `api_key`, or `None` when it is absent/blank.
fn seal_api_key(
    raw: Option<&str>,
    master_key: Option<&str>,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(key) = raw.map(str::trim).filter(|k| !k.is_empty()) else {
        return Ok(None);
    };
    let mk = master_key.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {
                "message": "AIGW_MASTER_KEY is not configured — cannot encrypt the provider api_key",
                "type": "master_key_missing",
            }})),
        )
    })?;
    encrypt_litellm_value_gcm(key, mk)
        .map(Some)
        .map_err(|e| bad_request(format!("could not encrypt api_key: {e}"), "encrypt_failed"))
}

fn validate_base_url(url: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if url.trim().is_empty() {
        return Err(bad_request(
            "base_url must not be empty",
            "invalid_base_url",
        ));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(bad_request(
            "base_url must start with http:// or https://",
            "invalid_base_url",
        ));
    }
    Ok(())
}

pub async fn list_instances(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(provider_id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    if state
        .db
        .get_web_search_provider(provider_id)
        .await
        .map_err(db_error)?
        .is_none()
    {
        return Err(not_found("provider"));
    }
    let rows = state
        .db
        .list_web_search_instances(provider_id)
        .await
        .map_err(db_error)?;
    let data: Vec<InstanceResponse> = rows
        .into_iter()
        .map(|r| InstanceResponse::from_row(r, state.aigw_master_key.as_deref()))
        .collect();
    let count = data.len();
    Ok(Json(json!({"data": data, "count": count})))
}

pub async fn create_instance(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(provider_id): Path<i64>,
    Json(body): Json<InstanceRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    if state
        .db
        .get_web_search_provider(provider_id)
        .await
        .map_err(db_error)?
        .is_none()
    {
        return Err(not_found("provider"));
    }
    if body.name.trim().is_empty() {
        return Err(bad_request("name must not be empty", "invalid_name"));
    }
    validate_base_url(&body.base_url)?;
    let api_key = seal_api_key(body.api_key.as_deref(), state.aigw_master_key.as_deref())?;

    let now = now_rfc3339();
    let row = WebSearchInstanceRow {
        id: 0,
        provider_id,
        name: body.name.trim().to_string(),
        base_url: body.base_url.trim_end_matches('/').to_string(),
        api_key,
        weight: body.weight,
        enabled: body.enabled.unwrap_or(true),
        probe_result: json!({}),
        created_at: now.clone(),
        updated_at: now,
    };
    let id = state
        .db
        .create_web_search_instance(&row)
        .await
        .map_err(db_error)?;

    let outcome = reload_registry(&state).await;
    let created = fetch_instance(&state, id).await?;
    Ok(Json(with_reload(json!(created), outcome)))
}

pub async fn get_instance(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    Ok(Json(json!(fetch_instance(&state, id).await?)))
}

/// Update an instance.
///
/// `api_key` follows the partial-update convention: omitted leaves the stored
/// credential alone, an empty string clears it, a value replaces it. Wiping a
/// key because a client did not resend it would be a silent outage on the next
/// search.
pub async fn update_instance(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<InstanceRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut row = state
        .db
        .get_web_search_instance(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("instance"))?;

    if body.name.trim().is_empty() {
        return Err(bad_request("name must not be empty", "invalid_name"));
    }
    validate_base_url(&body.base_url)?;
    if let Some(raw) = body.api_key.as_deref() {
        row.api_key = seal_api_key(Some(raw), state.aigw_master_key.as_deref())?;
    }
    row.name = body.name.trim().to_string();
    row.base_url = body.base_url.trim_end_matches('/').to_string();
    row.weight = body.weight;
    if let Some(on) = body.enabled {
        row.enabled = on;
    }
    row.updated_at = now_rfc3339();

    state
        .db
        .update_web_search_instance(&row)
        .await
        .map_err(db_error)?;

    let outcome = reload_registry(&state).await;
    let updated = fetch_instance(&state, id).await?;
    Ok(Json(with_reload(json!(updated), outcome)))
}

pub async fn delete_instance(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    if state
        .db
        .get_web_search_instance(id)
        .await
        .map_err(db_error)?
        .is_none()
    {
        return Err(not_found("instance"));
    }
    state
        .db
        .delete_web_search_instance(id)
        .await
        .map_err(db_error)?;
    let outcome = reload_registry(&state).await;
    Ok(Json(with_reload(
        json!({"status": "deleted", "id": id}),
        outcome,
    )))
}

pub async fn toggle_instance(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut row = state
        .db
        .get_web_search_instance(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("instance"))?;
    row.enabled = !row.enabled;
    row.updated_at = now_rfc3339();
    state
        .db
        .update_web_search_instance(&row)
        .await
        .map_err(db_error)?;

    let outcome = reload_registry(&state).await;
    let updated = fetch_instance(&state, id).await?;
    Ok(Json(with_reload(json!(updated), outcome)))
}

pub async fn test_instance(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let instance = state
        .db
        .get_web_search_instance(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("instance"))?;
    let provider = state
        .db
        .get_web_search_provider(instance.provider_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("provider"))?;

    let yaml = state.web_search.yaml_base();
    let proxy = yaml
        .as_ref()
        .map(|c| c.proxy_url.clone())
        .filter(|u| !u.trim().is_empty());
    let global_timeout = yaml.as_ref().map(|c| c.timeout_ms).unwrap_or(5000);
    let timeout_ms = provider
        .timeout_ms
        .filter(|t| *t > 0)
        .map(|t| t as u64)
        .unwrap_or(global_timeout);

    let cfg = aigw_core::websearch::WebSearchInstanceConfig {
        base_url: instance.base_url.clone(),
        api_key: instance.api_key.clone(),
        weight: instance.weight,
        enabled: instance.enabled,
    };

    let outcome = probe_instance(
        &provider.kind,
        &provider.name,
        &cfg,
        state.aigw_master_key.as_deref().unwrap_or_default(),
        proxy.as_deref(),
        timeout_ms,
    )
    .await;

    let probed_at = now_rfc3339();
    let snapshot = outcome.to_json(&probed_at);
    state
        .db
        .update_web_search_instance_probe(id, &snapshot, &probed_at)
        .await
        .map_err(db_error)?;

    Ok(Json(json!({
        "id": id,
        "ok": outcome.ok,
        "http_status": outcome.http_status,
        "latency_ms": outcome.latency_ms,
        "result_count": outcome.result_count,
        "parse_ok": outcome.parse_ok,
        "error": outcome.error,
        "probed_at": probed_at,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Reload endpoint
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Writes already reload on their own; this exists for the case where the
/// operator changed something the service could not see — a `config.yaml`
/// mount, most commonly.
pub async fn reload(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    match reload_registry(&state).await {
        Ok(providers) => Ok(Json(json!({
            "status": "ok",
            "providers": providers,
            "enabled": !providers.is_empty(),
        }))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {
                "message": format!("reload failed; the previous registry is still in use: {e}"),
                "type": "reload_failed",
            }})),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::{AppState, DEFAULT_KEY_TOKEN_LEN};
    use axum::Router;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    const MK: &str = "sk-master-test";

    async fn test_state() -> SharedState {
        let db = aigw_core::db::Database::init("sqlite::memory:")
            .await
            .unwrap();
        Arc::new(AppState {
            resolver: aigw_core::resolver::ModelResolver::new(db.clone(), None, "onprem"),
            router: aigw_core::router::Router::default(),
            db,
            master_key: Some(MK.to_string()),
            aigw_master_key: Some(MK.to_string()),
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
            token_provider: Arc::new(aigw_core::claude_token::TokenProvider::new()),
            web_search: aigw_core::websearch::WebSearchHandle::disabled(),
            mcp_disable_rest: false,
            metrics: None,
        })
    }

    fn build_router(state: SharedState) -> Router {
        use axum::routing::{get, post};
        Router::new()
            .route(
                "/admin/websearch/providers",
                get(list_providers).post(create_provider),
            )
            .route(
                "/admin/websearch/providers/batch-delete",
                post(batch_delete_providers),
            )
            .route(
                "/admin/websearch/providers/{id}",
                get(get_provider)
                    .put(update_provider)
                    .delete(delete_provider),
            )
            .route(
                "/admin/websearch/providers/{id}/toggle",
                post(toggle_provider),
            )
            .route(
                "/admin/websearch/providers/{id}/instances",
                get(list_instances).post(create_instance),
            )
            .route(
                "/admin/websearch/instances/{id}",
                get(get_instance)
                    .put(update_instance)
                    .delete(delete_instance),
            )
            .route(
                "/admin/websearch/instances/{id}/toggle",
                post(toggle_instance),
            )
            .route("/admin/websearch/instances/{id}/test", post(test_instance))
            .route("/admin/websearch/reload", post(reload))
            .route("/admin/websearch/enable-all", post(enable_all))
            .with_state(state)
    }

    async fn send(
        app: &Router,
        method: axum::http::Method,
        uri: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {MK}"));
        let rb = req
            .body(axum::body::Body::from(
                body.map(|b| b.to_string()).unwrap_or_default(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(rb).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), 65_536)
            .await
            .unwrap_or_default();
        let json = serde_json::from_slice(&bytes).unwrap_or(json!({}));
        (status, json)
    }

    async fn create_provider_helper(app: &Router, name: &str, order: Option<i64>) -> i64 {
        let (s, body) = send(
            app,
            axum::http::Method::POST,
            "/admin/websearch/providers",
            Some(json!({"name": name, "kind": "searxng", "failover_order": order})),
        )
        .await;
        assert_eq!(s, 200, "create provider failed: {body}");
        body["id"].as_i64().expect("provider id")
    }

    async fn add_instance_helper(
        app: &Router,
        provider_id: i64,
        name: &str,
        key: Option<&str>,
    ) -> i64 {
        let (s, body) = send(
            app,
            axum::http::Method::POST,
            &format!("/admin/websearch/providers/{provider_id}/instances"),
            Some(json!({
                "name": name,
                "base_url": format!("http://{name}.internal:8080"),
                "api_key": key,
            })),
        )
        .await;
        assert_eq!(s, 200, "add instance failed: {body}");
        body["id"].as_i64().expect("instance id")
    }

    #[tokio::test]
    async fn create_and_list_providers_roundtrip() {
        let app = build_router(test_state().await);
        let id = create_provider_helper(&app, "prov-a", Some(0)).await;
        add_instance_helper(&app, id, "inst-1", None).await;

        let (s, body) = send(
            &app,
            axum::http::Method::GET,
            "/admin/websearch/providers",
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["count"], 1);
        assert_eq!(body["data"][0]["name"], "prov-a");
        assert_eq!(body["data"][0]["kind"], "searxng");
        assert_eq!(
            body["data"][0]["is_default"], true,
            "only provider is the default"
        );
        assert_eq!(body["data"][0]["instances"][0]["name"], "inst-1");
        assert_eq!(
            body["data"][0]["cost_per_query"], 0.01,
            "the column default must be applied on the create path too"
        );
    }

    /// The key must never come back. It is stored encrypted and the response
    /// carries only a masked tail plus a boolean.
    #[tokio::test]
    async fn api_key_is_encrypted_at_rest_and_redacted_in_responses() {
        let state = test_state().await;
        let db = state.db.clone();
        let app = build_router(state);
        let id = create_provider_helper(&app, "p", Some(0)).await;
        let iid = add_instance_helper(&app, id, "i", Some("sk-secret-abcdef")).await;

        // At rest: ciphertext, not the plaintext.
        let row = db.get_web_search_instance(iid).await.unwrap().unwrap();
        let stored = row.api_key.expect("key stored");
        assert!(stored.starts_with("v2:gcm:"), "stored form was {stored}");
        assert!(!stored.contains("sk-secret-abcdef"));

        // In responses: masked, and `has_api_key` tells the UI one is set.
        let (s, body) = send(
            &app,
            axum::http::Method::GET,
            &format!("/admin/websearch/instances/{iid}"),
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["has_api_key"], true);
        let redacted = body["redacted_api_key"].as_str().unwrap();
        assert!(redacted.starts_with("****"), "{redacted}");
        assert!(redacted.ends_with("cdef"), "{redacted}");
        assert!(
            !serde_json::to_string(&body)
                .unwrap()
                .contains("sk-secret-abcdef"),
            "the plaintext key must not appear anywhere in the response"
        );
    }

    /// Omitting `api_key` on update must leave the stored credential alone —
    /// wiping it because a client did not resend it would break the next search.
    #[tokio::test]
    async fn update_instance_without_api_key_keeps_the_stored_one() {
        let state = test_state().await;
        let db = state.db.clone();
        let app = build_router(state);
        let id = create_provider_helper(&app, "p", Some(0)).await;
        let iid = add_instance_helper(&app, id, "i", Some("sk-keep-me")).await;
        let before = db
            .get_web_search_instance(iid)
            .await
            .unwrap()
            .unwrap()
            .api_key;

        let (s, _) = send(
            &app,
            axum::http::Method::PUT,
            &format!("/admin/websearch/instances/{iid}"),
            Some(json!({"name": "i-renamed", "base_url": "http://new.internal:8080"})),
        )
        .await;
        assert_eq!(s, 200);

        let after = db.get_web_search_instance(iid).await.unwrap().unwrap();
        assert_eq!(after.name, "i-renamed");
        assert_eq!(after.api_key, before, "omitted api_key must not clear it");
    }

    #[tokio::test]
    async fn unsupported_kind_is_rejected_with_a_hint() {
        let app = build_router(test_state().await);

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/providers",
            Some(json!({"name": "t", "kind": "tavily"})),
        )
        .await;
        assert_eq!(s, 400);
        let msg = body["error"]["message"].as_str().unwrap();
        assert!(msg.contains("tavily"), "{msg}");
        assert!(
            msg.contains("searxng"),
            "must list what IS supported: {msg}"
        );
        assert!(
            msg.contains("planned"),
            "a planned vendor must read differently from a typo: {msg}"
        );
        assert_eq!(body["error"]["type"], "unsupported_provider_kind");

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/providers",
            Some(json!({"name": "x", "kind": "exa"})),
        )
        .await;
        assert_eq!(s, 400);
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("unknown"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn duplicate_provider_name_is_rejected() {
        let app = build_router(test_state().await);
        create_provider_helper(&app, "dup", None).await;
        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/providers",
            Some(json!({"name": "dup", "kind": "searxng"})),
        )
        .await;
        assert_eq!(s, 409, "{body}");
        assert_eq!(body["error"]["type"], "DUPLICATE_PROVIDER_NAME");
    }

    #[tokio::test]
    async fn invalid_cost_is_rejected() {
        let app = build_router(test_state().await);
        // Only a negative finite value is expressible here: JSON has no NaN or
        // Infinity literal, so `json!({"cost": f64::NAN})` serializes to `null`,
        // which lands as "field omitted" and takes the 0.01 default. Non-finite
        // input is covered directly in `non_finite_cost_is_rejected` below.
        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/providers",
            Some(json!({"name": "c", "kind": "searxng", "cost_per_query": -1.0})),
        )
        .await;
        assert_eq!(s, 400, "a negative cost must be refused: {body}");
        assert_eq!(body["error"]["type"], "invalid_cost_per_query");
    }

    /// `validate_cost` must also refuse non-finite values. They cannot reach the
    /// handler through JSON (no literal exists, and `serde_json` rejects an
    /// overflowing literal like `1e400` as out-of-range before the extractor
    /// runs), but the guard stays because the function is the single gate any
    /// caller — present or future — goes through.
    #[test]
    fn non_finite_cost_is_rejected() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = validate_cost(bad).expect_err("non-finite cost must be refused");
            assert_eq!(err.0, StatusCode::BAD_REQUEST, "cost {bad}");
        }
        assert!(validate_cost(0.0).is_ok(), "free is allowed");
        assert!(validate_cost(0.25).is_ok());
    }

    /// The default provider is the one that serves un-named requests; deleting
    /// it would only fail later, on the next search.
    #[tokio::test]
    async fn default_provider_cannot_be_deleted() {
        let app = build_router(test_state().await);
        let first = create_provider_helper(&app, "first", Some(0)).await;
        let second = create_provider_helper(&app, "second", Some(1)).await;

        let (s, body) = send(
            &app,
            axum::http::Method::DELETE,
            &format!("/admin/websearch/providers/{first}"),
            None,
        )
        .await;
        assert_eq!(s, 409, "{body}");
        assert_eq!(body["error"]["type"], "WEB_SEARCH_PROVIDER_IN_USE");

        // The non-default one is deletable.
        let (s, _) = send(
            &app,
            axum::http::Method::DELETE,
            &format!("/admin/websearch/providers/{second}"),
            None,
        )
        .await;
        assert_eq!(s, 200);
    }

    #[tokio::test]
    async fn batch_delete_skips_the_default() {
        let app = build_router(test_state().await);
        let first = create_provider_helper(&app, "first", Some(0)).await;
        let second = create_provider_helper(&app, "second", Some(1)).await;

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/providers/batch-delete",
            Some(json!({"ids": [first, second]})),
        )
        .await;
        assert_eq!(s, 200, "{body}");
        assert_eq!(body["deleted_ids"], json!([second]));
        assert_eq!(body["skipped"][0]["id"], first);
        assert_eq!(body["skipped"][0]["reason"], "default_provider");
    }

    #[tokio::test]
    async fn deleting_a_provider_removes_its_instances() {
        let state = test_state().await;
        let db = state.db.clone();
        let app = build_router(state);
        let keep = create_provider_helper(&app, "keep", Some(0)).await;
        let doomed = create_provider_helper(&app, "doomed", Some(1)).await;
        add_instance_helper(&app, doomed, "gone", None).await;

        let (s, _) = send(
            &app,
            axum::http::Method::DELETE,
            &format!("/admin/websearch/providers/{doomed}"),
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert!(
            db.list_web_search_instances(doomed)
                .await
                .unwrap()
                .is_empty(),
            "instances must not outlive their provider"
        );
        assert!(db.get_web_search_provider(keep).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn toggle_flips_enabled_and_reloads() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_provider_helper(&app, "p", Some(0)).await;
        add_instance_helper(&app, id, "i", None).await;
        assert!(
            state.web_search.is_enabled(),
            "one provider means the layer is live"
        );

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/websearch/providers/{id}/toggle"),
            None,
        )
        .await;
        assert_eq!(s, 200, "{body}");
        assert_eq!(body["enabled"], false);
        assert!(
            !state
                .db
                .get_web_search_provider(id)
                .await
                .unwrap()
                .unwrap()
                .enabled
        );
        assert!(
            !state.web_search.is_enabled(),
            "disabling the last provider turns the layer off"
        );
    }

    /// The bulk counterpart: one click brings back every row the bulk-off took
    /// down, and the layer is live again without a restart.
    #[tokio::test]
    async fn enable_all_brings_the_layer_back() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_provider_helper(&app, "p", Some(0)).await;
        let iid = add_instance_helper(&app, id, "i", None).await;

        // Take both the provider and its instance down.
        for path in [
            format!("/admin/websearch/providers/{id}/toggle"),
            format!("/admin/websearch/instances/{iid}/toggle"),
        ] {
            let (s, body) = send(&app, axum::http::Method::POST, &path, None).await;
            assert_eq!(s, 200, "{body}");
        }
        assert!(
            !state.web_search.is_enabled(),
            "both rows off means the layer is off"
        );

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/enable-all",
            None,
        )
        .await;
        assert_eq!(s, 200, "{body}");
        assert_eq!(body["enabled_rows"], 2, "provider + instance");
        assert_eq!(body["reload"]["ok"], true);
        assert!(
            state.web_search.is_enabled(),
            "the layer must be live again without a restart"
        );
        assert!(
            state
                .db
                .get_web_search_instance(iid)
                .await
                .unwrap()
                .unwrap()
                .enabled,
            "the instance, not only the provider, must come back"
        );
    }

    /// The point of the whole stage: change the price and the running registry
    /// uses the new one without a restart.
    #[tokio::test]
    async fn price_change_is_live_without_restart() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_provider_helper(&app, "p", Some(0)).await;
        add_instance_helper(&app, id, "i", None).await;

        let before = state
            .web_search
            .current()
            .expect("live registry")
            .cost_per_query(None)
            .expect("price");
        assert_eq!(before, 0.01);

        let (s, _) = send(
            &app,
            axum::http::Method::PUT,
            &format!("/admin/websearch/providers/{id}"),
            Some(json!({"cost_per_query": 0.000123})),
        )
        .await;
        assert_eq!(s, 200);

        let after = state
            .web_search
            .current()
            .expect("live registry")
            .cost_per_query(None)
            .expect("price");
        assert_eq!(after, 0.000123, "the swap must be visible to readers");
    }

    /// A failed reload must leave the working registry alone: dropping to
    /// `None` would silently turn search off because of one bad row.
    #[tokio::test]
    async fn failed_reload_keeps_the_previous_registry() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_provider_helper(&app, "p", None).await;
        add_instance_helper(&app, id, "i", None).await;
        assert!(state.web_search.is_enabled());

        // A row with a kind that has no client, written straight to the DB — the
        // API would refuse this, which is exactly why the loader also has to
        // tolerate it.
        let now = now_rfc3339();
        state
            .db
            .create_web_search_provider(&WebSearchProviderRow {
                id: 0,
                name: "bad".to_string(),
                kind: "tavily".to_string(),
                cost_per_query: 0.01,
                enabled: true,
                failover_order: Some(9),
                timeout_ms: None,
                created_at: now.clone(),
                updated_at: now,
            })
            .await
            .unwrap();

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/reload",
            None,
        )
        .await;
        // The bad row is skipped by the loader, so the reload still succeeds —
        // and the good provider is still there.
        assert_eq!(s, 200, "{body}");
        let reg = state.web_search.current().expect("still enabled");
        assert_eq!(reg.provider_names(), vec!["p"]);
    }

    #[tokio::test]
    async fn unknown_ids_are_404() {
        let app = build_router(test_state().await);
        for uri in [
            "/admin/websearch/providers/999",
            "/admin/websearch/instances/999",
        ] {
            let (s, _) = send(&app, axum::http::Method::GET, uri, None).await;
            assert_eq!(s, 404, "{uri}");
        }
        let (s, _) = send(
            &app,
            axum::http::Method::POST,
            "/admin/websearch/providers/999/instances",
            Some(json!({"name": "i", "base_url": "http://x.internal:8080"})),
        )
        .await;
        assert_eq!(s, 404);
    }

    #[tokio::test]
    async fn base_url_must_be_http() {
        let app = build_router(test_state().await);
        let id = create_provider_helper(&app, "p", None).await;
        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/websearch/providers/{id}/instances"),
            Some(json!({"name": "i", "base_url": "ftp://nope"})),
        )
        .await;
        assert_eq!(s, 400, "{body}");
        assert_eq!(body["error"]["type"], "invalid_base_url");
    }

    /// The probe reports what it learned *and* records it, so the list can show
    /// the last known state without re-probing.
    #[tokio::test]
    async fn probe_writes_a_snapshot_and_reports_the_outcome() {
        let state = test_state().await;
        let db = state.db.clone();
        let app = build_router(state);
        let id = create_provider_helper(&app, "p", None).await;
        let iid = add_instance_helper(&app, id, "i", None).await;

        let (s, body) = send(
            &app,
            axum::http::Method::POST,
            &format!("/admin/websearch/instances/{iid}/test"),
            None,
        )
        .await;
        assert_eq!(s, 200, "{body}");
        assert_eq!(body["ok"], false, "nothing is listening on that port");
        assert!(!body["error"].is_null(), "{body}");

        let stored = db.get_web_search_instance(iid).await.unwrap().unwrap();
        assert_eq!(stored.probe_result["ok"], false);
        assert!(stored.probe_result.get("probed_at").is_some());
        assert_eq!(stored.probe_result["probed_at"], body["probed_at"]);
    }
}
