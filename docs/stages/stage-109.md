# Stage 109: 预算重置 · cron 界面重构 — 统计端点 + 面板 + 预览 + 触发 Dialog

**Phase**: 39 补充 — Budget Reset UI 友好化
**优先级**: P0（用户反馈「预算重置 · cron」界面不友好）
**状态**: ✅ 完成
**预估**: 8h
**前置**: Stage 94-97（Budget Reset 后端 + 前端 tab 已就绪）

---

## 核心预期

把 Jobs 页「预算重置」sub-tab 从占位改为「决策就绪」管理视图：

1. **新端点 `GET /admin/budget-reset/stats`**：返回 per-entity-type ready/total 计数、即将重置 preview（上限 ~10/类）、上次终态重置 job、诚实近似的 `next_tick_at`。复用现有扫描谓词 + count_* 助手，无 schema 变更。
2. **BudgetResetStatsCard**：hero 卡 —— 自动重置状态 Badge / 待重置总数（`ready_total`）/ 上次重置时间（无则「从未重置」）/ next-tick 倒计时（由 `next_tick_at` 每次 30s 轮询重校准，不再用本地假 setInterval）。
3. **BudgetResetPreview**：分实体迷你块（密钥/团队/用户/组织 ready/total，可点击过滤）+ 即将重置表（类型 Badge / 别名 / 周期 / 已用/上限 / 上次重置时间），空态文案。
4. **BudgetResetTriggerDialog**：范围 segmented 控制（所有/密钥/用户/团队/组织）+ 实时估算「将重置 N 个实体」+ 确认 → POST `/admin/jobs/trigger` → 成功跳转 job 详情。`ready_total===0` 时禁用确认。
5. **job 表 trigger 列本地化**（cron→定时 / manual→手动）+ job 详情标题同理；`formatStepResult` 增加 budget_reset 分支渲染 `new_reset_at`/`reset_at_utc` 等，不再截断 JSON。

## 设计要点

- **展示真实状态，不展示虚构**：所有数字来自后端扫描（每 60s 已运行）或 job 历史；`next_tick_at = Utc::now() + BUDGET_RESET_TICK_INTERVAL`（引擎无 last-tick 心跳，前端标注「约每 60 秒」）。
- **新 core 函数**（`crates/aigw-core/src/budget/resetter.rs`）：`BUDGET_RESET_TICK_INTERVAL` 常量（`tick_interval()` 复用，单一事实源）；`count_expired_resets`（带 `!= ''` 守卫，org 走 budgets JOIN，复用 `now_func` 三方言）；`preview_expired`（展示列 + 方言化 `budget_reset_at` 文本 + `ORDER BY alias LIMIT ?`）；`budget_reset_stats` 编排。
- **前端组件** 新建于 `pages/jobs/components/`：`budget-reset-stats-card.tsx` / `budget-reset-preview.tsx` / `budget-reset-trigger-dialog.tsx`；`index.tsx` 删内联 `BudgetResetTrigger` + 假倒计时 `BudgetResetPanel`。
- **TOCTOU**：预览与触发之间 cron tick 可能入队，`create_job` 对活跃 job 去重不会双重置，估算标注「约」。

## TDD

- **core UT +4**：`count_expired_resets` 空库 / 混合状态（含空字符串 duration 守卫）；`preview_expired` 返回别名非 hash + max_budget 解析；`budget_reset_stats` 四类合并计数 + preview 类型齐全。
- **后端 real BDD +2**：`budget-reset stats` 返回 ready 计数与 preview；非 admin 401。
- **前端 BDD +3**：概览统计 + 即将重置预览；空数据空态；触发前预览确认（范围→估算→确认→POST→跳转）。

## 验收门禁

- aigw-core lib 全绿（371 passed）+ 新 4 UT
- mock BDD 215 passed（含 2 新场景）
- frontend build + lint 绿（仅既有 fixtures.ts 2 error 属预存）
- jobs.feature 87 passed × 3 viewports + dashboard/i18n 42 passed
- 手动 curl `/admin/budget-reset/stats` 返回 counts/preview/last_reset/next_tick_at

## 关键文件

- `crates/aigw-core/src/budget/resetter.rs`（+count/preview/stats fns）
- `crates/aigw-server/src/routes/jobs.rs` / `main.rs`（stats handler + 路由）
- `crates/aigw-frontend/src/pages/jobs/{index.tsx, job-detail.tsx, components/}`（3 新组件）
- `crates/aigw-frontend/src/lib/api/jobs.ts` / `i18n/locales/{zh-CN,en}.json`
- `crates/aigw-frontend/tests/{features/jobs.feature, steps/{jobs.steps, api-mocks}.ts}`
- `crates/aigw-server/tests/{features/budget_reset.feature, bdd_steps/{budget_reset_steps,common}.rs}`
