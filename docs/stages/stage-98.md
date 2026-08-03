# Stage 98: 路由端点 BDD 补全 — health metrics + router_settings PATCH + deleted_list

**Phase**: 40 — BDD Coverage Enhancement  
**优先级**: P0  
**状态**: ⏳ 待开始  
**预估**: 12h  
**前置**: 无（纯 BDD 新增，不改业务代码）

---

## 核心预期

补齐 2026-08-03 BDD 覆盖率审计发现的 **9 个路由端点缺失 BDD**，共 13 个场景：

| # | 端点 | Method + Path | 新 Feature 文件 | 场景数 |
|---|------|---------------|---------------|--------|
| 1 | `health_latest` | GET /health/latest | `health.feature` (追加) | 2 |
| 2 | `prometheus_metrics` | GET /metrics | `health.feature` (追加) | 1 |
| 3 | `health_metrics` | GET /health/metrics | `health.feature` (追加) | 2 |
| 4 | `patch_key` | PATCH /key/{token}/router/settings | `router_settings.feature` (新建) | 2 |
| 5 | `patch_team` | PATCH /team/{id}/router/settings | `router_settings.feature` (新建) | 2 |
| 6 | `team_deleted_list` | GET /team/deleted | `deleted_list.feature` (新建) | 1 |
| 7 | `model_deleted_list` | GET /model/deleted | `deleted_list.feature` (新建) | 1 |
| 8 | `user_deleted_list` | GET /user/deleted | `deleted_list.feature` (新建) | 1 |
| 9 | `org_deleted_list` | GET /org/deleted | `deleted_list.feature` (新建) | 1 |

**总计: 13 场景 × 3 feature 文件（1 追加 + 2 新建）**

---

## 设计要点

### Part A: health.feature 追加（5 场景）

`crates/aigw-server/tests/features/health.feature` 现有 6 场景已覆盖 `health`/`readiness`/`liveliness`/`system_info`/`model_health_check_all`/`model_health_check`。追加 5 场景：

```gherkin
# 追加到 health.feature 末尾

Scenario: health_latest 返回最新检查记录
  Given health_checks 表中有 3 条历史检查记录
  When 发送 GET /health/latest 请求
  Then 响应状态码为 200
  And 响应包含最新一条记录的 timestamp/model_name/status

Scenario: health_latest 无记录时返回空
  Given health_checks 表为空
  When 发送 GET /health/latest 请求
  Then 响应状态码为 200
  And 响应 body 为 {} 或空对象

Scenario: prometheus_metrics 返回期待格式
  When 发送 GET /metrics 请求
  Then 响应状态码为 200
  And Content-Type 为 Prometheus exposition format
  And 响应体包含 "aigw_" 前缀的指标名

Scenario: health_metrics 返回 JSON metrics（admin 认证）
  Given 数据库连接正常
  When 发送 GET /health/metrics 请求（admin 认证）
  Then 响应状态码为 200
  And 响应包含 uptime_seconds/db_pool_size/key_count/model_count 字段

Scenario: health_metrics 无认证返回 401
  When 发送 GET /health/metrics 请求（无认证）
  Then 响应状态码为 401
```

**BDD step 实现**: `crates/aigw-server/tests/bdd_steps/health_steps.rs` 追加对应 `#[given]`/`#[when]`/`#[then]` 步骤。
- `health_latest` 需 mock `health_checks` 表插入测试数据（1 条最新 success + 旧记录）。
- `prometheus_metrics` 需 mock 指标注册器含 `aigw_*` 指标（`health.rs` 的 `prometheus_metrics` handler 使用 `MetricsRecorder::registry()` 渲染）。
- `health_metrics` 需 mock admin 认证 + DB pool 状态。

### Part B: router_settings.feature（新建，4 场景）

新建文件 `crates/aigw-server/tests/features/router_settings.feature`：

```gherkin
@mock
Feature: Router Settings — key/team 级别覆盖

  Scenario: patch_key 设置 key 级别 router_settings
    Given 存在 key "patch-test-key"
    When 使用 master-key 发送 PATCH /key/patch-test-key/router/settings 请求
      | cooldown_time | 30 |
    Then 响应状态码为 200
    And GET /key/info?key=patch-test-key 返回 router_settings.cooldown_time 为 30

  Scenario: patch_key 无认证返回 401
    When 发送 PATCH /key/some-key/router/settings 请求（无认证）
    Then 响应状态码为 401

  Scenario: patch_team 设置 team 级别 router_settings
    Given 存在 team "patch-team-1"
    When 使用 master-key 发送 PATCH /team/patch-team-1/router/settings 请求
      | retry_count | 2 |
    Then 响应状态码为 200
    And GET /team/info?team_id=patch-team-1 返回 router_settings.retry_count 为 2

  Scenario: patch_team 无认证返回 401
    When 发送 PATCH /team/some-team/router/settings 请求（无认证）
    Then 响应状态码为 401
```

**BDD step 实现**: `crates/aigw-server/tests/bdd_steps/router_settings_steps.rs`（新建），参考 `router_settings_steps.rs` 现有的 `get_global`/`put_global` step 模式。
- `patch_key` 需 mock `PATCH /key/{token}/router/settings` 路由 + JSON body 解析。
- `patch_team` 同上，参数为 `team_id` 字符串。

### Part C: deleted_list.feature（新建，4 场景）

新建文件 `crates/aigw-server/tests/features/deleted_list.feature`：

```gherkin
@mock
Feature: Deleted Entity Lists — 软删除实体归档查询

  Background:
    Given 数据库中已有 "team-1"/"team-2" 两个正常 team

  Scenario: GET /team/deleted 返回已删除 team
    Given team "team-x" 已被软删除（deleted_at 已记录）
    When 使用 admin 认证发送 GET /team/deleted 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "teams" 数组包含 team_alias 为 "team-x" 的记录
    And "team-1"/"team-2" 不在返回结果中

  Scenario: GET /model/deleted 返回已删除 model
    Given model "model-archived" 已被软删除
    When 使用 admin 认证发送 GET /model/deleted 请求
    Then 响应状态码为 200
    And "models" 数组包含 model_name 为 "model-archived" 的记录

  Scenario: GET /user/deleted 返回已删除 user
    Given user "deleted-user" 已被软删除
    When 使用 admin 认证发送 GET /user/deleted 请求
    Then 响应状态码为 200
    And "users" 数组包含 user_id 为 "deleted-user" 的记录

  Scenario: GET /org/deleted 返回已删除 org
    Given org "old-org" 已被软删除
    When 使用 admin 认证发送 GET /org/deleted 请求
    Then 响应状态码为 200
    And "orgs" 数组包含 organization_id 为 "old-org" 的记录
```

**BDD step 实现**: `crates/aigw-server/tests/bdd_steps/deleted_list_steps.rs`（新建）。四张表的 `deleted_list` step 模式完全一致（参考 `keys_steps.rs` 的 `key_deleted_list` step），可复用同一个 when + 四个给定。

---

## BDD step 模块注册

- `crates/aigw-server/tests/bdd_steps/mod.rs` 添加：
  - `pub mod router_settings_steps;`
  - `pub mod deleted_list_steps;`
- 不需要修改 `router_settings_steps.rs` 的模块声明（已存在），仅追加 `patch_key`/`patch_team` 两个 handler 的 step。

---

## 方言差异与 real BDD 必要性

本 Stage 的所有 BDD 场景都是 **mock BDD**（`@mock` 标签，sqlite::memory:）。理由：
- `deleted_list` 端点：纯 SELECT 查询，SQL 语法三方言一致（`LIMIT ? OFFSET ?`），mock 覆盖路由+鉴权+分页逻辑，方言差异低风险。
- `router_settings` PATCH：JSON column merge 是 Rust 侧逻辑，不涉及 SQL 方言差异。
- `health_latest`/`prometheus_metrics`/`health_metrics`：不涉及跨方言 SQL，纯 HTTP 端点逻辑。

**不需要 real BDD**。

---

## TDD

- **BDD（mock）**: 13 场景（health 5 + router_settings 4 + deleted_list 4）
- 先写 Gherkin 场景 → 定义 step 骨架 → `task bdd` 跑红 → 实现 step → 跑绿
- 引用现有 `@mock` test runner（`bdd.rs`），无需新建 runner

---

## 验收门禁

| task | 类型 | 预期 | 说明 |
|------|------|------|------|
| `task bdd` | **mock BDD** | 新增 13 + 回归 ~178 = ~191 pass | 主要验证：`@mock` 标签，sqlite::memory: |
| `task test` | 单元测试 | 回归无退化 | aigw-core + aigw-server lib 全绿 |
| `task fe-bdd` | 前端 BDD | 回归无退化 | 前端无变更，仅回归 |

> **本 Stage 仅涉及 `task bdd`（mock BDD），不涉及 `task bdd-real-mysql/pg/sqlite`（real BDD），也不涉及 `task fe-bdd` 新增场景。**
