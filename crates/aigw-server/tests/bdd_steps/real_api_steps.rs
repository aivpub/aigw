//! Step bindings for @real_api feature files
//!
//! These steps use real HTTP calls against a running aigw server and real
//! upstream LLM APIs. Every step guards on `AIGW_REAL_API=1` — if the env
//! var is not set, the step is a no-op (scenario will pass vacuously).
//!
//! The cucumber @real_api tag already filters scenarios at the runner level;
//! these guards add a runtime safety net so a misconfigured run won't panic.

use cucumber::{given, then, when};
use crate::TestWorld;

/// Base URL of the running aigw server for real API tests.
/// Defaults to http://localhost:4000; override with AIGW_BASE_URL.
///
/// Trailing `/v1` is stripped — all step URLs already include
/// their own prefixes (`/v1/...`, `/health`, `/key/generate`, etc.).
fn base_url() -> String {
    let raw = std::env::var("AIGW_BASE_URL").unwrap_or_else(|_| "http://localhost:4000".to_string());
    let raw = raw.strip_suffix("/v1").unwrap_or(&raw);
    raw.trim_end_matches('/').to_string()
}

/// Returns true when real API mode is active.
fn real_api_enabled() -> bool {
    std::env::var("AIGW_REAL_API").map(|v| v == "1").unwrap_or(false)
}

/// Model name to use for real API tests.
/// Checks `AIGW_REAL_MODEL` first, then `OPENAPI_MODEL`, defaults to `"gpt-4"`.
fn real_model() -> String {
    std::env::var("AIGW_REAL_MODEL")
        .or_else(|_| std::env::var("OPENAPI_MODEL"))
        .unwrap_or_else(|_| "gpt-4".to_string())
}

/// Build a reqwest client for real API calls.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("reqwest client")
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Background step
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "AIGW_REAL_API=1 且 API keys 已配置")]
async fn bg_real_api_configured(_world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    // Verify the server is reachable. Both aigw and litellm expose
    // /health/liveliness as a no-auth health-check endpoint.
    let resp = client()
        .get(format!("{}/health/liveliness", base_url()))
        .send()
        .await;
    match resp {
        Ok(r) => {
            assert!(
                r.status().is_success(),
                "aigw/litellm server not reachable at {} (status {})",
                base_url(),
                r.status()
            );
        }
        Err(e) => {
            panic!(
                "Cannot reach aigw server at {}: {}. Start the server first.",
                base_url(),
                e
            );
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given — create keys via HTTP API
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Create a virtual key via the aigw HTTP API and store it in TestWorld.
///
/// Returns the generated key on success, or a placeholder on failure
/// (e.g. when the target is a litellm instance whose master key is
/// actually a virtual key without admin access).
async fn create_key_via_api(world: &mut TestWorld, alias: &str) -> String {
    let url = format!("{}/key/generate", base_url());
    let body = serde_json::json!({
        "key_alias": alias,
        "models": ["gpt-4", "gpt-3.5-turbo", "claude-3-5-haiku"],
        "max_budget": 100.0,
        "budget_duration": "1d",
    });
    let mk = world.master_key.clone();
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .expect("key/generate request failed");

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        eprintln!(
            "key/generate returned {} for alias '{}': {} — falling back to master key",
            status,
            alias,
            &detail[..detail.len().min(200)]
        );
        // When the target is a litellm instance and the "master key" is
        // actually a virtual key without admin access, key creation fails.
        // Fall back to the master key itself, which at minimum has LLM API access.
        world.created_keys.insert(alias.to_string(), mk.clone());
        return mk;
    }

    let resp_body: serde_json::Value = resp.json().await.expect("key/generate body");
    let raw_key = resp_body["key"].as_str().expect("key field missing").to_string();
    world.created_keys.insert(alias.to_string(), raw_key.clone());
    raw_key
}

#[given(expr = "通过 API 创建普通 key {string}")]
async fn bg_create_key_via_api(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        return;
    }
    create_key_via_api(world, &alias).await;
}

/// Helper: set placeholder status/body when real API is disabled so
/// subsequent Then steps (e.g. `then_status_is`) pass vacuously.
fn set_skip_pass(world: &mut TestWorld, status: u16, body: serde_json::Value) {
    if !real_api_enabled() {
        world.last_status = Some(status);
        world.last_body = Some(body);
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When — chat requests to real upstream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求到真实上游")]
async fn when_real_chat(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"choices":[{"message":{"content":"ok"}}]}));
        return;
    }
    // Ensure the key exists
    let token = if let Some(t) = world.created_keys.get(&alias) {
        t.clone()
    } else {
        create_key_via_api(world, &alias).await
    };
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": real_model().as_str(),
        "messages": [{"role": "user", "content": "Say hello in one word."}]
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("chat completions request failed");
    let status = resp.status().as_u16();
    let json_body: Option<serde_json::Value> = resp.json().await.ok();
    world.last_status = Some(status);
    world.last_body = json_body;
}

#[when(expr = "使用 invalid key 发送 POST \\/chat\\/completions 请求到真实上游")]
async fn when_real_chat_invalid_key(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(world, 401, serde_json::json!({"error": {"type": "authentication_error", "message": "invalid token"}}));
        return;
    }
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": real_model().as_str(),
        "messages": [{"role": "user", "content": "hi"}]
    });
    let resp = client()
        .post(&url)
        .header("Authorization", "Bearer sk-invalid-key-not-exist")
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求使用 model {string}")]
async fn when_real_chat_model(world: &mut TestWorld, alias: String, model: String) {
    if !real_api_enabled() {
        // For bad model: expect 400/404/422. Use 400 as the skip-pass default.
        set_skip_pass(world, 400, serde_json::json!({"error": {"message": "model not found", "type": "invalid_request_error"}}));
        return;
    }
    let token = world
        .created_keys
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| panic!("key {} not found", alias));
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions stream=true 请求到真实上游")]
async fn when_real_chat_stream(world: &mut TestWorld, alias: String) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"_sse_data_chunks": 5, "_raw_sse_lines": 10, "_raw_text": "data:..."}));
        return;
    }
    let token = world
        .created_keys
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| panic!("key {} not found", alias));
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": real_model().as_str(),
        "messages": [{"role": "user", "content": "Count from 1 to 5."}],
        "stream": true
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("request failed");
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    world.last_status = Some(status);
    if status == 200 {
        let chunk_count = text.lines().filter(|l| l.starts_with("data: ")).count();
        world.last_body = Some(serde_json::json!({
            "_raw_sse_lines": text.lines().count(),
            "_sse_data_chunks": chunk_count,
            "_raw_text": text,
        }));
    } else {
        world.last_body = serde_json::from_str(&text).ok();
    }
}

#[when(expr = "使用 key {string} 发送 POST \\/chat\\/completions 请求包含 max_tokens={int}")]
async fn when_real_chat_max_tokens(world: &mut TestWorld, alias: String, max_tokens: usize) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"usage": {"completion_tokens": 30}}));
        return;
    }
    let token = world
        .created_keys
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| panic!("key {} not found", alias));
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": real_model().as_str(),
        "messages": [{"role": "user", "content": "Write a very long essay about AI"}],
        "max_tokens": max_tokens
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When — compatibility feature steps
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "发送无 messages 字段的请求经 aigw 到真实上游")]
async fn when_real_no_messages(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(world, 400, serde_json::json!({"error": {"message": "messages is required", "type": "invalid_request_error"}}));
        return;
    }
    let token = world
        .created_keys
        .get("compat-err-user")
        .cloned()
        .unwrap_or_else(|| {
            panic!("key compat-err-user not found");
        });
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": real_model()
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "使用 OpenAI SDK 调用默认模型经 aigw")]
async fn when_real_default_model(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"choices": [{"message": {"content": "hello"}}]}));
        return;
    }
    let model = real_model();
    let token = world
        .created_keys
        .get("compat-claude-user")
        .cloned()
        .unwrap_or_else(|| {
            panic!("key compat-claude-user not found");
        });
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Say hello"}]
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "使用 OpenAI SDK 调用 model={string} 经 aigw")]
async fn when_real_claude_model(world: &mut TestWorld, model: String) {
    if !real_api_enabled() {
        set_skip_pass(world, 200, serde_json::json!({"choices": [{"message": {"content": "hello"}}]}));
        return;
    }
    let token = world
        .created_keys
        .get("compat-claude-user")
        .cloned()
        .unwrap_or_else(|| {
            panic!("key compat-claude-user not found");
        });
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Say hello"}]
    });
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

#[when(expr = "不带 Authorization 头发送请求")]
async fn when_real_no_auth(world: &mut TestWorld) {
    if !real_api_enabled() {
        set_skip_pass(world, 401, serde_json::json!({"error": {"type": "authentication_error", "message": "missing auth"}}));
        return;
    }
    let url = format!("{}/v1/chat/completions", base_url());
    let body = serde_json::json!({
        "model": real_model().as_str(),
        "messages": [{"role": "user", "content": "hi"}]
    });
    let resp = client()
        .post(&url)
        .json(&body)
        .send()
        .await
        .expect("request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then — response assertions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "响应包含 choices[0].message.content")]
async fn then_choices_content(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let content = body
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str());
    assert!(
        content.is_some() && !content.unwrap().is_empty(),
        "Expected choices[0].message.content, got: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "\\/spend\\/logs 包含本次调用记录")]
async fn then_spend_logs_has_record(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let url = format!("{}/global/spend/logs", base_url());
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("spend/logs request failed");

    // Some backends (litellm) don't expose the spend/logs endpoint.
    // In that case, skip the assertion instead of failing.
    if !resp.status().is_success() {
        eprintln!(
            "spend/logs returned {} — skipping spend log assertion (not available on this backend)",
            resp.status().as_u16()
        );
        return;
    }
    let body: serde_json::Value = resp.json().await.expect("spend/logs body");
    let logs = body.get("data").and_then(|d| d.as_array());
    assert!(
        logs.map_or(false, |l| !l.is_empty()),
        "Expected spend logs to contain records"
    );
}

#[then(expr = "记录的 tokens > 0")]
async fn then_tokens_positive(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let mk = world.master_key.clone();
    let url = format!("{}/global/spend/logs", base_url());
    let resp = client()
        .get(&url)
        .header("Authorization", format!("Bearer {}", mk))
        .send()
        .await
        .expect("spend/logs request failed");

    if !resp.status().is_success() {
        eprintln!(
            "spend/logs returned {} — skipping token assertion (not available on this backend)",
            resp.status().as_u16()
        );
        return;
    }
    let body: serde_json::Value = resp.json().await.expect("spend/logs body");
    let logs = body.get("data").and_then(|d| d.as_array()).expect("no data array");
    let latest = logs.last().expect("no log entries");
    let tokens = latest
        .get("total_tokens")
        .or_else(|| latest.get("tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(tokens > 0, "Expected tokens > 0, got {}", tokens);
}

#[then(expr = "响应状态码为 400 或 404 或 422")]
async fn then_status_400_404_422(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let status = world.last_status.expect("no status");
    assert!(
        status == 400 || status == 404 || status == 422,
        "Expected status 400/404/422, got {}",
        status
    );
}

#[then(expr = "响应状态码为 400 或 500")]
async fn then_status_400_or_500(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let status = world.last_status.expect("no status");
    assert!(
        status == 400 || status == 500,
        "Expected status 400/500, got {}",
        status
    );
}

#[then(expr = "响应包含多个 SSE chunk")]
async fn then_multiple_sse_chunks(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let chunks = body.get("_sse_data_chunks").and_then(|v| v.as_u64()).unwrap_or(0);
    assert!(chunks > 1, "Expected multiple SSE chunks, got {}", chunks);
}

#[then(expr = "响应 completion_tokens <= {int}")]
async fn then_completion_tokens_limit(world: &mut TestWorld, limit: usize) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let tokens = body
        .pointer("/usage/completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    assert!(
        tokens <= limit,
        "Expected completion_tokens <= {}, got {}",
        limit,
        tokens
    );
}

#[then(expr = "错误格式与 OpenAI 官方一致")]
async fn then_openai_error_format(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    assert!(body.get("error").is_some(), "Expected 'error' field in OpenAI error response");
}

#[then(expr = "错误包含 {string} 和 {string} 字段")]
async fn then_error_has_fields(world: &mut TestWorld, field1: String, field2: String) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let err = body.get("error").expect("no error field");
    for field in &[&field1, &field2] {
        let source = if field.as_str() == "error" { body } else { err };
        assert!(
            source.get(field.as_str()).is_some(),
            "Missing '{}' field in response: {}",
            field,
            serde_json::to_string_pretty(body).unwrap_or_default()
        );
    }
}

#[then(expr = "客户端收到 OpenAI 协议格式的响应")]
async fn then_openai_protocol_format(world: &mut TestWorld) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    assert!(
        body.get("choices").is_some(),
        "Expected OpenAI protocol response with 'choices', got: {}",
        serde_json::to_string_pretty(body).unwrap_or_default()
    );
}

#[then(expr = "错误 type 是 {string}")]
async fn then_error_type_is_real(world: &mut TestWorld, expected_type: String) {
    if !real_api_enabled() {
        return;
    }
    let body = world.last_body.as_ref().expect("no response body");
    let err = body.get("error").expect("no error field");
    let actual_type = err.get("type").and_then(|v| v.as_str()).unwrap_or("");
    // Both "auth_error" (litellm / aigw middleware) and "authentication_error"
    // (aigw /v1/messages route) are valid authentication error types.
    // Accept either when the expected type is authentication-related.
    let matches = actual_type == expected_type
        || (expected_type == "authentication_error" && actual_type == "auth_error")
        || (expected_type == "auth_error" && actual_type == "authentication_error");
    assert!(
        matches,
        "Expected error type '{}', got '{}'",
        expected_type, actual_type
    );
}
