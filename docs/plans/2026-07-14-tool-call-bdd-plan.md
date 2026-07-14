# /v1/messages 工具调用端到端 BDD 测试方案

**日期**: 2026-07-14
**状态**: 待审计
**依赖**: Stage 51（MessageAdapter tool 转换已完成）

---

## 1. 背景

Stage 51 实现了 `AnthropicToOpenAI` 的 tool_use/tool_result ↔ tool_calls 双向转换（含流式），但当前缺少端到端 BDD 场景验证完整链路。需要设计 BDD 测试来验证 HTTP 层面的工具调用往返。

## 2. 测试目标

1. 验证非流式 tool_use → tool_calls → tool_use 往返正确
2. 验证流式 SSE tool_use → tool_calls → tool_use 往返正确
3. 验证 tool_result → "tool" role → assistant tool_use 完整多轮对话

## 3. 测试架构

```
reqwest (模拟 Claude 客户端)
    │  POST /v1/messages
    │  {"model":"claude", "messages":[{role:"assistant", content:[{type:"tool_use",...}]}], ...}
    ▼
aigw-server (随机端口, BDD ServerGuard 管理)
    │  AnthropicToOpenAI.adapt_request() → tool_use → tool_calls
    │  stream_options: {"include_usage": true} 注入
    ▼
Mock Upstream (随机端口, mock_upstream.rs)
    │  /v1/chat/completions 接受 OpenAI 格式请求
    │  返回预配置的 tool_calls 响应（非流式 JSON 或流式 SSE array）
    ▼
aigw ← AnthropicToOpenAI.adapt_response() → tool_calls → tool_use
    │  (流式: AnthropicToOpenAIStream 逐 chunk 转换)
    ▼
验证点:
  - HTTP 状态码 = 200
  - 响应 content[] 含 tool_use block 且字段值正确
  - 响应 stop_reason = "tool_use"
  - 流式场景验证 SSE event 序列: content_block_start → content_block_delta →
    message_delta
```

## 4. 测试方案：HTTP 客户端直接调用

**选择原因**: 不需要引入 Python/Node SDK 依赖，不需要真实上游 API key。直接用 `reqwest` 模拟 Claude 客户端行为发 HTTP 请求。

### 4.1 Mock Upstream 增强

当前 `mock_upstream.rs` 的 `openai_handler` 不支持流式 SSE 响应（注释 TODO）。需要增强：

**A. 非流式 Mock 配置**（已有，无需修改）:

```rust
mock.set_response("/v1/chat/completions", 200, json!({
    "id": "chatcmpl-001",
    "object": "chat.completion",
    "choices": [{
        "index": 0,
        "message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
                "id": "call_abc",
                "type": "function",
                "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
            }]
        },
        "finish_reason": "tool_calls"
    }],
    "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}
}));
```

**B. 流式 SSE 响应**（新增）：

```rust
// 新增: 识别流式请求并返回 SSE chunks
// Mock response 为 JSON array 时，每个元素作为一个 SSE chunk 发送
mock.set_response("/v1/chat/completions", 200, json!([
    {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]},
    {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":""}}]}}]},
    {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":\"NYC\"}"}}]}}]},
    {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]},
    {"id":"1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":50,"completion_tokens":20,"total_tokens":70}}
]));
```

mock_upstream.rs 改造要点：
- 当 mock response body 是 `Value::Array` 时，遍历数组逐元素发 SSE frame `data: {json}\n\n`
- 数组尾部的 `[DONE]` 标志由 handler 自动追加

### 4.2 BDD 场景设计

#### Scenario 1: 非流式工具调用往返

```gherkin
Scenario: /v1/messages 非流式工具调用往返
  Given 已启动 mock upstream
  And mock upstream 的后端会 tool_calls 响应到 /v1/chat/completions
    """
    {
      "id": "chatcmpl-001",
      "object": "chat.completion",
      "choices": [{
        "index": 0,
        "message": {
          "role": "assistant", "content": null,
          "tool_calls": [{
            "id": "call_abc", "type": "function",
            "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
          }]
        },
        "finish_reason": "tool_calls"
      }],
      "usage": {"prompt_tokens": 50, "completion_tokens": 20, "total_tokens": 70}
    }
    """
  And 已配置 model "claude-tool" 指向 mock upstream
  When 向 aigw 的 /v1/messages 发送含 tool_use 的请求
    """
    {
      "model": "claude-tool",
      "max_tokens": 1024,
      "messages": [{
        "role": "assistant",
        "content": [{
          "type": "tool_use",
          "id": "toolu_01",
          "name": "get_weather",
          "input": {"city": "NYC"}
        }]
      }]
    }
    """
  Then 响应状态码为 200
  And 响应 content 含 type="tool_use" 的 block
  And 该 tool_use block 的 id 为 "call_abc"
  And 该 tool_use block 的 name 为 "get_weather"
  And 该 tool_use block 的 input.city 为 "NYC"
  And 响应 stop_reason 为 "tool_use"
  And 响应格式为 Anthropic Messages 格式
```

#### Scenario 2: 流式工具调用（SSE chunks）

```gherkin
Scenario: /v1/messages 流式工具调用
  Given 已启动 mock upstream
  And mock upstream 返回流式 /v1/chat/completions 的 tool_call chunks
    """
    [
      {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]},
      {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"id":"call_abc","type":"function","function":{"name":"get_weather","arguments":""}}]}}]},
      {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"city\":\"NYC\"}"}}]}}]},
      {"id":"1","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]},
      {"id":"1","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":50,"completion_tokens":20,"total_tokens":70}}
    ]
    """
  And 已配置 model "claude-tool" 指向 mock upstream
  When 向 aigw 的 /v1/messages 发送流式含 tool_use 的请求
    """
    {"model":"claude-tool","max_tokens":1024,"stream":true,"messages":[{"role":"user","content":"What is the weather in NYC?"}]}
    """
  Then 响应状态码为 200
  And 收到 SSE event "message_start"
  And 收到 SSE event "content_block_start" type="tool_use"
  And 收到 SSE event "content_block_delta" delta_type="input_json_delta"
  And 收到 SSE event "message_delta" stop_reason="tool_use"
```

#### Scenario 3: tool_result → tool role → 多轮对话

```gherkin
Scenario: /v1/messages tool_result 到 OpenAI tool role 的转换
  Given 已启动 mock upstream
  And mock upstream 返回 /v1/chat/completions 文本响应
  And 已配置 model "claude-tool" 指向 mock upstream
  When 向 aigw 的 /v1/messages 发送含 tool_result 的请求
    """
    {
      "model": "claude-tool",
      "max_tokens": 1024,
      "messages": [
        {"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_01", "name": "get_weather", "input": {"city": "NYC"}}]},
        {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_01", "content": "72F, sunny"}]},
        {"role": "user", "content": "Should I bring an umbrella?"}
      ]
    }
    """
  Then 响应状态码为 200
  # 验证 /v1/messages handler 正确处理了 tool_result → tool role 的转换
  # mock upstream 收到的请求中，中间那条消息应该是 role="tool"
```

### 4.3 Mock Upstream 返回的 raw response 字段

当前 `MockResponse` 用 `body: Value` 存单个 JSON。需要支持两种模式：

| 模式 | body 格式 | 行为 |
|------|----------|------|
| 非流式 | `{...}` (single JSON object) | 直接 JSON 返回（现有行为） |
| 流式 | `[{...}, {...}, ...]` (JSON array) | 遍历数组逐元素发送 `data: {item}\n\n`，末尾追加 `data: [DONE]\n\n` |

改造 `openai_handler`:

```rust
async fn openai_handler(...) -> Result<Response, (StatusCode, Json<Value>)> {
    let is_stream = body_val.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    
    if is_stream {
        let chunks = match &mock.body {
            Value::Array(arr) => arr.clone(),
            _ => vec![mock.body.clone()],
        };
        let stream = tokio_stream::iter(chunks.into_iter().map(|c| {
            Ok::<_, Infallible>(
                format!("data: {}\n\n", serde_json::to_string(&c).unwrap_or_default()).into_bytes()
            )
        }));
        return Ok(Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap());
    }
    
    // 非流式: 现有逻辑
    Ok((StatusCode::OK, Json(mock.body)))
}
```

## 5. 实施步骤

### 新增文件

| 文件 | 内容 |
|------|------|
| `tests/features/tool_call.feature` | 3 个 tool_use BDD 场景 |
| `tests/bdd_steps/tool_call_steps.rs` | Step 绑定：Given/When/Then |

### 修改文件

| 文件 | 变更 |
|------|------|
| `tests/bdd_support/mock_upstream.rs` | `openai_handler` 支持流式 SSE + JSON array 响应 |
| `tests/bdd_steps/messages_steps.rs` | 新增 `发送含 tool_use 请求` 等 step |
| `tests/bdd_steps/mod.rs` | 注册 `tool_call_steps` 模块 |

### 时间预估

| 步骤 | 预估 |
|------|------|
| Mock upstream SSE 改造 | 1h |
| 写 3 个 .feature 场景 | 0.5h |
| Step 绑定实现 | 1.5h |
| 跑通测试 | 0.5h |
| **合计** | **3.5h** |

## 6. 风险与对策

| 风险 | 对策 |
|------|------|
| Mock upstream SSE bytes stream 与 aigw 的 SSE parser 格式不对齐 | 严格按照 `data: {json}\n\n` 格式发帧 |
| `AnthropicToOpenAIStream` tool_use delta 解析跨 chunk 有边界 bug | 在流式场景中逐 chunk 验证 event 序列，快速定位 |
| `ServerGuard` 启动 aigw 需要 `AIGW_TEST_START_SERVER=1` | 本地开发时默认设置此环境变量 |
