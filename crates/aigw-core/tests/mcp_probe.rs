//! Probe behaviour against real sockets — one axum router per case on
//! `127.0.0.1:0` (ADR-041).
//!
//! An integration test rather than a unit test because the code under test needs
//! a listening endpoint, and the required behaviours differ per case (405,
//! `200` + HTML, two cursor pages, an endlessly repeated cursor, an SSE body).
//! `bdd_support::mock_upstream` is a fixed-shape LLM mock; growing five switches
//! on it would be harder to read than six independent routers.

use std::sync::{Arc, Mutex};

use aigw_core::mcp::probe::{
    parse_jsonrpc_body, parse_jsonrpc_body_for, parse_upstream_tool, read_jsonrpc_response,
    ProbeError, MAX_TOOL_PAGES,
};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

struct Upstream {
    base_url: String,
    methods: Arc<Mutex<Vec<String>>>,
    initialize_count: Arc<Mutex<usize>>,
}

async fn spawn(app: Router) -> Upstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let methods = Arc::new(Mutex::new(Vec::new()));
    let initialize_count = Arc::new(Mutex::new(0usize));
    let app = app
        .layer(axum::Extension(MethodLog {
            methods: methods.clone(),
            initialize_count: initialize_count.clone(),
        }))
        .into_make_service();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Upstream {
        base_url: format!("http://{addr}/mcp"),
        methods,
        initialize_count,
    }
}

#[derive(Clone)]
struct MethodLog {
    methods: Arc<Mutex<Vec<String>>>,
    initialize_count: Arc<Mutex<usize>>,
}

fn record(log: &MethodLog, method: &str) {
    log.methods.lock().unwrap().push(method.to_string());
    if method == "initialize" {
        *log.initialize_count.lock().unwrap() += 1;
    }
}

fn method_of(body: &Value) -> String {
    body.get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn id_of(body: &Value) -> Value {
    body.get("id").cloned().unwrap_or(Value::Null)
}

fn tool(name: &str) -> Value {
    json!({
        "name": name,
        "description": format!("{name} tool"),
        "inputSchema": {"type": "object", "properties": {"q": {"type": "string"}}},
    })
}

#[tokio::test]
async fn discover_selects_the_newest_supported_version_and_skips_initialize() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(
            |axum::Extension(log): axum::Extension<MethodLog>, Json(body): Json<Value>| async move {
                let method = method_of(&body);
                record(&log, &method);
                let id = id_of(&body);
                match method.as_str() {
                    "server/discover" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {
                            "resultType": "complete",
                            "supportedVersions": ["2025-11-25", "2026-07-28"],
                            "ttlMs": 60000,
                            "serverInfo": {"name": "modern", "version": "1.0"},
                        }
                    })),
                    "tools/list" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"tools": [tool("search")]},
                    })),
                    _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
                }
            },
        ),
    ))
    .await;

    let outcome = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect("probe");

    assert_eq!(outcome.protocol_version, "2026-07-28");
    assert_eq!(outcome.ttl_ms, Some(60000));
    assert_eq!(
        outcome.server_info.as_ref().unwrap()["name"],
        Value::String("modern".into())
    );
    assert_eq!(outcome.tools.len(), 1);
    assert_eq!(outcome.tools[0].tool_name, "search");
    assert_eq!(
        *upstream.initialize_count.lock().unwrap(),
        0,
        "modern path must not send initialize"
    );
}

#[tokio::test]
async fn discover_405_falls_back_to_initialize_and_carries_the_session_id() {
    let sessions = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let seen = sessions.clone();
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(
            move |axum::Extension(log): axum::Extension<MethodLog>,
                  headers: HeaderMap,
                  Json(body): Json<Value>| async move {
                let method = method_of(&body);
                record(&log, &method);
                let id = id_of(&body);
                seen.lock().unwrap().push(
                    headers
                        .get("Mcp-Session-Id")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string),
                );
                match method.as_str() {
                    "server/discover" => (
                        axum::http::StatusCode::METHOD_NOT_ALLOWED,
                        Json(json!({"error": "no discover here"})),
                    )
                        .into_response(),
                    "initialize" => (
                        [("Mcp-Session-Id", "sess-42")],
                        Json(json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": {
                                "protocolVersion": "2025-06-18",
                                "serverInfo": {"name": "legacy"},
                            }
                        })),
                    )
                        .into_response(),
                    "notifications/initialized" => {
                        (axum::http::StatusCode::ACCEPTED, "").into_response()
                    }
                    "tools/list" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"tools": [tool("read")]},
                    }))
                    .into_response(),
                    _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})).into_response(),
                }
            },
        ),
    ))
    .await;

    let outcome = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect("probe");

    assert_eq!(outcome.protocol_version, "2025-06-18");
    assert_eq!(*upstream.initialize_count.lock().unwrap(), 1);
    let methods = upstream.methods.lock().unwrap().clone();
    assert_eq!(
        methods,
        vec![
            "server/discover".to_string(),
            "initialize".to_string(),
            "notifications/initialized".to_string(),
            "tools/list".to_string(),
        ]
    );
    let sessions = sessions.lock().unwrap().clone();
    assert_eq!(sessions[1], None, "initialize carries no session yet");
    assert_eq!(
        sessions[3].as_deref(),
        Some("sess-42"),
        "tools/list must carry the session id issued by initialize"
    );
}

#[tokio::test]
async fn tools_list_follows_next_cursor_across_pages() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(
            |axum::Extension(log): axum::Extension<MethodLog>, Json(body): Json<Value>| async move {
                let method = method_of(&body);
                record(&log, &method);
                let id = id_of(&body);
                match method.as_str() {
                    "server/discover" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"supportedVersions": ["2026-07-28"]},
                    })),
                    "tools/list" => {
                        let cursor = body
                            .get("params")
                            .and_then(|p| p.get("cursor"))
                            .and_then(Value::as_str);
                        match cursor {
                            None => Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {"tools": [tool("a"), tool("b")], "nextCursor": "page-2"},
                            })),
                            Some("page-2") => Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {"tools": [tool("c")]},
                            })),
                            Some(other) => Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "error": {"code": -32602, "message": format!("bad cursor {other}")},
                            })),
                        }
                    }
                    _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
                }
            },
        ),
    ))
    .await;

    let outcome = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect("probe");
    let names: Vec<&str> = outcome.tools.iter().map(|t| t.tool_name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn a_tool_name_repeated_across_pages_is_kept_once() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(
            |axum::Extension(log): axum::Extension<MethodLog>, Json(body): Json<Value>| async move {
                let method = method_of(&body);
                record(&log, &method);
                let id = id_of(&body);
                match method.as_str() {
                    "server/discover" => Json(json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"supportedVersions": ["2026-07-28"]},
                    })),
                    "tools/list" => {
                        let cursor = body
                            .get("params")
                            .and_then(|p| p.get("cursor"))
                            .and_then(Value::as_str);
                        match cursor {
                            None => Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {"tools": [tool("search")], "nextCursor": "page-2"},
                            })),
                            Some(_) => Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {"tools": [tool("search"), tool("read")]},
                            })),
                        }
                    }
                    _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
                }
            },
        ),
    ))
    .await;

    let outcome = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect("a repeated name must not fail the probe");
    let names: Vec<&str> = outcome.tools.iter().map(|t| t.tool_name.as_str()).collect();
    assert_eq!(names, vec!["search", "read"]);
}

#[tokio::test]
async fn a_200_with_an_html_body_is_a_failure() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(|| async {
            (
                [("Content-Type", "text/html; charset=utf-8")],
                "<!doctype html><html><body>enable JSON first</body></html>",
            )
        }),
    ))
    .await;

    let err = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect_err("HTML is not a JSON-RPC answer");
    match err {
        ProbeError::UnsupportedContentType(ct) => assert!(ct.contains("text/html"), "{ct}"),
        other => panic!("expected UnsupportedContentType, got {other:?}"),
    }
}

#[tokio::test]
async fn an_upstream_echoing_its_own_cursor_stops_at_the_page_cap() {
    let pages = Arc::new(Mutex::new(0usize));
    let counter = pages.clone();
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(
            move |axum::Extension(log): axum::Extension<MethodLog>, Json(body): Json<Value>| {
                let counter = counter.clone();
                async move {
                    let method = method_of(&body);
                    record(&log, &method);
                    let id = id_of(&body);
                    match method.as_str() {
                        "server/discover" => Json(json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": {"supportedVersions": ["2026-07-28"]},
                        })),
                        "tools/list" => {
                            let mut n = counter.lock().unwrap();
                            *n += 1;
                            let page = *n;
                            Json(json!({
                                "jsonrpc": "2.0", "id": id,
                                "result": {
                                    "tools": [tool(&format!("t{page}"))],
                                    "nextCursor": "same-cursor-forever",
                                },
                            }))
                        }
                        _ => Json(json!({"jsonrpc": "2.0", "id": id, "result": {}})),
                    }
                }
            },
        ),
    ))
    .await;

    let outcome = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect("probe");
    assert_eq!(outcome.tools.len(), MAX_TOOL_PAGES);
    assert_eq!(*pages.lock().unwrap(), MAX_TOOL_PAGES);
}

#[tokio::test]
async fn an_sse_body_yields_the_same_tools_as_the_json_branch() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(
            |axum::Extension(log): axum::Extension<MethodLog>, Json(body): Json<Value>| async move {
                let method = method_of(&body);
                record(&log, &method);
                let id = id_of(&body);
                let json_body = match method.as_str() {
                    "server/discover" => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"supportedVersions": ["2026-07-28"]},
                    }),
                    "tools/list" => json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {"tools": [tool("search"), tool("read")]},
                    }),
                    _ => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                };
                let sse = format!(
                    ": heartbeat\n\nevent: message\ndata: {}\n\n",
                    serde_json::to_string(&json_body).unwrap()
                );
                ([("Content-Type", "text/event-stream")], sse).into_response()
            },
        ),
    ))
    .await;

    let outcome = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect("probe");
    let names: Vec<&str> = outcome.tools.iter().map(|t| t.tool_name.as_str()).collect();
    assert_eq!(names, vec!["search", "read"]);
    assert_eq!(outcome.protocol_version, "2026-07-28");
}

#[tokio::test]
async fn a_server_error_surfaces_the_status_and_the_truncated_body() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(|| async {
            let body = "x".repeat(2000);
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(json!({"detail": body})),
            )
        }),
    ))
    .await;

    let err = aigw_core::mcp::probe_server(&upstream.base_url, None, &[])
        .await
        .expect_err("502 is a failure");
    assert_eq!(err.upstream_status(), Some(502));
    let body = err.body().expect("body is carried");
    assert!(
        body.chars().count() <= 500,
        "body must be truncated: {}",
        body.len()
    );
}

#[tokio::test]
async fn the_authorization_header_carries_the_resolved_token() {
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let log_seen = seen.clone();
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let log_seen = log_seen.clone();
            async move {
                log_seen.lock().unwrap().push(
                    headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string),
                );
                let id = id_of(&body);
                Json(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"supportedVersions": ["2026-07-28"], "tools": []},
                }))
            }
        }),
    ))
    .await;

    aigw_core::mcp::probe_server(&upstream.base_url, Some("sk-upstream"), &[])
        .await
        .expect("probe");
    assert_eq!(
        seen.lock().unwrap()[0].as_deref(),
        Some("Bearer sk-upstream")
    );
}

#[tokio::test]
async fn read_jsonrpc_response_hands_back_the_envelope_from_a_socket() {
    let upstream = spawn(Router::new().route(
        "/mcp",
        post(|| async { Json(json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": []}})) }),
    ))
    .await;

    let resp = reqwest::Client::new()
        .post(&upstream.base_url)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .send()
        .await
        .expect("send");
    let envelope = read_jsonrpc_response(resp).await.expect("envelope");
    assert_eq!(envelope["id"], 1);
}

#[test]
fn parse_jsonrpc_body_rejects_a_body_without_result_or_error() {
    let err = parse_jsonrpc_body(200, "application/json", r#"{"jsonrpc":"2.0","id":1}"#)
        .expect("an object envelope with neither member is still an envelope");
    assert_eq!(err["id"], 1);

    let err = parse_jsonrpc_body(200, "application/json", "<html></html>").unwrap_err();
    assert!(matches!(err, ProbeError::Malformed(_)), "{err:?}");

    let err = parse_jsonrpc_body(200, "text/plain", "{}").unwrap_err();
    assert!(matches!(err, ProbeError::UnsupportedContentType(_)));

    let err = parse_jsonrpc_body(
        200,
        "application/json",
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}"#,
    )
    .unwrap_err();
    match err {
        ProbeError::Rpc { code, message, .. } => {
            assert_eq!(code, -32601);
            assert_eq!(message, "nope");
        }
        other => panic!("expected Rpc, got {other:?}"),
    }
}

#[test]
fn parse_upstream_tool_keeps_the_wire_schema_verbatim() {
    let parsed = parse_upstream_tool(&json!({
        "name": "search",
        "inputSchema": {"type": "object", "required": ["q"]},
    }))
    .unwrap();
    assert_eq!(parsed.input_schema["required"], json!(["q"]));
    assert_eq!(parsed.description, "");
    assert_eq!(parsed.annotations, None);
}

#[test]
fn parse_jsonrpc_body_for_picks_the_envelope_with_the_expected_id() {
    let sse = "data: {\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{\"tools\":[{\"name\":\"foreign\"}]}}\n\n\
               data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"mine\"}]}}\n\n";
    let envelope =
        parse_jsonrpc_body_for(200, "text/event-stream", sse, Some(&json!(1))).expect("envelope");
    assert_eq!(envelope["id"], 1);
    assert_eq!(envelope["result"]["tools"][0]["name"], "mine");

    let err = parse_jsonrpc_body_for(200, "text/event-stream", sse, Some(&json!(2))).unwrap_err();
    assert!(matches!(err, ProbeError::Malformed(_)), "{err:?}");
}
