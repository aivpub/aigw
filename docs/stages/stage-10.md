# Stage 10: Claude /v1/messages 端点 + SSE Streaming（BDD 驱动）

**Status**: Planning
**Phase**: Phase 5 — 最小化后端补齐（RGR 驱动）
**预估工时**: 6-8h
**依赖**: Stage 9（Provider 适配层）

## Goal

实现 Anthropic 原生 `/v1/messages` 端点，支持非流式与 SSE 流式调用。客户端可用 Claude 协议访问任一上游（OpenAI 或 Claude），通过 Stage 9 的适配器自动转换。同时为现有 `/v1/chat/completions` 增加 SSE 流式代理能力。BDD 分 `@mock` 与 `@real_api` 两组场景，real 场景通过 Claude SDK / OpenAI SDK 验证端到端兼容性。

## Claude API 规范要点

> 以下为 Anthropic 官方 `/v1/messages` 契约，Stage 9 的 `ClaudeAdapter` 与本 Stage 的端点实现必须严格遵守。

### 流式判断

- **判定字段**：请求体中的 `stream` 布尔字段，默认 `false`
- **不是** `Accept` header（与 OpenAI 一致，由 body 字段决定）
- SDK 用 `.stream()` vs `.create()`，底层都是设置 `stream` 字段

### 请求必填项

**Headers**:
- `Content-Type: application/json`
- `anthropic-version: 2023-06-01`（必填，否则 400）
- `x-api-key: <key>`（或 `Authorization: Bearer <key>`，二选一）

**Body**:
- `model`（必填）
- `messages`（必填，非空数组）
- `max_tokens`（必填，>0）

### messages.content 双形态

```json
// 简写：字符串
{"role":"user","content":"hello"}

// 完整：content block 数组
{"role":"user","content":[{"type":"text","text":"hello"}]}
```

支持的 block 类型：`text` / `image` / `document` / `tool_use` / `tool_result` / `thinking`

### 非流式响应

- Content-Type: `application/json`
- 单个 Message 对象：
  ```json
  {
    "id": "msg_...",
    "type": "message",
    "role": "assistant",
    "content": [{"type":"text","text":"..."}],
    "model": "claude-3-sonnet",
    "stop_reason": "end_turn",
    "stop_sequence": null,
    "usage": {
      "input_tokens": 10,
      "output_tokens": 20,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 0
    }
  }
  ```
- `stop_reason` 取值：`end_turn` / `max_tokens` / `stop_sequence` / `tool_use` / `pause_turn` / `refusal`

### 流式响应（SSE）

- Content-Type: `text/event-stream`
- 事件格式：`event: <type>\ndata: <json>\n\n`
- 事件类型序列：
  1. `message_start` — `message.content` 为空数组，`stop_reason` 为 null
  2. `content_block_start` — 含 `index` 和 block 类型
  3. `content_block_delta` — delta 子类型：`text_delta` / `input_json_delta`（tool_use 部分 JSON）/ `thinking_delta` / `signature_delta`
  4. `content_block_stop` — 含 `index`
  5. `message_delta` — 含 `stop_reason`，**usage 是累计值**（非增量）
  6. `message_stop`
  - `ping` / `error` 可在任何时刻插入
- tool_use 流式：`input_json_delta` 是部分 JSON 字符串，客户端需累积后解析

### 错误格式

- **非流式**：`{"type":"error","error":{"type":"...","message":"..."},"request_id":"..."}`
- **流式**：作为 `event: error` SSE 事件发送（HTTP 200 之后）
- HTTP 状态码：400/401/402/403/404/413/429/500/504/529

### 兼容性测试策略

- BDD 分 `@mock`（默认执行，使用 mock 上游）和 `@real_api`（默认跳过）两组
- `@real_api` 场景用 Claude SDK / OpenAI SDK 作为客户端，验证客户端视角的真实兼容性
- 检测到 `AIGW_REAL_API=1` 或对应 API key 环境变量（`ANTHROPIC_API_KEY` / `OPENAI_API_KEY`）时执行 real 场景
- real 场景使用独立 feature 文件：`features/real/*.feature`

## 端点契约

### POST /v1/messages（非流式）

```json
// Request
{
  "model": "claude-3-sonnet",
  "max_tokens": 1024,
  "messages": [{"role":"user","content":"hi"}],
  "system": "You are helpful",
  "stream": false
}
// Response（200）
{
  "id": "msg_...",
  "type": "message",
  "role": "assistant",
  "content": [{"type":"text","text":"..."}],
  "model": "claude-3-sonnet",
  "stop_reason": "end_turn",
  "stop_sequence": null,
  "usage": {"input_tokens":10,"output_tokens":20}
}
```

### POST /v1/messages（流式 SSE）

```
event: message_start
data: {"type":"message_start","message":{"id":"msg_...","role":"assistant","content":[],"model":"claude-3-sonnet","stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}

event: message_stop
data: {"type":"message_stop"}
```

### 错误响应（对齐 Anthropic）

```json
// 400 缺少必填字段
{
  "type": "error",
  "error": {"type":"invalid_request_error","message":"max_tokens is required"},
  "request_id": "req_..."
}

// 401 鉴权失败
{
  "type": "error",
  "error": {"type":"authentication_error","message":"invalid x-api-key"},
  "request_id": "req_..."
}

// 404 模型不存在
{
  "type": "error",
  "error": {"type":"not_found_error","message":"model not found"},
  "request_id": "req_..."
}
```

## 请求流转

```
客户端 POST /v1/messages
  ↓
鉴权（x-api-key 或 Bearer）+ anthropic-version 检查
  ↓
解析 body：model / messages / max_tokens / stream / system / tools
  ↓
查找 model → 获取 provider 配置（model_params.provider）
  ↓
选择适配器：Claude→{provider协议}（Stage 9 注册表）
  ↓
transform_request（客户端 Claude 请求 → 上游请求）
  ↓
调用上游（非流式 / SSE 流式，由 stream 字段决定）
  ↓
非流式：transform_response → 返回 application/json
流式：transform_stream_chunk（状态机驱动）→ 返回 text/event-stream
```

## 关键交付件

1. `crates/aigw-server/src/routes/v1_messages.rs` — `/v1/messages` 路由（非流式 + 流式）
2. `crates/aigw-server/src/routes/v1_chat_completions.rs` — 增强 SSE 流式支持
3. `crates/aigw-core/src/streaming/mod.rs` — SSE 流处理工具
4. `crates/aigw-core/src/streaming/openai_sse.rs` — OpenAI SSE chunk 解析/生成
5. `crates/aigw-core/src/streaming/claude_sse.rs` — Claude SSE chunk 解析/生成
6. `crates/aigw-server/src/upstream/client.rs` — 上游 HTTP 客户端（流式 + 非流式）
7. `tests/bdd/features/messages.feature` — /v1/messages BDD 场景（@mock）
8. `tests/bdd/features/chat_streaming.feature` — SSE 流式 BDD 场景（@mock）
9. `tests/bdd/features/real/messages_real.feature` — 真实 Claude/OpenAI API 兼容性（@real_api）
10. `tests/bdd/steps/messages_steps.rs` — Step bindings
11. `tests/bdd/steps/streaming_steps.rs` — Step bindings

## BDD 场景

### messages.feature（@mock）

```gherkin
@mock
Feature: Claude /v1/messages 端点

  Scenario: 非流式调用 Claude 上游
    Given mock Claude 上游已启动
    And 已配置 model "claude-3" 指向 mock Claude
    When 发送 POST /v1/messages 请求
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 200
    And 响应 Content-Type 为 application/json
    And 响应 type 为 "message"
    And 响应 role 为 "assistant"
    And 响应包含 content 数组
    And 响应包含 stop_reason
    And 响应包含 usage.input_tokens

  Scenario: 非流式调用 OpenAI 上游（协议转换）
    Given mock OpenAI 上游已启动
    And 已配置 model "gpt-4" 指向 mock OpenAI
    When 发送 POST /v1/messages 请求
      """
      {"model":"gpt-4","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 200
    And 响应 type 为 "message"
    And content 为 OpenAI 响应转换而来

  Scenario: content 字符串简写兼容
    Given 已配置 Claude provider
    When 发送 POST /v1/messages 请求
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 上游收到的 messages.content 为字符串 "hi"

  Scenario: content block 数组完整形式
    Given 已配置 Claude provider
    When 发送 POST /v1/messages 请求
      """
      {"model":"claude-3","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}],"max_tokens":100}
      """
    Then 上游收到的 messages.content 为 block 数组

  Scenario: system 字段传递
    Given 已配置 Claude provider
    When 发送含 system 字段的 /v1/messages 请求
    Then 上游请求包含 system 字段

  Scenario: 必填 anthropic-version header
    When 发送 POST /v1/messages 未带 anthropic-version
    Then 响应状态码为 400
    And 错误 type 为 "invalid_request_error"

  Scenario: 缺少 max_tokens 报错
    When 发送 POST /v1/messages 请求
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}]}
      """
    Then 响应状态码为 400
    And 错误信息包含 "max_tokens"

  Scenario: 未认证请求被拒绝
    When 发送 POST /v1/messages 未带 x-api-key 或 Bearer
    Then 响应状态码为 401
    And 错误 type 为 "authentication_error"

  Scenario: 模型不存在
    When 发送 POST /v1/messages 请求 model="unknown-model"
    Then 响应状态码为 404
    And 错误 type 为 "not_found_error"

  Scenario: 流式调用 stream=true
    Given 已配置 Claude provider
    When 发送 stream=true 的 POST /v1/messages
    Then 响应 Content-Type 为 text/event-stream
    And 响应包含 "event: message_start"
    And 响应包含 "event: content_block_delta"
    And 响应包含 "event: message_stop"

  Scenario: 流式调用 OpenAI 上游（协议转换）
    Given mock OpenAI 上游已启动
    And 已配置 model "gpt-4" 指向 mock OpenAI
    When 发送 stream=true 的 POST /v1/messages（model=gpt-4）
    Then SSE 流为 Claude 格式（message_start/content_block_delta/message_stop）

  Scenario: 错误响应格式对齐 Anthropic
    Given 上游返回 429
    When 发送 POST /v1/messages
    Then 响应体为 {"type":"error","error":{...},"request_id":...}
```

### chat_streaming.feature（@mock）

```gherkin
@mock
Feature: SSE 流式代理

  Scenario: /v1/chat/completions 流式调用 OpenAI 上游
    Given mock OpenAI 上游已启动
    And 已配置 model "gpt-4" 指向 mock OpenAI
    When 发送 stream=true 的 POST /v1/chat/completions
    Then 响应 Content-Type 为 text/event-stream
    And 响应包含 "data: " 前缀的 SSE chunk
    And 最后一个 chunk 为 "data: [DONE]"

  Scenario: /v1/chat/completions 流式调用 Claude 上游（协议转换）
    Given mock Claude 上游已启动
    And 已配置 model "claude-3" 指向 mock Claude
    When 发送 stream=true 的 POST /v1/chat/completions（model=claude-3）
    Then SSE 流为 OpenAI 格式（data: {chunk} / [DONE]）

  Scenario: 4 种组合全覆盖
    Given 已配置 OpenAI 和 Claude 上游
    When 分别发送 O→O、O→C、C→O、C→C 流式请求
    Then 4 个响应均为对应协议的 SSE 流

  Scenario: 上游错误以 SSE event 传递
    Given 上游在流中途返回 429
    When 发送流式请求
    Then 客户端收到 "event: error" SSE 事件

  Scenario: 流式 usage 累计
    Given Claude mock 上游返回 message_delta usage
    When 转 OpenAI 流式
    Then OpenAI 最后 chunk 含累计 usage
```

### messages_real.feature（@real_api）

```gherkin
@real_api
Feature: 真实 Claude API 兼容性

  Scenario: Claude SDK 调用 /v1/messages 非流式
    Given AIGW_REAL_API=1 且 ANTHROPIC_API_KEY 已配置
    When 使用 Claude SDK 调用 aigw /v1/messages
    Then SDK 成功解析响应
    And 响应 stop_reason 为合法值
    And 响应 usage 字段非空

  Scenario: Claude SDK 流式调用
    Given AIGW_REAL_API=1 且 ANTHROPIC_API_KEY 已配置
    When 使用 Claude SDK stream=true 调用 aigw
    Then SDK 成功解析完整事件流
    And 事件序列为 message_start → content_block_* → message_stop

  Scenario: OpenAI 模型经 /v1/messages 调用
    Given AIGW_REAL_API=1 且 OPENAI_API_KEY 已配置
    When 使用 Claude SDK 调用 aigw model="gpt-4"
    Then SDK 成功解析（虽然上游是 OpenAI）

  Scenario: 错误格式与官方一致
    Given AIGW_REAL_API=1
    When 发送缺少 max_tokens 的请求
    Then 错误格式与 Anthropic 官方一致
```

## SSE 转换核心

### OpenAI SSE chunk
```
data: {"id":"...","choices":[{"delta":{"content":"Hello"},"index":0}]}

data: [DONE]
```

### Claude SSE event
```
event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
```

转换逻辑：解析 OpenAI `delta.content` → 生成 Claude `content_block_delta` event；收到 `[DONE]` → 生成 `message_stop` event。状态机维护在 Stage 9 的 `StreamState`。

## RGR 循环

1. **Red**: 写 `messages.feature`（@mock）+ `chat_streaming.feature`（@mock）+ `messages_real.feature`（@real_api）→ 失败（端点不存在）
2. **Green**: 实现 `/v1/messages` 路由 + SSE 流处理 + 错误格式 → 逐场景通过
3. **Refactor**: 提取 SSE 解析/生成公共逻辑到 `streaming` 模块

## 验收标准

- [ ] `messages.feature`（@mock）≥ 12 个 Scenario 全部通过
- [ ] `chat_streaming.feature`（@mock）≥ 5 个 Scenario 全部通过
- [ ] `messages_real.feature`（@real_api）≥ 4 个 Scenario（需 API key 时跳过，不阻断 CI）
- [ ] `/v1/messages` 非流式调用可用（Claude 上游 + OpenAI 上游转换）
- [ ] `/v1/messages` SSE 流式调用可用
- [ ] `/v1/chat/completions` SSE 流式调用可用
- [ ] 4 种流式组合（O→O、O→C、C→O、C→C）全部通过
- [ ] 请求必填校验：anthropic-version、max_tokens、model、messages
- [ ] messages.content 支持字符串简写与 block 数组两种形式
- [ ] 错误响应格式对齐 Anthropic（`{"type":"error","error":{...},"request_id":...}`）
- [ ] 流式错误以 `event: error` SSE 事件传递
- [ ] 上游错误正确传递给客户端
- [ ] 鉴权生效（x-api-key 或 Bearer 二选一）
- [ ] `@real_api` 场景默认跳过，`AIGW_REAL_API=1` 时执行

## 风险

| 风险 | 缓解 |
|------|------|
| SSE chunk 边界解析 | 使用 `EventSource` 风格的缓冲行解析器 |
| 流式转换背压 | 使用 `tokio::sync::mpsc` channel 控制流量 |
| 上游超时 | 配置可调 timeout，默认 30s |
| Claude SSE event 类型多 | 优先实现 message_start/content_block_delta/message_stop，其余作为 passthrough |
| real_api 测试成本 | 默认跳过，CI 不依赖；本地手动 `task bdd-real` |
| Anthropic 错误格式细节 | 严格对齐官方文档，BDD 校验 `type` / `error.type` / `request_id` 字段 |
