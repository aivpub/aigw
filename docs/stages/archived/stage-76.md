# Stage 76: 多 DB 真实端到端 BDD — SUM 聚合簇 + 应用层 keys 覆盖

**Phase**: 29 — Cross-DB BDD Hardening
**状态**: ⏳ 待开始（文档就绪，待命）
**预估**: 12h
**依赖**: Stage 73（`real_db_seed` 工具 + helper）；与 Stage 74/75 无硬依赖可并行

---

## Context

本 Stage 覆盖剩余 spend 接口：纯 SUM 聚合（keys/users/tags/global）+ 应用层聚合（global/keys）。这些接口 SQL 较简单，但有**引号列名**和 **LIKE 转义**等跨 DB 细节，且 4 个接口中 `/spend/users`、`/spend/tags` **零 BDD 覆盖**。

### 方言风险点

| 接口 | DB 方法 | 风险 |
|------|---------|------|
| `/spend/keys` | get_spend_by_key (db.rs:1385等) | 简单 SUM WHERE，低风险，补覆盖 |
| `/spend/users` | get_spend_by_user (db.rs:1394等) | **`"user"` 引号列名**（SQLite/PG 需引号、MySQL 不用），零覆盖，**高** |
| `/spend/tags` | get_spend_by_tag (db.rs:1403等) | SUM + `LIKE '%tag%'`，PG 版 `request_tags::text LIKE` 特殊 cast，零覆盖，**高** |
| `/global/spend` | get_global_spend (db.rs:1413等) | 全表 SUM，低风险 |
| `/global/spend/keys` | query_spend_logs (db.rs:2064) | 应用层聚合（内存 SUM + 分组），低-中 |

`/spend/users` 的 `"user"` 列名：SQLite/PG 把 `user` 当保留字需双引号，MySQL 不需——这是又一类三 DB 差异点，零覆盖。

---

## 目标

| # | 目标 | 验收 |
|---|------|------|
| 1 | keys/users/tags/global 四 SUM 路由 `@real_api @needs_upstream_db` 场景 | 三 DB 下 SUM 结果一致 |
| 2 | global/keys 应用层聚合场景 | 三 DB 下应用层聚合结果一致 |
| 3 | 重点验证 `/spend/users` 的 `"user"` 引号列名 | 三 DB 下按 user 聚合正确 |
| 4 | 重点验证 `/spend/tags` 的 LIKE 转义 | 三 DB 下 tag 匹配正确 |
| 5 | 复用 Stage 73 `real_db_seed` | 不重复实现 |

---

## 实现方案

### Part A — feature（1h）

**新建** `crates/aigw-server/tests/features/real/spend_sum_cluster_real.feature`：

```gherkin
@real_api @needs_upstream_db
Feature: SUM 聚合簇 + 应用层 keys 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 下验证简单 SUM 与应用层聚合
  因为 /spend/users 的 "user" 引号列名、/spend/tags 的 LIKE 转义存在跨 DB 差异

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 /global/spend 返回总 spend
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入若干 spend_logs 并查询 /global/spend
    Then 响应状态码为 200
    And global spend 等于灌入总额

  Scenario: 使用带 user 的 key 查询 /spend/users
    Given 通过 API 创建带 user_id 的普通 key "sum-user-key"
    When 向 aigw 测试库灌入该 user 的 spend_logs 并使用该 key 查询 /spend/users
    Then 响应状态码为 200
    And spend/users 返回该 user 的累计 spend

  Scenario: master-key 查询 /spend/tags 按 tag 匹配
    When 向 aigw 测试库灌入带 request_tags 的 spend_logs 并查询 /spend/tags?tag=X
    Then 响应状态码为 200
    And spend/tags 返回匹配 tag 的累计 spend

  Scenario: master-key 查询 /global/spend/keys 应用层聚合
    When 向 aigw 测试库灌入多 key 的 spend_logs 并查询 /global/spend/keys
    Then 响应状态码为 200
    And global/spend/keys 应用层聚合结果正确

  Scenario: /spend/users 无认证返回 401
    When 发送 GET /spend/users 请求（无认证）
    Then 响应状态码为 401

  Scenario: /spend/tags 缺少 tag 参数返回 400
    Given 一个普通 key "tag-noarg" 已生成
    When 使用 key "tag-noarg" 发送 GET /spend/tags 请求（无 tag 参数）
    Then 响应状态码为 400
```

### Part B — step bindings（6h）

**新建** `crates/aigw-server/tests/bdd_steps/spend_sum_cluster_steps.rs`，`mod.rs` 注册。

复用 `real_db_seed`，关键扩展：
- `SeedRow` 追加可选 `user`/`request_tags`（JSON）。`seed_spend_logs` INSERT 追加这两列（`request_tags` 用 `SourcePool` 按方言序列化，或直接 NULLIF 兜底）。
- `#[when("向 aigw 测试库灌入该 user 的 spend_logs 并使用该 key 查询 /spend/users")]`：灌入指定 `user` 的行，用该 user 的 key（HTTP `/spend/users` 用 SpendAuth 的 `auth.user_id`）查询 → 断言 `spend` = 预期。**重点验证 `"user"` 列名三 DB 一致**。
- `#[when("...查询 /spend/tags?tag=X")]`：灌入 `request_tags` 含 X 的行 → HTTP `/spend/tags?tag=X` → 断言匹配。**重点验证 LIKE 三 DB 一致**。
- `/global/spend/keys`：灌入多 key 行 → HTTP → 断言应用层聚合（按 api_key 分组 + SUM）。

### Part C — 灌数据扩展（2h）

`SeedRow` 增 `user: Option<String>`、`request_tags: Option<serde_json::Value>`。`seed_spend_logs`：
- `user` 列：直接插入（列名 `"user"` 在三 DB schema 已统一加引号，见 migration）。
- `request_tags`：按方言序列化（PG/MySQL JSON、SQLite TEXT），或用 `SourcePool::value_to_target_literal`（native.rs:827）统一。

### Part D — 带条件的 key 创建（1h）

`/spend/users` 用 SpendAuth 的 `user_id`，需创建带 `user_id` 的 virtual key。`real_db_seed::ensure_virtual_key` 扩展可选 `user_id` 参数，或通过 `/key/generate` API 创建（复用 `real_api_steps::create_key_via_api`，但需传 user_id——可能需新 step）。

---

## 验证

```bash
cargo check -p aigw-server --test bdd
cargo test --test bdd -p aigw-server
task bdd-real-pg && task bdd-real-mysql && task bdd-real-sqlite
# 红→绿:
#   /spend/users: 临时去掉 PG 分支 "user" 的引号 → PG 报语法错 → 恢复 → 通过
#   /spend/tags: 临时改坏 PG 的 request_tags::text cast → PG 失败 → 恢复 → 通过
```

## 门禁

- [ ] 三 DB 下 SUM 聚合簇四路由 + 应用层 keys 场景通过
- [ ] `/spend/users` 的 `"user"` 列名三 DB 一致（红→绿验证）
- [ ] `/spend/tags` 的 LIKE 匹配三 DB 一致（红→绿验证）
- [ ] mock 零回归、SKIP 正常

---

## Phase 29 收尾（本 Stage 完成后）

Stage 73-76 全部完成后，Phase 29 覆盖：

| 接口 | Stage | 风险降级 |
|------|-------|----------|
| `/global/spend/keys/rankings` | 73 | 极高→已控 |
| `/global/spend/activity` | 74 | 极高→已控 |
| `/spend/models` + `/global/spend/models` | 75 | 高→已控 |
| `/spend/providers` + `/global/spend/providers` | 75 | 高→已控 |
| `/spend/users` | 76 | 高→已控 |
| `/spend/tags` | 76 | 高→已控 |
| `/spend/keys` | 76 | 中→已控 |
| `/global/spend` | 76 | 中→已控 |
| `/global/spend/keys` | 76 | 低-中→已控 |

明细接口（`/spend/logs`、`/global/spend/logs`）低风险，`/spend/logs` 已有 real BDD，不纳入 Phase 29 核心。

**Phase 29 合计**: 4 Stage，44h，覆盖 11/13 spend 接口（含全部 5 个聚合高风险 + 2 个极高）。
