//! Step bindings for responses.feature
//!
//! Given steps ("mock upstream", "model pointing to mock") are reused from
//! e2e_steps.rs. This module only defines the Responses-API-specific
//! When and Then steps.

use axum::http::Method;
use cucumber::{then, when};

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

#[then(expr = "响应 Content-Type 包含 \"text/event-stream\"")]
async fn then_content_type_sse(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no response body");
    let ct = body
        .get("__content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        ct.contains("text/event-stream"),
        "Expected Content-Type to contain 'text/event-stream', got '{}'",
        ct
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
        path, substr, actual
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
    assert!(
        log.spend > 0.0,
        "spend should be > 0, got {}",
        log.spend
    );
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
async fn then_json_output_contains_type(world: &mut TestWorld, field: String, expected_type: String) {
    let body = world.last_body.as_ref().expect("no response body");
    let arr = body
        .get(&field)
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!("Expected '{}' to be array, got:\n{}",
                field,
                serde_json::to_string_pretty(body).unwrap_or_default())
        });
    let found = arr.iter().any(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some(&expected_type)
    });
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
    let output = body.get("output").and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("no output array"));
    let fc = output.iter().find(|item| {
        item.get("type").and_then(|v| v.as_str()) == Some("function_call")
    }).expect("no function_call in output");
    let val = fc.get(&field);
    assert!(
        val.is_some() && !val.unwrap().is_null(),
        "Expected function_call to have non-null field '{}', got: {:?}",
        field,
        fc
    );
}
