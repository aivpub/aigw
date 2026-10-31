//! `/mcp-rest/*` — stateless MCP debug endpoints (Stage 143 §3.4).
//!
//! Plain JSON in, JSON out, and **no protocol negotiation**: the point is that a
//! BDD scenario or an operator with `curl` can reach an upstream without
//! reproducing a dual-stack handshake. That is also what makes their behaviour
//! differ from the agent path, which is why `general_settings.mcp_disable_rest`
//! exists and production is expected to switch them off.
//!
//! The endpoints stay registered when disabled and answer 404 from inside the
//! handler: a route table that changes shape with configuration is harder to
//! build in tests than a status code.

use aigw_core::mcp::catalog_cache;
use aigw_core::mcp::probe::{read_jsonrpc_response, PROBE_TIMEOUT};
use aigw_core::mcp::session::{self, METHOD_TOOLS_CALL, METHOD_TOOLS_LIST};
use aigw_core::models::{McpServerRow, McpToolRow, UpstreamTool, MCP_CALL_STATUS_FAILED};
use axum::extract::State;
use axum::http;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::warn;

use super::keys::SharedState;
use super::mcp_gateway::{
    call_row, check_tool_allowed, effective_owner, finish, mark, outbound_headers, request_id_of,
    resolve_bearer, target, CallContext, McpAuditAuth, Outcome,
};
use super::mcp_relay;

#[derive(Debug, Deserialize, Serialize)]
pub struct ToolsListBody {
    pub alias: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ToolsCallBody {
    pub alias: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

/// The request as the audit trail stores it: what the client sent, re-serialised
/// from the parsed body. These endpoints have no raw envelope — the extractor
/// consumed it — and a parsed body written back out is the same JSON.
fn request_bytes<T: Serialize>(body: &T) -> Vec<u8> {
    serde_json::to_vec(body).unwrap_or_default()
}

fn disabled() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": {"message": "not found", "type": "not_found"}})),
    )
        .into_response()
}

fn upstream_error(status: StatusCode, message: &str, code: &str) -> Response {
    (
        status,
        Json(json!({"error": {"message": message, "type": "upstream_error", "code": code}})),
    )
        .into_response()
}

async fn enabled_servers(state: &SharedState) -> Vec<McpServerRow> {
    state
        .db
        .list_mcp_servers()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.enabled)
        .collect()
}

/// The cached tools of `server`. Never probes: a debug `tools/list` that could
/// reach the upstream would be indistinguishable from the agent path, and the
/// cache/snapshot pair exists precisely so it does not have to.
fn snapshot(server: &McpServerRow, rows: Vec<McpToolRow>) -> Vec<McpToolRow> {
    match catalog_cache::get(server.id) {
        Some(entry) => entry.tools.as_ref().clone(),
        None => rows,
    }
}

/// A `tools/list` is a client call, so it is logged like any other — including
/// when it fails. `health` is not: it is a probe of the registry, not a call a
/// client made, and logging it would drown the audit trail in operator probes.
pub async fn tools_list(
    State(state): State<SharedState>,
    McpAuditAuth(auth): McpAuditAuth,
    http::request::Parts { extensions, .. }: http::request::Parts,
    Json(body): Json<ToolsListBody>,
) -> Response {
    if state.mcp_disable_rest {
        return disabled();
    }

    let started = std::time::Instant::now();
    let request = request_bytes(&body);
    let row = call_row(
        body.alias.as_deref().unwrap_or("*"),
        &auth,
        request_id_of(&extensions),
    );
    let row = mark(row, Some(METHOD_TOOLS_LIST), None, 0, started);

    let servers = match body.alias.as_deref() {
        Some(alias) => match state.db.get_mcp_server_by_alias(alias).await {
            Ok(Some(server)) if server.enabled => vec![server],
            Ok(_) => {
                return finish(
                    &state,
                    row,
                    Outcome::rpc(
                        StatusCode::NOT_FOUND,
                        None,
                        session::METHOD_NOT_FOUND,
                        &format!("no enabled MCP server registered under \"{alias}\""),
                    ),
                    &request,
                )
                .await
            }
            Err(e) => {
                warn!(alias, error = %e, "MCP server lookup failed");
                return finish(
                    &state,
                    row,
                    Outcome::rpc(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        None,
                        session::INTERNAL_ERROR,
                        "could not read the MCP registry",
                    ),
                    &request,
                )
                .await;
            }
        },
        None => enabled_servers(&state).await,
    };

    let mut tools: Vec<UpstreamTool> = Vec::new();
    let mut server_views = Vec::new();
    let mut visible: Vec<String> = Vec::new();
    for server in &servers {
        match resolve_bearer(&state, &auth, server).await {
            Ok(_) => {}
            Err(reason) if reason == "user_required" => continue,
            Err(reason) => {
                warn!(alias = %server.alias, reason, "MCP credential resolution failed");
                continue;
            }
        }
        let stored = state
            .db
            .list_mcp_tool_catalog(server.id)
            .await
            .unwrap_or_default();
        let rows = snapshot(server, stored);
        server_views.push(json!({
            "alias": server.alias,
            "tool_count": rows.len(),
            "protocol_version": server.protocol_version,
        }));
        tools.extend(rows.iter().map(UpstreamTool::from_row));
        visible.push(server.alias.clone());
    }

    let mut row = row;
    if visible.len() == 1 {
        row.server_alias = visible.remove(0);
    }
    row.latency_ms = started.elapsed().as_millis() as i64;
    let call_id = row.call_id.clone();
    let answer = json!({"tools": tools, "servers": server_views});
    let response = serde_json::to_vec(&answer).unwrap_or_default();
    row.response_bytes = response.len() as i64;
    let limit = mcp_relay::body_limit(&state.mcp_log_retention);
    mcp_relay::record_with_bodies(&state.db, limit, &row, &request, &response).await;
    mcp_relay::stamp_call_id((StatusCode::OK, Json(answer)).into_response(), &call_id)
}

pub async fn tools_call(
    State(state): State<SharedState>,
    McpAuditAuth(auth): McpAuditAuth,
    http::request::Parts { extensions, .. }: http::request::Parts,
    Json(body): Json<ToolsCallBody>,
) -> Response {
    if state.mcp_disable_rest {
        return disabled();
    }

    let started = std::time::Instant::now();
    let request = request_bytes(&body);
    let arguments_bytes = body.arguments.to_string().len();
    let mut row = call_row(&body.alias, &auth, request_id_of(&extensions));
    row.method = METHOD_TOOLS_CALL.to_string();
    row.tool_name = Some(body.tool.clone());
    row.request_bytes = arguments_bytes as i64;

    let server = match state.db.get_mcp_server_by_alias(&body.alias).await {
        Ok(Some(server)) if server.enabled => server,
        Ok(_) => {
            let row = mark(
                row,
                Some(METHOD_TOOLS_CALL),
                Some(body.tool),
                arguments_bytes,
                started,
            );
            return finish(
                &state,
                row,
                Outcome::rpc(
                    StatusCode::NOT_FOUND,
                    None,
                    session::METHOD_NOT_FOUND,
                    &format!("no enabled MCP server registered under \"{}\"", body.alias),
                ),
                &request,
            )
            .await;
        }
        Err(e) => {
            warn!(alias = %body.alias, error = %e, "MCP server lookup failed");
            let row = mark(
                row,
                Some(METHOD_TOOLS_CALL),
                Some(body.tool),
                arguments_bytes,
                started,
            );
            return finish(
                &state,
                row,
                Outcome::rpc(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    None,
                    session::INTERNAL_ERROR,
                    "could not read the MCP registry",
                ),
                &request,
            )
            .await;
        }
    };

    if let Err(outcome) = check_tool_allowed(&state, &auth, &server, &body.tool, None).await {
        let row = mark(
            row,
            Some(METHOD_TOOLS_CALL),
            Some(body.tool),
            arguments_bytes,
            started,
        );
        return finish(&state, row, outcome, &request).await;
    }

    let (bearer, token_owner) = match resolve_bearer(&state, &auth, &server).await {
        Ok(pair) => pair,
        Err(reason) if reason == "user_required" => {
            let row = mark(
                row,
                Some(METHOD_TOOLS_CALL),
                Some(body.tool),
                arguments_bytes,
                started,
            );
            return finish(
                &state,
                row,
                Outcome::rpc(
                    StatusCode::FORBIDDEN,
                    None,
                    session::INSUFFICIENT_SCOPE,
                    "this server requires a per-user credential and none is configured",
                ),
                &request,
            )
            .await;
        }
        Err(reason) => {
            warn!(alias = %body.alias, reason, "MCP credential resolution failed");
            let row = mark(
                row,
                Some(METHOD_TOOLS_CALL),
                Some(body.tool),
                arguments_bytes,
                started,
            );
            return finish(
                &state,
                row,
                Outcome::rpc(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    None,
                    session::INTERNAL_ERROR,
                    "could not decrypt the upstream credential",
                ),
                &request,
            )
            .await;
        }
    };
    let headers = outbound_headers(&state, &server, bearer.as_deref());
    let token_owner = effective_owner(&headers, token_owner);
    row.token_owner = token_owner.to_string();

    let ctx = CallContext {
        headers,
        server,
        token_owner,
    };
    let envelope = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": METHOD_TOOLS_CALL,
        "params": {"name": body.tool, "arguments": body.arguments},
    });

    // The answer is kept as a value until the audit row is written: the body
    // recorded must be the one the client receives, not a second rendering of it.
    let (response, answer): (Response, Value) = match session::forward(&target(&ctx), &envelope)
        .await
    {
        Ok(upstream) => match read_jsonrpc_response(upstream).await {
            Ok(answer) => {
                let response = (StatusCode::OK, Json(answer.clone())).into_response();
                (response, answer)
            }
            Err(e) => {
                warn!(alias = %row.server_alias, error = %e, "upstream answer was unreadable");
                row.status = MCP_CALL_STATUS_FAILED.to_string();
                row.error_code = Some("upstream_response_invalid".to_string());
                let response = upstream_error(
                    StatusCode::BAD_GATEWAY,
                    &e.to_string(),
                    "upstream_response_invalid",
                );
                (response, json!({"error": {"message": e.to_string()}}))
            }
        },
        Err(e) => {
            row.status = MCP_CALL_STATUS_FAILED.to_string();
            row.error_code = Some(e.code().to_string());
            let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::BAD_GATEWAY);
            let response = upstream_error(status, &e.to_string(), e.code());
            (response, json!({"error": {"message": e.to_string()}}))
        }
    };
    let answer_bytes = serde_json::to_vec(&answer).unwrap_or_default();
    row.response_bytes = answer_bytes.len() as i64;
    row.latency_ms = started.elapsed().as_millis() as i64;
    let call_id = row.call_id.clone();
    let limit = mcp_relay::body_limit(&state.mcp_log_retention);
    mcp_relay::record_with_bodies(&state.db, limit, &row, &request, &answer_bytes).await;
    mcp_relay::stamp_call_id(response, &call_id)
}

pub async fn health(
    State(state): State<SharedState>,
    McpAuditAuth(auth): McpAuditAuth,
) -> Response {
    if state.mcp_disable_rest {
        return disabled();
    }

    let mut servers = Vec::new();
    for server in enabled_servers(&state).await {
        let started = std::time::Instant::now();
        let bearer = match resolve_bearer(&state, &auth, &server).await {
            Ok((bearer, _)) => bearer,
            Err(reason) => {
                servers.push(json!({
                    "alias": server.alias,
                    "reachable": false,
                    "protocol_version": server.protocol_version,
                    "latency_ms": started.elapsed().as_millis() as i64,
                    "error": reason,
                }));
                continue;
            }
        };
        let headers = outbound_headers(&state, &server, bearer.as_deref());
        let target = session::UpstreamTarget {
            url: &server.url,
            auth: headers.auth(),
            extra_headers: headers.extra(),
            stack: session::upstream_stack(server.protocol_version.as_deref()),
            timeout: PROBE_TIMEOUT,
        };
        let status = session::reachable(&target).await;
        servers.push(json!({
            "alias": server.alias,
            "reachable": status.is_some(),
            "protocol_version": server.protocol_version,
            "latency_ms": started.elapsed().as_millis() as i64,
            "http_status": status,
        }));
    }

    (StatusCode::OK, Json(json!({"servers": servers}))).into_response()
}
