//! Step bindings for responses.feature
//!
//! Given steps ("mock upstream", "model pointing to mock") are reused from
//! e2e_steps.rs. This module only defines the Responses-API-specific
//! When and Then steps.

use axum::http::Method;
use cucumber::{given, then, when};

use crate::TestWorld;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn send_responses_request(world: &mut TestWorld, alias: &str, body: serde_json::Value) {
    let state = world.ensure_state().await;
    use axum::Router;
    use tower::util::ServiceExt;

    let app = Router::new()
        .route(
            "/v1/responses",
            axum::routing::post(aigw_server::routes::responses::responses_handler),
        )
        // Mirror main.rs layer order so the x-call-id header (TD-006) is
        // present: SetRequestIdLayer generates the RequestId extension, and
        // PropagateRequestIdLayer writes it back to the response header.
        .layer(tower_http::request_id::PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-call-id"),
        ))
        .layer(tower_http::request_id::SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            aigw_core::request_id::UuidV7RequestId,
        ))
        .with_state(state);

    let token = world
        .created_keys
        .get(alias)
        .unwrap_or_else(|| panic!("key '{}' not found", alias));

    let req = axum::http::Request::builder()
        .method(Method::POST)
        .uri("/v1/responses")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(axum::body::Body::from(body.to_string()))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    let status = response.status().as_u16();
    let resp_headers = response.headers().clone();
    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();

    let is_json = resp_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.starts_with("application/json"))
        .unwrap_or(false);

    let json_body: Option<serde_json::Value> = if is_json {
        serde_json::from_slice(&body_bytes).ok()
    } else {
        let text = String::from_utf8_lossy(&body_bytes).to_string();
        Some(serde_json::json!({
            "__raw": text,
            "__content_type": resp_headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
        }))
    };
    world.last_status = Some(status);
    world.last_body = json_body;
    world.last_headers = Some(resp_headers);
}
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/responses 请求不带 model")]
async fn when_post_responses_no_model(world: &mut TestWorld, alias: String) {
    send_responses_request(world, &alias, serde_json::json!({"input": "test"})).await;
}

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/responses 请求不带 input")]
async fn when_post_responses_no_input(world: &mut TestWorld, alias: String) {
    send_responses_request(world, &alias, serde_json::json!({"model": "gpt-4o"})).await;
}

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/responses 请求")]
async fn when_post_responses(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": "hello from BDD test"
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 POST \\/v1\\/responses 流式请求")]
async fn when_post_responses_stream(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": "streaming test",
            "stream": true
        }),
    )
    .await;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(regex = r#"^响应 JSON 中 \"(.+)\" 为 \"(.+)\"$"#)]
async fn then_json_field_is(world: &mut TestWorld, field: String, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let actual = body
        .get(&field)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!(
                "Expected JSON field '{}' to be '{}', got:\n{}",
                field,
                expected,
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    assert_eq!(
        actual, expected,
        "Field '{}' expected '{}', got '{}'",
        field, expected, actual
    );
}

#[then(regex = r#"^响应 JSON 中 \"(.+)\" 数组长度大于 (\d+)$"#)]
async fn then_json_array_len_gt_min(world: &mut TestWorld, field: String, min: usize) {
    let body = world.last_body.as_ref().expect("no response body");
    let arr = body
        .get(&field)
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!(
                "Expected '{}' to be array, got:\n{}",
                field,
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    assert!(
        arr.len() > min,
        "Expected '{}' length > {}, got {}",
        field,
        min,
        arr.len()
    );
}

#[then(expr = "响应 Content-Type 包含 {string}")]
async fn then_content_type_sse(world: &mut TestWorld, expected_ct: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let ct = body
        .get("__content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        ct.contains(&expected_ct),
        "Expected Content-Type to contain '{}', got '{}'",
        expected_ct,
        ct
    );
}

#[then(regex = r#"^响应原始流包含 "(.+)" 事件$"#)]
async fn then_raw_stream_contains_event(world: &mut TestWorld, event: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let raw = body
        .get("__raw")
        .and_then(|v| v.as_str())
        .expect("no __raw streaming body");
    assert!(
        raw.contains(&format!("event: {}", event)),
        "Expected stream to contain 'event: {}', got:\n{}",
        event,
        raw
    );
}

#[then(regex = r#"^响应原始流中 "(.*)" 出现在 "(.*)" 之前$"#)]
async fn then_raw_stream_orders(world: &mut TestWorld, first: String, second: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let raw = body
        .get("__raw")
        .and_then(|v| v.as_str())
        .expect("no __raw streaming body");
    let first_pos = raw.find(&first).unwrap_or_else(|| {
        panic!("stream does not contain '{first}':\n{raw}");
    });
    let second_pos = raw.find(&second).unwrap_or_else(|| {
        panic!("stream does not contain '{second}':\n{raw}");
    });
    assert!(
        first_pos < second_pos,
        "'{first}' must appear before '{second}', got {first_pos} vs {second_pos}:\n{raw}"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// TD-006: x-call-id response header
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应头包含 x-call-id 且匹配 SpendLog call_id")]
async fn then_gw_call_id_header_matches(world: &mut TestWorld) {
    let headers = world
        .last_headers
        .as_ref()
        .expect("no captured response headers");
    let call_id = headers
        .get("x-call-id")
        .and_then(|v| v.to_str().ok())
        .expect("response missing x-call-id header");
    assert!(!call_id.is_empty(), "x-call-id header should not be empty");
    // The header must equal the SpendLog.call_id (gateway request id = UUID v7)
    let expected = call_id.to_string();
    let log = get_latest_spend_log(world).await;
    assert_eq!(
        log.call_id, expected,
        "x-call-id header '{}' must match SpendLog.call_id '{}'",
        expected, log.call_id
    );
}

#[then(regex = r#"^响应 JSON \"(.+)\" 为 \"(.+)\"$"#)]
async fn then_json_path_eq(world: &mut TestWorld, path: String, expected: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let val = resolve_json_path(body, &path);
    let actual = val.as_str().unwrap_or_else(|| {
        panic!("JSON path '{}': expected string, got {:?}", path, val);
    });
    assert_eq!(
        actual, expected,
        "JSON path '{}': expected '{}', got '{}'",
        path, expected, actual
    );
}

#[then(regex = r#"^响应 JSON \"(.+)\" 包含 \"(.+)\"$"#)]
async fn then_json_path_contains(world: &mut TestWorld, path: String, substr: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let val = resolve_json_path(body, &path);
    let actual = val.as_str().unwrap_or_else(|| {
        panic!("JSON path '{}': expected string, got {:?}", path, val);
    });
    assert!(
        actual.contains(&substr),
        "JSON path '{}': expected to contain '{}', got '{}'",
        path,
        substr,
        actual
    );
}

fn resolve_json_path<'a>(mut current: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    for part in path.split('.') {
        current = current.get(part).unwrap_or_else(|| {
            panic!("JSON path '{}': missing field '{}'", path, part);
        });
    }
    current
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// SpendLog assertions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

async fn get_latest_spend_log(world: &mut TestWorld) -> aigw_core::models::SpendLog {
    let state = world.ensure_state().await;
    let logs = state
        .db
        .query_spend_logs(None, Some(1))
        .await
        .expect("query spend logs");
    logs.into_iter().next().expect("no spend log found")
}

#[then(expr = "SpendLog 中最近一条记录的 call_id 非空")]
async fn then_spendlog_call_id_nonempty(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    assert!(!log.call_id.is_empty(), "call_id should not be empty");
}

#[then(expr = "SpendLog 中最近一条流式记录的 response 含完整文本")]
async fn then_spendlog_stream_response_has_content(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    let response = log
        .response
        .as_ref()
        .expect("streaming spend log must carry a response");
    // The final upstream chunk only holds `finish_reason`; a response that
    // stored just that one has no content at all.
    let content = response["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("");
    assert!(
        !content.is_empty(),
        "streamed response content must be merged from all chunks, got: {response}"
    );
    assert!(
        response["streaming"].as_bool().unwrap_or(false),
        "streaming flag must be preserved: {response}"
    );
}

#[then(expr = "SpendLog 中最近一条记录的 prompt_tokens 大于 0")]
async fn then_spendlog_prompt_tokens_positive(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    assert!(
        log.prompt_tokens > 0,
        "prompt_tokens should be > 0, got {}",
        log.prompt_tokens
    );
}

#[then(expr = "SpendLog 中最近一条记录的 completion_tokens 大于 0")]
async fn then_spendlog_completion_tokens_positive(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    assert!(
        log.completion_tokens > 0,
        "completion_tokens should be > 0, got {}",
        log.completion_tokens
    );
}

#[then(expr = "SpendLog 中最近一条记录的 spend 大于 0")]
async fn then_spendlog_spend_positive(world: &mut TestWorld) {
    let log = get_latest_spend_log(world).await;
    assert!(log.spend > 0.0, "spend should be > 0, got {}", log.spend);
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 102: Bridge step definitions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送带 instructions 的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_instructions(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "instructions": "You are a helpful assistant",
            "input": [{"role":"user","content":"hi"}]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送带 function tools 的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_function_tools(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": [{"role":"user","content":"what is the weather?"}],
            "tools": [{"type":"function","name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送带 web_search_preview tool 的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_web_search(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": [{"role":"user","content":"latest news"}],
            "tools": [{"type":"web_search_preview"}]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送带 code_interpreter tool 的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_code_interpreter(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": [{"role":"user","content":"analyze data"}],
            "tools": [{"type":"code_interpreter"}]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送带 namespace tool 的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_namespace(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": [{"role":"user","content":"spawn an agent"}],
            "tools": [{
                "type": "namespace",
                "name": "multi_agent_v1",
                "tools": [
                    {"type": "function", "name": "spawn_agent", "parameters": {"type": "object", "properties": {}}}
                ]
            }]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送带 developer role 的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_developer_role(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "instructions": "base rules",
            "input": [
                {"role":"developer","content":[{"type":"input_text","text":"<permissions instructions>"}]},
                {"role":"user","content":[{"type":"input_text","text":"hi"}]}
            ]
        }),
    )
    .await;
}

#[when(expr = "使用 key {string} 发送 Codex 形状的 \\/v1\\/responses 请求")]
async fn when_post_codex_shaped_request(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "instructions": "You are a coding agent running in the Codex CLI.",
            "input": [
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"<permissions>"}]},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}
            ],
            "tools": [
                {"type":"function","name":"exec_command","strict":false,"parameters":{"type":"object","properties":{}}},
                {"type":"namespace","name":"multi_agent_v1","tools":[
                    {"type":"function","name":"spawn_agent","strict":false,"parameters":{"type":"object","properties":{}}}
                ]},
                {"type":"web_search","external_web_access":false}
            ],
            "tool_choice": "auto",
            "parallel_tool_calls": true
        }),
    )
    .await;
}

/// Configure the mock upstream to answer `/v1/chat/completions` with a
/// multi-frame Chat SSE stream, so the Responses bridge's stream conversion runs.
#[given(expr = "mock 上游 chat 返回多帧流式响应")]
async fn given_mock_chat_streams(_world: &mut TestWorld) {
    use crate::bdd_steps::e2e_steps::mock_upstream;
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");

    let frame = |delta: serde_json::Value,
                 finish_reason: Option<&str>,
                 usage: Option<serde_json::Value>| {
        let mut obj = serde_json::json!({
            "id": "chatcmpl-stream-mock",
            "object": "chat.completion.chunk",
            "created": 1700000000,
            "model": "gpt-4o",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}]
        });
        if let Some(u) = usage {
            obj["usage"] = u;
        }
        obj
    };

    // Four separate stream items: the first three carry no `[DONE]`, which is
    // exactly the real-upstream shape that a single-item body cannot reproduce.
    let chunk = |v: &serde_json::Value| format!("data: {}\n\n", v).into_bytes();
    upstream.set_sse_chunks(
        "/v1/chat/completions",
        vec![
            chunk(&frame(
                serde_json::json!({"role": "assistant", "content": "Hel"}),
                None,
                None,
            )),
            chunk(&frame(serde_json::json!({"content": "lo"}), None, None)),
            chunk(&frame(
                serde_json::json!({}),
                Some("stop"),
                Some(
                    serde_json::json!({"prompt_tokens": 7, "completion_tokens": 2, "total_tokens": 9}),
                ),
            )),
            b"data: [DONE]\n\n".to_vec(),
        ],
    );
}

#[when(expr = "使用 key {string} 发送带 tool 历史的 \\/v1\\/responses 请求")]
async fn when_post_responses_with_tool_history(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "instructions": "You are a coding agent.",
            "input": [
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"<permissions>"}]},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"run echo hello"}]},
                {"type":"function_call","id":"fc_1","call_id":"call_abc123",
                 "name":"exec_command","arguments":"{\"cmd\":\"echo hello\"}"},
                {"type":"function_call_output","id":"fco_1","call_id":"call_abc123",
                 "output":"Process exited with code 0\nOutput:\nhello\n"}
            ],
            "tools": [
                {"type":"function","name":"exec_command","strict":false,
                 "parameters":{"type":"object","properties":{"cmd":{"type":"string"}}}},
                {"type":"web_search","external_web_access":false}
            ],
            "tool_choice": "auto"
        }),
    )
    .await;
}

/// Body of the most recent request the mock upstream received.
async fn last_upstream_request_body() -> serde_json::Value {
    use crate::bdd_steps::e2e_steps::mock_upstream;
    let mu = mock_upstream().lock().await;
    let upstream = mu.as_ref().expect("mock upstream not started");
    upstream
        .recorded_requests()
        .last()
        .map(|r| r.body.clone())
        .expect("mock upstream received no request")
}

#[then(expr = "上游收到的 tools 不含 {string}")]
async fn then_upstream_tools_not_contains(_world: &mut TestWorld, name: String) {
    let body = last_upstream_request_body().await;
    let names: Vec<String> = body
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    t.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !names.contains(&name),
        "upstream tools must not contain '{}', got {:?}",
        name,
        names
    );
}

#[then(expr = "上游收到的 tools 含 {string}")]
async fn then_upstream_tools_contains(_world: &mut TestWorld, name: String) {
    let body = last_upstream_request_body().await;
    let names: Vec<String> = body
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    t.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        names.contains(&name),
        "upstream tools must contain '{}', got {:?}",
        name,
        names
    );
}

#[then(expr = "上游收到的 messages 不含 role {string}")]
async fn then_upstream_messages_not_contains_role(_world: &mut TestWorld, role: String) {
    let body = last_upstream_request_body().await;
    let roles: Vec<String> = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("role").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !roles.contains(&role),
        "upstream messages must not contain role '{}', got {:?}",
        role,
        roles
    );
}

#[then(expr = "上游收到的 messages 含 role {string}")]
async fn then_upstream_messages_contains_role(_world: &mut TestWorld, role: String) {
    let body = last_upstream_request_body().await;
    let roles: Vec<String> = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("role").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        roles.contains(&role),
        "upstream messages must contain role '{}', got {:?}",
        role,
        roles
    );
}

#[when(expr = "使用 key {string} 发送带 function tools 的 \\/v1\\/responses 请求含工具调用响应")]
async fn when_post_responses_with_tool_call_response(world: &mut TestWorld, alias: String) {
    send_responses_request(
        world,
        &alias,
        serde_json::json!({
            "model": "gpt-4o",
            "input": [{"role":"user","content":"weather in Paris?"}],
            "tools": [{"type":"function","name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}]
        }),
    )
    .await;
}

#[then(regex = r#"^响应 JSON 中 \"(.+)\" 包含 type 为 \"(.+)\" 的项$"#)]
async fn then_json_output_contains_type(
    world: &mut TestWorld,
    field: String,
    expected_type: String,
) {
    let body = world.last_body.as_ref().expect("no response body");
    let arr = body
        .get(&field)
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!(
                "Expected '{}' to be array, got:\n{}",
                field,
                serde_json::to_string_pretty(body).unwrap_or_default()
            )
        });
    let found = arr
        .iter()
        .any(|item| item.get("type").and_then(|v| v.as_str()) == Some(&expected_type));
    assert!(
        found,
        "Expected '{}' to contain item with type '{}', got:\n{}",
        field,
        expected_type,
        serde_json::to_string_pretty(&arr).unwrap_or_default()
    );
}

#[then(regex = r#"^该 function_call 的 \"(.+)\" 存在$"#)]
async fn then_function_call_field_exists(world: &mut TestWorld, field: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let output = body
        .get("output")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("no output array"));
    let fc = output
        .iter()
        .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("function_call"))
        .expect("no function_call in output");
    let val = fc.get(&field);
    assert!(
        val.is_some() && !val.unwrap().is_null(),
        "Expected function_call to have non-null field '{}', got: {:?}",
        field,
        fc
    );
}
