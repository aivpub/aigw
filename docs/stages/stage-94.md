# Stage 94: 后端 — duration 解析 + BudgetResetter AsyncTask + Budget CRUD + 启动 backfill

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: ⏳ 待开始（从原 Stage 91 推后）
**预估**: 16h
**前置**: Stage 90（已完成，无代码交集）、Stage 84（AsyncTask+Engine 已建成，本 Stage 复用）

---

## 核心预期

1. **duration 解析 + 标准化 reset_at 计算**：新增 `crates/aigw-core/src/budget/duration.rs`，解析 `budget_duration`（`30s`/`1h`/`24h`/`7d`/`30d`/`1mo` + hourly/daily/weekly/monthly 词别名）为秒数；`compute_next_reset_at(duration, now, tz, reset_time)` 标准化对齐（24h→UTC 0 点、7d→周一 0 点、30d/1mo→月初 1 号 0 点、Nh/Nm/Ns→N 边界）。

2. **BudgetResetter AsyncTask**：新增 `crates/aigw-core/src/budget/resetter.rs`，`impl AsyncTask`，`step_type()="budget_reset"`。`tick(db)` 扫四表过期记录生成 step；`execute(db, step)` 批量 `UPDATE spend=0, budget_reset_at=compute_next(...)`；`tick_interval()`=60s；`steps_from_payload()` 支持手动 trigger。

3. **DB 层批量 reset**：`reset_spend_for_keys/teams/users/orgs` × 3 方言，`WHERE <pk> IN (...) AND budget_duration IS NOT NULL`，返回 reset 行数。

4. **Budget CRUD API**：`/budget/new|list|info|update|delete`（对齐 litellm）。keys/teams/users/orgs 创建更新端点透传 `budget_duration` + `soft_budget`，写入时若 duration 非 NULL 自动算 budget_reset_at。

5. **启动期 backfill**：`main.rs` 调 `backfill_missing_reset_at(db)`，补算「duration 非 NULL 且 reset_at IS NULL」的行。

6. **Engine 注册**：`main.rs` Engine 块新增 `engine.register(budget_resetter)`。

7. **配置**：`GeneralSettings` 增 `budget_reset: Option<BudgetResetConfig>`（enabled 默认 true / interval_secs 默认 60 / timezone 默认 UTC / reset_time 默认 00:00）。

---

## 背景

详见 `docs/research/2026-07-30-budget-reset-gap.md` §1-2。budgets 表 + 四实体表的 budget 列 Stage 1 就 schema 对齐，但从未实现周期 reset。Body Archive 的 AsyncTask+Engine（Stage 82-84）已建成可复用，前端 `KNOWN_STEP_TYPES` 已硬编码 `budget_reset` 但后端无实现。

---

## TDD

- **UT（~18）**：duration 解析 8 + compute_next_reset_at 6 + reset_spend 4 + backfill 2 + NaN 防御 1 + 幂等 1
- **BDD（mock）**：budget_reset.feature 6 场景
- **real BDD**：三后端 budget_reset 场景

---

## 验收门禁

- aigw-core lib 全绿 + Stage 94 新增 UT 18 全绿
- mock BDD budget_reset 6 场景全绿
- real BDD 三后端 budget_reset 场景全绿
