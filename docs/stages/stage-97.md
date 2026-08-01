# Stage 97: 全栈联调 — 多级 BudgetEnforcer + soft/hard 双轨 + real BDD 三后端 + 收尾

**Phase**: 39 — Budget Reset 周期任务 + 配置
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 8h
**前置**: Stage 94 + Stage 95 + Stage 96

---

## 核心预期

1. **多级 BudgetEnforcer**：扩展 `check_budget()` → 逐级检查 `key → user → team → org`，而非只查 key。任一超限即 403。各实体从自身 `spend` 列与 `max_budget` 比较（不是 key.spend 累加到上级）。org 的 `max_budget` 从 budgets 表取。

2. **soft_budget 超限记日志不拒绝**：超 `soft_budget` 记 `tracing::warn!`，请求继续。告警通道留 TD-007。

3. **历史用量聚合补全**：`get_spend_by_team()` + `get_spend_by_org()`（从 spend_logs SUM）。

4. **周期任务端到端联调**：配 budget_duration → trigger → spend 清零 → 请求放行。

5. **real BDD 三后端**：sqlite/pg/mysql 完整链路（创建带 duration 的 key → trigger reset → 验证 spend=0 + reset_at 滚动 + 下次请求通过）。

6. **文档收尾**：roadmap Phase 39 ✅ + next-steps 总结 + tech-debt TD-007 + ADR-024。

---

## 多级检查逻辑

```rust
pub async fn check_budget_multi(db: &Database, auth: &KeyIdentity) -> Result<(), BudgetError> {
    // 1. Key 级（始终检查）
    let key = db.get_key_by_token(&auth.token_hash).await?;
    check_entity("key", key.spend, key.max_budget_f64())?;

    // 2. User 级（如果关联）
    if let Some(ref uid) = auth.user_id {
        if let Some(user) = db.get_user_by_id(uid).await? {
            check_entity("user", user.spend, user.max_budget_f64())?;
        }
    }

    // 3. Team 级（如果关联）
    if let Some(ref tid) = auth.team_id {
        if let Some(team) = db.get_team_by_id(tid).await? {
            check_entity("team", team.spend, team.max_budget_f64())?;
        }
    }

    // 4. Org 级（如果关联，配额从 budgets 表取）
    if let Some(ref oid) = auth.organization_id {
        if let Some(org) = db.get_organization_by_id(oid).await? {
            let budget = db.get_budget_by_id(&org.budget_id).await?;
            if let Some(b) = budget {
                check_entity("organization", org.spend, b.max_budget_f64())?;
            }
        }
    }
    Ok(())
}
```

每层的 `check_entity` 做 `entity.spend >= entity.max_budget? → BudgetError::Exceeded`。任一超限即返回错误——不需要继续检查下层（最小成本原则）。

---

## real BDD 场景（概览）

每个场景在 SQLite、PostgreSQL（testcontainers）、MySQL（testcontainers）三个后端各执行一遍。

### 场景 1：多级检查——key 未超但 user 超了
创建 user（max_budget=10, spend=9.5）+ key（max_budget=100, user_id=该user）→ 发请求（cost=1.0）→ key 检查通过（spend=1.0 < 100）→ user 检查拒绝（spend=10.5 > 10）→ 返回 403 BudgetExceeded

### 场景 2：多级检查——team 级拒绝
创建 team（max_budget=5, spend=4.8）+ key（关联该 team）→ 发请求（cost=0.5）→ key 通过 → team 拒绝（spend=5.3 > 5）→ 403

### 场景 3：多级检查——全通过
创建 key（max_budget=100）+ user（max_budget=200）+ team（max_budget=500）→ 发请求（cost=1.0）→ key/user/team 全部通过 → 200

### 场景 4：org 级检查（JOIN budgets 表）
创建 org（关联 budget max_budget=20）+ team（关联该 org）+ key（关联该 team）→ 发请求直到 org.spend 接近 20 → 验证请求被子 org budget 拒绝

### 场景 5：完整链路——spend 更新 → reset → 恢复
创建 key（budget_duration="24h", max_budget=10）→ 发请求直到 spend=9.0 → 最后一次请求被拒绝（超限）→ trigger reset → spend=0 → 请求恢复通过

### 场景 6：历史用量聚合
创建多条 spend_logs（不同 key/user/team/org）→ 调用 `get_spend_by_team()` / `get_spend_by_org()` → 验证 SUM 值与预期一致

---

## 验收门禁

- aigw-core lib + aigw-server lib 全绿
- mock BDD 全量回归无降级
- **real BDD 三后端（SQLite/PG/MySQL）6 场景全部通过（硬性要求，任一失败不可交付）**
- 端到端手动验证：spend 更新 → 多级检查触发 403 → 手动 trigger reset → spend 清零 → 请求恢复通过
- 四份文档同步更新（stage-roadmap / next-steps / tech-debt TD-007 / ADR-024）
