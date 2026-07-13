# ADR-004: 使用原生连接池替代 AnyPool 进行跨数据库迁移

> 日期：2026-07-13
> 状态：实验验证通过，待实施

## 根因

`task bdd-real-pg` 和 `task bdd-real-sqlite` 的迁移用例长期失败，根因是 **`sqlx::AnyPool` 的类型擦除不适合跨数据库数据传输**。

### 问题本质

aigw 的迁移场景是：

```
数据库 A → 读取 → 业务逻辑（解密/加密） → 写入 → 数据库 B
```

这本质上是一个 **ETL pipeline**，三种数据库（PG / SQLite / MySQL）的源和目标任意组合有 9 种路径。

`sqlx::Any` 的设计目标是用统一 API 操作同构数据库，类型系统做的是"最大公约数"——当源和目标都使用 AnyPool 时会发生：

| 操作 | PG | SQLite | MySQL |
|------|-----|--------|-------|
| decode `JSONB`/`JSON` | ❌ 无法 decode 为 `serde_json::Value` | ❌ `BLOB` 无法 decode | ✅ |
| decode `BOOLEAN` | ❌ 无法 decode 为 `bool` | ✅ decode 为 `i64` | ⚠️ |
| bind String → `jsonb` | ❌ PG binary protocol 拒绝 | — | ❌ MySQL strict JSON 校验 |
| bind String → `boolean` | ❌ PG："integer→boolean" | — | — |
| bind String → `numeric` | ❌ PG binary protocol 拒绝 | — | — |

当前代码的应对方式是在 SQL 生成层和绑定层大量手写 hack：
- `COALESCE(col::text, '')` 把 PG 原生类型全转成 text 再读
- `CAST(NULLIF($1, '') AS double precision)` 服务端强制转换
- `normalize_mysql_type()` 手写 MySQL 类型规范化
- `pg_type_for_cast()` / `cast_expr()` 手写 PG 类型映射

这些 hack 导致 `remote_import.rs`（1856 行）和 `remote_export.rs`（1069 行）极度膨胀，每修一个 bug 就加一个特殊 case。

### 为什么 Go GORM 能轻易做到而 Rust ORM 做不了

Go 的 `database/sql` + GORM 通过**反射**在运行时做类型转换：

```go
type VirtualKey struct {
    Token    string  `gorm:"column:token"`
    Metadata string  `gorm:"column:metadata;type:json"`  // 任何 DB 都能转
    Blocked  bool    `gorm:"column:blocked"`             // INTEGER → bool
}
```

Rust ORM（Diesel/SeaORM）的类型映射是**编译期绑定具体数据库类型**的——`PgPool` 知道 `JSONB → Value`，`SqlitePool` 知道 `BLOB → Vec<u8>`，但不存在一个运行时分发的"万能连接"。

## 实验

### 方案

用 sqlx 的三个原生连接池分别读写各自数据库，中间统一为 `Vec<(col_name, serde_json::Value)>` 传递数据，写入时根据目标列的数据库类型做 final coercion。

```
读:  PgPool/SqlitePool/MySqlPool → native decode → UnifiedRow (Value-based)
写:  UnifiedRow → value_to_target_literal → native bind → target Pool
```

### PoC 结果

测试文件：`crates/aigw-migrate/tests/native_pool_poc.rs`

| 测试 | 场景 | 关键验证点 |
|------|------|-----------|
| `test_sqlite_native_type_decode` | SQLite 读 | BLOB→JSON, INTEGER→i64, REAL→f64, DATETIME→String ✅ |
| `test_sqlite_to_pg_roundtrip` | SQLite→PG | BLOB→JSONB, INTEGER→BOOLEAN, REAL→DOUBLE ✅ |
| `test_pg_to_sqlite_roundtrip` | PG→SQLite | JSONB→BLOB, BOOLEAN→INTEGER, DOUBLE→REAL ✅ |
| `test_virtual_keys_sqlite_to_pg_roundtrip` | 真实 schema | 24 列，16 个 JSONB，5 个 TIMESTAMPTZ ✅ |

全部 4 个测试通过。

### 类型映射规律

三类需要特殊处理的跨数据库类型转换：

**JSON 类**：
```
PG JSONB  ←→  Value  ←→  SQLite BLOB(JSON bytes)  ←→  MySQL JSON
```
- 读：PG/MySQL 原生 decode Value；SQLite decode Vec<u8> → parse JSON
- 写：PG 用 `::jsonb` cast；SQLite 序列化为 bytes；MySQL 原生 bind

**布尔类**：
```
PG BOOLEAN  ←→  bool  ←→  SQLite INTEGER(0/1)  ←→  MySQL TINYINT(1)
```
- 读：PG/MySQL 原生 decode bool；SQLite i64 → !=0
- 写：PG 原生 bool；SQLite true→1/false→0；MySQL 原生 bool

**浮点类**：
```
PG DOUBLE PRECISION  ←→  f64  ←→  SQLite REAL  ←→  MySQL DOUBLE
```
- 全部 native decode/bind，无需特殊处理

**文本/整数/时间戳**：全部 native decode/bind，无需特殊处理。

## 实施计划

### 新增

1. `crates/aigw-migrate/src/native_pool.rs` — 原生连接池抽象
   - `SourcePool` enum（PgPool | SqlitePool | MySqlPool）
   - `read_rows()` — 读取并转为 `Vec<UnifiedRow>`
   - `read_column_types()` — 获取列表及类型

2. `crates/aigw-migrate/src/type_coercion.rs` — 类型强制转换层
   - `UnifiedRow = Vec<(String, Value)>`
   - `value_to_target_literal(Value, target_type) → String`
   - `source_type_to_value(native_row, col_idx, source_type) → Value`

### 修改

3. `remote_import.rs` — 用原生 pool + 类型转换层重写
   - 删除 `build_pg_select()`（不再需要 `::text` 绕过）
   - 删除 `pg_insert_values_expr()` / `cast_expr()`
   - 删除 `normalize_mysql_type()`、`pg_type_for_cast()`
   - 用 `native_pool` + `type_coercion` 替代

4. `remote_export.rs` — 同上

### 预期收益

- `remote_import.rs`: ~1856 → ~500 行
- `remote_export.rs`: ~1069 → ~400 行
- 消除所有手写的 `CAST(NULLIF(...))`、`COALESCE(::text)`、`normalize_mysql_type`
- BDD 测试修复为副作用（类型转换逻辑集中到一层）
