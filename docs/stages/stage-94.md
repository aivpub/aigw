# Stage 94: 后端 — 实体 spend 异步增量更新 + daily_spend 全维度补全 + 失败路径修复

**Phase**: 39 — Budget Reset 周期任务 + 配置  
**优先级**: P0  
**状态**: ⏳ 待开始  
**预估**: 12h  
**前置**: Stage 93（已完成，无代码交集）、Stage 90（已完成）、Stage 84（AsyncTask+Engine 已建成）

---

## 核心预期

1. **DB 层 `increment_*_spend` 方法**：`Database` 新增 4 个 `increment_key/user/team/org_spend()` 方法 × 3 方言（SQLite/MySQL/PG），`UPDATE <table> SET spend = spend + ? WHERE <pk> = ?`，原子增量操作。

2. **chat.rs 接入增量更新**：所有成功路径（非 streaming + streaming）在 `insert_spend_log` 后，用 `tokio::spawn` 异步事务批量更新所有关联实体的 spend。按 auth 中实际关联实体决定更新哪些（key 始终更新，user/team/org 非 NULL 时才更新）。

3. **v1_messages.rs 接入增量更新**：同 chat.rs。需要先从 key 查询中提取 `team_id` / `organization_id`（当前只提取 `token_hash` / `user_id`）。

4. **daily_spend 全维度补全**：所有 queue 调用从仅 `DailySpendKind::User` 扩展到 User / Team / Organization / EndUser / Agent 五个维度（只对有值的维度 queue）。

5. **失败路径 team_id/org_id 修复**：chat.rs 约 6 处 + v1_messages.rs 约 4 处失败路径 spend_log 构造中 `team_id: None / organization_id: None` 改为使用 auth 中的实际值。

6. **NaN 防御**：`BudgetEnforcer` 的 `max_budget_f64` 解析加 `f64::is_finite()` 检查。详见架构文档 §7。

---

## 设计要点

- **DB 增量用 `spend = spend + ?`**（非 `spend = ?` 赋绝对值），DB 层原子操作，无客户端读-改-写竞态。
- **entity spend 用 `tokio::spawn` 异步更新**：请求路径不阻塞。spawn 任务内用一个事务包裹所有实体 UPDATE——要么全成功要么全失败。透支窗口 ~ms 级可忽略。崩溃时最多丢一个 spawned task（下次请求补上）。
- **v1_messages.rs auth** 当前只有 `(token_hash, user_id)` 二元组，需改为 `(token_hash, user_id, team_id, organization_id)`。Key 对象已有 `key.team_id` / `key.organization_id`，只是没提取。
- **daily_spend 多维度**：在已有 `queue.queue(ds_log)` 后追加 clone + 改 entity_id/kind 后 queue，无需改 `daily_spend_queue.rs` 本身。
- **失败路径修复**：spend_log 构造时从 auth 取 team_id / org_id 替代硬编码 `None`。
- **NaN 防御**：`mb.is_finite() && mb > 0.0` 替代原 `mb > 0.0`。背景：IEEE 754 规定 `NaN > 0.0 = false` 导致预算检查静默失效；`inf` 同理。对齐 litellm 安全公告 GHSA-2rv4-xv66-fpjg。

---

## 方言差异与 real BDD 必要性

Stage 94 的核心变更 `UPDATE ... SET spend = spend + ?` 在三个方言中的语义一致（都是 DB 侧原子操作），但以下差异必须通过 real BDD 在三后端（SQLite/PG/MySQL）实际执行验证：

| 差异点 | SQLite | MySQL | Postgres |
|--------|--------|-------|----------|
| `spend` 列类型 | `REAL` | `DOUBLE` | `DOUBLE PRECISION` |
| 浮点精度 | 双精度 | 双精度 | 双精度 |
| UPDATE 影响行数 | `changes()` | `affected_rows` | `GET DIAGNOSTICS` |
| 主键定位方式 | B-tree | B-tree（InnoDB） | B-tree |
| `f64::NAN` 行为 | `NaN = ?` 不匹配任何行 | 同 | 同 |

虽然 `spend = spend + ?` 的基本语义一致，但浮点精度、零值边界、并发事务隔离级别在三方言中存在差异。mock BDD（sqlite::memory:）只能验证 SQLite，PG/MySQL 必须 real BDD。

---

## TDD

- **UT（~22）**：increment_spend key/user/team/org 各 1 + BudgetEnforcer NaN/Inf 2 + spend_log consistency + daily_spend multi-kind 2
- **BDD（mock）**：spend_tracking.feature 6 场景（key/user/team/org 增量、单 key 无关联实体、失败路径保留 ID、daily_spend 写入）
- **real BDD**：三后端（SQLite/PG/MySQL）spend_tracking 场景

---

## real BDD 场景（概览）

每个场景在 SQLite、PostgreSQL（testcontainers）、MySQL（testcontainers）三个后端各执行一遍。

### 场景 1：key spend 增量
创建带 spend=0 的 key → 发一次请求（cost=0.05）→ 验证 `key.spend = 0.05` → 再发一次 → 验证 `key.spend = 0.10`

### 场景 2：key + user + team + org 关联层级增量
创建 key（关联 user → team → org）→ 发请求 → 验证 key.spend/user.spend/team.spend/org.spend 都增加了相同 cost

### 场景 3：单 key 无关联实体
创建 key（user_id/team_id/org_id 均为 NULL）→ 发请求 → 验证只有 key.spend 增加，user/team/org 表无影响

### 场景 4：缺失实体行 UPDATE 影响 0 行
key 关联了一个不存在的 user_id → 发请求 → `UPDATE users SET spend = spend + ? WHERE user_id = ?` 影响 0 行但不报错（正确行为）→ 验证 key.spend 仍然正确更新

### 场景 5：失败路径保留 team_id/org_id
创建 key（关联 team）→ 发请求到无效 upstream → 4xx/5xx 失败 → 验证 spend_logs 中 team_id 为 key 的 team_id（非 NULL）

### 场景 6：daily_spend 多维度写入
创建 key（关联 user + team + org）→ 发请求 → 等待 daily_spend_queue drain → 验证 daily_user_spend / daily_team_spend / daily_organization_spend 三条记录都存在，end_user/agent 无记录因为未设置

---

## 验收门禁

- aigw-core lib 全绿 + Stage 94 新增 UT 22 全绿
- mock BDD spend_tracking 6 场景全绿（sqlite::memory:）
- **real BDD 三后端（SQLite/PG/MySQL）4 场景全部通过（硬性要求，任一失败不可交付）**
- 手动 curl：发请求 → key.spend 增长 → 下次 BudgetEnforcer 读到非零值
