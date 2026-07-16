# Stage 61: AnthropicPassthrough + OpenAIToAnthropic

**Phase**: 22 — Anthropic 原生上游适配
**状态**: ⏳ 待开始
**预估**: 8h
**依赖**: 无

---

## 目标

实现两个新的 `MessageAdapter` + `StreamAdapter` 实现，补全 `select_adapter` 的缺失矩阵：

| Client Protocol | ProviderType | Before | After |
|----------------|-------------|--------|-------|
| Anthropic | AnthropicNative | ❌ None → 400 | ✅ `AnthropicPassthrough` |
| OpenAI | AnthropicNative | ❌ None → 400 | ✅ `OpenAIToAnthropic` |

---

## 变更范围

| 文件 | 变更 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | 新增 `AnthropicPassthrough` struct + `OpenAIToAnthropic` struct 及 trait 实现；复用 `DefaultAdapter` 已有转换方法；新增 `OpenAIToAnthropicStream` |

---

## 实现要点

### 1. `AnthropicPassthrough`

```rust
pub struct AnthropicPassthrough;
```

**`MessageAdapter` trait**:

- `client_protocol()` → `ClientProtocol::Anthropic`
- `adapt_request(req, deployment)`: body passthrough + 注入 headers:
  - `x-api-key: deployment.api_key`
  - `anthropic-version: 2023-06-01`
  - `content-type: application/json`
  - 保留上游返回的 `x-request-id` 映射到 aigw 的 `request_id`
- `adapt_response(resp)`: passthrough（直接返回原始 JSON body）
- `create_stream_adapter()`: 返回 `AnthropicPassthroughStream`

**`StreamAdapter` trait** (`AnthropicPassthroughStream`):

- Anthropic SSE event 逐行透传（`event: xxx\ndata: {...}\n\n` 格式不变）
- 不做 chunk 解析或转换

### 2. `OpenAIToAnthropic`

```rust
pub struct OpenAIToAnthropic;
```

**`MessageAdapter` trait**:

- `client_protocol()` → `ClientProtocol::OpenAI`
- `adapt_request`: 复用 `DefaultAdapter::openai_to_claude_request`（已有）→ Anthropic Messages body
- `adapt_response`: 复用 `DefaultAdapter::claude_to_openai_response`（已有）→ OpenAI Chat Completions response
- `create_stream_adapter()`: 返回 `OpenAIToAnthropicStream`

**`StreamAdapter` trait** (`OpenAIToAnthropicStream`):

与 `AnthropicToOpenAIStream` 方向相反：OpenAI SSE chunk → Anthropic SSE event。

状态机：
```rust
struct OpenAIToAnthropicStream {
    model: String,
    message_id: String,
    current_block_index: i32,
    current_block: Option<BlockType>,
    started: bool,
    content_blocks: Vec<ClaudeContentBlock>,  // 累积完成的 blocks（用于 content_block_stop → message_stop）
}

enum BlockType {
    Text,
    ToolUse { id: String, name: String },
}
```

`next()` 方法：
| OpenAI SSE chunk 字段 | Anthropic SSE event |
|----------------------|---------------------|
| 首个 chunk (started==false) | `message_start` |
| `choices[].delta.content` (text) | `content_block_start { type: "text" }` → `content_block_delta { text_delta }` |
| `choices[].delta.tool_calls[].id` (新 tool) | `content_block_start { type: "tool_use", id, name }` |
| `choices[].delta.tool_calls[].function.arguments` | `content_block_delta { input_json_delta }` |
| `choices[].finish_reason` | `content_block_stop` → `message_delta { stop_reason }` → `message_stop` |
| `usage` (最后一个 chunk) | `message_delta { usage }` |

finish_reason 映射对齐 `AnthropicToOpenAIStream` 的反向：
```rust
"tool_calls" => "tool_use"
"stop"       => "end_turn"
"length"     => "max_tokens"
```

---

## 单元测试（10）

| # | Struct | 场景 |
|---|--------|------|
| UT-1 | AnthropicPassthrough | `adapt_request`: body 内容不变 |
| UT-2 | AnthropicPassthrough | `adapt_request`: x-api-key + anthropic-version header 注入 |
| UT-3 | AnthropicPassthrough | `adapt_response`: error JSON 透传 |
| UT-4 | AnthropicPassthrough | stream: 多个 Anthropic SSE event 逐行透传 |
| UT-5 | OpenAIToAnthropic | `adapt_request`: system+user+assistant → ClaudeMessageRequest |
| UT-6 | OpenAIToAnthropic | `adapt_response`: ClaudeMessageResponse → ChatCompletionResponse |
| UT-7 | OpenAIToAnthropic | `adapt_request`: tool_calls → tool_use (id/name/input) |
| UT-8 | OpenAIToAnthropic | stream: `text_delta` 逐 chunk → `content_block_delta.text_delta` |
| UT-9 | OpenAIToAnthropic | stream: `tool_calls` → `content_block_start.tool_use` + `input_json_delta` |
| UT-10 | OpenAIToAnthropic | stream: 空 body / `[DONE]` 边界 |

---

## 门禁

- [ ] `cargo test adapter` 全部通过（含新增 10 UT）
- [ ] `cargo test` 全量通过
