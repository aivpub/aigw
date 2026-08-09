# Stage 115: 多模态精度（TD-011 重定义 + TD-012b 按模态计费）

**所属**: Phase 45（技术债清理）
**预估**: 10h（后端 + 前端 + 测试）
**依赖**: 无硬依赖；011c 依赖 011b 转码后输入

---

## 1. 目标

多模态技术债三个子项，含两个**方案变更**（相对原 TD 描述）：
1. **TD-011b→转码** — HEIC/AVIF 改由前端 canvas 转码 JPEG/WebP（而非后端解析 ISO-BMFF）
2. **TD-011c** — Anthropic downsizing 规则模拟（target=1568 tokens）
3. **TD-012b** — 多模态 embedding 按模态单价计费
4. **TD-011a→视频输入**（可选，工作量高）— Playground 视频上传/发送

## 2. 方案变更说明

### 2.1 TD-011b: HEIC/AVIF 前端转码（方案变更）

**原方案**：后端 header parser 加 HEIC/AVIF 解析。**变更理由**：
- HEIC/AVIF 是 ISO-BMFF 容器，递归 box 解析复杂度高、零依赖实现脆弱
- Chrome/Firefox **不渲染 HEIC**（仅 Safari/Apple 生态），后端解析出尺寸也无法在 web 预览
- **前端转码一举三得**：上传即预览 + 送后端可渲染 + 引擎 PNG/JPEG/WebP/GIF 解析天然命中

**改动**：
- `compressImage`（Stage 114 已有）扩展：检测 `image/heic`/`image/avif` → 尝试 `createImageBitmap` 解码 → canvas → JPEG/WebP；浏览器无法解码（非 Safari）→ 明确提示"HEIC 请在 Safari 打开或转存"
- Playground `RASTER_MIME` 加 `image/heic|avif`（仅转码路径）
- 引擎侧不新增解析器（转码后天然覆盖）

**TDD**: fe-bdd 2 场景 × 3 viewports（HEIC 上传转码预览 + 非 Safari 提示）+ core UT 1（HEIC 输入在引擎层仍落空但前端已转码的契约）

### 2.2 TD-011c: Anthropic downsizing

**现状**（`image_tokens.rs:257 estimate_anthropic`）：`⌈w/28⌉ × ⌈h/28⌉` 精确，但 Anthropic 模型端对超大图有"超出 max_tokens 自动缩放"规则（target=1568 tokens）。

**改动**：`estimate_anthropic` 计算后若 > 1568：按比例缩放到 target（保留宽高比）后重算。

**TDD**: ~3 UT
| # | Test | 断言 |
|---|------|------|
| 1 | 小图（≤1568）不变 | 原公式结果 |
| 2 | 超大图 downsizing | >1568 → 缩放后重算 ≈ target |
| 3 | 极端宽高比 | 缩放保留 ratio |

### 2.3 TD-012b: 多模态 embedding 按模态计费

**现状**：`Deployment.input_cost_per_token` 单一标量，gemini-embedding-2 按模态（image $0.45 / audio $6.50 / video $12.00 per 1M）无法表达。

**改动**：
- `Deployment` 增 `modal_pricing: Option<{image, audio, video}>`（从 model_info 提取）
- `calc_spend` 增重载：多模态输入按 `∑ per-modal tokens × modal price`；无配置回退标量
- embeddings.rs 透传多模态 input 标记（input 数组元素带 modal 类型）

**TDD**: ~4 UT
| # | Test | 断言 |
|---|------|------|
| 1 | 无 modal_pricing 回退标量 | 现有行为不变 |
| 2 | 单模态 image 计费 | image tokens × $0.45/M |
| 3 | 混合 audio+video | 分别计价求和 |
| 4 | 未知模态回退标量 | 不 panic |

### 2.4 TD-011a: Playground 视频输入（可选）

**现状**：无视频支持；Qwen3.5+ 原生多模态含 video token id（`image_tokens.rs` 注释已确认）。

**改动**（可选，若工作量允许）：
- Playground 文件选择 `accept="video/*"` + 上传/预览
- 请求体序列化 `video_url`（OpenAI 侧）/ `video` content block（Claude 侧）
- log-viewer 渲染 `<video>`
- **token 估算留待真实负载**（TD-011a 剩余部分维持待触发）

**TDD**: fe-bdd 2 场景 × 3 viewports

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/image_tokens.rs` | 修改 | estimate_anthropic downsizing |
| `crates/aigw-core/src/deployment.rs` | 修改 | modal_pricing 字段 |
| `crates/aigw-core/src/resolver.rs` | 修改 | 提取 modal_pricing |
| `crates/aigw-server/src/routes/chat.rs` | 修改 | calc_spend 多模态重载调用 |
| `crates/aigw-frontend/src/lib/image.ts` | 修改 | HEIC/AVIF 转码 |
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 修改 | video 输入（可选）|
| `crates/aigw-frontend/src/components/log-viewer/utils.ts` | 修改 | video 渲染（可选）|

## 4. 验收标准

- [ ] `task test` UT 全绿（含 TD-011c/TD-012b 新增 ~7）
- [ ] `task bdd` mock BDD 全绿
- [ ] `task fe-build` + `task fe-bdd` 全绿（HEIC 转码 + 可选视频场景）
- [ ] Anthropic 超大图估算贴合官方缩放规则；多模态 embedding 按模态计费
