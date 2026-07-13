# Rust 多数据库 SQL 转换/ORM 迁移方案调研

> 日期：2026-07-13
> 问题背景：`task bdd-real-pg/sqlite` 两个失败用例长期修不好，当前手动处理跨数据库类型转换的代码越来越复杂，需要评估是否有现成库可用。

## 1. 当前系统痛点分析

### 架构现状

当前 aigw 使用 `sqlx::AnyPool` 作为统一连接层，在 `remote_import.rs` 和 `remote_export.rs` 中手写了大量跨数据库兼容代码：

| 痛点 | 具体表现 | 代码位置 |
|------|----------|----------|
| **PG 原生类型 decode** | AnyPool 无法 decode jsonb/timestamp/text[]，需 `::text` 绕过 | `build_pg_select()` |
| **PG binary protocol 类型绑定** | 无法 bind String 到 numeric/boolean 列，需 `CAST(NULLIF(…))` 服务端转换 | `bind_cell()`, `cast_expr()` |
| **空字符串 vs NULL** | SQLite 空字符串到 PG numeric/boolean 列报错 | `normalize_string_value()` |
| **MySQL JSON 严格校验** | MySQL 拒绝空字符串/截断的 JSON 值 | `bind_value_from_row()` 中的 sanitize 逻辑 |
| **标识符引用差异** | PG/SQLite 用双引号，MySQL 用反引号 | `quote_ident()` |
| **占位符差异** | PG 用 `$1`, SQLite/MySQL 用 `?` | `placeholder()` |
| **CONFLICT 语法差异** | PG: `ON CONFLICT DO NOTHING`，SQLite: `INSERT OR IGNORE`，MySQL: `INSERT IGNORE` | `insert_prefix()` |
| **加密密钥轮转** | 迁移 credential_values 时解密→再加密 | `migrate_credentials()` |

**核心矛盾**：当前问题本质上不是"SQL 方言差异"，而是**跨数据库数据迁移时的类型强制转换问题**。

## 2. Rust 生态库调研

### 2.1 sqlparser-rs (Apache DataFusion)

**定位**：SQL 解析器/词法分析器，生成 AST

| 维度 | 评价 |
|------|------|
| 支持方言 | GenericDialect, MySQL, PostgreSQL, SQLite, BigQuery, Snowflake, DuckDB, ClickHouse, MSSQL, Oracle, Hive, Redshift, Spark, Databricks, Teradata |
| DDL 转换 | ❌ 不支持。只解析不转换，解析后需手动遍历 AST 做方言改写 |
| 类型映射 | ❌ 不涉及。纯粹是语法层面 |
| 数据迁移 | ❌ 不涉及。无查询执行能力 |
| 适用场景 | 构建 SQL 分析工具、格式化工具、简单方言改写 |

**结论**：可用于辅助生成不同方言的 SQL 字符串（如标识符引用），但无法解决类型强制转换、空值处理等核心问题。

### 2.2 SeaQuery

**定位**：动态 SQL 查询构建器（Query Builder），非 ORM

| 维度 | 评价 |
|------|------|
| 支持数据库 | MySQL, PostgreSQL, SQLite |
| DDL 生成 | ✅ 定义一次 Table，自动生成各方言的 CREATE TABLE / ALTER TABLE |
| 查询生成 | ✅ 支持 SELECT/INSERT/UPDATE/DELETE，自动处理标识符引用和占位符 |
| 类型映射 | ⚠️ 有限。定义 column type 用抽象类型（Integer, Text 等），但 INSERT 时不处理值类型转换 |
| 数据迁移 | ❌ 不包含。只生成 SQL 字符串，不执行，不处理行级数据转换 |
| 加密支持 | ❌ 不涉及 |

**SeaQuery 示例**：
```rust
// 定义一次，生成三种方言
Table::create()
    .table(Char::Table)
    .col(ColumnDef::new(Char::Id).integer().not_null().auto_increment().primary_key())
    .to_string(MysqlQueryBuilder);    // -> MySQL DDL
    .to_string(PostgresQueryBuilder); // -> PG DDL
    .to_string(SqliteQueryBuilder);   // -> SQLite DDL
```

**结论**：可以替代当前手写的 DDL 生成部分，但核心的数据迁移逻辑（类型强制转换 + 加密轮转）仍需自己手写。没有提供 `AnyPool` 式的运行时数据库切换。

### 2.3 Diesel

**定位**：编译时类型安全的 ORM + Query Builder

| 维度 | 评价 |
|------|------|
| 支持数据库 | PostgreSQL, MySQL, SQLite |
| Schema 管理 | CLI 工具 `diesel migration generate/run/revert` |
| 迁移方式 | 每个 migration 是 `up.sql` + `down.sql` 原始 SQL 文件 |
| 类型安全 | ✅ 编译时检查，schema.rs 由数据库内省自动生成 |
| 跨数据库 | ❌ schema.rs 绑定具体数据库类型，不支持运行时切换 |
| 数据迁移 | ❌ 不设计为"从 DB A 搬到 DB B"的工具 |
| 动态 DB URL | ❌ 编译时确定数据库类型 |

**结论**：Diesel 的迁移是传统 schema migration（版本化管理 DDL），不是跨数据库数据迁移。且编译时绑定数据库类型，无法实现当前 `AnyPool` 的运行时多数据库支持。

### 2.4 SeaORM

**定位**：基于 SeaQuery + sqlx 的异步 ORM

| 维度 | 评价 |
|------|------|
| 支持数据库 | PostgreSQL, MySQL, SQLite |
| 迁移方式 | CLI + 原始 SQL 文件（同 Diesel） |
| 类型安全 | ✅ Entity 定义自动生成 |
| 跨数据库数据迁移 | ❌ 不支持，与 Diesel 类似 |
| 动态数据库切换 | ⚠️ 通过 sqlx 底层支持，但 SeaORM 自身不提供工具 |

### 2.5 sqlx（当前使用）

**定位**：异步 SQL 工具包

| 维度 | 评价 |
|------|------|
| AnyPool | ✅ 唯一支持运行时多数据库切换的库 |
| 原生驱动 | ✅ 各数据库有独立驱动，性能好 |
| 类型限制 | ❌ AnyPool 无法 decode PG 原生类型（jsonb, text[] 等） |
| 二进制协议 | ❌ PG 驱动用 binary protocol，不能隐式 text→numeric |

### 2.6 PRQL / GlueSQL

| 库 | 评价 |
|----|------|
| **PRQL** | 编译到 SQL 的查询语言，生成方言特定 SQL，但只做查询不做数据迁移 |
| **GlueSQL** | 纯 Rust 嵌入式数据库引擎，支持 SQL，但不解决跨数据库迁移问题 |

## 3. 综合对比

| 能力 | 当前手写 | sqlparser-rs | SeaQuery | Diesel | sqlx | 理想方案 |
|------|----------|-------------|----------|--------|------|----------|
| 多数据库运行时切换 | ✅ | ❌ | ❌ | ❌ | ✅ | ✅ |
| DDL 方言生成 | ⚠️ 手写 | ⚠️ AST 改写 | ✅ | ⚠️ SQL 文件 | ❌ | ✅ |
| 类型强制转换 | ⚠️ 手写 CAST | ❌ | ❌ | ❌ | ❌ | ✅ |
| 空值处理 | ⚠️ 手写 | ❌ | ❌ | ❌ | ❌ | ✅ |
| 加密密钥轮转 | ✅ | ❌ | ❌ | ❌ | ❌ | ⚠️ |
| 标识符引用 | ⚠️ 手写 | ⚠️ AST 改写 | ✅ | ⚠️ SQL 文件 | ❌ | ✅ |
| 占位符 | ⚠️ 手写 | ⚠️ AST 改写 | ✅ | ⚠️ SQL 文件 | ❌ | ✅ |
| 行级数据转换 | ✅ | ❌ | ❌ | ❌ | ❌ | ⚠️ |

## 4. 核心结论

### 没有现成的库可以直接解决当前问题

**原因**：aigw 的迁移是一个特殊的"跨数据库 ETL + 加密密钥轮转"场景，不是标准的 ORM Schema Migration。

具体来说，没有任何一个 Rust 库同时提供：
1. 运行时多数据库连接切换（只有 sqlx `AnyPool` 有）
2. 跨数据库数据类型的自动强制转换（type coercion）
3. 加密字段的密钥轮转（这完全是你业务特有的）

### SeaQuery 是最有价值的改进方向

如果要重构，**SeaQuery 是唯一值得引入的库**。它的价值在于：

1. **消除 DDL 手写差异**：`quote_ident()`、`placeholder()`、`insert_prefix()` 这些 SQL 方言差异可以完全交给 SeaQuery
2. **类型安全的 Schema 定义**：表结构可以统一定义一次
3. **减少出错面**：当前大量 `if is_pg(target_url)` 分支，SeaQuery 可以消除大半

但 SeaQuery 不能解决的核心问题仍然需要自己处理：
- 数据值的类型强制转换（SQLite TEXT → PG numeric）
- 空字符串 → NULL 转换
- 加密密钥轮转逻辑

### 推荐方案

**短期（修当前 bug）**：继续修复当前手写代码，聚焦于两个失败用例的具体错误。

**中期（重构，可达性高）**：引入 SeaQuery 替代手写 SQL 生成，保留 sqlx AnyPool 执行层 + 自定义类型转换层。

```
┌──────────────────────────────────────┐
│  aigw-migrate (业务逻辑)              │
│  - 加密轮转                          │
│  - 行级数据过滤/转换                  │
│  - 批次管理                          │
├──────────────────────────────────────┤
│  类型转换层 (新写)                    │
│  - 源类型 → 目标类型映射表            │
│  - 空值规范化                         │
│  - JSON sanitize                     │
├──────────────────────────────────────┤
│  SeaQuery (引入)                     │
│  - DDL/SQL 生成                      │
│  - 方言差异处理                       │
├──────────────────────────────────────┤
│  sqlx AnyPool (保留)                 │
│  - 运行时数据库连接                   │
│  - SQL 执行                          │
└──────────────────────────────────────┘
```

**长期（不推荐）**：放弃 AnyPool，为每种数据库单独编写迁移逻辑。代码量翻 3 倍，维护成本极高，不推荐。

### 不建议的路线

- **sqlparser-rs AST 改写**：过于底层，相当于写一个半成品 SQL 编译器中的 dialect lowering pass
- **Diesel/SeaORM migration**：它们的 migration 是 DDL 版本管理，不是跨数据库数据搬运
- **PRQL**：只做查询，不做数据迁移

## 5. 参考

- [sqlparser-rs](https://github.com/apache/datafusion-sqlparser-rs) — Apache DataFusion 项目的 SQL 解析器
- [SeaQuery](https://github.com/SeaQL/sea-query) — 动态 SQL 查询构建器
- [Diesel](https://github.com/diesel-rs/diesel) — 编译时安全 ORM
- [SeaORM](https://github.com/SeaQL/sea-orm) — 异步 ORM
- [sqlx](https://github.com/launchbadge/sqlx) — 异步 SQL 工具包
