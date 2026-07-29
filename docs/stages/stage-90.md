# Stage 90: Upstream Prompt Cache Token Extraction & 三级差异化计费

**Phase**: 36 — Upstream Prompt Cache Detection & Differentiated Billing
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: 无（独立改动，chat.rs / v1_messages.rs / deployment.rs / calc_spend）

---

## 核心预期

1. **上游缓存 token 解析**：从 provider 返回的 `response->usage` 中提取 `cache_read_input_tokens`（Anthropic 顶层）和 `prompt_tokens_details.cached_tokens`（OpenAI 兼容 `prompt_tokens_details`），以及对应的 `cache_creation_input_tokens` / `cache_write_tokens`，归一化后传入计费逻辑。

2. **三级差异化计费**：`calc_spend` 不再用单一 `input_cost_per_token`，改为拆分为三档：
   - `regular_prompt = prompt_tokens - cache_read - cache_creation`
   - `spend = regular * input_cost + cache_read * cache_read_cost + cache_creation * cache_creation_cost + completion * output_cost`
   - **fallback**：当 `cache_read_cost` 或 `cache_creation_cost` 不存在时，回退到 `input_cost`（不溢出、不欠费）。

3. **Deployment 定价字段扩展**：`Deployment` 增加 `cache_read_input_token_cost: Option<f64>` 和 `cache_creation_input_token_cost: Option<f64>`，从 `model_info` 或 `litellm_params` 提取。

4. **Anthropic 归一化**：Anthropic 返回的 `input_tokens` 不含缓存 token，aigw 在调用 `calc_spend` 前将 `prompt_tokens += cache_read + cache_creation`。

---

## 背景

litellm 源码调研确认（`docs/research/2026-07-28-upstream-prompt-cache-detection-and-billing.md`）：

- 上游 provider 缓存命中的信息**不通过 `cache_hit` 字段表达**（`cache_hit='True'` 仅代表 litellm 自身 Redis 缓存拦截）
- 上游缓存 token 存放在 `response->usage` 中，两套格式：
  - **Anthropic**：`usage.cache_read_input_tokens`、`usage.cache_creation_input_tokens`（顶层）
  - **OpenAI 兼容**：`usage.prompt_tokens_details.cached_tokens`、`usage.prompt_tokens_details.cache_write_tokens`
- litellm 对缓存 token 使用**三级差异化价格**（`cache_read_input_token_cost` 通常为 `input_cost` 的 10%-50%；`cache_creation_input_token_cost` 约为 125%）
- 当前 aigw `calc_spend` 对所有 prompt token 用同一价格，存在**缓存 token 计费失真**风险

---

## 设计

### ① Deployment 定价字段扩展（`crates/aigw-core/src/deployment.rs`）

```rust
pub struct Deployment {
    // ... existing fields ...
    /// USD per cache-read input token（从缓存读入的 token 价格，通常是 input_cost 的 10%-50%）
    pub cache_read_input_token_cost: Option<f64>,
    /// USD per cache-creation input token（新写入缓存的 token 价格，通常比 input_cost 贵 ~25%）
    pub cache_creation_input_token_cost: Option<f64>,
}
```

### ② 定价提取扩展（`crates/aigw-server/src/routes/chat.rs`）

`extract_pricing()` 增加两个缓存价格字段的提取（从 `model_info` → `litellm_params` 两级 fallback）：

```rust
fn extract_pricing(model_info: &Value, params_json: &Value) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let input = model_info.get("input_cost_per_token")...
    let output = model_info.get("output_cost_per_token")...
    // 新增
    let cache_read = model_info.get("cache_read_input_token_cost")
        .or_else(|| params_json.get("cache_read_input_token_cost"));
    let cache_create = model_info.get("cache_creation_input_token_cost")
        .or_else(|| params_json.get("cache_creation_input_token_cost"));
    (input, output, cache_read, cache_create)
}
```

`ResolvedUpstream` 同步增加两个字段。`ModelResolver::resolve()` 调用点同步提取。

### ③ calc_spend 重构（`crates/aigw-server/src/routes/chat.rs`）

旧签名：
```rust
fn calc_spend(prompt_tokens: i32, completion_tokens: i32, input_cost: Option<f64>, output_cost: Option<f64>) -> f64
```

新签名：
```rust
fn calc_spend(
    prompt_tokens: i32,
    completion_tokens: i32,
    input_cost: Option<f64>,
    output_cost: Option<f64>,
    cache_read_tokens: i32,               // 新增
    cache_creation_tokens: i32,            // 新增
    cache_read_cost: Option<f64>,          // 新增
    cache_creation_cost: Option<f64>,      // 新增
) -> f64
```

**三级计费公式**：
```rust
let regular = 0.max(prompt_tokens - cache_read_tokens - cache_creation_tokens) as f64;

// fallback: 缓存价格缺失时回退到常规 input_cost
let read_cost = cache_read_cost.unwrap_or(input_cost.unwrap_or(0.0));
let create_cost = cache_creation_cost.unwrap_or(input_cost.unwrap_or(0.0));
let base_input = input_cost.unwrap_or(0.0);

regular * base_input
    + cache_read_tokens as f64 * read_cost
    + cache_creation_tokens as f64 * create_cost
    + completion_tokens as f64 * output_cost.unwrap_or(0.0)
```

### ④ 上游 response 缓存 token 解析

#### 4.1 流式路径（`chat.rs` streaming handler）

在 `while let Some(chunk_result) = stream.next().await` 的 `usage` 提取分支中（~L1265），增加缓存 token 提取：

```rust
if let Some(usage) = val.get("usage") {
    stream_prompt_tokens = ...;
    stream_completion_tokens = ...;
    stream_total_tokens = ...;
    // 新增：提取缓存 token
    stream_cache_read = extract_cache_read_tokens(usage);
    stream_cache_creation = extract_cache_creation_tokens(usage);
}
```

在调用 `calc_spend` 时传入（L1302）。

#### 4.2 非流式路径（`chat.rs` 同步 handler + `v1_messages.rs`）

从 `resp_body["usage"]` 提取缓存 token，传入 `calc_spend`。共用同一个提取辅助函数：

```rust
/// 从 usage JSON 提取 cache_read tokens（兼容 Anthropic + OpenAI 两套格式）
fn extract_cache_read_tokens(usage: &Value) -> i32 {
    // Anthropic: usage.cache_read_input_tokens
    if let Some(v) = usage.get("cache_read_input_tokens").and_then(|v| v.as_i64()) {
        return v as i32;
    }
    // OpenAI: usage.prompt_tokens_details.cached_tokens
    usage.get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

/// 从 usage JSON 提取 cache_creation tokens
fn extract_cache_creation_tokens(usage: &Value) -> i32 {
    // Anthropic: usage.cache_creation_input_tokens
    if let Some(v) = usage.get("cache_creation_input_tokens").and_then(|v| v.as_i64()) {
        return v as i32;
    }
    // OpenAI: usage.prompt_tokens_details.cache_write_tokens 或 cache_creation_tokens
    let details = usage.get("prompt_tokens_details");
    details.and_then(|d| d.get("cache_write_tokens"))
        .or_else(|| details.and_then(|d| d.get("cache_creation_tokens")))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}
```

### ⑤ Anthropic 归一化

Anthropic 返回的 `input_tokens`（映射为 `prompt_tokens`）不含缓存 token。需要在传入 `calc_spend` 前归一化：

```rust
let effective_prompt = if deployment.provider_type.is_anthropic_style() {
    prompt_tokens + cache_read + cache_creation
} else {
    prompt_tokens
};
```

判断 `is_anthropic_style()`：检查 `custom_llm_provider == "anthropic"` 或 `usage` 中 Anthropic 特征字段存在。

> **注意**：aigw 当前极少使用 Anthropic 原生上游（仅 2 条 `cache_read_input_tokens`），但归一化逻辑应内置以备后续。

### ⑥ 调用点变化汇总

| 文件 | 位置 | 改动 |
|------|------|------|
| `chat.rs` 流式 handler | L1265-1278 | usage 提取加缓存 token（保存到局部变量） |
| `chat.rs` 流式 handler | L1302 | `calc_spend(...)` 加 4 个参数 |
| `chat.rs` 非流式 | L1554-1562 | resp_body usage 提取 + `calc_spend` 加参数 |
| `v1_messages.rs` 流式 | ~L837 | usage 提取加缓存 token |
| `v1_messages.rs` 非流式 | ~L1013-1024 | resp_body usage 提取 + `calc_spend` |
| `v1_messages.rs` 失败路径 | ~L680+ | 仅传 0 缓存 token（失败无 usage） |
| `chat.rs` 超时路径 | ~L567 | 仅传 0 缓存 token |

### ⑦ 每日聚合表写入（本 Stage 仅铺数据，不做表修改）

`DailySpendLog` struct（`models.rs:163-181`）和 `DailySpendKind` enum 需要增加 `cache_read_input_tokens` 和 `cache_creation_input_tokens` 字段。daily_spend_queue.rs 的 INSERT/UPDATE SQL 需要写入这两个字段。

> **注意**：`daily_*_spend` 表已经在 015 迁移中定义了 `cache_read_input_tokens BIGINT` 和 `cache_creation_input_tokens BIGINT` 列，但 `DailySpendLog` struct 目前不含这些字段。本 Stage 补全 Rust struct 并写入实际值。

---

## 不做（边界）

- ❌ **不修改** `spend_logs` 表结构（`cache_hit`/`cache_key` 列保持不变）
- ❌ **不写入** `metadata.additional_usage_values`（与 litellm 不同，aigw 不做此兼容层——数据已在 `response` JSONB 中）
- ❌ **不新增** `spend_logs.cache_read_input_tokens` / `cache_creation_input_tokens` 列（复杂度收益比不划算，daily 表已覆盖汇总）
- ❌ **不实现** litellm 自身 Redis 缓存（`cache_hit='True'` 路径），属于 LT-Redis 范畴
- ❌ **不解析** DeepSeek 的 `prompt_cache_hit_tokens` 字段（DeepSeek SDK 内部已映射为 `cached_tokens`，由 SDK 层归一化，aigw 无需额外处理）

---

## 测试策略

### UT（`crates/aigw-core/tests/` + `crates/aigw-server/src/routes/`）

| # | 测试 | 覆盖 |
|---|------|------|
| 1 | `test_calc_spend_no_cache_no_pricing` | 缓存 token=0，无定价 → 返回 0 |
| 2 | `test_calc_spend_with_cache_read_openai` | `prompt=600, cache_read=500, cache_create=0` → 常规 100×ic + 500×crc |
| 3 | `test_calc_spend_with_cache_read_anthropic` | `prompt=100, cache_read=500, is_anthropic=true` → norm 600 → 常规 100×ic + 500×crc |
| 4 | `test_calc_spend_cache_cost_fallback` | 缓存价格 None → 回退到 `input_cost` |
| 5 | `test_calc_spend_with_cache_create` | 含 `cache_creation_tokens` → 使用 `cache_creation_cost` |
 6 | `test_extract_cache_read_openai_format` | `prompt_tokens_details.cached_tokens = 500` → 返回 500 |
| 7 | `test_extract_cache_read_anthropic_format` | `cache_read_input_tokens = 500` → 返回 500 |
| 8 | `test_extract_cache_creation_openai_write_tokens` | `prompt_tokens_details.cache_write_tokens = 50` → 返回 50 |
| 9 | `test_extract_cache_creation_anthropic_format` | `cache_creation_input_tokens = 50` → 返回 50 |
| 10 | `test_extract_cache_read_none` | 无缓存字段 → 返回 0 |

### BDD（real BDD — 三后端真实数据库）

| # | 场景 | 覆盖 |
|---|------|------|
| 1 | 流式 chat 含 cached_tokens → spend 按缓存价计算 | chat.rs streaming |
| 2 | 非流式 chat 含 cached_tokens → spend 按缓存价计算 | chat.rs non-streaming |

> BDD 通过 mock upstream 返回含 `prompt_tokens_details.cached_tokens` 的 response 来验证。

### 门禁

- `cargo test -p aigw-core` 全量通过
- `cargo test -p aigw-server` 全量通过
- mock BDD 场景不减（新增 2 场景）
- real BDD sqlite/pg/mysql 三后端通过

---

## 关键决策

1. **fallback 用 input_cost 而非 0**：缓存定价字段缺失时不溢出不欠费，对齐 litellm `_cost_per_token_custom_pricing_helper` 逻辑（`cache_read_cost = custom_cost_per_token.get("cache_read_input_token_cost", input_cost_per_token)`）。

2. **Anthropic 归一化在调用侧做，不在 calc_spend 内部**：`calc_spend` 保持纯粹——参数含义明确（传入的 `prompt_tokens` 是已归一的），不做 provider-type 分支判断。

3. **不在 spend_logs 加缓存 token 列**：litellm 也没有这些列（`LiteLLM_SpendLogs` 只有 `cache_hit`/`cache_key`），缓存 token 在 `response` JSONB 和 `daily_*_spend` 表中。加列需 migration + 前端适配，收益有限。

4. **daily_spend 列已在 015 migration 就绪**：只需补 Rust struct 和写入逻辑，无需新 migration。

5. **`v1_messages.rs` 错误/超时路径不传缓存 token**：失败时无有效 usage object，`cache_read`/`cache_creation` 传 0。spend 已为 0 或部分信息不全，缓存 token 无法提取也无实际影响。

---

## 设计文档

- 调研文档：`docs/research/2026-07-28-upstream-prompt-cache-detection-and-billing.md`
- 路线图：`docs/stages/stage-roadmap.md`（Phase 36）
