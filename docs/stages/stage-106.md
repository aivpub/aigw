# Stage 106: Image Token Engine

**Phase**: 43 — Image Token Usage Tracking
**优先级**: P1
**状态**: ✅ 完成（2026-08-08，`45d7323`）
**预估**: 10h（实际 6h）
**前置**: 无（独立 Phase）
**后置**: Stage 107（Handler 集成依赖本模块）

---

## 核心预期

1. **客户端图像 token 计算引擎**：从 request body 中提取 base64 图片 → 解析宽高（PNG/JPEG/WebP/GIF header parser）→ 按 provider 公式计算 image token 数。

2. **上游 image_tokens 解析器**（辅助）：对于极少数通过非 OpenAI-compat 协议连接的 provider（DashScope 原生、未来 Gemini），从 response usage 提取 `image_tokens`。但 aigw 的主路径上此函数几乎总是返回 None。

3. **Minimal image header parser**：PNG IHDR / JPEG SOF / WebP VP8/VP8L/VP8X / GIF 头解析，零新增 crate 依赖。

4. **计算策略 auto-sniff**：从 model name 自动识别公式（不需要 model_info 配置）。

---

## 背景

**结论（二轮调研修正）**：在 OpenAI-compatible 协议下，**所有主流 provider 都不返回 image token 分解**。

| Provider | 协议 | 返回 image_tokens? | 需客户端计算? |
|----------|------|-------------------|-------------|
| Qwen | OpenAI-compat | ❌ | ✅ ViT factor=28/32 |
| OpenAI | Chat Completions | ❌ | ✅ Tiling formula |
| Anthropic | Messages | ❌（公式公开且精确） | ✅ 按官方公式精确计算 |
| DeepSeek | Chat Completions | ❌ | ✅（需推断公式） |
| GLM-4V | Chat Completions | ❌ | ✅（需推断公式） |
| Qwen | DashScope native | ✅ | —（aigw 不用此协议） |

**关键证据**：

1. [阿里云 vision 文档](https://help.aliyun.com/zh/model-studio/vision) — Qwen OpenAI-compat endpoint 实测仅返回 `prompt_tokens`/`completion_tokens`/`total_tokens`，无 `image_tokens`。**阿里云官方推荐客户端估算**："可通过以下代码计算图像或视频的 Token 消耗。估算结果仅供参考，实际用量以 API 响应为准。"

2. [OpenAI OpenAPI Spec](https://raw.githubusercontent.com/openai/openai-openapi/master/openapi.yaml) — `prompt_tokens_details` 仅含 `cached_tokens` + `audio_tokens`，无 `image_tokens`。

3. [Anthropic Vision 文档](https://docs.anthropic.com/en/docs/build-with-claude/vision) — 公开视觉 token 精确公式：`⌈width/28⌉ × ⌈height/28⌉`。

4. [DeepSeek API 文档](https://api-docs.deepseek.com/api/create-chat-completion) — usage 无 image token 字段。

5. litellm/OpenRouter/OneAPI 源码确认均不做客户端预计算。

**在 aigw 的 OpenAI-compatible 主路径上，客户端计算不是 fallback——它就是唯一路径。**

---

## 设计

### 1. 模块结构（`crates/aigw-core/src/image_tokens.rs`）

```
image_tokens.rs
├── calculate_image_tokens(body: &Value, upstream_model: &str) -> Option<i32>
│   ├── [primary] Walk messages[].content.parts[].image_url
│   ├── decode_image_dimensions(data_url) → Option<(w, h, media_type)>
│   ├── infer_strategy(model) → OpenAI | ViT28 | ViT32 | Anthropic28 | None
│   ├── estimate_single(w, h, strategy) → tokens
│   └── [auxiliary] Try extract from upstream usage first
│       └── extract_image_tokens_from_usage(usage) → Option<i32>
│           ├── Format 1: prompt_tokens_details.image_tokens (Qwen OpenAI-compat — rare)
│           └── Format 2: usage.image_tokens (DashScope native — not on aigw main path)
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
└── (internal) calculation strategies
    ├── infer_strategy(model) → CalculationStrategy
    ├── estimate_single(w, h, strategy) → u32
    ├── estimate_openai_tiling(w, h) → u32
    ├── estimate_vit_dynamic(w, h, factor, min_px, max_px) → u32
    └── estimate_anthropic(w, h) → u32  // Exact formula from docs
```

### 2. 上游解析器（辅助路径）

```rust
/// Try to extract image tokens from provider usage JSON.
///
/// NOTE: On OpenAI-compatible protocol (aigw's main path), this almost always
/// returns None — no major provider returns image_tokens on this protocol.
/// Only useful for DashScope native or future Gemini provider paths.
pub fn extract_image_tokens_from_usage(usage: &Value) -> Option<i32> {
    // Format 1: OpenAI-compat prompt_tokens_details.image_tokens (rare)
    if let Some(v) = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("image_tokens"))
        .and_then(|v| v.as_i64())
    {
        return Some(v as i32);
    }
    // Format 2: DashScope native top-level image_tokens (not on aigw main path)
    if let Some(v) = usage.get("image_tokens").and_then(|v| v.as_i64()) {
        return Some(v as i32);
    }
    None
}
```

### 3. 计算策略（主路径）

```rust
#[derive(Debug, Clone, Copy)]
enum CalculationStrategy {
    OpenAITiling,   // 85 + 170 × tiles
    Qwen25VL,       // ViT factor 28 + dynamic resolution
    Qwen3VL,        // ViT factor 32 + dynamic resolution
    Anthropic,      // Exact formula: ⌈w/28⌉ × ⌈h/28⌉ (官方公开)
}

fn infer_strategy(upstream_model: &str) -> Option<CalculationStrategy> {
    let m = upstream_model.to_lowercase();
    if (m.contains("qwen2.5") || m.contains("qwen25"))
        && (m.contains("vl") || m.contains("vision"))
    {
        Some(CalculationStrategy::Qwen25VL)
    } else if m.contains("qwen3") && (m.contains("vl") || m.contains("vision")) {
        Some(CalculationStrategy::Qwen3VL)
    } else if m.contains("gpt-4")
        && (m.contains("vision") || m.contains("-o") || m.contains("turbo"))
    {
        Some(CalculationStrategy::OpenAITiling)
    } else if m.contains("claude")
        && (m.contains("opus") || m.contains("sonnet") || m.contains("haiku"))
    {
        // Anthropic 官方公开了精确的视觉 token 公式，不是估算
        Some(CalculationStrategy::Anthropic)
    } else {
        None
    }
}
```

### 4. 计算公式

| 策略 | 公式 | 精度 | 来源 |
|------|------|------|------|
| `OpenAITiling` | Low: 85 / High: `85 + 170 × ⌈w'/512⌉ × ⌈h'/512⌉` | 近似 | OpenAI pricing docs |
| `Qwen25VL` | `⌈h/28⌉ × ⌈w/28⌉`, clamped [4, 16384], dynamic [3136, 12845056] px | 近似 | HuggingFace config + 阿里云估算公式 |
| `Qwen3VL` | `⌈h/32⌉ × ⌈w/32⌉`, clamped [4, 16384], dynamic [65536, 16777216] px | 近似 | HuggingFace config |
| `Anthropic` | `⌈w/28⌉ × ⌈h/28⌉` + model downsizing rules | **精确** | [Anthropic 官方文档](https://docs.anthropic.com/en/docs/build-with-claude/vision) |

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
