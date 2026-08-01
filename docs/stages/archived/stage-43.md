# Stage 43: `/v1/messages` 流式 Token 计数 + 集成测试

**Phase**: 14 — `/v1/messages` 接口修复
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 1.5h
**依赖**: Stage 41, Stage 42

---

## 目标

1. 流式模式下捕获实际 token 使用量（而非始终为 0）
2. BDD + 手工端到端验证 `/v1/messages` 完整功能

## 验收标准

- [ ] 流式请求在 OpenAI upstream 请求中添加 `stream_options: {"include_usage": true}`
- [ ] 流式模式从 SSE 最后一个 chunk 中提取 `usage`，写入 SpendLog
- [ ] 非流式模式 SpendLog 已有正确 token 计数，不受影响
- [ ] BDD 场景：非流式 `/v1/messages` 请求返回正确的 Anthropic 格式响应
- [ ] BDD 场景：流式 `/v1/messages` 请求返回正确的 Anthropic SSE 事件
- [ ] 手工验证：`curl` 测试非流式 + 流式 `/v1/messages`

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改：adapter 请求加 `stream_options` + SSE 捕获 usage chunk |
| `tests/features/v1_messages.feature` | 新增：BDD 端到端场景 |
| `crates/aigw-server/src/routes/v1_messages.rs` tests | 新增：SSE 转换单元测试 + usage 捕获测试 |

## 技术方案

### 改动 1：OpenAI 请求增加 `stream_options`

在 `DefaultAdapter::claude_to_openai_request()` 中设置 `stream: true` 后，`ChatCompletionRequest` 需要：

```rust
// 在 claude_to_openai_request 中:
ChatCompletionRequest {
    // ... 
    stream: req.stream.unwrap_or(false),
    stream_options: if req.stream.unwrap_or(false) {
        Some(StreamOptions { include_usage: true })
    } else {
        None
    },
    // ...
}
```

或在 v1_messages handler 中 post-process 请求 JSON：

```rust
// After building upstream_body:
if is_stream {
    if let Some(obj) = upstream_body.as_object_mut() {
        obj.insert("stream_options".to_string(), json!({"include_usage": true}));
    }
}
```

OpenAI 在收到 `stream_options: {"include_usage": true}` 后，会在最后一个 chunk 中附带 usage 信息：
```json
{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}
```

### 改动 2：SSE 解析循环中捕获 usage

在 Stage 41 的 SSE 解析循环中，追踪最后看到的 `ChatCompletionChunk` 的 `usage` 字段（如果 ChatCompletionChunk 有的话 — 需要先确认 model 定义）：

```rust
let mut last_usage: Option<(i32, i32)> = None; // (prompt_tokens, completion_tokens)

// In the SSE parse loop:
if let Ok(chunk) = serde_json::from_str::<serde_json::Value>(json_str) {
    if let Some(usage) = chunk.get("usage") {
        last_usage = Some((
            usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        ));
    }
}
```

然后用 `last_usage` 值写 SpendLog（替代当前的 `total_tokens: 0, prompt_tokens: 0, completion_tokens: 0`）。

### 改动 3：BDD 场景

```gherkin
Feature: Claude Messages API

  Scenario: Non-streaming message request returns Claude format
    Given a model "claude-sonnet" exists in proxy_models pointing to mock upstream
    When a POST request to "/v1/messages" with valid Claude request body
    Then the response status is 200
    And the response has Anthropic format: type="message", role="assistant"
    And the response content contains text

  Scenario: Streaming message request returns Anthropic SSE events
    Given a model "claude-sonnet" exists in proxy_models pointing to mock upstream
    When a POST request to "/v1/messages" with stream=true
    Then the response is text/event-stream
    And the SSE events contain message_start, content_block_start, content_block_delta
    And the stream ends with message_stop

  Scenario: Missing API key returns authentication_error
    When a POST request to "/v1/messages" without x-api-key header
    Then the response status is 401
    And error type is "authentication_error"
```

### 改动 4：手工验证

```bash
# 非流式
curl -s -X POST http://localhost:3000/v1/messages \
  -H "x-api-key: sk-master-key" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet","max_tokens":50,"messages":[{"role":"user","content":"Say hello"}]}' | jq .

# 流式
curl -s -N -X POST http://localhost:3000/v1/messages \
  -H "x-api-key: sk-master-key" \
  -H "anthropic-version: 2023-06-01" \
  -H "Content-Type: application/json" \
  -d '{"model":"claude-sonnet","max_tokens":50,"stream":true,"messages":[{"role":"user","content":"Say hello"}]}'
```

### 改动 5：单元测试

- `test_sse_conversion_basic` — 模拟 OpenAI SSE chunks，验证 Anthropic SSE 格式输出
- `test_sse_cross_chunk_boundary` — buffer 跨 chunk 解析
- `test_sse_done_marker` — `[DONE]` 触发 message_stop
- `test_sse_with_usage` — 有 usage 的 chunk 正确提取 token 计数

## 风险

- `ChatCompletionChunk` 模型可能不含 `usage` 字段 → 用泛型 `serde_json::Value` 解析更安全
- BDD 场景需要 mock upstream → 复用现有的 mock 上游基础设施
- 手工验证依赖真实上游环境（需配置有效的 UPSTREAM_LLM_URL 和 API key）
