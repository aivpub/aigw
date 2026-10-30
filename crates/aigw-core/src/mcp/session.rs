//! Dual-stack MCP negotiation (Stage 143).
//!
//! Two stacks must be spoken at once: Claude Code opens with `server/discover`
//! (`2026-07-28`, no session), Codex sends only `initialize` (`2025-06-18` plus
//! `Mcp-Session-Id`). The two sides are negotiated independently — a modern
//! client against a legacy upstream is a normal combination, not an error.
//!
//! Inbound (gateway → client) is answered here rather than forwarded: the
//! gateway's own capability statement is what the client must hear, and
//! forwarding a legacy upstream's `initialize` would tell a modern client that
//! the gateway supports an older stack.
//!
//! Outbound (gateway → upstream) reuses the negotiated version stored by the
//! probe (`mcp_servers.protocol_version`): modern upstreams get the request
//! as-is, legacy ones get a fresh `initialize` per request. Session reuse is
//! deliberately absent — the `Mcp-Session-Id` belongs to the upstream, and the
//! gateway's replicas rotate (ADR-040), so a cached id would go stale.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};
use tracing::warn;

use crate::mcp::probe::{
    read_jsonrpc_response, ProbeError, LEGACY_PROTOCOL_VERSION, MODERN_PROTOCOL_VERSION,
};

pub const METHOD_NOT_FOUND: i64 = -32601;
pub const HEADER_BODY_MISMATCH: i64 = -32020;
pub const INSUFFICIENT_SCOPE: i64 = -32003;
pub const PARSE_ERROR: i64 = -32700;
pub const INTERNAL_ERROR: i64 = -32603;

pub const METHOD_DISCOVER: &str = "server/discover";
pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_TOOLS_LIST: &str = "tools/list";
pub const METHOD_TOOLS_CALL: &str = "tools/call";

/// Ceiling for one upstream round trip, connect-to-headers. Generous because a
/// tool call may legitimately take a while; a hung upstream still has to end.
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamStack {
    Modern,
    Legacy,
}

/// `None` (never probed) is treated as legacy: `initialize` is the stack every
/// server is required to answer, whereas `server/discover` exists only in the
/// newer one.
pub fn upstream_stack(protocol_version: Option<&str>) -> UpstreamStack {
    match protocol_version {
        Some(v) if v == MODERN_PROTOCOL_VERSION => UpstreamStack::Modern,
        _ => UpstreamStack::Legacy,
    }
}

#[derive(Debug)]
pub enum UpstreamError {
    Timeout,
    Transport(String),
    Negotiation(ProbeError),
}

impl UpstreamError {
    pub fn code(&self) -> &'static str {
        match self {
            UpstreamError::Timeout => "timeout",
            UpstreamError::Transport(_) => "transport_error",
            UpstreamError::Negotiation(_) => "upstream_initialize_failed",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            UpstreamError::Timeout => 504,
            _ => 502,
        }
    }
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpstreamError::Timeout => write!(f, "upstream timed out"),
            UpstreamError::Transport(e) => write!(f, "upstream transport error: {e}"),
            UpstreamError::Negotiation(e) => write!(f, "upstream initialize failed: {e}"),
        }
    }
}

/// How the resolved upstream credential is put on the wire.
///
/// Two variants rather than one `Header` variant: `bearer_auth` is what the MCP
/// spec prescribes and what `reqwest` normalises, while a custom header is an
/// operator's setting that must go out verbatim (TAPD and 工蜂 want the bare
/// token, no `Bearer ` prefix). Keeping them apart also keeps the semantics
/// legible in review — "the protocol's Bearer" is not "a configured header".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamAuth<'a> {
    Bearer(&'a str),
    Header { name: &'a str, value: &'a str },
}

pub struct UpstreamTarget<'a> {
    pub url: &'a str,
    pub auth: Option<UpstreamAuth<'a>>,
    pub extra_headers: &'a [(String, String)],
    pub stack: UpstreamStack,
    pub timeout: Duration,
}

fn client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

pub fn server_info() -> Value {
    json!({"name": "aigw", "version": env!("CARGO_PKG_VERSION")})
}

pub fn session_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// The modern answer: a capability statement, and nothing from `initialize`'s
/// shape (`protocolVersion` as a scalar makes Claude Code fall back to the
/// legacy handshake).
pub fn discover_response(id: Option<&Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "result": {
            "resultType": "complete",
            "supportedVersions": [MODERN_PROTOCOL_VERSION],
            "capabilities": {"tools": {}},
            "_meta": {"serverInfo": server_info()},
        }
    })
}

pub fn initialize_response(id: Option<&Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "result": {
            "protocolVersion": LEGACY_PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": server_info(),
        }
    })
}

pub fn rpc_error(id: Option<&Value>, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "error": {"code": code, "message": message.into()},
    })
}

pub fn empty_tools_response(id: Option<&Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "result": {"tools": []},
    })
}

pub fn request_id(body: &Value) -> Option<Value> {
    body.get("id").cloned()
}

pub fn request_method(body: &Value) -> Option<&str> {
    body.get("method").and_then(Value::as_str)
}

pub fn tool_name_of(body: &Value) -> Option<String> {
    body.get("params")
        .and_then(|p| {
            p.get("name")
                .and_then(Value::as_str)
                .or_else(|| p.get("tool").and_then(Value::as_str))
        })
        .map(str::to_string)
}

/// `params.name` and `params.tool` are two spellings of the same field, and the
/// whole body is forwarded. An ACL gate that only sees the spelling
/// [`tool_name_of`] picked would let an upstream reading the other one run a
/// tool nobody approved, so a disagreement is treated like the `Mcp-Name`
/// mismatch above: malformed, not a permission question. Callers answer it with
/// `HEADER_BODY_MISMATCH` — one envelope-contradiction class, one code — not
/// with `PARSE_ERROR`, because the body itself is perfectly valid JSON.
pub fn check_tool_spellings_consistency(body: &Value) -> Result<(), String> {
    let params = body.get("params");
    let name = params.and_then(|p| p.get("name")).and_then(Value::as_str);
    let tool = params.and_then(|p| p.get("tool")).and_then(Value::as_str);
    match (name, tool) {
        (Some(name), Some(tool)) if name != tool => Err(format!(
            "params name \"{name}\" does not match params tool \"{tool}\""
        )),
        _ => Ok(()),
    }
}

/// The `Mcp-Name` header, when both sides carry a name, must agree. Guessing
/// which one is true is a consistency attack surface and a reliable way to make
/// an upstream's RFC 9421 signature check fail.
pub fn check_header_body_consistency(
    header_tool: Option<&str>,
    body: &Value,
) -> Result<(), String> {
    let Some(header_tool) = header_tool.filter(|h| !h.is_empty()) else {
        return Ok(());
    };
    let Some(body_tool) = tool_name_of(body) else {
        return Ok(());
    };
    if header_tool == body_tool {
        Ok(())
    } else {
        Err(format!(
            "Mcp-Name header \"{header_tool}\" does not match params name \"{body_tool}\""
        ))
    }
}

fn request_builder(
    target: &UpstreamTarget<'_>,
    protocol_version: &str,
    session: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = client()
        .post(target.url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", protocol_version);
    if let Some(auth) = target.auth {
        req = match auth {
            UpstreamAuth::Bearer(token) => req.bearer_auth(token),
            UpstreamAuth::Header { name, value } => req.header(name, value),
        };
    }
    for (name, value) in target.extra_headers {
        req = req.header(name, value);
    }
    if let Some(session) = session {
        req = req.header("Mcp-Session-Id", session);
    }
    req
}

async fn post_jsonrpc(
    target: &UpstreamTarget<'_>,
    body: &Value,
    protocol_version: &str,
    session: Option<&str>,
) -> Result<reqwest::Response, UpstreamError> {
    tokio::time::timeout(
        target.timeout,
        request_builder(target, protocol_version, session)
            .json(body)
            .send(),
    )
    .await
    .map_err(|_| UpstreamError::Timeout)?
    .map_err(|e| UpstreamError::Transport(e.to_string()))
}

async fn read_envelope(
    target: &UpstreamTarget<'_>,
    resp: reqwest::Response,
) -> Result<(Value, Option<String>), UpstreamError> {
    let session = resp
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let envelope = tokio::time::timeout(target.timeout, read_jsonrpc_response(resp))
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(UpstreamError::Negotiation)?;
    Ok((envelope, session))
}

/// Establish the upstream side of the stack. Modern upstreams need nothing;
/// legacy ones get `initialize` plus `notifications/initialized`, and the
/// session id they hand out is returned so the caller can echo it to the client.
pub async fn negotiate(target: &UpstreamTarget<'_>) -> Result<Option<String>, UpstreamError> {
    match target.stack {
        UpstreamStack::Modern => Ok(None),
        UpstreamStack::Legacy => initialize_upstream(target).await.map(Some),
    }
}

async fn initialize_upstream(target: &UpstreamTarget<'_>) -> Result<String, UpstreamError> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": METHOD_INITIALIZE,
        "params": {
            "protocolVersion": LEGACY_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": server_info(),
        },
    });
    let resp = post_jsonrpc(target, &body, LEGACY_PROTOCOL_VERSION, None).await?;
    let (_, session) = read_envelope(target, resp).await?;
    let session = session.unwrap_or_default();

    let notify = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let session_ref = (!session.is_empty()).then_some(session.as_str());
    if let Err(e) = post_jsonrpc(target, &notify, LEGACY_PROTOCOL_VERSION, session_ref).await {
        warn!(error = %e, "notifications/initialized did not reach the upstream");
    }
    Ok(session)
}

/// Send the client's request upstream. Legacy upstreams open a fresh session
/// first; a client session id is *not* forwarded because it names a session the
/// upstream never created.
pub async fn forward(
    target: &UpstreamTarget<'_>,
    body: &Value,
) -> Result<reqwest::Response, UpstreamError> {
    match target.stack {
        UpstreamStack::Modern => post_jsonrpc(target, body, MODERN_PROTOCOL_VERSION, None).await,
        UpstreamStack::Legacy => {
            let session = initialize_upstream(target).await?;
            let session = (!session.is_empty()).then_some(session);
            post_jsonrpc(target, body, LEGACY_PROTOCOL_VERSION, session.as_deref()).await
        }
    }
}

/// One `server/discover` round trip, used only by `/mcp-rest/health`. Any HTTP
/// answer means the upstream is reachable — a 405 is a legacy server saying so.
///
/// Takes the full [`UpstreamTarget`] rather than a bearer: a health check that
/// authenticates differently from the call path reports "unreachable" for every
/// upstream using a custom header, which is exactly the server it matters for.
pub async fn reachable(target: &UpstreamTarget<'_>) -> Option<u16> {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": METHOD_DISCOVER});
    match post_jsonrpc(target, &body, MODERN_PROTOCOL_VERSION, None).await {
        Ok(resp) => Some(resp.status().as_u16()),
        Err(e) => {
            warn!(error = %e, url = target.url, "MCP health probe failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_stack_treats_unprobed_servers_as_legacy() {
        assert_eq!(
            upstream_stack(Some(MODERN_PROTOCOL_VERSION)),
            UpstreamStack::Modern
        );
        assert_eq!(
            upstream_stack(Some(LEGACY_PROTOCOL_VERSION)),
            UpstreamStack::Legacy
        );
        assert_eq!(upstream_stack(None), UpstreamStack::Legacy);
    }

    #[test]
    fn discover_response_has_no_initialize_shape() {
        let value = discover_response(Some(&json!(7)));
        assert_eq!(value["id"], json!(7));
        assert_eq!(
            value["result"]["supportedVersions"],
            json!([MODERN_PROTOCOL_VERSION])
        );
        assert_eq!(value["result"]["resultType"], json!("complete"));
        assert!(value["result"].get("protocolVersion").is_none());
        assert!(value["result"].get("serverInfo").is_none());
    }

    #[test]
    fn initialize_response_carries_a_scalar_protocol_version() {
        let value = initialize_response(Some(&json!("abc")));
        assert_eq!(value["id"], json!("abc"));
        assert_eq!(
            value["result"]["protocolVersion"],
            json!(LEGACY_PROTOCOL_VERSION)
        );
        assert!(value["result"].get("supportedVersions").is_none());
    }

    #[test]
    fn rpc_error_echoes_the_request_id() {
        let value = rpc_error(Some(&json!(3)), METHOD_NOT_FOUND, "nope");
        assert_eq!(value["id"], json!(3));
        assert_eq!(value["error"]["code"], json!(METHOD_NOT_FOUND));
        assert_eq!(rpc_error(None, PARSE_ERROR, "x")["id"], Value::Null);
    }

    #[test]
    fn header_and_body_names_must_agree() {
        let body = json!({"method": "tools/call", "params": {"name": "search"}});
        assert!(check_header_body_consistency(Some("search"), &body).is_ok());
        assert!(check_header_body_consistency(None, &body).is_ok());
        assert!(check_header_body_consistency(Some(""), &body).is_ok());
        let err = check_header_body_consistency(Some("read"), &body).unwrap_err();
        assert!(err.contains("read") && err.contains("search"), "{err}");
    }

    #[test]
    fn header_check_is_silent_when_the_body_names_nothing() {
        let body = json!({"method": "tools/list"});
        assert!(check_header_body_consistency(Some("search"), &body).is_ok());
    }

    #[test]
    fn tool_name_accepts_both_params_spellings() {
        assert_eq!(
            tool_name_of(&json!({"params": {"name": "a"}})).as_deref(),
            Some("a")
        );
        assert_eq!(
            tool_name_of(&json!({"params": {"tool": "b"}})).as_deref(),
            Some("b")
        );
        assert_eq!(tool_name_of(&json!({})), None);
    }

    #[test]
    fn the_two_params_spellings_must_not_disagree() {
        assert!(check_tool_spellings_consistency(
            &json!({"params": {"name": "same", "tool": "same"}})
        )
        .is_ok());
        assert!(check_tool_spellings_consistency(&json!({"params": {"name": "a"}})).is_ok());
        assert!(check_tool_spellings_consistency(&json!({"params": {"tool": "b"}})).is_ok());
        assert!(check_tool_spellings_consistency(&json!({"method": "tools/list"})).is_ok());

        let err = check_tool_spellings_consistency(&json!({"params": {"name": "a", "tool": "b"}}))
            .unwrap_err();
        assert!(err.contains("a") && err.contains("b"), "{err}");
    }

    fn request_headers(
        auth: Option<UpstreamAuth<'_>>,
        extra: &[(String, String)],
    ) -> reqwest::header::HeaderMap {
        let target = UpstreamTarget {
            url: "http://127.0.0.1:1/mcp",
            auth,
            extra_headers: extra,
            stack: UpstreamStack::Modern,
            timeout: Duration::from_secs(1),
        };
        request_builder(&target, MODERN_PROTOCOL_VERSION, Some("sess-1"))
            .build()
            .expect("build")
            .headers()
            .clone()
    }

    #[test]
    fn bearer_auth_is_written_with_the_protocol_prefix() {
        let headers = request_headers(Some(UpstreamAuth::Bearer("tk")), &[]);
        assert_eq!(headers.get("authorization").unwrap(), "Bearer tk");
    }

    #[test]
    fn a_custom_auth_header_carries_the_bare_token_and_no_authorization() {
        let headers = request_headers(
            Some(UpstreamAuth::Header {
                name: "X-Tapd-Access-Token",
                value: "tk",
            }),
            &[],
        );
        assert_eq!(headers.get("x-tapd-access-token").unwrap(), "tk");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn extra_headers_are_attached_alongside_the_auth_header() {
        let extra = vec![
            ("X-Project-Id".to_string(), "p-1".to_string()),
            ("X-Tenant".to_string(), "t-1".to_string()),
        ];
        let headers = request_headers(Some(UpstreamAuth::Bearer("tk")), &extra);
        assert_eq!(headers.get("x-project-id").unwrap(), "p-1");
        assert_eq!(headers.get("x-tenant").unwrap(), "t-1");
        assert_eq!(headers.get("authorization").unwrap(), "Bearer tk");
    }

    #[test]
    fn no_credential_means_no_auth_header_but_extras_still_go_out() {
        let extra = vec![("X-Project-Id".to_string(), "p-1".to_string())];
        let headers = request_headers(None, &extra);
        assert!(headers.get("authorization").is_none());
        assert_eq!(headers.get("x-project-id").unwrap(), "p-1");
    }

    #[test]
    fn the_gateway_still_owns_the_protocol_headers() {
        let headers = request_headers(None, &[]);
        assert_eq!(
            headers.get("mcp-protocol-version").unwrap(),
            MODERN_PROTOCOL_VERSION
        );
        assert_eq!(headers.get("mcp-session-id").unwrap(), "sess-1");
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
    }
}
