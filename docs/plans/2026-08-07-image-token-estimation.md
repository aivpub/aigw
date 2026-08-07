# Image Token Estimation for Multimodal LLMs — 总体规划

**日期**: 2026-08-07
**状态**: Draft — 规划阶段，待审批
**作者**: Claude Code Agent (multi-subagent research)

---

## 1. 需求背景

### 1.1 问题描述

aigw 当前对多模态模型（如 qwen3.5-vl、gpt-4o 等）的 token 计费仅依赖上游 provider 返回的 `usage.prompt_tokens` 总数。但**通过 OpenAI-compatible 协议连接时，几乎所有 provider 都不返回 image token 分解**：

- **Qwen OpenAI-compat 模式** — ❌ 仅 `prompt_tokens` / `completion_tokens` / `total_tokens`，无 `image_tokens`
- **Qwen DashScope 原生模式** — ✅ 返回 `usage.image_tokens`（但 aigw 通过 OpenAI-compat 连接，不走原生协议）
- **OpenAI (GPT-4V/4o)** — ❌ `prompt_tokens_details` 不含 `image_tokens`
- **Anthropic (Claude)** — ❌ 图片合并在 `input_tokens`
- **DeepSeek-VL** — ❌ `usage` 仅 `prompt_tokens` / `completion_tokens` / `total_tokens`，无 `image_tokens`
- **智谱 GLM-4V** — ❌ 未返回 image token 分解
- **Google Gemini** — ✅ `promptTokensDetails[]` 按 modality 分解（唯一例外，但 aigw 当前无 Gemini provider）

**结论**：在 aigw 的实际部署场景中（OpenAI-compatible 协议），**所有主流 provider 都不返回 image token 分解**。客户端估算是唯一可行的路径。

**阿里云官方也推荐客户端估算**（[文档](https://help.aliyun.com/zh/model-studio/vision)）：
> "可通过以下代码计算图像或视频的 Token 消耗。估算结果仅供参考，实际用量以 API 响应为准。"

这导致两个问题：
1. **数据缺失**：SpendLog 中无法区分文本与图片的 token 消耗。
2. **成本归因缺失**：运营无法评估多模态调用的图片 token 占比。

### 1.2 image tokens 计费策略

**核心结论：image_tokens 不改计费公式。** `image_tokens` 是 `prompt_tokens` 的子集：

```
prompt_tokens = text_tokens + image_tokens
spend = prompt_tokens × input_cost_per_token + completion_tokens × output_cost_per_token
```

image_tokens 的价值是**分析与对账**，不是独立计费：
- 运营了解图片 token 占比
- 估算值与上游 `prompt_tokens` 交叉验证
- 按 Stage 90 cache_tokens 模式（cache_tokens 有独立定价字段是因为缓存读写单价不同；image_tokens 不从 prompt_tokens 中拆出单独计费）

### 1.3 业界标准对比

| Provider | 协议 | 返回 image tokens? | 计算公式 |
|----------|------|-------------------|---------|
| **Qwen (OpenAI-compat)** | `/v1/chat/completions` | ❌ | ViT: `(H/factor) × (W/factor)` |
| **Qwen (DashScope native)** | DashScope SDK | ✅ `usage.image_tokens` | — |
| **OpenAI** | `/v1/chat/completions` | ❌ | `85 + 170 × tiles` |
| **Anthropic** | `/v1/messages` | ❌（公式公开） | `⌈w/28⌉ × ⌈h/28⌉` visual tokens |
| **DeepSeek** | `/v1/chat/completions` | ❌ | 未知 |
| **智谱 GLM-4V** | `/v1/chat/completions` | ❌ | 未知 |
| **Google Gemini** | Gemini API | ✅ `promptTokensDetails[]` | per-modality |

**业界实践**：经过对 litellm、OpenRouter、OneAPI 源码和文档的实际调研：

| Gateway | 是否做客户端预计算？ | 证据 |
|---------|-------------------|------|
| **litellm** | ❌ **不做** | [源码](https://github.com/BerriAI/litellm) `llm_cost_calc/utils.py`：`image_tokens = getattr(usage.prompt_tokens_details, 'image_tokens', 0) or 0` — 无 fallback 估算 |
| **OpenRouter** | ❌ **不做** | [API 文档](https://openrouter.ai/docs/api-reference/overview) `prompt_tokens_details` 不含 input image token 分解 |
| **OneAPI** | ❌ **不做** | [README](https://github.com/songquanpeng/one-api) 计费公式仅文本 token |

**结论**：现有主流开源网关**没有一家**实现客户端 image token 预计算。aigw 如果要做，将是**行业领先**的差异化功能。

---

## 2. 技术调研

### 2.1 Qwen2.5-VL Vision Encoder

| 参数 | 值 |
|------|-----|
| Vision encoder type | ViT (Vision Transformer) + window attention + SwiGLU + RMSNorm |
| patch_size | 14 |
| spatial_merge_size | 2 |
| Effective factor (pixels per token) | 14 × 2 = **28** |
| `min_pixels` (default) | 3,136 (= 4 tokens, `4×28²`) |
| `max_pixels` (default) | 12,845,056 (= 16,384 tokens, `16384×28²`) |
| Resize method | Dynamic resolution — 保持宽高比，resize 到 min/max_pixels 范围内 |
| Token formula | `image_tokens = ceil(H/28) × ceil(W/28)` |

**示例**：
- 560×420 图片 → `(560/28) × (420/28) = 20 × 15 = 300 tokens`
- 1024×1024 图片 → `ceil(1024/28) × ceil(1024/28) ≈ 37 × 37 = 1,369 tokens`
- 2048×1536 图片 → `ceil(2048/28) × ceil(1536/28) ≈ 74 × 55 = 4,070 tokens`

### 2.2 Qwen3-VL Vision Encoder

| 参数 | 值 |
|------|-----|
| patch_size | 16 |
| spatial_merge_size | 2 |
| Effective factor (pixels per token) | 16 × 2 = **32** |
| `min_pixels` (default) | 65,536 (= 64 tokens, `64×32²`) |
| `max_pixels` (default) | 16,777,216 (= 16,384 tokens) |
| Token formula | `image_tokens = ceil(H/32) × ceil(W/32)` |

### 2.3 Qwen/DashScope API Usage — 两种模式，结果不同

**重要发现（2026-08-07 二轮调研）**：阿里云 Qwen 的两种 API 模式行为完全不同：

**OpenAI-compatible 模式**（[文档](https://help.aliyun.com/zh/model-studio/vision)，`/v1/chat/completions`）— **不返回 image_tokens**：
```json
"usage": {
    "prompt_tokens": 1270,
    "completion_tokens": 54,
    "total_tokens": 1324
}
```
仅 `prompt_tokens` / `completion_tokens` / `total_tokens`，无 `prompt_tokens_details`，无 `image_tokens`。图片 token 静默合并到 `prompt_tokens`。

**DashScope 原生模式**（DashScope SDK，非 OpenAI 协议）— ✅ 返回：
```json
"usage": {
    "output_tokens": 55,
    "input_tokens": 1271,
    "image_tokens": 1247
}
```
但 `input_tokens`/`output_tokens`（非 `prompt_tokens`/`completion_tokens`）且无 `prompt_tokens_details` 嵌套。这是一个完全不同的 response schema。

**aigw 通过 OpenAI-compatible 协议连接 Qwen**（`ProviderType::OpenAICompatible`），所以 qwen-vl 在 aigw 的实际路径中也不返回 image_tokens。

**阿里云官方推荐客户端估算**（[文档](https://help.aliyun.com/zh/model-studio/vision)）：
> "可通过以下代码计算图像或视频的 Token 消耗。估算结果仅供参考，实际用量以 API 响应为准。"

### 2.4 DeepSeek API Usage — 不含 image_tokens

[DeepSeek API 文档](https://api-docs.deepseek.com/api/create-chat-completion)：
```json
"usage": {
    "completion_tokens": 0,
    "prompt_tokens": 0,
    "prompt_cache_hit_tokens": 0,
    "prompt_cache_miss_tokens": 0,
    "total_tokens": 0,
    "completion_tokens_details": { "reasoning_tokens": 0 }
}
```
无 `image_tokens`，无 `prompt_tokens_details` 其他字段。

### 2.5 OpenAI API Usage — 不含 image_tokens

[OpenAI OpenAPI Spec](https://raw.githubusercontent.com/openai/openai-openapi/master/openapi.yaml)：
`prompt_tokens_details` 仅含 `cached_tokens` + `audio_tokens`，**无 `image_tokens`**。

### 2.6 Anthropic API Usage — 不含 image_tokens，但公式公开且精确

Anthropic **不**在 API 返回 image_tokens 分解。但[官方文档](https://docs.anthropic.com/en/docs/build-with-claude/vision)公开了精确的视觉 token 计算公式：

> "Claude views images in patches. Each patch is a 28×28-pixel block — a visual token. An image costs ⌈width/28⌉ × ⌈height/28⌉ visual tokens."

包含 model-specific downsizing rules 和 max token limits（standard 1568, high-res 4784 for Claude 4.7+）。Anthropic 的公式可以**精确计算**（不是估算）——因为 Anthropic 自己就是按这个公式计数的。

### 2.7 总结：OpenAI-compatible 协议下无人返回 image_tokens

| Provider | 协议 | 返回 image_tokens? | 需客户端计算? |
|----------|------|-------------------|-------------|
| Qwen | OpenAI-compat | ❌ | ✅ ViT factor=28/32 |
| OpenAI | Chat Completions | ❌ | ✅ Tiling formula |
| Anthropic | Messages | ❌（公式公开且精确） | ✅ 按公式精确计算 |
| DeepSeek | Chat Completions | ❌ | ✅（需推断公式） |
| GLM-4V | Chat Completions | ❌ | ✅（需推断公式） |
| Qwen | DashScope native | ✅ | —（不用此协议） |
| Gemini | Gemini API | ✅ | —（无 provider） |

**在 aigw 实际面对的 OpenAI-compatible 路径上，所有 provider 都不返回 image token 分解。客户端计算不是 fallback——它就是唯一的路径。**

---

## 3. 架构设计

### 3.1 核心原则：上游优先 + 客户端 fallback

```
image_tokens = ① 解析上游 response（Qwen/Gemini 有）
             → ② 无则客户端估算（OpenAI/Anthropic 没有）
             → ③ 估算失败 → NULL（标记 unknown，不打假数据）
```

估算策略仅用于 **OpenAI 和 Anthropic 的 fallback 路径**，不在 Deployment 上配置。Qwen 直接读上游返回值即可。

### 3.2 数据流

```
Client Request
     │
     ▼
┌─────────────────────────────────────┐
│ 1. Handler (chat.rs/v1_messages.rs) │
│    - Parse request body             │
│    - Extract image base64 from      │
│      content parts / blocks         │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 2. Upstream Call                    │
│    - Forward adapted request        │
│    - Receive usage from upstream    │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 3. Image Token Resolution           │
│    (aigw-core::image_tokens)        │
│                                     │
│  ┌── Try parse upstream ──────────┐ │
│  │ Qwen: usage.prompt_tokens_     │ │
│  │        details.image_tokens    │ │
│  │ DashScope: usage.image_tokens  │ │
│  │ Gemini: promptTokensDetails[]  │ │
│  └── Found? → use it ✅ ─────────┘ │
│                                     │
│  ┌── Fallback (OpenAI/Anthropic) ─┐ │
│  │ Decode base64 → dimensions     │ │
│  │ Apply tiling/ViT formula       │ │
│  └── Result → use estimate ⚠️ ────┘ │
│                                     │
│  ┌── Both fail → NULL ────────────┐ │
│  │ Log debug, don't fake data     │ │
│  └────────────────────────────────┘ │
└──────────────┬──────────────────────┘
               │
               ▼
┌─────────────────────────────────────┐
│ 4. Spend Log Write                  │
│    - image_tokens: Option<i32>      │
│    - image_tokens_source:           │
│      "upstream" | "estimated" | null│
│    - spend calculated as before     │
│      (image_tokens ⊆ prompt_tokens) │
└─────────────────────────────────────┘
```

### 3.3 关键决策

1. **上游返回值优先**：Qwen/Gemini 直接解析，永远不估算（它们返回的比我们估算的更准）。
2. **只对 OpenAI/Anthropic 做 fallback 估算**：它们不返回 image tokens。对 Qwen 不做估算——如果 Qwen response 缺失 image_tokens（罕见），记 NULL 不造假数据。
3. **image_token_strategy 不在 Deployment 上**：它不是模型配置，而是解析逻辑的一部分。Handler 只需要知道"是否有上游返回值"，不需要"用哪种策略估算"（auto-sniff 足够）。
4. **`image_tokens_source` 标记来源**：新的 SpendLog metadata 字段，区分 `"upstream"` vs `"estimated"`，对账时溯源。
5. **不影响 calc_spend**：image_tokens 是 prompt_tokens 的子集，不改变任何计费逻辑。
6. **图片尺寸从 base64 解码获取**：轻量 header parser（PNG/JPEG/WebP/GIF），零新增 crate 依赖。

---

## 4. Phase 43: Image Token Usage Tracking

| Stage | 目标 | 类型 | 预估 |
|-------|------|------|------|
| Stage 106 | **Core engine: upstream parser + fallback estimator** | 后端 | 10h |
| Stage 107 | **Handler integration + SpendLog/DailySpendLog + BDD** | 后端+测试 | 10h |
| Stage 108 | **Frontend display + Real API BDD + docs** | 全栈+文档 | 8h |

**Phase 43 合计**: 28h，3 Stages。

---

## 5. Stage 106: Core Image Token Engine（10h）

**目标**：在 aigw-core 中构建 `image_tokens` 模块，提供两个能力：
1. **上游返回值解析** — 从 provider usage 中提取 image_tokens
2. **客户端 fallback 估算** — 对不返回的 provider（OpenAI/Anthropic）按公式估算

### 5.1 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/image_tokens.rs` | **新建** | 核心模块 |
| `crates/aigw-core/src/lib.rs` | 修改 | `pub mod image_tokens;` |

### 5.2 模块 API 设计

```rust
// ━━━ image_tokens.rs 公开 API ━━━

/// Try to extract image_tokens from provider usage JSON.
/// Covers: Qwen OpenAI-compat (prompt_tokens_details.image_tokens),
///         Qwen DashScope native (image_tokens top-level)
/// Returns None if not reported by upstream.
pub fn extract_image_tokens_from_usage(usage: &Value) -> Option<i32>;

/// Estimate image tokens from a ChatCompletion request body.
/// Only call when extract_image_tokens_from_usage returned None.
/// Auto-detects strategy from model name:
///   gpt-4*/vision/turbo → OpenAI tiling
///   claude → OpenAI tiling (approximation)
///   qwen2.5-vl* → Qwen25VL (for verification/reconciliation)
///   qwen3-vl* → Qwen3VL (for verification/reconciliation)
///   otherwise → 0
pub fn estimate_image_tokens_from_body(
    body: &Value,
    upstream_model: &str,
) -> u32;

/// Estimate image tokens from Anthropic content blocks.
/// Only call when upstream doesn't report image_tokens.
pub fn estimate_image_tokens_from_blocks(
    blocks: &[Value],
    upstream_model: &str,
) -> u32;

/// Minimal image dimension decoder — PNG/JPEG/WebP/GIF headers only.
pub fn decode_image_dimensions(data_url: &str) -> Option<(u32, u32, String)>;
```

### 5.3 上游解析器设计

```rust
/// Parse image_tokens from upstream usage response.
///
/// Three formats supported:
/// 1. OpenAI-compat: usage.prompt_tokens_details.image_tokens (Qwen, litellm)
/// 2. DashScope native: usage.image_tokens (top-level)
/// 3. Gemini: usage.promptTokensDetails[] (future)
pub fn extract_image_tokens_from_usage(usage: &Value) -> Option<i32> {
    // Format 1: OpenAI-compat prompt_tokens_details.image_tokens
    if let Some(v) = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("image_tokens"))
        .and_then(|v| v.as_i64())
    {
        return Some(v as i32);
    }
    // Format 2: DashScope native top-level image_tokens
    if let Some(v) = usage
        .get("image_tokens")
        .and_then(|v| v.as_i64())
    {
        return Some(v as i32);
    }
    None
}
```

### 5.4 估算器设计（仅 fallback 用）

```rust
/// Client-side fallback estimation strategies.
///
/// INVARIANT: Only called when extract_image_tokens_from_usage() → None.
/// Strategy is auto-detected from model name; no Deployment config needed.
#[derive(Debug, Clone, Copy)]
enum EstimationStrategy {
    /// OpenAI: 85 + 170 × tiles (512×512 tiles)
    OpenAITiling,
    /// Qwen2.5-VL: factor=28, dynamic resolution [3136, 12845056]
    Qwen25VL,
    /// Qwen3-VL: factor=32, dynamic resolution [65536, 16777216]
    Qwen3VL,
}

fn infer_strategy(upstream_model: &str) -> Option<EstimationStrategy> {
    let m = upstream_model.to_lowercase();
    if (m.contains("qwen2.5") || m.contains("qwen25")) && (m.contains("vl") || m.contains("vision")) {
        Some(EstimationStrategy::Qwen25VL)
    } else if m.contains("qwen3") && (m.contains("vl") || m.contains("vision")) {
        Some(EstimationStrategy::Qwen3VL)
    } else if m.contains("gpt-4") && (m.contains("vision") || m.contains("-o") || m.contains("turbo")) {
        Some(EstimationStrategy::OpenAITiling)
    } else if m.contains("claude") && (m.contains("opus") || m.contains("sonnet") || m.contains("haiku")) {
        Some(EstimationStrategy::OpenAITiling)
    } else {
        None
    }
}

fn estimate_single(width: u32, height: u32, strategy: EstimationStrategy) -> u32;
fn estimate_openai_tiling(w: u32, h: u32) -> u32;
fn estimate_vit_dynamic(w: u32, h: u32, factor: u32, min_px: u64, max_px: u64) -> u32;

// ━━━ Header parser (PNG/JPEG/WebP/GIF, ~200 lines, zero new deps) ━━━
fn decode_image_dimensions(data_url: &str) -> Option<(u32, u32, String)>;
fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)>;
fn parse_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)>;
fn parse_webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)>;
fn parse_gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)>;
```

### 5.5 关键决策

- **Qwen 策略仅用于验证/对账**：Qwen 本身就返回 image_tokens，我们的估算值只用于对比验证，不作为最终值。Handler 中永远不会对 Qwen 走 fallback 路径。
- **auto-sniff 足够，不需要 model_info 配置**：估算策略不是业务配置，而是技术实现细节。model name 匹配已经覆盖 >99% 场景（qwen/gpt/claude 命名规范明确）。
- **不引入 image crate**：header parser 读 PNG IHDR / JPEG SOF / WebP VP8 / GIF header 获取宽高，~200 行纯 Rust，零依赖。

### 5.6 TDD — 15 unit tests

| # | Test | Coverage |
|---|------|----------|
| 1 | `test_extract_qwen_openai_compat` | `prompt_tokens_details.image_tokens` → Some |
| 2 | `test_extract_dashscope_native` | `usage.image_tokens` top-level → Some |
| 3 | `test_extract_openai_none` | OpenAI format (no image_tokens) → None |
| 4 | `test_extract_anthropic_none` | Anthropic format → None |
| 5 | `test_extract_missing_usage` | No usage field → None |
| 6 | `test_estimate_openai_1024x1024` | 765 tokens |
| 7 | `test_estimate_openai_low_res` | 85 tokens |
| 8 | `test_estimate_qwen25vl_560x420` | 300 tokens |
| 9 | `test_estimate_qwen25vl_clamped_min` | Clamped to 4 tokens |
| 10 | `test_estimate_qwen3vl_1024x1024` | 1024 tokens |
| 11 | `test_img_parse_png` | PNG IHDR width/height |
| 12 | `test_img_parse_jpeg` | JPEG SOF width/height |
| 13 | `test_img_parse_webp_lossy` | VP8 format |
| 14 | `test_img_parse_gif` | GIF format |
| 15 | `test_img_parse_unsupported` | BMP → None |

---

## 6. Stage 107: Handler Integration + SpendLog + BDD（10h）

**目标**：将 engine 接入 handler 的 upstream response → SpendLog 写入链路。

### 6.1 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/models.rs` | 修改 | SpendLog 增加 `image_tokens: Option<i32>`；DailySpendLog 增加 `image_tokens: i64` |
| `crates/aigw-core/src/daily_spend_queue.rs` | 修改 | 写入 image_tokens 到 daily_*_spend 表 |
| `crates/aigw-server/src/routes/chat.rs` | 修改 | 非流式 + 流式路径：解析 upstream → fallback 估算 → 写入 SpendLog |
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改 | Anthropic 路径同样处理 |
| `crates/aigw-server/tests/features/spend.feature` | 修改 | 新增 8 BDD 场景 |
| `crates/aigw-server/tests/bdd_steps/spend_steps.rs` | 修改 | Step 实现 |
| `data/sqlite/025_image_tokens.sql` | **新建** | SQLite 迁移 |
| `data/postgres/025_image_tokens.sql` | **新建** | PostgreSQL 迁移 |
| `data/mysql/025_image_tokens.sql` | **新建** | MySQL 迁移 |

### 6.2 DB Migration 025

```sql
-- spend_logs: image tokens consumed (NULLABLE — absent if model has no image)
ALTER TABLE spend_logs ADD COLUMN image_tokens INTEGER;

-- metadata: source tracking (populated by handler, not a column)
-- Will store {"image_tokens_source": "upstream"|"estimated"|null}
-- Already in metadata JSON — no schema change needed.

-- 6 daily_*_spend tables: accumulate image tokens per day
ALTER TABLE daily_user_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_team_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_organization_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_end_user_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_agent_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_tag_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
```

### 6.3 Handler 集成逻辑

```rust
// ━━━ In chat.rs (non-streaming) ━━━

// Step 1: After upstream call, parse usage
let usage: Value = response_body["usage"].clone();

// Step 2: Try upstream first
let image_tokens = image_tokens::extract_image_tokens_from_usage(&usage);
let source = if image_tokens.is_some() { "upstream" } else { "" };

// Step 3: Fallback estimate if upstream didn't report
let image_tokens = image_tokens.or_else(|| {
    let est = image_tokens::estimate_image_tokens_from_body(
        &body, &deployment.upstream_model,
    );
    if est > 0 { source = "estimated"; Some(est as i32) } else { None }
});

// Step 4: Write to SpendLog
SpendLog {
    // ... existing fields ...
    image_tokens,
    metadata: Some(json!({
        // ... existing metadata fields ...
        "image_tokens_source": source,
    })),
}
```

```rust
// ━━━ In v1_messages.rs (Anthropic path) ━━━

// Anthropic usage has no image_tokens field → always falls through to estimate
let image_tokens = image_tokens::extract_image_tokens_from_usage(&usage)
    .or_else(|| {
        let est = image_tokens::estimate_image_tokens_from_blocks(
            &blocks, &deployment.upstream_model,
        );
        if est > 0 { Some(est as i32) } else { None }
    });
```

**流式路径**：Phase 1 INSERT 写 `image_tokens = NULL`，Phase 2 UPDATE（收到 final usage chunk 后）写最终值。

### 6.4 关键决策

1. **字段名 `image_tokens`（不是 `estimated_image_tokens`）**：因为 Qwen 的值是精确上游返回值，不是估算。source 字段区分来源。
2. **`image_tokens_source` 存在 metadata JSON 中**：不是独立列——只在对账时需要溯源，不需要建索引查询。metadata 已有 JSON 字段，零 schema 变更。
3. **image_tokens 不计入 calc_spend**：是 prompt_tokens 子集，已包含在现有计费公式中。
4. **DailySpendLog 新增 image_tokens**：跟随 Stage 90 cache_tokens 模式。
5. **Qwen 路径不触发估算**：`extract_image_tokens_from_usage` 总是先运行，Qwen 的 response 包含 image_tokens 所以直接返回，永远不会走到 fallback。

### 6.5 BDD Scenarios（8 个）

```gherkin
Feature: Image Token Tracking

  Scenario: Qwen returns image_tokens via OpenAI-compat — stored as "upstream"
    Given 一个 qwen2.5-vl 模型已配置
    And 上游返回 prompt_tokens_details.image_tokens = 400
    When 发送 POST /v1/chat/completions 请求（含图片）
    Then SpendLog 中 image_tokens 为 400
    And metadata.image_tokens_source 为 "upstream"

  Scenario: DashScope native returns image_tokens — stored as "upstream"
    Given 一个 DashScope 直连模型
    And 上游返回 usage.image_tokens = 350
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 350
    And metadata.image_tokens_source 为 "upstream"

  Scenario: OpenAI GPT-4o doesn't return image_tokens — fallback estimate used
    Given 一个 gpt-4o 模型已配置
    And 上游不返回 image_tokens（OpenAI 标准行为）
    And 请求体包含一张 1024x1024 的 base64 JPEG 图片
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 765（tiling formula）
    And metadata.image_tokens_source 为 "estimated"

  Scenario: Anthropic doesn't return image_tokens — fallback estimate used
    Given 一个 claude-sonnet 模型已配置
    And messages 请求包含一个 image content block
    When 发送 POST /v1/messages 请求
    Then SpendLog 中 image_tokens > 0
    And metadata.image_tokens_source 为 "estimated"

  Scenario: Text-only request — no image tokens
    Given 一个 qwen2.5-vl 模型已配置
    And 请求体为纯文本
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 NULL

  Scenario: Multiple images summed correctly
    Given 一个 gpt-4o 模型已配置
    And 请求体包含 3 张 512x512 的图片
    When 发送 chat 请求
    Then SpendLog 中 image_tokens 为 255（85 × 3）

  Scenario: Daily spend aggregation includes image tokens
    Given 多个带图片的请求已发送
    When 查询 daily_spend 聚合数据
    Then 响应包含 image_tokens 字段且值 > 0

  Scenario: Old SpendLog records are NULL
    Given 一条在 image_tokens 功能上线前的支出记录
    When 查询该记录的 SpendLog 详情
    Then image_tokens 字段为 NULL
```

---

## 7. Stage 108: Frontend Display + Real API BDD + Docs（8h）

**目标**：前端 SpendLog + Usage 页面展示 image tokens，真实 API BDD 验证 Qwen 解析正确，文档收尾。

### 7.1 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-frontend/src/types/spend.ts` | 修改 | SpendLog 类型增加 `image_tokens?: number \| null` |
| `crates/aigw-frontend/src/pages/spend-logs/detail.tsx` | 修改 | 抽屉 Usage 区域显示 image_tokens |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 修改 | 列表多模态标记 🖼️ |
| `crates/aigw-frontend/src/i18n/locales/en.json` | 修改 | +3 keys |
| `crates/aigw-frontend/src/i18n/locales/zh-CN.json` | 修改 | +3 keys |
| `crates/aigw-frontend/tests/features/spend-logs.feature` | 修改 | +1 E2E 场景 |
| `docs/stages/stage-106.md` | **新建** | Stage 106 文档 |
| `docs/stages/stage-107.md` | **新建** | Stage 107 文档 |
| `docs/stages/stage-108.md` | **新建** | Stage 108 文档 |
| `docs/stages/stage-roadmap.md` | 修改 | Phase 43 规划回写 |
| `docs/11-next-steps.md` | 修改 | 更新当前进度 |
| `docs/12-technical-debt.md` | 修改 | TD-010 登记 |

### 7.2 前端变更

1. **SpendLog 详情抽屉**：在 Usage 区域增加 `Image Tokens` 行：
   - 仅 `image_tokens` 非 NULL 时显示
   - Tooltip："From upstream provider"（source=upstream）或 "Estimated from image dimensions"（source=estimated）
   - 来源标记用不同颜色 Badge 区分

2. **SpendLog 列表**：多模态请求行显示 🖼️ 图标标记（`image_tokens` 非 NULL）。

3. **i18n keys**：
   | Key | EN | ZH |
   |-----|----|----|
   | `spend.imageTokens` | Image Tokens | 图片 Token |
   | `spend.imageTokensUpstream` | Reported by upstream provider | 服务商返回值 |
   | `spend.imageTokensEstimated` | Estimated from image dimensions | 基于图片尺寸估算 |

### 7.3 Real API BDD

用真实 Qwen API 验证 **解析正确**（不验证估算——Qwen 本身返回真实值）：

```gherkin
@real_api @needs_upstream_qwen
Feature: Image Token Tracking — Real API

  Scenario: Qwen2.5-VL returns image_tokens via OpenAI-compat endpoint
    Given 通过真实 Qwen2.5-VL API 发送含图片的 chat 请求
    When 获取 response usage
    Then prompt_tokens_details.image_tokens > 0
    And image_tokens < prompt_tokens（image 是 prompt 的子集）

  Scenario: Qwen text-only request has no image_tokens
    Given 通过真实 Qwen API 发送纯文本请求
    When 获取 response usage
    Then prompt_tokens_details.image_tokens 不存在或为 0
```

### 7.4 Documentation

- `docs/stages/stage-106.md` — "Image Token Engine"
- `docs/stages/stage-107.md` — "Handler Integration + BDD"
- `docs/stages/stage-108.md` — "Frontend Display + Real API + Docs"
- `docs/stages/stage-roadmap.md` — Phase 43 登记
- `docs/11-next-steps.md` — 更新进度
- `docs/12-technical-debt.md` — TD-010（视频不支持、HEIC/AVIF 不支持）

---

## 8. 数据模型变更总结

### SpendLog
| 字段 | 类型 | 说明 |
|------|------|------|
| `image_tokens` | `Option<i32>` (NULLABLE) | 图片消耗 token，上游返回值优先，无则估算，失败则 NULL |

### SpendLog.metadata (JSON)
| 键 | 类型 | 说明 |
|------|------|------|
| `image_tokens_source` | `"upstream" \| "estimated" \| null` | 数据来源标记 |

### DailySpendLog (6 表)
| 字段 | 类型 | 说明 |
|------|------|------|
| `image_tokens` | `i64` (NOT NULL DEFAULT 0) | 每日图片 token 汇总 |

**无需 Deployment 变更。不需要 model_info 配置。估算策略基于 model name auto-sniff。**

---

## 9. Migration 025 Summary

三个方言迁移文件 — 纯 ADD COLUMN，无数据回填：

| 表 | 列 | 类型 | 默认值 |
|----|-----|------|--------|
| `spend_logs` | `image_tokens` | INTEGER | NULL |
| `daily_user_spend` | `image_tokens` | BIGINT | 0 |
| `daily_team_spend` | `image_tokens` | BIGINT | 0 |
| `daily_organization_spend` | `image_tokens` | BIGINT | 0 |
| `daily_end_user_spend` | `image_tokens` | BIGINT | 0 |
| `daily_agent_spend` | `image_tokens` | BIGINT | 0 |
| `daily_tag_spend` | `image_tokens` | BIGINT | 0 |

---

## 10. 风险 & 边界情况

| 风险 | 影响 | 缓解 |
|------|------|------|
| 图片 header 解析不支持某些格式 | 估算为 0 | PNG/JPEG/WebP/GIF 覆盖 >95%，其余 debug log |
| Qwen 突然不返回 image_tokens | 回退到估算 | 估算值标记 source="estimated"，不影响功能 |
| OpenAI 未来新增 image_tokens 字段 | 双源冲突 | extract 始终优先 upstream，自动切换 |
| 大图片 base64 decode 开销 | CPU spike | O(n) 字节操作，1MB < 1ms，不计入响应延迟 |
| Anthropic 估算偏差（公式未公开） | 估值不准 | 标记 source="estimated"，仅分析用 |

### 技术债（TD-010）

1. **视频不支持**（temporal_patch_size + mRoPE）
2. **HEIC/AVIF 不支持**（需专用解码器）
3. **Anthropic 估算用 OpenAI 公式近似**（未公开 Claude 的确切公式）

---

## 11. 依赖关系

```
Stage 106 (Core Engine)
    │
    ▼
Stage 107 (Handler + SpendLog + BDD)
    │
    ▼
Stage 108 (Frontend + Real BDD + Docs)
```

严格串行。与 Phase 41（Responses API）无文件冲突，可独立交付。

---

## 12. 修订记录

| 版本 | 日期 | 修订 |
|------|------|------|
| v1.0 | 2026-08-07 | 初始版本 |
| v1.1 | 2026-08-07 | 重大修正：Qwen/DashScope 实际返回 image_tokens（最完整），OpenAI/Anthropic 不返回；主流网关均不做预计算 |
| v2.0 | 2026-08-07 | **架构重构**：改为"上游优先 + fallback 估算"模式；`image_token_strategy` 从 Deployment 移除（auto-sniff 足够）；字段名改为 `image_tokens`（非 estimated）；新增 `image_tokens_source` metadata；估算策略仅对 OpenAI/Anthropic 触发；Qwen 走纯解析路径；计费策略明确（不拆分 image 独立定价） |
