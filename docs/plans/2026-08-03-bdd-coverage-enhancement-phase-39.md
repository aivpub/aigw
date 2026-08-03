# Phase 39: BDD Coverage Enhancement — 提升场景覆盖

**Phase**: 39 — BDD 场景覆盖率提升  
**优先级**: P0（按调研结论，先补测试防回归，再开新功能）  
**状态**: 规划中  
**背景调研**: `docs/research/2026-08-03-bdd-coverage-audit.md`  
**创建日期**: 2026-08-03

---

## 背景

2026-08-03 完成全量 BDD 覆盖审计（三路 subagent 并行扫描），发现：

- **10 个路由端点缺失 BDD**: health metrics 3 端点 + router_settings PATCH 2 端点 + deleted_list 4 端点 + docs 1（跳过）
- **6 个 aigw-core 内部模块无测试**: `daily_spend_queue.rs` 零测试（P0 风险）+ `rate_limiter.rs` + `middleware/auth_gateway.rs` + `middleware/rate_limit.rs` + `tenant.rs` + `metrics.rs`
- **aigw-migrate BDD 偏弱**: 仅 10 场景覆盖迁移基本路径，step-filter/skip-columns/cursor-resume/pre-check 等高级功能无 BDD

**核心原则**: RDD 驱动——先补测试防回归，再开新功能（budget reset Phase 40）。每 Stage 强制 TDD 红绿循环 + mock BDD + real BDD 三后端验证。

---

## Stage 拆分（3 Stage，共 36h）

```
Phase 39: BDD Coverage Enhancement
├── Stage 98: 路由端点补 BDD (12h)
│   ├── health metrics 端点 (health_latest, prometheus_metrics, health_metrics)
│   ├── router_settings PATCH 端点 (patch_key, patch_team)
│   └── deleted_list 端点 (team/model/user/org deleted)
├── Stage 99: 内部模块 + middleware 补测 (14h)
│   ├── daily_spend_queue UT (P0 风险)
│   ├── rate_limiter 429 BDD
│   ├── middleware auth_gateway UT
│   └── middleware rate_limit BDD
└── Stage 100: aigw-migrate 高级功能 BDD (10h)
    ├── pre-check BDD + verify BDD
    ├── step-filter + skip-columns BDD
    ├── cursor resume BDD
    └── 全量 BDD 回归 + 覆盖率报告
```

---

## Stage 98: 路由端点补 BDD（12h）

### 核心预期

补齐调研报告确认的 9 个缺失路由端点 BDD 场景，每个场景 ready→act→assert 完整覆盖正常路径和错误路径。

### 端点清单

| # | 端点 | Method + Path | 新 Feature 文件 | 场景数 | 复杂度 |
|---|------|---------------|---------------|--------|--------|
| 1 | `health_latest` | GET /health/latest | `health.feature` (追加) | 2 | 低 |
| 2 | `prometheus_metrics` | GET /metrics | `health.feature` (追加) | 1 | 低 |
| 3 | `health_metrics` | GET /health/metrics | `health.feature` (追加) | 2 | 低 |
| 4 | `patch_key` | PATCH /key/{token}/router/settings | `router_settings.feature` (新) | 2 | 低 |
| 5 | `patch_team` | PATCH /team/{id}/router/settings | `router_settings.feature` (新) | 2 | 低 |
| 6 | `team_deleted_list` | GET /team/deleted | `deleted_list.feature` (新) | 1 | 低 |
| 7 | `model_deleted_list` | GET /model/deleted | `deleted_list.feature` (新) | 1 | 低 |
| 8 | `user_deleted_list` | GET /user/deleted | `deleted_list.feature` (新) | 1 | 低 |
| 9 | `org_deleted_list` | GET /org/deleted | `deleted_list.feature` (新) | 1 | 低 |

**合计: 13 BDD 场景 + 3 feature 文件**

### 场景设计

#### Part A: health.feature 追加（5 场景）

```gherkin
# health.feature 追加场景（续现有 health.feature 6 场景）

Scenario: health_latest 返回最新检查记录
  Given health_checks 表中有 3 条历史检查记录（2 success, 1 failure）
  When 发送 GET /health/latest 请求
  Then 响应状态码为 200
  And 响应包含最新一条记录的 timestamp
  And 响应包含 model_name 和 status 字段

Scenario: health_latest 无记录时返回空
  Given health_checks 表为空
  When 发送 GET /health/latest 请求
  Then 响应状态码为 200
  And 响应 body 为 {} 或空对象

Scenario: prometheus_metrics 返回期待格式
  When 发送 GET /metrics 请求
  Then 响应状态码为 200
  And Content-Type 为 "text/plain; version=0.0.4" 或类似 Prometheus exposition format
  And 响应体包含 "aigw_" 前缀的指标名

Scenario: health_metrics 返回 JSON metrics
  Given 数据库连接正常
  When 发送 GET /health/metrics 请求（admin 认证）
  Then 响应状态码为 200
  And 响应包含 uptime_seconds 字段
  And 响应包含 db_pool_size 或类似字段
  And 响应包含 key_count 和 model_count 字段

Scenario: health_metrics 无认证返回 401
  When 发送 GET /health/metrics 请求（无认证）
  Then 响应状态码为 401
```

#### Part B: router_settings.feature（新建，4 场景）

```gherkin
@mock
Feature: Router Settings — key/team 级别覆盖

  Scenario: patch_key 设置 key 级别 router_settings
    Given 存在 key "patch-test-key"（无 router_settings）
    When 使用 master-key 发送 PATCH /key/patch-test-key/router/settings 请求
      | cooldown_time | 30 |
    Then 响应状态码为 200
    And 查询 GET /key/info?key=patch-test-key 返回 router_settings.cooldown_time 为 30

  Scenario: patch_key 无认证返回 401
    When 发送 PATCH /key/some-key/router/settings 请求（无认证）
    Then 响应状态码为 401

  Scenario: patch_team 设置 team 级别 router_settings
    Given 存在 team "patch-team-1"
    When 使用 master-key 发送 PATCH /team/patch-team-1/router/settings 请求
      | retry_count | 2 |
    Then 响应状态码为 200
    And 查询 GET /team/info?team_id=patch-team-1 返回 router_settings.retry_count 为 2

  Scenario: patch_team 无认证返回 401
    When 发送 PATCH /team/some-team/router/settings 请求（无认证）
    Then 响应状态码为 401
```

#### Part C: deleted_list.feature（新建，4 场景）

```gherkin
@mock
Feature: Deleted Entity Lists — 软删除实体归档查询

  Background:
    Given 数据库中已有 teams, models, users, orgs 各 2 条正常记录

  Scenario: GET /team/deleted 返回已删除 team 列表
    Given team "archive-team-1" 已被软删除
    When 使用 master-key 发送 GET /team/deleted 请求
    Then 响应状态码为 200
    And 响应 teams 数组包含 "archive-team-1"
    And 响应 teams 数组不包含未删除的 team

  Scenario: GET /model/deleted 返回已删除 model 列表
    Given model "archive-model-1" 已被软删除
    When 使用 master-key 发送 GET /model/deleted 请求
    Then 响应状态码为 200
    And 响应 models 数组包含 "archive-model-1"
    And 响应 models 数组不包含未删除的 model

  Scenario: GET /user/deleted 返回已删除 user 列表
    Given user "archive-user-1" 已被软删除
    When 使用 master-key 发送 GET /user/deleted 请求
    Then 响应状态码为 200
    And 响应 users 数组包含 "archive-user-1"
    And 响应 users 数组不包含未删除的 user

  Scenario: GET /org/deleted 返回已删除 org 列表
    Given org "archive-org-1" 已被软删除
    When 使用 master-key 发送 GET /org/deleted 请求
    Then 响应状态码为 200
    And 响应 orgs 数组包含 "archive-org-1"
    And 响应 orgs 数组不包含未删除的 org
```

### BDD Step 实现

`deleted_list.feature` 和 `router_settings.feature` 需要新增 BDD step 模块：

- `crates/aigw-server/tests/bdd_steps/deleted_list_steps.rs`
- `crates/aigw-server/tests/bdd_steps/router_settings_steps.rs`

参考现有 `keys_steps.rs`/`admin_jobs_steps.rs` 模式实现 `#[given]`/`#[when]`/`#[then]` 步骤。

### 验收门禁

- 新增 13 BDD 场景全绿（`task bdd`）
- 现有全量 BDD 回归无退化（目标 ~178 pass）
- 代码改动仅涉及 BDD step 文件 + feature 文件，不动业务代码

---

## Stage 99: 内部模块 + middleware 补测（14h）

### 核心预期

补齐调研报告发现的关键内部模块测试缺口，重点是 `daily_spend_queue.rs`（P0 风险——生产关键路径零测试）。

### 子任务

#### Part A: daily_spend_queue.rs 单元测试（P0，6h）

**风险等级**: 🔴 P0 — 每日消费预聚合批处理，跨 SQLite/MySQL/PG，当前零测试覆盖。

**实现计划**:
1. 在 `daily_spend_queue.rs` 末尾添加 `#[cfg(test)] mod tests`
2. 测试用例设计（参考现有 `rate_limiter.rs` 的单元测试模式）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_queue_single_kind_user() {
        // 验证：queue 一条 DailySpendKind::User → drain 后写入 daily_user_spend
    }

    #[tokio::test]
    async fn test_queue_multiple_kinds() {
        // 验证：queue User + Team + Org → drain 后三种 daily_spend 表各有记录
    }

    #[tokio::test]
    async fn test_drain_empty_queue_returns_zero() {
        // 验证：空队列 drain → 返回 0 条写入
    }

    #[tokio::test]
    async fn test_drain_flushes_all_pending() {
        // 验证：queue 10 条 → drain → 10 条全部写入
    }

    #[tokio::test]
    async fn test_daily_spend_sum_aggregation() {
        // 验证：同一 key 多次 queue → drain 后 daily_spend 正确 SUM
    }

    #[tokio::test]
    async fn test_queue_after_drain_restarts_count() {
        // 验证：drain 后新 queue → 新 drain 只写新记录
    }

    #[tokio::test]
    async fn test_concurrent_queue_and_drain() {
        // 验证：多生产者并发 queue + 单 drain 无数据竞态
    }
}
```

**关键约束**: `daily_spend_queue` 依赖 `Database`（db trait），测试需用 `sqlite::memory:` 实例。

#### Part B: rate_limiter 429 BDD（3h）

```gherkin
# rate_limit.feature（新建）

@mock
Feature: Rate Limiting — RPM/TPM enforcement

  Scenario: RPM 超限返回 429
    Given key "rate-test-key" 的 rpm_limit 为 2
    And 过去 1 分钟内已发送 2 个请求
    When 使用 key "rate-test-key" 发送 POST /v1/chat/completions 请求
    Then 响应状态码为 429
    And 响应体包含 "rate limit" 错误信息

  Scenario: TPM 超限返回 429
    Given key "token-test-key" 的 tpm_limit 为 100
    And 过去 1 分钟内已消费 100 tokens
    When 使用 key "token-test-key" 发送 POST /v1/chat/completions 请求（预估 tokens > 剩余额度）
    Then 响应状态码为 429

  Scenario: 未超限正常通过
    Given key "normal-key" 的 rpm_limit 为 100
    When 使用 key "normal-key" 发送 POST /v1/chat/completions 请求
    Then 响应状态码为 200
```

#### Part C: middleware auth_gateway.rs 单元测试（2h）

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_deployment_mode_onprem_bypasses_tenant_check() {}

    #[tokio::test]
    async fn test_deployment_mode_saas_enforces_org_isolation() {}

    #[tokio::test]
    async fn test_tenant_identity_from_header() {}

    #[tokio::test]
    async fn test_missing_tenant_header_in_saas_returns_401() {}
}
```

#### Part D: middleware rate_limit.rs 单元测试（3h）

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_enforce_limits_passes_when_under_budget() {}

    #[tokio::test]
    async fn test_enforce_limits_fails_when_over_budget() {}

    #[tokio::test]
    async fn test_enforce_limits_passes_when_under_rpm() {}

    #[tokio::test]
    async fn test_enforce_limits_fails_when_over_rpm() {}

    #[tokio::test]
    async fn test_enforce_limits_checks_both_budget_and_rate() {}
}
```

### 验收门禁

- `daily_spend_queue` UT 7 个全绿
- `rate_limiter` BDD 3 场景全绿
- `auth_gateway` UT 4 个全绿
- `rate_limit` middleware UT 5 个全绿
- 新增测试合计 **19 个**，现有全量 UT + BDD 回归无退化

---

## Stage 100: aigw-migrate 高级功能 BDD（10h）

### 核心预期

补齐 aigw-migrate 缺失的 BDD 场景——pre-check、verify、step-filter、skip-columns、cursor resume、MySQL/PG 特定路径。参考现有 `migration.feature` / `migration_sync.feature` 模式。

### Part A: PreCheck BDD（4 场景，3h）

```gherkin
# migration_precheck.feature（新建）

@real_api
Feature: aigw-migrate PreCheck — 迁移前 6 项自动化检查

  Scenario: PreCheck 全量通过（SQLite→SQLite）
    Given source 和 target 库表结构完整、数据一致、master_key 正确
    When 执行 aigw-migrate pre-check —source-url ... —target-url ... —master-key ...
    Then 6 项检查全部通过
    And 退出码为 0

  Scenario: PreCheck 源表缺失报错
    Given source 库缺少 proxy_models 表
    When 执行 aigw-migrate pre-check ...
    Then 退出码非 0
    And stderr 包含 "missing table" 或类似错误信息

  Scenario: PreCheck master_key 错误报错
    Given source 库表完整但 master_key 错误
    When 执行 aigw-migrate pre-check ...
    Then 退出码非 0
    And stderr 包含 "master_key" 或 "decrypt" 错误信息

  Scenario: PreCheck 源空表不报错（warning 级别）
    Given source 库某表行数为 0
    When 执行 aigw-migrate pre-check ...
    Then 退出码为 0（空表是 warning 不是 error）
    And stdout 包含 "0 rows" 或类似 warning
```

### Part B: Verify standalone BDD（2 场景，2h）

```gherkin
# migration_verify.feature（新建）

@real_api
Feature: aigw-migrate Verify — 迁移后 12 表行数比对

  Scenario: Verify 同 schema 库全匹配
    Given source 和 target 是相同数据的两个库
    When 执行 aigw-migrate verify —source-url ... —target-url ...
    Then 退出码为 0
    And stdout 显示 12 张表行数全部一致

  Scenario: Verify 行数不匹配报错
    Given target 库比 source 库少 1 行 spend_logs
    When 执行 aigw-migrate verify —source-url ... —target-url ...
    Then 退出码非 0
    And stdout 显示 spend_logs 行数不一致
```

### Part C: Step Filter + Skip Columns BDD（3 场景，3h）

```gherkin
# migration_advanced.feature（新建）

@real_api
Feature: aigw-migrate 高级功能 — step filter + skip columns

  Scenario: --step-filter 只执行 plain tables（step 2）
    Given source litellm 库有完整数据
    When 执行 remote-import —step-filter 2 —target-url ...
    Then 退出码为 0
    And plain tables（users/keys/teams/orgs 等）有数据
    And credentials/proxy_models/spend_logs 表为空（step 3-5 未执行）

  Scenario: --skip-body 跳过 spend_logs body 字段
    Given source 库 spend_logs 有 body 数据
    When 执行 remote-import —skip-body —target-url ...
    Then 退出码为 0
    And 迁移后的 spend_logs 中 messages/response/proxy_server_request 为 NULL

  Scenario: --skip-columns 跳指定列
    Given source 库有数据
    When 执行 remote-import —skip-columns spend_logs.messages —target-url ...
    Then 退出码为 0
    And spend_logs 迁移成功但 messages 列为 NULL
```

### Part D: Cursor Resume BDD（2 场景，2h）

```gherkin
# migration_cursor.feature（新建）

@real_api
Feature: aigw-migrate Cursor Resume — 断点续迁

  Scenario: spend_logs 断点续迁（cursor resume）
    Given source 库有 100 条 spend_logs（start_time 分布在 7 天内）
    When 执行 remote-import —spend-log-resume-after "2026-01-05T00:00:00Z"
    Then 退出码为 0
    And 只迁移了 start_time > 2026-01-05 的记录（约 50-60 条）
    And start_time <= 2026-01-05 的记录未被重复迁移

  Scenario: 幂等重跑不重复
    Given 迁移已完成
    When 再次执行相同命令
    Then 退出码为 0
    And target 库行数与上次一致（INSERT OR IGNORE 生效）
```

### 验收门禁

- 新增 11 BDD 场景全绿（`task bdd-real-sqlite`）
- real BDD 三后端（SQLite/PG/MySQL）中 pre-check + verify 覆盖三后端
- 现有 migrate 27 UT + 10 BDD 无退化
- 全量 BDD 回归 ≥ 190+ pass

---

## 门禁矩阵

| 层 | Stage 98 | Stage 99 | Stage 100 | 全量 |
|---|---------|---------|---------|------|
| aigw-core lib UT | 回归 | 回归 | 回归 | ≥264 + 新增 ≥16 = ≥280 |
| aigw-server lib UT | 回归 | 回归 | 回归 | ≥110 |
| aigw-migrate UT | 回归 | 回归 | 回归 | ≥27 |
| mock BDD | +13 场景 | +3 场景 | - | 178 + 16 = ≥194 |
| real BDD SQLite | - | - | +11 场景 | ≥47 |
| real BDD PG | - | - | +5 场景（pre-check + verify） | ≥41 |
| real BDD MySQL | - | - | +5 场景（pre-check + verify） | ≥41 |

## 依赖关系

```
Stage 98（路由 BDD）── 无依赖，可直接开始
Stage 99（内部 UT+BDD）── 无依赖，可与 98 并行
Stage 100（migrate BDD）── 无依赖，可与 98+99 并行
```

三个 Stage 修改不同文件（98 改 server BDD steps/features，99 改 aigw-core src 和 server BDD，100 改 aigw-migrate 测试），无合并冲突风险，可并行开发。

## 关键决策

1. **Phase 39 插队**: 原因——调研发现 `daily_spend_queue.rs` 零测试 + 10 个路由端点缺失 BDD，先补测试防回归再开新功能（budget reset Phase 40）是对 Phase 40 工程质量的保护。
2. **原 Phase 39 顺延为 Phase 40**: budget reset 工作 (Stage 94-97) 不受影响，仅编号后移。
3. **docs.rs 跳过 BDD**: 纯 HTML 页面，无业务逻辑，BDD 价值为零。
4. **其他低优先级纯内部模块不纳入本期**: `config.rs`、`deployment.rs`、`instance.rs`、`otel_tracing.rs`、`metrics.rs`（metrics 本身有 5 个 UT），BDD 价值低或无法 BDD。
5. **deleted_list BDD 用 mock**: 已有 `key_deleted_list` 作为参考实现，四张表 deleted_list 模式完全一致，mock API 即可验证路由+鉴权+分页逻辑。
6. **aigw-migrate BDD 用 real**: CLI 子命令的 BDD 需要通过 `task bdd-real-*` 调用真实 migrate 二进制 + 真实 DB（testcontainers），mock 无法覆盖 CLI 参数解析。
7. **`tenant.rs` 本期跳过**: 当前 SaaS 部署模式未启用，多租户 BDD 无实际场景可测。待 SaaS 部署需求确认后再补。

## 成功标准

- [ ] Stage 98: 13 BDD 全绿，全量回归 178→191+
- [ ] Stage 99: 19 UT+BDD 全绿，全量 UT 254→270+
- [ ] Stage 100: 11 real BDD 全绿，三后端一致
- [ ] BDD 端点覆盖率 ≥ 93%（从当前 84.5% 提升）
- [ ] 全量门禁: `task test` + `task bdd` + `task bdd-real-sqlite` + `task bdd-real-pg` + `task bdd-real-mysql` + `task fe-bdd` 全绿
- [ ] `daily_spend_queue.rs` 从零测试覆盖提升到 7 个 UT
- [ ] roadmap 同步更新: Phase 39→BDD Enhancement, Phase 40→Budget Reset

## 变更文件清单

### Stage 98
- `crates/aigw-server/tests/features/health.feature`（追加 5 场景）
- `crates/aigw-server/tests/features/router_settings.feature`（新建，4 场景）
- `crates/aigw-server/tests/features/deleted_list.feature`（新建，4 场景）
- `crates/aigw-server/tests/bdd_steps/health_steps.rs`（追加 step）
- `crates/aigw-server/tests/bdd_steps/router_settings_steps.rs`（新建）
- `crates/aigw-server/tests/bdd_steps/deleted_list_steps.rs`（新建）
- `crates/aigw-server/tests/bdd_steps/mod.rs`（注册新模块）

### Stage 99
- `crates/aigw-core/src/daily_spend_queue.rs`（追加 `#[cfg(test)] mod tests`）
- `crates/aigw-core/src/middleware/auth_gateway.rs`（追加 `#[cfg(test)] mod tests`）
- `crates/aigw-core/src/middleware/rate_limit.rs`（追加 `#[cfg(test)] mod tests`）
- `crates/aigw-server/tests/features/rate_limit.feature`（新建，3 场景）
- `crates/aigw-server/tests/bdd_steps/rate_limit_steps.rs`（新建）

### Stage 100
- `crates/aigw-server/tests/features/real/migration_precheck.feature`（新建）
- `crates/aigw-server/tests/features/real/migration_verify.feature`（新建）
- `crates/aigw-server/tests/features/real/migration_advanced.feature`（新建）
- `crates/aigw-server/tests/features/real/migration_cursor.feature`（新建）
- `crates/aigw-server/tests/bdd_steps/migration_steps.rs`（追加 `precheck`/`verify` step）

### 文档
- `docs/stages/stage-roadmap.md`（新增 Phase 39，原 Phase 39→Phase 40）
- `docs/stages/stage-98.md` ~ `stage-100.md`（新建）
- `docs/11-next-steps.md`（同步更新）
