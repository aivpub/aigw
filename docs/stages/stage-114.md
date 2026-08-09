# Stage 114: 前端体验（TD-009a/b 图片压缩 + TD-008a/b i18n 增强）

**所属**: Phase 45（技术债清理）
**预估**: 10h（前端 + 测试）
**依赖**: 无（TD-009 与 TD-008 独立可并行）
**状态**: ✅ 完成（2026-08-09）

---

## 1. 目标

前端两项技术债收敛为一个 Stage，均纯前端、fe-bdd 可独立验证：
1. **TD-009a/b** — Playground 图片上传压缩（canvas）+ 超大图请求体防御
2. **TD-008a/b** — i18n 翻译懒加载 + 翻译 TS 类型安全

## 2. 核心设计

### 2.1 TD-009a: 图片上传压缩

**现状**（`playground/index.tsx`）：`FileReader.readAsDataURL` 直传 base64，无压缩；`RASTER_MIME = /^image\/(png|jpe?g|gif|webp)$/i`，`MAX_IMAGE_BYTES = 20MB`。

**改动**：
- 新增 `compressImage(file): Promise<File>` 工具：`createImageBitmap`/`Image` → canvas → `toBlob('image/jpeg', 0.8)`，最长边 2048px 等比缩放；透明 PNG 保留 PNG
- 上传/粘贴路径统一走压缩：压缩后体积 < 原图且仍满足 RASTER_MIME 则用压缩结果
- 压缩后估算 `∑ data URL 长度`，超限（>24 MiB）前端 toast 提示并拒绝（`MAX_REQUEST_BODY = 24MB`）
- 后端 `request_body_limit_mb=32` 已兜底（Stage 已有）

**TDD**: fe-bdd 3 场景 × 3 viewports = 9 执行
| 场景 | 断言 |
|------|------|
| 上传大图（>2048px）压缩后体积下降 | 捕获的 data URL 长度 < 原图 |
| 超大图（>24MB 压缩后仍超）被拒绝 | toast + 无请求发出 |
| 小图不压缩原样发送 | 保持原数据 URL |

### 2.2 TD-008a: 翻译懒加载

**现状**：所有翻译 bundle 在单一 JS chunk（`i18n/index.ts` 同步 `import en/zh-CN`）。

**改动**：i18next 按命名空间动态 `import()` 拆包：
- `i18n/index.ts`：en 同步 eager（首屏登录/401 不白屏）+ **检测语言 eager**（`navigator=zh-CN` 首访同步加载 zh bundle）+ 另一语言动态 `import()`（Vite code-split 独立 chunk）
- `useSuspense` 不启用（保持 init 同步）

**TDD**: `task fe-build` 产物确认分包（多 JS chunk）+ 全量 fe-bdd 回归

### 2.3 TD-008b: 翻译 TS 类型

**改动**：`scripts/fe-i18n-types` 从 `en.json` 生成 `resources.d.ts`（增广 `i18next.CustomTypeOptions`），使 `t('key')` 编译期校验 + IDE 补全；**不翻转全局 strict**（避免波及 20+ 文件既有错误）。

**TDD**: `tsc -b` 通过（含新类型约束）

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 修改 | compressImage 接入 + 超限拦截 + `window.__AIGW_MAX_IMAGE_BODY__` 测试 override |
| `crates/aigw-frontend/src/lib/image.ts` | 新增 | `compressImage` + `dataUrlBytes` 压缩工具 |
| `crates/aigw-frontend/src/i18n/index.ts` | 修改 | 懒加载（en eager + detected-lang eager + other-lang lazy） |
| `crates/aigw-frontend/src/i18n/resources.d.ts` | 新增 | TS 类型（~874 keys，`scripts/fe-i18n-types` 生成） |
| `scripts/fe-i18n-types` | 新增 | resources.d.ts 再生脚本（Taskfile fe-lint 前置） |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 修改 | +5 key（health.min/keys.deletedKeys/keys.tpmLabel+rpmLabel/drawer.tabDescription+tabParams） |
| `crates/aigw-frontend/src/pages/{dashboard,budgets,usage,spend-logs}/index.tsx` 等 5 文件 | 修改 | 动态 t() 类型安全 cast（TD-008b）+ dashboard.spend 拼写修复 |
| `crates/aigw-frontend/tests/features/playground.feature` | 修改 | +3 场景（压缩/小图原样/超大拒绝） |
| `crates/aigw-frontend/tests/steps/{playground.steps,api-mocks}.ts` | 修改 | 大图夹具 + 压缩断言 + per-page body reset |
| `crates/aigw-frontend/tests/fixtures/large-photo.png` | 新增 | 2400x2400 ~3.3MB 可压缩 PNG 夹具 |
| `Taskfile.yml` | 修改 | fe-lint 加 fe-i18n-types + tsc |

## 4. 验收标准

- [x] `task fe-build` 产物分包（zh-CN 独立 lazy chunk 25kB + main chunk）
- [x] `task fe-lint`（fe-i18n-types 再生 + oxlint + tsc -b）通过
- [x] `task fe-bdd` 全量回归通过（含新增 3 场景 × 3 viewports = 9 执行）
- [x] Playground 上传 2400x2400 图压缩后体积显著下降；超限（override 模拟）拒绝 toast 无请求

---

## Implementation Notes

### Implementation Differences（vs 设计）
| 设计 | 实际 | 原因 |
|------|------|------|
| `compressImage(file): Promise<File>` | `compressImage(file): Promise<CompressResult>`（返回 data URL + 原始/压缩体积，便于断言） | E2E 需比较压缩前后 data URL 长度 |
| `useSuspense` + 全懒加载 | en 同步 eager + 检测语言 eager + other-lang 动态 import | 登录页/401 跳转依赖同步翻译；`navigator=zh-CN` 首访不能白屏 |
| 超大图（>24MB）真实夹具 | `window.__AIGW_MAX_IMAGE_BODY__` 测试 override（设 1 byte + 上传 3.3MB 照片触发 reject） | sessionStorage 配额 ~5MB，无法构造 >24MB data URL |
| `i18next-resources-for-ts` 库 | 自写 `scripts/fe-i18n-types`（python 生成） | 非项目依赖，自生成更可控 |
| `tsc -b 严格模式` | `tsc -b` 默认模式 + CustomTypeOptions 增广（不翻转全局 strict） | 全局 strict 会暴露 20+ 文件既有错误，超出 Stage 范围 |

### Technical Decisions Made
- **压缩策略「两者取其小」**：compressImage 返回 `min(原图, 压缩后)` —— 小图（1x1 PNG）保持原样保真，仅大图（>2048px 且 JPEG 更小）降采样。透明 PNG 白底填色转 JPEG。
- **i18n 检测语言 eager**：`i18n.init` 后若 `i18n.language`（检测结果）无 bundle，动态 import detected-lang —— 中文首访（navigator=zh-CN 无 localStorage）首帧即中文。
- **typed resources 暴露 5 个真实 bug**：health.min / keys.deletedKeys / keys.tpmLabel+rpmLabel / drawer.tabDescription+tabParams 缺失（补 en+zh）+ dashboard.spend 拼写错误（→spendLogs）。
- **动态 t() 类型安全**：5 处 `t(变量)` cast `as never`（sidebar labelKey / jobs STEP_LABELS / usage PRESET_LABELS / spend-logs PRESET_LABEL_MAP / budgets durationLabel 改用模块 i18n.t 避免 TS2589 深递归）。
- **per-page captured-body reset**：`mockAllApis` 每页重置 `lastChatBody`，避免 cross-scenario 泄漏污染「no request」断言。

### Testing Evidence
- **fe-bdd**：3 新场景 × 3 viewports = 9/9 ✅（大图压缩 JPEG<2MB / 小图原样 PNG / 超大拒绝 toast+无请求）；i18n-switcher 9/9 ✅（中文首访/英文/localStorage override）；全量回归 ✅。
- **fe-build**：`zh-CN-*.js` 25kB lazy chunk 独立于 main（1,279 kB）——TD-008a 分包达成。
- **fe-lint**：fe-i18n-types 再生幂等 + oxlint（仅既有 warning）+ `tsc -b` 通过。

### Known Limitations
- `compressImage` 的 HEIC/AVIF 解码失败（非 Safari）→ 返回原图（上传路径 RASTER_MIME 已拒，TD-011b 前端转码留 Stage 115）。
- `window.__AIGW_MAX_IMAGE_BODY__` 是测试专用 override，生产默认 24MiB。
- typed `t()` 对变量 key 需 `as never` cast（编译期不能追踪变量字符串）。
