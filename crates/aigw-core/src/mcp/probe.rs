//! Upstream negotiation and `tools/list` discovery (Phase 55, Stage 141).
//!
//! Two protocol stacks exist in the wild and the probe must speak both: Claude
//! Code opens with `server/discover` (`MCP-Protocol-Version: 2026-07-28`) and
//! never sends `initialize`, while Codex sends only `initialize`
//! (`2025-06-18`). Trying one and giving up on failure would make half the
//! upstreams unreachable with no error that says why.
//!
//! 1. `server/discover`. A `result.supportedVersions` array means the modern
//!    stack; the newest entry becomes the negotiated version and `result.ttlMs`
//!    the cache TTL.
//! 2. Anything else (405, 4xx, no `supportedVersions`) falls back to
//!    `initialize` → `Mcp-Session-Id` → `notifications/initialized`.
//! 3. `tools/list` follows `nextCursor` to the end, capped at
//!    [`MAX_TOOL_PAGES`] so an upstream that echoes its own cursor cannot spin
//!    forever.
//!
//! IO and pure logic are separate on purpose: [`read_jsonrpc_response`], the
//! envelope readers and [`apply_conflicts`] are total functions over a response
//! or a tool list, and Stage 143 reuses [`read_jsonrpc_response`] for its own
//! outbound negotiation. Anything parsed here is parsed once.

use std::collections::HashSet;
use std::time::Duration;

use serde_json::{json, Value};
use tracing::warn;

use crate::models::{ProbeOutcome, UpstreamTool};

pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LEGACY_PROTOCOL_VERSION: &str = "2025-06-18";
pub const MAX_TOOL_PAGES: usize = 50;
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
pub const PROBE_BODY_LIMIT: usize = 500;
/// `mcp_tool_catalog.tool_name` is `VARCHAR(255)` on MySQL. A longer name would
/// fail the INSERT and turn a discoverable upstream into a generic `db_error`,
/// so it is rejected here — as a probe failure that says what is wrong.
pub const MAX_TOOL_NAME_LEN: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// The request never produced a response (DNS, refused, timeout).
    Transport(String),
    /// Non-2xx. Carries the status and the truncated body so the operator sees
    /// what the upstream actually answered instead of "probe failed".
    Http { status: u16, body: String },
    /// 2xx with a content type we cannot read — HTML error pages land here,
    /// which is the whole point of checking the type rather than the status.
    UnsupportedContentType(String),
    /// 2xx with a body that is not a usable JSON-RPC envelope.
    Malformed(String),
    /// A JSON-RPC `error` member.
    Rpc {
        code: i64,
        message: String,
        body: String,
    },
}

impl ProbeError {
    pub fn upstream_status(&self) -> Option<u16> {
        match self {
            ProbeError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn body(&self) -> Option<&str> {
        match self {
            ProbeError::Http { body, .. } | ProbeError::Rpc { body, .. } => Some(body),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Transport(e) => write!(f, "transport error: {e}"),
            ProbeError::Http { status, body } => {
                write!(f, "upstream returned HTTP {status}: {body}")
            }
            ProbeError::UnsupportedContentType(ct) => write!(
                f,
                "upstream answered with content type \"{ct}\" — expected application/json or \
                 text/event-stream"
            ),
            ProbeError::Malformed(e) => write!(f, "malformed JSON-RPC response: {e}"),
            ProbeError::Rpc { code, message, .. } => {
                write!(f, "upstream JSON-RPC error {code}: {message}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// Truncate a body for the error payload. Counts characters, not bytes, so a
/// multi-byte character at the boundary cannot be split.
pub fn truncate_body(body: &str) -> String {
    let mut out = String::new();
    for (i, ch) in body.chars().enumerate() {
        if i >= PROBE_BODY_LIMIT {
            break;
        }
        out.push(ch);
    }
    out
}

/// Pull the JSON-RPC envelope out of a response, dispatching on its content
/// type.
///
/// Streamable HTTP lets an upstream answer a plain POST with
/// `text/event-stream`, in which case the result arrives as one `data:` event.
/// Everything else — HTML in particular, which SearXNG taught us to expect from
/// a `200` — is a failure: a status code alone does not mean the answer is
/// usable.
pub async fn read_jsonrpc_response(resp: reqwest::Response) -> Result<Value, ProbeError> {
    read_jsonrpc_response_for(resp, None).await
}

/// [`read_jsonrpc_response`] for a caller that knows which request it sent.
///
/// An SSE stream may interleave events that belong to other requests, so an
/// envelope is only the answer when its `id` matches. `None` keeps the lenient
/// behaviour for callers that cannot know the id (Stage 143's relay forwards the
/// client's envelope verbatim).
pub async fn read_jsonrpc_response_for(
    resp: reqwest::Response,
    expected_id: Option<&Value>,
) -> Result<Value, ProbeError> {
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let text = resp
        .text()
        .await
        .map_err(|e| ProbeError::Transport(e.to_string()))?;
    parse_jsonrpc_body_for(status, &content_type, &text, expected_id)
}

/// The pure half of [`read_jsonrpc_response`]: status + content type + body in,
/// envelope out. Split out so every branch is testable without a socket.
pub fn parse_jsonrpc_body(
    status: u16,
    content_type: &str,
    text: &str,
) -> Result<Value, ProbeError> {
    parse_jsonrpc_body_for(status, content_type, text, None)
}

pub fn parse_jsonrpc_body_for(
    status: u16,
    content_type: &str,
    text: &str,
    expected_id: Option<&Value>,
) -> Result<Value, ProbeError> {
    let content_type = content_type.to_ascii_lowercase();
    let is_json = content_type.contains("application/json");
    let is_sse = content_type.contains("text/event-stream");

    if !is_json && !is_sse {
        return Err(ProbeError::UnsupportedContentType(
            if content_type.is_empty() {
                "missing".to_string()
            } else {
                content_type
            },
        ));
    }

    if status >= 400 {
        return Err(ProbeError::Http {
            status,
            body: truncate_body(text),
        });
    }

    let envelope = if is_json {
        serde_json::from_str::<Value>(text.trim())
            .map_err(|e| ProbeError::Malformed(format!("{e}: {}", truncate_body(text))))?
    } else {
        first_result_event(text, expected_id).ok_or_else(|| {
            ProbeError::Malformed("no matching JSON-RPC result event in SSE stream".into())
        })?
    };

    if let Some(err) = envelope.get("error") {
        let code = err.get("code").and_then(Value::as_i64).unwrap_or(0);
        let message = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("upstream error")
            .to_string();
        return Err(ProbeError::Rpc {
            code,
            message,
            body: truncate_body(&envelope.to_string()),
        });
    }
    if !envelope.is_object() {
        return Err(ProbeError::Malformed(
            "JSON-RPC envelope is not an object".into(),
        ));
    }
    Ok(envelope)
}

/// The first `data:` payload in an SSE body that is a JSON-RPC envelope carrying
/// `result` or `error`. Notifications and comments have neither, so they are
/// skipped; the stream may legitimately start with a heartbeat.
///
/// With `expected_id` set, only the envelope answering *that* request counts.
/// A stream can carry unrelated events (progress notifications, another
/// in-flight call), and taking the first one that merely has a `result` member
/// would report a foreign payload as this request's answer.
fn first_result_event(body: &str, expected_id: Option<&Value>) -> Option<Value> {
    let mut data: Vec<&str> = Vec::new();
    let mut events: Vec<String> = Vec::new();
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data.is_empty() {
                events.push(data.join("\n"));
                data.clear();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if !data.is_empty() {
        events.push(data.join("\n"));
    }

    events.into_iter().find_map(|payload| {
        let value: Value = serde_json::from_str(&payload).ok()?;
        if !value.is_object() || (value.get("result").is_none() && value.get("error").is_none()) {
            return None;
        }
        match expected_id {
            Some(expected) if value.get("id") != Some(expected) => None,
            _ => Some(value),
        }
    })
}

/// The newest entry of `supportedVersions`, by dotted numeric comparison.
///
/// Lexicographic order would put `2025-11-25` after `2026-07-28`; the segments
/// are numbers, so compare them as numbers.
pub fn newest_version(versions: &[String]) -> Option<String> {
    versions.iter().max_by_key(|v| version_key(v)).cloned()
}

fn version_key(version: &str) -> Vec<u32> {
    let mut key: Vec<u32> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<u32>().unwrap_or(0))
        .collect();
    // A trailing non-numeric segment is a pre-release of the version before it
    // (`2026-07-28-alpha` < `2026-07-28`). Without this marker the two produce
    // the same key and `max_by_key` picks whichever came first.
    let is_release = version.ends_with(|c: char| c.is_ascii_digit());
    key.push(if is_release { u32::MAX } else { 0 });
    key
}

/// The name an upstream entry declares, or `None` when it has none. One
/// definition of "nameless", shared by [`parse_upstream_tool`] (which rejects
/// one) and [`parse_tools`] (which skips one) so the two cannot disagree.
fn declared_tool_name(value: &Value) -> Option<&str> {
    value
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
}

/// Parse one entry of an upstream `tools/list` result.
///
/// The upstream spells these `name` / `inputSchema` (the MCP wire names); the
/// gateway stores and serves `tool_name` / `input_schema`. The rename happens
/// here, once, rather than being carried as an alias through every layer.
///
/// A nameless entry is an error here because there is nothing to store; the
/// list-level caller is the one that decides to skip it instead.
pub fn parse_upstream_tool(value: &Value) -> Result<UpstreamTool, ProbeError> {
    let tool_name = declared_tool_name(value)
        .ok_or_else(|| ProbeError::Malformed("a tools/list entry carries no name".into()))?
        .to_string();
    if tool_name.chars().count() > MAX_TOOL_NAME_LEN {
        return Err(ProbeError::Malformed(format!(
            "tool name \"{}\" is longer than {MAX_TOOL_NAME_LEN} characters",
            truncate_body(&tool_name)
        )));
    }
    Ok(UpstreamTool {
        tool_name,
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        input_schema: value
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({})),
        annotations: value.get("annotations").cloned(),
        has_conflict: false,
    })
}

/// Parse a `tools/list` result. A nameless entry is skipped rather than failing
/// the page: an upstream that emits one alongside real tools is still usable,
/// and there is no name to store it under. Over-long names are not skipped —
/// those the probe reports, so the operator learns why the catalogue is
/// incomplete.
pub fn parse_tools(result: &Value) -> Result<Vec<UpstreamTool>, ProbeError> {
    match result.get("tools").and_then(Value::as_array) {
        Some(tools) => tools
            .iter()
            .filter(|tool| declared_tool_name(tool).is_some())
            .map(parse_upstream_tool)
            .collect(),
        None => Ok(Vec::new()),
    }
}

pub fn next_cursor(result: &Value) -> Option<String> {
    result
        .get("nextCursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
}

/// Flag every tool whose name also exists under another server, and return the
/// conflicting names (sorted, deduplicated).
///
/// Two servers offering a tool of the same name is normal; silently letting one
/// win would make `{alias}__{tool}` point at the wrong implementation. The flag
/// is what the UI colours red, so it has to be computed against the *other*
/// servers' snapshots, not against this one's.
pub fn apply_conflicts(
    tools: &mut [UpstreamTool],
    other_server_tool_names: &[String],
) -> Vec<String> {
    let others: HashSet<&str> = other_server_tool_names.iter().map(String::as_str).collect();
    let mut conflicts: Vec<String> = Vec::new();
    for tool in tools.iter_mut() {
        tool.has_conflict = others.contains(tool.tool_name.as_str());
        if tool.has_conflict {
            conflicts.push(tool.tool_name.clone());
        }
    }
    conflicts.sort();
    conflicts.dedup();
    conflicts
}

struct Session {
    client: reqwest::Client,
    url: String,
    bearer: Option<String>,
    protocol_version: String,
    session_id: Option<String>,
    next_id: u64,
}

impl Session {
    fn build_request(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", &self.protocol_version)
            .json(body);
        if let Some(bearer) = self.bearer.as_deref() {
            req = req.bearer_auth(bearer);
        }
        if let Some(session) = self.session_id.as_deref() {
            req = req.header("Mcp-Session-Id", session);
        }
        req
    }

    /// Send one request and return the envelope plus the response's session id.
    async fn call(&mut self, method: &str, params: Option<Value>) -> Result<Value, ProbeError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut body = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if let Some(params) = params {
            body["params"] = params;
        }
        let resp = self
            .build_request(&body)
            .send()
            .await
            .map_err(|e| ProbeError::Transport(e.to_string()))?;
        if let Some(session) = resp
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
        {
            self.session_id = Some(session.to_string());
        }
        read_jsonrpc_response_for(resp, Some(&json!(id))).await
    }

    async fn notify(&mut self, method: &str) -> Result<(), ProbeError> {
        let body = json!({"jsonrpc": "2.0", "method": method});
        let resp = self
            .build_request(&body)
            .send()
            .await
            .map_err(|e| ProbeError::Transport(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProbeError::Http {
                status,
                body: truncate_body(&text),
            });
        }
        Ok(())
    }
}

/// Probe one upstream: negotiate, list every tool, and flag name collisions.
///
/// `other_server_tool_names` is every tool name stored under the *other*
/// servers, so `has_conflict` describes the gateway-wide truth rather than this
/// server's snapshot.
pub async fn probe_server(
    url: &str,
    bearer: Option<&str>,
    other_server_tool_names: &[String],
) -> Result<ProbeOutcome, ProbeError> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| ProbeError::Transport(e.to_string()))?;

    let mut session = Session {
        client,
        url: url.to_string(),
        bearer: bearer.map(str::to_string),
        protocol_version: MODERN_PROTOCOL_VERSION.to_string(),
        session_id: None,
        next_id: 1,
    };

    let (protocol_version, server_info, ttl_ms) = match discover(&mut session).await {
        Some(modern) => modern,
        None => {
            session.protocol_version = LEGACY_PROTOCOL_VERSION.to_string();
            let legacy = initialize(&mut session).await?;
            session.notify("notifications/initialized").await?;
            legacy
        }
    };

    let mut tools = list_all_tools(&mut session).await?;
    let conflicts = apply_conflicts(&mut tools, other_server_tool_names);

    Ok(ProbeOutcome {
        protocol_version,
        server_info,
        ttl_ms,
        tools,
        conflicts,
    })
}

type Negotiated = (String, Option<Value>, Option<i64>);

/// Modern negotiation. `None` means "fall back to `initialize`" — which covers
/// 405, any other 4xx, a transport failure, and a 200 whose result carries no
/// `supportedVersions`.
async fn discover(session: &mut Session) -> Option<Negotiated> {
    let envelope = session.call("server/discover", None).await.ok()?;
    let result = envelope.get("result")?;
    let versions: Vec<String> = result
        .get("supportedVersions")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let newest = newest_version(&versions)?;
    session.protocol_version = newest.clone();
    Some((
        newest,
        result.get("serverInfo").cloned(),
        result.get("ttlMs").and_then(Value::as_i64),
    ))
}

async fn initialize(session: &mut Session) -> Result<Negotiated, ProbeError> {
    let envelope = session
        .call(
            "initialize",
            Some(json!({
                "protocolVersion": LEGACY_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "aigw", "version": env!("CARGO_PKG_VERSION")},
            })),
        )
        .await?;
    let result = envelope.get("result").unwrap_or(&Value::Null);
    let version = result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(LEGACY_PROTOCOL_VERSION)
        .to_string();
    session.protocol_version = version.clone();
    Ok((
        version,
        result.get("serverInfo").cloned(),
        result.get("ttlMs").and_then(Value::as_i64),
    ))
}

async fn list_all_tools(session: &mut Session) -> Result<Vec<UpstreamTool>, ProbeError> {
    let mut tools: Vec<UpstreamTool> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut dropped = 0usize;
    let mut cursor: Option<String> = None;
    for page in 0..MAX_TOOL_PAGES {
        let params = cursor.take().map(|cursor| json!({"cursor": cursor}));
        let envelope = session.call("tools/list", params).await?;
        let result = envelope.get("result").cloned().unwrap_or(Value::Null);
        for tool in parse_tools(&result)? {
            if seen.insert(tool.tool_name.clone()) {
                tools.push(tool);
            } else {
                dropped += 1;
            }
        }
        match next_cursor(&result) {
            Some(next) => cursor = Some(next),
            None => break,
        }
        if page + 1 == MAX_TOOL_PAGES {
            warn!(
                pages = MAX_TOOL_PAGES,
                tools = tools.len(),
                "tools/list kept returning a cursor; stopped at the page cap"
            );
        }
    }
    if dropped > 0 {
        warn!(
            dropped,
            kept = tools.len(),
            "upstream repeated tool names across pages or within a page; kept the first of each"
        );
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_version_compares_numerically_not_lexicographically() {
        let versions = vec![
            "2025-06-18".to_string(),
            "2025-11-25".to_string(),
            "2026-07-28".to_string(),
            "2026-10-01".to_string(),
        ];
        assert_eq!(newest_version(&versions).as_deref(), Some("2026-10-01"));
        assert_eq!(newest_version(&[]), None);
    }

    #[test]
    fn newest_version_prefers_a_release_over_its_own_prerelease() {
        let versions = vec!["2026-07-28-alpha".to_string(), "2026-07-28".to_string()];
        assert_eq!(newest_version(&versions).as_deref(), Some("2026-07-28"));
    }

    #[test]
    fn apply_conflicts_flags_only_shared_names() {
        let mut tools = vec![
            UpstreamTool {
                tool_name: "search".into(),
                description: String::new(),
                input_schema: json!({}),
                annotations: None,
                has_conflict: false,
            },
            UpstreamTool {
                tool_name: "read".into(),
                description: String::new(),
                input_schema: json!({}),
                annotations: None,
                has_conflict: false,
            },
        ];
        let conflicts = apply_conflicts(&mut tools, &["search".to_string()]);
        assert_eq!(conflicts, vec!["search".to_string()]);
        assert!(tools[0].has_conflict);
        assert!(!tools[1].has_conflict);
    }

    #[test]
    fn parse_upstream_tool_maps_wire_names_to_column_names() {
        let tool = parse_upstream_tool(&json!({
            "name": "search",
            "description": "find things",
            "inputSchema": {"type": "object"},
            "annotations": {"readOnlyHint": true}
        }))
        .expect("tool");
        assert_eq!(tool.tool_name, "search");
        assert_eq!(tool.description, "find things");
        assert_eq!(tool.input_schema, json!({"type": "object"}));
        assert_eq!(tool.annotations, Some(json!({"readOnlyHint": true})));
        assert!(!tool.has_conflict);
    }

    #[test]
    fn parse_tools_skips_entries_without_a_name() {
        let tools = parse_tools(&json!({"tools": [{"name": "a"}, {"description": "no name"}]}))
            .expect("tools");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_name, "a");
        assert_eq!(tools[0].input_schema, json!({}));
        assert_eq!(tools[0].annotations, None);
    }

    #[test]
    fn parse_tools_rejects_a_name_that_would_not_fit_the_column() {
        let name = "t".repeat(MAX_TOOL_NAME_LEN + 1);
        let err = parse_tools(&json!({"tools": [{"name": name}]})).unwrap_err();
        assert!(
            matches!(err, ProbeError::Malformed(ref m) if m.contains("longer than")),
            "got {err:?}"
        );
    }

    #[test]
    fn first_result_event_skips_heartbeats_and_notifications() {
        let sse = ": ping\n\n\
                   data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n";
        let envelope = first_result_event(sse, None).expect("envelope");
        assert_eq!(envelope["id"], 1);
    }

    #[test]
    fn first_result_event_ignores_an_envelope_answering_another_request() {
        let sse = "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{\"tools\":[{\"name\":\"other\"}]}}\n\n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"tools\":[]}}\n\n";
        let envelope = first_result_event(sse, Some(&json!(7))).expect("envelope");
        assert_eq!(envelope["id"], 7);
    }

    #[test]
    fn first_result_event_ignores_a_foreign_error_and_keeps_looking() {
        let sse = "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"error\":{\"code\":-1,\"message\":\"not yours\"}}\n\n\
                   data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"tools\":[]}}\n\n";
        let envelope = first_result_event(sse, Some(&json!(7))).expect("envelope");
        assert_eq!(envelope["id"], 7);
    }

    #[test]
    fn first_result_event_supports_multiline_data() {
        let sse = "event: message\ndata: {\"jsonrpc\":\"2.0\",\ndata: \"id\":7,\"result\":{}}\n\n";
        let envelope = first_result_event(sse, Some(&json!(7))).expect("envelope");
        assert_eq!(envelope["id"], 7);
    }

    #[test]
    fn first_result_event_absent_when_stream_has_only_notifications() {
        assert!(first_result_event(
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/x\"}\n\n",
            None
        )
        .is_none());
        assert!(first_result_event("", None).is_none());
    }

    #[test]
    fn truncate_body_counts_characters() {
        let long = "日".repeat(PROBE_BODY_LIMIT + 10);
        let cut = truncate_body(&long);
        assert_eq!(cut.chars().count(), PROBE_BODY_LIMIT);
        assert_eq!(truncate_body("short"), "short");
    }
}
