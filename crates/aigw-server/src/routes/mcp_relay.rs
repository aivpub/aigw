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
pub(crate) async fn record_with_bodies(
    db: &Database,
    limit: Option<usize>,
    row: &McpCallLogRow,
    request: &[u8],
    response: &[u8],
) {
    record(db, row).await;
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
pub(crate) async fn relay(
    upstream: reqwest::Response,
    db: Database,
    row: McpCallLogRow,
    request: Vec<u8>,
    limit: Option<usize>,
) -> Response {
    let call_id = row.call_id.clone();
    let response = if is_event_stream(&upstream) {
        relay_event_stream(upstream, db, row, request, limit)
    } else {
        relay_buffered(upstream, db, row, &request, limit).await
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
) -> Response {
    let status = upstream_status(&upstream);
    let content_type = upstream_content_type(&upstream);
    let body = match upstream.bytes().await {
        Ok(bytes) => bytes.to_vec(),
        Err(e) => {
            warn!(alias = %row.server_alias, error = %e, "upstream body read failed");
            row.status = MCP_CALL_STATUS_FAILED.to_string();
            row.error_code = Some("transport_error".to_string());
            Vec::new()
        }
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
) -> Response {
    let status = upstream_status(&upstream);
    let (tx, rx) = mpsc::unbounded_channel::<Result<Vec<u8>, std::io::Error>>();
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
                        forwarded += bytes.len() as i64;
                        keep_prefix(&mut answer, &bytes, limit);
                        if tx.send(Ok(bytes.to_vec())).is_err() {
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
        row.response_bytes = forwarded;
        record_with_bodies(&db, limit, &row, &request, &answer).await;
    });
    sse_response(rx, status)
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
