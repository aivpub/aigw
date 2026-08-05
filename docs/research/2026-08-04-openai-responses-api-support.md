# OpenAI Responses API 接入调研报告

**日期**: 2026-08-04
**调研者**: Claude Code
**状态**: 完成

---

## 1. 背景

OpenAI 于 2025 年推出全新的 Responses API（`POST /v1/responses`），定位为 Chat Completions API 的继任者。与 Chat Completions API（`/v1/chat/completions`）不同，Responses API 采用统一输入模型，原生支持内置工具（web search、code interpreter、MCP tools）、多模态（音频/图片）、更细粒度的流式事件和服务器端会话管理。

当前 aigw 的支持矩阵：

| 端点 | 客户端协议 | 状态 |
|------|----------|------|
| `/v1/chat/completions` | OpenAI Chat | ✅ 已支持 |
| `/v1/models` | OpenAI Model List | ✅ 已支持 |
| `/v1/messages` | Anthropic Messages | ✅ 已支持 |
| `/v1/responses` | OpenAI Responses | ❌ 未支持 |

---

## 2. OpenAI Responses API 特性概述

### 2.1 端点

| 方法 | 路径 | 功能 |
|------|------|------|
| POST | `/v1/responses` | 创建响应 |
| GET | `/v1/responses/{response_id}` | 获取响应 |
| DELETE | `/v1/responses/{response_id}` | 删除响应 |
| POST | `/v1/responses/{response_id}/cancel` | 取消进行中的响应 |
| GET | `/v1/responses/{response_id}/input_items` | 列出输入项目 |

### 2.2 请求格式差异（vs Chat Completions）

| 特性 | Chat Completions | Responses API |
|------|-----------------|---------------|
| 消息格式 | `messages: [{role, content}]` | `input: [{role, content}]` 或 `input: "text"` |
| 模型参数 | `model` (顶层) | `model` (顶层) |
| 流式 | `stream: boolean` | `stream: boolean` |
| 工具定义 | `tools: [{type, function}]` | `tools: [{type: "function"/"web_search_preview"/"code_interpreter"/"mcp"...}]` |
| 推理强度 | 无 | `reasoning: {effort, summary}` |
| 多模态输入 | 仅图片 URL (content array) | 原生 audio/image/file 类型 |
| 会话 | stateless | `conversation` / `previous_response_id` 服务器端会话 |

### 2.3 响应格式差异

| 特性 | Chat Completions | Responses API |
|------|-----------------|---------------|
| 输出字段 | `choices: [{message, finish_reason}]` | `output: [{type, content, ...}]` |
| 用量 | `usage: {prompt_tokens, completion_tokens, total_tokens}` | `usage: {input_tokens, output_tokens, total_tokens}` |
| 状态 | 无 | `status: "completed"/"failed"/"in_progress"/...` |
| 工具调用 | `choices[0].message.tool_calls` | `output` 中 `type="function_call"` 项 |
| 流式事件 | `data: {"choices":[{"delta":{...}}]}` | 多种类型事件（见 2.5） |

### 2.4 Usage 字段映射

```
Chat Completions              →  Responses API
usage.prompt_tokens           →  usage.input_tokens
usage.completion_tokens       →  usage.output_tokens
usage.total_tokens            →  usage.total_tokens
prompt_tokens_details.cached  →  usage.input_tokens_details.cached_tokens
```

### 2.5 流式事件类型

Responses API 流式事件比 Chat Completions 的 SSE delta 更细粒度：

- **生命周期**: `response.created` → `response.in_progress` → `response.completed` / `response.failed`
- **文本**: `response.output_text.delta` / `response.output_text.done`
- **音频**: `response.output_audio.delta` / `response.output_audio.done` + transcript 事件
- **推理**: `response.reasoning_text.delta` / `response.reasoning_summary_text.delta`
- **工具调用**: `response.function_call_arguments.delta` / `response.code_interpreter_call_code.delta`
- **搜索**: `response.file_search_call.searching` / `response.web_search_call.searching`

---

## 3. litellm 的 Responses API 支持

litellm 对 OpenAI Responses API 的支撑是**成熟且功能齐全**的（稳定，非 beta）。需 litellm v1.63.8+。

### 3.1 litellm 暴露的端点

| 方法 | 路由 | 用途 |
|------|------|------|
| POST | `/v1/responses` | 创建 Response |
| GET | `/v1/responses/{response_id}` | 获取 Response |
| DELETE | `/v1/responses/{response_id}` | 删除 Response |
| POST | `/v1/responses/{response_id}/cancel` | 取消进行中的 Response |
| POST | `/v1/responses/compact` | 服务端对话压缩 |
| WS | `ws://host:40001/responses` | WebSocket 模式 |

### 3.2 双路径架构

litellm 根据目标 provider 采用两条完全不同的路径：

**路径 A — 原生 Responses API 转发**：对 OpenAI / Azure 等原生支持 `/v1/responses` 的 provider，请求直接以 OpenAI Responses API 格式转发，不做任何转换。

**路径 B — 桥接至 Chat Completions**：对缺少原生 Responses API 支持的 provider（Anthropic、Gemini、Bedrock、Vertex AI 等），litellm 自动将 `/v1/responses` 请求桥接至 `/v1/chat/completions` 管道。转换逻辑根据 `model_prices_and_context_window.json` 中预设的 `mode` 字段决定。

对于基于 `openai/` 前缀 + 自定义 `api_base` 的第三方 OpenAI 兼容 provider（llama.cpp、vLLM、LM Studio），可通过 `use_chat_completions_api: true` 或前缀 `openai/chat_completions/<model>` 强制走桥接。

**路径 C — 反向桥接（Chat-to-Responses）**：通过 `openai/responses/` 前缀或全局 `route_all_chat_openai_to_responses` 标志，将 `/chat/completions` 请求桥接至 Responses API（仅 `openai` provider）。

### 3.3 模型模式

每个模型在 litellm 的 `model_prices_and_context_window.json` 中有 `mode` 属性：

- **`mode: responses`**（自动使用 Responses API）：`o3-deep-research`、`o4-mini-deep-research`、`o1-pro`、`o3-pro`、`gpt-5.1-codex` 及变体、`codex-mini-latest`
- **`mode: chat`**（默认 Chat Completions，需前缀启用 Responses）：`gpt-4o`、`gpt-4.1`、`gpt-5`、`gpt-5-mini`、`o3`、`o4-mini`

### 3.4 litellm 特性矩阵

| 特性 | 状态 |
|------|------|
| 成本追踪 | ✅ |
| 日志记录 | ✅ |
| 终端追踪 | ✅ |
| 流式 | ✅ |
| WebSocket 模式 | ✅ |
| 故障转移 (Fallbacks) | ✅ |
| 负载均衡 (Loadbalancing) | ✅ |
| 护栏 (Guardrails) | ✅（非流式仅限输入输出文本）|

---

## 4. aigw 当前架构分析

### 4.1 路由注册（`crates/aigw-server/src/main.rs`）

```rust
let app = Router::new()
    .route("/v1/chat/completions", post(chat::chat_completions))
    .route("/v1/messages", post(v1_messages::messages_handler))
    // ...
```

### 4.2 处理器模板（`chat.rs`）

Chat completions handler 的执行流程：

1. **认证提取** — `ChatAuth` FromRequestParts: Bearer Token / Cookie JWT
2. **请求校验** — 验证 `model`、`messages` 字段，检测 `stream`
3. **Key 权限 & 预算检查** — 查找 key 的允许模型列表、检查预算
4. **上游解析** — `ModelResolver::resolve()` → `Vec<Deployment>`
5. **路由选择** — `Router::pick_deployment()` 从候选 deployment 中选择
6. **适配器选择** — `select_adapter(ClientProtocol, ProviderType)`
7. **请求适配** — `adapter.adapt_request(body, deployment)` 将客户端格式转换为上游格式
8. **上游 URL 构建** — 根据 provider_type 拼接路径（`chat/completions` 或 `messages`）
9. **HTTP 调用** — `reqwest_middleware::ClientWithMiddleware` + 重试
10. **响应处理**：
    - 流式：SSE passthrough → 两阶段 SpendLog（Phase1 占位 → Phase2 更新）
    - 非流式：解析 JSON → 计算 spend → 插入 SpendLog → 更新 entity spend → 记录指标 → 排队 daily_spend
11. **Spend 计算** — `calc_spend()` 三级缓存计费

### 4.3 适配器矩阵（`crates/aigw-core/src/adapter.rs`）

```
                 Provider
                 OpenAICompat  AnthropicNative
Client  OpenAI   Passthrough   OpenAI→Anthropic
        Anthropic Anthropic→OpenAI  Passthrough
```

`select_adapter()` 返回 `Option<&'static dyn MessageAdapter>`。

### 4.4 `MessageAdapter` trait

```rust
pub trait MessageAdapter: Send + Sync {
    fn client_protocol(&self) -> ClientProtocol;
    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError>;
    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError>;
    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>>;
}
```

### 4.5 上游 URL 路径决定

```rust
let upstream_path = match deployment.provider_type {
    ProviderType::AnthropicNative => "messages",
    _ => "chat/completions",
};
```

---

## 5. 实现方案

### 5.1 方案选型

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| A: 纯 Passthrough | 接受 `/v1/responses` 请求，不做协议转换，直接转发到支持 Responses API 的上游 | 实现简单，代码量少 | 只能对接支持 Responses API 的上游（如 OpenAI） |
| B: 协议转换 | 接受 `/v1/responses` 请求，转换回 `/v1/chat/completions` 格式发给不支持 Responses API 的上游 | 可以对接任意上游 | 实现复杂，协议差异大（input vs messages, output vs choices 等） |
| C: 混合模式（Passthrough + litellm-style） | 默认 Passthrough，如上游不支持 `/v1/responses` 则降级到方案 B 协议转换 | 体验最佳 | 实现最复杂 |

**推荐方案 A — 纯 Passthrough**，理由：
- 对齐 aigw 上游协议边界（只做 OpenAI 兼容 upstream 通用接入，见 charter NG9）
- `chat.rs` 中的 `OpenAIPassthrough` 已验证 Passthrough 模式可行
- 需要 `/v1/responses` 的用户通常连接的是 OpenAI 或其兼容服务（如 litellm）
- 方案 B/C 可作为后续 Phase 的增强

### 5.2 架构设计

```
Client (Responses API) → aigw /v1/responses
                              │
                              ├─ ChatAuth 认证
                              ├─ ModelResolver::resolve()
                              ├─ Router::pick_deployment()
                              ├─ select_adapter(ClientProtocol::OpenAI, ...) → OpenAIPassthrough
                              ├─ upstream URL: {api_base}/responses  ← 关键差异
                              ├─ HTTP 调用 (streaming/non-streaming)
                              ├─ Responses API response 解析（usage 字段映射）
                              └─ SpendLog 记录
```

与 `chat.rs` 的主要差异点：

| 组件 | `/v1/chat/completions` | `/v1/responses` |
|------|----------------------|-----------------|
| 请求字段校验 | `model`, `messages` | `model`, `input` |
| 上游路径 | `chat/completions` | `responses` |
| 适配器 | `OpenAIPassthrough` | `OpenAIPassthrough` |
| 响应 Usage 解析 | `usage.prompt_tokens` / `usage.completion_tokens` | `usage.input_tokens` / `usage.output_tokens` |
| 流式事件格式 | SSE `choices[0].delta` | SSE 多类型事件 |
| 错误格式 | OpenAI error | OpenAI error（同） |

### 5.3 代码变更清单

**新建文件**:
- `crates/aigw-server/src/routes/responses.rs` — 新的 handler 模块（~800 行，代码复刻自 `chat.rs` 但适配 Responses API）
- `docs/stages/stage-101.md` — Stage 101 设计文档

**修改文件**:
- `crates/aigw-server/src/routes/mod.rs` — 添加 `pub mod responses;`
- `crates/aigw-server/src/main.rs` — 注册 `/v1/responses` 路由
- `docs/stages/stage-roadmap.md` — 新增 Phase 41
- `docs/11-next-steps.md` — 更新下一步计划
- `docs/01-charter.md` — 更新端点覆盖状态

**不复用 chat.rs handler 的原因**:
1. `chat.rs` 已达 2905 行，进一步膨胀会降低可维护性
2. Responses API 的请求/响应格式与 Chat Completions 差异大（`input` vs `messages`, `output` vs `choices`, `usage.input_tokens` vs `usage.prompt_tokens`）
3. 独立 handler 可以独立测试、独立演进
4. 两个端点共享基础架构（认证、解析、路由、适配器）已经够多，handler 层分离开更清晰

### 5.4 共享代码策略

从 `chat.rs` 复用（通过 `pub(crate)` 或提取到共享模块）：
- `ChatAuth` — 认证提取器（已共享，定义在 `chat.rs`）
- `calc_spend()` — spend 计算（需要适配 Responses API usage 字段名）
- 错误响应格式 — OpenAI 标准错误 JSON
- SpendLog 构建逻辑 — 大部分字段填充相同

**Spend 计算的 Responses API 适配**:

```rust
// Chat Completions:
let prompt_tokens = usage["prompt_tokens"].as_u64();
let completion_tokens = usage["completion_tokens"].as_u64();

// Responses API:
let prompt_tokens = usage["input_tokens"].as_u64();
let completion_tokens = usage["output_tokens"].as_u64();
let total_tokens = usage["total_tokens"].as_u64();
```

### 5.5 流式事件处理

Responses API 流式事件采用 SSE 格式，但事件类型更多。Passthrough 模式下直接透传 SSE chunk，不需要解析事件类型。唯一需要从流中提取的是 **usage 信息**（通常在 `response.completed` 事件中），用于 SpendLog 的 Phase 2 更新。

与 `chat.rs` 相同的两阶段 SpendLog 模式：
- Phase 1: 请求开始时 INSERT 占位行（`status = 'streaming'`）
- Phase 2: 流结束/取消时 UPDATE（tokens + spend + status）

对于 Responses API，Phase 2 更新需要从流式事件的 `response.completed` 或最后的 `response.failed` 事件中提取 usage。

### 5.6 不支持的功能（显式排除）

| 功能 | 原因 |
|------|------|
| 协议转换（Responses → Chat Completions） | 后续 Phase，纯 Passthrough 先落地 |
| GET `/v1/responses/{id}` 取回 | 后续 Phase，当前优先核心创建端点 |
| DELETE `/v1/responses/{id}` | 后续 Phase |
| POST `/v1/responses/{id}/cancel` | 后续 Phase |
| 内置工具（web_search, code_interpreter） | 上游原生支持，网关不实现 |
| 会话管理（conversation） | 上游原生支持，网关不管理 |
| 多模态输入（audio, image, file） | 上游原生支持，网关不处理 |
| Anthropic Native upstream | 后续 Phase，需协议转换 |

---

## 6. 风险评估

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|---------|
| Usage 字段名不一致 | 中 | 中 | parse 时同时尝试 `input_tokens` / `prompt_tokens` 两套字段名 |
| 流式事件格式变化 | 低 | 中 | Passthrough 模式不解析事件，后续变更不影响 |
| 上游不支持 `/v1/responses` | 高 | 高 | 返回明确的错误信息，建议用户配置支持 Responses API 的上游 |
| chat.rs 与 responses.rs 代码重复 | 中 | 低 | 接受初期重复，后续提取共享逻辑到 `aigw-core` |

---

## 7. Stage 规划

单 Phase（Phase 41），单 Stage（Stage 101），预估 **8h**。

完整规划见 `docs/stages/stage-101.md`。

---

## 8. 参考资料

- [OpenAI Responses API Reference](https://platform.openai.com/docs/api-reference/responses)
- [litellm Responses API Support](https://docs.litellm.ai/docs/providers/openai#openai-v1responses--gpt-5-pro)
- [OpenAI Python SDK — Responses API](https://github.com/openai/openai-python/blob/main/api.md)
- aigw charter: `docs/01-charter.md`
- aigw adapter: `crates/aigw-core/src/adapter.rs`
- aigw chat handler: `crates/aigw-server/src/routes/chat.rs`
