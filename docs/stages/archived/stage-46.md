# Stage 46: aigw-migrate --skip-columns 选择性迁移

**Phase**: 15 — 第二轮反馈改进
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 3h

---

## 目标

为 `aigw-migrate remote-import` 增加列选择性跳过功能，允许用户排除某些字段（如 messages、response 等大 body 文本）不迁移到 aigw。

## 验收标准

- [ ] `--skip-body` 预设：跳过 `spend_logs.messages` 和 `spend_logs.response`
- [ ] `--skip-columns <table.col,table.col,...>` 参数：指定要跳过的列
- [ ] 跳过列时 INSERT 排除该列（写 NULL 或 DEFAULT 值）
- [ ] 迁移完成后输出 "N columns skipped across M tables: ..." 摘要
- [ ] 与已有功能兼容（不影响全量迁移默认行为）
- [ ] 单元测试：column skip 逻辑、skip-body 预设

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-migrate/src/remote_import.rs` | 修改：增加列过滤逻辑 + CLI 参数解析 |
| `crates/aigw-migrate/src/main.rs` | 修改：新增 CLI args `--skip-body`, `--skip-columns` |

## 技术方案

### CLI 参数定义

```
aigw-migrate remote-import \
  --skip-body                                    # 快捷：跳过 spend_logs.messages,spend_logs.response
  --skip-columns spend_logs.metadata,virtual_keys.metadata
```

```rust
#[derive(Parser)]
struct RemoteImportArgs {
    // ... existing args ...
    /// Skip large body columns (messages, response) in spend_logs
    #[arg(long)]
    skip_body: bool,

    /// Comma-separated table.column pairs to skip during import
    /// Example: --skip-columns spend_logs.messages,spend_logs.response
    #[arg(long, value_delimiter = ',')]
    skip_columns: Vec<String>,
}
```

### 跳过逻辑

在 `migrate_proxy_models()` 等迁移函数中，INSERT 前过滤列：

```rust
fn should_skip_column(table: &str, column: &str, skip_columns: &HashSet<(String, String)>, skip_body: bool) -> bool {
    if skip_body && table == "spend_logs" {
        return column == "messages" || column == "response";
    }
    skip_columns.contains(&(table.to_string(), column.to_string()))
}
```

在构造 INSERT 列列表时：

```rust
let columns: Vec<_> = target_columns
    .iter()
    .filter(|col| !should_skip_column("spend_logs", col, &skip_columns, skip_body))
    .collect();
```

跳过的列写入 NULL 或使用 DEFAULT 值（取决于 `skip_body` profile 还是 `skip_columns`）。

### 实现步骤

1. 在 `main.rs` 中添加 CLI 参数
2. 解析 `--skip-columns` 为 `HashMap<String, HashSet<String>>`（table_name → set of columns）
3. 将 `skip_body` 和 `skip_columns` 传入各迁移函数
4. 在 `build_column_merge()` 过滤阶段移除要跳过的列
5. INSERT 时略过被排除的列
6. 迁移结束时打印跳过统计

### 跳过统计输出

```
Import Summary:
  spend_logs: 668,000 rows
  proxy_models: 12 rows
  ...

Skipped columns:
  spend_logs.messages       (--skip-body)
  spend_logs.response       (--skip-body)
  virtual_keys.metadata     (--skip-columns)
```

## 依赖

- 无（独立后端 CLI 改动，不影响运行时）

## 风险

- 跳过 messages/response 列后，aigw 中这些字段为 NULL → Spend Logs 详情抽屉（Stage 42）无法展示消息内容（属于预期行为：用户选择了跳过，自然看不到）
- 列名大小写问题（PostgreSQL vs MySQL vs SQLite）→ 使用 `target_column_info()` 中获取的列名，已正确处理
