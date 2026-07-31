# Stage 95: 前端 — 实体表单内联 budget 配置 + budget_reset Job Tab 补全

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: ⏳ 待开始（从原 Stage 92 推后）
**预估**: 16h
**前置**: Stage 94（后端 API + AsyncTask 契约就绪）

---

## 核心预期

1. **keys 表单内联 budget**（`src/pages/keys/index.tsx`）：create/edit Dialog 增 `budget_duration` 下拉（None / Daily / Weekly / Monthly / Custom 四档）+ `soft_budget` 输入。列表 `Budget` 列展示 `max_budget / duration`。

2. **teams / users / orgs 表单内联**：同 keys，加 `budget_duration` 下拉 + `soft_budget`。

3. **budget_reset Job Tab 补全**：替换占位为 `BudgetResetPanel`——统计卡 + Trigger 按钮（按 entity_type 选 keys/teams/users/orgs/all）。

---

## BDD

- budgets.feature 8 场景 × 3 viewports
- jobs.feature 增 3 场景

---

## 验收门禁

- frontend build green + TypeScript noEmit 零错误
- Playwright BDD：budgets.feature 8 场景 × 3 viewports + jobs 新增 3 场景全绿
