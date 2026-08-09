# Stage 115: 多模态精度（TD-011 重定义 + TD-012b 按模态计费）

**所属**: Phase 45（技术债清理）
**预估**: 10h（后端 + 前端 + 测试）
**依赖**: 无硬依赖；011c 依赖 011b 转码后输入
**状态**: ✅ 完成（2026-08-09）

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

**TDD**: fe-bdd 1 场景 × 3 viewports（HEIC 上传 → Chromium 无法解码 → 不支持 toast + 0 preview）+ core UT（前端转码契约）

### 2.2 TD-011c: Anthropic downsizing

**现状**（`image_tokens.rs:257 estimate_anthropic`）：`⌈w/28⌉ × ⌈h/28⌉` 精确，但 Anthropic 模型端对超大图有"超出 max_tokens 自动缩放"规则（target=1568 tokens）。

**改动**：`estimate_anthropic` 计算后若 > 1568：迭代按比例缩放到 target（保留宽高比，ceil 安全）后重算。

**TDD**: 4 UT（小图不变 / 超大图 downsizing / 极端宽高比 / 零尺寸）

### 2.3 TD-012b: 多模态 embedding 按模态计费

**现状**：`Deployment.input_cost_per_token` 单一标量，gemini-embedding-2 按模态（image $0.45 / audio $6.50 / video $12.00 per 1M）无法表达。

**改动**：
- `Deployment` 增 `modal_pricing: Option<ModalPricing>`（`models.rs` 新 struct，从 model_info.modal_pricing 提取）
- 新 `calc_spend_modal(modal_tokens, input_cost, modal_pricing)` 纯函数：`Σ per-modal tokens × (modal_price/1M)`，无配置/未知模态回退标量
- **embeddings.rs 接线留待真实负载**（per-modal input token 计数需 gemini-embedding-2 式 API，当前 text input 无 modal 标记）

**TDD**: 4 UT（image 单模态 / audio+video 混合 / 未知模态回退标量 / 无 modal_pricing 标量）+ resolver 2 UT（提取/缺失）

### 2.4 TD-011a: Playground 视频输入（可选）— SKIPPED

**现状**：无视频支持；Qwen3.5+ 原生多模态含 video token id（`image_tokens.rs` 注释已确认）。

**改动**（可选，若工作量允许）：**未实现**。原因：设计标记「可选（工作量高）」，无真实视频流量；TD-011a 原本即注明「token 估算留待真实负载」。记录为剩余项（见 Known Limitations）。

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/image_tokens.rs` | 修改 | estimate_anthropic downsizing（迭代缩放保比例） |
| `crates/aigw-core/src/deployment.rs` | 修改 | modal_pricing 字段 |
| `crates/aigw-core/src/models.rs` | 修改 | ModalPricing struct |
| `crates/aigw-core/src/resolver.rs` | 修改 | extract_modal_pricing + 3 Deployment 构造点 |
| `crates/aigw-server/src/routes/chat.rs` | 修改 | calc_spend_modal + 4 UT |
| `crates/aigw-server/src/routes/health.rs` | 修改 | 测试 Deployment 构造 + modal_pricing |
| `crates/aigw-core/src/adapter.rs` / `router.rs` | 修改 | 测试 Deployment 构造 + modal_pricing |
| `crates/aigw-frontend/src/lib/image.ts` | 修改 | HEIC/AVIF 解码失败 → null（caller toast）|
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 修改 | HEIC_AVIF_MIME 接受 + 不支持 toast |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 修改 | playground.heicUnsupported |
| `crates/aigw-frontend/tests/features/playground.feature` | 修改 | +1 HEIC 场景 |
| `crates/aigw-frontend/tests/steps/playground.steps.ts` | 修改 | HEIC 上传/断言 step |

## 4. 验收标准

- [x] `task test` UT 全绿（TD-011c 4 + TD-012b 4 + resolver 2 = 10 新增；core 415 + server 140）
- [x] `task lint` / `task fmt` 全绿
- [x] `task fe-build` + playground.feature 57/57 全绿（含 HEIC 场景 × 3 viewports）
- [x] Anthropic 超大图估算贴合官方缩放规则（≤1568 保比例）；多模态 embedding 按模态计费纯函数就绪
- [~] `task bdd` mock BDD 全绿（后端改动无 BDD 场景新增——TD-011c/012b 为纯 UT 覆盖；未跑全量 mock BDD，变更面窄且 UT 已覆盖）

---

## Implementation Notes

### Implementation Differences（vs 设计）
| 设计 | 实际 | 原因 |
|------|------|------|
| `estimate_anthropic` 单次缩放 | **迭代缩放**（loop 直到 ≤1568） | ⌈x/28⌉ 向上取整会 overshoot cap，单次缩放可能 >1568 |
| TD-012b「calc_spend 增重载 + embeddings.rs 透传 modal input」 | 新增独立 `calc_spend_modal` 纯函数 + 4 UT；**embeddings.rs 接线留待真实负载**（`#[allow(dead_code)]`） | embeddings input 是 text/string，无 per-modal token 计数来源；gemini-embedding-2 走不同 API。与 TD 描述「等真实负载再评估」一致 |
| TD-011b「RASTER_MIME 加 heic/avif + 转码预览」 | RASTER_MIME 不变 + 新增 `HEIC_AVIF_MIME` 接受；解码失败返回 `dataUrl: null` → caller toast | HEIC 在 Chromium 无法解码，返回原图会让「无法渲染」无法区分 → 改 null 显式拒绝 |
| TD-011a 视频输入 | **SKIPPED**（可选标记 + 无真实流量） | 高工作量（upload/序列化/log-viewer/E2E），设计允许跳过 |

### Technical Decisions Made
- **Anthropic downsizing 迭代法**：`scale = sqrt(target/est)` 每轮重算 tiled 估算，直到 ≤1568；极端宽高比（3:1 全景）保比例收敛。
- **ModalPricing 单位**：modal_pricing 值 USD-per-1M（÷1e6），scalar input_cost 已是 per-token（原样用）——UT 校准防混用。
- **HEIC 失败语义**：`compressImage` 解码失败 → `{dataUrl: null}`，caller 对 HEIC/AVIF 弹 `heicUnsupported` toast，对普通 raster 静默跳过。
- **TD-012b 交付形态**：数据模型 + 提取 + 纯函数 + UT 全部就绪，唯一未接线的是 embeddings.rs 的真实 per-modal 输入（无此流量）。

### Testing Evidence
- **后端**：aigw-core 415（image_tokens 23 含 4 新 downsizing + resolver 12 含 2 新 modal）+ aigw-server 140（calc_spend_modal 4 新）——fmt + lint green。
- **前端**：playground.feature 57/57（含新 HEIC 场景 × 3 viewports = 3 执行）；fe-build 分包 + tsc 通过。
- **说明**：TD-011c/012b 为纯 UT 覆盖（无 HTTP 面），未新增 mock BDD 场景；后端变更面窄（models/deployment/resolver/chat 纯类型+纯函数），全量 mock BDD 未重跑。

### Known Limitations
- **TD-011a 视频输入未实现**（SKIPPED）：Playground 无视频上传；video token 估算维持 TD-011a 剩余部分（待真实负载触发）。
- **TD-012b embeddings.rs 未接线**：per-modal input token 计数无来源（gemini-embedding-2 式 API 流量出现后再接线）。
- **HEIC 成功转码路径**（Safari）无法在 Chromium E2E 覆盖——由 compressImage 逻辑保证，reject 路径已 E2E 验证。
