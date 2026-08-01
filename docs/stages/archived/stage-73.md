# Stage 73: 多 DB 真实端到端 BDD 基础设施 + keys/rankings 覆盖

**Phase**: 29 — Cross-DB BDD Hardening
**状态**: ⏳ 待开始（文档就绪，待命）
**预估**: 10h
**依赖**: Stage 69（提供 `aggregate_spend_by_keys` + `/global/spend/keys/rankings` 端点）；与 Stage 72 无硬依赖可并行
**关联修复**: commit `29168b5`（GROUP BY `vk.key_alias` PG 报错修复）
**本 Phase**: Stage 73-76 的 4 个 Stage 中的**第 1 个，奠定基础设施**

---

## Context — 为什么做这件事

### 触发事件

`GET /global/spend/keys/rankings` 在 **PostgreSQL 部署**下报错：

```
SQL error: column "vk.key_alias" must appear in the GROUP BY clause
or be used in an aggregate function
```

### 根因

`crates/aigw-core/src/db.rs:2305` 的 `aggregate_spend_by_keys` SQL：`SELECT` 选了 `vk.key_alias`（来自 LEFT JOIN）但 `GROUP BY` 只有 `sl.api_key`。PostgreSQL 严格执行 SQL 标准（非聚合列必须在 GROUP BY 中），SQLite/MySQL 默认宽松 → **bug 只在 PG 部署暴露**。

### 已有修复（commit `29168b5`）

1. **代码修复**：`GROUP BY sl.api_key, vk.key_alias`（`vk.key_alias` 经 JOIN 条件 + PK 函数依赖，分组基数不变）。
2. **DB 层回归测试**：`integration_test.rs::test_postgres_aggregate_spend_by_keys`（testcontainers 真实 PG，红→绿已验证）。

### 缺口与本 Phase 定位

DB 层有保护，**接口层（路由/SpendAuth/HTTP 响应/跨 DB 方言）无多 DB 覆盖**。本仓库 mock BDD 默认跑 SQLite（`bdd.rs:46` `sqlite::memory:`），**永远无法发现跨 DB 方言差异**。

调研（见附录 A）发现 13 个 spend 接口中 **4 个零 BDD 覆盖**，其中 `activity` 和 `keys/rankings` 是方言代码最多、风险最高的两个。本 Phase 把 spend 聚合接口拆成 4 个 Stage 纳入多 DB 真实端到端 BDD，每个 8-12h：

| Stage | 主题 | 预估 | 接口 |
|-------|------|------|------|
| **73（本）** | 基础设施 + keys/rankings | 10h | `/global/spend/keys/rankings`（极高，已修） |
| 74 | activity（方言代码最多） | 12h | `/global/spend/activity`（极高，零覆盖） |
| 75 | models + providers | 10h | `/spend/{models,providers}` + `/global/spend/{models,providers}`（高） |
| 76 | SUM 聚合簇 + 应用层 | 12h | `/spend/{keys,users,tags}` + `/global/spend` + `/global/spend/keys`（高/中） |

**Stage 73 是基础**：提取 `pub(crate)` helper + 封装 `SourcePool` 灌数据/清理的可复用工具，供 74-76 复用，避免每个 Stage 重复造轮子。

---

## 现成基础设施（无需新建，本 Stage 复用并提为可复用）

| 能力 | 位置 | 说明 |
|------|------|------|
| 自动建库 + 起真实 aigw | `Taskfile.yml:44-92` `bdd-real-sqlite/pg/mysql` | 设 `AIGW_TEST_DB_DRIVER` + `AIGW_TEST_START_SERVER=1` |
| 多 DB 生命周期 | `tests/bdd_support/test_db.rs` | `TestDatabaseManager::from_env()` |
| `@real_api` 过滤 | `bdd.rs:113-125` | `AIGW_REAL_API=1` 只跑 `@real_api` 场景 |
| 直连测试库灌数据 | `aigw-migrate::native::SourcePool` | `connect(url)` + `execute_raw(sql)` + `time_literal()`（native.rs:62/165/336） |
| 上游库 SKIP 模式 | `migration_sync_steps.rs:81` | 无 `AIGW_UPSTREAM_DB_URL` 时优雅跳过 |

---

## 目标

| # | 目标 | 验收 |
|---|------|------|
| 1 | 提取可复用 helper（`pub(crate)`）+ 封装 SourcePool 灌数据/清理工具 | 74-76 直接 `use`，不重复实现 |
| 2 | keys/rankings `@real_api @needs_upstream_db` 场景 | SQLite/PG/MySQL 三 DB 下 200 + 排序 + key_alias 回填 |
| 3 | 无 `AIGW_UPSTREAM_DB_URL` 时优雅 SKIP | 不破坏纯 real-API 环境 |
| 4 | mock BDD 路径零回归 | 新场景 mock 模式 `set_skip_pass` 空跑通过 |

---

## 实现方案

### Part A — 基础设施：可复用 helper + SourcePool 工具（3h）

**A1. 提取 `pub(crate)` helper**（`real_api_steps.rs:134-159`）

`base_url()` / `client()` / `real_api_enabled()` 当前模块私有 → 改 `pub(crate)`，供 74-76 `use super::real_api_steps::{...}`。只增可见性，无风险。

**A2. 新建 `crates/aigw-server/tests/bdd_steps/real_db_seed.rs`** —— 可复用灌数据工具

封装供 74-76 通用：
```rust
pub(crate) async fn seed_spend_logs(db_url: &str, rows: &[SeedRow]) -> Result<()>
// 用 SourcePool::connect + execute_raw + time_literal 跨方言插入 spend_logs
// 内部先 DELETE WHERE request_id LIKE 'bdd-%' 幂等前置清理

pub(crate) async fn ensure_virtual_key(db_url: &str, token_hash: &str, alias: &str) -> Result<()>
// 幂等插入 virtual_keys（复用 spend_steps.rs:67-111 字段集）

pub(crate) struct SeedRow { pub request_id, pub api_key, pub spend, pub total_tokens, pub model, pub status, pub ts_iso8601 }

pub(crate) async fn query_rankings_via_http(mk: &str) -> (u16, Option<Value>)
// GET {base_url}/global/spend/keys/rankings?... 带 master key
```
在 `bdd_steps/mod.rs` 注册 `pub mod real_db_seed;`。

### Part B — keys/rankings feature + step（5h）

**B1. 新建** `crates/aigw-server/tests/features/real/spend_rankings_real.feature`：

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

  Scenario: 无认证访问 keys/rankings 返回 401
    When 不携带 Authorization 发送 GET /global/spend/keys/rankings 请求
    Then 响应状态码为 401

  Scenario: 普通用户访问 keys/rankings 返回 403
    Given 通过 API 创建普通 key "rank-nonadmin"
    When 使用 key "rank-nonadmin" 发送 GET /global/spend/keys/rankings 请求
    Then 响应状态码为 403
```

**B2. 新建** `crates/aigw-server/tests/bdd_steps/spend_rankings_steps.rs`：

- `#[when("向 aigw 测试库灌入两条已知 spend_logs 并查询 keys/rankings")]`：关/无上游库 → `set_skip_pass`；否则 `real_db_seed::ensure_virtual_key` × 2 + `seed_spend_logs`（key_a 10+3=13、key_b 5）→ `query_rankings_via_http`。
- `#[then("keys/rankings 首条 total_spend 最大且 key_alias 已回填")]`：断言数组 len≥2、`[0].total_spend > [1].total_spend`、`[0].key_alias` 非空。
- 401/403 场景复用现有 step 或新增最小 step。

### Part C — mock 路径不动（确认）

`bdd.rs::ensure_state`（mock `sqlite::memory:`）与 `common.rs::build_spend_router` 不改 —— 本场景只走真实 HTTP，mock BDD 不跑 `@real_api` 场景。

---

## 不改的地方

| 文件 | 原因 |
|------|------|
| `bdd.rs::ensure_state` | mock 路径保持 SQLite |
| `common.rs::build_spend_router` | mock 不跑此场景 |
| `Taskfile.yml` | `bdd-real-*` 已存在，新场景自动被跑 |
| `.github/workflows/bdd.yml` | `bdd-real` job `if: false` 保持（需密钥） |
| `integration_test.rs::test_postgres_aggregate_spend_by_keys` | DB 层保护，保留与本 BDD 互补 |

---

## 验证

```bash
cargo check -p aigw-server --test bdd                              # 1. 编译
cargo test --test bdd -p aigw-server                                # 2. mock 零回归
AIGW_REAL_API=1 AIGW_TEST_DB_DRIVER=postgres AIGW_TEST_START_SERVER=1 \
  AIGW_UPSTREAM_DB_URL=... task bdd-real-pg                         # 3. PG 端到端
task bdd-real-mysql && task bdd-real-sqlite                         # 4. MySQL/SQLite
# 5. 红→绿:临时回退 db.rs:2318 GROUP BY 修复 → PG 场景复现 42803 → 恢复 → 通过
```

## 门禁

- [ ] `cargo check` 无 warning
- [ ] mock BDD 全量通过（新场景空跑不回归）
- [ ] PG/MySQL/SQLite 三 DB 下 `spend_rankings_real.feature` 通过
- [ ] 红→绿验证通过
- [ ] 无 `AIGW_UPSTREAM_DB_URL` 时不报错（SKIP）

---

## 附录 A — 覆盖范围调研（2026-07-22）

13 个 spend 路由覆盖矩阵：

| 路由 | DB 方法(db.rs) | 聚合? | Mock BDD | Real BDD | 风险 |
|------|----------------|-------|----------|----------|------|
| `/spend/logs` | query_spend_logs_with_status_filter (3940) | 明细 | ✅多 | ✅ | 低 |
| `/spend/keys` | get_spend_by_key (2076) | SUM | auth | ❌ | 中 |
| `/spend/users` | get_spend_by_user (2084) | SUM | ❌ | ❌ | **高** |
| `/spend/tags` | get_spend_by_tag (2092) | SUM+LIKE | ❌ | ❌ | **高** |
| `/spend/models` | aggregate_spend_by_model (2108) | GROUP BY | ✅多 | ❌ | **高** |
| `/spend/providers` | aggregate_spend_by_provider (2121) | GROUP BY | ✅多 | ❌ | **高** |
| `/global/spend` | get_global_spend (2100) | SUM | auth | ❌ | 中 |
| `/global/spend/logs` | query_spend_logs_with_status_filter | 明细 | ✅多 | ❌ | 低 |
| `/global/spend/keys` | query_spend_logs (2064) | 应用层 | ❌ | ❌ | 低-中 |
| `/global/spend/models` | aggregate_spend_by_model (2108) | GROUP BY | ✅ | ❌ | **高** |
| `/global/spend/providers` | aggregate_spend_by_provider (2121) | GROUP BY | ✅ | ❌ | **高** |
| `/global/spend/activity` | query_activity_metadata(2200)+daily(2247) | GROUP BY+CASE | ❌ | ❌ | **极高** |
| `/global/spend/keys/rankings` | aggregate_spend_by_keys (2305) | LEFT JOIN+GROUP BY | ❌ | ❌ | **极高**(已修) |

**关键发现**：
- 4 接口零 BDD 覆盖：`/spend/users`、`/spend/tags`、`/global/spend/activity`、`/global/spend/keys/rankings`，后两个方言代码最多。
- `/global/spend/activity`（db.rs:2200-2301）三 DB 占位符(`$N` vs `?`)、类型转换(MySQL `CAST(DATE() AS CHAR)` / PG `DATE()::TEXT` / SQLite `DATE()`)、`build_activity_filter`(db.rs:2347)方言代码量全模块第一，零覆盖 → Stage 74。
- `/global/spend/keys/rankings` 唯一 LEFT JOIN → 本 Stage 73。

明细接口（logs）低风险（占位符差异），`/spend/logs` 已有 real BDD，`/global/spend/logs` 仅 admin 差异，暂不纳入 Phase 29 核心。
