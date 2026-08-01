# Stage 51: MessageAdapter + tool 双向转换

**Phase**: 17 — 代理转发架构重构（P1）
**状态**: ⏳ 待开始
**预估**: 5h
**依赖**: Stage 50（ModelResolver + Deployment）

---

## 目标

重构 adapter 层：将写死的 `DefaultAdapter` 替换为基于 trait 的 `MessageAdapter` 体系，并实现 `AnthropicToOpenAI` 的完整工具调用转换（这是 Claude Code 兼容的核心能力）。

1. 定义 `MessageAdapter` trait + `StreamAdapter` trait
2. 实现 `OpenAIPassthrough`（透传）
3. 实现 `AnthropicToOpenAI`（含 tool_use/tool_result ↔ tool_calls 双向转换）
4. 新增 `select_adapter()` 基于 (client_protocol, provider_type) 选择
5. `v1_messages.rs` 迁移到新 adapter 体系

## 验收标准

- [ ] `MessageAdapter` trait 定义于 adapter.rs：`adapt_request`, `adapt_response`, `stream_adapter`, `client_protocol`
- [ ] `StreamAdapter` trait 定义于 adapter.rs：`next(&mut self, chunk)`, `finish(&mut self)` — 使用 `&mut self` 维护跨 chunk 状态
- [ ] `ClientProtocol` enum：`OpenAI`, `Anthropic`
- [ ] `OpenAIPassthrough` 实现：请求透传（替换 model 字段），响应透传，无流式转换
- [ ] `AnthropicToOpenAI` 实现：
  - **请求方向**: system message、text message、tool_use → tool_calls、tool_result → tool role
  - **响应方向（非流式）**: content + tool_calls → ClaudeContentBlock[TEXT, TOOL_USE, ...]
  - **流式**: OpenAI SSE chunk → Claude SSE event（含 content_block_start/delta、tool_use、message_delta）
- [ ] `AnthropicToOpenAI` 可以通过 `deployment.raw_params` 读取 `custom_llm_provider`，用于未来按厂商过滤参数（本 Stage 暂不实现，仅预留接口）
- [ ] `v1_messages.rs` 使用 `select_adapter()` 替代直接调 `DefaultAdapter`
- [ ] **TDD**:
  - UT: `OpenAIPassthrough` 请求/响应透传
  - UT: `AnthropicToOpenAI.adapt_request` — 纯文本消息、system message
  - UT: `AnthropicToOpenAI.adapt_request` — tool_use → tool_calls
  - UT: `AnthropicToOpenAI.adapt_request` — tool_result → tool role message
  - UT: `AnthropicToOpenAI.adapt_request` — 混合消息（text + tool_use + tool_result）
  - UT: `AnthropicToOpenAI.adapt_response` — text + tool_calls → ClaudeContentBlock[]
  - UT: `AnthropicToOpenAI` stream — text chunk 转换（message_start → content_block_delta）
  - UT: `AnthropicToOpenAI` stream — tool_use chunk 转换（content_block_start + input_json_delta）
  - UT: `AnthropicToOpenAI` stream — finish（message_delta + stop_reason + usage）
  - UT: `select_adapter()` — 4 种组合
- [ ] **BDD**: `/v1/messages` 含 tool_use 请求 → OpenAI mock upstream 返回 tool_calls → 响应含 tool_use content block
- [ ] **门禁**: 全量 UT + 新增 UT 全部通过；全量 BDD 回归 + 新增 tool_use BDD 通过

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/adapter.rs` | **重构** — trait 定义 + 实现 `OpenAIPassthrough` + `AnthropicToOpenAI` + `select_adapter()` |
| `crates/aigw-core/src/models.rs` | 修改 — `ClaudeContentBlock` 新增 `tool_use`/`tool_result` 变体；`ChatMessage` 新增 `tool_calls`/`tool_call_id`；新增 `ToolCall`/`ToolCallFunction` |
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改 — 使用 `select_adapter()` 替代直接调 `DefaultAdapter` |
| `crates/aigw-server/tests/bdd_steps/messages_steps.rs` | 修改 — 新增 tool_use BDD step |

## 技术方案

### A. MessageAdapter trait

```rust
pub enum ClientProtocol {
    OpenAI,     // /v1/chat/completions
    Anthropic,  // /v1/messages
}

pub trait MessageAdapter: Send + Sync {
    fn client_protocol(&self) -> ClientProtocol;
    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value>;
    fn adapt_response(&self, body: Value) -> Result<Value>;
    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>>;
}

pub trait StreamAdapter: Send {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>>;
    fn finish(&mut self) -> Option<Vec<u8>>;
}
```

> `StreamAdapter` 使用 `&mut self` 而非 `&self`：流式 tool_use 需要跨 chunk 累积 index 状态。

### B. AnthropicToOpenAI — tool 转换规则

#### 请求方向：Claude Messages → OpenAI Chat

| Claude 输入 | OpenAI 输出 |
|------------|------------|
| `role: "user"`, `content: [{ type: "text", text }]` | `role: "user"`, `content: text` |
| `role: "user"`, `content: [{ type: "tool_result", tool_use_id, content }]` | `role: "tool"`, `tool_call_id`, `content` |
| `role: "assistant"`, `content: [{ type: "text", text }]` | `role: "assistant"`, `content: text` |
| `role: "assistant"`, `content: [{ type: "tool_use", id, name, input }]` | `role: "assistant"`, `content: null`, `tool_calls: [{ id, type: "function", function: { name, arguments } }]` |
| `system` (text/blocks) | `role: "system"` message |
| `tools: [{ name, description, input_schema }]` | `tools: [{ type: "function", function: { name, description, parameters } }]` |

关键转换：
- `tool_use.input` 是 JSON Object → `tool_calls[].function.arguments` 是 JSON String
- 多个 tool_use block 在一个 assistant message 里 → 多个 tool_calls

#### 响应方向（非流式）：OpenAI Chat → Claude Messages

| OpenAI 响应 | Claude 输出 |
|------------|------------|
| `choices[].message.content` | `content: [{ type: "text", text }]` |
| `choices[].message.tool_calls[]` | `content: [{ type: "tool_use", id, name, input }]` |
| `choices[].finish_reason: "tool_calls"` | `stop_reason: "tool_use"` |

#### 流式响应：OpenAI SSE chunk → Claude SSE event

| OpenAI chunk delta | Claude SSE event |
|-------------------|-----------------|
| `role: "assistant"` | `message_start` |
| `content: "text..."` (首个 text) | `content_block_start { type: "text" }` + `content_block_delta { text_delta }` |
| `tool_calls[0].id` (首个 tool_call) | `content_block_start { type: "tool_use", id, name }` |
| `tool_calls[0].function.arguments` (追加) | `content_block_delta { input_json_delta }` |
| `finish_reason` | `message_delta { stop_reason }` |
| `usage` (最后一帧) | `message_delta { usage }` |

**跨 chunk 状态维护（StreamAdapter 内部）：**
```rust
struct AnthropicToOpenAIStream {
    model: String,
    request_id: String,
    message_id: String,
    current_block_index: i32,
    current_block_type: Option<BlockType>, // Text | ToolUse
    started: bool,  // 是否已发送 message_start
}
```

### C. select_adapter()

```rust
fn select_adapter(client: ClientProtocol, provider: &ProviderType)
    -> Option<&'static dyn MessageAdapter>
{
    match (client, provider) {
        (ClientProtocol::OpenAI, ProviderType::OpenAICompatible)
            => Some(&OpenAIPassthrough),
        (ClientProtocol::Anthropic, ProviderType::OpenAICompatible)
            => Some(&AnthropicToOpenAI),
        _ => None,
    }
}
```

## 风险

- **tool_use.input ↔ tool_calls[].arguments 类型差异**：input 是 JSON object，arguments 是 JSON string，serialize/deserialize 方向必须对
- **流式 tool_use 跨 chunk 状态**：OpenAI 分多个 chunk 发送 arguments，需要累积拼接再转成 Claude 格式
- **content_block index**：Claude 的 content_block_start 需要 index，流式场景下随着不同 block 类型切换递增
- **finish_reason 映射**：`tool_calls` → `tool_use`，不是标准映射，需要显式处理
