# Stage 95: 后端 — duration 解析 + BudgetResetter AsyncTask + Budget CRUD + 启动 backfill + 配额层级约束

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 20h
**前置**: Stage 94（实体 spend 异步增量写入就绪，reset 才有意义）

---

## 核心预期

1. **duration 解析 + 标准化 reset_at 计算**：新增 `crates/aigw-core/src/budget/duration.rs`，解析 `budget_duration`（`30s`/`1h`/`24h`/`7d`/`30d`/`1mo` + 词别名 hourly/daily/weekly/monthly）为秒数；`compute_next_reset_at(duration, now, tz, reset_time)` 标准化对齐（24h→UTC 0 点、7d→周一 0 点、30d/1mo→月初 1 号 0 点）。

2. **BudgetResetter AsyncTask**：新增 `crates/aigw-core/src/budget/resetter.rs`，`impl AsyncTask`，`step_type()="budget_reset"`。`tick(db)` 扫过期记录（`WHERE budget_reset_at < now() OR (budget_reset_at IS NULL AND budget_duration IS NOT NULL)`）。`execute(db, step)` 批量 `UPDATE spend=0, budget_reset_at=compute_next(...)`。`tick_interval()`=60s。`steps_from_payload()` 支持手动 trigger。

3. **DB 层批量 reset**：`reset_spend_for_keys/teams/users/orgs` × 3 方言。

4. **Budget CRUD API**：`/budget/new|list|info|update|delete`（对齐 litellm）。

5. **配额层级约束（写入时校验）**：keys/users/teams/orgs 的 POST/PUT 端点新增校验——下级 `max_budget` 不得大于直接上级实体的 `max_budget`。详见架构文档 §3.4。

6. **启动期 backfill**：`main.rs` 调 `backfill_missing_reset_at(db)`。

7. **Engine 注册 + 配置**。

---

## 扫描目标

BudgetResetter::tick 扫描以下表寻找过期记录：

| 表 | budget_reset_at 来源 | budget_duration 来源 | 备注 |
|----|---------------------|---------------------|------|
| `virtual_keys` | 自身 `budget_reset_at` 列 | 自身 `budget_duration` 列 | 即 key |
| `teams` | 自身 `budget_reset_at` 列 | 自身 `budget_duration` 列 | |
| `users` | 自身 `budget_reset_at` 列 | 自身 `budget_duration` 列 | |
| `organizations` | `budgets` 表的 `budget_reset_at` 列（通过 `budget_id` JOIN）| `budgets` 表的 `budget_duration` 列 | org 自身无这些列 |

过期判定 SQL（keys/teams/users 为表内判断，orgs 需 JOIN）：

```sql
-- keys/teams/users: 表内判断
SELECT * FROM virtual_keys
WHERE budget_reset_at < now()
   OR (budget_reset_at IS NULL AND budget_duration IS NOT NULL)

-- orgs: 需 JOIN budgets 表
SELECT o.*, b.budget_reset_at, b.budget_duration
FROM organizations o
JOIN budgets b ON o.budget_id = b.budget_id
WHERE b.budget_reset_at < now()
   OR (b.budget_reset_at IS NULL AND b.budget_duration IS NOT NULL)
```

---

## budget_duration 配置入口

`budget_duration` 是**实体级属性**，在创建/编辑实体时配置，不是全局配置：

| 实体 | 配置位置 | 字段来源 | 提供者 |
|------|---------|---------|--------|
| keys | POST/PUT `/key/generate` `/key/update` | 自身 `budget_duration` 列 | 后端 API（Stage 95）|
| teams | POST/PUT `/team/new` `/team/update` | 自身 `budget_duration` 列 | 后端 API（Stage 95）|
| users | POST/PUT `/user/new` `/user/update` | 自身 `budget_duration` 列 | 后端 API（Stage 95）|
| orgs | budget 关联到 `budgets` 表 | budgets 表 `budget_duration` 列 | 后端 API（Stage 95）|

前端 UI（Stage 96）提供固定下拉选项：

```
budget_duration: [None ▼] [Daily (24h) ▼] [Weekly (7d) ▼] [Monthly (30d) ▼] [Custom...]
```

Custom 选时弹出文本框允许自由输入（`1h`/`3d` 等）。固定选项降低出错率，Custom 保留灵活性。

如果不配 `budget_duration`（NULL），该实体的 spend 永远不被 reset——适用"临时放量"或"不限周期"的场景。

---

## 配额层级约束

### 约束范围

只约束 `max_budget`（额度上限），不约束 `budget_duration`（周期）。详见架构文档 §3.5-3.6。

| 约束 | 是否执行 |
|------|---------|
| `child.max_budget ≤ parent.max_budget` | ✅ 写入时校验 |
| `child.budget_duration ≤ parent.budget_duration` | ❌ 不校验（各自独立） |

### 各层独立 Reset

每个实体按自己的 `budget_duration` 独立重置。**上级 reset 不触发下级 reset**：

```
User 7 天 reset → 只改 user.spend = 0，不动 key.spend
Key 30 天 reset → 只改 key.spend = 0，不动 user.spend
```

多级 max_budget 检查（Stage 97）保证即使 Key 周期长于 User，运行时仍不会绕过上级限制。

### 后期关联的处理

约束只在写入时校验"当前"的直接上级。后期关联变更时：如果新上级的 `max_budget` 小于当前 `max_budget`，更新操作被拒绝。reset 行为始终独立——关联变更不触发任何实体的 spend 重置。

---

## 方言差异与 real BDD 必要性

Stage 95 的批量 reset SQL 在三方言中存在以下差异：

| 差异点 | SQLite | MySQL | Postgres |
|--------|--------|-------|----------|
| 批量 UPDATE 语法 | `UPDATE t SET ... WHERE pk IN (...)` | 同 | 同 |
| 日期比较 | `datetime('now')` | `NOW()` | `NOW()` |
| `budget_reset_at` 类型 | `DATETIME` (TEXT) | `DATETIME` | `TIMESTAMPTZ` |
| NULL 保护子句 | `IS NULL` / `IS NOT NULL` | 同 | 同 |
| UPDATE 返回行数 | `changes()` | `ROW_COUNT()` | `GET DIAGNOSTICS` |
| 事务隔离级别 | SERIALIZABLE | REPEATABLE READ (InnoDB) | READ COMMITTED |
| JOIN UPDATE | 不支持 `UPDATE ... JOIN` | 支持 | 支持（`UPDATE ... FROM`） |

**关键风险**：orgs 的 reset 需要 JOIN budgets 表。SQLite 不支持 `UPDATE ... JOIN` 语法，必须用子查询 `UPDATE organizations SET spend=0 WHERE organization_id IN (SELECT ... FROM budgets WHERE ...)`。这个三方言差异仅在 real BDD 中暴露。

---

## TDD

- **UT（~22）**：duration 解析 8 + compute_next_reset_at 6 + reset_spend 4（keys/teams/users/orgs）+ backfill 2 + 层级约束 2 + 幂等 1
- **BDD（mock）**：budget_reset.feature 6 场景 + budget_constraint.feature 4 场景
- **real BDD**：三后端 10 场景

---

## real BDD 场景（概览）

每个场景在 SQLite、PostgreSQL（testcontainers）、MySQL（testcontainers）三个后端各执行一遍。

### 场景 1：key reset（基本流程）
创建 key（max_budget=10, budget_duration="24h", budget_reset_at=昨天 UTC 0 点, spend=8.5）→ trigger budget_reset job → 验证 key.spend=0, budget_reset_at 滚动到下一个 UTC 0 点

### 场景 2：NULL 保护——有 duration 但无 reset_at
创建 key（budget_duration="7d", budget_reset_at=NULL）→ 首次 tick 应补算并重置 → 验证 reset_at 不再为 NULL

### 场景 3：多实体批量 reset
创建 3 个 key2 个到期 1 个未到期 → trigger reset → 验证只有 2 个到期 key 的 spend=0，未到期 key 的 spend 不变

### 场景 4：org reset（JOIN budgets 表）
创建 org（关联 budget_duration="30d" 的 budget）→ trigger reset → 验证 org.spend=0, budget.budget_reset_at 滚动到下月 1 号

### 场景 5：层级约束——key > user 被拒
创建 user（max_budget=50）→ 尝试创建 key（max_budget=100, user_id=该user）→ 返回 400 "Key budget cannot exceed user budget"

### 场景 6：层级约束——key ≤ user 通过
创建 user（max_budget=100）→ 创建 key（max_budget=50, user_id=该user）→ 200 成功

### 场景 7：上级 reset 不级联下级
创建 user（max_budget=100, budget_duration="24h", bargain_reset_at=昨天 UTC 0 点）+ key（max_budget=30, budget_duration="30d, user_id=该user）→ 使 key.spend=25, user.spend=25 → trigger reset → 验证 user.spend=0, **key.spend 仍然为 25（未被重置）** → 再次发请求（cost=3）→ key 级检查：key.spend(25+3=28) < key.max_budget(30) → 通过 → user 级检查：user.spend(0+3=3) < user.max_budget(100) → 通过 → 请求放行 → 验证 key.spend=28, user.spend=3

### 场景 8：子长于父——key 独立 reset 不受 user 周期影响
创建 user（max_budget=100, budget_duration="7d"）+ key（max_budget=50, budget_duration="30d, user_id=该user）→ 两周期结束后 → trigger reset → 验证**key 和 user 都被各自独立的周期重置**（user 按其7d周期，key 按其30d周期，两者触发条件不同）→ 验证未到期的实体不被重置

### 场景 9：后期关联——更新 user_id 时约束检查
创建 user（max_budget=30）+ key（max_budget=50, user_id=NULL）→ 尝试更新 key 挂上该 user → 返回 400 "Key budget cannot exceed user budget" → 更新 key 的 max_budget 为 20 → 再次尝试挂上 user → 200 成功

### 场景 10：后期关联——变更上级后各自 reset 独立
创建 user 已重置（user.spend=0）+ key（spend=15, user_id=该user）→ trigger user reset（不触发 key reset）→ 验证 user.spend=0, key.spend=15（不变）→ 发请求 → key 级通过（15+1=16 < 50）→ user 级通过（0+1=1 < 100）→ 请求放行

---

## 验收门禁

- aigw-core lib + aigw-server lib 全绿
- mock BDD budget_reset 6 场景 + budget_constraint 4 场景全绿（sqlite::memory:）
- **real BDD 三后端（SQLite/PG/MySQL）10 场景全部通过（硬性要求，任一失败不可交付）**
- 手动验证：写 Key $100 挂 User $50 → 400 拒绝；写 Key $30 挂 User $50 → 200 通过
- 手动验证：创建 user(budget_duration="24h") + key(budget_duration="30d", user_id=该user) → trigger user reset → user.spend=0, key.spend 不变 → 发请求仍然受各自最大限额约束
