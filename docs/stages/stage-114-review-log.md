# Stage 114 Review Log

**Stage**: Stage 114（前端体验 — TD-009a/b 图片压缩 + TD-008a/b i18n 增强）
**Review Type**: Design + Code
**Review Date**: 2026-08-09
**Reviewer**: lead（RDD Loop 自审）

## Review Summary

设计审查看出 4 个实现风险（2 High + 1 Medium + 1 Low）全部在实现阶段处置；代码实现后自查无 Critical/High，1 个 Medium（test fixture 可行性）已通过窗口 override 解决。

## Design Review (Gate 2)

| Sev | Finding | 处置 |
|-----|---------|------|
| High | E2E 夹具 PNG_1PX(1x1) 太小，无法验证压缩体积下降 | 新增 tests/fixtures/large-photo.png（2400x2400 ~3.3MB，可压缩） |
| High | i18n 全懒加载会白屏（登录/401 依赖同步翻译） | en 同步 eager + 检测语言 eager + 另一语言 lazy chunk |
| Medium | tsconfig 无 strict；开全局 strict 波及 20+ 文件 | resources.d.ts 增广 CustomTypeOptions（不翻转 strict） |
| Low | fe-bdd 需 vite:5173 + mock；确认无后端可跑 | 已验证（webServer 自启 + mock **/* 拦截） |

## Code Review (Gate 4)

| Sev | Finding | 处置 |
|-----|---------|------|
| Medium | >24MiB body-limit 无法用真实 fixture 测试（sessionStorage 配额 ~5MB） | 组件读 `window.__AIGW_MAX_IMAGE_BODY__` 测试 override；E2E 设 1 byte + 上传 3.3MB 照片 → 真实触发 reject |
| Low | `no chat request` 断言受 cross-scenario lastChatBody 泄漏污染 | mockAllApis per-page reset captured bodies |
| Low | playwright-bdd 把 `(..)` 当 capture group → step 文本匹配失败 | 重写 step 文本避免括号 |
| Info | 中文首访（navigator=zh-CN 无 localStorage）需同步加载 zh bundle | i18n.init 后检测 `i18n.language` eager import detected-lang |

## Bonus fixes (TD-008b typed resources surfaced REAL bugs)
- 4 个缺失 i18n key（health.min / keys.deletedKeys / keys.tpmLabel+rpmLabel / drawer.tabDescription+tabParams）已补 en+zh
- 1 个拼写错误（dashboard.spend → dashboard.spendLogs）
- 5 个动态 t() 调用点 cast 为类型安全

## Verification
- 3 新 BDD 场景 × 3 viewports = 9/9 ✅
- i18n-switcher 9/9 ✅（中文首访 + 英文 + localStorage override）
- 全量 fe-bdd 回归（待确认）
- fe-build chunk 分包（zh-CN 25kB lazy）+ fe-lint（oxlint + tsc）✅

## Resolution Summary
Design: 4 (2 High + 1 Medium + 1 Low) — 全部解决
Code: 4 (1 Medium + 2 Low + 1 Info) — 全部解决
**All Critical/High Fixed**: Yes
