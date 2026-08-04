# PG Budget Reset 失败根因分析

**日期**: 2026-08-04
**状态**: 🔴 根因已确认，需要修复

---

## 根因：SQL 占位符不兼容 —— 使用 `?` 而非 `$1, $2, ...`

`resetter.rs` 和 `main.rs` 的 `backfill_missing_reset_at()` 中的所有 SQL 查询在 PG 后端使用了 `?` 占位符（MySQL/SQLite 语法），但 PostgreSQL 要求使用 `$1`, `$2`, ... 占位符。

具体问题位置：

### 1. `resetter.rs` 中的 `scan_entity_table()` (第 111-150 行)

`scan_entity_table` 和 `scan_organizations` 没有单独的 PG 分支——三个后端复用同一条 SQL：
```rust
let sql = format!(
    "SELECT {pk}, budget_duration FROM {table}
     WHERE budget_duration IS NOT NULL
       AND (budget_reset_at IS NULL OR budget_reset_at < {now_f})"
);
```

对于 PG，`now_func` 返回 `NOW()`（这是对的），但 SQL 中没有使用 `?` 占位符，所以这条 SQL 本身没有占位符问题。

然而 `scan_organizations` 同理——没有 `?` 占位符，这条 SQL 在 PG 中没问题。

### 2. 根因：`execute_reset()` 第 256-306 行 —— SELECT 查询

```rust
let sql = r#"
    SELECT b.budget_reset_at
    FROM organizations o
    JOIN budgets b ON o.budget_id = b.budget_id
    WHERE o.organization_id = ?
"#;
```

在 PG 中 `?` 不是有效占位符，应该使用 `$1`。**这会导致 `execute_reset` 执行失败**。

同样，第 283-287 行：
```rust
let sql = format!(
    "SELECT budget_reset_at FROM {} WHERE {} = ?",
    entity_type.table_name(),
    entity_type.pk_column()
);
```

在 PG 中 `?` 同样是无效的。

### 3.根因：`execute_reset()` 第 321-324 行 —— UPDATE 查询

```rust
let sql_budget = r#"
    UPDATE budgets SET budget_reset_at = ?
    WHERE budget_id = (SELECT budget_id FROM organizations WHERE organization_id = ?)
"#;
```

在 PG 中 `?` 无效。

第 355-358 行：
```rust
let sql = format!(
    "UPDATE {} SET spend = 0, budget_reset_at = ? WHERE {} = ?",
    entity_type.table_name(),
    entity_type.pk_column()
);
```

同上。

### 4. 根因：`main.rs` 中 `backfill_missing_reset_at()`，第 590-860 行

backfill 函数对所有查询使用 `?`，包括 SELECT 和 UPDATE：

```rust
"SELECT token, budget_duration FROM virtual_keys
 WHERE budget_duration IS NOT NULL AND budget_reset_at IS NULL"

"UPDATE virtual_keys SET budget_reset_at = ? WHERE token = ?"
```

在 PG 中 `?` 无效。

### 5. 引擎层不受影响

`Engine::create_job()` 在 `engine.rs` 第 195-207 行已经正确使用 `$1, $2, ...` 语法处理 PostgreSQL（`create_job_pg`），并且使用 `$7::timestamptz` 进行类型转换。引擎层对 PG 的处理是健康的。

---

## 为什么这个问题只影响 PG 生产环境，而 SQLite 和 MySQL 没问题

SQLite 和 MySQL 都原生支持 `?` 占位符，而 PostgreSQL 的 sqlx 驱动严格要求 `$N` 风格的编号占位符。

---

## 为什么现有测试没有捕获

- 所有单元测试都使用 `Database::init("sqlite::memory:")` —— 只测试 SQLite。
- Mock BDD 同样使用 SQLite。
- 虽然设计了 `bdd-real-pg` 任务，但 budget_reset 的 PG 真实 BDD 测试并未运行或测试场景不够深入，未能覆盖到 `execute_reset` 的执行路径。

---

## 修复方案

### 选项 A：为 PG 单独编写 SQL（类似 `engine.rs` 中的 `create_job_pg`）

> 参见 [[engine-pg-dollar-sign-pattern]] 中的现有正确模式。

**优点**：完全掌控 PG 语法，可以进行类型转换（如 `$1::timestamptz`）
**缺点**：代码重复

### 选项 B：在 PG 分支中将 `?` 替换为 `$1, $2, ...`

**优点**：保留现有代码结构，仅修改 PG 的分支逻辑
**缺点**：当参数顺序改变时容易出错

### 选项 C：使用 sqlx 的跨方言 `?` 兼容模式

部分 sqlx 版本支持在 PG 中将 `?` 转换为 `$N`，但 aigw 使用的 sqlx 0.8 默认不启用此特性。

---

## 推荐：方案 A —— 为 PG 单独编写 SQL

这是 `engine.rs` 中已在使用的一致模式。所有受影响的函数需要改为：

1. **`execute_reset()`**：为 PG SELECT 和 UPDATE 使用 `$1/$2` 风格
2. **`backfill_missing_reset_at()`**：为 PG SELECT 和 UPDATE 使用 `$1/$2` 风格
3. **`scan_entity_table()` 和 `scan_organizations()`**：这些 SQL 目前没有 `?` 占位符，所以不需要修改（仅需验证）

### 受影响的文件

| 文件 | 代码行数 | 修改内容 |
|------|---------|---------|
| `crates/aigw-core/src/budget/resetter.rs` | 250-385 | `execute_reset` — PG 分支的 SELECT + UPDATE 语句使用 `$1/$2` |
| `crates/aigw-server/src/main.rs` | 590-860 | `backfill_missing_reset_at` — PG 分支的 SELECT + UPDATE 语句使用 `$1/$2` |

### 验证方式

```bash
# 修复后运行 PG 真实 BDD 测试
task bdd-real-pg -- --features budget_reset

# 或完整运行
task test        # 单元测试
task bdd         # Mock BDD
task bdd-real-pg # PG 真实 BDD
```

---

## 相关记忆

- [[mysql-json-literal-bug]] — 另一处按后端区分的 SQL 差异（MySQL 需要 CAST(CAST(X'hex' AS CHAR) AS JSON)）
- [[engine-pg-dollar-sign-pattern]] — engine.rs 第 346-380 行：PG SQL 使用 `$1, $2, ...` 以及 `$7::timestamptz` 进行类型转换

---

## 状态

- [x] 根因分析
- [ ] 修复 `resetter.rs` 的 `execute_reset` 函数
- [ ] 修复 `main.rs` 的 `backfill_missing_reset_at` 函数
- [ ] task test（编译 + 单元测试）
- [ ] task bdd（mock BDD）
- [ ] task bdd-real-pg（PG 真实 BDD）
- [ ] task bdd-real-sqlite + task bdd-real-mysql（回归测试）
- [ ] 提交 PR
