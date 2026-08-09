# Phase 45 规划：技术债清理（Tech Debt Cleanup）

**日期**: 2026-08-09
**背景**: Phase 44 全部完成（116/116 Stages）+ 在途 P1 收尾（Responses 稳定 + TD-006/TD-007）全部交付。剩余技术债需按风险/依赖/现实性排序收敛为可交付 Stage。

---

## 1. 现状核实结论

| TD | 可解性 | 核实结论 |
|----|--------|----------|
| **TD-004** BDD @real_api 键泄漏 | ✅ **已解决**（`b199000` 2026-07-20）| before/after_scenario hook 已在 `real_api_steps.rs`，账本刚补标 Resolved（`cb84341`）|
| **TD-005** Async Engine panic + shutdown | ✅ 可解 | `engine.rs` 三 loop 无 `catch_unwind`；`Engine::run` 无 CancellationToken。`tokio-util 0.7.19`（含 CancellationToken）已在 registry 缓存；main.rs 已有 `shutdown_signal()` 可复用 |
| **TD-010a** health embedding 探测 | ✅ 可解 | `health.rs:266 run_and_save_health_check` 对所有模型 POST `/chat/completions`，无 `model_info.mode` 分支。resolver 已能从 model_info 取 mode |
| **TD-003** BDD 覆盖率报告 | ✅ 可解 | 纯脚本/工具：scenario → 端点映射 + 覆盖率报告 |
| **TD-009a/b/e** 图片压缩 + body 防御 + 外链 | ✅ 可解（前端为主）| Playground canvas 压缩 + 请求体估算 + 外链缩略图代理 |
| **TD-008a/b/c/d** i18n 增量 | ✅ 可解（纯前端）| 懒加载 / TS 类型 / 后端错误多语言 / RTL |
| **TD-011a** 视频 token 估算 | ⚠️ **重定义** | 当前协议路径无视频 token 分解；更有价值的是先支持 Playground 视频输入 |
| **TD-011b** HEIC/AVIF 解析 | ⚠️ **方案变更** | HEIC/AVIF 是 ISO-BMFF 容器（递归 box 解析复杂度高），且 Chrome/Firefox 不渲染 HEIC → 改为**前端转码 JPEG/WebP**（一举解决上传/预览/估算）|
| **TD-011c** Anthropic downsizing | ⚠️ 可解 | 在 `estimate_anthropic` 加"超出 max_tokens 自动缩放"（target=1568）规则，纯 core 改动 |
| **TD-012b** 多模态 embedding 计费 | ⚠️ 可解（中等）| Deployment 定价标量 → 按模态映射 + calc_spend 感知 |
| **TD-008/009/011** 综合 | 视使用量 | 部分条目明确"视使用量触发" |

---

## 2. Stage 拆分（3 Stage，串行依赖）

### Stage 113：后端可靠性加固（TD-005 + TD-010a + TD-003）— 8h

| 子项 | 内容 |
|------|------|
| **TD-005** Async Engine 容错 + 优雅关闭 | ① 三个 loop（tick/exec/cleanup）体用 `std::panic::AssertUnwindSafe + catch_unwind` 包裹：panic 时 log + sleep 30s + 继续，防 loop 静默死亡；② `Engine::run` 接收 `CancellationToken`（`tokio-util` sync），`tokio::select!` 监听，优雅退出前等待 in-flight step；③ main.rs 把 `shutdown_signal()` 转成 CancellationToken 传入。TDD: ~6 UT（panic 恢复 / token 取消 / 正常退出） |
| **TD-010a** health embedding 探测 | `run_and_save_health_check` 在 resolve 后读 `deployment.raw_params` / model_info `mode`；`mode=="embed"` 分支：POST `{api_base}/embeddings` `{model, input:["ping"]}`，Auth Bearer；非 embed 保持现有 chat 探测。TDD: ~4 UT + 1 BDD |
| **TD-003** BDD 覆盖率报告 | 脚本 `scripts/bdd-coverage.rs`（或 shell）：解析 `tests/features/*.feature` 场景 → 匹配 handler 路由表 → 输出未覆盖端点 + 覆盖率%。接 `task bdd-coverage`。TDD: 脚本自测 1 |

**依赖**: TD-010a 需 resolver 已暴露 mode（已具备）。TD-005 独立。TD-003 独立。
**门禁**: `task test` + `task bdd` + fmt + lint。

### Stage 114：前端体验（TD-009a/b + TD-008a/b）— 10h

| 子项 | 内容 |
|------|------|
| **TD-009a** 图片压缩 | Playground 上传前 canvas 压缩（最长边 2048px + JPEG 0.8），降 base64 体积与 token 成本；粘贴图片同路径。TDD: fe-bdd 2 场景 × 3 viewports |
| **TD-009b** 超大图 body 防御 | 压缩后估算 `∑ data URL 长度`，超限（>24 MiB）前端提示拒绝；后端已有 `request_body_limit_mb=32` 兜底。TDD: fe-bdd 1 场景 |
| **TD-008a** 翻译懒加载 | i18next 按命名空间动态 `import()` 拆包，首屏减体积。TDD: build + fe-bdd 回归 |
| **TD-008b** 翻译 TS 类型 | 从 en.json 生成 `resources.d.ts`，`t('key')` 编译期校验 + IDE 补全。TDD: tsc 通过 |

**依赖**: 无（纯前端，TD-009 与 TD-008 独立）。
**门禁**: `task fe-build` + `task fe-lint` + `task fe-bdd`。

### Stage 115：多模态精度（TD-011 重定义 + TD-012b）— 10h

| 子项 | 内容 |
|------|------|
| **TD-011b→转码** | Playground 支持 HEIC/AVIF：上传经 canvas 转码 JPEG/WebP（浏览器无法解码时提示）；`RASTER_MIME` 加 `image/heic`/`image/avif` 走转码路径；引擎侧维持 PNG/JPEG/WebP/GIF 解析（转码后天然覆盖）。TDD: fe-bdd 2 场景 + core UT（HEIC 输入拒后转码断言） |
| **TD-011c** Anthropic downsizing | `estimate_anthropic` 加"超出 max_tokens 自动缩放"：像素超阈值时按 target=1568 tokens 反推缩放后重算。TDD: ~3 UT |
| **TD-012b** 多模态 embedding 计费 | Deployment 增可选 `modal_pricing: {image, audio, video}`（model_info 扩展），`calc_spend` 感知多模态输入 → 按模态单价计费；无配置回退标量。TDD: ~4 UT |
| **TD-011a→视频输入**（可选，工作量高）| Playground 视频上传/发送（mp4/webm，`accept="video/*"`）+ 请求体 `video_url` 序列化 + log-viewer 渲染。token 估算留待真实负载。TDD: fe-bdd 2 场景 × 3 viewports |

**依赖**: 无硬依赖；011 转码与 012b 独立，011c 依赖 011b 转码后输入。
**门禁**: `task test` + `task bdd` + `task fe-build` + `task fe-bdd`。

---

## 3. 交付顺序决策

- **Stage 113 优先**：纯后端、改动收敛、风险低（TD-005 生产稳定性 + TD-010a 修复 embedding 误报 + TD-003 工具），立即释放价值。
- **Stage 114 次之**：前端体验优化，fe-bdd 可独立验证。
- **Stage 115 最后**：多模态精度，含两个"方案变更"（HEIC 转码、视频输入重定义），依赖真实使用场景验证。

## 4. 后续跟进（Stage 113-115 完成后）

- TD-008c（后端错误多语言）、TD-008d（RTL）、TD-009e（外链缩略图代理）、TD-011a 视频 token 估算（等真实视频负载）→ 视使用量触发。
- 长期路线 LT-BodyMetrics / LT-BodyCompact / LT-BodyLifecycle 视数据量触发。

## 5. 验收标准（跨 Stage）

- `task test` / `task bdd` / `task fe-bdd` 全绿（Stage 115 新增场景 × 3 viewports）
- TD-005：Engine panic 后 loop 存活（UT 断言）+ Ctrl+C/SIGTERM 优雅退出（log 断言）
- TD-010a：embedding-only 模型 health check 走 `/embeddings` 不再 400（BDD 断言 healthy）
- TD-003：`task bdd-coverage` 输出覆盖率报告（≥90% 端点覆盖）
- TD-009a/b：Playground 上传大图压缩后体积显著下降 + 超限拦截
- TD-008a/b：`npm run build` 产物分包 + `tsc` 严格模式通过
- TD-011b：HEIC/AVIF 上传 → 转码 → 预览 + token 估算命中
- TD-011c：Anthropic 大图估算贴合官方缩放规则
- TD-012b：多模态 embedding 按模态单价计费（UT 断言）
