//! MCP server registry administration — /admin/mcp/* (Stage 141).
//!
//! Endpoints:
//! - GET    /admin/mcp/servers                     — list (paged, status/search/sort, tool_count)
//! - GET    /admin/mcp/servers/all                 — every server (alias dropdown)
//! - POST   /admin/mcp/servers                     — create
//! - POST   /admin/mcp/servers/batch-delete        — batch delete
//! - GET    /admin/mcp/servers/{id}                — detail (with tool_count)
//! - PUT    /admin/mcp/servers/{id}                — update
//! - DELETE /admin/mcp/servers/{id}                — delete, no in-use guard
//! - POST   /admin/mcp/servers/{id}/toggle         — enable ↔ disable
//! - POST   /admin/mcp/servers/{id}/probe          — dual-stack negotiation + tools/list
//! - GET    /admin/mcp/servers/{id}/tools          — stored catalog snapshot
//! - GET    /admin/mcp/servers/{id}/tokens         — upstream credentials (redacted)
//! - POST   /admin/mcp/servers/{id}/tokens         — add a credential (sealed)
//! - DELETE /admin/mcp/servers/{id}/tokens/{tid}   — remove a credential
//! - POST   /admin/mcp/servers/{id}/tokens/{tid}/default — make it the owner's default
//! - POST   /admin/mcp/reload                      — re-probe every enabled server
//!
//! Deleting a server takes no in-use guard: unlike a proxy, no registered model
//! depends on an MCP server — the only consequence is one fewer `{alias}__` tool
//! prefix on the next `tools/list`. A probe failure likewise leaves the previous
//! snapshot in place, so an upstream hiccup cannot empty the tool catalogue.

use aigw_core::db::McpCallFilter;
use aigw_core::mcp::acl::{is_known_scope, ToolRule};
use aigw_core::mcp::catalog_cache::{self, DEFAULT_TTL_MS};
use aigw_core::mcp::credential::{is_token_usable, seal_token, unseal_token};
use aigw_core::mcp::{probe_server, validate_alias, ProbeError};
use aigw_core::models::{
    CatalogSnapshot, CreateMcpServerRequest, CredentialScope, McpServerRow, McpServerTokenRow,
    McpTokenResponse, McpToolRow, UpdateMcpServerRequest, MCP_OWNER_SERVER, MCP_OWNER_USER,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{info, warn};

use super::spend::{require_admin, SpendAuth};
use crate::routes::keys::SharedState;

const DEFAULT_TRANSPORT: &str = "streamable_http";
/// `display_name` and `token_name` are `VARCHAR(255)` and `transport` is
/// `VARCHAR(32)` on MySQL. Over-length input fails the INSERT under strict mode
/// (error 1406) and surfaces as a 500, while SQLite and PostgreSQL accept it —
/// so the limit is enforced here, before the row reaches any dialect.
pub(crate) const MAX_NAME_LEN: usize = 255;
const MAX_TRANSPORT_LEN: usize = 32;

#[derive(Debug, Deserialize)]
pub struct McpListQuery {
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

#[derive(Debug, Deserialize)]
pub struct CreateMcpTokenBody {
    pub token_name: String,
    pub token_value: String,
    #[serde(default)]
    pub owner_type: Option<String>,
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub expires_at: Option<String>,
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

fn duplicate_token_name(token_name: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::CONFLICT,
        Json(json!({"error": {
            "message": format!("token \"{token_name}\" already exists for this owner"),
            "type": "DUPLICATE_TOKEN_NAME",
        }})),
    )
}

/// The duplicate check above is a read-then-write with no lock, so two concurrent
/// creates for the same `(server, owner, token_name)` can both pass it. The
/// unique index is then the real arbiter and the loser gets a SQL error — which
/// must come back as the same 409 the check promises, not a 500. Detection is by
/// SQLSTATE rather than driver kind so all three dialects land here.
fn is_unique_violation(e: &aigw_core::db::DbError) -> bool {
    use aigw_core::db::DbError;
    let DbError::Sql(sqlx::Error::Database(db)) = e else {
        return false;
    };
    matches!(db.code().as_deref(), Some("23505" | "1062" | "2067"))
}

fn master_key_missing() -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": {
            "message": "AIGW_MASTER_KEY is not configured — cannot encrypt or decrypt MCP tokens",
            "type": "master_key_missing",
        }})),
    )
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn validate_url(url: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if url.trim().is_empty() {
        return Err(bad_request("url must not be empty", "invalid_url"));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(bad_request(
            "url must start with http:// or https://",
            "invalid_url",
        ));
    }
    Ok(())
}

pub(crate) fn validate_len(
    value: &str,
    field: &str,
    max: usize,
    kind: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    if value.chars().count() > max {
        return Err(bad_request(
            format!("{field} must be at most {max} characters"),
            kind,
        ));
    }
    Ok(())
}

/// An `expires_at` the token store can compare against. An unparseable string
/// would still be stored, and `is_token_usable` reads it as expired — the token
/// would be dead on arrival with no error anywhere.
fn normalize_expires_at(raw: Option<&str>) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    let Some(raw) = raw.map(str::trim).filter(|e| !e.is_empty()) else {
        return Ok(None);
    };
    chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| {
        bad_request(
            format!("expires_at must be an RFC 3339 timestamp, got \"{raw}\""),
            "invalid_expires_at",
        )
    })?;
    Ok(Some(raw.to_string()))
}

async fn fetch_server(
    state: &SharedState,
    id: i64,
) -> Result<McpServerRow, (StatusCode, Json<Value>)> {
    state
        .db
        .get_mcp_server(id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("MCP server"))
}

/// Attach the catalog size to a single-server read. The paged list gets the same
/// number from the `LEFT JOIN` aggregate; the detail path has one server, so one
/// extra cheap query is simpler than a second SELECT shape that must stay in
/// step with the first.
async fn with_tool_count(
    state: &SharedState,
    mut server: McpServerRow,
) -> Result<McpServerRow, (StatusCode, Json<Value>)> {
    server.tool_count = state
        .db
        .count_mcp_tool_catalog(server.id)
        .await
        .map_err(db_error)?;
    Ok(server)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Servers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn list_mcp_servers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<McpListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
    let offset = (page as i64 - 1) * page_size as i64;
    let limit = page_size as i64;

    let (rows, total_count) = tokio::try_join!(
        state.db.list_mcp_servers_paged(
            limit,
            offset,
            query.status.as_deref(),
            query.search.as_deref(),
            query.sort_by.as_deref(),
            query.sort_order.as_deref(),
        ),
        state
            .db
            .count_mcp_servers(query.status.as_deref(), query.search.as_deref()),
    )
    .map_err(db_error)?;

    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    Ok(Json(json!({
        "object": "list",
        "data": rows,
        "count": rows.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

pub async fn list_all_mcp_servers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let rows = state.db.list_mcp_servers().await.map_err(db_error)?;
    Ok(Json(
        json!({"object": "list", "data": rows, "count": rows.len()}),
    ))
}

pub async fn create_mcp_server(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<CreateMcpServerRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    validate_alias(&body.alias)
        .map_err(|e| bad_request(format!("invalid alias: {e}"), "invalid_alias"))?;
    if body.display_name.trim().is_empty() {
        return Err(bad_request(
            "display_name must not be empty",
            "invalid_display_name",
        ));
    }
    validate_len(
        body.display_name.trim(),
        "display_name",
        MAX_NAME_LEN,
        "invalid_display_name",
    )?;
    validate_url(&body.url)?;
    let transport = body
        .transport
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(DEFAULT_TRANSPORT)
        .to_string();
    validate_len(
        &transport,
        "transport",
        MAX_TRANSPORT_LEN,
        "invalid_transport",
    )?;

    if state
        .db
        .get_mcp_server_by_alias(&body.alias)
        .await
        .map_err(db_error)?
        .is_some()
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": {
                "message": format!("an MCP server with alias \"{}\" already exists", body.alias),
                "type": "DUPLICATE_ALIAS",
            }})),
        ));
    }

    let now = now_rfc3339();
    let row = McpServerRow {
        id: 0,
        alias: body.alias.clone(),
        display_name: body.display_name.trim().to_string(),
        url: body.url.trim().to_string(),
        transport,
        credential_scope: body
            .credential_scope
            .unwrap_or(CredentialScope::DEFAULT)
            .as_str()
            .to_string(),
        enabled: body.enabled.unwrap_or(true),
        protocol_version: None,
        tool_count: 0,
        created_at: now.clone(),
        updated_at: now,
    };
    let id = state.db.create_mcp_server(&row).await.map_err(db_error)?;
    let created = fetch_server(&state, id).await?;
    Ok(Json(serde_json::to_value(created).unwrap_or(json!({}))))
}

pub async fn get_mcp_server(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let server = fetch_server(&state, id).await?;
    let server = with_tool_count(&state, server).await?;
    Ok(Json(serde_json::to_value(server).unwrap_or(json!({}))))
}

pub async fn update_mcp_server(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<UpdateMcpServerRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut server = fetch_server(&state, id).await?;

    if let Some(name) = body.display_name {
        if name.trim().is_empty() {
            return Err(bad_request(
                "display_name must not be empty",
                "invalid_display_name",
            ));
        }
        validate_len(
            name.trim(),
            "display_name",
            MAX_NAME_LEN,
            "invalid_display_name",
        )?;
        server.display_name = name.trim().to_string();
    }
    if let Some(url) = body.url {
        validate_url(&url)?;
        let url = url.trim().to_string();
        // A different endpoint means the stored snapshot describes a different
        // upstream, and the negotiated version no longer applies either.
        if server.url != url {
            server.protocol_version = None;
            catalog_cache::invalidate(id);
        }
        server.url = url;
    }
    if let Some(scope) = body.credential_scope {
        server.credential_scope = scope.as_str().to_string();
    }
    if let Some(enabled) = body.enabled {
        server.enabled = enabled;
    }
    server.updated_at = now_rfc3339();

    state
        .db
        .update_mcp_server(&server)
        .await
        .map_err(db_error)?;
    let updated = fetch_server(&state, id).await?;
    let updated = with_tool_count(&state, updated).await?;
    Ok(Json(serde_json::to_value(updated).unwrap_or(json!({}))))
}

pub async fn delete_mcp_server(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server(&state, id).await?;
    state.db.delete_mcp_server(id).await.map_err(db_error)?;
    catalog_cache::invalidate(id);
    Ok(Json(json!({"status": "deleted"})))
}

pub async fn batch_delete_mcp_servers(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Json(body): Json<BatchDeleteBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut deleted_ids = Vec::with_capacity(body.ids.len());
    let mut skipped = Vec::new();
    for id in body.ids {
        match state.db.get_mcp_server(id).await.map_err(db_error)? {
            Some(_) => {
                state.db.delete_mcp_server(id).await.map_err(db_error)?;
                catalog_cache::invalidate(id);
                deleted_ids.push(id);
            }
            None => skipped.push(json!({"id": id, "reason": "not_found"})),
        }
    }
    Ok(Json(json!({
        "status": "ok",
        "deleted_ids": deleted_ids,
        "skipped": skipped,
    })))
}

pub async fn toggle_mcp_server(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let mut server = fetch_server(&state, id).await?;
    server.enabled = !server.enabled;
    server.updated_at = now_rfc3339();
    state
        .db
        .update_mcp_server(&server)
        .await
        .map_err(db_error)?;
    let server = with_tool_count(&state, server).await?;
    Ok(Json(serde_json::to_value(server).unwrap_or(json!({}))))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Probe
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// The credential a probe puts on the wire.
///
/// `user_required` gets none: the admin probe has no user identity, and falling
/// back to the shared token would make the probe succeed against a server the
/// actual caller cannot reach.
async fn probe_bearer(
    state: &SharedState,
    server: &McpServerRow,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    if server.scope() == CredentialScope::UserRequired {
        return Ok(None);
    }
    let stored = state
        .db
        .default_mcp_token(server.id, MCP_OWNER_SERVER, "")
        .await
        .map_err(db_error)?;
    let Some(token) = stored.filter(|t| is_token_usable(t, chrono::Utc::now())) else {
        return Ok(None);
    };
    let mk = state
        .aigw_master_key
        .as_deref()
        .ok_or_else(master_key_missing)?;
    match unseal_token(&token.token_value, mk) {
        Ok(plain) => Ok(Some(plain)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {
                "message": format!("could not decrypt token {}: {e}", token.id),
                "type": "token_unseal_failed",
            }})),
        )),
    }
}

fn probe_failed(error: &ProbeError) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error": {
            "message": error.to_string(),
            "type": "probe_failed",
            "upstream_status": error.upstream_status(),
            "body": error.body(),
        }})),
    )
}

/// Probe one server and, on success, replace its snapshot.
///
/// The snapshot replacement happens only after a *successful* probe: an upstream
/// timeout must not cost the operator the catalogue it took a working probe to
/// collect. `others` is the tool-name set the conflict flag is computed against;
/// the reload path passes one set for every probe rather than reading
/// `mcp_tool_names_elsewhere` mid-loop, so two servers sharing a name both get
/// flagged instead of only the one probed second.
async fn probe_and_store(
    state: &SharedState,
    server: &McpServerRow,
    others: &[String],
) -> Result<(aigw_core::models::ProbeOutcome, Vec<McpToolRow>), (StatusCode, Json<Value>)> {
    let bearer = probe_bearer(state, server).await?;
    let outcome = probe_server(&server.url, bearer.as_deref(), others)
        .await
        .map_err(|e| probe_failed(&e))?;

    let fetched_at = now_rfc3339();
    let rows: Vec<McpToolRow> = outcome
        .tools
        .iter()
        .map(|tool| McpToolRow {
            id: 0,
            server_id: server.id,
            tool_name: tool.tool_name.clone(),
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            annotations: tool.annotations.clone(),
            has_conflict: tool.has_conflict,
            fetched_at: fetched_at.clone(),
        })
        .collect();

    state
        .db
        .replace_mcp_tool_catalog(server.id, &rows)
        .await
        .map_err(db_error)?;

    let mut updated = server.clone();
    updated.protocol_version = Some(outcome.protocol_version.clone());
    updated.updated_at = fetched_at.clone();
    state
        .db
        .update_mcp_server(&updated)
        .await
        .map_err(db_error)?;

    let ttl_ms = outcome
        .ttl_ms
        .filter(|ttl| *ttl > 0)
        .unwrap_or(DEFAULT_TTL_MS);
    catalog_cache::put(server.id, rows.clone(), &fetched_at, ttl_ms);
    info!(
        server_id = server.id,
        alias = %server.alias,
        tools = rows.len(),
        protocol_version = %outcome.protocol_version,
        "MCP probe succeeded"
    );

    Ok((outcome, rows))
}

pub async fn probe_mcp_server(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let server = fetch_server(&state, id).await?;
    let others = state
        .db
        .mcp_tool_names_elsewhere(server.id)
        .await
        .map_err(db_error)?;
    let (outcome, _) = probe_and_store(&state, &server, &others).await?;
    Ok(Json(serde_json::to_value(outcome).unwrap_or(json!({}))))
}

pub async fn list_mcp_server_tools(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server(&state, id).await?;
    let rows = state.db.list_mcp_tool_catalog(id).await.map_err(db_error)?;
    Ok(Json(
        serde_json::to_value(CatalogSnapshot::from_rows(&rows)).unwrap_or(json!({})),
    ))
}

/// Re-probe every enabled server.
///
/// Nothing is cleared first. Wiping the table would destroy the snapshot of a
/// disabled server (never re-probed here) and of an enabled server whose
/// upstream is briefly down — the same data a failed probe is supposed to
/// protect. Each successful probe replaces only its own server's rows.
///
/// Conflict sets are read once, before any probe writes, so two servers sharing
/// a tool name both come out flagged. Reading `mcp_tool_names_elsewhere` inside
/// the loop would let a server probed later see an already-replaced snapshot
/// while the earlier one saw the old one — asymmetric by loop order.
///
/// Disabled servers are skipped: they serve no traffic, so refreshing their
/// catalogue buys nothing, and a reload must not stall on an upstream an
/// operator deliberately switched off. Their snapshots survive untouched.
pub async fn reload_mcp(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let servers = state.db.list_mcp_servers().await.map_err(db_error)?;
    let mut conflict_sets = HashMap::new();
    for server in servers.iter().filter(|s| s.enabled) {
        let others = state
            .db
            .mcp_tool_names_elsewhere(server.id)
            .await
            .map_err(db_error)?;
        conflict_sets.insert(server.id, others);
    }

    let mut results = Vec::new();
    for server in servers.iter().filter(|s| s.enabled) {
        let others = conflict_sets.get(&server.id).cloned().unwrap_or_default();
        match probe_and_store(&state, server, &others).await {
            Ok((outcome, rows)) => results.push(json!({
                "id": server.id,
                "alias": server.alias,
                "ok": true,
                "protocol_version": outcome.protocol_version,
                "tool_count": rows.len(),
                "conflicts": outcome.conflicts,
            })),
            Err((status, body)) => {
                warn!(server_id = server.id, alias = %server.alias, "MCP reload probe failed");
                results.push(json!({
                    "id": server.id,
                    "alias": server.alias,
                    "ok": false,
                    "upstream_status": status.as_u16(),
                    "error": body.0,
                }));
            }
        }
    }
    let failed = results.iter().filter(|r| r["ok"] == json!(false)).count();
    Ok(Json(json!({
        "status": if failed == 0 { "ok" } else { "partial" },
        "probed": results.len(),
        "failed": failed,
        "servers": results,
    })))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tokens
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub async fn list_mcp_tokens(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server(&state, id).await?;
    let rows = state.db.list_mcp_tokens(id).await.map_err(db_error)?;
    let data: Vec<McpTokenResponse> = rows.iter().map(McpTokenResponse::from).collect();
    let count = data.len();
    Ok(Json(
        json!({"object": "list", "data": data, "count": count}),
    ))
}

pub async fn create_mcp_token(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<CreateMcpTokenBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server(&state, id).await?;

    if body.token_name.trim().is_empty() {
        return Err(bad_request(
            "token_name must not be empty",
            "invalid_token_name",
        ));
    }
    validate_len(
        body.token_name.trim(),
        "token_name",
        MAX_NAME_LEN,
        "invalid_token_name",
    )?;
    let plaintext = body.token_value.trim();
    if plaintext.is_empty() {
        return Err(bad_request(
            "token_value must not be empty",
            "invalid_token_value",
        ));
    }

    let owner_type = body
        .owner_type
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(MCP_OWNER_SERVER)
        .to_string();
    let owner_id = match owner_type.as_str() {
        MCP_OWNER_SERVER => String::new(),
        MCP_OWNER_USER => {
            let owner_id = body
                .owner_id
                .as_deref()
                .map(str::trim)
                .filter(|o| !o.is_empty())
                .ok_or_else(|| {
                    bad_request(
                        "owner_id is required when owner_type is \"user\"",
                        "invalid_owner_id",
                    )
                })?
                .to_string();
            if state
                .db
                .get_user_by_id(&owner_id)
                .await
                .map_err(db_error)?
                .is_none()
            {
                return Err(bad_request(
                    format!("no user with id \"{owner_id}\""),
                    "unknown_owner_id",
                ));
            }
            owner_id
        }
        other => {
            return Err(bad_request(
                format!("owner_type must be \"server\" or \"user\", got \"{other}\""),
                "invalid_owner_type",
            ))
        }
    };

    let token_name = body.token_name.trim().to_string();
    let existing = state.db.list_mcp_tokens(id).await.map_err(db_error)?;
    if existing
        .iter()
        .any(|t| t.owner_type == owner_type && t.owner_id == owner_id && t.token_name == token_name)
    {
        return Err(duplicate_token_name(&token_name));
    }
    let expires_at = normalize_expires_at(body.expires_at.as_deref())?;

    let mk = state
        .aigw_master_key
        .as_deref()
        .ok_or_else(master_key_missing)?;
    let sealed = seal_token(plaintext, mk)
        .map_err(|e| bad_request(format!("could not encrypt token: {e}"), "encrypt_failed"))?;

    let now = now_rfc3339();
    let row = McpServerTokenRow {
        id: 0,
        server_id: id,
        owner_type: owner_type.clone(),
        owner_id: owner_id.clone(),
        token_name: token_name.clone(),
        token_value: sealed,
        is_default: false,
        enabled: true,
        expires_at,
        created_by: auth.key_alias.clone().unwrap_or_default(),
        created_at: now.clone(),
        updated_at: now,
    };
    let token_id = state.db.create_mcp_token(&row).await.map_err(|e| {
        if is_unique_violation(&e) {
            duplicate_token_name(&token_name)
        } else {
            db_error(e)
        }
    })?;
    if body.is_default {
        state
            .db
            .set_default_mcp_token(id, &owner_type, &owner_id, token_id)
            .await
            .map_err(db_error)?;
    }

    let created = state
        .db
        .get_mcp_token(token_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("MCP token"))?;
    Ok(Json(
        serde_json::to_value(McpTokenResponse::from(&created)).unwrap_or(json!({})),
    ))
}

async fn fetch_server_token(
    state: &SharedState,
    server_id: i64,
    token_id: i64,
) -> Result<McpServerTokenRow, (StatusCode, Json<Value>)> {
    let token = state
        .db
        .get_mcp_token(token_id)
        .await
        .map_err(db_error)?
        .ok_or_else(|| not_found("MCP token"))?;
    if token.server_id != server_id {
        return Err(not_found("MCP token"));
    }
    Ok(token)
}

pub async fn delete_mcp_token(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path((id, token_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server_token(&state, id, token_id).await?;
    state
        .db
        .delete_mcp_token(token_id)
        .await
        .map_err(db_error)?;
    Ok(Json(json!({"status": "deleted"})))
}

pub async fn set_default_mcp_token(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path((id, token_id)): Path<(i64, i64)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let token = fetch_server_token(&state, id, token_id).await?;
    state
        .db
        .set_default_mcp_token(id, &token.owner_type, &token.owner_id, token_id)
        .await
        .map_err(db_error)?;
    let token = fetch_server_token(&state, id, token_id).await?;
    Ok(Json(
        serde_json::to_value(McpTokenResponse::from(&token)).unwrap_or(json!({})),
    ))
}

#[derive(Debug, Deserialize)]
pub struct AllowlistBody {
    pub rules: Vec<ToolRule>,
}

#[derive(Debug, Deserialize)]
pub struct McpCallQuery {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
    pub server_alias: Option<String>,
    pub tool_name: Option<String>,
    pub status: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tool allowlist
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn allowlist_response(rules: Vec<ToolRule>) -> Json<Value> {
    Json(json!({"object": "list", "data": rules, "count": rules.len()}))
}

/// The table's primary key is `(server_id, tool_name, scope_type, scope_id)`, so
/// two entries that agree on those three fields are the same rule twice. Letting
/// the INSERT fail would report a caller mistake as a 500 and, worse, look
/// exactly like a database fault. The same tool under a *different* scope is a
/// legitimate second rule and must pass.
fn reject_duplicate_rules(rules: &[ToolRule]) -> Result<(), (StatusCode, Json<Value>)> {
    let mut seen = std::collections::HashSet::new();
    for rule in rules {
        if !seen.insert((
            rule.tool_name.as_str(),
            rule.scope_type.as_str(),
            rule.scope_id.as_str(),
        )) {
            return Err(bad_request(
                format!(
                    "duplicate rule for tool \"{}\" scope {}:{}",
                    rule.tool_name, rule.scope_type, rule.scope_id
                ),
                "duplicate_rule",
            ));
        }
    }
    Ok(())
}

fn validate_rules(rules: &[ToolRule]) -> Result<(), (StatusCode, Json<Value>)> {
    for rule in rules {
        if !is_known_scope(&rule.scope_type) {
            return Err(bad_request(
                format!(
                    "scope_type must be one of key/team/org, got \"{}\"",
                    rule.scope_type
                ),
                "invalid_scope_type",
            ));
        }
        if rule.scope_id.trim().is_empty() {
            return Err(bad_request(
                format!("scope_id must not be empty for tool \"{}\"", rule.tool_name),
                "invalid_scope_id",
            ));
        }
        validate_len(&rule.tool_name, "tool_name", MAX_NAME_LEN, "too_long")?;
        validate_len(&rule.scope_id, "scope_id", MAX_NAME_LEN, "too_long")?;
    }
    reject_duplicate_rules(rules)
}

pub async fn get_mcp_allowlist(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server(&state, id).await?;
    let rules = state
        .db
        .list_mcp_tool_allowlist(id)
        .await
        .map_err(db_error)?;
    Ok(allowlist_response(rules))
}

/// Whole-set replacement: the posted rules become the server's entire allowlist.
/// That is the only shape in which "remove a rule" is expressible without a
/// second endpoint, and it matches what the tools dialog sends.
pub async fn put_mcp_allowlist(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(id): Path<i64>,
    Json(body): Json<AllowlistBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    fetch_server(&state, id).await?;
    validate_rules(&body.rules)?;
    state
        .db
        .replace_mcp_tool_allowlist(id, &body.rules)
        .await
        .map_err(db_error)?;
    let rules = state
        .db
        .list_mcp_tool_allowlist(id)
        .await
        .map_err(db_error)?;
    Ok(allowlist_response(rules))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Call log
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// The call log's `status` column holds `success` / `failed`
/// (`MCP_CALL_STATUS_*`), while the tools dialog's filter offers `success` /
/// `error`. Translating here keeps the UI's vocabulary from silently matching
/// nothing; no row can be stored as `error`, so the name is free to mean this.
fn call_status_filter(status: Option<&str>) -> Option<String> {
    status.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        if s == "error" {
            aigw_core::models::MCP_CALL_STATUS_FAILED.to_string()
        } else {
            s.to_string()
        }
    })
}

/// `created_at` is stored as TEXT/VARCHAR holding RFC 3339, so the range filter
/// is a string comparison. That is only meaningful when both sides are written
/// in the same spelling — `Z` and `+00:00` are the same instant but sort
/// differently — so the bound is parsed and re-rendered in the writer's own
/// format (`to_rfc3339`, which is what every insert path uses). An unparseable
/// bound is a 400 rather than a comparison that silently matches everything or
/// nothing.
fn call_time_bound(raw: &str, field: &str) -> Result<String, (StatusCode, Json<Value>)> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|at| at.with_timezone(&chrono::Utc).to_rfc3339())
        .map_err(|_| {
            bad_request(
                format!("{field} must be an RFC 3339 timestamp, got \"{raw}\""),
                "invalid_time_range",
            )
        })
}

pub async fn list_mcp_calls(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Query(query): Query<McpCallQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(30).clamp(1, 100);
    let offset = (page as i64 - 1) * page_size as i64;
    let limit = page_size as i64;

    let bound = |value: Option<String>, field: &str| -> Result<Option<String>, _> {
        match value
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            Some(raw) => call_time_bound(&raw, field).map(Some),
            None => Ok(None),
        }
    };

    let filter = McpCallFilter {
        server_alias: query
            .server_alias
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        tool_name: query
            .tool_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        status: call_status_filter(query.status.as_deref()),
        from: bound(query.from, "from")?,
        to: bound(query.to, "to")?,
    };

    let (rows, total_count) = tokio::try_join!(
        state.db.list_mcp_call_log_filtered(&filter, limit, offset),
        state.db.count_mcp_call_log(&filter),
    )
    .map_err(db_error)?;

    let total_pages = if total_count > 0 {
        ((total_count as f64) / (page_size as f64)).ceil() as i64
    } else {
        0
    };

    Ok(Json(json!({
        "object": "list",
        "data": rows,
        "count": rows.len(),
        "total_count": total_count,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::{AppState, DEFAULT_KEY_TOKEN_LEN};
    use aigw_core::models::VirtualKey;
    use axum::http::Method;
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
        use axum::routing::{delete, get, post};
        Router::new()
            .route(
                "/admin/mcp/servers",
                get(list_mcp_servers).post(create_mcp_server),
            )
            .route("/admin/mcp/servers/all", get(list_all_mcp_servers))
            .route(
                "/admin/mcp/servers/batch-delete",
                post(batch_delete_mcp_servers),
            )
            .route(
                "/admin/mcp/servers/{id}",
                get(get_mcp_server)
                    .put(update_mcp_server)
                    .delete(delete_mcp_server),
            )
            .route("/admin/mcp/servers/{id}/toggle", post(toggle_mcp_server))
            .route("/admin/mcp/servers/{id}/probe", post(probe_mcp_server))
            .route("/admin/mcp/servers/{id}/tools", get(list_mcp_server_tools))
            .route(
                "/admin/mcp/servers/{id}/tokens",
                get(list_mcp_tokens).post(create_mcp_token),
            )
            .route(
                "/admin/mcp/servers/{id}/tokens/{token_id}",
                delete(delete_mcp_token),
            )
            .route(
                "/admin/mcp/servers/{id}/tokens/{token_id}/default",
                post(set_default_mcp_token),
            )
            .route(
                "/admin/mcp/servers/{id}/allowlist",
                get(get_mcp_allowlist).put(put_mcp_allowlist),
            )
            .route("/admin/mcp/calls", get(list_mcp_calls))
            .route("/admin/mcp/reload", post(reload_mcp))
            .with_state(state)
    }

    async fn send(
        app: &Router,
        method: Method,
        uri: &str,
        token: &str,
        body: Option<Value>,
    ) -> (u16, Value) {
        let req = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .body(axum::body::Body::from(
                body.map(|b| b.to_string()).unwrap_or_default(),
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status().as_u16();
        let bytes = axum::body::to_bytes(resp.into_body(), 262_144)
            .await
            .unwrap_or_default();
        let json = serde_json::from_slice(&bytes).unwrap_or(json!({}));
        (status, json)
    }

    async fn create_server(app: &Router, alias: &str, body: Value) -> i64 {
        let (s, resp) = send(app, Method::POST, "/admin/mcp/servers", MK, Some(body)).await;
        assert_eq!(s, 200, "create {alias} failed: {resp}");
        resp["id"].as_i64().expect("server id")
    }

    fn base_body(alias: &str) -> Value {
        json!({
            "alias": alias,
            "display_name": format!("{alias} server"),
            "url": "http://127.0.0.1:1/mcp",
        })
    }

    async fn insert_key_without_admin(state: &SharedState) -> String {
        let raw = format!("sk-{}", uuid::Uuid::new_v4());
        let key = VirtualKey {
            token: aigw_core::crypto::hash_token(&raw),
            key_name: Some("plain-user".to_string()),
            key_alias: Some("plain-user".to_string()),
            soft_budget_cooldown: "false".to_string(),
            spend: 0.0,
            expires: None,
            models: json!([]),
            aliases: json!({}),
            config: json!({}),
            router_settings: None,
            user_id: Some("user-1".to_string()),
            team_id: None,
            agent_id: None,
            project_id: None,
            permissions: json!({}),
            max_parallel_requests: None,
            metadata: json!({}),
            blocked: None,
            tpm_limit: None,
            rpm_limit: None,
            max_budget: None,
            soft_budget: None,
            budget_duration: None,
            budget_reset_at: None,
            allowed_cache_controls: json!([]),
            allowed_routes: json!([]),
            policies: json!([]),
            access_group_ids: json!([]),
            model_spend: json!({}),
            model_max_budget: json!({}),
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
        raw
    }

    async fn insert_user(state: &SharedState, user_id: &str) {
        state
            .db
            .insert_user(&aigw_core::models::User {
                user_id: user_id.to_string(),
                user_alias: None,
                team_id: None,
                sso_user_id: None,
                organization_id: None,
                object_permission_id: None,
                password: None,
                teams: json!([]),
                user_role: Some("user".to_string()),
                max_budget: None,
                spend: 0.0,
                user_email: Some(format!("{user_id}@example.com")),
                models: json!([]),
                metadata: json!({}),
                max_parallel_requests: None,
                tpm_limit: None,
                rpm_limit: None,
                budget_duration: None,
                budget_reset_at: None,
                allowed_cache_controls: json!([]),
                policies: json!([]),
                model_spend: json!({}),
                model_max_budget: json!({}),
                virtual_keys_count: None,
                created_at: Some(chrono::Utc::now()),
                updated_at: Some(chrono::Utc::now()),
            })
            .await
            .expect("insert user");
    }

    /// A JSON-RPC stub on an ephemeral port advertising `tools`. Leaked for the
    /// rest of the test process: an idle listener costs nothing and the binary
    /// exits immediately after.
    async fn spawn_probe_stub(tools: Vec<&'static str>) -> String {
        use axum::routing::post;
        let names: Vec<Value> = tools
            .iter()
            .map(|name| json!({"name": name, "description": format!("{name} tool")}))
            .collect();
        let app = Router::new().route(
            "/mcp",
            post(move |Json(request): Json<Value>| {
                let names = names.clone();
                async move {
                    let id = request.get("id").cloned().unwrap_or(Value::Null);
                    let method = request
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let result = match method {
                        "server/discover" => json!({
                            "supportedVersions": ["2026-07-28"],
                            "ttlMs": 60_000,
                            "serverInfo": {"name": "mcp-test-stub"},
                        }),
                        "tools/list" => json!({"tools": names}),
                        _ => json!({}),
                    };
                    Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind probe stub");
        let addr = listener.local_addr().expect("probe stub addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}/mcp")
    }

    async fn seed_catalog(state: &SharedState, server_id: i64, names: &[&str]) {
        let fetched_at = chrono::Utc::now().to_rfc3339();
        let rows: Vec<McpToolRow> = names
            .iter()
            .map(|name| McpToolRow {
                id: 0,
                server_id,
                tool_name: name.to_string(),
                description: String::new(),
                input_schema: json!({}),
                annotations: None,
                has_conflict: false,
                fetched_at: fetched_at.clone(),
            })
            .collect();
        state
            .db
            .replace_mcp_tool_catalog(server_id, &rows)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn non_admin_key_is_forbidden_on_the_server_list() {
        let state = test_state().await;
        let raw = insert_key_without_admin(&state).await;
        let app = build_router(state);
        let (s, body) = send(&app, Method::GET, "/admin/mcp/servers", &raw, None).await;
        assert_eq!(s, 403);
        assert_eq!(body["error"]["type"], "forbidden");
    }

    #[tokio::test]
    async fn create_without_url_is_rejected() {
        let app = build_router(test_state().await);
        let (s, body) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers",
            MK,
            Some(json!({"alias": "no-url", "display_name": "No URL"})),
        )
        .await;
        assert_eq!(
            s, 422,
            "a missing required field is a deserialization error"
        );
        assert!(body.is_object());
    }

    #[tokio::test]
    async fn a_display_name_over_the_column_width_is_a_bad_request() {
        let app = build_router(test_state().await);
        let mut body = base_body("long-name");
        body["display_name"] = json!("名".repeat(MAX_NAME_LEN + 1));
        let (s, resp) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers",
            MK,
            Some(body.clone()),
        )
        .await;
        assert_eq!(s, 400, "over-length display_name: {resp}");
        assert_eq!(resp["error"]["type"], "invalid_display_name");

        body["display_name"] = json!("名".repeat(MAX_NAME_LEN));
        let (s, resp) = send(&app, Method::POST, "/admin/mcp/servers", MK, Some(body)).await;
        assert_eq!(s, 200, "the exact limit must be accepted: {resp}");

        let id = resp["id"].as_i64().expect("server id");
        let (s, resp) = send(
            &app,
            Method::PUT,
            &format!("/admin/mcp/servers/{id}"),
            MK,
            Some(json!({"display_name": "x".repeat(MAX_NAME_LEN + 1)})),
        )
        .await;
        assert_eq!(s, 400, "over-length display_name on update: {resp}");
        assert_eq!(resp["error"]["type"], "invalid_display_name");
    }

    #[tokio::test]
    async fn a_transport_over_the_column_width_is_a_bad_request() {
        let app = build_router(test_state().await);
        let mut body = base_body("long-transport");
        body["transport"] = json!("t".repeat(MAX_TRANSPORT_LEN + 1));
        let (s, resp) = send(&app, Method::POST, "/admin/mcp/servers", MK, Some(body)).await;
        assert_eq!(s, 400, "over-length transport: {resp}");
        assert_eq!(resp["error"]["type"], "invalid_transport");
    }

    #[tokio::test]
    async fn an_unparseable_expires_at_is_a_bad_request() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "expiry", base_body("expiry")).await;
        let (s, resp) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            Some(json!({
                "token_name": "bad-expiry",
                "token_value": "sk-upstream",
                "expires_at": "2026-13-45T99:99:99Z",
            })),
        )
        .await;
        assert_eq!(s, 400, "bad expires_at: {resp}");
        assert_eq!(resp["error"]["type"], "invalid_expires_at");

        let tokens = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            None,
        )
        .await
        .1;
        assert_eq!(tokens["count"], json!(0), "nothing may be stored: {tokens}");

        let (s, resp) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            Some(json!({
                "token_name": "good-expiry",
                "token_value": "sk-upstream",
                "expires_at": " 2030-01-01T00:00:00Z ",
            })),
        )
        .await;
        assert_eq!(
            s, 200,
            "a valid RFC 3339 timestamp must be accepted: {resp}"
        );
        assert_eq!(resp["expires_at"], json!("2030-01-01T00:00:00Z"));
    }

    #[tokio::test]
    async fn create_with_an_empty_url_is_a_bad_request() {
        let app = build_router(test_state().await);
        let (s, body) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers",
            MK,
            Some(json!({"alias": "empty-url", "display_name": "E", "url": "  "})),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_url");
    }

    #[tokio::test]
    async fn create_with_a_double_underscore_alias_is_a_bad_request() {
        let app = build_router(test_state().await);
        let (s, body) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers",
            MK,
            Some(json!({
                "alias": "a__b",
                "display_name": "Ambiguous",
                "url": "http://127.0.0.1:1/mcp",
            })),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_alias");
    }

    #[tokio::test]
    async fn a_duplicate_alias_is_a_conflict() {
        let app = build_router(test_state().await);
        create_server(&app, "dup", base_body("dup")).await;
        let (s, body) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers",
            MK,
            Some(base_body("dup")),
        )
        .await;
        assert_eq!(s, 409);
        assert_eq!(body["error"]["type"], "DUPLICATE_ALIAS");
    }

    #[tokio::test]
    async fn create_honours_enabled_false() {
        let app = build_router(test_state().await);
        let mut body = base_body("off");
        body["enabled"] = json!(false);
        create_server(&app, "off", body).await;

        let (s, list) = send(&app, Method::GET, "/admin/mcp/servers", MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(list["data"][0]["enabled"], json!(false));
        assert_eq!(list["data"][0]["tool_count"], json!(0));
    }

    #[tokio::test]
    async fn create_defaults_enabled_to_true_when_absent() {
        let app = build_router(test_state().await);
        create_server(&app, "on", base_body("on")).await;
        let (_, list) = send(&app, Method::GET, "/admin/mcp/servers", MK, None).await;
        assert_eq!(list["data"][0]["enabled"], json!(true));
    }

    #[tokio::test]
    async fn search_and_status_filters_agree_with_total_count() {
        let app = build_router(test_state().await);
        create_server(&app, "alpha", base_body("alpha")).await;
        create_server(&app, "beta", base_body("beta")).await;
        let mut off = base_body("gamma");
        off["enabled"] = json!(false);
        create_server(&app, "gamma", off).await;

        let (s, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/servers?search=al&page=1&page_size=1",
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["total_count"], json!(1));
        assert_eq!(body["count"], json!(1));
        assert_eq!(body["page_size"], json!(1));
        assert_eq!(body["total_pages"], json!(1));
        assert_eq!(body["object"], json!("list"));
        assert_eq!(body["data"][0]["alias"], json!("alpha"));

        let (_, enabled) = send(
            &app,
            Method::GET,
            "/admin/mcp/servers?status=enabled",
            MK,
            None,
        )
        .await;
        assert_eq!(enabled["total_count"], json!(2));
        let (_, disabled) = send(
            &app,
            Method::GET,
            "/admin/mcp/servers?status=disabled",
            MK,
            None,
        )
        .await;
        assert_eq!(disabled["total_count"], json!(1));
        assert_eq!(disabled["data"][0]["alias"], json!("gamma"));
    }

    #[tokio::test]
    async fn a_search_full_of_wildcards_matches_nothing() {
        let app = build_router(test_state().await);
        create_server(&app, "ab", base_body("ab")).await;
        create_server(&app, "a_b", base_body("a_b")).await;

        for (query, expected) in [
            ("%25", 0),
            ("a%25", 0),
            ("%25%25", 0),
            ("%5C", 0),
            ("a_b", 1),
        ] {
            let (s, body) = send(
                &app,
                Method::GET,
                &format!("/admin/mcp/servers?search={query}"),
                MK,
                None,
            )
            .await;
            assert_eq!(s, 200, "search={query} failed: {body}");
            assert_eq!(
                body["total_count"],
                json!(expected),
                "search={query} must treat % _ and \\ as literal text, not wildcards: {body}"
            );
            assert_eq!(
                body["count"],
                json!(expected),
                "list and count must agree for search={query}: {body}"
            );
        }
    }

    #[tokio::test]
    async fn page_size_is_clamped_to_the_hundred_ceiling() {
        let app = build_router(test_state().await);
        let (s, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/servers?page_size=5000",
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["page_size"], json!(100));
        assert_eq!(body["page"], json!(1));
    }

    #[tokio::test]
    async fn toggling_twice_returns_to_the_original_state() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "flip", base_body("flip")).await;
        let uri = format!("/admin/mcp/servers/{id}/toggle");
        let (s, first) = send(&app, Method::POST, &uri, MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(first["enabled"], json!(false));
        let (s, second) = send(&app, Method::POST, &uri, MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(second["enabled"], json!(true));
    }

    #[tokio::test]
    async fn detail_carries_tool_count_from_the_snapshot() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_server(&app, "counted", base_body("counted")).await;
        let (_, before) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}"),
            MK,
            None,
        )
        .await;
        assert_eq!(before["tool_count"], json!(0));

        let fetched_at = chrono::Utc::now().to_rfc3339();
        let rows: Vec<McpToolRow> = ["a", "b"]
            .iter()
            .map(|name| McpToolRow {
                id: 0,
                server_id: id,
                tool_name: name.to_string(),
                description: String::new(),
                input_schema: json!({}),
                annotations: None,
                has_conflict: false,
                fetched_at: fetched_at.clone(),
            })
            .collect();
        state.db.replace_mcp_tool_catalog(id, &rows).await.unwrap();

        let (_, detail) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}"),
            MK,
            None,
        )
        .await;
        assert_eq!(detail["tool_count"], json!(2));
        let (_, list) = send(&app, Method::GET, "/admin/mcp/servers", MK, None).await;
        assert_eq!(list["data"][0]["tool_count"], json!(2));
    }

    #[tokio::test]
    async fn tools_endpoint_returns_an_empty_snapshot_not_a_404() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "empty-tools", base_body("empty-tools")).await;
        let (s, body) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/tools"),
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["tools"], json!([]));
        assert_eq!(body["conflicts"], json!([]));
        assert_eq!(body["fetched_at"], Value::Null);
    }

    #[tokio::test]
    async fn tools_endpoint_rejects_an_unknown_server() {
        let app = build_router(test_state().await);
        let (s, body) = send(&app, Method::GET, "/admin/mcp/servers/9999/tools", MK, None).await;
        assert_eq!(s, 404);
        assert_eq!(body["error"]["type"], "not_found");
    }

    #[tokio::test]
    async fn a_probe_against_a_refused_endpoint_fails_and_keeps_the_old_snapshot() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_server(&app, "keeps", base_body("keeps")).await;

        let fetched_at = chrono::Utc::now().to_rfc3339();
        let rows = vec![McpToolRow {
            id: 0,
            server_id: id,
            tool_name: "survivor".to_string(),
            description: "still here".to_string(),
            input_schema: json!({"type": "object"}),
            annotations: None,
            has_conflict: false,
            fetched_at: fetched_at.clone(),
        }];
        state.db.replace_mcp_tool_catalog(id, &rows).await.unwrap();

        let (s, body) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/probe"),
            MK,
            None,
        )
        .await;
        assert_eq!(s, 502);
        assert_eq!(body["error"]["type"], "probe_failed");

        let (_, tools) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/tools"),
            MK,
            None,
        )
        .await;
        assert_eq!(tools["tools"][0]["tool_name"], json!("survivor"));
        assert_eq!(tools["fetched_at"], json!(fetched_at));
    }

    #[tokio::test]
    async fn a_token_response_never_carries_the_plaintext() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "with-token", base_body("with-token")).await;
        let (s, created) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            Some(json!({"token_name": "shared", "token_value": "sk-upstream-secret"})),
        )
        .await;
        assert_eq!(s, 200, "create token failed: {created}");
        assert_eq!(created["token_redacted"], json!("***"));
        assert!(created.get("token_value").is_none());

        let (s, list) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(list["count"], json!(1));
        assert_eq!(list["data"][0]["token_redacted"], json!("***"));
        assert!(
            !list.to_string().contains("sk-upstream-secret"),
            "plaintext leaked into the list response: {list}"
        );
    }

    #[tokio::test]
    async fn a_stored_token_is_sealed_not_plaintext() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_server(&app, "sealed", base_body("sealed")).await;
        let (s, _) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            Some(json!({"token_name": "t1", "token_value": "sk-very-secret"})),
        )
        .await;
        assert_eq!(s, 200);
        let stored = state.db.list_mcp_tokens(id).await.unwrap();
        assert!(
            stored[0].token_value.starts_with("v2:gcm:"),
            "token must be sealed, got {}",
            stored[0].token_value
        );
        assert!(!stored[0].token_value.contains("sk-very-secret"));
    }

    #[tokio::test]
    async fn setting_the_second_token_default_clears_the_first() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "two-tokens", base_body("two-tokens")).await;
        let uri = format!("/admin/mcp/servers/{id}/tokens");
        let (_, first) = send(
            &app,
            Method::POST,
            &uri,
            MK,
            Some(json!({"token_name": "one", "token_value": "k1", "is_default": true})),
        )
        .await;
        let (_, second) = send(
            &app,
            Method::POST,
            &uri,
            MK,
            Some(json!({"token_name": "two", "token_value": "k2"})),
        )
        .await;
        assert_eq!(first["is_default"], json!(true));
        assert_eq!(second["is_default"], json!(false));

        let second_id = second["id"].as_i64().unwrap();
        let (s, promoted) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens/{second_id}/default"),
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(promoted["is_default"], json!(true));

        let (_, list) = send(&app, Method::GET, &uri, MK, None).await;
        let flags: Vec<bool> = list["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["is_default"].as_bool().unwrap())
            .collect();
        assert_eq!(flags, vec![false, true]);
    }

    #[tokio::test]
    async fn a_user_owned_token_requires_a_real_user_id() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_server(&app, "user-token", base_body("user-token")).await;
        let uri = format!("/admin/mcp/servers/{id}/tokens");

        let (s, body) = send(
            &app,
            Method::POST,
            &uri,
            MK,
            Some(json!({"token_name": "mine", "token_value": "k", "owner_type": "user"})),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_owner_id");

        let (s, body) = send(
            &app,
            Method::POST,
            &uri,
            MK,
            Some(json!({
                "token_name": "mine", "token_value": "k",
                "owner_type": "user", "owner_id": "ghost",
            })),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "unknown_owner_id");

        insert_user(&state, "user-7").await;
        let (s, created) = send(
            &app,
            Method::POST,
            &uri,
            MK,
            Some(json!({
                "token_name": "mine", "token_value": "k",
                "owner_type": "user", "owner_id": "user-7",
            })),
        )
        .await;
        assert_eq!(s, 200, "create user token failed: {created}");
        assert_eq!(created["owner_type"], json!("user"));
        assert_eq!(created["owner_id"], json!("user-7"));
    }

    #[tokio::test]
    async fn an_unknown_owner_type_is_rejected() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "bad-owner", base_body("bad-owner")).await;
        let (s, body) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            Some(json!({"token_name": "x", "token_value": "k", "owner_type": "team"})),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_owner_type");
    }

    #[tokio::test]
    async fn a_duplicate_token_name_for_the_same_owner_is_a_conflict() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "dup-token", base_body("dup-token")).await;
        let uri = format!("/admin/mcp/servers/{id}/tokens");
        let body = json!({"token_name": "shared", "token_value": "k"});
        let (s, _) = send(&app, Method::POST, &uri, MK, Some(body.clone())).await;
        assert_eq!(s, 200);
        let (s, resp) = send(&app, Method::POST, &uri, MK, Some(body)).await;
        assert_eq!(s, 409);
        assert_eq!(resp["error"]["type"], "DUPLICATE_TOKEN_NAME");
    }

    #[tokio::test]
    async fn deleting_a_server_removes_its_tokens_and_catalog() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_server(&app, "cascade", base_body("cascade")).await;
        send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{id}/tokens"),
            MK,
            Some(json!({"token_name": "t", "token_value": "k"})),
        )
        .await;
        state
            .db
            .replace_mcp_tool_catalog(
                id,
                &[McpToolRow {
                    id: 0,
                    server_id: id,
                    tool_name: "gone".to_string(),
                    description: String::new(),
                    input_schema: json!({}),
                    annotations: None,
                    has_conflict: false,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                }],
            )
            .await
            .unwrap();

        let (s, _) = send(
            &app,
            Method::DELETE,
            &format!("/admin/mcp/servers/{id}"),
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200);
        assert!(state.db.list_mcp_tokens(id).await.unwrap().is_empty());
        assert!(state.db.list_mcp_tool_catalog(id).await.unwrap().is_empty());
        assert!(state.db.get_mcp_server(id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn deleting_an_unknown_server_is_a_404() {
        let app = build_router(test_state().await);
        let (s, body) = send(&app, Method::DELETE, "/admin/mcp/servers/4242", MK, None).await;
        assert_eq!(s, 404);
        assert_eq!(body["error"]["type"], "not_found");
    }

    #[tokio::test]
    async fn batch_delete_reports_every_removed_id() {
        let app = build_router(test_state().await);
        let a = create_server(&app, "batch-a", base_body("batch-a")).await;
        let b = create_server(&app, "batch-b", base_body("batch-b")).await;
        let (s, body) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers/batch-delete",
            MK,
            Some(json!({"ids": [a, b]})),
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["deleted_ids"], json!([a, b]));
        assert_eq!(body["skipped"], json!([]));
        let (_, list) = send(&app, Method::GET, "/admin/mcp/servers", MK, None).await;
        assert_eq!(list["total_count"], json!(0));
    }

    #[tokio::test]
    async fn batch_delete_reports_an_unknown_id_as_skipped() {
        let app = build_router(test_state().await);
        let a = create_server(&app, "batch-known", base_body("batch-known")).await;
        let (s, body) = send(
            &app,
            Method::POST,
            "/admin/mcp/servers/batch-delete",
            MK,
            Some(json!({"ids": [a, -1]})),
        )
        .await;
        assert_eq!(s, 200, "an unknown id must not fail the batch: {body}");
        assert_eq!(body["deleted_ids"], json!([a]));
        assert_eq!(body["skipped"], json!([{"id": -1, "reason": "not_found"}]));
    }

    #[tokio::test]
    async fn an_absurd_page_number_does_not_overflow_the_offset() {
        let app = build_router(test_state().await);
        create_server(&app, "paged", base_body("paged")).await;
        let (s, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/servers?page=2000000000&page_size=100",
            MK,
            None,
        )
        .await;
        assert_eq!(s, 200, "huge page: {body}");
        assert_eq!(body["count"], json!(0));
        assert_eq!(body["total_count"], json!(1));
    }

    #[tokio::test]
    async fn update_replaces_only_the_supplied_fields() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "partial", base_body("partial")).await;
        let (s, body) = send(
            &app,
            Method::PUT,
            &format!("/admin/mcp/servers/{id}"),
            MK,
            Some(json!({"display_name": "Renamed"})),
        )
        .await;
        assert_eq!(s, 200);
        assert_eq!(body["display_name"], json!("Renamed"));
        assert_eq!(body["alias"], json!("partial"));
        assert_eq!(body["enabled"], json!(true));
        assert_eq!(body["credential_scope"], json!("shared_or_user"));
    }

    #[tokio::test]
    async fn update_with_a_bad_url_is_rejected() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "bad-update", base_body("bad-update")).await;
        let (s, body) = send(
            &app,
            Method::PUT,
            &format!("/admin/mcp/servers/{id}"),
            MK,
            Some(json!({"url": "ftp://nope"})),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_url");
    }

    #[tokio::test]
    async fn a_token_cannot_be_deleted_through_another_server() {
        let app = build_router(test_state().await);
        let owner = create_server(&app, "owner", base_body("owner")).await;
        let other = create_server(&app, "other", base_body("other")).await;
        let (_, created) = send(
            &app,
            Method::POST,
            &format!("/admin/mcp/servers/{owner}/tokens"),
            MK,
            Some(json!({"token_name": "t", "token_value": "k"})),
        )
        .await;
        let token_id = created["id"].as_i64().unwrap();
        let (s, body) = send(
            &app,
            Method::DELETE,
            &format!("/admin/mcp/servers/{other}/tokens/{token_id}"),
            MK,
            None,
        )
        .await;
        assert_eq!(s, 404);
        assert_eq!(body["error"]["type"], "not_found");
    }

    #[tokio::test]
    async fn reload_with_no_servers_is_a_no_op_success() {
        let app = build_router(test_state().await);
        let (s, body) = send(&app, Method::POST, "/admin/mcp/reload", MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(body["status"], json!("ok"));
        assert_eq!(body["probed"], json!(0));
    }

    #[tokio::test]
    async fn reload_skips_disabled_servers() {
        let app = build_router(test_state().await);
        let mut body = base_body("switched-off");
        body["enabled"] = json!(false);
        create_server(&app, "switched-off", body).await;
        let (s, resp) = send(&app, Method::POST, "/admin/mcp/reload", MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(resp["probed"], json!(0));
    }

    #[tokio::test]
    async fn reload_keeps_a_disabled_servers_snapshot() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let mut body = base_body("sleeping");
        body["enabled"] = json!(false);
        let id = create_server(&app, "sleeping", body).await;
        seed_catalog(&state, id, &["kept", "also-kept"]).await;

        let (s, resp) = send(&app, Method::POST, "/admin/mcp/reload", MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(resp["probed"], json!(0));

        let (_, tools) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/tools"),
            MK,
            None,
        )
        .await;
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .filter_map(|t| t["tool_name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec!["also-kept", "kept"],
            "a reload must not wipe a disabled server's snapshot"
        );
    }

    #[tokio::test]
    async fn reload_flags_a_shared_tool_name_on_both_servers() {
        let stub = spawn_probe_stub(vec!["search"]).await;
        let state = test_state().await;
        let app = build_router(state.clone());

        let mut first = base_body("alpha");
        first["url"] = json!(stub);
        let alpha = create_server(&app, "alpha", first).await;
        let mut second = base_body("beta");
        second["url"] = json!(stub);
        let beta = create_server(&app, "beta", second).await;
        seed_catalog(&state, alpha, &["search"]).await;
        seed_catalog(&state, beta, &["search"]).await;

        let (s, body) = send(&app, Method::POST, "/admin/mcp/reload", MK, None).await;
        assert_eq!(s, 200, "reload failed: {body}");
        assert_eq!(body["failed"], json!(0), "reload body: {body}");

        let conflicts_of = |id: i64| -> Vec<String> {
            let entry = body["servers"]
                .as_array()
                .expect("servers array")
                .iter()
                .find(|s| s["id"].as_i64() == Some(id))
                .unwrap_or_else(|| panic!("server {id} missing from reload: {body}"));
            entry["conflicts"]
                .as_array()
                .expect("conflicts array")
                .iter()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect()
        };
        assert_eq!(
            conflicts_of(alpha),
            vec!["search".to_string()],
            "the first-probed server must see the conflict too: {body}"
        );
        assert_eq!(
            conflicts_of(beta),
            vec!["search".to_string()],
            "conflict flags must be symmetric: {body}"
        );
    }

    #[tokio::test]
    async fn reload_reports_a_failed_probe_without_aborting_the_rest() {
        let app = build_router(test_state().await);
        create_server(&app, "unreachable", base_body("unreachable")).await;
        let (s, body) = send(&app, Method::POST, "/admin/mcp/reload", MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(body["status"], json!("partial"));
        assert_eq!(body["failed"], json!(1));
        assert_eq!(body["servers"][0]["ok"], json!(false));
        assert_eq!(body["servers"][0]["upstream_status"], json!(502));
    }

    #[tokio::test]
    async fn list_all_servers_returns_every_alias() {
        let app = build_router(test_state().await);
        create_server(&app, "one", base_body("one")).await;
        create_server(&app, "two", base_body("two")).await;
        let (s, body) = send(&app, Method::GET, "/admin/mcp/servers/all", MK, None).await;
        assert_eq!(s, 200);
        assert_eq!(body["count"], json!(2));
        assert_eq!(body["object"], json!("list"));
    }

    // ── allowlist ──────────────────────────────────────────────────────

    async fn put_rules(app: &Router, id: i64, rules: Value) -> (u16, Value) {
        send(
            app,
            Method::PUT,
            &format!("/admin/mcp/servers/{id}/allowlist"),
            MK,
            Some(json!({ "rules": rules })),
        )
        .await
    }

    async fn read_rules(app: &Router, id: i64) -> Value {
        send(
            app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/allowlist"),
            MK,
            None,
        )
        .await
        .1
    }

    #[tokio::test]
    async fn an_empty_allowlist_reads_back_as_an_empty_list() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-empty", base_body("acl-empty")).await;
        let body = read_rules(&app, id).await;
        assert_eq!(body["object"], json!("list"));
        assert_eq!(body["data"], json!([]));
        assert_eq!(body["count"], json!(0));
    }

    #[tokio::test]
    async fn put_replaces_the_whole_set_rather_than_appending() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-replace", base_body("acl-replace")).await;

        let (s, first) = put_rules(
            &app,
            id,
            json!([
                {"tool_name": "search", "scope_type": "key", "scope_id": "h1", "allowed": false},
                {"tool_name": "read", "scope_type": "team", "scope_id": "team-1", "allowed": true},
            ]),
        )
        .await;
        assert_eq!(s, 200, "{first}");
        assert_eq!(first["count"], json!(2));

        let (s, second) = put_rules(
            &app,
            id,
            json!([{"tool_name": "write", "scope_type": "org", "scope_id": "org-1", "allowed": false}]),
        )
        .await;
        assert_eq!(s, 200, "{second}");
        assert_eq!(second["count"], json!(1), "{second}");

        let readback = read_rules(&app, id).await;
        assert_eq!(
            readback["data"], second["data"],
            "the second PUT is the whole truth: {readback}"
        );
    }

    #[tokio::test]
    async fn allowed_survives_the_round_trip_as_a_json_boolean() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-bool", base_body("acl-bool")).await;
        let (s, body) = put_rules(
            &app,
            id,
            json!([
                {"tool_name": "a", "scope_type": "key", "scope_id": "h1", "allowed": false},
                {"tool_name": "b", "scope_type": "key", "scope_id": "h1", "allowed": true},
            ]),
        )
        .await;
        assert_eq!(s, 200, "{body}");
        let flags: Vec<bool> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["allowed"].as_bool().unwrap_or(false))
            .collect();
        assert_eq!(
            flags,
            vec![false, true],
            "0/1 must never reach the UI: {body}"
        );
    }

    #[tokio::test]
    async fn an_unknown_scope_type_is_a_bad_request() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-scope", base_body("acl-scope")).await;
        let (s, body) = put_rules(
            &app,
            id,
            json!([{"tool_name": "a", "scope_type": "user", "scope_id": "u1", "allowed": true}]),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_scope_type");
        assert_eq!(
            read_rules(&app, id).await["count"],
            json!(0),
            "nothing stored"
        );
    }

    #[tokio::test]
    async fn a_blank_scope_id_is_a_bad_request() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-blank", base_body("acl-blank")).await;
        let (s, body) = put_rules(
            &app,
            id,
            json!([{"tool_name": "a", "scope_type": "team", "scope_id": "   ", "allowed": true}]),
        )
        .await;
        assert_eq!(s, 400);
        assert_eq!(body["error"]["type"], "invalid_scope_id");
    }

    #[tokio::test]
    async fn the_same_rule_twice_is_a_bad_request_not_a_server_error() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-dup", base_body("acl-dup")).await;
        let rule =
            json!({"tool_name": "search", "scope_type": "key", "scope_id": "h1", "allowed": false});
        let (s, body) = put_rules(&app, id, json!([rule, rule])).await;
        assert_eq!(
            s, 400,
            "a caller mistake must not look like a DB fault: {body}"
        );
        assert_eq!(body["error"]["type"], "duplicate_rule");
        assert_eq!(
            read_rules(&app, id).await["count"],
            json!(0),
            "whole PUT rejected"
        );
    }

    #[tokio::test]
    async fn the_same_tool_under_two_scopes_is_two_legitimate_rules() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-two-scopes", base_body("acl-two-scopes")).await;
        let (s, body) = put_rules(
            &app,
            id,
            json!([
                {"tool_name": "search", "scope_type": "team", "scope_id": "t_1", "allowed": false},
                {"tool_name": "search", "scope_type": "org", "scope_id": "o_1", "allowed": true},
            ]),
        )
        .await;
        assert_eq!(
            s, 200,
            "the key is the whole triple, not the tool name: {body}"
        );
        assert_eq!(body["count"], json!(2));
    }

    #[tokio::test]
    async fn an_allowlist_for_an_unknown_server_is_a_404() {
        let app = build_router(test_state().await);
        let (s, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/servers/4242/allowlist",
            MK,
            None,
        )
        .await;
        assert_eq!(s, 404);
        assert_eq!(body["error"]["type"], "not_found");

        let (s, body) = put_rules(&app, 4242, json!([])).await;
        assert_eq!(s, 404);
        assert_eq!(body["error"]["type"], "not_found");
    }

    #[tokio::test]
    async fn an_over_long_rule_field_is_a_bad_request_on_every_dialect() {
        let app = build_router(test_state().await);
        let id = create_server(&app, "acl-long", base_body("acl-long")).await;

        // MySQL declares tool_name/scope_id as VARCHAR(255); SQLite and PG take
        // TEXT. Without this check the same body is a 200 there and a 500 here.
        let long = "t".repeat(MAX_NAME_LEN + 1);
        let (s, body) = put_rules(
            &app,
            id,
            json!([{"tool_name": long, "scope_type": "key", "scope_id": "h1", "allowed": true}]),
        )
        .await;
        assert_eq!(s, 400, "over-long tool_name: {body}");
        assert_eq!(body["error"]["type"], "too_long");

        let (s, body) = put_rules(
            &app,
            id,
            json!([{"tool_name": "a", "scope_type": "key", "scope_id": long, "allowed": true}]),
        )
        .await;
        assert_eq!(s, 400, "over-long scope_id: {body}");
        assert_eq!(body["error"]["type"], "too_long");

        let (s, body) = put_rules(
            &app,
            id,
            json!([{
                "tool_name": "t".repeat(MAX_NAME_LEN),
                "scope_type": "key",
                "scope_id": "h1",
                "allowed": true,
            }]),
        )
        .await;
        assert_eq!(s, 200, "the exact column width must be accepted: {body}");
    }

    #[tokio::test]
    async fn a_non_admin_cannot_read_or_write_an_allowlist() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let id = create_server(&app, "acl-guarded", base_body("acl-guarded")).await;
        let raw = insert_key_without_admin(&state).await;

        let (s, body) = send(
            &app,
            Method::GET,
            &format!("/admin/mcp/servers/{id}/allowlist"),
            &raw,
            None,
        )
        .await;
        assert_eq!(s, 403);
        assert_eq!(body["error"]["type"], "forbidden");

        let (s, body) = send(
            &app,
            Method::PUT,
            &format!("/admin/mcp/servers/{id}/allowlist"),
            &raw,
            Some(json!({"rules": []})),
        )
        .await;
        assert_eq!(s, 403);
        assert_eq!(body["error"]["type"], "forbidden");
    }

    // ── call log ───────────────────────────────────────────────────────

    async fn record_call(state: &SharedState, alias: &str, tool: &str, status: &str) {
        state
            .db
            .insert_mcp_call_log(&aigw_core::models::McpCallLogRow {
                call_id: uuid::Uuid::new_v4().to_string(),
                server_alias: alias.to_string(),
                tool_name: Some(tool.to_string()),
                method: "tools/call".to_string(),
                key_id: Some("*master*".to_string()),
                user_id: None,
                token_owner: "anonymous".to_string(),
                status: status.to_string(),
                error_code: None,
                latency_ms: 5,
                request_bytes: 10,
                response_bytes: 20,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .await
            .expect("insert call log");
    }

    #[tokio::test]
    async fn the_call_log_envelope_matches_the_frontend_contract() {
        let state = test_state().await;
        let app = build_router(state.clone());
        record_call(&state, "alpha", "search", "success").await;
        record_call(&state, "beta", "read", "failed").await;

        let (s, body) = send(&app, Method::GET, "/admin/mcp/calls", MK, None).await;
        assert_eq!(s, 200, "{body}");
        assert_eq!(body["object"], json!("list"));
        assert_eq!(body["count"], json!(2));
        assert_eq!(body["total_count"], json!(2));
        assert_eq!(body["page"], json!(1));
        assert_eq!(body["page_size"], json!(30));
        let row = &body["data"][0];
        for field in [
            "call_id",
            "server_alias",
            "tool_name",
            "method",
            "key_id",
            "user_id",
            "token_owner",
            "status",
            "error_code",
            "latency_ms",
            "request_bytes",
            "response_bytes",
            "created_at",
        ] {
            assert!(row.get(field).is_some(), "row is missing {field}: {row}");
        }
    }

    #[tokio::test]
    async fn the_call_log_filters_by_alias_tool_and_status() {
        let state = test_state().await;
        let app = build_router(state.clone());
        record_call(&state, "alpha", "search", "success").await;
        record_call(&state, "alpha", "read", "failed").await;
        record_call(&state, "beta", "search", "success").await;

        let (_, by_alias) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?server_alias=alpha",
            MK,
            None,
        )
        .await;
        assert_eq!(by_alias["total_count"], json!(2));

        let (_, by_tool) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?tool_name=search",
            MK,
            None,
        )
        .await;
        assert_eq!(by_tool["total_count"], json!(2));

        let (_, by_status) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?status=success",
            MK,
            None,
        )
        .await;
        assert_eq!(by_status["total_count"], json!(2));

        let (_, both) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?server_alias=alpha&status=success",
            MK,
            None,
        )
        .await;
        assert_eq!(both["total_count"], json!(1));
        assert_eq!(both["data"][0]["tool_name"], json!("search"));
    }

    #[tokio::test]
    async fn the_error_status_alias_selects_failed_rows() {
        let state = test_state().await;
        let app = build_router(state.clone());
        record_call(&state, "alpha", "ok-tool", "success").await;
        record_call(&state, "alpha", "bad-tool", "failed").await;

        let (_, body) = send(&app, Method::GET, "/admin/mcp/calls?status=error", MK, None).await;
        assert_eq!(
            body["total_count"],
            json!(1),
            "the dialog's `error` means the stored `failed`: {body}"
        );
        assert_eq!(body["data"][0]["tool_name"], json!("bad-tool"));
    }

    #[tokio::test]
    async fn a_wildcard_in_the_tool_filter_is_matched_literally() {
        let state = test_state().await;
        let app = build_router(state.clone());
        record_call(&state, "alpha", "search", "success").await;
        record_call(&state, "alpha", "read", "success").await;

        let (_, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?tool_name=%25",
            MK,
            None,
        )
        .await;
        assert_eq!(
            body["total_count"],
            json!(0),
            "a bare % must not match every tool: {body}"
        );
    }

    #[tokio::test]
    async fn a_malformed_time_bound_is_a_bad_request() {
        let app = build_router(test_state().await);
        let (s, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?from=yesterday",
            MK,
            None,
        )
        .await;
        assert_eq!(s, 400, "{body}");
        assert_eq!(body["error"]["type"], "invalid_time_range");

        let (s, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?to=2026-13-45",
            MK,
            None,
        )
        .await;
        assert_eq!(s, 400, "{body}");
        assert_eq!(body["error"]["type"], "invalid_time_range");
    }

    #[tokio::test]
    async fn the_time_range_is_a_closed_interval() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let row = |call_id: &str, created_at: &str| aigw_core::models::McpCallLogRow {
            call_id: call_id.to_string(),
            server_alias: "alpha".to_string(),
            tool_name: Some("search".to_string()),
            method: "tools/call".to_string(),
            key_id: Some("*master*".to_string()),
            user_id: None,
            token_owner: "anonymous".to_string(),
            status: "success".to_string(),
            error_code: None,
            latency_ms: 1,
            request_bytes: 1,
            response_bytes: 1,
            created_at: created_at.to_string(),
        };
        for (id, at) in [
            ("early", "2026-01-01T00:00:00+00:00"),
            ("inside", "2026-02-01T00:00:00+00:00"),
            ("late", "2026-03-01T00:00:00+00:00"),
        ] {
            state.db.insert_mcp_call_log(&row(id, at)).await.unwrap();
        }

        let (_, body) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?from=2026-01-01T00:00:00Z&to=2026-02-01T00:00:00Z",
            MK,
            None,
        )
        .await;
        let ids: Vec<&str> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["call_id"].as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["inside", "early"],
            "both bounds are inclusive and the bound is normalised to the writer's format: {body}"
        );
    }

    #[tokio::test]
    async fn the_call_log_pages_without_overlap() {
        let state = test_state().await;
        let app = build_router(state.clone());
        for tool in ["t1", "t2", "t3"] {
            record_call(&state, "alpha", tool, "success").await;
        }

        let (_, first) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?page=1&page_size=2",
            MK,
            None,
        )
        .await;
        assert_eq!(first["count"], json!(2));
        assert_eq!(first["total_count"], json!(3));
        assert_eq!(first["total_pages"], json!(2));

        let (_, second) = send(
            &app,
            Method::GET,
            "/admin/mcp/calls?page=2&page_size=2",
            MK,
            None,
        )
        .await;
        assert_eq!(second["count"], json!(1));
        let first_ids: Vec<&str> = first["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["call_id"].as_str())
            .collect();
        let second_ids: Vec<&str> = second["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["call_id"].as_str())
            .collect();
        assert!(
            first_ids.iter().all(|id| !second_ids.contains(id)),
            "pages overlap"
        );
    }

    #[tokio::test]
    async fn a_non_admin_cannot_read_the_call_log() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let raw = insert_key_without_admin(&state).await;
        let (s, body) = send(&app, Method::GET, "/admin/mcp/calls", &raw, None).await;
        assert_eq!(s, 403);
        assert_eq!(body["error"]["type"], "forbidden");
    }
}
