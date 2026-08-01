# Budget Reset 周期任务 — 架构与实现规划

> **项目**: aigw (AI Gateway — litellm proxy Rust 最小兼容替代)  
> **日期**: 2026-08-01  
> **状态**: Phase 39 最终规划。整合自原调研 `docs/research/2026-07-30-budget-reset-gap.md`、初版汇报 `docs/research/2026-08-01-budget-reset-architecture-report.md`、v2 `docs/research/2026-08-01-budget-reset-architecture-v2.md`、v3 `docs/research/2026-08-01-budget-reset-architecture-v3.md`，以上四份文档均作废。原 Phase 37 规划 `docs/plans/2026-07-30-budget-reset-phase-37.md` 亦作废，以本文档为准。

---

## 1. 背景与问题

### 1.1 当前架构的完整写入路径（代码实况）

每次 API 请求完成后，aigw 执行以下写入：

```
┌─────────────────────────────────────────────────────────────────────┐
│ 请求完成后的写入矩阵（当前）                                          │
│                                                                      │
│ ① INSERT spend_logs         同步，请求路径直接执行                    │
│    chat.rs:1744 / v1_messages.rs:1114                                │
│    字段：api_key, "user", team_id, organization_id, spend,           │
│           prompt_tokens, completion_tokens, ...                      │
│                                                                      │
│ ② daily_spend_queue.queue() 异步，非阻塞 channel → 后台 10s drain   │
│    chat.rs:1772-1805 / v1_messages.rs:1142-1175                      │
│    ⚠️ 当前只写 DailySpendKind::User（硬编码）                        │
│    缺失：Team / Organization / EndUser / Agent / Tag                 │
│                                                                      │
│ ③ UPDATE 实体表.spend        ❌ 未实现                               │
│    keys / teams / users / organizations 的 spend 列从未更新          │
│    → BudgetEnforcer 读到的永远是 0.0                                 │
│    → 预算超限保护完全失效                                            │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.2 spend 存在三层，各自独立

| 层 | 表 | 写入 | 读取 | 重置 | 目的 |
|----|-----|------|------|------|------|
| 审计流水 | `spend_logs` | 同步 INSERT | `SUM(spend)` 实时聚合 | 永不重置 | 账单/审计/历史查询 |
| 分析聚合 | `daily_*_spend` | 异步 channel 批量 upsert | 预聚合查询 | 永不重置 | Usage 趋势图/按模型统计 |
| 配额计数 | 实体表 `spend` 列 | ❌未实现 → 异步事务 UPDATE | BudgetEnforcer | **周期清零** | 配额门禁 |

这三层互相独立，缺一不可：
- 没有审计流水 → 看不到历史账单
- 没有分析聚合 → 趋势图全表扫描太慢
- 没有配额计数 → 预算限制无效

### 1.3 当前缺失项汇总

| 缺失项 | 影响 | 优先级 |
|--------|------|--------|
| 实体表 spend 列未更新 | 预算检查永远不触发 | P0 |
| daily_spend 只写 User 维度 | Team/Org/EndUser/Agent/Tag 分析不可用 | P0 |
| Team/Org 级历史用量查询 | 只能查 key 和 user 的总量 | P1 |
| 周期重置逻辑 | 配额无法恢复 | P0 |
| 失败路径 team_id/org_id 写为 None | 失败请求的实体关联丢失 | P1（本次顺修） |
| 多层级配额检查 | 上级超限不拒，容易出现 parent 超了 child 还能用 | P1 |
| 下级配额 ≤ 上级约束 | 配置了相互矛盾的额度无法发现 | P1 |

---

## 2. 写入策略

### 2.1 决策：entity spend 用 `tokio::spawn` 异步事务更新，daily_spend 保持 channel 异步

```
                     │  同步/异步            │  理由
─────────────────────┼──────────────────────┼────────────────────
INSERT spend_logs    │  同步                 │  审计，不可丢
entity spend UPDATE  │  tokio::spawn 异步    │  请求零延迟，事务包裹
daily_spend queue    │  异步（channel）       │  10s 批量 upsert，已实现
```

**为什么 tokio::spawn 而非同步**：spend_logs INSERT 完成后立即返回响应，entity UPDATE 在 spawned task 中用事务包裹——要么全成功（key/user/team/org 一起更新），要么全失败。请求路径零延迟增加。透支窗口 ~ms 级（两次并发请求间的调度间隙），远小于网络延迟和 upstream latency，可接受。进程崩溃时最多丢失一个 spawned task（下次请求补上）。

**为什么 tokio::spawn 而非 channel 队列**：daily_spend_queue 是 10s 批量 drain，延迟太大（响应返回 10s 后配额才更新），不适合配额门禁。tokio::spawn 立即执行。

**为什么用事务**：key + user + team + org 的 spend 更新在一个 DB 事务里（`spend = spend + ? WHERE pk = ?`）。要么全成功要么全失败，不会出现"key 更新了但 team 没更新"的半一致状态。

---

## 3. 实体层级关系与配额模型

### 3.1 数据关系（当前 Schema）

```
virtual_keys:         user_id (NULLABLE), team_id (NULLABLE), organization_id (NULLABLE)
users:                team_id (NULLABLE), organization_id (NULLABLE)
teams:                organization_id (NULLABLE)
organizations:        budget_id (NOT NULL → 引用 budgets 表)

spend_logs:           api_key + "user" + team_id + organization_id
                      每个请求的所有实体 ID 都写在日志行里
```

**所有关系都是 NULLABLE，没有强制层级**：
- Key 可以不关联任何 User
- User 可以不关联任何 Team
- Team 可以不关联任何 Org

### 3.2 实体配额字段

| 实体 | max_budget | budget_duration | budget_reset_at | spend | 来源 |
|------|-----------|-----------------|-----------------|-------|------|
| virtual_keys | ✅ 自身列 | ✅ 自身列 | ✅ 自身列 | ✅ 自身列 | 内联 |
| users | ✅ 自身列 | ✅ 自身列 | ✅ 自身列 | ✅ 自身列 | 内联 |
| teams | ✅ 自身列 | ✅ 自身列 | ✅ 自身列 | ✅ 自身列 | 内联 |
| organizations | ❌ 无此列 | ❌ 无此列 | ❌ 无此列 | ✅ 自身列 | budgets 表（通过 budget_id FK） |

### 3.3 更新策略：按实际关联更新

请求完成后，`auth`（KeyIdentity）中已有 `user_id / team_id / organization_id`（来自 key 查询，`auth.rs:498-500`）。不需要额外 DB 查询。

```rust
// 请求完成后的异步事务写入
let db = state.db.clone();
let tk = auth.token_hash.clone();
let uid = auth.user_id.clone();
let tid = auth.team_id.clone();
let oid = auth.organization_id.clone();
tokio::spawn(async move {
    db.transaction(|tx| {
        tx.increment_key_spend(&tk, cost);                         // Key — 始终更新
        if let Some(ref uid) = uid { tx.increment_user_spend(uid, cost); }  // User — 如果关联
        if let Some(ref tid) = tid { tx.increment_team_spend(tid, cost); }  // Team — 如果关联
        if let Some(ref oid) = oid { tx.increment_org_spend(oid, cost); }   // Org — 如果关联
        Ok(())
    }).await;
});
```

### 3.4 配额层级约束（写入时校验，非请求时校验）

**核心原则**：下级实体的 `max_budget` 不得大于直接上级实体的 `max_budget`。保证配置的自洽性，避免"Key $100 挂在 Team $50 下面，Key 花到 $55 被 Team 拒"的用户困惑。

```
Key.max_budget ≤ User.max_budget ≤ Team.max_budget ≤ Org.max_budget
（仅当关联了上层且上层有限额时才约束；NULL 上层 = 无限）
```

**校验时机**：实体创建/更新时（POST/PUT 端点），在写入前校验。不是请求路径预算检查时校验（那时只做 quota > spend 判断）。

**校验逻辑**：
```
写 Key 时：
  if key.max_budget 存在 AND key.user_id 存在:
      user = get_user_by_id(key.user_id)
      if user.max_budget 存在 AND key.max_budget > user.max_budget → 400 "Key budget cannot exceed user budget"
  if key.max_budget 存在 AND key.team_id 存在:
      team = get_team_by_id(key.team_id)
      if team.max_budget 存在 AND key.max_budget > team.max_budget → 400 "Key budget cannot exceed team budget"
  if key.max_budget 存在 AND key.organization_id 存在:
      检查 org 的 budget（从 budgets 表取）
      如果 org budget 存在 AND key.max_budget > org.max_budget → 400

写 User 时：
  if user.max_budget 存在 AND user.team_id 存在:
      team = get_team_by_id(user.team_id)
      if team.max_budget 存在 AND user.max_budget > team.max_budget → 400

写 Team 时：
  if team.max_budget 存在 AND team.organization_id 存在:
      检查 org 的 budget
      if org budget 存在 AND team.max_budget > org.max_budget → 400
```

**默认行为**：如果上级没有设置 `max_budget`（NULL），下级不约束——上级不限，下级也不限。如果下级不设 `max_budget`，也不检查——允许"下级不限，上级限总额"的场景。

### 3.5 各层独立 Reset，不级联

**核心原则：每个实体按自己的 `budget_duration` 独立重置，上级 reset 不触发下级 reset。**

```
User 7 天 reset → 只改 user.spend = 0，不动任何 key.spend
Key  30 天 reset → 只改 key.spend = 0，不动 user/team/org
```

**为什么不能级联？** Key 配 `max_budget=$10, budget_duration="30d"`，如果因为上级 User 7 天就给它清零，意味着 Key 的 30 天周期形同虚设——不是用户配置时的预期。多级 max_budget 检查（Stage 97）已经提供了层级保护：即使 Key 的 spend 不被上级 reset 影响，它自己的 max_budget 和上级的 max_budget 双重约束仍然有效，不会出现绕过。

**子长于父是合法场景。** Key 配 30d/$30、User 配 7d/$100——Key 享有更长周期稳定性，User 用更短周期控制总池子。两者独立，不矛盾。

**后期关联的处理。** Key 创建时可能没关联 User，后来才挂上去——因此 reset 周期约束也无法在创建时校验。多级 check_budget 保证不管什么时候关联，运行时都会检查。

### 3.6 reset 周期与 max_budget 的约束边界

| 约束项 | 是否约束 | 方式 | 时机 |
|--------|---------|------|------|
| `child.max_budget ≤ parent.max_budget` | ✅ 约束 | 写入时校验 | POST/PUT |
| 上级 reset 是否级联下级 | ❌ 不级联 | 各自独立 reset | tick |
| child.budget_duration 与 parent 的关系 | ❌ 不约束 | 各自独立 | — |
| 运行时多级超限 | ✅ 检查 | check_budget 逐级检查 | 每次请求 |

### 3.7 配额检查策略（请求时校验，三阶段推进）

```
Phase 39 Stage 94：
  ✅ 所有关联实体的 spend 更新（key + user + team + org）
  ✅ Key 级 BudgetEnforcer 检查（已有，spend 更新后生效）

Phase 39 Stage 95（预算配置时约束）：
  ✅ 配额层级约束校验（写入时）→ 防非法配置写入

Phase 39 Stage 97（请求时多级检查）：
  ✅ 扩展 BudgetEnforcer::check_budget → 逐级检查
     1. key.spend  >= key.max_budget?    → 403（始终检查）
     2. user.spend >= user.max_budget?   → 403（如果 key 关联了 user）
     3. team.spend >= team.max_budget?   → 403（如果关联了 team）
     4. org.spend  >= org.max_budget?    → 403（如果关联了 org）
```

**每一层独立检查，任一超限即拒**。不是 key.spend 累加到上级再判断，而是上级自己的 `spend` 列（由所有属于它的 requests 共同累加）与自己的 `max_budget` 比较。

---

## 4. budgets 表的作用

### 4.1 与其他实体的区别

```
budgets 表：独立的配额模板，有自己的 max_budget / budget_duration / budget_reset_at

orgs 表：
  budget_id (NOT NULL → FK budgets)
  没有 max_budget 列！            ← 配额从 budgets 表取
  没有 budget_duration 列！       ← 周期从 budgets 表取
  
keys/teams/users 表：
  max_budget 列（NULLABLE）       ← 自身有，可选引用 budgets
  budget_duration 列（NULLABLE）  ← 自身有
  budget_id 列（NULLABLE → FK）   ← 可选引用 budgets
```

**orgs 和其他实体的根本区别**：org 的配额只存在 budgets 表里，自身没有冗余列。key/team/user 可以把配额直接配在自己行上（内联），也可以引用 budgets 模板。

**对 Phase 39 的影响**：org 的 reset 和层级约束校验需要额外 JOIN budgets 表。

---

## 5. daily_spend 补全

### 5.1 现状：6 张表只写了 1 张

```
daily_user_spend:         ✅ 写入（kind: User）
daily_team_spend:         ❌ 未写入
daily_organization_spend: ❌ 未写入
daily_end_user_spend:     ❌ 未写入
daily_agent_spend:        ❌ 未写入
daily_tag_spend:          ❌ 未写入
```

### 5.2 修复策略

每次请求完成后，对每条可用实体 ID 各 queue 一条（仍是异步 channel 发送）：

```rust
// User（已有）
queue.queue(make_ds_log(spend_log.user, ..., DailySpendKind::User));

// Team（新增）
if let Some(ref tid) = spend_log.team_id {
    queue.queue(make_ds_log(tid, ..., DailySpendKind::Team));
}

// Org（新增）
if let Some(ref oid) = spend_log.organization_id {
    queue.queue(make_ds_log(oid, ..., DailySpendKind::Organization));
}

// EndUser（新增）
if let Some(ref euid) = spend_log.end_user {
    queue.queue(make_ds_log(euid, ..., DailySpendKind::EndUser));
}

// Agent（新增，当前始终 None 但保留入口）
if let Some(ref aid) = spend_log.agent_id {
    queue.queue(make_ds_log(aid, ..., DailySpendKind::Agent));
}
```

---

## 6. 顺修：失败路径 team_id/org_id 丢失

### 6.1 问题

`chat.rs` 所有失败路径（timeout/4xx/5xx）都硬编码：
```rust
team_id: None,
organization_id: None,
```

但 `auth.team_id` 和 `auth.organization_id` 在鉴权时已经拿到了。

### 6.2 修复

把 auth.team_id / org_id clone 到失败路径的 spend_log 构造中。涉及 `chat.rs` 约 6 处 + `v1_messages.rs` 约 4 处。

---

## 7. NaN 防御（litellm GHSA-2rv4-xv66-fpjg）

### 7.1 漏洞原理

IEEE 754 规定 `NaN` 的所有比较都返回 `false`：

```
NaN > 0.0   → false  // budget 检查永远不触发
inf > 100.0 → true   // spend 永远超不过 inf，检查永远不过
```

如果 `max_budget` 被设置为 `NaN` 或 `Infinity`（通过 JSON 解析、DB 直接操作等），预算检查静默失效。

### 7.2 litellm 的修复

在 `litellm/proxy/auth/auth_checks.py` 的 `_user_max_budget_check` 和全局 proxy budget 检查两处加 `math.isfinite()` 守卫。

### 7.3 aigw 对应修复

`crates/aigw-core/src/budget.rs:72` 一行改动：

```rust
// 修复前
Some(mb) if mb > 0.0 => mb,

// 修复后
Some(mb) if mb.is_finite() && mb > 0.0 => mb,
```

---

## 8. Phase 39 Stage 拆分

### 8.1 总览

| Stage | 目标 | 类型 | 预估 | 核心交付 |
|-------|------|------|------|----------|
| **Stage 94** | 实体 spend 异步增量更新 + daily_spend 全维度补全 + 失败路径修复 | 后端 | 12h | 4 个 `increment_*_spend` 方法 × 3 方言 + chat.rs/v1_messages.rs 通路 + daily_spend 5 维度 + 失败路径 team_id/org_id + NaN 防御 |
| **Stage 95** | duration 解析 + BudgetResetter AsyncTask + Budget CRUD + 启动 backfill + 配额层级约束 | 后端+测试 | 20h | `budget/duration.rs` + `budget/resetter.rs` + Budget CRUD API + 配置写入时层级约束校验 + Engine 注册 + 18 UT + 6 BDD + 3 方言 real BDD |
| **Stage 96** | 前端 — 实体表单内联 + budget_reset Job Tab | 前端+E2E | 16h | keys/teams/users/orgs 表单 budget_duration 下拉 + soft_budget + Jobs Tab 补全 + 11 Playwright BDD × 3 viewports |
| **Stage 97** | 全栈联调 — 多级 BudgetEnforcer + real BDD + 收尾 | 全栈+测试 | 8h | 扩展 BudgetEnforcer 逐级检查 + soft_budget 记日志 + 历史用量 team/org 聚合 + real BDD 三后端 + ADR-024 + TD-007 |

**Phase 39 合计**: 56h，4 Stages。

### 8.2 依赖关系

```
Stage 94（spend 写入 + daily_spend）  ← 基础：让 spend 列有值
    ├──→ Stage 95（reset + CRUD + 层级约束）  ← reset 依赖 spend 正确写入
    │       ├──→ Stage 96（前端）              ← 依赖后端 API + AsyncTask
    │       └──→ Stage 97（联调 + 多级检查）   ← 依赖全部就绪
```

### 8.3 各 Stage 详细

#### Stage 94：实体 spend 异步增量更新

**核心交付**：
1. DB 层 4 个 `increment_key/user/team/org_spend()` × 3 方言（12 条 SQL）
2. chat.rs / v1_messages.rs 在 `insert_spend_log` 后用 `tokio::spawn` + 事务批量更新
3. daily_spend 扩展到 5 个维度
4. 失败路径 team_id/org_id 修复
5. NaN 防御

**TDD**: ~22 UT + 6 BDD + real BDD 三后端

#### Stage 95：duration 解析 + BudgetResetter + CRUD + 层级约束

**核心交付**：
1. `budget/duration.rs`：解析 `1h/24h/7d/30d/1mo` + 词别名，`compute_next_reset_at` 标准化对齐
2. `budget/resetter.rs`：`impl AsyncTask`，tick 扫过期 → execute 批量 UPDATE spend=0
3. Budget CRUD API：`/budget/new|list|info|update|delete`
4. **配额层级约束**：在 keys/users/teams/orgs 的 POST/PUT 端点中校验下级 `max_budget ≤ 上级 max_budget`
5. 启动期 backfill + Engine 注册 + 配置

**TDD**: ~18 UT + 6 BDD + real BDD 三后端

#### Stage 96：前端表单 + Job Tab

同原规划，不变。

#### Stage 97：多级 BudgetEnforcer + 联调 + 收尾

**核心交付**：
1. **扩展 BudgetEnforcer**：从只检查 key 扩展到逐级检查 key → user → team → org，任一超限即 403
2. soft_budget 超限记 `tracing::warn!` 不拒绝
3. 历史用量聚合补全：`get_spend_by_team()` + `get_spend_by_org()`
4. real BDD 三后端完整链路
5. 文档收尾（ADR-024 / TD-007）

---

## 9. 修正后的架构总图

```
请求进入 → 鉴权（获取 auth: KeyIdentity { user_id, team_id, organization_id }）

请求完成 → 同步：
  INSERT spend_logs (api_key, user, team_id, org_id, spend)

请求完成 → tokio::spawn 异步（事务包裹）：
  UPDATE keys  SET spend = spend + cost WHERE token = ?        ← 始终
  UPDATE users SET spend = spend + cost WHERE user_id = ?      ← 如果关联
  UPDATE teams SET spend = spend + cost WHERE team_id = ?      ← 如果关联
  UPDATE orgs  SET spend = spend + cost WHERE org_id = ?       ← 如果关联

请求完成 → 异步 channel：
  daily_spend_queue.queue(User) + queue(Team) + queue(Org) + queue(EndUser) + queue(Agent)
  ↓ 10s 批量 upsert

配额检查（请求路径，Stage 97 扩展）：
  BudgetEnforcer::check_budget(db, auth):
    1. key.spend  >= key.max_budget?    → 403
    2. user.spend >= user.max_budget?   → 403（如果关联）
    3. team.spend >= team.max_budget?   → 403（如果关联）
    4. org.spend  >= org.max_budget?    → 403（如果关联）

配置写入时约束（Stage 95 新增）：
  POST/PUT key/user/team/org → 校验 child.max_budget ≤ parent.max_budget

周期重置（Stage 95）：
  BudgetResetter::tick()  →  扫 budget_reset_at < now()
  BudgetResetter::execute() → UPDATE spend=0, reset_at = compute_next()

历史用量（Stage 97 补全）：
  get_spend_by_key/user/team/org/global → SELECT SUM(spend) FROM spend_logs WHERE ...
```

---

## 10. 风险与回退

- **风险 1**：tokio::spawn 异步更新，崩溃时丢失最新一次计数。缓解：spend_logs 已留存审计完整记录，spend 计数差一次在当前配额周期内影响极小（~一个请求的费用）。
- **风险 2**：多层级检查增加 DB 查询（最多 4 次 PK lookup）。缓解：key 鉴权时已缓存 key 数据；user/team/org 的 PK lookup 都是 O(1)。如果需要优化，可以一次 JOIN 查询。
- **风险 3**：标准化对齐算法边界错误。缓解：chrono 处理，UT 覆盖边界。
- **回退**：`budget_reset.enabled: false` 关闭周期任务。

---

## 11. 参考来源

### aigw 现有代码
- `crates/aigw-server/src/routes/chat.rs:1653-1809` — 请求完成写入路径
- `crates/aigw-server/src/routes/chat.rs:460-509` — 鉴权路径，KeyIdentity 构造
- `crates/aigw-core/src/middleware/mod.rs:36-45` — KeyIdentity struct
- `crates/aigw-core/src/middleware/rate_limit.rs:120-159` — BudgetEnforcer 调用点
- `crates/aigw-core/src/daily_spend_queue.rs` — 异步批量写入
- `crates/aigw-core/src/budget.rs` — BudgetEnforcer（当前只检查 key）
- `crates/aigw-core/src/db.rs:1754-1783` — get_spend_by_key/user，SUM from spend_logs
- `crates/aigw-core/src/db.rs:3820-4106` — Org CRUD trait + impls
- `crates/aigw-core/src/db.rs:4105-4489` — Team CRUD trait + impls
- `crates/aigw-core/src/db.rs:4489-4726` — User CRUD trait + impls
- `crates/aigw-core/src/models.rs:28-83` — VirtualKey struct（spend + budget 字段）
- `crates/aigw-core/src/models.rs:247-260` — Organization struct（无 max_budget，有 budget_id）
- `crates/aigw-core/src/models.rs:264-324` — Team struct（有 max_budget + spend）
- `crates/aigw-core/src/models.rs:328-379` — User struct（有 max_budget + spend）
- `crates/aigw-core/src/models.rs:403-442` — Budget struct

### litellm 参考
- `litellm/proxy/common_utils/reset_budget_job.py` — ResetBudgetJob 核心
- `litellm/proxy/auth/auth_checks.py` — `math.isfinite` NaN 防御
- `litellm/litellm_core_utils/duration_parser.py` — get_next_standardized_reset_time
- https://docs.litellm.ai/docs/proxy/cost_tracking — spend 追踪文档
- GHSA-2rv4-xv66-fpjg — NaN budget bypass 安全公告
