# Stage 114: 前端体验（TD-009a/b 图片压缩 + TD-008a/b i18n 增强）

**所属**: Phase 45（技术债清理）
**预估**: 10h（前端 + 测试）
**依赖**: 无（TD-009 与 TD-008 独立可并行）

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
- `i18n/index.ts`：`useSuspense` + 懒加载 `import('../locales/en.json')`/`zh-CN.json`
- Vite 自动 code-split 成独立 chunk，首屏只载当前语言

**TDD**: `task fe-build` 产物确认分包（多 JS chunk）+ 全量 fe-bdd 回归 333 pass

### 2.3 TD-008b: 翻译 TS 类型

**改动**：脚本从 `en.json` 生成 `resources.d.ts`（`i18next-resources-for-ts` 或自定义），使 `t('key')` 有编译期校验 + IDE 补全。

**TDD**: `tsc -b` 严格模式通过（含新类型约束）

## 3. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-frontend/src/pages/playground/index.tsx` | 修改 | compressImage + 超限拦截 |
| `crates/aigw-frontend/src/lib/image.ts` | 新增 | 压缩工具 |
| `crates/aigw-frontend/src/i18n/index.ts` | 修改 | 懒加载 |
| `crates/aigw-frontend/src/i18n/resources.d.ts` | 新增 | TS 类型 |
| `crates/aigw-frontend/tests/features/playground.feature` | 修改 | +2 场景 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | 修改 | 捕获压缩后 data URL |

## 4. 验收标准

- [ ] `task fe-build` 产物分包（≥2 JS chunk）
- [ ] `task fe-lint` + `tsc -b` 通过
- [ ] `task fe-bdd` 全量回归 333+ pass（含新增 3 场景 × 3 viewports）
- [ ] Playground 上传 4K 图压缩后体积显著下降；超 24MB 拒绝
