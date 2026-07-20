# spend_logs 断点续传能力设计方案

> 日期：2026-07-20
> 状态：待审计
> 审计结论：litellm 源表主键为 `request_id`（UUID），无自增 `id` 列。方案以 `startTime` 为锚点，利用 `@@index([startTime])` 索引，幂等写入兜底同秒重复。

---

## 0. 审计前置结论

### 0.1 litellm 源表 schema（`schema.prisma:581-618`）

```prisma
model LiteLLM_SpendLogs {
  request_id          String @id        // UUID 主键，非自增整数
  call_type           String
  startTime           DateTime
  endTime             DateTime
  // ... 其他列 ...
  @@index([startTime])                  // 单列索引
}
```

- **没有自增 `id` 列**，主键就是 `request_id`（UUID 字符串）。
- `@@index([startTime])` 索引覆盖 `WHERE startTime >= T` + `ORDER BY startTime`。

### 0.2 aigw 侧 schema（`migrations/*/002_spend_logs.sql`）

```sql
request_id TEXT PRIMARY KEY              -- UUID，无自增 id
start_time DATETIME NOT NULL             -- 对应 litellm startTime（camelCase → snake_case 自动转换）
```

同为 `request_id` 主键。`INSERT OR IGNORE` / `ON CONFLICT DO NOTHING` 保证幂等。

### 0.3 锚点选择

- **不可用：UUID `request_id`** — 随机散列，无法做 `>` 范围锚定。
- **不可用：自增 `id`** — 源表不存在此列。
- **选用：`startTime`** — 有时间含义，已有单列索引 `@@index([startTime])`，支持 `WHERE startTime >= T ORDER BY startTime`。

同秒内的重复读取由幂等写入（主键冲突跳过）兜底，无需额外的 UUID 级精确锚点。

---

## 1. 需求与现状

### 1.1 需求

spend_logs 表通常是迁移中数据量最大的一张表（百万到千万级），迁移耗时长，存在以下实际需求：

1. **断点续传**：迁移中断后，能从上次停止的位置继续，而不是全部重新开始
2. **增量迁移**：先迁移历史数据，后续补充新增数据
3. **时间范围迁移**：只迁移某段时间内的日志

### 1.2 现状

当前 `remote-import` 对 spend_logs 的查询为：

```sql
SELECT * FROM "LiteLLM_SpendLogs"              -- 无 LIMIT
SELECT * FROM "LiteLLM_SpendLogs" LIMIT N       -- 有 --spend-log-limit
```

**缺失：**
- 无 `ORDER BY`，迁移顺序不确定（取决于数据库存储顺序）
- 无法指定起点（无 cursor 机制）
- 无进度记录

当前可用的部分控制：

| 能力 | 命令 | 局限 |
|------|------|------|
| 只迁 spend_logs | `--step-filter 5` | ✅ |
| 限制行数 | `--spend-log-limit 10000` | 只能从头开始，无法跳过已迁移部分 |
| 跳过字段 | `--skip-body` / `--skip-columns` | ✅ |

---

## 2. 方案设计

### 2.1 核心思路

以 `startTime` 为锚点。利用 litellm 已有的 `@@index([startTime])` 索引，`ORDER BY startTime` + `WHERE startTime >= T` 都走索引。

同秒边界处可能重复读取少量记录，但目标表以 `request_id` 为主键，`INSERT OR IGNORE` / `ON CONFLICT DO NOTHING` 会静默跳过，无副作用。

### 2.2 新增 CLI 参数

```bash
aigw-migrate remote-import \
  --spend-log-resume-after "2026-07-15T10:30:00Z" \
  --spend-log-end-before "2026-07-20T00:00:00Z"
```

| 新增参数 | 类型 | 默认值 | 说明 |
|----------|------|--------|------|
| `--spend-log-resume-after` | `Option<String>` | `None` | ISO 8601 时间，从 `startTime >= T` 的记录开始迁移。不传则从最早开始 |
| `--spend-log-end-before` | `Option<String>` | `None` | ISO 8601 时间，只迁移 `startTime < T` 的记录。不传则持续到表尾 |

总共 2 个参数，语义清晰。

### 2.3 SQL 生成

#### 全量迁移（无 cursor）

```sql
SELECT * FROM "LiteLLM_SpendLogs"
  ORDER BY "startTime" ASC
  LIMIT 10000;
```

#### 指定起点

```sql
SELECT * FROM "LiteLLM_SpendLogs"
  WHERE "startTime" >= '2026-07-15 10:30:00'
  ORDER BY "startTime" ASC
  LIMIT 10000;
```

#### 时间窗口

```sql
SELECT * FROM "LiteLLM_SpendLogs"
  WHERE "startTime" >= '2026-07-01 00:00:00'
    AND "startTime" < '2026-08-01 00:00:00'
  ORDER BY "startTime" ASC
  LIMIT 10000;
```

全部命中 `@@index([startTime])` 索引前缀扫描。

### 2.4 `read_rows_with_cursor` 实现

在 `native.rs` 新增方法：

```rust
pub struct CursorRange {
    pub resume_after: Option<String>,  // ISO 8601
    pub end_before:  Option<String>,  // ISO 8601
}

pub async fn read_rows_with_cursor(
    &self,
    table: &str,
    cursor: &CursorRange,
    limit: Option<usize>,
) -> anyhow::Result<Vec<UnifiedRow>> {
    let quoted = self.quote_ident(table);
    let mut parts = vec![format!("SELECT * FROM {}", quoted)];
    let mut conditions: Vec<String> = Vec::new();

    if let Some(t) = &cursor.resume_after {
        let t_literal = self.time_literal(t);
        conditions.push(format!("\"startTime\" >= {}", t_literal));
    }
    if let Some(end) = &cursor.end_before {
        let end_literal = self.time_literal(end);
        conditions.push(format!("\"startTime\" < {}", end_literal));
    }
    if !conditions.is_empty() {
        parts.push(format!("WHERE {}", conditions.join(" AND ")));
    }

    parts.push("ORDER BY \"startTime\" ASC".to_string());

    if let Some(n) = limit {
        parts.push(format!("LIMIT {}", n));
    }

    let sql = parts.join(" ");
    // match self { SourcePool::Pg/Sqlite/Mysql → execute }
}
```

### 2.5 时间字面量生成

跨数据库的时间字面量生成：

| 数据库 | 输入（ISO 8601） | SQL 字面量 |
|--------|-----------------|-----------|
| PostgreSQL | `2026-07-15T10:30:00Z` | `'2026-07-15 10:30:00+00'::timestamptz` |
| MySQL | `2026-07-15T10:30:00Z` | `'2026-07-15 10:30:00'` |
| SQLite | `2026-07-15T10:30:00Z` | `'2026-07-15 10:30:00'` |

在 `SourcePool` 上新增 `time_literal(&self, iso8601: &str) -> String` 方法。

### 2.6 幂等性保证

- 目标表主键 `request_id`（UUID），写入 `INSERT OR IGNORE` / `ON CONFLICT DO NOTHING`
- **重复执行不会产生重复数据** — 相同 `request_id` 的行被忽略
- **同秒边界重复读取无害** — `resume_after` 使用 `>=`（inclusive），同秒内已迁移的记录会被重新读取，但写入时主键冲突跳过
- 操作人员只需记录进度输出中最新的 `startTime`，中断后填入 `--spend-log-resume-after` 即可

### 2.7 迁移进度提示

每 1000 行打印一次当前进度：

```
  [PROGRESS] spend_logs: 50000 rows ...  resume: 2026-07-15T10:30:05Z
```

最后一行就是续传参数：

```bash
aigw-migrate remote-import --step-filter 5 \
  --spend-log-resume-after "2026-07-15T10:30:05Z"
```

---

## 3. 涉及文件

| 文件 | 变更 |
|------|------|
| `crates/aigw-migrate/src/main.rs` | `RemoteImport` 结构新增 2 个 CLI 参数 |
| `crates/aigw-migrate/src/remote_import.rs` | `run_filtered` / `migrate_spend_logs` 签名更新；进度输出 |
| `crates/aigw-migrate/src/native.rs` | 新增 `CursorRange`、`read_rows_with_cursor`、`time_literal` |
| `crates/aigw-migrate/src/lib.rs` | 导出签名更新 |
| `docs/aigw-migrate.md` | 更新工具参考文档 |
| `crates/aigw-migrate/tests/` | 新增断点续传测试用例 |

---

## 4. 使用场景

### 场景 A：首次全量迁移

```bash
aigw-migrate remote-import --step-filter 5
```

### 场景 B：中断后继续

```bash
# 第一次运行中断前最后一行进度输出:
# [PROGRESS] spend_logs: 518000 rows ...  resume: 2026-07-15T10:30:05Z

# 从该时间继续
aigw-migrate remote-import --step-filter 5 \
  --spend-log-resume-after "2026-07-15T10:30:05Z"
```

### 场景 C：时间窗口迁移

```bash
# 只迁 7 月份的日志
aigw-migrate remote-import --step-filter 5 \
  --spend-log-resume-after "2026-07-01T00:00:00Z" \
  --spend-log-end-before "2026-08-01T00:00:00Z"
```

### 场景 D：增量补充

```bash
# 历史迁移完成后，补充昨天之后的新日志
aigw-migrate remote-import --step-filter 5 \
  --spend-log-resume-after "2026-07-19T00:00:00Z"
```

### 场景 E：limit + range 组合

```bash
aigw-migrate remote-import --step-filter 5 \
  --spend-log-resume-after "2026-07-01T00:00:00Z" \
  --spend-log-limit 5000
```

---

## 5. 性能分析

### 5.1 索引利用

`@@index([startTime])` 单列索引。

| SQL 模式 | 索引使用 |
|----------|---------|
| `ORDER BY startTime` | 覆盖排序，顺序遍历索引叶节点 |
| `WHERE startTime >= T ORDER BY startTime` | 索引 seek + 顺序扫描 |
| `WHERE startTime >= T AND startTime < T2 ORDER BY startTime` | 索引范围扫描 |

全部命中 `@@index([startTime])`，不会触发全表扫描。

### 5.2 ORDER BY 的性能影响

- **有索引时**：直接顺序扫描索引，O(K) where K = LIMIT 结果集大小。带 `ORDER BY` 的 `LIMIT 10000` 通常在几百毫秒内。
- **无索引时**（边缘）：全表排序，但 litellm 标准 schema 已内置该索引。

真正的瓶颈在 INSERT 写入侧，不在读取侧。

---

## 6. 测试计划

### 单元测试

| 测试 | 场景 | 验证点 |
|------|------|--------|
| `test_read_rows_full_scan` | 无 cursor | `ORDER BY startTime`，行为确定 |
| `test_read_rows_resume_after` | `resume_after` only | `startTime >= T` |
| `test_read_rows_time_window` | `resume_after` + `end_before` | `T <= startTime < T2`，边界正确 |
| `test_read_rows_limit_and_cursor` | cursor + limit | limit 截断 + 排序正确 |
| `test_migrate_resume_idempotent` | 先迁全量，再"续传"同一时间 | 目标表行数不变（幂等） |
| `test_migrate_resume_no_gap` | 迁到时间 T，再从 T 续传 | 目标表包含全量，无中间缺失 |

### 集成测试

| 测试 | 场景 |
|------|------|
| SQLite → SQLite 断点续传 | `--spend-log-resume-after` 继续 |
| PG → PG 断点续传 | 同上 |
| MySQL → PG 断点续传 | 跨数据库 |

---

## 7. 实现步骤

| 步骤 | 内容 | 预估 |
|------|------|------|
| 1 | `native.rs`：新增 `CursorRange`、`read_rows_with_cursor`、`time_literal` | 30 min |
| 2 | `main.rs`：新增 2 个 CLI 参数，构建 `CursorRange` | 10 min |
| 3 | `remote_import.rs`：`migrate_spend_logs` 用 `read_rows_with_cursor` 替换 `read_rows_with_limit`；进度输出 | 20 min |
| 4 | `lib.rs`：导出签名更新 | 5 min |
| 5 | 单元测试 | 30 min |
| 6 | 集成测试 | 30 min |
| 7 | `docs/aigw-migrate.md` 更新 | 10 min |

**总计：~2h**

---

## 8. 边界条件与注意事项

| 边界 | 处理 |
|------|------|
| `resume_after` 指向不存在的记录 | `startTime >= T`，从该时间之后第一条开始 |
| `end_before` <= `resume_after` | 返回 0 行，WARN |
| 同秒重复读取 | 幂等写入兜底，无副作用 |
| 源表 0 行 | 返回空 Vec，提前返回 `Ok(0)` |
| 无 `@@index([startTime])`（edge case） | 查询退化但不会报错。pre-check 可提示 |
| `--spend-log-limit` 与 cursor 组合 | 正交：limit 控制批次大小，cursor 控制起点 |

---

## 9. 与现有参数的关系

| 参数 | 交互 |
|------|------|
| `--step-filter 5` | 仅影响是否执行 step 5，与 cursor 正交 |
| `--spend-log-limit` | 控制每批取出行数。cursor 控制起点，limit 控制批量大小 |
| `--skip-body` / `--skip-columns` | 字段跳过逻辑不变 |
