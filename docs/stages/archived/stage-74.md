# Stage 74: 多 DB 真实端到端 BDD — activity 接口覆盖

**Phase**: 29 — Cross-DB BDD Hardening
**状态**: ⏳ 待开始（文档就绪，待命）
**预估**: 12h
**依赖**: Stage 73（提供 `real_db_seed` 灌数据工具 + `pub(crate)` helper）；与 Stage 72 无硬依赖可并行

---

## Context — 为什么是 activity 优先级最高

`/global/spend/activity`（`global_spend_activity` handler，spend.rs:815）调用 `query_activity_metadata`（db.rs:2200）+ `query_activity_daily`（db.rs:2247），是 spend 模块**方言适配代码量第一**的接口，且**零 BDD 覆盖**（mock 和 real 都无）。

### 三 DB 方言差异点（db.rs:2200-2301）

| 方言维度 | SQLite | MySQL | PostgreSQL |
|----------|--------|-------|------------|
| 占位符 | `?` | `?` | `$1`/`$2`/`$3`+ |
| 日期→文本 | `DATE(start_time)`（TEXT） | `CAST(DATE(start_time) AS CHAR)` | `DATE(start_time)::TEXT` |
| 动态过滤 | `build_activity_filter(...,0,false)` | 同 SQLite | `build_activity_filter(...,3,true)` 用 `$N` |

`build_activity_filter`（db.rs:2347）按 `use_dollar` 切换 `$N` vs `?`，并按 user_id/team_id/organization_id 动态拼 AND 子句 + 引号列名（`"user"`）。任一 DB 的占位符序号、类型转换、引号处理出错，都会在该 DB 下静默错误或报错——但 mock BDD 跑 SQLite **永远测不到 PG/MySQL 路径**。

### 与 keys/rankings 的区别

keys/rankings 是 LEFT JOIN + GROUP BY 列遗漏（已修）。activity 是**纯聚合（无 JOIN）但多重 SUM/COUNT/CASE WHEN + 日期类型转换 + 动态过滤占位符序号**，是另一类方言风险。本 Stage 验证 activity 在三 DB 下结果一致。

---

## 目标

| # | 目标 | 验收 |
|---|------|------|
| 1 | activity `@real_api @needs_upstream_db` 场景覆盖 metadata + daily | 三 DB 下 metadata 7 字段 + daily 数组结果一致 |
| 2 | 验证 user_id/team_id/org_id 三种动态过滤 | 带 filter 的请求三 DB 不报错且结果正确 |
| 3 | 验证日期边界（跨天 spend_logs 归入正确日期桶） | daily 数组按天分组正确 |
| 4 | 复用 Stage 73 的 `real_db_seed` 工具 | 不重复实现灌数据逻辑 |

---

## 实现方案

### Part A — feature 文件（1h）

**新建** `crates/aigw-server/tests/features/real/spend_activity_real.feature`：

```gherkin
@real_api @needs_upstream_db
Feature: /global/spend/activity 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 三种 DB 下验证 activity 聚合
  因为该接口三 DB 占位符/日期转换/动态过滤方言差异最大且零覆盖

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 activity 返回 metadata 和 daily
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入跨天 spend_logs 并查询 activity
    Then 响应状态码为 200
    And activity metadata 7 个字段数值正确
    And activity daily 按天分组且数值正确

  Scenario: activity 支持 user_id 过滤
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入不同 user 的 spend_logs 并带 user_id 查询 activity
    Then 响应状态码为 200
    And activity metadata 仅统计该 user 的数据

  Scenario: activity 支持 team_id 过滤
    When 向 aigw 测试库灌入不同 team 的 spend_logs 并带 team_id 查询 activity
    Then 响应状态码为 200
    And activity metadata 仅统计该 team 的数据

  Scenario: activity 无认证返回 401
    When 不携带 Authorization 发送 GET /global/spend/activity 请求
    Then 响应状态码为 401

  Scenario: 普通用户访问 activity 返回 403
    Given 通过 API 创建普通 key "act-nonadmin"
    When 使用 key "act-nonadmin" 发送 GET /global/spend/activity 请求
    Then 响应状态码为 403
```

### Part B — step bindings（5h）

**新建** `crates/aigw-server/tests/bdd_steps/spend_activity_steps.rs`，`mod.rs` 注册。

复用 Stage 73 的 `real_db_seed::seed_spend_logs` / `ensure_virtual_key`，灌入带 `user`/`team_id`/`start_time`（跨天）的 spend_logs：
- `seed_spend_logs` 需扩展支持 `user`/`team_id`/`start_time` 字段（Stage 73 的 `SeedRow` 已含 `ts_iso8601`，本 Stage 在此基础上追加可选 `user`/`team_id`/`organization_id`）。
- HTTP：`GET /global/spend/activity?start_date=...&end_date=...[&user_id=...][&team_id=...][&organization_id=...]`。

step：
- `#[when("向 aigw 测试库灌入跨天 spend_logs 并查询 activity")]`：灌入 2 天各若干条（含 success/failure 混合 status）→ HTTP 请求 → 存 body。
- `#[then("activity metadata 7 个字段数值正确")]`：断言 `metadata.{total_spend,total_requests,successful_requests,failed_requests,total_tokens,prompt_tokens,completion_tokens}` = 预期（按灌入数据算）。
- `#[then("activity daily 按天分组且数值正确")]`：断言 `daily` 数组长度 = 灌入天数、每行数值正确、按日期升序。
- `#[then("activity metadata 仅统计该 user 的数据")]`：带 user_id 过滤后 metadata 只含该 user 的行。
- team_id 过滤同上。

### Part C — 灌数据扩展（2h）

`real_db_seed::SeedRow` 增可选字段 `user`/`team_id`/`organization_id`/`status`，`seed_spend_logs` 的 INSERT SQL 追加这些列（用 `SourcePool` 拼，时间走 `time_literal`）。幂等清理 `DELETE FROM spend_logs WHERE request_id LIKE 'bdd-act-%'`。

---

## 验证

```bash
cargo check -p aigw-server --test bdd
cargo test --test bdd -p aigw-server                                    # mock 零回归
task bdd-real-pg && task bdd-real-mysql && task bdd-real-sqlite         # 三 DB 端到端
# 红→绿:临时改坏 build_activity_filter 的 PG $N 序号 → PG 场景失败 → 恢复 → 通过
```

## 门禁

- [ ] 三 DB 下 `spend_activity_real.feature` 全场景通过
- [ ] metadata 7 字段 + daily 分组三 DB 一致
- [ ] user_id/team_id 过滤三 DB 一致
- [ ] mock 零回归、SKIP 正常
