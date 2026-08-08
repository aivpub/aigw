//! Image token estimation for multimodal LLM requests.
//!
//! On the OpenAI-compatible protocol (aigw's main path), no major provider
//! returns an `image_tokens` breakdown — Qwen/OpenAI/Anthropic/DeepSeek/GLM
//! all merge image tokens silently into `prompt_tokens`.  This module fills
//! that gap with client-side estimation: it walks the request body for base64
//! images, decodes their dimensions with a zero-dependency header parser, and
//! applies the provider's token formula (auto-sniffed from the model name).
//!
//! `image_tokens` is a subset of `prompt_tokens` — it does NOT change
//! `calc_spend` (upstream already bills total prompt tokens).  It exists for
//! analysis & reconciliation only.

use serde_json::Value;

/// Strategy describing how a model converts image pixels into tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalculationStrategy {
    /// OpenAI GPT-4V / 4o tiling: 85 + 170 × ⌈w/512⌉ × ⌈h/512⌉
    OpenAITiling,
    /// Qwen2.5-VL: ViT factor 28, dynamic resolution clamp
    Qwen25VL,
    /// Qwen3-VL: ViT factor 32, dynamic resolution clamp
    Qwen3VL,
    /// Anthropic (official public formula): ⌈w/28⌉ × ⌈h/28⌉
    Anthropic,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Top-level entry points
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Calculate image tokens for an OpenAI-compatible request body.
///
/// Walks `messages[].content[].image_url` parts, decodes each image's
/// dimensions, sums the per-image estimate using the strategy sniffed from the
/// model name.  Returns `None` when there are no image parts or no strategy
/// matches the model.
pub fn calculate_image_tokens(body: &Value, upstream_model: &str) -> Option<i32> {
    let strategy = infer_strategy(upstream_model)?;
    let messages = body.get("messages")?.as_array()?;
    let mut total: u32 = 0;
    let mut found = false;
    for msg in messages {
        let content = msg.get("content");
        match content {
            Some(Value::Array(parts)) => {
                for part in parts {
                    // A content array mixes text and image parts — a text part
                    // must not short-circuit the loop (use continue, not ?).
                    let url = match part
                        .get("image_url")
                        .and_then(|u| u.get("url"))
                        .and_then(|v| v.as_str())
                    {
                        Some(u) => u,
                        None => continue,
                    };
                    if let Some((w, h, _media_type)) = decode_image_dimensions(url) {
                        total += estimate_single(w, h, strategy);
                        found = true;
                    }
                }
            }
            Some(Value::String(s)) => {
                // Some clients put a single data URL directly in content.
                if let Some((w, h, _media_type)) = decode_image_dimensions(s) {
                    total += estimate_single(w, h, strategy);
                    found = true;
                }
            }
            _ => {}
        }
    }
    if found {
        Some(total as i32)
    } else {
        None
    }
}

/// Estimate image tokens from Claude (Anthropic) content blocks.
///
/// Walks `messages[].content[].source.data` base64 blobs.  Uses the
/// Anthropic official formula when the model is a Claude variant, otherwise
/// falls back to the sniffed strategy.
pub fn estimate_image_tokens_from_blocks(blocks: &[Value], upstream_model: &str) -> u32 {
    let strategy = infer_strategy(upstream_model);
    let mut total: u32 = 0;
    for msg in blocks {
        let Some(Value::Array(items)) = msg.get("content") else {
            continue;
        };
        for item in items {
            let is_image = item
                .get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "image")
                .unwrap_or(false);
            if !is_image {
                continue;
            }
            let source = item.get("source");
            let (data, media_type) = match source {
                Some(Value::Object(s)) => {
                    let data = s.get("data").and_then(|v| v.as_str()).unwrap_or("");
                    let mt = s.get("media_type").and_then(|v| v.as_str()).unwrap_or("");
                    (data, mt.to_string())
                }
                _ => continue,
            };
            if let Some((w, h, _)) = decode_data_dimensions(data, &media_type) {
                total += estimate_single(w, h, strategy.unwrap_or(CalculationStrategy::Anthropic));
            }
        }
    }
    total
}

/// Try to extract image tokens from a provider usage JSON object.
///
/// NOTE: On OpenAI-compatible protocol (aigw's main path), this almost always
/// returns `None` — no major provider returns image_tokens on this protocol.
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Strategy inference
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Auto-sniff the token formula from the upstream model name.
pub fn infer_strategy(upstream_model: &str) -> Option<CalculationStrategy> {
    let m = upstream_model.to_lowercase();
    if (m.contains("qwen2.5") || m.contains("qwen25")) && (m.contains("vl") || m.contains("vision"))
    {
        Some(CalculationStrategy::Qwen25VL)
    } else if m.contains("qwen3") && (m.contains("vl") || m.contains("vision")) {
        Some(CalculationStrategy::Qwen3VL)
    } else if m.contains("gpt-4")
        && (m.contains("vision")
            || m.contains("4o")
            || m.contains("turbo"))
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

/// Compute the token estimate for a single image.
pub fn estimate_single(w: u32, h: u32, strategy: CalculationStrategy) -> u32 {
    match strategy {
        CalculationStrategy::OpenAITiling => estimate_openai_tiling(w, h),
        CalculationStrategy::Qwen25VL => estimate_vit_dynamic(w, h, 28, 3136, 12_845_056),
        CalculationStrategy::Qwen3VL => estimate_vit_dynamic(w, h, 32, 65_536, 16_777_216),
        CalculationStrategy::Anthropic => estimate_anthropic(w, h),
    }
}

/// OpenAI GPT-4V / 4o tiling formula:
/// a single tile (≤512×512, low-res) → 85 tokens;
/// multi-tile (high-res) → 85 + 170 × ⌈w/512⌉ × ⌈h/512⌉ tiles.
fn estimate_openai_tiling(w: u32, h: u32) -> u32 {
    if w == 0 || h == 0 {
        return 0;
    }
    let tiles_w = w.div_ceil(512);
    let tiles_h = h.div_ceil(512);
    if tiles_w == 1 && tiles_h == 1 {
        // Low-res base: image fits a single tile, no tiling overhead.
        85
    } else {
        85 + 170 * tiles_w * tiles_h
    }
}

/// Qwen ViT dynamic-resolution formula: ⌈h/factor⌉ × ⌈w/factor⌉,
/// clamped to [min_tokens, max_tokens] and [min_px, max_px] dynamic window.
fn estimate_vit_dynamic(w: u32, h: u32, factor: u32, min_px: u32, max_px: u32) -> u32 {
    if w == 0 || h == 0 {
        return 0;
    }
    const MIN_TOKENS: u32 = 4;
    const MAX_TOKENS: u32 = 16_384;
    let px = w * h;
    let tokens = if px > max_px {
        // Ratio-preserving downscale to max_px before tiling.
        let scale = (max_px as f64 / px as f64).sqrt();
        let tw = ((w as f64 * scale) as u32).max(1);
        let th = ((h as f64 * scale) as u32).max(1);
        th.div_ceil(factor) * tw.div_ceil(factor)
    } else if px < min_px {
        (h.div_ceil(factor) * w.div_ceil(factor)).max(MIN_TOKENS)
    } else {
        h.div_ceil(factor) * w.div_ceil(factor)
    };
    tokens.clamp(MIN_TOKENS, MAX_TOKENS)
}

/// Anthropic official public visual token formula: ⌈w/28⌉ × ⌈h/28⌉.
/// This is exact per Anthropic docs (model downsizing rules apply upstream).
fn estimate_anthropic(w: u32, h: u32) -> u32 {
    if w == 0 || h == 0 {
        return 0;
    }
    w.div_ceil(28) * h.div_ceil(28)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Data URL & header parsing (zero external deps)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Decode a data URL (`data:{media_type};base64,{data}` or raw base64) into
/// image dimensions.
pub fn decode_image_dimensions(data_url: &str) -> Option<(u32, u32, String)> {
    let (b64, media_type) = parse_data_url(data_url);
    decode_data_dimensions(b64, media_type)
}

/// Base64-encode raw image bytes (test/utility helper).  Exposed so server
/// tests can build data URLs without re-adding a base64 dev-dependency.
pub fn encode_png_header(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Parse `data:image/png;base64,AAAA...` into (base64_payload, media_type).
/// Accepts a bare base64 string with an explicit media_type too.
pub fn parse_data_url(data_url: &str) -> (&str, &str) {
    if let Some(rest) = data_url.strip_prefix("data:") {
        if let Some((meta, b64)) = rest.split_once(',') {
            let media_type = meta.split(';').next().unwrap_or("image/png").trim();
            return (b64.trim(), media_type);
        }
    }
    (data_url.trim(), "image/png")
}

/// Decode base64 payload + media type into image dimensions.
pub fn decode_data_dimensions(b64: &str, media_type: &str) -> Option<(u32, u32, String)> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let (w, h) = parse_dimensions(&bytes, media_type)?;
    Some((w, h, media_type.to_string()))
}

/// Parse image dimensions from raw bytes using the media type.
pub fn parse_dimensions(bytes: &[u8], media_type: &str) -> Option<(u32, u32)> {
    let mt = media_type.to_lowercase();
    if mt.contains("png") {
        parse_png_dimensions(bytes)
    } else if mt.contains("jpeg") || mt.contains("jpg") {
        parse_jpeg_dimensions(bytes)
    } else if mt.contains("webp") {
        parse_webp_dimensions(bytes)
    } else if mt.contains("gif") {
        parse_gif_dimensions(bytes)
    } else {
        None
    }
}

/// PNG: 8-byte signature + IHDR chunk. Width at +16, height at +20 (BE u32).
fn parse_png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// JPEG: scan for SOF0 (0xC0) / SOF2 (0xC2) markers.
/// After the 2-byte marker: 2-byte length, 1-byte precision,
/// then height (2B) and width (2B) — so height at marker+5, width at marker+7.
fn parse_jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 9 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // 0xD9 (EOI) / 0xDA (SOS) terminate scanning.
        if marker == 0xD9 || marker == 0xDA {
            return None;
        }
        // Standalone markers (no length field): 0x01, 0xD0-0xD8 (RST/SOI).
        let standalone = marker == 0x01 || (0xD0..=0xD8).contains(&marker);
        if standalone {
            i += 2;
            continue;
        }
        // SOF markersall except DHT 0xC4 / JPG 0xC8 / DAC 0xCC).
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
            // Segment layout: [FF][marker][len hi][len lo][prec][H hi][H lo][W hi][W lo]
            //               i, i+1,  i+2,   i+3,   i+4,  i+5,  i+6,  i+7,  i+8
            if i + 9 > bytes.len() {
                return None;
            }
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
            if w == 0 || h == 0 {
                return None;
            }
            return Some((w as u32, h as u32));
        }
        let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if seg_len < 2 {
            return None;
        }
        i += 2 + seg_len;
    }
    None
}

/// WebP: RIFF container. Dispatches to VP8 / VP8L / VP8X.
fn parse_webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    match &bytes[12..16] {
        b"VP8 " => parse_webp_vp8(bytes),
        b"VP8L" => parse_webp_vp8l(bytes),
        b"VP8X" => parse_webp_vp8x(bytes),
        _ => None,
    }
}

/// VP8 lossy key frame: dimensions stored at +26/+28 as 14-bit LE values.
fn parse_webp_vp8(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 {
        return None;
    }
    let w = u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF;
    let h = u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w as u32, h as u32))
}

/// VP8L lossless: packed 4-byte field at +21.
/// w = (bits & 0x3FFF) + 1, h = ((bits >> 14) & 0x3FFF) + 1.
fn parse_webp_vp8l(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 25 {
        return None;
    }
    let b0 = bytes[21] as u32;
    let b1 = bytes[22] as u32;
    let b2 = bytes[23] as u32;
    let b3 = bytes[24] as u32;
    let bits = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
    let w = (bits & 0x3FFF) + 1;
    let h = ((bits >> 14) & 0x3FFF) + 1;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// VP8X extended: (w-1, h-1) encoded as 3-byte LE at +24.
fn parse_webp_vp8x(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 {
        return None;
    }
    let w = (bytes[24] as u32) | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16);
    let h = (bytes[27] as u32) | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16);
    let w = w + 1;
    let h = h + 1;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

/// GIF: 6-byte signature + logical screen descriptor.
/// Width at +6, height at +8 (LE u16).
fn parse_gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 {
        return None;
    }
    if &bytes[0..6] != b"GIF87a" && &bytes[0..6] != b"GIF89a" {
        return None;
    }
    let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests (TDD: 15 UT)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    /// Build a minimal valid PNG header (8-byte sig + IHDR) of given size.
    fn png_data_url(w: u32, h: u32) -> String {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes.extend_from_slice(&[0, 0, 0, 13]); // IHDR chunk length
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&w.to_be_bytes());
        bytes.extend_from_slice(&h.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, color type, etc.
        bytes.extend_from_slice(&[0, 0, 0, 0]); // CRC placeholder
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        format!("data:image/png;base64,{}", b64)
    }

    fn jpeg_bytes(w: u16, h: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0xFF, 0xD8]); // SOI
        bytes.push(0xFF);
        bytes.push(0xC0); // SOF0
        bytes.extend_from_slice(&[0, 11, 8]); // length + precision
        bytes.extend_from_slice(&h.to_be_bytes());
        bytes.extend_from_slice(&w.to_be_bytes());
        bytes
    }

    fn gif_bytes(w: u16, h: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GIF89a");
        bytes.extend_from_slice(&w.to_le_bytes());
        bytes.extend_from_slice(&h.to_le_bytes());
        bytes
    }

    // ── extract_image_tokens_from_usage ──

    #[test]
    fn test_extract_qwen_openai_compat() {
        let usage = serde_json::json!({
            "prompt_tokens": 1270,
            "prompt_tokens_details": { "image_tokens": 400, "cached_tokens": 0 },
        });
        assert_eq!(extract_image_tokens_from_usage(&usage), Some(400));
    }

    #[test]
    fn test_extract_dashscope_native() {
        let usage = serde_json::json!({
            "input_tokens": 1271,
            "output_tokens": 55,
            "image_tokens": 350,
        });
        assert_eq!(extract_image_tokens_from_usage(&usage), Some(350));
    }

    #[test]
    fn test_extract_openai_none() {
        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "prompt_tokens_details": { "cached_tokens": 0 },
        });
        assert_eq!(extract_image_tokens_from_usage(&usage), None);
    }

    #[test]
    fn test_extract_anthropic_none() {
        let usage = serde_json::json!({
            "input_tokens": 100,
            "output_tokens": 50,
        });
        assert_eq!(extract_image_tokens_from_usage(&usage), None);
    }

    #[test]
    fn test_extract_missing_usage() {
        let usage = serde_json::json!({});
        assert_eq!(extract_image_tokens_from_usage(&usage), None);
    }

    // ── estimate_single ──

    #[test]
    fn test_estimate_openai_1024x1024() {
        // 85 + 170 × ⌈1024/512⌉ × ⌈1024/512⌉ = 85 + 170×4 = 765
        assert_eq!(
            estimate_single(1024, 1024, CalculationStrategy::OpenAITiling),
            765
        );
    }

    #[test]
    fn test_estimate_openai_low_res() {
        // 256×256 is below 512 tiles → 85 (low-res base)
        assert_eq!(
            estimate_single(256, 256, CalculationStrategy::OpenAITiling),
            85
        );
    }

    #[test]
    fn test_estimate_qwen25vl_560x420() {
        // ⌈560/28⌉ × ⌈420/28⌉ = 20 × 15 = 300
        assert_eq!(
            estimate_single(560, 420, CalculationStrategy::Qwen25VL),
            300
        );
    }

    #[test]
    fn test_estimate_qwen25vl_clamped_min() {
        // 1×1 would be 0, clamped to 4 tokens
        assert_eq!(estimate_single(1, 1, CalculationStrategy::Qwen25VL), 4);
    }

    #[test]
    fn test_estimate_qwen3vl_1024x1024() {
        // ⌈1024/32⌉ × ⌈1024/32⌉ = 32 × 32 = 1024
        assert_eq!(
            estimate_single(1024, 1024, CalculationStrategy::Qwen3VL),
            1024
        );
    }

    // ── header parsers ──

    #[test]
    fn test_img_parse_png() {
        let url = png_data_url(640, 480);
        let (w, h, mt) = decode_image_dimensions(&url).unwrap();
        assert_eq!((w, h), (640, 480));
        assert_eq!(mt, "image/png");
    }

    #[test]
    fn test_img_parse_jpeg() {
        let bytes = jpeg_bytes(800, 600);
        let (w, h) = parse_dimensions(&bytes, "image/jpeg").unwrap();
        assert_eq!((w, h), (800, 600));
    }

    #[test]
    fn test_img_parse_webp_lossy() {
        // RIFF + WEBP + VP8 chunk: 12-byte container + 4-byte fourcc +
        // 10-byte VP8 frame header + 2×2-byte dimensions = 30 bytes total.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF\x00\x00\x00\x00WEBP");
        bytes.extend_from_slice(b"VP8 ");
        bytes.extend_from_slice(&[0u8; 10]); // frame header
        let w: u16 = 320 & 0x3FFF;
        let h: u16 = 240 & 0x3FFF;
        bytes.extend_from_slice(&w.to_le_bytes());
        bytes.extend_from_slice(&h.to_le_bytes());
        assert_eq!(bytes.len(), 30);
        let (pw, ph) = parse_dimensions(&bytes, "image/webp").unwrap();
        assert_eq!((pw, ph), (320, 240));
    }

    #[test]
    fn test_img_parse_gif() {
        let bytes = gif_bytes(120, 80);
        let (w, h) = parse_dimensions(&bytes, "image/gif").unwrap();
        assert_eq!((w, h), (120, 80));
    }

    #[test]
    fn test_img_parse_unsupported() {
        let bytes: &[u8] = b"BM\x00\x00\x00\x00\x00\x00\x00\x00"; // BMP header
        assert_eq!(parse_dimensions(bytes, "image/bmp"), None);
    }

    // ── integration ──

    #[test]
    fn test_calculate_from_body_qwen() {
        let body = serde_json::json!({
            "model": "qwen2.5-vl-72b",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": "describe" },
                    { "type": "image_url", "image_url": { "url": png_data_url(560, 420) } }
                ]
            }]
        });
        assert_eq!(calculate_image_tokens(&body, "qwen2.5-vl-72b"), Some(300));
    }

    #[test]
    fn test_calculate_from_body_no_images() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [{ "role": "user", "content": "hello" }]
        });
        assert_eq!(calculate_image_tokens(&body, "gpt-4o"), None);
    }

    #[test]
    fn test_estimate_anthropic_blocks() {
        // Raw PNG bytes (sig + IHDR 560×420), base64-encoded inside a block.
        let mut png = Vec::new();
        png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&560u32.to_be_bytes());
        png.extend_from_slice(&420u32.to_be_bytes());
        let blocks = serde_json::json!([{
            "role": "user",
            "content": [{
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": "image/png",
                    "data": base64::engine::general_purpose::STANDARD.encode(&png)
                }
            }]
        }]);
        // ⌈560/28⌉ × ⌈420/28⌉ = 20 × 15 = 300 (Anthropic formula)
        let est =
            estimate_image_tokens_from_blocks(blocks.as_array().unwrap(), "claude-sonnet-4-5");
        assert_eq!(est, 300);
    }
}
