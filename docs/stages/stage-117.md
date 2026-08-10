# Stage 117: A 类接线核心 — 限流 + 多级预算 + 告警 + max_parallel（S1）

**所属**: Phase 47（A 类接线 + 缓存）
**预估**: 16h（后端 + 测试）
**依赖**: 无（`enforce_limits`/`check_budget_multi`/`alerts` 均已实现待接线）
**状态**: ⏳ 待开始

---

## 1. 目标

把差距调研确认的 **A 类最大欠账**（代码已实现 + 有 UT，但请求路径零调用点，生产不生效）接入 4 个 LLM handler 请求入口：

1. **RPM/TPM 限流** — `enforce_limits`（含 `RateLimiter.check`）接入请求入口
2. **多级预算** — `check_budget_multi`（key→user→team→org）随 `enforce_limits` 生效（当前只生效单级 key max_budget）
3. **soft_budget 告警** — 命中 soft 阈值发 webhook（`aigw_core::alerts` 已实现）
4. **max_parallel_requests** — 每 deployment `tokio::sync::Semaphore` 并发上限

> 企业采购 demo 一测即穿——本 Stage 是差距报告 P0 最优先项。

## 2. 现状证据（已核实）

| 项 | 现状 | 证据 |
|----|------|------|
| `enforce_limits` | 完整实现（含多级预算 + RPM/TPM），仅 test 调用 | `middleware/rate_limit.rs:126`；`main.rs` 创建注入 AppState 后无请求路径调用 |
| `check_budget_multi` | key→user→team→org 逐级，生产零调用 | `budget.rs:152`，仅 `#[cfg(test)]` 调用 |
| `alerts` | `AlertDispatcher` + webhook 已实现（TD-007），未被引用 | `alerts.rs`（146 行） |
| `max_parallel_requests` | 字段存储于 key/budget 表，无执行 | 无信号量 |

## 3. 方案

### 3.1 共享 guard 函数

新建 `aigw_core::middleware::request_guard`（或扩展 rate_limit.rs），供 4 个 handler（chat / v1_messages / responses / embeddings）入口调用：

```rust
/// 请求入口全链 guard：多级预算 → soft_budget 告警 → RPM/TPM → max_parallel
pub async fn check_request_limits(
    db: &Database,
    rate_limiter: &RateLimiter,
    key: &KeyIdentity,
    token_estimate: u32,
) -> Result<(), LimitError>
```

- 复用现有 `enforce_limits` 逻辑，追加：soft_budget 告警分支 + max_parallel 检查。
- `token_estimate`：优先请求 body `max_tokens`（chat）/ `max_output_tokens`（responses）/ 无则 0（只查 RPM + 预算）。
- master key 直通（现有逻辑保留）。

### 3.2 接线点（4 handler）

| handler | 接线位置 | 说明 |
|---------|----------|------|
| chat.rs | 认证后、resolve 前 | 已有 `KeyIdentity` |
| v1_messages.rs | 认证后、resolve 前 | 同上 |
| responses.rs | 认证后、resolve 前 | 同上 |
| embeddings.rs | 认证后、resolve 前 | 同上 |

### 3.3 soft_budget 告警

`check_budget_multi` 返回软命中（`spent > soft_budget && spent < max_budget`）时：
- 调 `alerts::AlertDispatcher::dispatch(AlertEvent::BudgetSoftLimit { entity_type, spent, limit, key_alias })`（webhook 已实现）
- 记录 `tracing::warn!`
- 放行（soft 不拒绝）

### 3.4 max_parallel_requests

- 按 `(api_base, upstream_model)` 分桶维护 `tokio::sync::Semaphore` 表（`Arc<Semaphore>`），值来自 Deployment/key 的 `max_parallel_requests`（默认无限制）。
- 上游调用段 `acquire_owned()` → 完成后 release。
- 超限返回 429 + `Retry-After` 头（对齐 litellm `TooManyRequestsError`）。

### 3.5 竞态窗口文档化

预算检查（读 spend 列）→ 请求 → 完成后异步增量间的 TOCTOU 窗口：保持现状（检查前置 + 写回后置），在 `budget.rs` 注释 + ADR 文档化；`check_budget_multi` 用 DB 事务读快照缓解。

## 4. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/middleware/rate_limit.rs` | 修改 | 扩展 `check_request_limits` + soft_budget 告警 + max_parallel 检查 |
| `crates/aigw-core/src/middleware/mod.rs` | 修改 | 导出 |
| `crates/aigw-core/src/budget.rs` | 修改 | soft_budget 告警分支 + 事务快照读 |
| `crates/aigw-core/src/alerts.rs` | 修改 | `BudgetSoftLimit` 事件 + dispatcher 接线 |
| `crates/aigw-core/src/router.rs` | 修改 | max_parallel Semaphore 桶管理 |
| `crates/aigw-server/src/routes/chat.rs` | 修改 | 入口调 guard |
| `crates/aigw-server/src/routes/v1_messages.rs` | 修改 | 同上 |
| `crates/aigw-server/src/routes/responses.rs` | 修改 | 同上 |
| `crates/aigw-server/src/routes/embeddings.rs` | 修改 | 同上 |

## 5. TDD

- **core UT**（8-10）：`check_request_limits` master 直通 / RPM 超限 429 / TPM 超限 / 多级预算逐级拒绝（key→user→team→org）/ soft_budget 放行+告警触发 / max_parallel 排队与 429 / token_estimate 0 只查 RPM+预算 / 竞态快照读。
- **handler UT**（4）：每 handler 入口 guard 接线（mock identity）。
- **mock BDD**（4-5）：RPM 超限 429（`x-ratelimit` 头）/ 多级预算拒绝 / soft_budget webhook 告警 / max_parallel 429。
- **real BDD 三后端**（3-4）：预算/限流跨 SQLite/PG/MySQL 一致。

## 6. 验收标准

- [ ] `task test` UT 全绿（aigw-core + aigw-server）
- [ ] `task bdd` mock BDD 全绿（新增 4-5 场景）
- [ ] `task bdd-real-sqlite` / `bdd-real-pg` / `bdd-real-mysql` 全绿
- [ ] `task fmt` / `task lint` 全绿；cargo check 无 warning
- [ ] RPM/TPM 超限返回 429（BDD 断言 `x-ratelimit` 头）；多级预算逐级拒绝；soft_budget 触发 webhook；max_parallel 排队

## 7. 参考实现

- litellm `auth_checks.py:504-712`（auth 阶段全链预算/限流）+ `common_request_processing.py:1623`
- litellm `router.py:2886-2905`（Semaphore 包调用）
- litellm `budget_reservation.py:147`（spend 预扣）
