# Stage 99: 内部模块 + middleware 补测 — daily_spend_queue UT + rate_limiter BDD + auth_gateway UT

**Phase**: 40 — BDD Coverage Enhancement  
**优先级**: P0  
**状态**: ⏳ 待开始  
**预估**: 14h  
**前置**: 无（改 aigw-core src + server BDD，与 Stage 98/100 并行）

---

## 核心预期

补齐 2026-08-03 BDD 覆盖率审计发现的内部模块测试缺口。重点：`daily_spend_queue.rs` **零测试覆盖**（P0 风险——每日消费预聚合批处理，跨 SQLite/MySQL/PG，生产关键路径）。

| 子任务 | 模块 | 测试类型 | 新增数 | 预估 |
|--------|------|---------|--------|------|
| A | `daily_spend_queue.rs` | UT | 7 | 6h |
| B | `rate_limiter` middleware | BDD | 3 | 3h |
| C | `middleware/auth_gateway.rs` | UT | 4 | 2h |
| D | `middleware/rate_limit.rs` | UT | 5 | 3h |

**总计: 19 测试（7 UT + 3 BDD + 4 UT + 5 UT）**

---

## Part A: daily_spend_queue.rs 单元测试（P0, 6h）

### 风险等级

🔴 **P0** — 每日消费预聚合批处理队列，通过 `tokio::sync::mpsc` channel 收集 `DailySpendLog`，后台 10s drain 后批量 `UPSERT`（`INSERT ... ON CONFLICT DO UPDATE`）到三张 `daily_*_spend` 表（SQLite/MySQL/PG 三方言）。当前 **零测试覆盖**——任何 drain/queue/aggregate/UPSERT 逻辑变更无安全网。

### 实现计划

在 `crates/aigw-core/src/daily_spend_queue.rs` 末尾追加 `#[cfg(test)] mod tests`，使用 `sqlite::memory:` Database 实例。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::models::DailySpendKind;

    async fn setup_db() -> Database {
        // 用 sqlite::memory: 创建 Database::Sqlite，运行 migration
    }

    #[tokio::test]
    async fn test_queue_single_kind_user() {
        // 验证：queue 一条 DailySpendKind::User → drain → daily_user_spend 写入成功
        let db = setup_db().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let queue = DailySpendQueue { tx: tx.clone() };
        queue.queue(DailySpendLog { kind: DailySpendKind::User, ... });
        drop(tx);
        let count = aggregate_daily_spend(&db, &mut rx).await.unwrap();
        assert_eq!(count, 1);
        // 验证 daily_user_spend 表有记录
    }

    #[tokio::test]
    async fn test_queue_multiple_kinds() {
        // 验证：User + Team + Org 三 kind queue → drain 后三张表各有记录
    }

    #[tokio::test]
    async fn test_drain_empty_queue_returns_zero() {
        // 验证：空队列 drain → 返回 Ok(0)
    }

    #[tokio::test]
    async fn test_drain_flushes_all_pending() {
        // 验证：queue 10 条 → drain → 10 条全部写入，无遗漏
    }

    #[tokio::test]
    async fn test_daily_spend_sum_aggregation_correct() {
        // 验证：同一 entity_id 多次 queue → drain 后 spend 正确 SUM 聚合
        let db = setup_db().await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let queue = DailySpendQueue { tx: tx.clone() };
        // queue 3 条同 entity
        queue.queue(ds("user-a", 0.05));
        queue.queue(ds("user-a", 0.03));
        queue.queue(ds("user-a", 0.02));
        drop(tx);
        aggregate_daily_spend(&db, &mut rx).await.unwrap();
        // 查询 daily_user_spend WHERE entity_id='user-a' → spend = 0.10
    }

    #[tokio::test]
    async fn test_queue_after_drain_restarts() {
        // 验证：drain 后 channel 重新可用，新 queue 仅写新记录
    }

    #[tokio::test]
    async fn test_concurrent_queue_no_data_race() {
        // 验证：多生产者并发 queue + drop(tx) 后 drain → 所有记录全部写入，无丢失
    }
}
```

### 关键约束

1. **`daily_spend_queue` 依赖 `Database` trait**：现有 `aggregate_daily_spend` 函数签名是 `async fn aggregate_daily_spend(db: &Database, ...)`（非 trait 方法，是 standalone 函数），可直接用 `Database::Sqlite` 内存实例测试。
2. **Migration 需覆盖 daily_spend 表**：`daily_user_spend` / `daily_team_spend` / `daily_org_spend` / `daily_end_user_spend` / `daily_agent_spend` 五张表需在测试迁移中建表（参考 `bdd_support/test_db.rs` 的 migration 模式）。
3. **mpsc channel 测试模式**：queue 通过 `unbounded_channel` 的 `tx` 端发送，测试中 clone `tx` 给 `DailySpendQueue::new(tx)`，测试保留原始 `tx` 用于后续 queue 调用。`drop(tx)` 关闭 sender 端后 drain 退出循环。

---

## Part B: rate_limiter 429 BDD（3h）

**背景**: `middleware/rate_limit.rs` 的 `enforce_limits()` 在 RPM/TPM 超限时返回 `LimitError::RateLimited` → 429 状态码，但无 BDD 场景验证实际 HTTP 429 响应。

新建 `crates/aigw-server/tests/features/rate_limit.feature`：

```gherkin
@mock
Feature: Rate Limiting — RPM/TPM 超限返回 429

  Scenario: RPM 超限返回 429
    Given key "rpm-limit-key" 的 rpm_limit=2
    And 过去 1 分钟内已发送 2 个请求
    When 使用 key "rpm-limit-key" 发送 POST /v1/chat/completions 请求
    Then 响应状态码为 429
    And "rate limit" 在响应 body 中

  Scenario: TPM 超限返回 429
    Given key "tpm-limit-key" 的 tpm_limit=100
    And 过去 1 分钟内已消费 100 tokens
    When 使用 key "tpm-limit-key" 发送 POST /v1/chat/completions 请求
    Then 响应状态码为 429

  Scenario: 未超限正常通过
    Given key "normal-key" 的 rpm_limit=100
    When 使用 key "normal-key" 发送 POST /v1/chat/completions 请求
    Then 响应状态码为 200
```

**BDD step 实现**: `crates/aigw-server/tests/bdd_steps/rate_limit_steps.rs`（新建）。关键设计：
- 用 mock 的 `RateLimiter` 实例（`RateLimiter::new()` 空状态）预填充 `rate_limit_counter` 模拟历史请求数。
- `enforce_limits()` 调用链：step 中手动调用 `enforce_limits(&db, &limiter, &key, token_estimate)` → 验证返回 `LimitError::RateLimited` → mock HTTP 返回 429。
- 或更简化：直接在 mock upstream 层注入 429 响应（BDD 场景关注的是 HTTP 层的 429 行为，不关注 `RateLimiter` 内部实现）。

**简化实现建议**: 参考 `error_handling.feature` 的 upstream 429 透传场景——在 `mock_upstream.rs` 为特定 scenario 配置 upstream 返回 429 → handler 透传 429 → BDD 断言 429。这样可以验证完整的请求→handler→response 链路，而不需要 mock `RateLimiter` 内部状态。

---

## Part C: middleware auth_gateway.rs 单元测试（2h）

在 `crates/aigw-core/src/middleware/auth_gateway.rs` 末尾追加 `#[cfg(test)] mod tests`：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_deployment_mode_onprem_bypasses_tenant_check() {
        // DeploymentMode::OnPrem → 不需要 X-Tenant-Id header
    }

    #[tokio::test]
    async fn test_deployment_mode_saas_requires_tenant_header() {
        // DeploymentMode::SaaS → 缺少 X-Tenant-Id → 401
    }

    #[tokio::test]
    async fn test_tenant_identity_from_valid_header() {
        // 正确的 X-Tenant-Id → TenantIdentity 提取成功
    }

    #[tokio::test]
    async fn test_invalid_tenant_header_format_returns_401() {
        // X-Tenant-Id 格式错误 → 401
    }
}
```

**参考**: 现有 `middleware/mod.rs` 已有 `#[cfg(test)] mod tests`（6 个 x-api-key/Bearer 认证测试），auth_gateway 测试风格对齐。

---

## Part D: middleware rate_limit.rs 单元测试（3h）

在 `crates/aigw-core/src/middleware/rate_limit.rs` 末尾追加 `#[cfg(test)] mod tests`：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::rate_limiter::RateLimiter;

    async fn setup() -> (Database, RateLimiter) { /* sqlite::memory: */ }

    #[tokio::test]
    async fn test_enforce_limits_passes_when_under_budget_and_rate() {
        // key.spend=5, key.max_budget=100 → budget OK
        // RPM limit=100, rate counter under → rate OK
        // → enforce_limits 返回 Ok(())
    }

    #[tokio::test]
    async fn test_enforce_limits_fails_when_over_budget() {
        // key.spend=150, key.max_budget=100 → LimitError::BudgetExceeded
    }

    #[tokio::test]
    async fn test_enforce_limits_fails_when_over_rpm() {
        // RPM counter >= limit → LimitError::RateLimited
    }

    #[tokio::test]
    async fn test_enforce_limits_passes_when_no_budget_set() {
        // key.max_budget=None → unlimited → budget check skipped → passes
    }

    #[tokio::test]
    async fn test_enforce_limits_checks_budget_before_rate() {
        // Budget 已超 → 返回 BudgetExceeded（不检查 rate）
        // 验证预算检查在前，budget 超限时不再浪费 rate 检查
    }
}
```

**关键约束**: `enforce_limits()` 目前只检查 **key 级别的 budget**，不检查 user/team/org（多级 BudgetEnforcer 归 Stage 97）。测试范围对齐当前行为——只测 key 级别。

---

## 方言差异与 real BDD 必要性

| 测试 | SQLite mock 够用? | 原因 |
|------|-------------------|------|
| daily_spend_queue UT | ✅ 够用 | `aggregate_daily_spend` 的 UPSERT 在 SQLite/PG/MySQL 的方言实现虽不同（`ON CONFLICT DO UPDATE` vs `INSERT ... ON DUPLICATE KEY UPDATE`），但语义一致。`Database::Sqlite` 已在 `db.rs` 中有完整 SQLite 方言实现，测试覆盖 SQLite 即覆盖逻辑正确性。PG/MySQL 方言差异通过现有 `bdd-real-pg/mysql` 的 daily_spend 写入场景间接覆盖（Stage 94）。 |
| rate_limiter BDD | ✅ 够用 | mock upstream 注入 429 → handler 透传 → BDD 断言，纯 HTTP 层逻辑，不涉及 SQL 方言。 |
| auth_gateway UT | ✅ 够用 | 纯中间件逻辑（header 解析 + 模式判断），不依赖 DB。 |
| rate_limit UT | ✅ 够用 | budget check 走 `BudgetEnforcer::check_budget`（已有 7 个 UT），rate check 走 `RateLimiter::check`（已有 7 个 UT），本模块测试组合逻辑。 |

**不需要 real BDD**。

---

## TDD

- **Part A**: 7 UT — 先写空测试函数签名 → `task test` 跑红 → 实现 body → 跑绿
- **Part B**: 3 BDD — 先写 Gherkin → 定义 step 骨架 → `task bdd` 跑红 → 实现 step → 跑绿
- **Part C**: 4 UT — 同 Part A
- **Part D**: 5 UT — 同 Part A

---

## 验收门禁

| task | 类型 | 预期 | 说明 |
|------|------|------|------|
| `task test` | **单元测试** | 新增 16 UT + 回归 ≥ 264 = ≥ 280 pass | daily_spend_queue 7 + auth_gateway 4 + rate_limit 5 |
| `task bdd` | **mock BDD** | 新增 3 + 回归 ~178 = ~181 pass | rate_limiter.feature 3 场景 |
| `task bdd-real-sqlite` | real BDD | 回归 36 pass | 无新增 real BDD 场景 |
| `task bdd-real-pg` | real BDD | 回归 36 pass | 同上 |
| `task bdd-real-mysql` | real BDD | 回归 36 pass | 同上 |
| `task fe-bdd` | 前端 BDD | 回归无退化 | 前端无变更 |

> **本 Stage 涉及 `task test`（aigw-core UT）+ `task bdd`（mock BDD rate_limit 场景），不涉及 `task bdd-real-mysql/pg/sqlite` 新增场景，不涉及 `task fe-bdd` 新增场景。**
