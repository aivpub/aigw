# Stage 101: POST /v1/responses Passthrough 端点

**Phase**: 41 — OpenAI Responses API 接入
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 8h
**前置**: 无
**后置**: Stage 102（Responses→Chat 协议桥接）

---

## 核心预期

1. **新建端点**: `POST /v1/responses` — 接受 OpenAI Responses API 格式请求
2. **纯 Passthrough**: 接受请求后直接转发到上游 `{api_base}/responses`，不做协议转换
3. **流式 + 非流式**: 支持 `stream: true/false`
4. **认证对齐**: 复用 `ChatAuth` Bearer Token / Cookie JWT 认证
5. **适配器零改动**: 复用 `OpenAIPassthrough`，请求/响应透传
6. **SpendLog 正确记录**: 从 Responses API 响应中提取 `usage.input_tokens` / `usage.output_tokens` 写入 SpendLog
7. **现有功能不受影响**: 全量回归通过

---

## 背景

OpenAI 于 2025 年推出 Responses API (`POST /v1/responses`)，定位为 Chat Completions API 的继任者。litellm 目前已支持该端点。

本 Stage 先落地 Passthrough 端点骨架（验证认证→解析→上游→SpendLog 链路正确），Stage 102 在此基础上加协议转换适配器覆盖所有上游。

aigw 当前支持的 LLM 端点：
- `POST /v1/chat/completions` ✅
- `POST /v1/messages` ✅
- `POST /v1/responses` ❌（本 Stage 填补）

详见调研：`docs/research/2026-08-04-openai-responses-api-support.md`

---

## 适配器分析

**`OpenAIPassthrough` 无需修改**。它只做两件事：替换 `model` 字段 + 流式注入 `stream_options`。请求/响应格式完全不转换。Responses API 和 Chat Completions 的格式差异在 **handler 层**处理：

| 差异点 | chat.rs（ChatCompletions） | responses.rs（Responses Passthrough） |
|--------|--------------------------|--------------------------------------|
| 请求校验 | `body.get("messages")` | `body.get("input")` |
| 上游 URL 路径 | `chat/completions` | `responses` |
| 响应 Usage | `usage.prompt_tokens` / `completion_tokens` | `usage.input_tokens` / `output_tokens` + fallback |
| 适配器 | `OpenAIPassthrough` | `OpenAIPassthrough` **同一个** |

### 新增 `ClientProtocol::Responses`

虽然适配器复用 `OpenAIPassthrough`，但 handler 层需要区分协议类型来决定校验字段、上游 URL 路径、Usage 解析策略。

```rust
pub enum ClientProtocol {
    OpenAI,      // /v1/chat/completions
    Anthropic,   // /v1/messages
    Responses,   // /v1/responses ← new
}
```

`select_adapter` 新增 arm：

```rust
(ClientProtocol::Responses, ProviderType::OpenAICompatible) => Some(&OpenAIPassthrough),
```

---

## 架构

```
Client (Responses API) → aigw POST /v1/responses
                              │
                              ├─ ChatAuth 认证
                              ├─ 请求校验（model, input）
                              ├─ Key 权限 & 预算检查
                              ├─ ModelResolver::resolve()
                              ├─ Router::pick_deployment()
                              ├─ select_adapter(ClientProtocol::Responses, ...)
                              │      → OpenAIPassthrough
                              ├─ upstream URL: {api_base}/responses
                              ├─ HTTP 调用 (streaming/non-streaming)
                              ├─ 响应 Usage 解析（input_tokens/output_tokens + fallback）
                              └─ SpendLog 记录
```

---

## 实现要点

### 1. 新增 Handler 文件

**文件**: `crates/aigw-server/src/routes/responses.rs`

Handler 函数签名：

```rust
pub async fn responses_handler(
    State(state): State<SharedState>,
    ChatAuth(auth): ChatAuth,
    OptionalClientIp(client_ip): OptionalClientIp,
    headers: axum::http::HeaderMap,
    http::request::Parts { extensions, .. }: http::request::Parts,
    Json(body): Json<Value>,
) -> Result<axum::response::Response, (StatusCode, Json<Value>)>
```

核心流程与 `chat_completions` 一致，差异点：

```
a) 请求校验:
   - 校验 model 字段
   - 校验 input 字段（非 messages）
   - input 可以是 string（简写）或 array

b) 上游 URL 路径:
   - ProviderType::AnthropicNative → "messages"（与 chat.rs 相同）
   - ProviderType::OpenAICompatible → "responses"（差异：chat.rs 走 "chat/completions"）

c) 响应 Usage 解析（双 fallback）:
   let prompt_tokens = usage
       .get("input_tokens")       // Responses API 原生
       .or_else(|| usage.get("prompt_tokens"))  // Chat Completions fallback
       .and_then(|v| v.as_u64());

   let completion_tokens = usage
       .get("output_tokens")       // Responses API 原生
       .or_else(|| usage.get("completion_tokens"))  // Chat Completions fallback
       .and_then(|v| v.as_u64());

d) 流式处理:
   - 复用两阶段 SpendLog 模式（Phase 1 INSERT 占位 → Phase 2 UPDATE）
   - SSE chunk 透传（PassthroughStream）
   - 在 [DONE] 前最后一个包含 usage 的 JSON 对象中提取 tokens

e) 错误格式:
   复用 OpenAI 标准错误 JSON
```

### 2. 路由注册

**文件**: `crates/aigw-server/src/main.rs`

```rust
.route("/v1/responses", axum::routing::post(responses::responses_handler))
```

**文件**: `crates/aigw-server/src/routes/mod.rs`

```rust
pub mod responses;
```

### 3. 共享代码

- `crates/aigw-core/src/adapter.rs` — `ClientProtocol::Responses` 变体 + `select_adapter` arm（复用 `OpenAIPassthrough`）
- `crates/aigw-server/src/routes/chat.rs` — `calc_spend` + `extract_pricing` 改为 `pub(crate)`

---

## TDD 测试计划

### Unit Tests (aigw-server lib)

| # | 测试 | 描述 |
|---|------|------|
| 1 | `test_responses_missing_model` | 缺少 model 参数返回 400 |
| 2 | `test_responses_missing_input` | 缺少 input 参数返回 400 |
| 3 | `test_responses_nonstreaming` | 非流式请求正确转发并记录 SpendLog |
| 4 | `test_responses_streaming` | 流式请求 SSE 透传正确 |
| 5 | `test_responses_usage_input_output` | usage.input_tokens/output_tokens 正确解析 |
| 6 | `test_responses_usage_prompt_completion_fallback` | prompt_tokens/completion_tokens fallback |

### BDD Scenarios

新建文件 `crates/aigw-server/tests/features/responses.feature`：

```gherkin
@mock
Feature: OpenAI Responses API Passthrough — /v1/responses

  Background:
    Given 数据库中已有 key "sk-test-responses" 绑定模型 "gpt-4o"
    And MockUpstream 已启动

  Scenario: Non-streaming /v1/responses passthrough
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | [{"role":"user","content":"hello"}] |
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"
    And 响应 JSON 中 "output" 数组长度大于 0
    And MockUpstream 收到 POST /responses 请求

  Scenario: Streaming /v1/responses SSE passthrough
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 流式请求
      | model  | gpt-4o |
      | input  | [{"role":"user","content":"hello"}] |
      | stream | true |
    Then 响应状态码为 200
    And 响应 Content-Type 包含 "text/event-stream"
    And SSE 流包含至少一个 data chunk
    And MockUpstream 收到 POST /responses/stream 请求

  Scenario: /v1/responses with input string
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | "hello world" |
    Then 响应状态码为 200
    And MockUpstream 收到的请求体中 "input" 为 "hello world"

  Scenario: /v1/responses missing model returns 400
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | input | [{"role":"user","content":"test"}] |
    Then 响应状态码为 400
    And 响应 JSON 中 "error.type" 为 "invalid_request_error"
    And 响应 JSON 中 "error.message" 包含 "model"

  Scenario: /v1/responses missing input returns 400
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
    Then 响应状态码为 400
    And 响应 JSON 中 "error.type" 为 "invalid_request_error"
    And 响应 JSON 中 "error.message" 包含 "input"

  Scenario: /v1/responses spend log recorded
    When 使用 key "sk-test-responses" 发送 POST /v1/responses 请求
      | model | gpt-4o |
      | input | [{"role":"user","content":"token counting test"}] |
    Then 响应状态码为 200
    And SpendLog 中最近一条记录的 call_id 非空
    And SpendLog 中最近一条记录的 prompt_tokens 大于 0
    And SpendLog 中最近一条记录的 completion_tokens 大于 0
    And SpendLog 中最近一条记录的 spend 大于 0
    And SpendLog 中最近一条记录的 endpoint 为 "/v1/responses"
```

**BDD step 实现**: `crates/aigw-server/tests/bdd_steps/responses_steps.rs`（新建）。复用 `chat_steps.rs` 的认证注入、JSON body 构建、SSE 解析模式。MockUpstream 新增 `POST /responses` 和 `POST /responses/stream` 端点，返回标准 Responses API 格式响应（含 `object: "response"`、`output[]`、`usage.input_tokens`/`output_tokens`）。

---

## 门禁标准

|  | 要求 |
|---|------|
| cargo | 无编译错误 |
| aigw-core lib UT | 264 passed（适配器层无改动，零回归） |
| aigw-server lib UT | ≥ 116 passed（110 现有 + 6 新增） |
| mock BDD | ≥ 184 scenarios（178 现有 + 6 新增） |
| real BDD SQLite | 36 scenarios passed（零回归） |
| real BDD PG | 36 scenarios passed（零回归） |
| real BDD MySQL | 36 scenarios passed（零回归） |
| frontend build | green |

---

## 依赖关系

- **无前向依赖** — 独立模块
- Stage 102 依赖本 Stage（`ClientProtocol::Responses` + handler 骨架就绪后加适配器）

---

## 交付清单

- [ ] `crates/aigw-core/src/adapter.rs` — `ClientProtocol::Responses` 变体 + `select_adapter` arm
- [ ] `crates/aigw-server/src/routes/responses.rs` — Passthrough handler
- [ ] `crates/aigw-server/src/routes/mod.rs` — `pub mod responses;`
- [ ] `crates/aigw-server/src/main.rs` — 路由注册 `/v1/responses`
- [ ] `crates/aigw-server/src/routes/chat.rs` — `calc_spend` + `extract_pricing` → `pub(crate)`
- [ ] aigw-server lib UT × 6
- [ ] BDD × 6 + MockUpstream 扩展（`POST /responses` + `POST /responses/stream`）
