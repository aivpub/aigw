# Stage 96: 全栈联调 — soft/hard 双轨 + real BDD 三后端 + 收尾

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: ⏳ 待开始（从原 Stage 93 推后）
**预估**: 8h
**前置**: Stage 94（后端）+ Stage 95（前端）

---

## 核心预期

1. **soft/hard 双轨检查**：`BudgetEnforcer::check_budget` 扩展——`soft_budget` 超限记 tracing warn 日志但不拒绝，`max_budget` 超限拒绝（补 NaN 防御 `f64::is_finite`）。soft_budget 告警通道留 TD-007。

2. **周期任务端到端联调**：配 `budget_duration:"24h"` → trigger → spend 清零 → 请求放行。

3. **real BDD 三后端**：sqlite/pg/mysql 各跑完整 budget_reset 链路。

4. **文档收尾**：roadmap Phase 39 标记 ✅ + next-steps 总结 + tech-debt TD-007 + ADR-024。

---

## 验收门禁

- 全量回归：aigw-core lib + aigw-server lib + mock BDD + real BDD 三后端全绿
- 端到端手动验证通过
- 四份文档同步更新
