# Stage 103: 多模态适配修复 + 模型模式暴露 + 多模态 BDD

**Phase**: 42 — Playground 多模态图片能力
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 6.5h
**前置**: 无（独立于 Stage 104/105）
**后置**: Stage 104（Playground 图片输入依赖后端转换正确 + `/v1/models` 模式字段）

---

## 核心预期

1. **修复 `openai_message_to_claude` image 转换 bug**：当前 `crates/aigw-core/src/adapter.rs` L1113-1115 硬编码 `media_type: "image/jpeg"` 且把完整 `data:image/png;base64,...` URL 直接塞进 `data` 字段——Anthropic 上游要求 `data` 为纯 base64（无 `data:` 前缀）且 `media_type` 与实际格式匹配。修复 = 从 data URL 剥离 `data:` 前缀 + 推导 media_type，malformed 时 fallback 到 `image/png`。

2. **暴露 `model_info.mode` 到 `/v1/models`**：`ModelEntry` 增加可选 `model_info` 字段，master 路径从 `list_models()` 返回的完整 `ProxyModel` 透传（含 `mode`，用于前端识别多模态模型）；非 master 路径（key.models 只有 model_name 字符串）缺省不返回。向后兼容（`#[serde(skip_serializing_if = "Option::is_none")]`）。

3. **多模态后端 BDD**：补 6 个 mock BDD 场景覆盖图片透传/转换三条路径（chat / messages / anthropic native）+ `/v1/models` mode 字段 + 详情 body 保留 image。

4. **零回归**：`claude_message_to_openai` 已正确生成 `data:{media_type};base64,{data}`（无需改）；现有 62 个 adapter UT + 3 个 `test_models_list_*` 测试零改动（它们只断言 object/data-length/id）。

---

## 背景

用户要给 Playground 加图片能力，让 qwen3.5-vl 等多模态模型在 playground 中识别图片。后端当前的多模态支持是**部分正确**的：

- **正向（Claude→OpenAI）**：`claude_message_to_openai()`（adapter.rs L1196+）已把 Claude `image` block → OpenAI `image_url` content part，URL 格式 `data:{media_type};base64,{data}`（L1256/L1295）——正确，无需改。
- **反向（OpenAI→Claude）**：`openai_message_to_claude()`（adapter.rs L1084+）在 `ChatContent::Parts` 分支把 `image_url.url` 完整塞进 `ClaudeImageSource.data`，且 `media_type` 硬编码 `image/jpeg`（L1113-1115）——**bug**。Anthropic Messages API 要求 `source.data` 为纯 base64，`media_type` 为实际类型（`image/png`/`image/jpeg`/`image/gif`/`image/webp`）。发送 PNG 图片到 Claude 上游会 400。
- **`/v1/models`**：`ModelEntry` 只有 id/object/created/owned_by，前端无法区分多模态 vs 纯文本模型。

本 Stage 只修这两个最小缺口 + 补测试，不新增网关图片校验（litellm 亦无，Playground 客户端负责）。

---

## 设计

### ① `openai_message_to_claude` data URL 解析（`crates/aigw-core/src/adapter.rs`）

**改动位置**：`fn openai_message_to_claude`（L1084+），`ChatContent::Parts` 分支的 image 部分（L1106-1120）。

**修复**：新增私有 helper `parse_data_url(url: &str) -> (String, String)`（media_type, data）：

```rust
/// Parse a `data:<media_type>;base64,<payload>` URL into (media_type, data).
/// Falls back to ("image/png", full url) on malformed input so the request
/// still carries the payload (Anthropic may reject with 400, which is a
/// client-side problem, not a silent drop).
fn parse_data_url(url: &str) -> (String, String) {
    let Some(rest) = url.strip_prefix("data:") else {
        return ("image/png".into(), url.into());
    };
    let Some(comma) = rest.find(',') else {
        return ("image/png".into(), url.into());
    };
    let mime = &rest[..comma];
    let payload = &rest[comma + 1..];
    // mime may be "image/png;base64" — split off ";base64" and any params
    let media_type = mime.split(';').next().unwrap_or("image/png").to_string();
    if media_type.is_empty() {
        ("image/png".into(), payload.into())
    } else {
        (media_type, payload.into())
    }
}
```

`ClaudeImageSource` 构造改为：

```rust
let (media_type, data) = parse_data_url(&image_url.url);
source: Some(ClaudeImageSource {
    source_type: "base64".to_string(),
    media_type,
    data,
})
```

**边界情况**：
- 非 data URL（如 `https://...` 或裸 base64）→ media_type 回退 `image/png`，data 原样。Anthropic 只接受 base64，非 data URL 会 400——但这是客户端责任，网关不静默丢弃。
- 未知 MIME（`image/avif` 等）→ 原样传递（Anthropic 支持有限集，超集由上游裁决）。
- 空 payload → 原样传递空串。

### ② `/v1/models` 暴露 `model_info`（`crates/aigw-server/src/routes/chat.rs`）

**改动位置**：`ModelEntry`（L620-625）+ `models_list`（L2033-2090）。

```rust
pub struct ModelEntry {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
    /// Optional model metadata (mode, pricing, ...) — exposed so clients can
    /// distinguish multimodal (mode: "image"/"vision") from chat-only models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_info: Option<serde_json::Value>,
}
```

`models_list`：
- master 路径：`models.into_iter().map(|m| ModelEntry { ..., model_info: Some(m.model_info) })`（`list_models()` 已返回完整 `ProxyModel`）。
- 非 master 路径：key.models 只有 model_name 字符串（无 model_info），`model_info: None`。

**零回归保证**：现有 `test_models_list_*` 只断言 `object=="list"` + `data` 非空 + id 成员，插入的 `model_info: json!({})` 在 `skip_serializing_if` 下序列化为空对象，不破坏断言。

### ③ 多模态 BDD（6 场景）

| 文件 | 场景 | 覆盖 |
|------|------|------|
| `end_to_end.feature` | chat 图片透传 | OpenAI image_url parts → OpenAICompatible 上游 body 原样（recorded_requests body 断言） |
| `messages.feature` | messages 图片转 OpenAI | /v1/messages Claude image block → AnthropicToOpenAI → OpenAI content-parts（data: 前缀重建） |
| `anthropic_native.feature` | anthropic native 图片→Claude | OpenAIToAnthropic 反向：data URL 剥离 + media_type 推导 |
| `anthropic_native.feature` | 反向转换 data URL 剥离 | openai→claude→openai roundtrip 保留 image |
| `models.feature` | /v1/models mode 字段 | master key 序列化 `data[0].model_info.mode` |
| `models.feature` | 非 master 省略 model_info | key 模型列表无 model_info |

---

## TDD 测试计划

### Unit Tests（8）

| # | 位置 | 测试 | 覆盖 |
|---|------|------|------|
| 1 | adapter.rs | `test_parse_data_url_png` | `data:image/png;base64,xxx` → ("image/png", "xxx") |
| 2 | adapter.rs | `test_parse_data_url_jpeg_with_params` | `data:image/jpeg;base64;charset=utf-8,...` → MIME 前段 |
| 3 | adapter.rs | `test_parse_data_url_malformed` | 无逗号/非 data 前缀 → fallback image/png |
| 4 | adapter.rs | `test_openai_to_claude_image_preserves_type` | ContentPart image_url → ClaudeImageSource media_type/data 剥离 |
| 5 | adapter.rs | `test_claude_to_openai_image_data_prefix` | Claude image block → OpenAI image_url `data:{mime};base64,{data}` |
| 6 | adapter.rs | `test_image_roundtrip_openai_claude_openai` | openai→claude→openai 图片保留 |
| 7 | chat.rs | `test_models_list_master_includes_model_info` | /v1/models master 返回 `data[0].model_info.mode` |
| 8 | chat.rs | `test_models_list_key_omits_model_info` | 非 master key 不返回 model_info |

### BDD（6 mock 场景）

见设计 ③。

---

## 门禁标准

|  | 要求 |
|---|------|
| `task check` | 无编译错误 |
| `task test` | aigw-core + aigw-server 全绿（新增 8 UT，零回归） |
| `task test-bdd` | ≥ 6 新增 mock 场景全绿，现有 191+ 场景零回归 |

---

## 依赖关系

- **无前向依赖** — 3 个独立小改动。
- Stage 104 依赖本 Stage（Playground 发送图片依赖反向转换正确 + `/v1/models` 模式字段）。

---

## 交付清单

- [ ] `crates/aigw-core/src/adapter.rs` — `parse_data_url` helper + `openai_message_to_claude` 修复
- [ ] `crates/aigw-server/src/routes/chat.rs` — `ModelEntry.model_info` + `models_list` 透传
- [ ] aigw-core UT × 6 + aigw-server UT × 2
- [ ] BDD × 6（end_to_end / messages / anthropic_native / models）
- [ ] 文档：ADR-025 中记录 Stage 103 决策（Stage 105 收尾统一写）
