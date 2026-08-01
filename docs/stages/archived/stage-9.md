# Stage 9: Provider 适配转换层（BDD 驱动）

**Status**: Planning
**Phase**: Phase 5 — 最小化后端补齐（RGR 驱动）
**预估工时**: 6-8h
**依赖**: Stage 8（模型管理 CRUD）

## Goal

实现 ProviderAdapter trait 架构，支持 OpenAI ↔ Claude 双向格式转换（4 种组合：O→O、O→C、C→O、C→C），**流式与非流式独立转换路径**。参考 litellm 的 Python adapter 模式（`BaseConfig` trait + `AnthropicStreamWrapper` 状态机）和 litellm-rust 的 `crates/core/src/providers/` transformation 模块设计。为 Stage 10 的 `/v1/messages` 端点打基础。

## 参考来源

### litellm-rust（`~/works/projects/github.com/BerriAI/litellm/litellm-rust`）

litellm 官方 Rust 实现，3 crate workspace：
- `crates/core` — 纯转换层（providers/<provider>/<route>/transformation.rs），无网络
- `crates/ai-gateway` — axum 路由 + 网络 I/O
- `crates/python-bridge` — PyO3

provider 目录结构：`providers/src/<provider>/<route>/transformation.rs`，每个 provider 实现请求/响应转换。

### litellm Python adapter 模式

- **`BaseConfig`**（`litellm/llms/base_llm/chat/transformation.py`）— 抽象基类：
  - `transform_request` / `transform_response` / `map_openai_params` / `validate_environment` / `get_complete_url` / `get_error_class`
- **双向 adapter**（`litellm/llms/anthropic/experimental_pass_through/adapters/`）：
  - `AnthropicAdapter` — `translate_completion_input_params`（请求转换）/ `translate_completion_output_params`（非流式响应）/ `translate_completion_output_params_streaming`（流式转换）
  - `LiteLLMAnthropicMessagesAdapter` — messages/system 分离/tool choice/tool name 截断/image 格式
- **流式状态机** `AnthropicStreamWrapper`：
  - 状态：`sent_first_chunk` / `sent_content_block_start/finish` / `current_content_block_type` / `holding_chunk`
  - 事件序列：message_start → content_block_start → content_block_delta → content_block_stop → message_delta → message_stop
  - `_CombinedChunkSplitter` 处理 fake-stream（content + finish_reason 合并）
- **流式与非流式独立**：`translate_completion_output_params` vs `translate_completion_output_params_streaming`，两套独立逻辑

## 4 种转换组合

| 客户端协议 | 上游 Provider | 转换器 | 用例 |
|-----------|--------------|--------|------|
| OpenAI | OpenAI | OpenAIAdapter（passthrough） | 现有 `/v1/chat/completions` |
| OpenAI | Claude | OpenAIToClaude | 客户端用 OpenAI 格式调用 Claude 上游 |
| Claude | OpenAI | ClaudeToOpenAI | 客户端用 `/v1/messages` 调用 OpenAI 上游 |
| Claude | Claude | ClaudeAdapter（passthrough） | `/v1/messages` 直通 Claude 上游 |

## 关键交付件

1. `crates/aigw-core/src/provider_adapter/mod.rs` — `ProviderAdapter` trait + `Protocol` enum
2. `crates/aigw-core/src/provider_adapter/openai.rs` — `OpenAIAdapter`（passthrough）
3. `crates/aigw-core/src/provider_adapter/claude.rs` — `ClaudeAdapter`（passthrough）
4. `crates/aigw-core/src/provider_adapter/openai_to_claude.rs` — 请求/响应/流式转换
5. `crates/aigw-core/src/provider_adapter/claude_to_openai.rs` — 请求/响应/流式转换
6. `crates/aigw-core/src/provider_adapter/registry.rs` — 适配器注册表（按 src/dst 协议查找）
7. `crates/aigw-core/src/streaming/mod.rs` — SSE 流处理工具
8. `crates/aigw-core/src/streaming/state_machine.rs` — 流式状态机（参考 AnthropicStreamWrapper）
9. `tests/bdd/features/provider_adapter.feature` — 非流式转换 BDD
10. `tests/bdd/features/provider_adapter_streaming.feature` — 流式转换 BDD
11. `tests/bdd/steps/provider_adapter_steps.rs`

## ProviderAdapter Trait

参考 litellm `BaseConfig` + litellm-rust `transformation.rs` 模式：

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn name(&self) -> &'static str;

    /// 客户端请求 → 上游请求（非流式 & 流式共用）
    fn transform_request(
        &self,
        client_request: Value,
        target_protocol: Protocol,
    ) -> Result<Value, AdapterError>;

    /// 上游非流式响应 → 客户端响应
    fn transform_response(
        &self,
        upstream_response: Value,
        client_protocol: Protocol,
    ) -> Result<Value, AdapterError>;

    /// 上游流式响应（整体） → 客户端流式响应（整体）
    /// 用于"收集上游全部 chunk 后一次性转换"的场景
    fn transform_stream_batch(
        &self,
        upstream_chunks: Vec<String>,
        client_protocol: Protocol,
    ) -> Result<Vec<String>, AdapterError>;

    /// 上游单个 SSE chunk → 客户端 SSE chunk(s)
    /// 用于真正的逐 chunk 流式转换（状态机驱动）
    /// 返回 Vec 因为一个上游 chunk 可能产生 0-N 个下游 chunk
    fn transform_stream_chunk(
        &self,
        chunk: &str,
        state: &mut StreamState,
        client_protocol: Protocol,
    ) -> Result<Vec<String>, AdapterError>;
}

pub enum Protocol {
    OpenAI,
    Claude,
}

/// 流式状态机（参考 AnthropicStreamWrapper）
pub struct StreamState {
    pub sent_first_chunk: bool,
    pub sent_content_block_start: bool,
    pub sent_content_block_finish: bool,
    pub current_content_block_type: Option<String>,
    pub current_block_index: usize,
    pub holding_chunk: Option<String>,
    pub accumulated_usage: Option<Usage>,
}
```

## 转换规则（非流式）

### OpenAI → Claude（请求）

| OpenAI 字段 | Claude 字段 | 说明 |
|------------|------------|------|
| `messages` | `messages` | role/content 直接映射 |
| messages 中 system role | `system` 顶层字段 | OpenAI 在 messages 里，Claude 在顶层 |
| `max_tokens` | `max_tokens` | 直转 |
| `temperature` / `top_p` | 同名 | 直转 |
| `model` | `model` | 直转 |
| `stream` | `stream` | 直转 |
| `tools` | `tools` | 结构差异需转换 |
| `tool_choice` | `tool_choice` | auto/any/tool 映射 |
| `stop` | `stop_sequences` | 重命名 |

### Claude → OpenAI（响应）

| Claude 字段 | OpenAI 字段 | 说明 |
|------------|------------|------|
| `content` (list of blocks) | `choices[0].message.content` (string) | 拼接 text block |
| `stop_reason` | `choices[0].finish_reason` | end_turn→stop, max_tokens→length, tool_use→tool_calls |
| `usage.input_tokens` | `usage.prompt_tokens` | 重命名 |
| `usage.output_tokens` | `usage.completion_tokens` | 重命名 |
| content 中 tool_use block | `choices[0].message.tool_calls` | 转换 |

## 转换规则（流式）

### OpenAI SSE chunk → Claude SSE events

OpenAI 单个 chunk 可能只含 `delta.content`，但 Claude 需要完整事件序列：
- 第一个 chunk → 发 `message_start` + `content_block_start`
- delta.content → `content_block_delta`（text_delta）
- finish_reason 出现 → `content_block_stop` + `message_delta`（stop_reason）+ `message_stop`
- `[DONE]` → 不发新事件（已发 message_stop）

### Claude SSE events → OpenAI SSE chunk

- `message_start` → 不发（等第一个 content_block_delta）
- `content_block_delta` (text_delta) → OpenAI chunk `delta.content`
- `content_block_delta` (input_json_delta) → OpenAI chunk `delta.tool_calls[].function.arguments`
- `message_delta` (stop_reason) → OpenAI chunk `finish_reason` + `data: [DONE]`
- `message_stop` → 不发（已发 [DONE]）

### 状态机关键点

- 一个上游 chunk 可能产生 0-N 个下游 chunk（如 OpenAI 首个 chunk → 2 个 Claude 事件）
- `_CombinedChunkSplitter` 模式：当 content + finish_reason 在同一 chunk，拆分为独立事件
- usage 累计：Claude `message_delta` 的 usage 是累计值，OpenAI 无此概念，需在 message_stop 前合并

## BDD 场景

### provider_adapter.feature（非流式）

```gherkin
Feature: Provider 适配转换层（非流式）

  Scenario: OpenAI 请求直通 OpenAI 上游
    Given OpenAI 协议请求 {model:"gpt-4",messages:[...]}
    When 目标上游协议为 OpenAI
    Then 转换后请求保持不变

  Scenario: OpenAI 请求转 Claude 格式
    Given OpenAI 协议请求包含 system message
    When 转换为 Claude 协议
    Then system 内容移到顶层 system 字段
    And messages 不再包含 system role
    And max_tokens 字段保留

  Scenario: Claude 响应转 OpenAI 格式
    Given Claude 响应 content 为 [{type:"text",text:"hello"}]
    When 转换为 OpenAI 协议
    Then choices[0].message.content 为 "hello"
    And usage.prompt_tokens 来自 input_tokens
    And choices[0].finish_reason 来自 stop_reason

  Scenario: stop_reason 映射
    Given Claude 响应 stop_reason 为 "end_turn"
    When 转换为 OpenAI
    Then finish_reason 为 "stop"
    Given Claude 响应 stop_reason 为 "tool_use"
    When 转换为 OpenAI
    Then finish_reason 为 "tool_calls"

  Scenario: Claude 请求直通 Claude 上游
    Given Claude 协议请求 {model:"claude-3",messages:[...]}
    When 目标上游协议为 Claude
    Then 转换后请求保持不变

  Scenario: tools 参数转换
    Given OpenAI 请求包含 tools 定义
    When 转换为 Claude 协议
    Then tools 结构符合 Claude 规范

  Scenario: tool_choice 转换
    Given OpenAI 请求 tool_choice 为 "auto"
    When 转换为 Claude
    Then tool_choice 为 {"type":"auto"}

  Scenario: 适配器注册表查找
    Given 适配器注册表已初始化
    When 查找 OpenAI→Claude 适配器
    Then 返回 OpenAIToClaudeAdapter
    When 查找 Claude→OpenAI 适配器
    Then 返回 ClaudeToOpenAIAdapter

  Scenario: 未知协议报错
    Given 请求协议为 "unknown"
    When 转换请求
    Then 返回 AdapterError UnsupportedProtocol
```

### provider_adapter_streaming.feature（流式）

```gherkin
Feature: Provider 适配转换层（流式）

  Scenario: OpenAI 流式 chunk 转 Claude events（首个 chunk）
    Given OpenAI 首个 chunk 含 delta.content "Hello"
    When 流式转换为 Claude
    Then 产生 2 个事件
    And 第 1 个事件为 message_start
    And 第 2 个事件为 content_block_start

  Scenario: OpenAI 流式 chunk 转 Claude events（内容 chunk）
    Given OpenAI chunk 含 delta.content " world"
    And 状态机已发送 message_start
    When 流式转换为 Claude
    Then 产生 1 个事件 content_block_delta
    And delta.type 为 "text_delta"

  Scenario: OpenAI 流式 chunk 转 Claude events（结束 chunk）
    Given OpenAI chunk 含 finish_reason "stop"
    And 状态机已发送 content_block_delta
    When 流式转换为 Claude
    Then 产生 3 个事件
    And 依次为 content_block_stop, message_delta, message_stop
    And message_delta 的 stop_reason 为 "end_turn"

  Scenario: Claude events 转 OpenAI chunk（message_start）
    Given Claude 事件 message_start
    When 流式转换为 OpenAI
    Then 产生 0 个 chunk（等待 content_block_delta）

  Scenario: Claude events 转 OpenAI chunk（content_block_delta text）
    Given Claude 事件 content_block_delta 含 text_delta "Hello"
    When 流式转换为 OpenAI
    Then 产生 1 个 chunk
    And chunk 含 delta.content "Hello"

  Scenario: Claude events 转 OpenAI chunk（input_json_delta）
    Given Claude 事件 content_block_delta 含 input_json_delta "{\"loc"
    When 流式转换为 OpenAI
    Then 产生 1 个 chunk
    And chunk 含 delta.tool_calls

  Scenario: Claude events 转 OpenAI chunk（message_delta）
    Given Claude 事件 message_delta 含 stop_reason "end_turn"
    When 流式转换为 OpenAI
    Then 产生 1 个 chunk 含 finish_reason "stop"
    And 最后产生 data: [DONE]

  Scenario: Combined chunk splitting
    Given OpenAI chunk 同时含 delta.content 和 finish_reason
    When 流式转换为 Claude
    Then 拆分为 content_block_delta + content_block_stop + message_delta + message_stop

  Scenario: usage 累计合并
    Given Claude message_delta 含 usage output_tokens 15
    When 转换为 OpenAI
    Then OpenAI chunk 的 usage 为累计值
```

## RGR 循环

1. **Red**: 写 `provider_adapter.feature`（9 场景）+ `provider_adapter_streaming.feature`（9 场景）→ 失败
2. **Green**: 实现 trait + 4 适配器 + 流式状态机 → 逐场景通过
3. **Refactor**: 提取公共字段映射到 `field_mapping.rs`，状态机通用化

## 验收标准

- [ ] `provider_adapter.feature` ≥ 9 个 Scenario 全部通过
- [ ] `provider_adapter_streaming.feature` ≥ 9 个 Scenario 全部通过
- [ ] `ProviderAdapter` trait 定义清晰（4 个方法）
- [ ] 4 种转换组合全部实现且有单测
- [ ] 流式与非流式独立转换路径
- [ ] 流式状态机正确维护 sent_first_chunk 等状态
- [ ] 一个上游 chunk 可产生 0-N 个下游 chunk
- [ ] Combined chunk splitting 生效
- [ ] system message 转换正确（OpenAI messages → Claude 顶层 system）
- [ ] usage 字段转换正确（input/output_tokens ↔ prompt/completion_tokens）
- [ ] stop_reason 映射正确（end_turn→stop, tool_use→tool_calls）
- [ ] 适配器注册表支持按 (源协议, 目标协议) 查找
- [ ] 单元测试覆盖率 ≥ 85%

## 风险

| 风险 | 缓解 |
|------|------|
| tools/function calling 结构差异大 | 先支持基础转换，复杂 tool_choice 标记 TODO |
| 流式状态机复杂 | 参考 litellm `AnthropicStreamWrapper`，先实现 text block，tool_use 后续 |
| Claude content block 多类型 | 优先支持 text block，image/tool_use/thinking 后续迭代 |
| litellm-rust trait 不完全匹配 | 取其目录结构 + trait 思路，aigw 按自身需求设计 |
