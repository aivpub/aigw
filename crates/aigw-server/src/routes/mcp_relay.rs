//! Response relay for `/mcp/{alias}` (Stage 143, ADR-042).
//!
//! Lives in `aigw-server` rather than `aigw-core` because its product is an
//! `axum::body::Body`, and the SSE path needs `tokio-stream` — a dependency only
//! this crate carries.
//!
//! Two shapes are relayed: a buffered body passed through byte for byte, and an
//! event stream forwarded as it arrives. The stream carries a heartbeat comment
//! every [`HEARTBEAT_INTERVAL`]: `2026-07-28` dropped `Last-Event-ID` resumption,
//! so a stall is not recoverable and the only defence against an intermediary's
//! idle timeout is traffic that never stops.

use std::time::Duration;

use aigw_core::db::Database;
use aigw_core::mcp::body::cut;
use aigw_core::mcp::session::{self, METHOD_TOOLS_LIST};
use aigw_core::models::{McpCallBodyRow, McpCallLogRow, MCP_CALL_STATUS_FAILED};
use aigw_core::retention::McpLogRetentionConfig;
use axum::body::Body;
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tracing::warn;

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const HEARTBEAT_FRAME: &[u8] = b":\r\n";

/// Append the call's audit row. A write failure is logged and dropped: an
/// unreachable audit table must not turn a relayed response into an error.
pub(crate) async fn record(db: &Database, row: &McpCallLogRow) {
    if let Err(e) = db.insert_mcp_call_log(row).await {
        warn!(alias = %row.server_alias, error = %e, "could not write mcp_call_log");
    }
}

/// The audit row and the bodies behind it (Stage 152). Both inserts are
/// awaited rather than spawned: the row is useless to whoever opens the detail
/// drawer a second later if the body is still in flight, and one INSERT costs
/// nothing next to the upstream round trip it follows.
///
/// `limit` is `None` when `record_bodies` is off — the one value that means
/// "write nothing", carried instead of the whole config so the SSE task can
/// own it.
///
/// A `tools/list` answer keeps no body: it is the server's whole catalogue —
/// measured at 126 KB per aggregate call against 1 KB for a `tools/call` — and
/// the same data already lives in `mcp_tool_catalog`, timestamped by
/// `fetched_at`. Storing a second copy per listing spent 99% of this table on
/// the one method whose payload the database already owns. The log row stays,
/// so the call is still counted and timed; only the byte copy is dropped.
pub(crate) async fn record_with_bodies(
    db: &Database,
    limit: Option<usize>,
    row: &McpCallLogRow,
    request: &[u8],
    response: &[u8],
) {
    record(db, row).await;
    if row.method == METHOD_TOOLS_LIST {
        return;
    }
    let Some(limit) = limit else {
        return;
    };
    let (request_body, request_cut) = cut(request, limit);
    let (response_body, response_cut) = cut(response, limit);
    let body_row = McpCallBodyRow {
        call_id: row.call_id.clone(),
        request_body: Some(request_body),
        response_body: Some(response_body),
        truncated: request_cut || response_cut,
        created_at: row.created_at.clone(),
    };
    if let Err(e) = db.insert_mcp_call_body(&body_row).await {
        warn!(call_id = %row.call_id, error = %e, "could not write mcp_call_body");
    }
}

/// The per-body ceiling in bytes, or `None` when body recording is switched off.
pub(crate) fn body_limit(cfg: &McpLogRetentionConfig) -> Option<usize> {
    cfg.record_bodies
        .then(|| (cfg.max_body_kib as usize).saturating_mul(1024))
}

/// Name the audit row on the response so a client can reconcile the call it
/// just made with the `mcp_call_log` row it wrote. `main.rs` only propagates a
/// request-side `x-call-id`, which no client sends.
pub(crate) fn stamp_call_id(mut response: Response, call_id: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(call_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-call-id"), value);
    }
    response
}

/// Turn an upstream response into the client-facing one, completing `row` with
/// what actually went out and writing it once the body is on its way.
///
/// `request` is the JSON-RPC envelope the client sent and `limit` the body
/// ceiling (`None` when recording is off). Both are carried rather than looked
/// up so this module never has to know where the config lives.
///
/// `complete` is the method name when the client negotiated `2026-07-28`: that
/// revision dropped the "absent `resultType` means complete" bridge, so an
/// upstream that answers in the older shape would otherwise be rejected by the
/// client even though the bytes came from a perfectly valid legacy server. The
/// gateway is the respondent on this hop, so it finishes the envelope the
/// client's revision requires. `None` leaves the bytes untouched.
pub(crate) async fn relay(
    upstream: reqwest::Response,
    db: Database,
    row: McpCallLogRow,
    request: Vec<u8>,
    limit: Option<usize>,
    complete: Option<&str>,
) -> Response {
    let call_id = row.call_id.clone();
    let response = if is_event_stream(&upstream) {
        relay_event_stream(upstream, db, row, request, limit, complete)
    } else {
        relay_buffered(upstream, db, row, &request, limit, complete).await
    };
    stamp_call_id(response, &call_id)
}

fn is_event_stream(resp: &reqwest::Response) -> bool {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .contains("text/event-stream")
}

fn upstream_status(resp: &reqwest::Response) -> StatusCode {
    StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY)
}

fn upstream_content_type(resp: &reqwest::Response) -> Option<HeaderValue> {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok())
}

async fn relay_buffered(
    upstream: reqwest::Response,
    db: Database,
    mut row: McpCallLogRow,
    request: &[u8],
    limit: Option<usize>,
    complete: Option<&str>,
) -> Response {
    let status = upstream_status(&upstream);
    let content_type = upstream_content_type(&upstream);
    let raw = match upstream.bytes().await {
        Ok(bytes) => bytes.to_vec(),
        Err(e) => {
            warn!(alias = %row.server_alias, error = %e, "upstream body read failed");
            row.status = MCP_CALL_STATUS_FAILED.to_string();
            row.error_code = Some("transport_error".to_string());
            Vec::new()
        }
    };
    let body = match complete {
        Some(method) => completed_json(method, &raw).unwrap_or(raw),
        None => raw,
    };
    row.response_bytes = body.len() as i64;
    if !status.is_success() {
        row.status = MCP_CALL_STATUS_FAILED.to_string();
        row.error_code
            .get_or_insert_with(|| "upstream_error".to_string());
    } else if let Some(err) = jsonrpc_error(&body) {
        row.status = MCP_CALL_STATUS_FAILED.to_string();
        row.error_code.get_or_insert_with(|| {
            err.get("code")
                .and_then(Value::as_i64)
                .map(|code| code.to_string())
                .unwrap_or_else(|| "upstream_error".to_string())
        });
    }
    record_with_bodies(&db, limit, &row, request, &body).await;
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Some(ct) = content_type {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    response
}

/// The relayed answer with the modern result envelope finished, or `None` when
/// nothing had to change — bytes that are not a JSON-RPC envelope, an error
/// envelope, and an upstream that already speaks the modern shape all travel
/// exactly as they arrived.
fn completed_json(method: &str, body: &[u8]) -> Option<Vec<u8>> {
    let mut envelope: Value = serde_json::from_slice(body).ok()?;
    session::complete_envelope(method, RELAY_TTL_MS, &mut envelope)
        .then(|| serde_json::to_vec(&envelope).ok())
        .flatten()
}

/// A relayed listing is a pass-through, so the gateway cannot promise the
/// client a cache hit: the upstream may answer differently on the next call.
const RELAY_TTL_MS: i64 = 0;

/// An upstream may report a JSON-RPC failure inside an HTTP 200. The body is
/// still the client's answer, but the row must not read `success` with no code:
/// the locally-answered paths already call that a failure
/// (`Outcome::is_failure`). Only a buffered body can be read as one envelope —
/// the SSE path has no such luxury and keeps its own rule.
fn jsonrpc_error(body: &[u8]) -> Option<Value> {
    serde_json::from_slice::<Value>(body)
        .ok()?
        .get("error")
        .cloned()
}

/// The recorded answer of a stream is what was forwarded, joined — capped, so a
/// long-lived notification stream cannot turn one audit row into a megabyte.
/// `truncated` then says the tail is missing; `error_code` and the status still
/// describe the whole call because neither is read from the body.
fn relay_event_stream(
    upstream: reqwest::Response,
    db: Database,
    mut row: McpCallLogRow,
    request: Vec<u8>,
    limit: Option<usize>,
    complete: Option<&str>,
) -> Response {
    let status = upstream_status(&upstream);
    let (tx, rx) = mpsc::unbounded_channel::<Result<Vec<u8>, std::io::Error>>();
    // An SSE answer is one `data:` payload, so finishing the envelope means
    // rewriting the event rather than the stream: the bytes are held until the
    // first data line ends, and everything else — comments, other event types,
    // later events — still goes out as it arrives. Built here because the
    // rewriter owns the method name and so can outlive this borrow.
    let mut rewriter = complete.map(EventRewriter::new);
    tokio::spawn(async move {
        let mut chunks = std::pin::pin!(upstream.bytes_stream());
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;
        let mut forwarded: i64 = 0;
        let mut answer = Vec::new();
        loop {
            tokio::select! {
                chunk = chunks.next() => match chunk {
                    Some(Ok(bytes)) => {
                        let out = match rewriter.as_mut() {
                            Some(rewriter) => rewriter.feed(&bytes),
                            None => bytes.to_vec(),
                        };
                        if out.is_empty() {
                            continue;
                        }
                        forwarded += out.len() as i64;
                        keep_prefix(&mut answer, &out, limit);
                        if tx.send(Ok(out)).is_err() {
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        warn!(alias = %row.server_alias, error = %e, "upstream SSE stream broke");
                        row.status = MCP_CALL_STATUS_FAILED.to_string();
                        row.error_code = Some("stream_error".to_string());
                        break;
                    }
                    None => break,
                },
                _ = heartbeat.tick() => {
                    if tx.send(Ok(HEARTBEAT_FRAME.to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
        if let Some(held) = rewriter.as_mut().map(EventRewriter::flush) {
            forwarded += held.len() as i64;
            keep_prefix(&mut answer, &held, limit);
            let _ = tx.send(Ok(held));
        }
        row.response_bytes = forwarded;
        record_with_bodies(&db, limit, &row, &request, &answer).await;
    });
    sse_response(rx, status)
}

/// Incremental SSE rewriter for the modern result envelope.
///
/// An upstream answers a `tools/list` in one `data:` event, and the client
/// rejects that event if `2026-07-28`'s `resultType` fields are missing. The
/// payload is only complete at the end of the event, so this holds the stream
/// until it can read one that carries the answer. Progress notifications and
/// keepalive comments may come first; they are forwarded as they are, and the
/// answer event ends the rewriting — everything after it is the upstream's own.
///
/// Escapes are tracked so a `\n` inside a JSON string never counts as the end of
/// a `data:` line.
#[derive(Default)]
struct EventRewriter {
    method: String,
    buffer: Vec<u8>,
    finished: bool,
}

impl EventRewriter {
    fn new(method: &str) -> Self {
        Self {
            method: method.to_string(),
            ..Self::default()
        }
    }

    fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        if self.finished {
            return chunk.to_vec();
        }
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(end) = self.complete_event_len() {
            let event: Vec<u8> = self.buffer.drain(..end).collect();
            match completed_event(&self.method, &event) {
                Some(answer) => {
                    out.extend_from_slice(&answer);
                    self.finished = true;
                    out.extend_from_slice(&self.buffer);
                    self.buffer.clear();
                    break;
                }
                None => out.extend_from_slice(&event),
            }
        }
        out
    }

    /// Whatever is still held when the stream ends goes out as it came: an
    /// incomplete event cannot be parsed, and dropping it would be worse than
    /// forwarding a body the client can report.
    fn flush(&mut self) -> Vec<u8> {
        self.finished = true;
        std::mem::take(&mut self.buffer)
    }

    /// The length of the first event that carries a `data:` line, terminator
    /// included. A comment-only event is a keepalive, not the answer: ending the
    /// search there would leave the real payload unread.
    fn complete_event_len(&self) -> Option<usize> {
        let bytes = &self.buffer;
        let mut line_start = 0;
        let mut index = 0;
        let mut in_string = false;
        let mut escaped = false;
        let mut has_data = false;
        while index < bytes.len() {
            if bytes[index] != b'\n' || in_string {
                index += 1;
                continue;
            }
            let line = &bytes[line_start..index];
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            index += 1;
            if line.is_empty() {
                if has_data {
                    return Some(index);
                }
                has_data = false;
            } else if let Some(rest) = line.strip_prefix(b"data:") {
                has_data = true;
                let rest = rest.strip_prefix(b" ").unwrap_or(rest);
                for &payload in rest {
                    if escaped {
                        escaped = false;
                    } else if payload == b'\\' {
                        escaped = true;
                    } else if payload == b'"' {
                        in_string = !in_string;
                    }
                }
            }
            line_start = index;
        }
        None
    }
}

/// Finish the envelope carried by one event, or `None` when this event is not
/// the answer — a payload that does not parse, or one with no `result` and no
/// `error`, leaves the rewriter waiting for the event that is.
fn completed_event(method: &str, event: &[u8]) -> Option<Vec<u8>> {
    let text = String::from_utf8_lossy(event);
    let mut rewritten = String::with_capacity(text.len());
    let mut answered = false;
    let mut changed = false;
    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        let ending = if body.len() == line.len() { "" } else { "\n" };
        let Some(payload) = body
            .strip_suffix('\r')
            .unwrap_or(body)
            .strip_prefix("data:")
            .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
        else {
            rewritten.push_str(line);
            continue;
        };
        match serde_json::from_str::<Value>(payload) {
            Ok(mut envelope)
                if envelope.get("result").is_some() || envelope.get("error").is_some() =>
            {
                answered = true;
                if session::complete_envelope(method, RELAY_TTL_MS, &mut envelope) {
                    rewritten.push_str("data: ");
                    rewritten.push_str(&envelope.to_string());
                    changed = true;
                } else {
                    rewritten.push_str(line);
                }
            }
            _ => rewritten.push_str(line),
        }
        rewritten.push_str(ending);
    }
    answered.then(|| {
        if changed {
            rewritten.into_bytes()
        } else {
            event.to_vec()
        }
    })
}

/// Accumulate at most `limit` bytes; past that the answer is already a prefix and
/// the rest is dropped as it arrives. Recording off means recording nothing, not
/// recording an unbounded stream.
fn keep_prefix(acc: &mut Vec<u8>, chunk: &[u8], limit: Option<usize>) {
    let Some(limit) = limit else {
        return;
    };
    if acc.len() >= limit {
        return;
    }
    let take = (limit - acc.len()).min(chunk.len());
    acc.extend_from_slice(&chunk[..take]);
}

/// An open notification stream with no upstream behind it — the legacy answer to
/// `GET /mcp/{alias}`. Nothing is ever published; the heartbeat is the point.
pub(crate) fn notification_stream() -> Response {
    let (tx, rx) = mpsc::unbounded_channel::<Result<Vec<u8>, std::io::Error>>();
    tokio::spawn(async move {
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            heartbeat.tick().await;
            if tx.send(Ok(HEARTBEAT_FRAME.to_vec())).is_err() {
                break;
            }
        }
    });
    sse_response(rx, StatusCode::OK)
}

fn sse_response(
    rx: mpsc::UnboundedReceiver<Result<Vec<u8>, std::io::Error>>,
    status: StatusCode,
) -> Response {
    let mut response = Response::new(Body::from_stream(UnboundedReceiverStream::new(rx)));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn notification_stream_disables_proxy_buffering() {
        let response = notification_stream();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-cache"
        );
        assert_eq!(response.headers().get("x-accel-buffering").unwrap(), "no");
    }

    #[test]
    fn a_legacy_json_answer_gains_the_modern_fields() {
        let raw = br#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"a"}]}}"#;
        let body = completed_json(session::METHOD_TOOLS_LIST, raw).expect("rewritten");
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["result"]["resultType"], "complete");
        assert_eq!(value["result"]["ttlMs"], 0);
        assert_eq!(value["result"]["cacheScope"], "private");
        assert_eq!(value["result"]["tools"][0]["name"], "a");
        assert_eq!(value["id"], 2);
    }

    #[test]
    fn an_error_json_answer_is_left_alone() {
        let raw = br#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"no"}}"#;
        assert!(completed_json(session::METHOD_TOOLS_LIST, raw).is_none());
    }

    #[test]
    fn a_body_that_is_not_json_is_not_rewritten() {
        assert!(completed_json(session::METHOD_TOOLS_LIST, b"not json").is_none());
    }

    #[test]
    fn an_already_modern_json_answer_keeps_its_bytes() {
        let raw = br#"{"jsonrpc":"2.0","id":2,"result":{"tools":[],"resultType":"complete","ttlMs":9000,"cacheScope":"public"}}"#;
        assert!(completed_json(session::METHOD_TOOLS_LIST, raw).is_none());
    }

    #[test]
    fn an_sse_event_gains_the_modern_fields() {
        let event = b"event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[]}}\r\n\r\n";
        let out = completed_event(session::METHOD_TOOLS_CALL, event).expect("rewritten");
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("event: message\r\ndata: "), "{text}");
        let payload = text
            .split("data: ")
            .nth(1)
            .unwrap()
            .trim_end_matches("\r\n\r\n");
        let value: Value = serde_json::from_str(payload).unwrap();
        assert_eq!(value["result"]["resultType"], "complete");
    }

    #[test]
    fn an_sse_error_event_is_forwarded_byte_for_byte() {
        let event = b"data: {\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-1}}\r\n\r\n";
        let out = completed_event(session::METHOD_TOOLS_CALL, event).expect("an answer");
        assert_eq!(out, event);
    }

    #[test]
    fn a_notification_event_is_not_the_answer() {
        let event = b"data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\n";
        assert!(completed_event(session::METHOD_TOOLS_CALL, event).is_none());
    }

    #[test]
    fn a_comment_only_chunk_does_not_end_the_wait() {
        let mut rewriter = EventRewriter::new(session::METHOD_TOOLS_LIST);
        assert!(rewriter.feed(b":\r\n\r\n").is_empty());
        let out = rewriter.feed(b"data: {\"result\":{\"tools\":[]}}\r\n\r\n");
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with(":\r\n\r\n"), "{text}");
        let payload = text
            .split("data: ")
            .nth(1)
            .unwrap()
            .trim_end_matches("\r\n\r\n");
        let value: Value = serde_json::from_str(payload).unwrap();
        assert_eq!(value["result"]["ttlMs"], 0);
    }

    #[test]
    fn an_event_split_across_chunks_is_rewritten_once_complete() {
        let mut rewriter = EventRewriter::new(session::METHOD_TOOLS_LIST);
        assert!(rewriter.feed(b"data: {\"result\":{\"to").is_empty());
        assert!(rewriter.feed(b"ols\":[]}}\r\n").is_empty());
        let out = rewriter.feed(b"\r\n");
        assert!(!out.is_empty());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"ttlMs\":0"), "{text}");
    }

    #[test]
    fn a_newline_inside_a_json_string_is_not_an_event_boundary() {
        let mut rewriter = EventRewriter::new(session::METHOD_TOOLS_LIST);
        assert!(rewriter
            .feed(b"data: {\"result\":{\"tools\":[{\"description\":\"a")
            .is_empty());
        let out = rewriter.feed("\\nb\"}]}}\r\n\r\n".as_bytes());
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("resultType"), "{text}");
        assert!(text.contains("a\\nb"), "the description survives: {text}");
    }

    #[test]
    fn whatever_is_held_when_the_stream_ends_is_forwarded() {
        let mut rewriter = EventRewriter::new(session::METHOD_TOOLS_LIST);
        assert!(rewriter.feed(b"data: {\"result\":{\"to").is_empty());
        assert_eq!(rewriter.flush(), b"data: {\"result\":{\"to");
        assert_eq!(rewriter.feed(b"later"), b"later");
    }

    #[tokio::test]
    async fn notification_stream_opens_with_a_heartbeat_and_never_closes() {
        let response = notification_stream();
        let mut frames = response.into_body().into_data_stream();
        let first = tokio::time::timeout(Duration::from_millis(500), frames.next())
            .await
            .expect("the stream must speak without upstream traffic")
            .expect("a frame")
            .expect("no error");
        assert_eq!(first.as_ref(), HEARTBEAT_FRAME);
        let second =
            tokio::time::timeout(HEARTBEAT_INTERVAL + Duration::from_secs(5), frames.next())
                .await
                .expect("a second heartbeat")
                .expect("a frame")
                .expect("no error");
        assert_eq!(second.as_ref(), HEARTBEAT_FRAME);
    }
}
