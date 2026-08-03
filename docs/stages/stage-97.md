# Stage 97: 全栈联调 — 多级 BudgetEnforcer + soft/hard 双轨 + real BDD 三后端 + 收尾

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: 🔄 实现中
**预估**: 8h
**前置**: Stage 94 + Stage 95 + Stage 96
**Gate-2 评审**: 2026-08-03 (21 findings: 4C+6H+6M+5L, all C/H fixed in v1.1)

---

## 核心预期

1. **多级 BudgetEnforcer**: extends `check_budget()` → `check_budget_multi()` 逐级检查 `key → user → team → org`。任一超限即 403。各实体从自身 `spend` 列与 `max_budget` 比较（不是 key.spend 累加到上级）。org 的 `max_budget` 从 budgets 表取（JOIN）。

2. **entity_type 审计字段**: `BudgetError::Exceeded` 新增 `entity_type: String` 字段（"key"/"user"/"team"/"organization"），响应 body 携带（区分哪个层级拒绝），不泄漏实现细节。

3. **soft_budget 超限记日志不拒绝**: 超 `soft_budget` 记 `tracing::warn!`（含 entity_type + user_id/team_id + spent/limit），请求继续。告警通道留 TD-007。**新增**: 建议生产环境接入持久化审计表 `budget_rejections`（留作 LT-Audit 长期路线）。

4. **TOCTOU 竞态缓解**: 先更新 spend（`UPDATE SET spend = spend + ?` — Stage 94 已异步实现），预算检查在更新后执行（`spend >= max_budget? → reject`）。多级检查按 key→user→team→org 顺序，中间实体缺失时 **静默跳过 + `tracing::warn!`**（不拒请求，不暴露内部结构），对齐 litellm 行为（不因 FK 断链中断服务）。

5. **历史用量聚合补全**: 新增 `get_spend_by_team()` + `get_spend_by_org()`（trait 方法 + 3 方言 impl），从 spend_logs SUM。

6. **周期任务端到端联调**: 配 budget_duration → trigger → spend 清零 → 请求放行。

7. **real BDD 三后端**: sqlite/pg/mysql 完整链路（6 场景，见 §real BDD）。

8. **文档收尾**: roadmap Phase 39 ✅ + next-steps 总结 + tech-debt TD-007 + ADR-024（更新 §多级检查 + TOCTOU 策略）。

---

## 设计要点

### BudgetError 改造

```rust
#[derive(Debug, Error)]
pub enum BudgetError {
    #[error("{entity_type} budget exceeded: spent {spent:.4}, limit {limit:.4}")]
    Exceeded {
        entity_type: String,  // "key" | "user" | "team" | "organization"
        spent: f64,
        limit: f64,
    },
    #[error("Database error: {0}")]
    DbError(#[from] DbError),
}
```

**兼容性**: 现有 7 个单测使用 `BudgetError::Exceeded { spent, limit }` 模式匹配，需追加 `entity_type: _` 通配。

### 多级检查逻辑（v1.1 — 修复 Gate-2 发现）

```rust
/// 逐级检查 key → user → team → org。任一超限即 403。
/// 中间实体缺失：静默跳过 + tracing::warn!（不死，对齐 litellm）
pub async fn check_budget_multi(db: &Database, key: &KeyIdentity) -> Result<(), BudgetError> {
    // 1. Key 级（始终检查）
    let k = db.get_key_by_token(&key.token_hash).await?;
    if let Some(k) = k {
        check_entity("key", k.spend, k.max_budget_f64())?;
    }

    // 2. User 级（如果关联）
    if let Some(ref uid) = key.user_id {
        match db.get_user_by_id(uid).await {
            Ok(Some(u)) => check_entity("user", u.spend, u.max_budget_f64())?,
            Ok(None) => tracing::warn!(user_id=%uid, "user budget check skipped: entity not found"),
            Err(e) => return Err(BudgetError::DbError(e)),
        }
    }

    // 3. Team 级（如果关联）
    if let Some(ref tid) = key.team_id {
        match db.get_team_by_id(tid).await {
            Ok(Some(t)) => check_entity("team", t.spend, t.max_budget_f64())?,
            Ok(None) => tracing::warn!(team_id=%tid, "team budget check skipped: entity not found"),
            Err(e) => return Err(BudgetError::DbError(e)),
        }
    }

    // 4. Org 级（如果关联，配额从 budgets 表取）
    if let Some(ref oid) = key.organization_id {
        match db.get_organization_by_id(oid).await {
            Ok(Some(org)) => {
                if let Ok(Some(budget)) = db.get_budget_by_id(&org.budget_id).await {
                    check_entity("organization", org.spend, budget.max_budget_f64())?;
                }
            }
            Ok(None) => tracing::warn!(org_id=%oid, "org budget check skipped: entity not found"),
            Err(e) => return Err(BudgetError::DbError(e)),
        }
    }

    Ok(())
}

fn check_entity(entity_type: &str, spend: f64, max_budget: Option<f64>) -> Result<(), BudgetError> {
    let limit = match max_budget {
        Some(mb) if mb.is_finite() && mb > 0.0 => mb,
        _ => return Ok(()),
    };
    if spend > limit {  // > 不是 >= (对齐现有 key 级行为，边界值放行)
        return Err(BudgetError::Exceeded { entity_type: entity_type.to_string(), spent: spend, limit });
    }
    Ok(())
}
```

### TOCTOU 策略

```
Stage 94 已确保: spend 更新在前（tokio::spawn 异步事务）
Stage 97 添加:   预算检查在后（enforce_limits 入口）

竞态窗口: spend 已更新但 budget 检查尚未执行 (~ms)
缓解:     UPDATE 原子操作确保不丢计数
         多级检查读取的是已更新的 spend（因为 DB 事务已提交）
         检测到超限后返回 403（下一个请求也会超限，自愈）

已知残留窗口: 同一 key 的 2 个并发请求可能都通过检查（两者都读到 spend < max_budget，
             但累计 spend 超过 max_budget）。这是分布式系统固有 trade-off，
             litellm 同样存在。窗口 ~ms 级，生产影响可忽略。
```

### enforce_limits 改造

```rust
// crates/aigw-core/src/middleware/rate_limit.rs
pub async fn enforce_limits(
    db: &Database,
    rate_limiter: &RateLimiter,
    key: &KeyIdentity,
    token_estimate: u32,
) -> Result<(), LimitError> {
    if key.is_master_key { return Ok(()); }

    // 1. Multi-level budget check (key → user → team → org)
    budget::check_budget_multi(db, key)
        .await
        .map_err(|e| match e {
            BudgetError::Exceeded { entity_type, spent, limit } =>
                LimitError::BudgetExceeded { entity_type, spent, limit },
            BudgetError::DbError(err) =>
                LimitError::Internal(format!("Budget check failed: {}", err)),
        })?;

    // 2. Rate limits (unchanged)
    let key_data = db.get_key_by_token(&key.token_hash).await...;
    rate_limiter.check(&key.token_hash, rpm, tpm, token_estimate).await...;
    Ok(())
}
```

### LimitError 改造

```rust
// rate_limit.rs
BudgetExceeded {
    entity_type: String,  // NEW: which level rejected
    spent: f64,
    limit: f64,
},
```

响应 body 中包含 `entity_type`（"key"|"user"|"team"|"organization"），便于运维排查但不过度透露内部结构。

### DB 层新增方法

```rust
// db.rs trait:
async fn get_spend_by_team(&self, team_id: &str) -> Result<f64>;
async fn get_spend_by_org(&self, org_id: &str) -> Result<f64>;
```

3 方言实现 (SQLite/PG/MySQL):
```sql
SELECT COALESCE(SUM(spend), 0) FROM spend_logs WHERE team_id = ? 
SELECT COALESCE(SUM(spend), 0) FROM spend_logs WHERE organization_id = ?
```

### real_db_seed 新增实体辅助函数

```rust
// real_db_seed.rs 新增:
pub(crate) async fn ensure_organization(db_url: &str, org_id: &str, budget_id: &str, spend: f64);
pub(crate) async fn ensure_team(db_url: &str, team_id: &str, org_id: &str, max_budget: Option<f64>, spend: f64);
pub(crate) async fn ensure_user(db_url: &str, user_id: &str, team_id: &str, max_budget: Option<f64>, spend: f64);
pub(crate) async fn ensure_budget(db_url: &str, budget_id: &str, max_budget: f64);
pub(crate) async fn cleanup_entity(db_url: &str, entity_type: &str, entity_id: &str);
```

---

## real BDD 场景

每个场景在 SQLite、PostgreSQL（testcontainers）、MySQL（testcontainers）三个后端各执行一遍。

### 场景 1: 多级检查——key 未超但 user 超了
创建 user（max_budget=10, spend=9.5）+ key（max_budget=100, user_id=该user）→ 发请求（cost=1.0）→ key 检查通过（spend=1.0 < 100）→ user 检查拒绝（spend=10.5 > 10）→ 返回 403 BudgetExceeded entity_type="user"

### 场景 2: 多级检查——team 级拒绝
创建 team（max_budget=5, spend=4.8）+ key（关联该 team）→ 发请求（cost=0.5）→ key 通过 → team 拒绝（spend=5.3 > 5）→ 403 entity_type="team"

### 场景 3: 多级检查——全通过
创建 key（max_budget=100）+ user（max_budget=200）+ team（max_budget=500）→ 发请求（cost=1.0）→ key/user/team 全部通过 → 200

### 场景 4: org 级检查（JOIN budgets 表）
创建 org（关联 budget max_budget=20）+ team（关联该 org）+ key（关联该 team）→ 预置 org.spend=19.5 → 发请求（cost=1.0）→ key 通过 → user 通过 → team 通过 → org 检查拒绝（spend=20.5 > 20）→ 403 entity_type="organization"

### 场景 5: 完整链路——spend 更新 → reset → 恢复
创建 key（budget_duration="1h", max_budget=10）→ 发请求直到 spend=9.0 → 最后一次请求被拒绝（超限）→ trigger budget_reset → job 执行完成 → key.spend=0 → 请求恢复通过 200

### 场景 6: 历史用量聚合
创建多条 spend_logs（跨不同 key/user/team/org）→ 调用 `get_spend_by_team()` + `get_spend_by_org()` → 验证 SUM(spend) 与预期一致

---

## TDD

- **UT（~10）**: check_budget_multi 各层 (key/user/team/org 单层超限, 全通过, 中间实体缺失静默跳过, entity_type 字段正确, > 非 >= 边界) + LimitError::BudgetExceeded entity_type 字段
- **BDD (mock)**: 无需新增（多级检查依赖真实 DB 数据，mock 无法模拟多表关联）
- **real BDD**: 6 场景 × 三后端 (SQLite/PG/MySQL)

---

## 验收门禁

| task | 类型 | 预期 |
|------|------|------|
| `task test` | 单元测试 | 新增 ~10 UT + 回归 ≥ 264 = ≥ 274 pass |
| `task bdd` | mock BDD | 回归 ~178 pass（无新增 mock 场景） |
| `task bdd-real-sqlite` | real BDD | 新增 6 + 回归 36 = **42 pass** |
| `task bdd-real-pg` | real BDD | 新增 6 + 回归 36 = **42 pass** |
| `task bdd-real-mysql` | real BDD | 新增 6 + 回归 36 = **42 pass** |
| `task fe-bdd` | 前端 BDD | 回归无退化 |

## 变更文件清单

| 文件 | 变更 |
|------|------|
| `crates/aigw-core/src/budget.rs` | 改 BudgetError::Exceeded (+entity_type), 新增 check_budget_multi(), 更新 7 个现有测试 |
| `crates/aigw-core/src/middleware/rate_limit.rs` | 改 LimitError::BudgetExceeded (+entity_type), enforce_limits 调用 check_budget_multi |
| `crates/aigw-core/src/db.rs` | 新增 get_spend_by_team/get_spend_by_org (trait + 3 impl + dispatch) |
| `crates/aigw-server/tests/bdd_steps/real_db_seed.rs` | 新增 ensure_organization/team/user/budget + cleanup 函数 |
| `crates/aigw-server/tests/features/real/multi_level_budget.feature` | 新建 — 6 个 real BDD 场景 |
| `crates/aigw-server/tests/bdd_steps/budget_reset_steps.rs` | 追加 multi-level check 的 when/then step |

## 不纳入本期的项（登记为技术债/长期路线）

| 项 | 登记 |
|----|------|
| entity_type 信息泄漏风险（响应暴露层级结构） | ✅ 接受 — 运维价值 > 安全风险（现有 litellm 也在错误消息中返回 key 信息） |
| 并发请求双花（窗口 ~ms） | ✅ 接受 — 分布式系统固有 trade-off，litellm 同样存在 |
| 持久化审计表（budget_rejections） | ✅ 登记为 LT-Audit（生产运维需要时触发） |
| org check 2 次 DB round-trip 优化 | ✅ 暂不优化 — 当前 org 关联场景极少，JOIN 优化留到 LT-DBOpt |
