# Stage 100: aigw-migrate 高级功能 BDD — PreCheck + Verify + Step Filter + Skip Columns + Cursor Resume

**Phase**: 40 — BDD Coverage Enhancement  
**优先级**: P1  
**状态**: ⏳ 待开始  
**预估**: 10h  
**前置**: 无（改 aigw-migrate real BDD，与 Stage 98/99 并行）

---

## 核心预期

补齐 aigw-migrate 缺失的 BDD 场景——迁移前检查、迁移后校验、高级 CLI 参数（step filter、skip columns、cursor resume）。全部为 **real BDD**（`@real_api`），通过 `task bdd-real-*` 调用真实 `aigw-migrate` 二进制 + testcontainers DB 执行。

| Feature 文件 | 场景数 | 覆盖功能 | 预估 |
|-------------|--------|---------|------|
| `migration_precheck.feature` | 4 | PreCheck 6 项检查（全量通过/源表缺失/master_key 错误/空表 warning） | 3h |
| `migration_verify.feature` | 2 | Verify 12 表行数比对（匹配/不匹配） | 2h |
| `migration_advanced.feature` | 3 | `--step-filter` / `--skip-body` / `--skip-columns` | 3h |
| `migration_cursor.feature` | 2 | spend_logs cursor resume / 幂等重跑 | 2h |

**总计: 11 real BDD 场景 × 3 后端（SQLite/PG/MySQL）= SQLite 全覆盖，PG/MySQL 选 5 场景（pre-check + verify）**

---

## Part A: PreCheck BDD（3h）

新建 `crates/aigw-server/tests/features/real/migration_precheck.feature`：

```gherkin
@real_api
Feature: aigw-migrate PreCheck — 迁移前 6 项自动化检查

  Background:
    Given 两个临时 SQLite 数据库已创建（source + target）

  Scenario: PreCheck 全量通过
    Given source 库表结构完整（11 张表 + 各表至少 1 行数据）
    And target 库表结构完整
    And master_key 正确
    When 执行 aigw-migrate pre-check <source_url> <target_url> --master-key <key>
    Then 退出码为 0
    And stdout 显示 "All checks passed" 或 6/6 通过

  Scenario: PreCheck 源表缺失报错
    Given source 库缺少 proxy_models 表
    When 执行 aigw-migrate pre-check ...
    Then 退出码非 0
    And stderr 包含 "missing table" 或 "proxy_models"

  Scenario: PreCheck master_key 错误报错
    Given source 库表完整
    When 执行 aigw-migrate pre-check --master-key wrong_key
    Then 退出码非 0
    And stderr 包含 "master_key" 或 "decrypt" 或 "verification failed"

  Scenario: PreCheck 源空表不报错（warning）
    Given source 库某表（如 tags）行数为 0
    When 执行 aigw-migrate pre-check ...
    Then 退出码为 0
    And stdout 包含 "0 rows" 或 "empty" warning
```

### 设计要点

1. **PreCheck 是 CLI 子命令**（`aigw-migrate pre-check`），需用 `std::process::Command` 执行真实二进制，验证退出码和 stdout/stderr 内容。
2. **source/target 都是临时 SQLite 文件**：`tempfile::NamedTempFile` 创建 → 运行 migration 建表 → 灌测试数据 → 传给 CLI。
3. **三后端覆盖策略**: SQLite source 即可覆盖 4 场景（PreCheck 的 6 项检查逻辑与 DB 后端无关——connectivity/table existence/row counts/master_key/key valid/decrypt spot-check 都是通用逻辑）。PG/MySQL 各加 1 场景验证 connectivity check 在不同后端的表现。
4. **master_key 模拟**: 在临时 source 库中写入一条加密的 credential（用已知 key 加密），PreCheck 用正确/错误 key 验证 decrypt spot-check。

---

## Part B: Verify standalone BDD（2h）

新建 `crates/aigw-server/tests/features/real/migration_verify.feature`：

```gherkin
@real_api
Feature: aigw-migrate Verify — 迁移后 12 表行数比对

  Background:
    Given 两个临时 SQLite 数据库已创建

  Scenario: Verify 同 schema 库全匹配
    Given source 和 target 库各有相同的 11 张表，行数完全一致
    When 执行 aigw-migrate verify <source_url> <target_url>
    Then 退出码为 0
    And stdout 显示 "All 11 tables verified" 或 "All matched"

  Scenario: Verify 行数不匹配报错
    Given target 库 spend_logs 比 source 库少 2 行
    When 执行 aigw-migrate verify <source_url> <target_url>
    Then 退出码非 0
    And stdout 显示 spend_logs 行数不一致（source=N, target=N-2）
```

### 设计要点

1. **Verify 也是 CLI 子命令**，同 PreCheck 用 `std::process::Command` + 临时 SQLite。
2. **TABLE_MAPPINGS 完整性验证**: 间接验证 `lib.rs` 的 `TABLE_MAPPINGS` 常量覆盖全部 12 表对（如果未来加表但忘加到 TABLE_MAPPINGS，verify 会少比对）。
3. **三后端**: SQLite 全覆盖。PG/MySQL 各跑匹配场景验证方言连接正常。

---

## Part C: Step Filter + Skip Columns BDD（3h）

新建 `crates/aigw-server/tests/features/real/migration_advanced.feature`：

```gherkin
@real_api
Feature: aigw-migrate 高级功能 — step filter + skip columns

  Background:
    Given source litellm SQLite 库有完整数据（所有表非空）

  Scenario: --step-filter=2 只执行 plain tables
    When 执行 remote-import --step-filter 2 <source_url> <target_url>
    Then 退出码为 0
    And target 库中 virtual_keys/teams/users/organizations/proxy_models/spend_logs 等表有数据
    And target 库中 credentials 表为空（step 3 未执行）

  Scenario: --skip-body 跳过 spend_logs body 字段
    Given source 库 spend_logs 中有 body 数据（messages/response 非 NULL）
    When 执行 remote-import --skip-body <source_url> <target_url>
    Then 退出码为 0
    And target 库 spend_logs 中 messages 列为 NULL
    And target 库 spend_logs 中 response 列为 NULL

  Scenario: --skip-columns 跳过指定列
    When 执行 remote-import --skip-columns spend_logs.messages,spend_logs.response <source_url> <target_url>
    Then 退出码为 0
    And target 库 spend_logs 有数据但 messages 和 response 列为 NULL
    And target 库 spend_logs 的其他列（如 spend, prompt_tokens）有数据
```

### 设计要点

1. **source 库需 litellm 格式 schema**（camelCase 列名），不是 aigw 格式。`remote-import` 从 litellm 迁移到 aigw，source 必须符合 litellm schema。用临时 SQLite 执行 litellm migration（从 `crates/aigw-migrate` 的 migration SQL 反向创建）。
2. **`--step-filter` 场景**需验证 step 2=plain tables 执行 + step 3=credentials 跳过。更全面可加 step 4/5 场景（proxy_models/spend_logs），但 1 场景足够覆盖 CLI 参数解析 + step 调度逻辑。
3. **`--skip-body` 场景**验证 NULL body 列，对齐现有 `sync.rs` 中的 `test_skip_body_nulls_body_columns` UT。
4. **`--skip-columns` 场景**验证任意列跳过（不仅是 body 三列）。`spend_logs.messages` 格式为 `table.column`。

---

## Part D: Cursor Resume BDD（2h）

新建 `crates/aigw-server/tests/features/real/migration_cursor.feature`：

```gherkin
@real_api
Feature: aigw-migrate Cursor Resume — 断点续迁

  Background:
    Given source litellm SQLite 库有 100 条 spend_logs（start_time 分布在 7 天内）

  Scenario: spend_logs cursor resume 从指定时间点
    When 执行 remote-import --spend-log-resume-after "2026-01-05T00:00:00Z" <source_url> <target_url>
    Then 退出码为 0
    And target 库 spend_logs 行数为 ~50-60（只迁移了 resume-after 之后的记录）
    And target 库 start_time 最小值 > "2026-01-05T00:00:00Z"

  Scenario: 幂等重跑不产生重复行
    Given 迁移已完成
    When 再次执行相同 remote-import 命令
    Then 退出码为 0
    And target 库 spend_logs 总行数与首次迁移相同（INSERT OR IGNORE 生效）
```

### 设计要点

1. **`spend_logs` 用 `UNIXEPOCH()` 生成分布数据**: SQLite 无 `generate_series`，用 Python/Shell 脚本预生成 100 条 spend_logs INSERT 语句，`start_time` 均匀分布在 7 天内。
2. **cursor resume**: `--spend-log-resume-after` 参数传给 `CursorRange::After`，验证游标正确过滤。
3. **幂等重跑**: `INSERT OR IGNORE`（SQLite）/ `ON CONFLICT DO NOTHING`（PG/MySQL）保证同 call_id 不重复——这是现有行为，本场景验证。

---

## 现有 real BDD 基础设施复用

`crates/aigw-server/tests/bdd_support/` 已有完整的 real BDD 基础设施：

| 文件 | 用途 | 复用方式 |
|------|------|---------|
| `real_db_seed.rs` | 向 testcontainers DB 灌测试数据 | 直接复用 `real_db_seed::seed_*` 函数 |
| `real_api_steps.rs` | 原有 real BDD step 定义 | 追加 `precheck`/`verify`/`advanced`/`cursor` 步骤 |
| `server.rs` | `TestWorld` 共享状态 | 复用 `TestWorld.db`?（此处需确定 `aigw-migrate` CLI 如何传参——是通过 `std::process::Command` 还是通过 `TestWorld` 内的 crate 调用） |

### CLI 调用方式决策

**选择 `std::process::Command` 执行已编译的 `aigw-migrate` 二进制**：
- PreCheck/Verify/Sync CLI 的参数解析在 `main.rs`，单元测试只能测内部函数（`run_pre_check`/`run_verify`），BDD 需验证完整 CLI 链路（参数解析→子命令分发→逻辑执行→退出码）。
- 现有 `migration_sync.feature` 的 real BDD step 已经在用 `Command::new("aigw-migrate")` 模式（参考 `migration_sync_steps.rs`）。
- 需要确保 `task bdd-real-*` 在运行前已构建 `aigw-migrate` 二进制：`Taskfile.yml` 中 `bdd-real-*` task 需加 `deps: [build]` 或等效前置。

---

## 方言差异与 real BDD 策略

| 场景 | SQLite | PG | MySQL | 策略 |
|------|--------|-----|-------|------|
| PreCheck 全量通过 | ✅ | ✅ | ✅ | 三后端全跑 |
| PreCheck 源表缺失 | ✅ | - | - | SQLite 即可（逻辑与后端无关） |
| PreCheck master_key 错误 | ✅ | - | - | SQLite 即可 |
| PreCheck 空表 warning | ✅ | - | - | SQLite 即可 |
| Verify 匹配 | ✅ | ✅ | ✅ | 三后端全跑 |
| Verify 不匹配 | ✅ | - | - | SQLite 即可 |
| step-filter | ✅ | - | - | SQLite 即可（CLI 参数逻辑） |
| skip-body | ✅ | - | - | SQLite 即可 |
| skip-columns | ✅ | - | - | SQLite 即可 |
| cursor resume | ✅ | - | - | SQLite 即可（游标逻辑通用） |
| 幂等重跑 | ✅ | - | - | SQLite 即可 |

**三后端矩阵**: 11 场景 × SQLite 全覆盖，5 场景（pre-check 通过 + verify 匹配）× PG/MySQL 补充。共 SQLite 11 + PG 2 + MySQL 2 = 15 个执行组合。

---

## BDD step 实现

所有 step 追加到 `crates/aigw-server/tests/bdd_steps/migration_sync_steps.rs`（已存在，用于 `migration_sync.feature` 的 `@real_api` step），按功能分组：

- `precheck` step: `Given 两个临时 SQLite 数据库` / `When 执行 pre-check` / `Then 退出码`
- `verify` step: `When 执行 verify` / `Then 行数一致`
- `advanced` step: `When 执行 remote-import --step-filter 2` / `When 执行 remote-import --skip-body` / `When 执行 remote-import --skip-columns`
- `cursor` step: `Given 100 条 spend_logs` / `When 执行 remote-import --spend-log-resume-after`

---

## TDD

- 先写 Gherkin 场景 → 定义 step 骨架 → `task bdd-real-sqlite` 跑红 → 实现 step → 跑绿
- 红→绿循环：每个 Feature 文件独立循环验证

---

## 验收门禁

| task | 类型 | 预期 | 说明 |
|------|------|------|------|
| `task bdd-real-sqlite` | **real BDD (SQLite)** | 新增 11 + 回归 36 = **47 pass** | 主要验证：`@real_api`，temp SQLite DB，通过 `Command::new("aigw-migrate")` 执行 CLI |
| `task bdd-real-pg` | **real BDD (PG)** | 新增 2 + 回归 36 = **38 pass** | 仅 pre-check 全量通过 + verify 匹配 × PG testcontainers |
| `task bdd-real-mysql` | **real BDD (MySQL)** | 新增 2 + 回归 36 = **38 pass** | 仅 pre-check 全量通过 + verify 匹配 × MySQL testcontainers |
| `task test` | 单元测试 | aigw-migrate 27 UT 回归无退化 | 不变 |
| `task bdd` | mock BDD | 回归 ~178 pass | 无新增 mock BDD |
| `task fe-bdd` | 前端 BDD | 回归无退化 | 前端无变更 |

> **本 Stage 仅涉及 `task bdd-real-sqlite/pg/mysql`（real BDD），不涉及 `task bdd`（mock BDD）新增场景，也不涉及 `task fe-bdd` 新增场景。**
