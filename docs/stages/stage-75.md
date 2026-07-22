# Stage 75: 多 DB 真实端到端 BDD — models + providers 聚合覆盖

**Phase**: 29 — Cross-DB BDD Hardening
**状态**: ⏳ 待开始（文档就绪，待命）
**预估**: 10h
**依赖**: Stage 73（`real_db_seed` 工具 + helper）；与 Stage 74 无硬依赖可并行

---

## Context

`/spend/models`、`/global/spend/models`（handler spend.rs:604/761 → `aggregate_spend_by_model` db.rs:1420/1683/1918）和 `/spend/providers`、`/global/spend/providers`（handler spend.rs:642/655 → `aggregate_spend_by_provider` db.rs:1435/1698/1936）是 GROUP BY 聚合接口。mock BDD 有基础覆盖（auth + 有数据 200），但**无 real 多 DB 覆盖**。

### 方言风险点

1. **PG 版日期内联 + 注入面**（db.rs:1919-1925 / 1937-1943）：PG 分支把 start/end_date 用 `'...'::TIMESTAMPTZ` **字符串内联**而非 bind 参数（SQLite/MySQL 用 `?` bind）。虽有 `.replace('\'', "''")` 转义，但内联拼接路径在 PG 下与 SQLite/MySQL 行为差异最大，且易在边界（空日期、异常格式）出问题。
2. **providers 的 `COALESCE(NULLIF(custom_llm_provider,''),'unknown')`**：三 DB 对 NULLIF/COALESCE 空值处理一致，但 `sl.` 前缀 + 引号列名在不同 DB 行为需验证。
3. **handler 二次加工**：`global_spend_providers`（spend.rs:655）有 `build_decrypted_provider_map`（spend.rs:713）解密逻辑，属应用层，三 DB 下 DB 层结果一致后该层应一致。

### 覆盖矩阵（附录 A 摘录）

| 路由 | DB 方法 | 聚合 | Mock BDD | Real BDD | 风险 |
|------|---------|------|----------|----------|------|
| `/spend/models` | aggregate_spend_by_model | GROUP BY | ✅多 | ❌ | 高 |
| `/spend/providers` | aggregate_spend_by_provider | GROUP BY | ✅多 | ❌ | 高 |
| `/global/spend/models` | 同 | GROUP BY | ✅ | ❌ | 高 |
| `/global/spend/providers` | 同 | GROUP BY | ✅ | ❌ | 高 |

---

## 目标

| # | 目标 | 验收 |
|---|------|------|
| 1 | models 四路由 `@real_api @needs_upstream_db` 场景 | 三 DB 下 GROUP BY model 结果一致 |
| 2 | providers 四路由场景 | 三 DB 下 GROUP BY provider（含 unknown 兜底）一致 |
| 3 | 验证日期过滤（PG 内联 vs SQLite/MySQL bind） | 带日期过滤三 DB 结果一致 |
| 4 | 复用 Stage 73 `real_db_seed` | 不重复实现 |

---

## 实现方案

### Part A — feature（1h）

**新建** `crates/aigw-server/tests/features/real/spend_models_providers_real.feature`：

```gherkin
@real_api @needs_upstream_db
Feature: /spend/models + /spend/providers 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 下验证 model/provider 聚合
  因为 PG 版日期内联拼接与 SQLite/MySQL bind 参数行为差异大

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 /global/spend/models 按 model 聚合
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入多 model 的 spend_logs 并查询 /global/spend/models
    Then 响应状态码为 200
    And models 聚合按 model 分组且数值正确

  Scenario: /global/spend/models 支持日期过滤
    When 向 aigw 测试库灌入跨日期 spend_logs 并带日期查询 /global/spend/models
    Then 响应状态码为 200
    And models 聚合仅含日期范围内的数据

  Scenario: master-key 查询 /global/spend/providers 按 provider 聚合
    When 向 aigw 测试库灌入多 provider 的 spend_logs 并查询 /global/spend/providers
    Then 响应状态码为 200
    And providers 聚合按 provider 分组且空 provider 兜底为 unknown

  Scenario: /spend/models 需认证
    When 发送 GET /spend/models 请求（无认证）
    Then 响应状态码为 401

  Scenario: /global/spend/models 需管理员
    Given 通过 API 创建普通 key "mp-nonadmin"
    When 使用 key "mp-nonadmin" 发送 GET /global/spend/models 请求
    Then 响应状态码为 403
```

### Part B — step bindings（6h）

**新建** `crates/aigw-server/tests/bdd_steps/spend_models_providers_steps.rs`，`mod.rs` 注册。

复用 `real_db_seed::seed_spend_logs`，灌入多 `model`/`custom_llm_provider`（含空 provider）的 spend_logs：
- `#[when("向 aigw 测试库灌入多 model 的 spend_logs 并查询 /global/spend/models")]`：灌入 3 个 model 各若干条 → HTTP `/global/spend/models` → 存 body。
- `#[then("models 聚合按 model 分组且数值正确")]`：断言返回数组按 model 分组、`total_tokens`/`total_spend`/`requests` = 预期。
- 日期过滤场景：灌入跨日期数据，带 `start_date`/`end_date` 查询，断言只含范围内。
- providers 场景：灌入含空 `custom_llm_provider` 的行，断言兜底为 `unknown`。

### Part C — 灌数据扩展（1h）

`SeedRow` 已含 `model`/`status`/`ts_iso8601`，追加可选 `custom_llm_provider`（可空）。`seed_spend_logs` INSERT 追加该列。

---

## 验证

```bash
cargo check -p aigw-server --test bdd
cargo test --test bdd -p aigw-server
task bdd-real-pg && task bdd-real-mysql && task bdd-real-sqlite
# 红→绿:临时把 PG 分支日期改回 bind $N 但漏改序号 → PG 失败 → 恢复 → 通过
```

## 门禁

- [ ] 三 DB 下 models/providers 四路由场景通过
- [ ] 日期过滤三 DB 一致（重点验证 PG 内联路径）
- [ ] 空 provider 兜底 unknown 三 DB 一致
- [ ] mock 零回归、SKIP 正常
