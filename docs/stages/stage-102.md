# Stage 102: Responses → Chat Completions 协议桥接

**Phase**: 41 — OpenAI Responses API 接入
**优先级**: P1
**状态**: ✅ 完成（2026-08-05，6a3ab61；2026-08-08 文档回写）
**预估**: 14h
**前置**: Stage 101（Passthrough 端点骨架 + `ClientProtocol::Responses`）

---

## 核心预期

1. **新增适配器**: `ResponsesToChatCompletions` 实现 `MessageAdapter` + `StreamAdapter`
2. **透明桥接**: 客户端 `/v1/responses` → aigw 转换 → 上游 `/v1/chat/completions` → 响应转回 Responses API 格式
3. **全上游覆盖**: 不要求上游支持 `/v1/responses`，所有 OpenAI Compatible 上游均可用
4. **流式 SSE 事件格式转换正确**: Chat Completions SSE delta → Responses API SSE 多类型事件
5. **Stage 101 零回归**: 已有的 Passthrough 路径 BDD 全部保持绿色

---

## 背景

Stage 101 落地了 `/v1/responses` Passthrough 端点，但要求上游支持 `/v1/responses`（目前仅 OpenAI + litellm）。本 Stage 新增 `ResponsesToChatCompletions` 适配器，将 Responses API 格式透明转换为 Chat Completions 格式，使所有 OpenAI Compatible 上游均可接入。

litellm 对标：litellm 的"路径 B"桥接（对非 OpenAI provider 自动将 `/v1/responses` 转为 `/v1/chat/completions`）。

---

## 协议转换矩阵

### 请求：Responses API → Chat Completions

| Responses 字段 | Chat Completions 字段 | 转换方式 |
|---------------|----------------------|---------|
| `model` | `model` | 透传 |
| `input: "text"` | `messages: [{role:"user", content:"text"}]` | 字符串 → 单条 user 消息 |
| `input: [{role, content}]` | `messages: [...]` | 直接映射 |
| `instructions` | 前置 `messages` 插入 `{role:"system", content:instructions}` | system message |
| `stream` | `stream` | 透传 + 注入 `stream_options` |
| `max_output_tokens` | `max_tokens` | 字段重命名 |
| `temperature` | `temperature` | 透传 |
| `top_p` | `top_p` | 透传 |
| `tools: [{type:"function",...}]` | `tools: [{type:"function",...}]` | 透传 |
| `tools: [{type:"web_search_preview"/"code_interpreter"/"mcp"...}]` | — | **400 拒绝 + 错误提示** |
| `reasoning` | — | **丢弃 + `tracing::warn!`** |
| `previous_response_id` / `conversation` | — | **丢弃 + `tracing::warn!`** |
| `include[]` | — | **丢弃** |

### 响应：Chat Completions → Responses API（非流式）

```json
// 上游返回 (Chat Completions)
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "model": "gpt-4o",
  "choices": [{
    "index": 0,
    "message": {"role": "assistant", "content": "The capital is Paris."},
    "finish_reason": "stop"
  }],
  "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
}

// 转换后返回客户端 (Responses API)
{
  "id": "resp_chatcmpl-xxx",
  "object": "response",
  "status": "completed",
  "model": "gpt-4o",
  "output": [{
    "type": "message",
    "role": "assistant",
    "content": [{"type": "output_text", "text": "The capital is Paris."}]
  }],
  "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
}
```

### 流式：Chat Completions SSE → Responses API SSE

| 上游 SSE delta | aigw 输出 SSE event | 说明 |
|---------------|-------------------|------|
| 首 chunk: `delta: {role: "assistant"}` | 无事件 → `response.created` | 累积 role，发射 created 事件 |
| `delta: {content: "Hello"}` | `event: response.output_text.delta` `data: {"delta":"Hello"}` | 文本 delta |
| `delta: {tool_calls: [...]}` | `event: response.function_call_arguments.delta` `data: {"delta":"..."}` | 工具调用 delta |
| finish_reason + usage | `event: response.completed` `data: {"response":{...}}` | 终结事件，携带 usage |
| `data: [DONE]` | `data: [DONE]` | 流结束 |

---

## 适配器实现

**文件**: `crates/aigw-core/src/adapter.rs`

### `ResponsesToChatCompletions` — `MessageAdapter`

```rust
/// 将 OpenAI Responses API 请求桥接到 Chat Completions 上游。
pub struct ResponsesToChatCompletions;

impl MessageAdapter for ResponsesToChatCompletions {
    fn client_protocol(&self) -> ClientProtocol { ClientProtocol::Responses }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        // 1. 验证 tools 仅含 function 类型，否则返回 AdapterError::Unsupported
        // 2. 提取 input → 转换为 messages 数组
        //    - input 是 string → [{role:"user", content:string}]
        //    - input 是 array → 直接映射
        // 3. 提取 instructions → 在最前面插入 system message
        // 4. 字段重命名: max_output_tokens → max_tokens
        // 5. 删除: reasoning, previous_response_id, conversation, include[]
        // 6. 注入 model（deployment.upstream_model）+ stream_options
        Ok(new_body)
    }

    fn adapt_response(&self,: Value) -> Result<Value, AdapterError> {
        // 1. 提取 choices[0].message → output array
        //    - content（string） → [{"type":"output_text","text":content}]
        //    - tool_calls → 每项转 {"type":"function_call", ...}
        // 2. 字段重命名: prompt_tokens → input_tokens, completion_tokens → output_tokens
        // 3. 添加 status: "completed", object: "response"
        // 4. 错误响应（choices 为空）→ status: "failed" + error output
        Ok(new_body)
    }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(ResponsesToChatCompletionsStream::new()))
    }
}
```

### `ResponsesToChatCompletionsStream` — `StreamAdapter`

```rust
struct ResponsesToChatCompletionsStream {
    response_id: String,
    model: String,
    created_sent: bool,
    done: bool,
    text_content_index: usize,
    function_call_index: usize,
    tool_call_buf: HashMap<String, ToolCallState>,  // id → (name, arguments)
    pending_usage: Option<Value>,
}

struct ToolCallState {
    name: Option<String>,
    arguments: String,
}
```

`StreamAdapter::next()` 处理逻辑：

```
1. 解析 input chunk → JSON value
2. 如果 !created_sent:
   - 从 chunk 提取 model/id
   - 发射: event: response.created\n data: {...}\n\n
   - created_sent = true
3. 对 chunk 中的每个 choice:
   a) delta.content 非空:
      → event: response.output_text.delta\n
        data: {"delta":"...","content_index":N,"output_index":0}\n\n
   b) delta.tool_calls 非空:
      → 缓存 tool_call 状态（id → name, arguments）
      → 如果是新 tool_call（id 未见过）:
        发射 function_call_arguments.delta（index=N, delta=""）
      → 如果有 arguments delta:
        发射 function_call_arguments.delta（index=N, delta="..."）
4. 如果 chunk.choices[] 含有 finish_reason:
   - 缓存 usage 到 pending_usage（不立即发射，等 finish()）
5. 返回转换后的 buffer
```

`StreamAdapter::finish()`：

```
1. 如果 !done:
   - 遍历 tool_call_buf:
     → 发射 function_call_arguments.done（if=...）
   - 用 pending_usage 构造 response.completed 事件:
     event: response.completed\n
     data: {"response":{"id":"...","object":"response","status":"completed","usage":{...}}}\n\n
   - 发射: data: [DONE]\n\n
   - done = true
2. 返回 final buffer
```

---

## Handler 集成

修改 `responses.rs` handler：Stage 101 中 `select_adapter(ClientProtocol::Responses, ...)` 返回 `OpenAIPassthrough`，Stage 102 改为返回 `ResponsesToChatCompletions`。

上游 URL 路径从 `responses` 改为 `chat/completions`：

```rust
let adapter = select_adapter(ClientProtocol::Responses, &deployment.provider_type)
    .ok_or_else(|| ...)?;
// Stage 102: adapter = ResponsesToChatCompletions（adapt_request 会转换格式）
let upstream_body_val = adapter.adapt_request(body.clone(), &deployment)?;
// 桥接后上游 URL 走 chat/completions（不需要上游支持 /responses）
let upstream_path = match deployment.provider_type {
    ProviderType::AnthropicNative => "messages",
    _ => "chat/completions",
};
```

**注意**: `select_adapter` arm 在本 Stage 切换：

```rust
// Stage 101（Passthrough）:
(ClientProtocol::Responses, ProviderType::OpenAICompatible) => Some(&OpenAIPassthrough),

// Stage 102（Bridge）— 替换为:
(ClientProtocol::Responses, ProviderType::OpenAICompatible) => Some(&ResponsesToChatCompletions),
```

---

## TDD 测试计划

### aigw-core 适配器 Unit Tests

| # | 测试 | 描述 |
|---|------|------|
| 1 | `test_bridge_adapt_input_string` | `input: "hello"` → `messages: [{role:"user", content:"hello"}]` |
| 2 | `test_bridge_adapt_input_array` | `input: [{role, content}]` → `messages: [...]` 直接映射 |
| 3 | `test_bridge_adapt_instructions` | `instructions: "xxx"` → 前置一条 system message |
| 4 | `test_bridge_adapt_input_with_instructions` | input array + instructions → messages[0]=system, 其余=input |
| 5 | `test_bridge_adapt_max_output_tokens` | `max_output_tokens: 100` → `max_tokens: 100` |
| 6 | `test_bridge_adapt_tools_function` | function 类型 tools 数组透传 |
| 7 | `test_bridge_adapt_tools_web_search_rejected` | web_search_preview 工具 → AdapterError::Unsupported |
| 8 | `test_bridge_adapt_tools_code_interpreter_rejected` | code_interpreter 工具 → AdapterError::Unsupported |
| 9 | `test_bridge_adapt_drops_reasoning` | reasoning 字段从输出中移除 |
| 10 | `test_bridge_adapt_drops_previous_response_id` | previous_response_id 从输出中移除 |
| 11 | `test_bridge_adapt_response_nonstreaming` | 完整 ChatCompletions response → Responses API 格式 |
| 12 | `test_bridge_adapt_response_text_output` | message.content → output[0].content[0].type="output_text" |
| 13 | `test_bridge_adapt_response_tool_calls` | message.tool_calls → output[] 中 type="function_call" 项 |
| 14 | `test_bridge_adapt_response_usage_rename` | prompt_tokens→input_tokens, completion_tokens→output_tokens |
| 15 | `test_bridge_adapt_response_no_choices` | choices 为空数组 → status: "failed" |
| 16 | `test_bridge_stream_text_delta` | SSE delta.content → output_text.delta event |
| 17 | `test_bridge_stream_tool_call_delta` | SSE delta.tool_calls → function_call_arguments.delta event |
| 18 | `test_bridge_stream_response_created` | 首 SSE chunk → response.created event |
| 19 | `test_bridge_stream_response_completed` | finish() → response.completed + usage + [DONE] |

### BDD Scenarios

追加到 Stage 101 的 `responses.feature` 文件中（同一 feature 文件，新增 @bridge tag 场景）：

```gherkin
  # ─── Stage 102: 协议桥接场景 ───

  Scenario: /v1/responses bridge non-streaming
    Given MockUpstream 返回 ChatCompletions 格式响应
      | id      | chatcmpl-bridge-test |
      | model   | gpt-4o               |
      | choices | [{"index":0,"message":{"role":"assistant","content":"Bonjour"},"finish_reason":"stop"}] |
      | usage   | {"prompt_tokens":5,"completion_tokens":3,"total_tokens":8} |
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | [{"role":"user","content":"say hi in French"}] |
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"
    And 响应 JSON 中 "status" 为 "completed"
    And 响应 JSON 中 "output[0].type" 为 "message"
    And 响应 JSON 中 "output[0].content[0].type" 为 "output_text"
    And 响应 JSON 中 "output[0].content[0].text" 为 "Bonjour"
    And 响应 JSON 中 "usage.input_tokens" 为 5
    And 响应 JSON 中 "usage.output_tokens" 为 3
    And MockUpstream 收到 POST /chat/completions 请求

  Scenario: /v1/responses bridge streaming
    Given MockUpstream SSE 流式返回 ChatCompletions delta 序列
      | chunk_1 | {"choices":[{"delta":{"role":"assistant"},"index":0}]} |
      | chunk_2 | {"choices":[{"delta":{"content":"Hello"},"index":0}]}  |
      | chunk_3 | {"choices":[{"delta":{"content":" world"},"index":0}]} |
      | chunk_4 | {"choices":[{"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}} |
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 流式请求
      | model  | gpt-4o |
      | input  | [{"role":"user","content":"hi"}] |
      | stream | true |
    Then 响应状态码为 200
    And SSE 流中 event 序列为:
      | event                          | 出现次数 |
      | response.created               | 1        |
      | response.output_text.delta     | >= 2     |
      | response.completed             | 1        |
    And SSE 响应末尾有 "data: [DONE]"

  Scenario: /v1/responses bridge with instructions
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model        | gpt-4o |
      | instructions | You are a helpful assistant |
      | input        | [{"role":"user","content":"hi"}] |
    Then 响应状态码为 200
    And MockUpstream 收到的请求体中 "messages" 数组第一条为:
      | role    | system                             |
      | content | You are a helpful assistant        |
    And MockUpstream 收到的请求体中 "messages" 数组第二条的 "role" 为 "user"

  Scenario: /v1/responses bridge with function tools
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | [{"role":"user","content":"what is the weather?"}] |
      | tools | [{"type":"function","name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}] |
    Then 响应状态码为 200
    And MockUpstream 收到的请求体中 "tools" 数组长度为 1
    And MockUpstream 收到的请求体中 "tools[0].function.name" 为 "get_weather"

  Scenario: /v1/responses bridge tool call in response
    Given MockUpstream 返回 ChatCompletions 格式响应（含 tool_calls）
      | choices | [{"index":0,"message":{"role":"assistant","tool_calls":[{"id":"call_123","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]},"finish_reason":"tool_calls"}] |
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | [{"role":"user","content":"weather in Paris?"}] |
      | tools | [{"type":"function","name":"get_weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}] |
    Then 响应状态码为 200
    And 响应 JSON 中 "output" 数组包含 type 为 "function_call" 的项
    And 该 function_call 的 "call_id" 为 "call_123"
    And 该 function_call 的 "name" 为 "get_weather"
    And 该 function_call 的 "arguments" 包含 "Paris"

  Scenario: /v1/responses bridge web_search tool rejected
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | [{"role":"user","content":"latest news"}] |
      | tools | [{"type":"web_search_preview"}] |
    Then 响应状态码为 400
    And 响应 JSON 中 "error.type" 为 "invalid_request_error"
    And 响应 JSON 中 "error.message" 包含 "web_search_preview"
    And 响应 JSON 中 "error.message" 包含 "not supported"
```

**BDD step 实现**: 追加到 `crates/aigw-server/tests/bdd_steps/responses_steps.rs`。关键差异：
- MockUpstream 返回 ChatCompletions 格式（`object: "chat.completion"`，`choices[]`），验证客户端收到 Responses API 格式（`object: "response"`，`output[]`）
- 流式场景验证 SSE `event:` 字段序列（`response.created` → `response.output_text.delta` → `response.completed`）
- `instructions` / `tools` / `tool_calls` / `web_search_preview` 场景的 step 需要解析上游 MockUpstream 收到的请求体以验证转换正确性

**MockUpstream 扩展**: 复用 Stage 101 已注册的 `POST /responses` 端点。桥接场景下，MockUpstream 的 `/responses` 端点应返回 ChatCompletions 格式（模拟上游不支持 Responses API 的情况，由 handler 走桥接路径）。或者直接改 MockUpstream 的路由——桥接场景中 aigw handler 实际打的是 `/chat/completions` 而非 `/responses`，因此 BDD 使用已有 `mock_chat_completions` / `mock_chat_completions_stream` 端点即可，无需新增 MockUpstream 端点。

---

## 不支持的功能（显式排除）

| 功能 | 原因 |
|------|------|
| `GET /v1/responses/{id}` | 后续 Phase |
| `DELETE /v1/responses/{id}` | 后续 Phase |
| `POST /v1/responses/{id}/cancel` | 后续 Phase |
| `POST /v1/responses/compact` | 后续 Phase |
| 反向桥接（Chat Completions → Responses） | 后续 Phase（对标 litellm `route_all_chat_openai_to_responses`） |
| `reasoning` 参数 | Chat Completions 无对应语义，丢弃 |
| `previous_response_id` / `conversation` | 需要服务端会话管理 |
| 非 function 工具（web_search/code_interpreter/mcp） | 桥接无法映射，本 Stage 返回 400 |
| 多模态输入（audio/image/file） | 格式差异大，后续 Phase |
| Anthropic Native upstream | 需要 Responses→Anthropic 适配器，非本 Stage 范围 |

---

## 门禁标准

| 层 | 要求 |
|---|------|
| cargo check | 无编译错误 |
| aigw-core lib UT | ≥ 283 passed（264 现有 + 19 新增适配器 UT） |
| aigw-server lib UT | ≥ 116 passed（无新增 handler UT，Stage 101 已覆盖） |
| mock BDD | ≥ 190 scenarios（178 现有 + Stage 101 6 + Stage 102 6） |
| real BDD SQLite/PG/MySQL | 36/36/36 passed（零回归） |
| **Stage 101 BDD 回归** | **6/6 全部绿色**（桥接不应破坏 Passthrough 路径） |
| frontend build | green |

---

## 依赖关系

- **前置**: Stage 101（`ClientProtocol::Responses` 变体 + handler 骨架已就绪）
- **与 Stage 101 的隔离**: 适配器层是纯新增代码，handler 层只改 `select_adapter` 调用 + 上游 URL 路径。Stage 101 的 6 个 BDD 继续使用 Passthrough 模式（通过 `OpenAIPassthrough` arm），不受 Stage 102 影响。

---

## 交付清单

- [ ] `crates/aigw-core/src/adapter.rs` — `ResponsesToChatCompletions` + `ResponsesToChatCompletionsStream`
- [ ] `crates/aigw-core/src/adapter.rs` — `select_adapter` arm 从 `OpenAIPassthrough` 切换为 `ResponsesToChatCompletions`
- [ ] `crates/aigw-core/tests/` — 适配器 UT × 19（TDD 红→绿）
- [ ] `crates/aigw-server/src/routes/responses.rs` — handler 集成桥接适配器 + 上游 URL 改为 `chat/completions`
- [ ] BDD × 6（复用 Stage 101 MockUpstream，上游路径改为 `/v1/chat/completions`）
- [ ] 全量回归验证：Stage 101 6 BDD 全部保持绿色
