# Stage 73: 多 DB 真实端到端 BDD — Spend 聚合接口覆盖

**Phase**: 29 — Cross-DB BDD Hardening
**状态**: ⏳ 待开始（文档就绪，待命）
**预估**: 6h
**依赖**: Stage 69（提供 `aggregate_spend_by_keys` + `/global/spend/keys/rankings` 端点）；与 Stage 72 无硬依赖，可并行
**关联修复**: commit `29168b5`（GROUP BY `vk.key_alias` PG 报错修复）

---

## Context — 为什么做这件事

### 触发事件

`GET /global/spend/keys/rankings` 在 **PostgreSQL 部署**下报错：

```
SQL error: column "vk.key_alias" must appear in the GROUP BY clause
or be used in an aggregate function
```

### 根因

`crates/aigw-core/src/db.rs:2305` 的 `aggregate_spend_by_keys` SQL：

```sql
SELECT sl.api_key, vk.key_alias, SUM(...) ...
FROM spend_logs sl LEFT JOIN virtual_keys vk ON sl.api_key = vk.token
WHERE ...
GROUP BY sl.api_key          -- ← SELECT 选了 vk.key_alias 但 GROUP BY 只有 sl.api_key
```

PostgreSQL 严格执行 SQL 标准（非聚合列必须在 GROUP BY 中），SQLite/MySQL 默认宽松 → **bug 只在 PG 部署暴露**。

### 已有的两层修复（commit `29168b5`）

1. **代码修复**：`GROUP BY sl.api_key, vk.key_alias`。`vk.key_alias` 经 JOIN 条件 `sl.api_key = vk.token` 且 `vk.token` 是 PK，函数依赖于 `sl.api_key`，故分组基数不变，三后端语义等价。
2. **DB 层回归测试**：`crates/aigw-core/tests/integration_test.rs::test_postgres_aggregate_spend_by_keys` —— 用 testcontainers 起真实 PG，直调 `aggregate_spend_by_keys` 断言排序与 `key_alias` 回填。已做红→绿验证（回退修复复现 `42803` 报错，恢复后通过）。

### 缺口

DB 层有保护，**接口层（路由 / SpendAuth 鉴权 / HTTP 响应 JSON / 跨 DB 方言）没有**。而且本仓库 mock BDD 默认跑 SQLite（`bdd.rs:46` 用 `sqlite::memory:`），**永远无法发现这类跨 DB 方言差异**。需要把 spend 聚合类接口纳入多 DB 真实端到端 BDD。

### 现成基础设施（无需新建）

| 能力 | 位置 | 说明 |
|------|------|------|
| 自动建库 + 起真实 aigw 服务器 | `Taskfile.yml:44-92` `bdd-real-sqlite/pg/mysql` | 已有 3 个 task，设 `AIGW_TEST_DB_DRIVER` + `AIGW_TEST_START_SERVER=1` |
| 多 DB 生命周期 | `tests/bdd_support/test_db.rs` | `TestDatabaseManager::from_env()` 读 `AIGW_TEST_DB_DRIVER`，create/drop PG/MySQL/SQLite |
| `@real_api` 过滤 | `bdd.rs:113-125` | `AIGW_REAL_API=1` 时只跑 `@real_api` 场景 |
| 真实 HTTP 调用 + 清理 hook | `tests/bdd_steps/real_api_steps.rs` | `base_url()`/`client()`/`real_api_enabled()` + before/after scenario 清理 |
| 直连测试库灌数据 | `aigw-migrate::native::SourcePool` | `connect(url)` + `execute_raw(sql)` + `time_literal()` 跨方言时间字面量（native.rs:62/165/336） |
| 上游库 SKIP 模式 | `migration_sync_steps.rs:81` `bg_upstream_db_configured` | 无 `AIGW_UPSTREAM_DB_URL` 时优雅跳过，不报错 |

> **结论**：基础设施齐全，本 Stage 只需补 **1 个 feature 文件 + 1 个 step bindings 文件 + mod.rs 注册**，即可让 `/global/spend/keys/rankings` 在 SQLite/PG/MySQL 三种 DB 下端到端覆盖。

---

## 目标

| # | 目标 | 验收 |
|---|------|------|
| 1 | 新增 `@real_api @needs_upstream_db` 场景，HTTP 打真实 aigw 调 `/global/spend/keys/rankings` | PG/MySQL/SQLite 三 DB 下均 200 + 排序 + key_alias 回填 |
| 2 | 复用 `SourcePool::execute_raw` 直连 `AIGW_TEST_DB_URL` 灌确定性 spend_logs | 断言可重复，不依赖上游真实流量 |
| 3 | 无 `AIGW_UPSTREAM_DB_URL` 时优雅 SKIP | 不破坏纯 real-API（无上游库）环境 |
| 4 | mock BDD 路径零回归 | 新场景在 mock 模式 `set_skip_pass` 空跑通过 |

---

## 实现方案

### Part A — 新增 feature 文件（0.5h）

**新建** `crates/aigw-server/tests/features/real/spend_rankings_real.feature`，参照 `features/real/end_to_end_real.feature` 结构：

```gherkin
@real_api @needs_upstream_db
Feature: /global/spend/keys/rankings 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 三种 DB 下验证 keys 排名聚合
  以防跨 DB SQL 方言差异（如 PG 的 GROUP BY 严格模式）导致线上报错

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 keys/rankings 返回按 spend 降序的排名
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入两条已知 spend_logs 并查询 keys/rankings
    Then 响应状态码为 200
    And keys/rankings 首条 total_spend 最大且 key_alias 已回填
```

标签语义：
- `@real_api` — `bdd.rs:113-125` 过滤，仅 `AIGW_REAL_API=1` 时运行。
- `@needs_upstream_db` — 文档性标签，标记本场景需要 `AIGW_UPSTREAM_DB_URL`（或至少 `AIGW_TEST_DB_URL`）才能灌确定性数据；step 内做 SKIP 判断。

### Part B — 新增 step bindings（4h）

**新建** `crates/aigw-server/tests/bdd_steps/spend_rankings_steps.rs`，并在 `bdd_steps/mod.rs:19` 后追加 `pub mod spend_rankings_steps;`。

#### 复用现有 helper（最小改动：提为 `pub(crate)`）

`real_api_steps.rs:134-159` 的 `base_url()` / `client()` / `real_api_enabled()` 当前模块私有。改为 `pub(crate)`，在新文件 `use super::real_api_steps::{base_url, client, real_api_enabled};`。这是只增可见性的无风险改动，避免重复实现。

#### step 1：灌数据 + 发请求

```rust
#[when("向 aigw 测试库灌入两条已知 spend_logs 并查询 keys/rankings")]
async fn when_seed_and_query_rankings(world: &mut TestWorld) {
    // 关闭或无上游库 → set_skip_pass 占位，空跑通过
    if !real_api_enabled() || std::env::var("AIGW_UPSTREAM_DB_URL").is_err() {
        world.last_status = Some(200);
        world.last_body = Some(serde_json::json!([
            {"api_key":"k1","key_alias":"alias-a","total_spend":13.0},
            {"api_key":"k2","key_alias":"alias-b","total_spend":5.0}
        ]));
        return;
    }

    let test_db = std::env::var("AIGW_TEST_DB_URL")
        .expect("AIGW_TEST_DB_URL must be set by harness");
    let pool = aigw_migrate::native::SourcePool::connect(&test_db)
        .await.expect("connect test db");

    // 复用上游库中已存在的 virtual key token（或用 hash_token 构造确定性 key）
    // 用 SourcePool::time_literal() 生成跨方言时间字面量，避免再写方言分支
    // 插入 2 条 spend_logs：key_a 两次(10+3=13)、key_b 一次(5)
    // ... execute_raw(...) ...

    // HTTP 打真实 aigw
    let mk = world.master_key.clone();
    let resp = client()
        .get(format!("{}/global/spend/keys/rankings?start_date=2020-01-01&end_date=2030-12-31&limit=10", base_url()))
        .header("Authorization", format!("Bearer {}", mk))
        .send().await.expect("rankings request failed");
    world.last_status = Some(resp.status().as_u16());
    world.last_body = resp.json().await.ok();
}
```

**灌数据策略**（保证确定性）：
- 用 `aigw_core::crypto::hash_token("rank-key-a")` / `hash_token("rank-key-b")` 作为 `api_key`。
- 先确保 `virtual_keys` 中有对应记录（`key_alias` = `alias-a`/`alias-b`）——若无则 `INSERT INTO virtual_keys(token, key_alias, soft_budget_cooldown, spend, models, ...) VALUES (...)`（复用 `spend_steps.rs:67-111` 的 `VirtualKey` 字段集，但走 `SourcePool::execute_raw` 拼 SQL）。
- 再 `INSERT INTO spend_logs(request_id, api_key, spend, total_tokens, start_time, model, status, ...) VALUES (...)`，时间用 `pool.time_literal(iso8601)`。
- request_id 用固定前缀 + 区分（如 `bdd-rank-a-1`），便于幂等（`ON CONFLICT` 或先 `DELETE` 本场景特征行）。

#### step 2：断言

```rust
#[then("keys/rankings 首条 total_spend 最大且 key_alias 已回填")]
async fn then_rankings_sorted_and_alias(world: &mut TestWorld) {
    if !real_api_enabled() { return; }
    let body = world.last_body.as_ref().expect("no body");
    let arr = body.as_array().expect("rankings should be array");
    assert!(arr.len() >= 2, "should rank >= 2 keys, got {}", arr.len());
    let first_spend = arr[0].get("total_spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let second_spend = arr[1].get("total_spend").and_then(|v| v.as_f64()).unwrap_or(0.0);
    assert!(first_spend > second_spend, "first({}) should > second({})", first_spend, second_spend);
    let alias = arr[0].get("key_alias").and_then(|v| v.as_str());
    assert!(alias.is_some() && !alias.unwrap().is_empty(), "key_alias must be filled, got: {}", arr[0]);
}
```

### Part C — before-scenario hook 注册别名（0.5h，可选）

若灌数据用固定虚拟 key 而非上游已有 key，需把这些 key 的清理纳入 hook。最省事方案：灌数据 step 内部先 `DELETE FROM virtual_keys WHERE token IN (...)` + `DELETE FROM spend_logs WHERE request_id LIKE 'bdd-rank-%'`（幂等前置清理），则无需改 `KNOWN_TEST_ALIASES`。

**倾向**：step 内自清理，不改 `real_api_steps.rs:18` 的 `KNOWN_TEST_ALIASES`，保持改动隔离。

### Part D — 路由确认（mock 路径不动）

`tests/bdd_steps/common.rs::build_spend_router` 是 mock 路径用，**不补 rankings 路由** —— 本场景只走真实 HTTP（`bdd-real-*` task 已起服务器），不经 mock router。mock BDD 不会跑到这个 `@real_api` 场景。

---

## 不改的地方（明确边界）

| 文件 | 是否改 | 原因 |
|------|--------|------|
| `bdd.rs::ensure_state` | ❌ | mock 路径保持 `sqlite::memory:` |
| `common.rs::build_spend_router` | ❌ | mock 不跑此场景 |
| `Taskfile.yml` | ❌ | `bdd-real-pg/mysql/sqlite` 已存在，新场景自动被跑 |
| `.github/workflows/bdd.yml` | ❌ | `bdd-real` job `if: false` 保持（需密钥） |
| `integration_test.rs::test_postgres_aggregate_spend_by_keys` | ❌ 保留 | DB 层保护，与本 BDD（接口层）互补 |

---

## 验证（端到端）

### 1. 编译
```bash
cargo check -p aigw-server --test bdd
```

### 2. mock 路径不回归
```bash
cargo test --test bdd -p aigw-server
# 新场景在 mock 模式 set_skip_pass 空跑通过，不应新增失败
```

### 3. PG 真实端到端
```bash
# 起一个 PG + 跑迁移建表（或复用已有上游库）
docker run -d -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres:16
# 灌基础数据 / 指向已迁移的库
AIGW_REAL_API=1 AIGW_TEST_DB_DRIVER=postgres AIGW_TEST_START_SERVER=1 \
  AIGW_UPSTREAM_DB_URL=... task bdd-real-pg
# 或单跑该 feature：
AIGW_REAL_API=1 AIGW_BASE_URL=http://localhost:4000 AIGW_MASTER_KEY=... \
  cargo test --test bdd -p aigw-server -- features/real/spend_rankings_real.feature
```
断言：200 + 排序 + key_alias 非空。

### 4. MySQL / SQLite 真实端到端
同理 `task bdd-real-mysql` / `task bdd-real-sqlite`，验证三 DB 一致行为。

### 5. 红→绿再验证（证明场景有效）
```bash
# 临时回退 db.rs:2315 的 GROUP BY 修复（去掉 vk.key_alias）
# → PG 上该场景应失败（复现 42803 报错）
# 恢复修复 → 通过
```
与已提交的 `integration_test.rs` 集成测试形成**接口层 + DB 层双重保护**。

---

## 门禁标准

- [ ] `cargo check -p aigw-server --test bdd` 无 warning
- [ ] mock BDD 全量通过（新场景空跑不回归）
- [ ] PG/MySQL/SQLite 三 DB 下 `spend_rankings_real.feature` 通过
- [ ] 红→绿验证：回退修复复现 bug、恢复修复通过
- [ ] 无 `AIGW_UPSTREAM_DB_URL` 时不报错（SKIP）

---

## 后续跟进（不在本 Stage）

本 Stage 只覆盖 `keys/rankings`。同模式的 SQLite-only spend 聚合场景（`spend_aggregation.feature` 的 models/providers/logs）可逐步复制 `@real_api @needs_upstream_db` 版本到 `features/real/`，统一覆盖三 DB。遵循最小改动，本次不扩张。
