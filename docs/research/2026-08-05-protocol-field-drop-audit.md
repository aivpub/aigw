# aigw 协议转换字段静默丢弃：全面审计报告

> 日期: 2026-08-05
> 状态: P0 待修复，P1/P2 待排期
> 触发: DeepSeek V4 Flash thinking 模式多轮对话 400 错误

## 1. Context

上轮生产排查确认 DeepSeek V4 Flash（Anthropic 协议路径）因 `reasoning_content` 字段静默丢弃导致 400 错误。本报告扩展到全量审计——对所有 Anthropic↔OpenAI 协议转换涉及的 Rust 结构体逐字段对比 API 规范，识别所有静默丢弃的字段及其风险等级。

## 2. 核心机制：「结构体反序列化 = 字段过滤器」

`AnthropicToOpenAI` 适配器通过 Rust 强类型 struct 做 `serde_json::from_value` — 未定义的字段被 `serde` 自动丢弃，无任何警告。

**对比：`OpenAIPassthrough` 适配器**使用 `serde_json::Value` 直通，不做结构体反序列化，所有字段自动保留。`deepseek-v4-pro-202606` 走 tokenhub 中转（OpenAI 直通路径），因此 `reasoning_content` 正常保留。

字段流失发生在四个转换函数：

| 方向 | 函数 | 数据流 | 丢弃机制 |
|------|------|--------|----------|
| 请求 Anthropic→OpenAI | `AnthropicToOpenAI::adapt_request()` | `Value → ClaudeMessageRequest → ChatCompletionRequest → Value` | 两次 struct 反序列化过滤 |
| 响应 OpenAI→Anthropic | `AnthropicToOpenAI::adapt_response()` | `Value → ChatCompletionResponse → ClaudeMessageResponse → Value` | 两次 struct 反序列化过滤 |
| 流式 Anthropic→OpenAI | `AnthropicToOpenAIStream::next()` | `SSE → ChatCompletionChunk → ClaudeStreamEvent` | `Delta`/`ChunkChoice` 过滤 |
| 流式 OpenAI→Anthropic | `OpenAIToAnthropicStream::next()` | `SSE → ChatCompletionChunk → ClaudeStreamEvent` | 同上 |

## 3. 审计结果

### 3.1 🔴 严重（已触发/即将触发生产故障）

| # | 缺失字段 | 所属结构体 | 后果 | 状态 |
|---|---------|-----------|------|------|
| 1 | **`reasoning_content`** | `AssistantMessage` (`models.rs:613`), `ChatMessage` (`models.rs:541`), `Delta` (`models.rs:646`) | DeepSeek V4 Flash thinking 模式多轮对话 → 400 `invalid_request_error: "The reasoning_content in the thinking mode must be passed back to the API"` | **已确认生产故障** |
| 2 | **`ChatCompletionChunk.usage`** | `ChatCompletionChunk` (`models.rs:629`) | 流式请求的最终 token 用量被丢弃。上游在 SSE 最后 chunk 返回 `usage`（通过 `stream_options.include_usage`），Claude Code 永远收不到 token 统计 | **静默丢失**（`adapt_request` 手动注入了 `stream_options.include_usage=true`，但响应端 struct 没收这个字段） |
| 3 | **Anthropic `thinking` content block** | `ClaudeContentBlock` (`models.rs:1002`) | Claude extended thinking 内容块 `{type: "thinking", thinking: "...", signature: "..."}` 被 `claude_message_to_openai()` 过滤（只允许 text/image）；`claude_blocks_to_text()` 也只读 text，thinking 内容完全丢失 | **静默丢失** |
| 4 | **Anthropic `cache_read_input_tokens` / `cache_creation_input_tokens`** | `ClaudeUsage` (`models.rs:1048`) | Anthropic 的 prompt caching token 统计完全无法追踪。`ClaudeUsage` 只有 `input_tokens`/`output_tokens`，缓存命中节约的 token 信息丢失 | **静默丢失** |

### 3.2 🟡 中等（功能不完整，可能触发异常）

| # | 缺失字段 | 所属结构体 | 后果 |
|---|---------|-----------|------|
| 5 | **`Usage.prompt_tokens_details`** | `Usage` (`models.rs:621`) | 含 `cached_tokens`, `audio_tokens` — 无法追踪缓存命中 token，影响成本核算 |
| 6 | **`Usage.completion_tokens_details`** | `Usage` (`models.rs:621`) | 含 `reasoning_tokens`, `audio_tokens`, `accepted_prediction_tokens`, `rejected_prediction_tokens` — DeepSeek/OpenAI thinking 模型成本核算失效 |
| 7 | **`ChatCompletionRequest.response_format`** | `ChatCompletionRequest` (`models.rs:494`) | JSON Mode (`{type: "json_object"}`) 和 Structured Output (`{type: "json_schema", json_schema: {...}}`) 不支持 |
| 8 | **`ChatCompletionRequest.reasoning_effort`** | `ChatCompletionRequest` (`models.rs:494`) | 无法控制 DeepSeek/OpenAI o1/o3 thinking 深度 (`"low"/"medium"/"high"`) |
| 9 | **`ClaudeMessageRequest.thinking`** | `ClaudeMessageRequest` (`models.rs:944`) | Claude 的 `thinking: {type: "enabled", budget_tokens: N}` 在请求阶段就被丢弃，无法映射为 OpenAI 的 `reasoning_effort` |
| 10 | **`AssistantMessage.refusal`** / **`Delta.refusal`** | `AssistantMessage` (`models.rs:613`), `Delta` (`models.rs:646`) | 模型安全拒绝时 `refusal` 理由丢失，下游看到空 `content` 无解释 |
| 11 | **`ChatCompletionResponse.system_fingerprint`** / **`ChatCompletionChunk.system_fingerprint`** | `ChatCompletionResponse` (`models.rs:595`), `ChatCompletionChunk` (`models.rs:629`) | OpenAI/DeepSeek 返回的 system_fingerprint 丢失，错误追溯受影响 |

### 3.3 🟢 低（不影响当前使用场景，但不够完备）

| # | 缺失字段 | 所属结构体 | 影响 |
|---|---------|-----------|------|
| 12 | `ChatCompletionRequest.seed` | `ChatCompletionRequest` | 无法要求可复现输出 |
| 13 | `ChatCompletionRequest.logprobs` / `top_logprobs` | `ChatCompletionRequest` | log 概率不可用 |
| 14 | `ChatCompletionRequest.parallel_tool_calls` | `ChatCompletionRequest` | 无法禁用并行 tool calls |
| 15 | `ChatCompletionRequest.n` | `ChatCompletionRequest` | 不支持多个 completion |
| 16 | `ChatCompletionRequest.logit_bias` | `ChatCompletionRequest` | token 级别偏置 |
| 17 | `ChatCompletionRequest.modalities` | `ChatCompletionRequest` | 无法控制输出模态 (`["text", "audio"]`) |
| 18 | `ChatCompletionRequest.max_completion_tokens` | `ChatCompletionRequest` | o-series 模型用 `max_completion_tokens` 替代 `max_tokens` |
| 19 | `ChatCompletionRequest.prediction` | `ChatCompletionRequest` | Predicted Outputs 不支持 |
| 20 | `ChatCompletionRequest.audio` | `ChatCompletionRequest` | 音频输出配置不支持 |
| 21 | `ChatCompletionRequest.service_tier` | `ChatCompletionRequest` | 服务等级选择 |
| 22 | `ToolDef.strict` | `ToolDef` (`models.rs:521`) | Structured Output 严格模式 |
| 23 | `AssistantMessage.function_call` / `Delta.function_call` | `AssistantMessage`, `Delta` | 旧版 function_call（已废弃，但部分模型仍使用） |
| 24 | `AssistantMessage.audio` / `Delta.audio` | `AssistantMessage`, `Delta` | 音频响应的 delta |
| 25 | `AssistantMessage.annotations` | `AssistantMessage` | web search URL citations |
| 26 | `ContentPart.input_audio` | `ContentPart` (`models.rs:579`) | 音频输入模态 `{type: "input_audio", input_audio: {...}}` |
| 27 | `ContentPart.file` | `ContentPart` | 文件上传 `{type: "file", file: {...}}` |
| 28 | `Choice.logprobs` | `Choice` (`models.rs:605`) | 响应中的 logprobs |
| 29 | `ClaudeContentBlock.citations` | `ClaudeContentBlock` (`models.rs:1002`) | Anthropic 文本引文 |
| 30 | `ClaudeContentBlock.thinking` (block) | `ClaudeContentBlock` | thinking 专用 block 字段 (`thinking`, `signature`) |
| 31 | `ClaudeContentBlock.redacted_thinking` | `ClaudeContentBlock` | 脱敏 thinking block 字段 |
| 32 | `ClaudeContentBlock.server_tool_use` | `ClaudeContentBlock` | MCP server 工具调用 |
| 33 | `ClaudeContentBlock.search_result` | `ClaudeContentBlock` | Anthropic web search 结果 |
| 34 | `ClaudeContentBlock.usage` | `ClaudeContentBlock` | 按 content block 的 token 用量 |
| 35 | `ClaudeMessageRequest.disable_parallel_tool_use` | `ClaudeMessageRequest` (`models.rs:944`) | 禁用并行工具 |
| 36 | `ClaudeMessageRequest.service_tier` | `ClaudeMessageRequest` | 服务等级 |
| 37 | stop_reason `"refusal"` 映射 | `adapter.rs:1248` | refual → OpenAI 对应项未映射 |
| 38 | `ClaudeDelta.thinking_delta` / `signature_delta` | `ClaudeDelta` (`models.rs:1094`) | 流式 thinking 文本/签名 |

## 4. 生产数据佐证

### 4.1 `reasoning_content` 在 OpenAIPassthrough 路径正常

```
model: deepseek-v4-pro-202606
api_base: https://tokenhub.tencentmaas.com/v1  ← OpenAIPassthrough，不做 struct 反序列化
response: "reasoning_content": "We are asked: \"Say hello\"..."
```
→ 证明字段存在，只是 AnthropicToOpenAI 适配器路径丢失。

### 4.2 故障请求特征

```
model: deepseek-v4-flash
api_base: https://api.deepseek.com           ← 原厂直连
client: Claude Code (user-agent: claude-cli)  ← Anthropic 协议
请求路径: POST /v1/messages                    ← Anthropic endpoint
适配器: AnthropicToOpenAI                      ← struct 反序列化 → reasoning_content 丢失
错误: 400 - "The `reasoning_content` in the thinking mode must be passed back to the API."
```

### 4.3 24h 统计

| 模型 | 成功 | 失败 | 适配器路径 | reasoning_content 问题 |
|------|------|------|-----------|----------------------|
| deepseek-v4-flash | 703 | 2 | **AnthropicToOpenAI** | ✅ 已触发 |
| deepseek-v4-pro-202606 | 3473 | 1 (429) | **OpenAIPassthrough** (tokenhub) | ❌ 不受影响 |
| ep-iswqr9k0 / ep-s55rmple / ep-4d2bdbhd | 3432+ | 少量 429/504 | **OpenAIPassthrough** (tokenhub) | ❌ 不受影响 |

## 5. 修复优先级

### P0 — 阻塞生产（立即）

1. **`ChatMessage.reasoning_content`** + **`AssistantMessage.reasoning_content`** + **`Delta.reasoning_content`** — 修复 DeepSeek thinking 400
2. **`ChatCompletionChunk.usage`** — 修复流式 token 计数丢失

### P1 — 功能性缺陷（本阶段）

3. **`Usage.prompt_tokens_details` / `completion_tokens_details`** — 修复缓存/thinking token 成本核算
4. **Anthropic `thinking` block 处理** — `ClaudeContentBlock` 支持 `{type: "thinking"}` 和 `{type: "redacted_thinking"}`
5. **`ChatCompletionRequest.response_format`** — 支持 JSON Mode/Structured Output

### P2 — 完备性改进（后续）

6. `ChatCompletionRequest.reasoning_effort` / `ClaudeMessageRequest.thinking`
7. `AssistantMessage.refusal` / `Delta.refusal`
8. Anthropic `cache_read_input_tokens` / `cache_creation_input_tokens`
9. `system_fingerprint`
10. 其他 🟢 优先级字段

## 6. 实现范围（P0+P1）

### models.rs 修改

1. `ChatMessage` (`models.rs:541`): 添加 `reasoning_content: Option<String>`
2. `AssistantMessage` (`models.rs:613`): 添加 `reasoning_content: Option<String>`, `refusal: Option<String>`
3. `Delta` (`models.rs:646`): 添加 `reasoning_content: Option<String>`, `refusal: Option<String>`
4. `Usage` (`models.rs:621`): 添加 `prompt_tokens_details: Option<UsageDetails>`, `completion_tokens_details: Option<UsageDetails>`
5. `ChatCompletionChunk` (`models.rs:629`): 添加 `usage: Option<Usage>`
6. `ChatCompletionRequest` (`models.rs:494`): 添加 `response_format: Option<ResponseFormat>`, `reasoning_effort: Option<String>`
7. `ClaudeMessageRequest` (`models.rs:944`): 添加 `thinking: Option<ThinkingConfig>`
8. `ClaudeContentBlock` (`models.rs:1002`): 添加 thinking block 字段 (`thinking`, `signature`)
9. `ClaudeUsage` (`models.rs:1048`): 添加 `cache_read_input_tokens`, `cache_creation_input_tokens`
10. 新增辅助结构体：`UsageDetails`, `ResponseFormat`, `ThinkingConfig`

### adapter.rs 修改

1. `claude_message_to_openai()` (`adapter.rs:1112`):
   - assistant 消息含 `reasoning_content` 时传递；
   - assistant 消息含 `thinking` blocks 时转为 `reasoning_content`
2. `oai_response_to_claude_messages()` (`adapter.rs:205`):
   - `AssistantMessage.reasoning_content` → ClaudeContentBlock `{type: "thinking"}`
3. `AnthropicToOpenAIStream::next()` / `OpenAIToAnthropicStream::next()`:
   - `Delta.reasoning_content` → Claude SSE `thinking_delta` 事件
   - `ChatCompletionChunk.usage` → Claude SSE `message_delta` 的 usage

## 7. 验证计划

1. 单元测试：验证含 `reasoning_content` 的消息在 round-trip 中不丢失
2. 模拟 DeepSeek thinking 模式 → 连续 2 轮对话不 400
3. 验证 `usage` 在流式响应最后 chunk 正确传递到 Claude Code
4. 生产验证：同 API key 发送 tool_calls 多轮对话
