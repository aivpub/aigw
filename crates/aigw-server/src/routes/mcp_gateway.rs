//! `/mcp` and `/mcp/{alias}` — MCP passthrough (Stages 143/144).
//!
//! Two clients, two stacks: Claude Code opens with `server/discover`
//! (`2026-07-28`, no session) and Codex sends only `initialize`
//! (`2025-06-18`). The inbound stack is answered here rather than forwarded —
//! the client has to hear the gateway's capability statement, not the
//! upstream's. The outbound stack is negotiated independently from
//! `mcp_servers.protocol_version`, so a modern client against a legacy upstream
//! is a normal combination.
//!
//! The client's `Authorization` is an aigw virtual key. It authenticates and is
//! then dropped: the only credential that can go outbound is a row from
//! `mcp_server_tokens` (see `mcp::credential::resolve_outbound_token`, whose
//! signature has nowhere to put a client credential).
//!
//! `/mcp` fronts every enabled server at once: tool names are `{alias}__{tool}`,
//! and a call is routed by splitting the prefix. It goes through the same
//! `prepare` → `relay_forwarded` → `finish` path as `/mcp/{alias}`, so the
//! credential direction has exactly one execution point.
//!
//! Every outcome — answered locally or relayed, success or failure — writes one
//! `mcp_call_log` row. A failed call is the main input to troubleshooting, so
//! logging only successes would be no audit at all.

use aigw_core::db::Database;
use aigw_core::mcp::acl::{self, Actor};
use aigw_core::mcp::catalog;
use aigw_core::mcp::catalog_cache::{self, DEFAULT_TTL_MS};
use aigw_core::mcp::credential::{
    is_token_usable, resolve_outbound_token, unseal_token, TokenSource,
};
use aigw_core::mcp::probe::probe_server;
use aigw_core::mcp::session::{self, UpstreamStack, UpstreamTarget};
use aigw_core::middleware::KeyIdentity;
use aigw_core::models::{
    CredentialScope, McpCallLogRow, McpServerRow, McpToolRow, UpstreamTool, MCP_CALL_STATUS_FAILED,
    MCP_CALL_STATUS_SUCCESS, MCP_OWNER_SERVER, MCP_OWNER_USER,
};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tower_http::request_id::RequestId;
use tracing::warn;

use super::keys::SharedState;
use super::mcp_relay;
use super::spend::SpendAuth;

pub(crate) fn session_header() -> header::HeaderName {
    header::HeaderName::from_static("mcp-session-id")
}

fn tool_name_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("mcp-name")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
}

pub(crate) fn request_id_of(extensions: &http::Extensions) -> String {
    extensions
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string())
}

/// One call's ending, before it becomes an HTTP response.
pub(crate) struct Outcome {
    pub status: StatusCode,
    pub body: Value,
    pub error_code: Option<&'static str>,
    pub session: Option<String>,
}

impl Outcome {
    pub(crate) fn ok(body: Value) -> Self {
        Outcome {
            status: StatusCode::OK,
            body,
            error_code: None,
            session: None,
        }
    }

    pub(crate) fn rpc(status: StatusCode, id: Option<&Value>, code: i64, message: &str) -> Self {
        Outcome {
            status,
            body: session::rpc_error(id, code, message),
            error_code: Some(match code {
                session::METHOD_NOT_FOUND => "method_not_found",
                session::HEADER_BODY_MISMATCH => "header_body_mismatch",
                session::INSUFFICIENT_SCOPE => "insufficient_scope",
                session::PARSE_ERROR => "parse_error",
                _ => "internal_error",
            }),
            session: None,
        }
    }

    pub(crate) fn method_not_allowed(id: Option<&Value>, message: &str) -> Self {
        Outcome {
            status: StatusCode::METHOD_NOT_ALLOWED,
            body: session::rpc_error(id, session::METHOD_NOT_FOUND, message),
            error_code: Some("method_not_allowed"),
            session: None,
        }
    }

    fn upstream_failure(e: &session::UpstreamError, id: Option<&Value>) -> Self {
        Outcome {
            status: StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::BAD_GATEWAY),
            body: session::rpc_error(id, session::INTERNAL_ERROR, e.to_string()),
            error_code: Some(e.code()),
            session: None,
        }
    }

    pub(crate) fn is_failure(&self) -> bool {
        !self.status.is_success() || self.body.get("error").is_some()
    }
}

/// Everything a call needs after the registry lookup and credential resolution.
pub(crate) struct CallContext {
    pub server: McpServerRow,
    pub bearer: Option<String>,
    pub token_owner: &'static str,
}

fn token_owner(source: Option<TokenSource>) -> &'static str {
    match source {
        Some(TokenSource::Server) => "server",
        Some(TokenSource::User) => "user",
        None => "anonymous",
    }
}

fn unrecognised_alias(id: Option<&Value>, alias: &str) -> Outcome {
    Outcome::rpc(
        StatusCode::NOT_FOUND,
        id,
        session::METHOD_NOT_FOUND,
        &format!("no enabled MCP server registered under \"{alias}\""),
    )
}

fn internal(id: Option<&Value>, message: &str) -> Outcome {
    Outcome::rpc(
        StatusCode::INTERNAL_SERVER_ERROR,
        id,
        session::INTERNAL_ERROR,
        message,
    )
}

/// Resolve whose credential goes upstream and decrypt it.
///
/// `user_required` with no usable user token is a 403 here rather than a silent
/// downgrade to the shared token — falling back would hand out permissions the
/// server's scope explicitly withholds.
pub(crate) async fn resolve_bearer(
    state: &SharedState,
    auth: &KeyIdentity,
    server: &McpServerRow,
) -> Result<(Option<String>, &'static str), String> {
    let user_id = auth.user_id.clone().unwrap_or_default();
    let user_token = state
        .db
        .default_mcp_token(server.id, MCP_OWNER_USER, &user_id)
        .await
        .map_err(|e| e.to_string())?;
    let server_token = state
        .db
        .default_mcp_token(server.id, MCP_OWNER_SERVER, "")
        .await
        .map_err(|e| e.to_string())?;

    let scope = server.scope();
    let source = resolve_outbound_token(scope, user_token.as_ref(), server_token.as_ref());
    if scope == CredentialScope::UserRequired && source.is_none() {
        return Err("user_required".to_string());
    }

    let chosen = match source {
        Some(TokenSource::User) => user_token,
        Some(TokenSource::Server) => server_token,
        None => None,
    };
    // Every early return below sends no `Authorization` at all, so the row must
    // say `anonymous` — naming the source would report a credential that never
    // left the gateway.
    let token = match chosen.filter(|t| is_token_usable(t, chrono::Utc::now())) {
        None => return Ok((None, token_owner(None))),
        Some(token) if token.token_value.is_empty() => return Ok((None, token_owner(None))),
        Some(token) => token,
    };
    let mk = state
        .aigw_master_key
        .as_deref()
        .ok_or_else(|| "AIGW_WITHOUT_MASTER_KEY".to_string())?;
    let plain = unseal_token(&token.token_value, mk).map_err(|e| {
        warn!(token_id = token.id, error = %e, "MCP token unseal failed");
        "token_unseal_failed".to_string()
    })?;
    Ok((Some(plain), token_owner(source)))
}

async fn lookup_enabled_server(
    state: &SharedState,
    alias: &str,
    id: Option<&Value>,
) -> Result<McpServerRow, Outcome> {
    state
        .db
        .get_mcp_server_by_alias(alias)
        .await
        .map_err(|e| {
            warn!(alias, error = %e, "MCP server lookup failed");
            internal(id, "could not read the MCP registry")
        })?
        .filter(|s| s.enabled)
        .ok_or_else(|| unrecognised_alias(id, alias))
}

/// The call-side ACL gate. Every endpoint that forwards a `tools/call` runs it,
/// because an allowlist that only one of three entry points honours is not an
/// allowlist: `/mcp/{alias}` and `/mcp-rest/tools/call` reach the same upstream.
///
/// Deliberately *not* part of [`prepare`] / [`prepare_for`]: `tools/list` goes
/// through those too, and a forbidden tool still has to appear in the listing
/// (Stage 144 §3.3) — hiding it would look like the upstream never offered it.
pub(crate) async fn check_tool_allowed(
    state: &SharedState,
    auth: &KeyIdentity,
    server: &McpServerRow,
    tool: &str,
    id: Option<&Value>,
) -> Result<(), Outcome> {
    let rules = state
        .db
        .list_mcp_tool_allowlist(server.id)
        .await
        .map_err(|e| {
            warn!(alias = %server.alias, error = %e, "MCP allowlist read failed");
            internal(id, "could not read the tool allowlist")
        })?;
    if acl::is_allowed(&rules, tool, &Actor::from_identity(auth)) {
        return Ok(());
    }
    Err(Outcome::rpc(
        StatusCode::FORBIDDEN,
        id,
        session::INSUFFICIENT_SCOPE,
        &format!(
            "tool \"{tool}\" on server \"{}\" is not allowed for this identity",
            server.alias
        ),
    ))
}

/// The credential half of [`prepare`], for a caller that already has the row.
async fn prepare_for(
    state: &SharedState,
    auth: &KeyIdentity,
    server: McpServerRow,
    alias: &str,
    id: Option<&Value>,
) -> Result<CallContext, Outcome> {
    match resolve_bearer(state, auth, &server).await {
        Ok((bearer, token_owner)) => Ok(CallContext {
            server,
            bearer,
            token_owner,
        }),
        Err(reason) if reason == "user_required" => Err(Outcome::rpc(
            StatusCode::FORBIDDEN,
            id,
            session::INSUFFICIENT_SCOPE,
            &format!("server \"{alias}\" requires a per-user credential and none is configured"),
        )),
        Err(reason) if reason == "AIGW_WITHOUT_MASTER_KEY" => Err(internal(
            id,
            "AIGW_MASTER_KEY is not configured — cannot decrypt MCP tokens",
        )),
        Err(_) => Err(internal(id, "could not decrypt the upstream credential")),
    }
}

async fn prepare(
    state: &SharedState,
    auth: &KeyIdentity,
    alias: &str,
    id: Option<&Value>,
) -> Result<CallContext, Outcome> {
    let server = lookup_enabled_server(state, alias, id).await?;
    prepare_for(state, auth, server, alias, id).await
}

pub(crate) fn target(ctx: &CallContext) -> UpstreamTarget<'_> {
    UpstreamTarget {
        url: &ctx.server.url,
        bearer: ctx.bearer.as_deref(),
        stack: session::upstream_stack(ctx.server.protocol_version.as_deref()),
        timeout: session::UPSTREAM_TIMEOUT,
    }
}

pub(crate) fn call_row(alias: &str, auth: &KeyIdentity, call_id: String) -> McpCallLogRow {
    McpCallLogRow {
        call_id,
        server_alias: alias.to_string(),
        tool_name: None,
        method: String::new(),
        key_id: Some(auth.token_hash.clone()),
        user_id: auth.user_id.clone(),
        token_owner: "anonymous".to_string(),
        status: MCP_CALL_STATUS_SUCCESS.to_string(),
        error_code: None,
        latency_ms: 0,
        request_bytes: 0,
        response_bytes: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub(crate) fn mark(
    mut row: McpCallLogRow,
    method: Option<&str>,
    tool: Option<String>,
    request_bytes: usize,
    started: std::time::Instant,
) -> McpCallLogRow {
    row.method = method.unwrap_or("unknown").to_string();
    row.tool_name = tool;
    row.request_bytes = request_bytes as i64;
    row.latency_ms = started.elapsed().as_millis() as i64;
    row
}

pub(crate) async fn finish(db: &Database, row: McpCallLogRow, outcome: Outcome) -> Response {
    let mut row = row;
    if outcome.is_failure() {
        row.status = MCP_CALL_STATUS_FAILED.to_string();
        row.error_code = outcome.error_code.map(str::to_string);
    }
    let status = outcome.status;
    let body = serde_json::to_vec(&outcome.body).unwrap_or_default();
    row.response_bytes = body.len() as i64;
    let call_id = row.call_id.clone();
    mcp_relay::record(db, &row).await;

    let mut response = (status, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    if let Some(session) = outcome.session {
        if let Ok(value) = header::HeaderValue::from_str(&session) {
            response.headers_mut().insert(session_header(), value);
        }
    }
    mcp_relay::stamp_call_id(response, &call_id)
}

pub async fn post_mcp(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(alias): Path<String>,
    http::request::Parts { extensions, .. }: http::request::Parts,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = std::time::Instant::now();
    let row = call_row(&alias, &auth, request_id_of(&extensions));

    let parsed = match serde_json::from_slice::<Value>(&body) {
        Ok(value) if value.is_object() => value,
        _ => {
            let row = mark(row, None, None, body.len(), started);
            let outcome = Outcome::rpc(
                StatusCode::BAD_REQUEST,
                None,
                session::PARSE_ERROR,
                "request body is not a JSON-RPC object",
            );
            return finish(&state.db, row, outcome).await;
        }
    };

    let id = session::request_id(&parsed);
    let method = session::request_method(&parsed).map(str::to_string);
    let tool = if method.as_deref() == Some(session::METHOD_TOOLS_CALL) {
        session::tool_name_of(&parsed)
    } else {
        None
    };

    let server = match lookup_enabled_server(&state, &alias, id.as_ref()).await {
        Ok(server) => server,
        Err(outcome) => {
            let row = mark(row, method.as_deref(), tool, body.len(), started);
            return finish(&state.db, row, outcome).await;
        }
    };

    if let Err(message) =
        session::check_header_body_consistency(tool_name_header(&headers), &parsed)
    {
        let row = mark(row, method.as_deref(), tool, body.len(), started);
        let outcome = Outcome::rpc(
            StatusCode::BAD_REQUEST,
            id.as_ref(),
            session::HEADER_BODY_MISMATCH,
            &message,
        );
        return finish(&state.db, row, outcome).await;
    }

    if let Err(message) = session::check_tool_spellings_consistency(&parsed) {
        let row = mark(row, method.as_deref(), tool, body.len(), started);
        let outcome = Outcome::rpc(
            StatusCode::BAD_REQUEST,
            id.as_ref(),
            session::HEADER_BODY_MISMATCH,
            &message,
        );
        return finish(&state.db, row, outcome).await;
    }

    // Registry → ACL → credential, the same order as the aggregate endpoint: a
    // denied call must not reach the token table at all. The envelope's own
    // consistency is checked first, so a malformed request stays a 400 instead of
    // becoming a 403 that hides the real problem.
    if let Some(name) = tool.as_deref() {
        if let Err(outcome) = check_tool_allowed(&state, &auth, &server, name, id.as_ref()).await {
            let row = mark(row, method.as_deref(), tool, body.len(), started);
            return finish(&state.db, row, outcome).await;
        }
    }

    let ctx = match prepare_for(&state, &auth, server, &alias, id.as_ref()).await {
        Ok(ctx) => ctx,
        Err(outcome) => {
            let row = mark(row, method.as_deref(), tool, body.len(), started);
            return finish(&state.db, row, outcome).await;
        }
    };
    let mut row = row;
    row.token_owner = ctx.token_owner.to_string();

    let outcome = match method.as_deref() {
        Some(session::METHOD_DISCOVER) => Outcome::ok(session::discover_response(id.as_ref())),
        Some(session::METHOD_INITIALIZE) => Outcome {
            session: Some(session::session_id()),
            ..Outcome::ok(session::initialize_response(id.as_ref()))
        },
        Some(_) => {
            return relay_forwarded(
                &state,
                ctx,
                parsed,
                row,
                CallTail {
                    method: method.as_deref(),
                    tool,
                    request_bytes: body.len(),
                    started,
                },
            )
            .await
        }
        None => Outcome::rpc(
            StatusCode::BAD_REQUEST,
            id.as_ref(),
            session::PARSE_ERROR,
            "JSON-RPC request has no method",
        ),
    };

    let row = mark(row, method.as_deref(), tool, body.len(), started);
    finish(&state.db, row, outcome).await
}

/// What a call already knows about itself by the time it is relayed. These four
/// values only ever travel together — they are exactly `mark`'s arguments — so
/// bundling them keeps both call sites from growing an argument list.
struct CallTail<'a> {
    method: Option<&'a str>,
    tool: Option<String>,
    request_bytes: usize,
    started: std::time::Instant,
}

impl CallTail<'_> {
    fn stamp(&self, row: McpCallLogRow) -> McpCallLogRow {
        mark(
            row,
            self.method,
            self.tool.clone(),
            self.request_bytes,
            self.started,
        )
    }
}

/// Relay an ordinary MCP method upstream.
///
/// Exactly one `mcp_call_log` row per call: the relay writes it, once, after the
/// body is done — buffered after the bytes are read, streamed after the stream
/// closes. Nothing above this point writes one.
async fn relay_forwarded(
    state: &SharedState,
    ctx: CallContext,
    body: Value,
    row: McpCallLogRow,
    tail: CallTail<'_>,
) -> Response {
    let upstream = match session::forward(&target(&ctx), &body).await {
        Ok(upstream) => upstream,
        Err(e) => {
            let row = tail.stamp(row);
            return finish(&state.db, row, Outcome::upstream_failure(&e, None)).await;
        }
    };
    let row = tail.stamp(row);
    mcp_relay::relay(upstream, state.db.clone(), row).await
}

async fn open_stream(
    state: &SharedState,
    auth: &KeyIdentity,
    alias: &str,
    extensions: &http::Extensions,
    http_method: &str,
    modern: Outcome,
) -> Response {
    let started = std::time::Instant::now();
    let mut row = call_row(alias, auth, request_id_of(extensions));
    row.method = http_method.to_string();

    let ctx = match prepare(state, auth, alias, None).await {
        Ok(ctx) => ctx,
        Err(outcome) => {
            return finish(
                &state.db,
                mark(row, Some(http_method), None, 0, started),
                outcome,
            )
            .await
        }
    };
    row.token_owner = ctx.token_owner.to_string();

    match session::upstream_stack(ctx.server.protocol_version.as_deref()) {
        UpstreamStack::Modern => {
            let row = mark(row, Some(http_method), None, 0, started);
            finish(&state.db, row, modern).await
        }
        UpstreamStack::Legacy => {
            let row = mark(row, Some(http_method), None, 0, started);
            let call_id = row.call_id.clone();
            mcp_relay::record(&state.db, &row).await;
            let response = if http_method == "GET" {
                mcp_relay::notification_stream()
            } else {
                (
                    StatusCode::OK,
                    Json(json!({"status": "session terminated"})),
                )
                    .into_response()
            };
            mcp_relay::stamp_call_id(response, &call_id)
        }
    }
}

pub async fn get_mcp(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(alias): Path<String>,
    http::request::Parts { extensions, .. }: http::request::Parts,
) -> Response {
    let modern = Outcome::method_not_allowed(
        None,
        "the modern MCP stack has no standalone notification stream",
    );
    open_stream(&state, &auth, &alias, &extensions, "GET", modern).await
}

pub async fn delete_mcp(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    Path(alias): Path<String>,
    http::request::Parts { extensions, .. }: http::request::Parts,
) -> Response {
    let modern =
        Outcome::method_not_allowed(None, "the modern MCP stack has no sessions to terminate");
    open_stream(&state, &auth, &alias, &extensions, "DELETE", modern).await
}

/// `mcp_call_log.server_alias` for an aggregate `tools/list` that spanned more
/// than one upstream. A sentinel, not an alias: `validate_alias` admits only
/// `[A-Za-z0-9_.-]`, so no registration can collide with it, and
/// `GET /admin/mcp/calls?server_alias=*aggregate*` stays a plain equality.
pub(crate) const AGGREGATE_ALIAS: &str = "*aggregate*";

/// `POST /mcp` — every enabled server behind one endpoint (Stage 144 §3.1/§3.2).
pub async fn post_mcp_aggregate(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
    http::request::Parts { extensions, .. }: http::request::Parts,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let started = std::time::Instant::now();
    let row = call_row(AGGREGATE_ALIAS, &auth, request_id_of(&extensions));

    let parsed = match serde_json::from_slice::<Value>(&body) {
        Ok(value) if value.is_object() => value,
        _ => {
            let row = mark(row, None, None, body.len(), started);
            let outcome = Outcome::rpc(
                StatusCode::BAD_REQUEST,
                None,
                session::PARSE_ERROR,
                "request body is not a JSON-RPC object",
            );
            return finish(&state.db, row, outcome).await;
        }
    };

    let id = session::request_id(&parsed);
    let method = session::request_method(&parsed).map(str::to_string);

    let is_notification = id.is_none()
        && method
            .as_deref()
            .map(|m| m.starts_with("notifications/"))
            .unwrap_or(false);
    if is_notification {
        let row = mark(row, method.as_deref(), None, body.len(), started);
        mcp_relay::record(&state.db, &row).await;
        return StatusCode::ACCEPTED.into_response();
    }

    if let Err(message) =
        session::check_header_body_consistency(tool_name_header(&headers), &parsed)
    {
        let row = mark(row, method.as_deref(), None, body.len(), started);
        let outcome = Outcome::rpc(
            StatusCode::BAD_REQUEST,
            id.as_ref(),
            session::HEADER_BODY_MISMATCH,
            &message,
        );
        return finish(&state.db, row, outcome).await;
    }

    // Same reason as the header/body check above: a body whose two tool
    // spellings disagree is malformed, so it must be a 400 here — before
    // `call_aggregate_tool` reaches the ACL, which only sees one of them.
    if let Err(message) = session::check_tool_spellings_consistency(&parsed) {
        let row = mark(row, method.as_deref(), None, body.len(), started);
        let outcome = Outcome::rpc(
            StatusCode::BAD_REQUEST,
            id.as_ref(),
            session::HEADER_BODY_MISMATCH,
            &message,
        );
        return finish(&state.db, row, outcome).await;
    }

    match method.as_deref() {
        Some(session::METHOD_DISCOVER) => {
            let outcome = Outcome::ok(session::discover_response(id.as_ref()));
            let row = mark(row, method.as_deref(), None, body.len(), started);
            finish(&state.db, row, outcome).await
        }
        Some(session::METHOD_INITIALIZE) => {
            let outcome = Outcome {
                session: Some(session::session_id()),
                ..Outcome::ok(session::initialize_response(id.as_ref()))
            };
            let row = mark(row, method.as_deref(), None, body.len(), started);
            finish(&state.db, row, outcome).await
        }
        Some(session::METHOD_TOOLS_LIST) => {
            list_aggregate_tools(
                &state,
                &auth,
                &parsed,
                row,
                id.as_ref(),
                body.len(),
                started,
            )
            .await
        }
        Some(session::METHOD_TOOLS_CALL) => {
            call_aggregate_tool(
                &state,
                &auth,
                &parsed,
                row,
                id.as_ref(),
                body.len(),
                started,
            )
            .await
        }
        Some(other) => {
            let outcome = Outcome::method_not_allowed(
                id.as_ref(),
                &format!(
                    "the aggregate endpoint serves tools/list and tools/call; \"{other}\" is only available on /mcp/{{alias}}"
                ),
            );
            let row = mark(row, method.as_deref(), None, body.len(), started);
            finish(&state.db, row, outcome).await
        }
        None => {
            let outcome = Outcome::rpc(
                StatusCode::BAD_REQUEST,
                id.as_ref(),
                session::PARSE_ERROR,
                "JSON-RPC request has no method",
            );
            let row = mark(row, method.as_deref(), None, body.len(), started);
            finish(&state.db, row, outcome).await
        }
    }
}

/// The tools `tools/list` should aggregate for one server: the hot cache,
/// refreshed from the upstream on a miss. A failed refresh returns the stored
/// snapshot and the reason instead of dropping the server — partial success
/// beats a listing that silently loses an upstream (§8).
async fn catalog_tools(
    state: &SharedState,
    server: &McpServerRow,
    bearer: Option<&str>,
) -> (Vec<UpstreamTool>, Option<String>) {
    if let Some(entry) = catalog_cache::get(server.id) {
        return (entry.upstream_tools(), None);
    }

    let others = state
        .db
        .mcp_tool_names_elsewhere(server.id)
        .await
        .unwrap_or_default();

    match probe_server(&server.url, bearer, &others).await {
        Ok(outcome) => {
            let fetched_at = chrono::Utc::now().to_rfc3339();
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
            if let Err(e) = state.db.replace_mcp_tool_catalog(server.id, &rows).await {
                warn!(server_id = server.id, error = %e, "MCP catalog write failed");
            }
            let ttl_ms = outcome
                .ttl_ms
                .filter(|ttl| *ttl > 0)
                .unwrap_or(DEFAULT_TTL_MS);
            catalog_cache::put(server.id, rows, &fetched_at, ttl_ms);
            (outcome.tools, None)
        }
        Err(e) => {
            warn!(alias = %server.alias, error = %e, "MCP catalog probe failed, serving the snapshot");
            let stored = state
                .db
                .list_mcp_tool_catalog(server.id)
                .await
                .unwrap_or_default();
            (
                stored.iter().map(UpstreamTool::from_row).collect(),
                Some(e.to_string()),
            )
        }
    }
}

fn tools_page_cursor(parsed: &Value) -> Option<&str> {
    parsed
        .get("params")
        .and_then(|p| p.get("cursor"))
        .and_then(Value::as_str)
}

fn tools_response(id: Option<&Value>, page: &catalog::Page, warnings: &[Value]) -> Value {
    let mut result = json!({
        "tools": page.entries.iter().map(|entry| entry.to_wire()).collect::<Vec<_>>(),
    });
    if let Some(cursor) = &page.next_cursor {
        result["nextCursor"] = json!(cursor);
    }
    if !warnings.is_empty() {
        result["warnings"] = json!(warnings);
    }
    json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "result": result,
    })
}

/// Merge every visible server's catalogue into one prefixed, deterministically
/// ordered page.
///
/// A `user_required` server with no usable user token is dropped silently (H4):
/// one user who has not configured a token must not fail the whole listing.
/// Every other resolution or probe failure keeps the server and reports a
/// warning, because there the listing is still complete for that upstream.
async fn list_aggregate_tools(
    state: &SharedState,
    auth: &KeyIdentity,
    parsed: &Value,
    mut row: McpCallLogRow,
    id: Option<&Value>,
    request_bytes: usize,
    started: std::time::Instant,
) -> Response {
    let method = Some(session::METHOD_TOOLS_LIST);
    let servers = match state.db.list_mcp_servers().await {
        Ok(servers) => servers,
        Err(e) => {
            warn!(error = %e, "MCP registry read failed");
            let row = mark(row, method, None, request_bytes, started);
            return finish(
                &state.db,
                row,
                internal(id, "could not read the MCP registry"),
            )
            .await;
        }
    };

    let mut visible: Vec<(McpServerRow, Option<String>)> = Vec::new();
    let mut warnings: Vec<Value> = Vec::new();
    for server in servers.into_iter().filter(|s| s.enabled) {
        match resolve_bearer(state, auth, &server).await {
            Ok((bearer, _)) => visible.push((server, bearer)),
            Err(reason) if reason == "user_required" => continue,
            Err(reason) => {
                warn!(alias = %server.alias, reason, "MCP credential resolution failed");
                warnings.push(json!({"alias": server.alias, "reason": reason}));
            }
        }
    }

    // Cold caches cost one upstream round trip each, so they run together: a
    // serial loop in front of three unreachable servers would block the client
    // for 3 × PROBE_TIMEOUT and turn one dead upstream into an unusable
    // aggregate (§8). `join_all` keeps input order, which H3 needs.
    let collected = futures::future::join_all(
        visible
            .iter()
            .map(|(server, bearer)| catalog_tools(state, server, bearer.as_deref())),
    )
    .await;

    let mut sources: Vec<(String, Vec<UpstreamTool>)> = Vec::with_capacity(visible.len());
    for ((server, _), (tools, failure)) in visible.into_iter().zip(collected) {
        if let Some(reason) = failure {
            warnings.push(json!({"alias": server.alias, "reason": reason}));
        }
        sources.push((server.alias, tools));
    }

    if sources.len() == 1 {
        if let Some((alias, _)) = sources.first() {
            row.server_alias = alias.clone();
        }
    }

    let entries = catalog::merge(sources);
    let page = catalog::page(
        &entries,
        tools_page_cursor(parsed),
        catalog::DEFAULT_PAGE_SIZE,
    );
    let outcome = Outcome::ok(tools_response(id, &page, &warnings));
    let row = mark(row, method, None, request_bytes, started);
    finish(&state.db, row, outcome).await
}

/// Route one `tools/call` by its prefix.
///
/// Order is the one Stage 144 §3.2 states, and it is load-bearing: prefix →
/// registry → ACL → credential. An ACL-denied call therefore never touches the
/// token table, and an unseal failure can never mask a denial.
async fn call_aggregate_tool(
    state: &SharedState,
    auth: &KeyIdentity,
    parsed: &Value,
    mut row: McpCallLogRow,
    id: Option<&Value>,
    request_bytes: usize,
    started: std::time::Instant,
) -> Response {
    let method = Some(session::METHOD_TOOLS_CALL);

    let Some(name) = session::tool_name_of(parsed) else {
        let row = mark(row, method, None, request_bytes, started);
        let outcome = Outcome::rpc(
            StatusCode::BAD_REQUEST,
            id,
            session::PARSE_ERROR,
            "tools/call has no params.name",
        );
        return finish(&state.db, row, outcome).await;
    };

    let Some((alias, tool)) = catalog::split_prefixed(&name) else {
        let row = mark(row, method, Some(name.clone()), request_bytes, started);
        let outcome = Outcome::rpc(
            StatusCode::NOT_FOUND,
            id,
            session::METHOD_NOT_FOUND,
            &format!("\"{name}\" is not a prefixed tool name (expected \"{{alias}}__{{tool}}\")"),
        );
        return finish(&state.db, row, outcome).await;
    };
    let alias = alias.to_string();
    let tool = tool.to_string();
    row.server_alias = alias.clone();

    let server = match lookup_enabled_server(state, &alias, id).await {
        Ok(server) => server,
        Err(outcome) => {
            let row = mark(row, method, Some(tool), request_bytes, started);
            return finish(&state.db, row, outcome).await;
        }
    };

    if let Err(outcome) = check_tool_allowed(state, auth, &server, &tool, id).await {
        let row = mark(row, method, Some(tool), request_bytes, started);
        return finish(&state.db, row, outcome).await;
    }

    let ctx = match prepare_for(state, auth, server, &alias, id).await {
        Ok(ctx) => ctx,
        Err(outcome) => {
            let row = mark(row, method, Some(tool), request_bytes, started);
            return finish(&state.db, row, outcome).await;
        }
    };
    row.server_alias = alias;
    row.token_owner = ctx.token_owner.to_string();

    let mut forwarded = parsed.clone();
    forwarded["params"]["name"] = json!(tool);
    // Only `params.name` is rewritten to the unprefixed name; leaving a stale
    // `params.tool` behind would hand the upstream a second, unchecked spelling.
    if let Some(params) = forwarded["params"].as_object_mut() {
        params.remove("tool");
    }

    relay_forwarded(
        state,
        ctx,
        forwarded,
        row,
        CallTail {
            method,
            tool: Some(tool),
            request_bytes,
            started,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::keys::{AppState, DEFAULT_KEY_TOKEN_LEN};
    use aigw_core::mcp::credential::seal_token;
    use aigw_core::models::{McpServerRow, McpServerTokenRow, VirtualKey};
    use axum::body::Body;
    use axum::http::Method;
    use axum::Router;
    use std::sync::Arc;
    use tower::ServiceExt;

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
            .route("/mcp", post(post_mcp_aggregate))
            .route(
                "/mcp/{alias}",
                get(get_mcp).post(post_mcp).delete(delete_mcp),
            )
            .route(
                "/mcp-rest/tools/list",
                post(super::super::mcp_rest::tools_list),
            )
            .route(
                "/mcp-rest/tools/call",
                post(super::super::mcp_rest::tools_call),
            )
            .route("/mcp-rest/health", get(super::super::mcp_rest::health))
            .with_state(state)
    }

    fn server_row(alias: &str, url: &str, scope: &str, protocol: Option<&str>) -> McpServerRow {
        McpServerRow {
            id: 0,
            alias: alias.to_string(),
            display_name: format!("{alias} server"),
            url: url.to_string(),
            transport: "streamable_http".to_string(),
            credential_scope: scope.to_string(),
            enabled: true,
            protocol_version: protocol.map(str::to_string),
            tool_count: 0,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    async fn insert_server(state: &SharedState, server: McpServerRow, token: Option<&str>) -> i64 {
        let id = state.db.create_mcp_server(&server).await.expect("server");
        if let Some(plain) = token {
            let sealed = seal_token(plain, MK).expect("seal");
            let token_id = state
                .db
                .create_mcp_token(&McpServerTokenRow {
                    id: 0,
                    server_id: id,
                    owner_type: MCP_OWNER_SERVER.to_string(),
                    owner_id: String::new(),
                    token_name: "shared".to_string(),
                    token_value: sealed,
                    is_default: true,
                    enabled: true,
                    expires_at: None,
                    created_by: "test".to_string(),
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                })
                .await
                .expect("token");
            state
                .db
                .set_default_mcp_token(id, MCP_OWNER_SERVER, "", token_id)
                .await
                .expect("default token");
        }
        id
    }

    async fn insert_plain_key(state: &SharedState) -> String {
        let raw = format!("sk-{}", uuid::Uuid::new_v4());
        state
            .db
            .insert_key(&VirtualKey {
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
            })
            .await
            .expect("insert key");
        raw
    }

    async fn send(
        app: &Router,
        method: Method,
        uri: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
        body: Value,
    ) -> (u16, Value, http::HeaderMap) {
        let mut builder = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("Content-Type", "application/json");
        if let Some(token) = token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = builder.body(Body::from(body.to_string())).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap_or_default();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value, headers)
    }

    async fn call_log(
        db: &aigw_core::db::Database,
        alias: &str,
    ) -> Vec<aigw_core::models::McpCallLogRow> {
        db.list_mcp_call_log(Some(alias), 50, 0)
            .await
            .expect("log rows")
    }

    fn discover() -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover"})
    }

    #[tokio::test]
    async fn missing_authorization_is_401() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("demo", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let app = build_router(state);
        let (status, _, _) = send(&app, Method::POST, "/mcp/demo", None, &[], discover()).await;
        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn unknown_alias_is_404_with_method_not_found() {
        let state = test_state().await;
        let app = build_router(state);
        let (status, body, _) =
            send(&app, Method::POST, "/mcp/nope", Some(MK), &[], discover()).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], session::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn disabled_alias_is_404_with_method_not_found() {
        let state = test_state().await;
        let mut row = server_row("off", "http://127.0.0.1:1/mcp", "shared", None);
        row.enabled = false;
        insert_server(&state, row, None).await;
        let app = build_router(state);
        let (status, body, _) =
            send(&app, Method::POST, "/mcp/off", Some(MK), &[], discover()).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], session::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn server_discover_answers_the_modern_stack_only() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("demo", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let app = build_router(state.clone());
        let (status, body, headers) =
            send(&app, Method::POST, "/mcp/demo", Some(MK), &[], discover()).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["result"]["supportedVersions"],
            json!([aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION])
        );
        assert!(body["result"].get("protocolVersion").is_none());
        assert!(body["result"].get("serverInfo").is_none());
        let rows = call_log(&state.db, "demo").await;
        assert_eq!(
            headers.get("x-call-id").and_then(|v| v.to_str().ok()),
            Some(rows[0].call_id.as_str()),
            "the handshake row must be reconcilable too"
        );
    }

    #[tokio::test]
    async fn initialize_answers_the_legacy_stack_with_a_session_header() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("demo", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let app = build_router(state);
        let body = json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {}});
        let (status, answer, headers) =
            send(&app, Method::POST, "/mcp/demo", Some(MK), &[], body).await;
        assert_eq!(status, 200, "{answer}");
        assert_eq!(
            answer["result"]["protocolVersion"],
            json!(aigw_core::mcp::probe::LEGACY_PROTOCOL_VERSION)
        );
        let session = headers
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .expect("Mcp-Session-Id header");
        assert_eq!(session.len(), 36, "a UUID v7: {session}");
    }

    #[tokio::test]
    async fn modern_get_is_405() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row(
                "demo",
                "http://127.0.0.1:1/mcp",
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let app = build_router(state);
        let (status, _, _) = send(&app, Method::GET, "/mcp/demo", Some(MK), &[], Value::Null).await;
        assert_eq!(status, 405);
    }

    #[tokio::test]
    async fn legacy_get_opens_a_notification_stream() {
        use tokio_stream::StreamExt;

        let state = test_state().await;
        insert_server(
            &state,
            server_row("legacy", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let app = build_router(state.clone());
        let request = axum::http::Request::builder()
            .method(Method::GET)
            .uri("/mcp/legacy")
            .header("Authorization", format!("Bearer {MK}"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(response.headers().get("x-accel-buffering").unwrap(), "no");
        let call_id = response
            .headers()
            .get("x-call-id")
            .and_then(|v| v.to_str().ok())
            .expect("no x-call-id on the notification stream")
            .to_string();

        let mut frames = response.into_body().into_data_stream();
        let first = tokio::time::timeout(std::time::Duration::from_millis(500), frames.next())
            .await
            .expect("the stream must speak without upstream traffic")
            .expect("a frame")
            .expect("no error");
        assert_eq!(first.as_ref(), mcp_relay::HEARTBEAT_FRAME);

        let rows = call_log(&state.db, "legacy").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].method, "GET");
        assert_eq!(
            call_id, rows[0].call_id,
            "the stream's client still needs to find its audit row"
        );
    }

    #[tokio::test]
    async fn modern_delete_is_405() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row(
                "demo",
                "http://127.0.0.1:1/mcp",
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let app = build_router(state);
        let (status, _, _) = send(
            &app,
            Method::DELETE,
            "/mcp/demo",
            Some(MK),
            &[],
            Value::Null,
        )
        .await;
        assert_eq!(status, 405);
    }

    #[tokio::test]
    async fn header_and_body_tool_names_must_agree() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("demo", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let app = build_router(state);
        let body = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "search"}});
        let (status, answer, _) = send(
            &app,
            Method::POST,
            "/mcp/demo",
            Some(MK),
            &[("Mcp-Name", "read")],
            body,
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(answer["error"]["code"], session::HEADER_BODY_MISMATCH);
        assert!(answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("read"));
    }

    #[tokio::test]
    async fn user_required_without_a_user_token_is_insufficient_scope() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("locked", "http://127.0.0.1:1/mcp", "user_required", None),
            Some("sk-shared-should-not-be-used"),
        )
        .await;
        let raw = insert_plain_key(&state).await;
        let app = build_router(state);
        let (status, answer, _) = send(
            &app,
            Method::POST,
            "/mcp/locked",
            Some(&raw),
            &[],
            discover(),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(answer["error"]["code"], session::INSUFFICIENT_SCOPE);
    }

    #[tokio::test]
    async fn unrecognised_body_is_a_parse_error() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("demo", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let alias = "demo";
        let app = build_router(state.clone());
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri(format!("/mcp/{alias}"))
            .header("Authorization", format!("Bearer {MK}"))
            .body(Body::from("not json"))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status().as_u16(), 400);
        let rows = call_log(&state.db, alias).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, MCP_CALL_STATUS_FAILED);
        assert_eq!(rows[0].error_code.as_deref(), Some("parse_error"));
    }

    // ── passthrough stubs ──────────────────────────────────────────────

    #[derive(Default)]
    struct Recorder {
        methods: Vec<String>,
        authorizations: Vec<Option<String>>,
    }

    type SharedRecorder = Arc<std::sync::Mutex<Recorder>>;

    fn recorded(recorder: &SharedRecorder) -> (Vec<String>, Vec<Option<String>>) {
        let guard = recorder.lock().unwrap();
        (guard.methods.clone(), guard.authorizations.clone())
    }

    /// A JSON-RPC stub that records every method and `Authorization` it sees.
    /// `legacy` selects the old stack: `server/discover` answers 405 and only
    /// `initialize` works.
    async fn spawn_recording_stub(legacy: bool) -> (String, SharedRecorder) {
        let recorder: SharedRecorder = Arc::new(std::sync::Mutex::new(Recorder::default()));
        let sink = recorder.clone();
        let app = Router::new().route(
            "/mcp",
            axum::routing::post(move |headers: http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                let sink = sink.clone();
                async move {
                    let method = body
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    {
                        let mut guard = sink.lock().unwrap();
                        guard.methods.push(method.clone());
                        guard.authorizations.push(auth);
                    }
                    let id = body.get("id").cloned().unwrap_or(Value::Null);
                    if legacy && method == "server/discover" {
                        return (
                            StatusCode::METHOD_NOT_ALLOWED,
                            [(header::CONTENT_TYPE, "application/json")],
                            String::from(
                                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"no"}}"#,
                            ),
                        );
                    }
                    let result = match method.as_str() {
                        "server/discover" => json!({
                            "supportedVersions": [aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION],
                            "ttlMs": 60000,
                        }),
                        "initialize" => json!({
                            "protocolVersion": aigw_core::mcp::probe::LEGACY_PROTOCOL_VERSION,
                            "capabilities": {"tools": {}},
                            "serverInfo": {"name": "stub", "version": "1"},
                        }),
                        "tools/call" => json!({"content": [{"type": "text", "text": "pong"}]}),
                        _ => json!({}),
                    };
                    let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "application/json")],
                        response.to_string(),
                    )
                }
            }),
        );
        let session = if legacy { "session-abc" } else { "" };
        let app = if legacy {
            let session = session.to_string();
            app.layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let session = session.clone();
                    async move {
                        let mut response = next.run(request).await;
                        response.headers_mut().insert(
                            header::HeaderName::from_static("mcp-session-id"),
                            header::HeaderValue::from_str(&session).unwrap(),
                        );
                        response
                    }
                },
            ))
        } else {
            app
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/mcp"), recorder)
    }

    fn tools_call_body() -> Value {
        json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "ping"}})
    }

    #[tokio::test]
    async fn modern_upstream_never_sees_initialize() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(false).await;
        insert_server(
            &state,
            server_row(
                "demo",
                &url,
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            Some("sk-upstream"),
        )
        .await;
        let app = build_router(state);
        send(
            &app,
            Method::POST,
            "/mcp/demo",
            Some(MK),
            &[],
            tools_call_body(),
        )
        .await;
        let (methods, _) = recorded(&recorder);
        assert_eq!(methods, vec!["tools/call".to_string()], "{methods:?}");
    }

    #[tokio::test]
    async fn a_modern_handshake_never_reaches_a_legacy_upstream() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(true).await;
        insert_server(
            &state,
            server_row("demo", &url, "shared", None),
            Some("sk-upstream"),
        )
        .await;
        let app = build_router(state);
        let (status, body, _) =
            send(&app, Method::POST, "/mcp/demo", Some(MK), &[], discover()).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["result"]["supportedVersions"],
            json!([aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION])
        );
        let (methods, _) = recorded(&recorder);
        assert!(
            methods.is_empty(),
            "the inbound handshake is answered here, not forwarded: {methods:?}"
        );
    }

    #[tokio::test]
    async fn a_legacy_upstream_opens_its_own_session_for_a_forwarded_call() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(true).await;
        insert_server(
            &state,
            server_row("demo", &url, "shared", None),
            Some("sk-upstream"),
        )
        .await;
        let app = build_router(state);
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp/demo",
            Some(MK),
            &[],
            tools_call_body(),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["content"][0]["text"], json!("pong"));
        let (methods, _) = recorded(&recorder);
        assert_eq!(
            methods,
            vec![
                "initialize".to_string(),
                "notifications/initialized".to_string(),
                "tools/call".to_string()
            ],
            "the upstream sees no client handshake, only its own: {methods:?}"
        );
    }

    #[tokio::test]
    async fn the_outbound_bearer_comes_from_the_token_table_not_the_client() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(false).await;
        insert_server(
            &state,
            server_row(
                "demo",
                &url,
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            Some("sk-upstream"),
        )
        .await;
        let raw = insert_plain_key(&state).await;
        let app = build_router(state);
        let (status, _, _) = send(
            &app,
            Method::POST,
            "/mcp/demo",
            Some(&raw),
            &[],
            tools_call_body(),
        )
        .await;
        assert_eq!(status, 200);
        let (_, authorizations) = recorded(&recorder);
        assert_eq!(
            authorizations,
            vec![Some("Bearer sk-upstream".to_string())],
            "the client's aigw key must never leave the gateway"
        );
    }

    #[tokio::test]
    async fn a_shared_server_without_a_token_sends_no_authorization_at_all() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(false).await;
        insert_server(
            &state,
            server_row(
                "anon",
                &url,
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let raw = insert_plain_key(&state).await;
        let app = build_router(state);
        let (status, _, _) = send(
            &app,
            Method::POST,
            "/mcp/anon",
            Some(&raw),
            &[],
            tools_call_body(),
        )
        .await;
        assert_eq!(status, 200);
        let (_, authorizations) = recorded(&recorder);
        assert_eq!(
            authorizations,
            vec![None],
            "a tokenless shared server must reach the upstream with no Authorization header at all, \
             and the client's virtual key must never be substituted for it"
        );
    }

    #[tokio::test]
    async fn an_expired_shared_token_sends_no_authorization_and_logs_anonymous() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(false).await;
        let id = insert_server(
            &state,
            server_row(
                "stale",
                &url,
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let sealed = seal_token("sk-expired", MK).expect("seal");
        let token_id = state
            .db
            .create_mcp_token(&McpServerTokenRow {
                id: 0,
                server_id: id,
                owner_type: MCP_OWNER_SERVER.to_string(),
                owner_id: String::new(),
                token_name: "shared".to_string(),
                token_value: sealed,
                is_default: true,
                enabled: true,
                expires_at: Some((chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339()),
                created_by: "test".to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .await
            .expect("token");
        state
            .db
            .set_default_mcp_token(id, MCP_OWNER_SERVER, "", token_id)
            .await
            .expect("default token");
        let app = build_router(state.clone());
        let (status, _, _) = send(
            &app,
            Method::POST,
            "/mcp/stale",
            Some(MK),
            &[],
            tools_call_body(),
        )
        .await;
        assert_eq!(status, 200);
        let (_, authorizations) = recorded(&recorder);
        assert_eq!(
            authorizations,
            vec![None],
            "an expired token is withheld, so the upstream saw no credential"
        );
        let rows = call_log(&state.db, "stale").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].token_owner, "anonymous",
            "the row must say no credential went out, not name the token it declined to send"
        );
    }

    #[tokio::test]
    async fn an_upstream_500_is_relayed_and_logged_as_failed() {
        let state = test_state().await;
        let app_router = Router::new().route(
            "/mcp",
            axum::routing::post(|| async {
                (StatusCode::INTERNAL_SERVER_ERROR, "upstream exploded")
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app_router).await;
        });
        insert_server(
            &state,
            server_row(
                "broken",
                &format!("http://{addr}/mcp"),
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let app = build_router(state.clone());
        let (status, _, _) = send(
            &app,
            Method::POST,
            "/mcp/broken",
            Some(MK),
            &[],
            tools_call_body(),
        )
        .await;
        assert_eq!(status, 500);
        let rows = call_log(&state.db, "broken").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, MCP_CALL_STATUS_FAILED);
        assert_eq!(rows[0].method, "tools/call");
        assert_eq!(rows[0].tool_name.as_deref(), Some("ping"));
    }

    #[tokio::test]
    async fn a_jsonrpc_error_behind_http_200_is_relayed_and_logged_as_failed() {
        let state = test_state().await;
        let answer = r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32602,"message":"bad"}}"#;
        let app_router = Router::new().route(
            "/mcp",
            axum::routing::post(move || async move { (StatusCode::OK, answer) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app_router).await;
        });
        insert_server(
            &state,
            server_row(
                "picky",
                &format!("http://{addr}/mcp"),
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let app = build_router(state.clone());
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/mcp/picky")
            .header("Authorization", format!("Bearer {MK}"))
            .header("Content-Type", "application/json")
            .body(Body::from(tools_call_body().to_string()))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            200,
            "an HTTP 200 is still an HTTP 200 to the client"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        assert_eq!(
            bytes.as_ref(),
            answer.as_bytes(),
            "the relayed body must stay byte-for-byte what the upstream sent"
        );

        let rows = call_log(&state.db, "picky").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, MCP_CALL_STATUS_FAILED);
        assert_eq!(
            rows[0].error_code.as_deref(),
            Some("-32602"),
            "the JSON-RPC error code, not a generic label"
        );
    }

    #[tokio::test]
    async fn tools_rest_list_does_not_negotiate_with_the_upstream() {
        let state = test_state().await;
        let (url, recorder) = spawn_recording_stub(false).await;
        let id = insert_server(
            &state,
            server_row("demo", &url, "shared", None),
            Some("sk-upstream"),
        )
        .await;
        state
            .db
            .replace_mcp_tool_catalog(
                id,
                &[aigw_core::models::McpToolRow {
                    id: 0,
                    server_id: id,
                    tool_name: "ping".to_string(),
                    description: "ping tool".to_string(),
                    input_schema: json!({"type": "object"}),
                    annotations: None,
                    has_conflict: false,
                    fetched_at: "2026-01-01T00:00:00Z".to_string(),
                }],
            )
            .await
            .unwrap();
        let app = build_router(state.clone());
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp-rest/tools/list",
            Some(MK),
            &[],
            json!({"alias": "demo"}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["tools"][0]["tool_name"], json!("ping"));
        let (methods, _) = recorded(&recorder);
        assert!(methods.is_empty(), "no upstream traffic: {methods:?}");
        let rows = call_log(&state.db, "demo").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].method, "tools/list");
        assert_eq!(rows[0].status, MCP_CALL_STATUS_SUCCESS);
    }

    #[tokio::test]
    async fn tools_rest_list_omits_a_user_required_server_the_caller_cannot_use() {
        let state = test_state().await;
        insert_server(
            &state,
            server_row("locked", "http://127.0.0.1:1/mcp", "user_required", None),
            Some("sk-shared-unused"),
        )
        .await;
        let raw = insert_plain_key(&state).await;
        let app = build_router(state.clone());
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp-rest/tools/list",
            Some(&raw),
            &[],
            json!({}),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["servers"], json!([]));
        assert_eq!(body["tools"], json!([]));
    }

    #[tokio::test]
    async fn a_streamed_call_writes_exactly_one_log_row() {
        let state = test_state().await;
        let app_router = Router::new().route(
            "/mcp",
            axum::routing::post(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/event-stream")],
                    r#"data: {"jsonrpc":"2.0","id":9,"result":{"content":[]}}
"#,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app_router).await;
        });
        insert_server(
            &state,
            server_row(
                "streaming",
                &format!("http://{addr}/mcp"),
                "shared",
                Some(aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION),
            ),
            None,
        )
        .await;
        let app = build_router(state.clone());
        let (status, _, headers) = send(
            &app,
            Method::POST,
            "/mcp/streaming",
            Some(MK),
            &[],
            tools_call_body(),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(headers.get("x-accel-buffering").unwrap(), "no");
        let rows = call_log(&state.db, "streaming").await;
        assert_eq!(rows.len(), 1, "one call, one row: {rows:?}");
        assert_eq!(rows[0].status, MCP_CALL_STATUS_SUCCESS);
    }

    #[tokio::test]
    async fn a_failed_lookup_is_still_logged() {
        let state = test_state().await;
        let app = build_router(state.clone());
        let (status, _, _) =
            send(&app, Method::POST, "/mcp/ghost", Some(MK), &[], discover()).await;
        assert_eq!(status, 404);
        let rows = call_log(&state.db, "ghost").await;
        assert_eq!(
            rows.len(),
            1,
            "a failed call is the main troubleshooting input"
        );
        assert_eq!(rows[0].status, MCP_CALL_STATUS_FAILED);
        assert_eq!(rows[0].method, "server/discover");
        assert_eq!(rows[0].key_id.as_deref(), Some("*master*"));
        assert_eq!(rows[0].token_owner, "anonymous");
    }

    // ── aggregate endpoint ─────────────────────────────────────────────

    /// The catalog cache is a process static keyed by server id (ADR-041), and
    /// every test here has its own in-memory DB whose ids start at 1 — so two
    /// aggregate tests running at once would serve each other's tools. The lock
    /// plus a `clear()` on entry is what makes the cache assertions mean
    /// something; without it a steady-state `tools/list` would sometimes hit an
    /// entry another test wrote.
    async fn aggregate_guard() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        let guard = LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        catalog_cache::clear();
        guard
    }

    /// A modern upstream that answers `tools/list` with a fixed catalogue and
    /// echoes the `tools/call` name, so a test can see which name went out.
    async fn spawn_tools_stub(tools: Vec<String>) -> (String, SharedRecorder) {
        let tools: Arc<Vec<String>> = Arc::new(tools);
        let recorder: SharedRecorder = Arc::new(std::sync::Mutex::new(Recorder::default()));
        let sink = recorder.clone();
        let app = Router::new().route(
            "/mcp",
            axum::routing::post(
                move |headers: http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                    let sink = sink.clone();
                    let tools = tools.clone();
                    async move {
                        let method = body
                            .get("method")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let name = body["params"]["name"].as_str().unwrap_or_default();
                        let auth = headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);
                        {
                            let mut guard = sink.lock().unwrap();
                            guard.methods.push(if method == "tools/call" {
                                format!("tools/call:{name}")
                            } else {
                                method.clone()
                            });
                            guard.authorizations.push(auth);
                        }
                        let id = body.get("id").cloned().unwrap_or(Value::Null);
                        let result = match method.as_str() {
                            "server/discover" => json!({
                                "supportedVersions": [aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION],
                                "ttlMs": 60000,
                            }),
                            "tools/list" => json!({
                                "tools": tools
                                    .iter()
                                    .map(|name| json!({
                                        "name": name,
                                        "description": format!("{name} tool"),
                                        "inputSchema": {"type": "object"},
                                    }))
                                    .collect::<Vec<_>>(),
                            }),                            "tools/call" => json!({"content": [{"type": "text", "text": name}]}),
                            _ => json!({}),
                        };
                        (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string(),
                        )
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}/mcp"), recorder)
    }

    fn tools_list_body(cursor: Option<&str>) -> Value {
        let mut params = json!({});
        if let Some(cursor) = cursor {
            params["cursor"] = json!(cursor);
        }
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": params})
    }

    fn prefixed_call(name: &str) -> Value {
        json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": name, "arguments": {}}})
    }

    fn tool_names(body: &Value) -> Vec<String> {
        body["result"]["tools"]
            .as_array()
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|t| t["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn stub_tools(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn numbered_tools(count: usize) -> Vec<String> {
        (1..=count).map(|i| format!("t{i}")).collect()
    }

    fn calls_seen(recorder: &SharedRecorder) -> Vec<String> {
        recorded(recorder)
            .0
            .into_iter()
            .filter(|m| m.starts_with("tools/call:"))
            .collect()
    }

    #[tokio::test]
    async fn aggregate_list_merges_every_server_under_its_prefix() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (alpha, _) = spawn_tools_stub(stub_tools(&["search"])).await;
        let (beta, _) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(&state, server_row("alpha", &alpha, "shared", None), None).await;
        insert_server(&state, server_row("beta", &beta, "shared", None), None).await;
        let app = build_router(state.clone());

        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            tools_list_body(None),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            tool_names(&body),
            vec!["alpha__search".to_string(), "beta__read".to_string()]
        );

        let rows = call_log(&state.db, AGGREGATE_ALIAS).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].method, "tools/list");
        assert_eq!(rows[0].status, MCP_CALL_STATUS_SUCCESS);
    }

    #[tokio::test]
    async fn aggregate_call_routes_to_the_owning_upstream_with_the_unprefixed_name() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (alpha, alpha_log) = spawn_tools_stub(stub_tools(&["search"])).await;
        let (beta, beta_log) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(
            &state,
            server_row("alpha", &alpha, "shared", None),
            Some("sk-alpha"),
        )
        .await;
        insert_server(
            &state,
            server_row("beta", &beta, "shared", None),
            Some("sk-beta"),
        )
        .await;
        let app = build_router(state.clone());

        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            prefixed_call("beta__read"),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["result"]["content"][0]["text"], json!("read"));
        assert_eq!(calls_seen(&alpha_log), Vec::<String>::new());
        assert_eq!(calls_seen(&beta_log), vec!["tools/call:read".to_string()]);

        let (_, beta_auth) = recorded(&beta_log);
        assert_eq!(
            beta_auth.last().cloned().flatten().as_deref(),
            Some("Bearer sk-beta"),
            "the aggregate path sends the server row, never the client key"
        );

        let rows = call_log(&state.db, "beta").await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tool_name.as_deref(), Some("read"));
        assert_eq!(rows[0].token_owner, "server");
    }

    #[tokio::test]
    async fn an_unregistered_prefix_is_a_404_method_not_found() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let app = build_router(state.clone());
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            prefixed_call("ghost__search"),
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], session::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn an_unprefixed_tool_name_is_a_404_method_not_found() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let app = build_router(state.clone());
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            prefixed_call("search"),
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], session::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn a_user_required_server_without_a_user_token_is_hidden_from_the_list() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        insert_server(
            &state,
            server_row("locked", "http://127.0.0.1:1/mcp", "user_required", None),
            Some("sk-unused"),
        )
        .await;
        let (open, _) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(&state, server_row("open", &open, "shared", None), None).await;
        let raw = insert_plain_key(&state).await;
        let app = build_router(state.clone());

        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(&raw),
            &[],
            tools_list_body(None),
        )
        .await;
        assert_eq!(
            status, 200,
            "one missing personal token must not fail the listing"
        );
        assert_eq!(tool_names(&body), vec!["open__read".to_string()]);
        assert!(body["result"].get("warnings").is_none(), "{body}");
    }

    #[tokio::test]
    async fn a_single_visible_server_logs_its_own_alias() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (only, _) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(&state, server_row("solo", &only, "shared", None), None).await;
        let app = build_router(state.clone());

        let (status, _, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            tools_list_body(None),
        )
        .await;
        assert_eq!(status, 200);
        let rows = call_log(&state.db, "solo").await;
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn an_acl_denied_tool_is_still_listed_and_its_call_is_a_403() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (upstream, upstream_log) = spawn_tools_stub(stub_tools(&["search"])).await;
        let id = insert_server(
            &state,
            server_row("demo", &upstream, "shared", None),
            Some("sk-demo"),
        )
        .await;
        let raw = insert_plain_key(&state).await;
        state
            .db
            .replace_mcp_tool_allowlist(
                id,
                &[aigw_core::mcp::acl::ToolRule {
                    tool_name: "search".to_string(),
                    scope_type: aigw_core::mcp::acl::SCOPE_KEY.to_string(),
                    scope_id: aigw_core::crypto::hash_token(&raw),
                    allowed: false,
                }],
            )
            .await
            .unwrap();
        let app = build_router(state.clone());

        let (list_status, list_body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(&raw),
            &[],
            tools_list_body(None),
        )
        .await;
        assert_eq!(list_status, 200);
        assert_eq!(
            tool_names(&list_body),
            vec!["demo__search".to_string()],
            "a forbidden tool stays visible: hiding it would look like the upstream never had it"
        );

        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(&raw),
            &[],
            prefixed_call("demo__search"),
        )
        .await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"]["code"], session::INSUFFICIENT_SCOPE);
        assert_eq!(
            calls_seen(&upstream_log),
            Vec::<String>::new(),
            "an ACL denial must not reach the upstream"
        );

        let rows = call_log(&state.db, "demo").await;
        let denied = rows
            .iter()
            .find(|r| r.method == "tools/call")
            .expect("the denial is logged like any other failure");
        assert_eq!(denied.status, MCP_CALL_STATUS_FAILED);
        assert_eq!(denied.error_code.as_deref(), Some("insufficient_scope"));
    }

    #[tokio::test]
    async fn an_unreachable_upstream_keeps_the_other_servers_and_reports_a_warning() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        insert_server(
            &state,
            server_row("down", "http://127.0.0.1:1/mcp", "shared", None),
            None,
        )
        .await;
        let (up, _) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(&state, server_row("up", &up, "shared", None), None).await;
        let app = build_router(state.clone());

        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            tools_list_body(None),
        )
        .await;
        assert_eq!(
            status, 200,
            "partial success beats an empty listing: {body}"
        );
        assert_eq!(tool_names(&body), vec!["up__read".to_string()]);
        assert_eq!(body["result"]["warnings"][0]["alias"], json!("down"));
    }

    #[tokio::test]
    async fn a_cached_catalogue_does_not_reach_the_upstream_again() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (up, recorder) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(&state, server_row("demo", &up, "shared", None), None).await;
        let app = build_router(state.clone());

        for _ in 0..2 {
            let (status, _, _) = send(
                &app,
                Method::POST,
                "/mcp",
                Some(MK),
                &[],
                tools_list_body(None),
            )
            .await;
            assert_eq!(status, 200);
        }
        let lists = recorded(&recorder)
            .0
            .into_iter()
            .filter(|m| m == "tools/list")
            .count();
        assert_eq!(lists, 1, "the second listing came from the cache");
    }

    #[tokio::test]
    async fn the_gateway_cursor_pages_without_overlap_and_ignores_upstream_cursors() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (up, _) = spawn_tools_stub(numbered_tools(catalog::DEFAULT_PAGE_SIZE + 1)).await;
        insert_server(&state, server_row("big", &up, "shared", None), None).await;
        let app = build_router(state.clone());

        let (first_status, first, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            tools_list_body(None),
        )
        .await;
        assert_eq!(first_status, 200);
        let first_names = tool_names(&first);
        assert_eq!(first_names.len(), catalog::DEFAULT_PAGE_SIZE);
        let cursor = first["result"]["nextCursor"]
            .as_str()
            .expect("one more tool than a page produces a cursor")
            .to_string();

        let (second_status, second, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            tools_list_body(Some(&cursor)),
        )
        .await;
        assert_eq!(second_status, 200);
        let second_names = tool_names(&second);
        assert!(
            first_names.iter().all(|n| !second_names.contains(n)),
            "pages overlap: {first_names:?} / {second_names:?}"
        );
        assert!(second["result"].get("nextCursor").is_none());
    }

    #[tokio::test]
    async fn aggregate_discover_and_initialize_are_answered_without_an_upstream() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (up, recorder) = spawn_tools_stub(stub_tools(&["read"])).await;
        insert_server(&state, server_row("demo", &up, "shared", None), None).await;
        let app = build_router(state.clone());

        let (status, body, _) = send(&app, Method::POST, "/mcp", Some(MK), &[], discover()).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["result"]["supportedVersions"],
            json!([aigw_core::mcp::probe::MODERN_PROTOCOL_VERSION])
        );

        let (status, _, headers) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {}}),
        )
        .await;
        assert_eq!(status, 200);
        assert!(headers.get("mcp-session-id").is_some());
        assert!(
            recorded(&recorder).0.is_empty(),
            "neither call reached an upstream"
        );
    }

    #[tokio::test]
    async fn a_method_the_aggregate_endpoint_does_not_serve_is_a_405() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let app = build_router(state.clone());
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[],
            json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list", "params": {}}),
        )
        .await;
        assert_eq!(status, 405);
        assert_eq!(body["error"]["code"], session::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn aggregate_header_and_body_tool_names_must_still_agree() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (up, _) = spawn_tools_stub(stub_tools(&["search"])).await;
        insert_server(&state, server_row("demo", &up, "shared", None), None).await;
        let app = build_router(state.clone());
        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp",
            Some(MK),
            &[("Mcp-Name", "demo__other")],
            prefixed_call("demo__search"),
        )
        .await;
        assert_eq!(status, 400);
        assert_eq!(body["error"]["code"], session::HEADER_BODY_MISMATCH);
    }

    #[tokio::test]
    async fn a_forbidden_params_tool_cannot_hide_behind_an_allowed_params_name() {
        let _guard = aggregate_guard().await;
        let state = test_state().await;
        let (upstream, upstream_log) = spawn_tools_stub(stub_tools(&["search"])).await;
        let id = insert_server(
            &state,
            server_row("demo", &upstream, "shared", None),
            Some("sk-demo"),
        )
        .await;
        let raw = insert_plain_key(&state).await;
        let rules = |tool: &str, allowed: bool| aigw_core::mcp::acl::ToolRule {
            tool_name: tool.to_string(),
            scope_type: aigw_core::mcp::acl::SCOPE_KEY.to_string(),
            scope_id: aigw_core::crypto::hash_token(&raw),
            allowed,
        };
        state
            .db
            .replace_mcp_tool_allowlist(id, &[rules("search", true), rules("forbidden", false)])
            .await
            .unwrap();
        let app = build_router(state.clone());

        let (status, body, _) = send(
            &app,
            Method::POST,
            "/mcp/demo",
            Some(&raw),
            &[],
            json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "tools/call",
                "params": {"name": "search", "tool": "forbidden"},
            }),
        )
        .await;
        assert_eq!(
            status, 400,
            "two tool spellings that disagree are malformed, not merely forbidden: {body}"
        );
        assert_eq!(body["error"]["code"], session::HEADER_BODY_MISMATCH);
        assert_eq!(
            calls_seen(&upstream_log),
            Vec::<String>::new(),
            "the malformed request must never reach the upstream"
        );
    }
}
