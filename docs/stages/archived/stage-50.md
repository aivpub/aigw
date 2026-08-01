# Stage 50: ModelResolver + Deployment

**Phase**: 17 — 代理转发架构重构（P1）
**状态**: ⏳ 待开始
**预估**: 4h
**依赖**: 无

---

## 目标

新建 `ModelResolver` 组件，统一 chat.rs 和 v1_messages.rs 中的模型→上游解析逻辑，消除 ~230 行重复代码。

1. 新建 `Deployment` 值对象 — 一条 proxy_models 行解析后的纯数据载体（含解密后的 raw_params）
2. 新建 `ModelResolver` — `resolve(model_name) → Vec<Deployment>`，迁移现有 `resolve_upstream_params` 逻辑
3. 替换 chat.rs 调用点，行为不变

## 验收标准

- [ ] `Deployment` struct 定义于 `crates/aigw-core/src/deployment.rs`，含全部 7 个字段（含 `raw_params: Value`）
- [ ] `ProviderType` enum 定义于同文件，含 `OpenAICompatible` 和 `AnthropicNative` 两个变体
- [ ] `ModelResolver` 定义于 `crates/aigw-core/src/resolver.rs`，含 `resolve()` 方法
- [ ] `resolve()` 返回 `Result<Vec<Deployment>, (StatusCode, Json<Value>)>`，同一 model_name 可匹配多行
- [ ] `resolve()` 内部逻辑：查 proxy_models → 解密 litellm_params → 解析 credential 引用 → 提取定价 → 从 `custom_llm_provider` 推断 `provider_type` → env var fallback（注：`raw_params` 保存解密后的完整 litellm_params）
- [ ] chat.rs 中 `chat_completions()` 调用 `resolver.resolve()` 替代 `resolve_upstream_params()`
- [ ] **TDD**:
  - UT: Deployment 构造（含 raw_params）
  - UT: resolve 查表命中
  - UT: resolve 加密字段解密
  - UT: resolve credential 引用解析
  - UT: resolve 定价提取（model_info 主源 + litellm_params 回退）
  - UT: resolve provider_type 推断（custom_llm_provider 主源 + api_base fallback）
  - UT: resolve 模型不在表中时 env var fallback
  - UT: resolve 解密失败时错误类型
- [ ] **门禁**: 全量 UT (316+) + BDD (92 scenarios) 回归通过
  - **不新增 BDD scenario**：Stage 50 是纯内部重构，HTTP 端点行为不变。现有 BDD 全量回归即安全网。

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/deployment.rs` | **新建** — `Deployment` struct + `ProviderType` enum |
| `crates/aigw-core/src/resolver.rs` | **新建** — `ModelResolver` + `resolve()` |
| `crates/aigw-core/src/lib.rs` | 修改 — 新增 `pub mod deployment; pub mod resolver;` |
| `crates/aigw-server/src/routes/chat.rs` | 修改 — `resolve_upstream_params()` → `resolver.resolve()` |
| `crates/aigw-server/src/routes/keys.rs` | 修改 — `AppState` 增加 `resolver: ModelResolver` |
| `crates/aigw-server/src/main.rs` | 修改 — 初始化 `ModelResolver` 注入 `AppState` |

## 技术方案

### A. Deployment

```rust
// crates/aigw-core/src/deployment.rs

#[derive(Debug, Clone)]
pub struct Deployment {
    pub api_base: String,
    pub api_key: Option<String>,
    pub upstream_model: String,
    pub provider_type: ProviderType,
    pub input_cost_per_token: Option<f64>,
    pub output_cost_per_token: Option<f64>,
    /// 解密后的 litellm_params JSON（保留全部原始字段，
    /// 供 MessageAdapter 读取 provider 特定参数）
    pub raw_params: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderType {
    OpenAICompatible,
    AnthropicNative,
}
```

### B. ModelResolver::resolve() 核心流程

```
resolve(model_name) → Vec<Deployment>

  1. db.list_models_by_name(model_name) → Vec<ProxyModel>

  2. for each ProxyModel:
     - 判断 litellm_params 是 plaintext JSON 还是加密密文
     - 如需解密 → aigw_master_key.decrypt → parse JSON
     - decrypt_json_fields(嵌套加密字段，如 api_key)
     - 提取 model_info 中的定价（input/output_cost_per_token）
       fallback: litellm_params 中的定价
     - 检查 litellm_credential_name → 查询 credentials 表 → 合并
     - 推断 provider_type:
       主源: 解密后的 litellm_params.custom_llm_provider
         "anthropic" → AnthropicNative
         "openai"/"deepseek"/"ollama"/"hosted_vllm"/... → OpenAICompatible
       Fallback: api_base 含 "anthropic" → AnthropicNative
     - 构造 Deployment { api_base, api_key, upstream_model,
                          raw_params: params_json, ... }

  3. 如果 Vec 为空 + deployment_mode != "test" → env var fallback
     返回单个 Deployment { api_base=env("OPENAI_BASE_URL"),
                           api_key=env("OPENAI_API_KEY"),
                           provider_type=OpenAICompatible, ... }
```

### C. chat.rs 调用点迁移

```rust
// 旧:
let resolved = resolve_upstream_params(&state, _model).await?;
let upstream_url = format!("{}/chat/completions", resolved.api_base...);
let upstream_body = ...; // model 替换为 resolved.model_name

// 新:
let deployments = state.resolver.resolve(_model).await?;
let deployment = &deployments[0];
let upstream_url = format!("{}/chat/completions", deployment.api_base...);
let upstream_body = ...; // model 替换为 deployment.upstream_model
```

> 当前只取 `deployments[0]`。后续 Router Phase 阶段遍历选。

## 风险

- `resolve_upstream_params` 是 ~230 行的复杂函数，迁移时需逐段对齐，不能遗漏任何分支
- credential 引用合并逻辑容易有字段覆盖顺序的 bug（litellm_params 字段 vs credential_values 字段的优先级）
- 因为只做代码搬家不改变行为，BDD 回归是最好的安全网
- `provider_type` 主源 `custom_llm_provider` 字段在已加密的 litellm_params 中，必须先解密才能读取；加密失败时 fallback 到 api_base 推断
