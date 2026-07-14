# aigw 代理转发架构重构方案

**日期**: 2026-07-13（最后修订 2026-07-14）
**状态**: 规划中
**参考**: litellm 架构分析、现有代码审计

---

## 1. 现状 vs 目标

### 当前架构问题

```
GET /v1/models   ──→ models_list()
POST /v1/chat/completions ──→ chat_completions()
POST /v1/messages ──→ messages_handler()
  │
  ├── 各自独立 resolve upstream，逻辑重复
  ├── provider_registry (AppState) 定义了但从未使用
  ├── router_state (AppState) 定义了但从未使用  
  ├── DefaultAdapter 只有 text 转换，无 tool_use/tool_result
  ├── models.rs ClaudeContentBlock 无 tool_use/tool_result 变体
  └── 无 Deployment 抽象，无负载均衡、fallback
```

### 目标架构

```
                        ┌──────────────────────────┐
                        │   axum Router (main.rs)   │
                        │   POST /v1/chat/completions│
                        │   POST /v1/messages        │
                        │   GET  /v1/models          │
                        └───────────┬────────────────┘
                                    │
                        ┌───────────▼────────────────┐
                        │  Identity (已有)            │
                        │  ChatAuth / x-api-key /     │
                        │  JWT cookie                 │
                        └───────────┬────────────────┘
                                    │
                        ┌───────────▼────────────────┐
                        │  ModelResolver (新建)       │
                        │  统一模型解析入口            │
                        │  · model_name →             │
                        │    Vec<Deployment>          │
                        │  · 查 proxy_models          │
                        │  · 解密 litellm_params      │
                        │  · 解析 credential 引用      │
                        │  · 提取定价                  │
                        │  · fallback env vars         │
                        └───────────┬────────────────┘
                                    │
                        ┌───────────▼────────────────┐
                        │  MessageAdapter (重构)       │
                        │  按 (client_protocol,        │
                        │      provider_type)          │
                        │  选择对应 adapter 实现        │
                        │  · OpenAIPassthrough        │
                        │  · AnthropicToOpenAI         │
                        │    (含 tool 双向转换)         │
                        │  · (后续) OpenAIToAnthropic   │
                        └───────────┬────────────────┘
                                    │
                        ┌───────────▼────────────────┐
                        │  Upstream Call + Spend Log  │
                        │  (复用现有 HTTP/spend)        │
                        └────────────────────────────┘
```

---

## 2. 重构范围

### 不做的（本次范围外）

| 项目 | 原因 |
|------|------|
| litellm 式的 100+ provider 注册 | 当前只需 OpenAI + Anthropic 两个协议 |
| Router 负载均衡/fallback/cooldown | 代码已有（router.rs, provider.rs），后续 Phase 激活 |
| 厂商级参数适配（DeepSeek/Bedrock 特殊处理） | 非当前上游需求 |

### 要做的

| 优先级 | 模块 | 内容 |
|--------|------|------|
| **P0** | `ModelResolver` | 新建统一模型解析层，消除 chat.rs 和 v1_messages.rs 中的重复逻辑 |
| **P0** | `MessageAdapter` trait + tool 转换 | 重新设计双向消息格式转换 trait；`AnthropicToOpenAI` 完整实现 tool_use/tool_result ↔ tool_calls 转换 |
| **P1** | `Deployment` 抽象 | model_name → Vec<Deployment>，proxy_models 行解析后的纯值对象 |
| **P1** | Handler 瘦身 | chat.rs、v1_messages.rs 瘦身为薄层，通用逻辑下沉到 ModelResolver + MessageAdapter |

---

## 3. 详细设计

### 3.1 Deployment — 单条上游配置

```rust
// aigw-core/src/deployment.rs (新建)

/// 一条上游 Deployment — 一个 proxy_models 行解析后的产物。
/// ModelResolver::resolve() 返回 Vec<Deployment>，
/// 同 model_name 下可能有多个 entry（不同 api_base/key）。
#[derive(Debug, Clone)]
pub struct Deployment {
    /// 上游 API base URL
    pub api_base: String,
    /// 上游 API key（解密后的明文）
    pub api_key: Option<String>,
    /// 发送给上游的模型名（可能与 aigw 代理名不同）
    pub upstream_model: String,
    /// 上游类型 — 决定选哪个 MessageAdapter
    pub provider_type: ProviderType,
    /// USD per input token
    pub input_cost_per_token: Option<f64>,
    /// USD per output token
    pub output_cost_per_token: Option<f64>,
    /// 解密后的 litellm_params JSON（保留全部原始字段，
    /// 供 MessageAdapter 读取 provider 特定参数如 rpm/tpm/region 等）
    pub raw_params: Value,
}

/// 上游 Provider 类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderType {
    /// OpenAI 兼容 (OpenAI / DeepSeek / Ollama / vLLM / ...)
    OpenAICompatible,
    /// Anthropic Messages API 原生
    AnthropicNative,
}
```

**当前 Phase** handler 拿到 `Vec<Deployment>` 取 `[0]` 用。后续 Router Phase handler 遍历选。

### 3.2 ModelResolver — 模型→上游解析

```rust
// aigw-core/src/resolver.rs (新建)

pub struct ModelResolver {
    db: Database,
    aigw_master_key: Option<String>,
    deployment_mode: String,
}

impl ModelResolver {
    /// 解析 model_name 对应的所有 upstream Deployment。
    /// 替代 chat.rs 中的 resolve_upstream_params()。
    ///
    /// 返回 Vec 而非单个：同一 model_name 在 proxy_models
    /// 中可能存在多个 entry（不同的 api_base/key）。
    /// 当前 Phase handler 取 [0] 使用；Router Phase 阶段遍历选择。
    pub async fn resolve(
        &self,
        model_name: &str,
    ) -> Result<Vec<Deployment>, (StatusCode, Json<Value>)> {
        // 1. 查 proxy_models 表（model_name 可能匹配多行）
        // 2. 逐行解密 litellm_params
        // 3. 解析 credential 引用
        // 4. 提取定价
        // 5. 从 custom_llm_provider 推断 provider_type
        //    "anthropic" → AnthropicNative，其余 → OpenAICompatible
        // 6. Fallback 到 env vars（非 test 模式，model 不在表中时）
        // 7. 返回 Vec<Deployment>
    }
}
```

> `provider_type` 判断：从解密后的 `litellm_params.custom_llm_provider` 读取。
> `"anthropic"` → `AnthropicNative`，`"openai"`/`"deepseek"`/`"ollama"`/`"hosted_vllm"` 等 → `OpenAICompatible`。
> 没有 `custom_llm_provider` 字段时 fallback 到从 `api_base` 推断。

### 3.3 MessageAdapter trait — 消息格式双向转换

```rust
// aigw-core/src/adapter.rs (重构)

/// 客户端协议
pub enum ClientProtocol {
    OpenAI,      // /v1/chat/completions
    Anthropic,    // /v1/messages  
}

/// 消息格式转换器 — OpenAI Chat ↔ Anthropic Messages 双向
pub trait MessageAdapter: Send + Sync {
    /// 返回此 adapter 对应的客户端协议
    fn client_protocol(&self) -> ClientProtocol;

    /// 转换请求：客户端格式 → 上游格式
    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value>;

    /// 转换非流式响应：上游格式 → 客户端格式
    fn adapt_response(&self, body: Value) -> Result<Value>;

    /// 返回流式 chunk 转换器（SSE 逐块转换）
    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>>;
}

/// 流式 chunk 转换器 — 维护跨 chunk 状态（如 tool_use index 累计）
pub trait StreamAdapter: Send {
    /// 消费一个上游 SSE chunk，返回转换后的客户端 chunk（可能为 None）
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>>;
    /// 上游流结束，返回收尾 chunk（如 message_delta）
    fn finish(&mut self) -> Option<Vec<u8>>;
}
```

> `StreamAdapter` 需要 `&mut self` 而非 `&self`：流式 tool_use 转换需要累积跨 chunk 的 index 状态（Claude 的 `content_block_start` 带 index，OpenAI 的 `tool_calls[].index` 同理）。

### 3.4 AnthropicToOpenAI — tool 转换设计

**这是 Claude Code 兼容性的核心。** Claude Code 通过 `/v1/messages` 发送请求时会使用 tool_use，上游 OpenAI 返回 tool_calls。当前 DefaultAdapter 完全不处理这些 block。

#### 请求方向：Claude Messages → OpenAI Chat

| Claude 输入 | OpenAI 输出 |
|------------|------------|
| `role: "user"` + text block | `role: "user"`, `content: text` |
| `role: "user"` + tool_result block | `role: "tool"`, `tool_call_id`, `content` |
| `role: "assistant"` + text block | `role: "assistant"`, `content: text` |
| `role: "assistant"` + tool_use block | `role: "assistant"`, `content: null`, `tool_calls: [...]` |
| `system` (text/blocks) | `role: "system"` message（已有） |
| `tools` 数组 | `tools` 数组（透传，格式兼容） |

```rust
// models.rs — ClaudeContentBlock 新增变体
pub enum ClaudeContentBlock {
    Text { text: String },
    Image { source: ClaudeImageSource },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

// ChatMessage 新增字段
pub struct ChatMessage {
    pub role: String,
    pub content: ChatContent,
    pub name: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub tool_call_id: Option<String>,
}

pub struct ToolCall {
    pub id: String,
    pub r#type: String,        // "function"
    pub function: ToolCallFunction,
}

pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,     // JSON string
}
```

#### 响应方向（非流式）：OpenAI Chat → Claude Messages

| OpenAI 响应 | Claude 输出 |
|------------|------------|
| `choices[].message.content` (text) | `content: [{ type: "text", text }]` |
| `choices[].message.tool_calls[]` | `content: [{ type: "tool_use", id, name, input }]` |
| `choices[].finish_reason` | `stop_reason`（已有） |

#### 流式响应：OpenAI SSE chunk → Claude SSE event

| OpenAI chunk | Claude event |
|-------------|-------------|
| delta.role: "assistant" | `message_start` |
| delta.content (text) | `content_block_start` (text) + `content_block_delta` |
| delta.tool_calls[].function.name | `content_block_start` (tool_use) |
| delta.tool_calls[].function.arguments | `content_block_delta` (input_json_delta) |
| finish_reason | `message_delta` (stop_reason) |
| usage (最后一帧) | `message_delta` (usage) |

关键：`StreamAdapter` 需要维护跨 chunk 状态（当前是哪个 tool_use index，arguments 是否已开始），不能是简单的逐 chunk 1:1 映射。

### 3.5 MessageAdapter 选择逻辑

| (客户端协议, 上游类型) | 实现 | 行为 |
|------------------------|------|------|
| (OpenAI, OpenAICompatible) | `OpenAIPassthrough` | 透传（替换 model 字段） |
| (Anthropic, OpenAICompatible) | `AnthropicToOpenAI` | Messages ↔ Chat，含 tool 双向转换 |
| (Anthropic, AnthropicNative) | — | 暂不实现（返回 unsupported 错误） |
| (OpenAI, AnthropicNative) | — | 暂不实现（返回 unsupported 错误） |

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

### 3.6 Handler 瘦身

改造后的 handler 只做：

```rust
// 改造后 chat.rs 核心流程
pub async fn chat_completions(...) -> ... {
    // 1. 校验 & auth（不变）
    let body: Value = ...;
    let model = body["model"].as_str()...;

    // 2. 解析全部上游 Deployment
    let deployments = resolver.resolve(&model).await?;
    let deployment = &deployments[0]; // 当前取第一个，后续 Router 遍历选

    // 3. 选择 MessageAdapter
    let adapter = select_adapter(ClientProtocol::OpenAI, &deployment.provider_type)?;

    // 4. 转换请求 → upstream body
    let upstream_body = adapter.adapt_request(body, deployment)?;

    // 5. 拼 upstream URL，发请求
    let url = format!("{}/chat/completions", deployment.api_base.trim_end_matches('/'));
    let mut req = client.post(&url).json(&upstream_body);
    if let Some(ref key) = deployment.api_key {
        req = req.header("Authorization", format!("Bearer {}", key));
    }
    let resp = req.send().await?;

    // 6. 转换响应 + 记录 spend
    let final_response = adapter.adapt_response(resp)?;
    spend_log(deployment, &final_response).await;

    Ok(final_response)
}
```

v1_messages.rs 同理，ClientProtocol 为 Anthropic，upstream URL 为 `/v1/messages`。

---

## 4. 文件变更清单

### 新建文件

| 文件 | 内容 |
|------|------|
| `crates/aigw-core/src/deployment.rs` | `Deployment` struct + `ProviderType` enum |
| `crates/aigw-core/src/resolver.rs` | `ModelResolver` + `resolve() → Vec<Deployment>` |

### 修改文件

| 文件 | 变更 |
|------|------|
| `crates/aigw-core/src/lib.rs` | 新增 `pub mod deployment; pub mod resolver;` |
| `crates/aigw-core/src/adapter.rs` | trait 重写为 `MessageAdapter` + `StreamAdapter`；实现 `OpenAIPassthrough`、`AnthropicToOpenAI`（含 tool 双向转换）；`select_adapter()` |
| `crates/aigw-core/src/models.rs` | `ClaudeContentBlock` 新增 `tool_use`/`tool_result` 变体；`ChatMessage` 新增 `tool_calls`/`tool_call_id` 字段；新增 `ToolCall`/`ToolCallFunction` 类型 |
| `crates/aigw-server/src/routes/chat.rs` | 瘦身：用 `ModelResolver::resolve()` + `select_adapter()` 替代 `resolve_upstream_params()` |
| `crates/aigw-server/src/routes/v1_messages.rs` | 瘦身：用 `ModelResolver::resolve()` + `select_adapter()` 替代直接调 `DefaultAdapter` |
| `crates/aigw-server/src/routes/keys.rs` | `AppState` 增加 `resolver: ModelResolver` |
| `crates/aigw-server/src/main.rs` | 初始化 `ModelResolver` 注入 `AppState` |

---

## 5. 实施策略（TDD + BDD 驱动）

### Stage 50：ModelResolver + Deployment（4h）

**TDD 先行：**
1. UT: `Deployment` 构造与 Debug display
2. UT: `ModelResolver::resolve()` — 查表命中、解密 litellm_params、credential 引用解析、定价提取
3. UT: `resolve()` — 模型不在表中时 env var fallback
4. UT: `resolve()` — 加密字段解密失败时错误类型

**实现（RED→GREEN→REFACTOR）：**
5. 新建 `deployment.rs` — `ProviderType` + `Deployment`
6. 新建 `resolver.rs` — `ModelResolver::resolve()`，迁移 `chat.rs` 中 `resolve_upstream_params` 逻辑
7. 替换 `chat.rs` 中对 `resolve_upstream_params` 的调用为 `resolver.resolve()`

**门禁：**
8. 全量 UT + BDD 回归通过（92 scenarios）

### Stage 51：MessageAdapter + tool 转换（5h）

**TDD 先行：**
1. UT: `OpenAIPassthrough` — 请求透传（model 替换）、响应透传
2. UT: `AnthropicToOpenAI.adapt_request` — 纯文本消息、system message
3. UT: `AnthropicToOpenAI.adapt_request` — tool_use → tool_calls
4. UT: `AnthropicToOpenAI.adapt_request` — tool_result → tool role message
5. UT: `AnthropicToOpenAI.adapt_response` — content + tool_calls → ClaudeContentBlock[]
6. UT: `AnthropicToOpenAI` stream — text chunk 转换、tool_use chunk 转换（content_block_start + delta）、usage 收尾
7. UT: `select_adapter()` — 4 种 (client, provider) 组合的选择结果
8. BDD: `/v1/messages` 含 tool_use 的请求 → OpenAI mock upstream 返回 tool_calls → Claude Code 格式响应

**实现（RED→GREEN→REFACTOR）：**
9. `models.rs` 新增 `ClaudeContentBlock::ToolUse`/`::ToolResult`、`ChatMessage.tool_calls`/`tool_call_id`、`ToolCall`/`ToolCallFunction`
10. 重构 `adapter.rs` — 定义 `MessageAdapter` trait + `StreamAdapter` trait
11. 实现 `OpenAIPassthrough`
12. 实现 `AnthropicToOpenAI`（迁移 DefaultAdapter + 新增 tool 转换）
13. `v1_messages.rs` 使用 `select_adapter()`

**门禁：**
14. 全量 UT + BDD 回归通过 + 新增 tool BDD scenario 通过

### Stage 52：Handler 瘦身（3h）

**TDD 先行：**
1. BDD: chat_completions 端点行为对比（重构前后输出一致）
2. BDD: /v1/messages 端点行为对比（重构前后输出一致）

**实现（RED→GREEN→REFACTOR）：**
3. `chat.rs` — upstream call + spend logging 提取公共逻辑
4. `v1_messages.rs` — 同步瘦身
5. 清理注释和死代码

**门禁：**
6. 全量 UT + BDD 回归通过（316 UT + 92 BDD scenarios + 前端 108 tests）

---

## 6. 风险与对策

| 风险 | 对策 |
|------|------|
| 重构导致 BDD 测试回归 | 每阶段跑全量测试，Stage 50 先合并再继续 |
| `provider_type` 判断不准确 | 主源：`litellm_params.custom_llm_provider`；fallback：api_base 推断。DeepSeek/Ollama/vLLM 等 OpenAI family 统一为 `OpenAICompatible`，参数差异由长期 `OpenAIFamilyAdapter` trait 子类覆盖处理 |
| MessageAdapter 过度设计 | 当前值实现实际需要的 2 个 adapter（Passthrough + AnthropicToOpenAI），其余返回 unsupported |
| tool_use 流式转换复杂（跨 chunk 状态） | `StreamAdapter` 使用 `&mut self` 维护累积状态；逐事件类型编写 UT 覆盖每种 SSE chunk 组合 |
| Anthropic tool_use.input 是 JSON object，OpenAI tool_calls.function.arguments 是 JSON string | 序列化/反序列化方向明确：input → JSON string（请求方向），JSON string → input（响应方向） |

---

## 7. 长期扩展：OpenAI Family 子厂商参数适配

### 背景

存量数据库中 `custom_llm_provider` 值包括 `openai`、`deepseek`、`ollama`、`hosted_vllm` 等。这些厂商 API 格式与 OpenAI 兼容（`/v1/chat/completions`），但存在参数差异：

| 厂商 | 差异 |
|------|------|
| DeepSeek | 不支持 `frequency_penalty`/`presence_penalty` |
| Ollama | 需要 `num_ctx`，不支持 `logprobs` |
| vLLM | 支持 `repetition_penalty`，部分参数名不同 |

当前 Phase 17 所有 OpenAI 兼容厂商统一使用 `OpenAIPassthrough`（透传全部参数），差异不影响正常工作。当用户反馈实际参数报错时触发此扩展。

### 设计：Trait 继承 + 子类覆盖

```rust
/// OpenAI Family 共享的 MessageAdapter 扩展 —
/// 子厂商只 override 参数差异，消息格式转换逻辑完全继承
trait OpenAIFamilyAdapter: MessageAdapter {
    /// 过滤/改写请求参数，默认透传
    fn filter_params(&self, mut body: Value, _deployment: &Deployment) -> Value {
        body
    }

    /// 过滤/改写响应参数，默认透传
    fn filter_response(&self, body: Value) -> Value {
        body
    }
}

// 实现示例
struct DeepSeekAdapter;
impl MessageAdapter for DeepSeekAdapter { /* = OpenAIPassthrough */ }
impl OpenAIFamilyAdapter for DeepSeekAdapter {
    fn filter_params(&self, mut body: Value, _d: &Deployment) -> Value {
        body.as_object_mut().map(|o| {
            o.remove("frequency_penalty");
            o.remove("presence_penalty");
        });
        body
    }
}

struct OllamaAdapter;
impl MessageAdapter for OllamaAdapter { /* = OpenAIPassthrough */ }
impl OpenAIFamilyAdapter for OllamaAdapter {
    fn filter_params(&self, mut body: Value, d: &Deployment) -> Value {
        body.as_object_mut().map(|o| {
            o.remove("logprobs");
            o.remove("logit_bias");
            if let Some(num_ctx) = d.raw_params.get("num_ctx") {
                o.insert("num_ctx".to_string(), num_ctx.clone());
            }
        });
        body
    }
}
```

### select_adapter 扩展（未来）

```rust
fn select_adapter(client: ClientProtocol, deployment: &Deployment)
    -> Option<Box<dyn MessageAdapter>>
{
    match (client, &deployment.provider_type) {
        (ClientProtocol::OpenAI, ProviderType::OpenAICompatible) => {
            // 根据 raw_params.custom_llm_provider 派生子 adapter
            let provider = deployment.raw_params
                .get("custom_llm_provider")
                .and_then(|v| v.as_str())
                .unwrap_or("openai");
            match provider {
                "deepseek" => Some(Box::new(DeepSeekAdapter)),
                "ollama" => Some(Box::new(OllamaAdapter)),
                _ => Some(Box::new(OpenAIPassthrough)),
            }
        }
        // ...
    }
}
```

**触发条件**: 用户反馈具体厂商参数报错时启动。P3 长期。
