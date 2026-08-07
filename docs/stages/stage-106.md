# Stage 106: Image Token Engine

**Phase**: 43 — Image Token Usage Tracking
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: 无（独立 Phase）
**后置**: Stage 107（Handler 集成依赖本模块）

---

## 核心预期

1. **上游 image_tokens 解析器**：从 provider usage JSON 提取 `image_tokens`，覆盖 Qwen OpenAI-compat（`prompt_tokens_details.image_tokens`）和 DashScope native（`usage.image_tokens` 顶层字段）两种格式。

2. **客户端 fallback 估算引擎**：对不返回 image_tokens 的 provider（OpenAI、Anthropic），从 request body 中提取 base64 图片 → 解析宽高 → 按 provider 特定公式估算 token 数。

3. **Minimal image header parser**：PNG IHDR / JPEG SOF / WebP VP8/VP8L/VP8X / GIF 头解析，零新增 crate 依赖。

4. **估算策略 auto-sniff**：从 model name 自动识别策略（不需要 model_info 配置）。

---

## 背景

多模态请求的 image token 用量数据分布不均：
- **Qwen/DashScope** ✅ 返回 `prompt_tokens_details.image_tokens`（OpenAI 兼容）和 `usage.image_tokens`（DashScope 原生）
- **Google Gemini** ✅ 返回 `promptTokensDetails[]` per-modality
- **OpenAI** ❌ `prompt_tokens_details` 仅含 `cached_tokens` + `audio_tokens`
- **Anthropic** ❌ 图片 token 合并在 `input_tokens` 中

对不返回的 provider，业界网关（litellm/OpenRouter/OneAPI）均不做客户端估算——`image_tokens` 默认为 0。aigw 填补这一空缺，为 OpenAI/Anthropic 做 fallback 估算。

对返回的 provider（Qwen），直接解析上游值，永远不做估算——上游返回值比我们的估算更精确。

---

## 设计

### 1. 模块结构（`crates/aigw-core/src/image_tokens.rs`）

```
image_tokens.rs
├── extract_image_tokens_from_usage(usage: &Value) -> Option<i32>
│   ├── Format 1: prompt_tokens_details.image_tokens（Qwen OpenAI-compat）
│   └── Format 2: usage.image_tokens（DashScope native top-level）
│
├── estimate_image_tokens_from_body(body: &Value, upstream_model: &str) -> u32
│   ├── Walk messages[].content.parts[].image_url
│   ├── decode_image_dimensions(data_url) -> Option<(w, h, media_type)>
│   ├── infer_strategy(model) → OpenAI | ViT28 | ViT32 | None
│   └── estimate_single(w, h, strategy) → tokens
│
├── estimate_image_tokens_from_blocks(blocks: &[Value], upstream_model: &str) -> u32
│   └── Walk Claude content blocks [].source.data
│
├── decode_image_dimensions(data_url: &str) -> Option<(u32, u32, String)>
│   ├── parse_data_url(data_url) → (media_type, base64_str)
│   ├── base64 decode → raw bytes
│   └── parse_*_dimensions(bytes, media_type) → (w, h)
│       ├── parse_png_dimensions  (IHDR chunk)
│       ├── parse_jpeg_dimensions (SOF0/SOF2 marker)
│       ├── parse_webp_dimensions (VP8/VP8L/VP8X RIFF)
│       └── parse_gif_dimensions  (logical screen descriptor)
│
└── (internal) estimation strategies
    ├── infer_strategy(model) → Option<EstimationStrategy>
    ├── estimate_single(w, h, strategy) → u32
    ├── estimate_openai_tiling(w, h) → u32
    └── estimate_vit_dynamic(w, h, factor, min_px, max_px) → u32
```

### 2. 上游解析器

```rust
/// Try to extract image tokens from provider usage JSON.
///
/// Covers two formats:
///   1. OpenAI-compat: usage.prompt_tokens_details.image_tokens (Qwen, litellm)
///   2. DashScope native: usage.image_tokens (top-level)
///
/// Returns None if not reported by upstream (OpenAI, Anthropic).
pub fn extract_image_tokens_from_usage(usage: &Value) -> Option<i32> {
    // Format 1: OpenAI-compat
    if let Some(v) = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("image_tokens"))
        .and_then(|v| v.as_i64())
    {
        return Some(v as i32);
    }
    // Format 2: DashScope native top-level
    if let Some(v) = usage.get("image_tokens").and_then(|v| v.as_i64()) {
        return Some(v as i32);
    }
    None
}
```

### 3. 估算策略

```rust
#[derive(Debug, Clone, Copy)]
enum EstimationStrategy {
    OpenAITiling,
    Qwen25VL,  // factor 28
    Qwen3VL,   // factor 32
}

fn infer_strategy(upstream_model: &str) -> Option<EstimationStrategy> {
    let m = upstream_model.to_lowercase();
    if (m.contains("qwen2.5") || m.contains("qwen25"))
        && (m.contains("vl") || m.contains("vision"))
    {
        Some(EstimationStrategy::Qwen25VL)
    } else if m.contains("qwen3") && (m.contains("vl") || m.contains("vision")) {
        Some(EstimationStrategy::Qwen3VL)
    } else if m.contains("gpt-4")
        && (m.contains("vision") || m.contains("-o") || m.contains("turbo"))
    {
        Some(EstimationStrategy::OpenAITiling)
    } else if m.contains("claude")
        && (m.contains("opus") || m.contains("sonnet") || m.contains("haiku"))
    {
        Some(EstimationStrategy::OpenAITiling)
    } else {
        None
    }
}
```

**Qwen 的策略仅用于验证/对账**，handler 中 Qwen 永远走上游解析路径。

### 4. 计算公式

| 策略 | 公式 | 备注 |
|------|------|------|
| `OpenAITiling` | Low: 85 / High: `85 + 170 × ceil(w'/512) × ceil(h'/512)` | w',h' 缩放到 max≤2048, min≤768 |
| `Qwen25VL` | `ceil(h/28) × ceil(w/28)`, clamped [4, 16384] | Dynamic resolution [3136, 12845056] px |
| `Qwen3VL` | `ceil(h/32) × ceil(w/32)`, clamped [4, 16384] | Dynamic resolution [65536, 16777216] px |

### 5. Image Header Parser

不引入 image crate，仅读文件头获取宽高：

| 格式 | 方法 | 偏移量 |
|------|------|--------|
| PNG | IHDR chunk (8-byte sig + "IHDR" + data) | Width at +16, Height at +20 (BE u32) |
| JPEG | SOF0/SOF2 marker (0xFF 0xC0/0xC2) | Height at marker+5, Width at marker+7 (BE u16) |
| WebP VP8 | Key frame header | Dimensions at +26/+28 (14-bit LE) |
| WebP VP8L | Packed 4 bytes at +21 | w = (bits & 0x3FFF) + 1, h = ((bits >> 14) & 0x3FFF) + 1 |
| WebP VP8X | Extended header at +24 | (w-1, h-1) encoded as 3-byte LE |
| GIF | Logical screen descriptor | Width at +6, Height at +8 (LE u16) |

---

## 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/image_tokens.rs` | **新建** | 核心模块 (~350 行) |
| `crates/aigw-core/src/lib.rs` | 修改 | `pub mod image_tokens;` + re-export |

---

## TDD

15 个内联 `#[cfg(test)]` UT（在 `image_tokens.rs` 底部）：

| # | Test | 验证 |
|---|------|------|
| 1 | `test_extract_qwen_openai_compat` | `prompt_tokens_details.image_tokens` → Some(400) |
| 2 | `test_extract_dashscope_native` | `usage.image_tokens` top-level → Some(350) |
| 3 | `test_extract_openai_none` | OpenAI format → None |
| 4 | `test_extract_anthropic_none` | Anthropic format → None |
| 5 | `test_extract_missing_usage` | No usage field → None |
| 6 | `test_estimate_openai_1024x1024` | 765 tokens |
| 7 | `test_estimate_openai_low_res` | 256x256 → 85 tokens |
| 8 | `test_estimate_qwen25vl_560x420` | 300 tokens |
| 9 | `test_estimate_qwen25vl_clamped_min` | 1x1 → 4 tokens (min clamp) |
| 10 | `test_estimate_qwen3vl_1024x1024` | 1024 tokens |
| 11 | `test_img_parse_png` | PNG IHDR → correct width/height |
| 12 | `test_img_parse_jpeg` | JPEG SOF → correct width/height |
| 13 | `test_img_parse_webp_lossy` | VP8 → correct dimensions |
| 14 | `test_img_parse_gif` | GIF → correct dimensions |
| 15 | `test_img_parse_unsupported` | BMP → None |

---

## Gate 门禁

- `task check` 通过（零编译错误）
- `cargo test -p aigw-core` 15 新 UT 全绿 + 全量零回归
- `cargo clippy -p aigw-core` 无新增 warning
