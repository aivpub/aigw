# Stage 96: 前端 — 实体表单内联 budget 配置 + budget_reset Job Tab 补全

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 16h
**前置**: Stage 95（后端 API + AsyncTask 契约就绪）

---

## 核心预期

1. **keys 表单内联 budget**（`src/pages/keys/index.tsx`）：create/edit Dialog 增 `budget_duration` 下拉（None / Daily / Weekly / Monthly / Custom 四档，Custom 时弹文本框）+ `soft_budget` 输入。列表 `Budget` 列展示 `max_budget / duration`。

2. **teams / users / orgs 表单内联**：同 keys，加 `budget_duration` 下拉 + `soft_budget`（orgs 当前只有 budget_id 只读，补 max_budget + budget_duration 内联；budget_id 关联留后续）。

3. **budget_reset Job Tab 补全**（`src/pages/jobs/index.tsx`）：把 `BudgetResetPlaceholder` 替换为 `BudgetResetPanel`——统计卡（待重置数 / 上次重置时间 / 下次预计）+ 手动 Trigger 按钮（按 entity_type 选 keys/teams/users/orgs/all）。复用 `triggerJob({step_type:"budget_reset", payload:{entity_type, entity_ids?}})`。

---

## 设计要点

- **下拉四档**：降低出错率，Custom 档允许自由输入（1h / 3d 等）。
- **budget_reset_at 不暴露**：用户配周期，后端算时刻。
- **复用现有组件**：Select / Input / Dialog / Table / Badge / TriggerDialog 全有。
- **daily_spend 多维度不影响前端**：Usage 页面查询接口不变。

---

## BDD

- budgets.feature 8 场景 × 3 viewports（keys/teams/users/orgs budget_duration 下拉 + 提交 + 列展示）
- jobs.feature 增 3 场景（budget_reset Tab trigger + 统计卡 + 手动 trigger）

---

## 验收门禁

- frontend build green + TypeScript noEmit 零错误
- Playwright BDD：budgets.feature 8 场景 × 3 viewports + jobs 新增 3 场景全绿
