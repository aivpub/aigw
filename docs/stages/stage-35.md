# Stage 35: daily_spend 聚合表迁移 + 定时写入

**Phase**: 13 — 前端反馈改进 + SSE Streaming + TTFT
**状态**: ⏳ 待开始
**预估**: 3.5h

---

## 目标

1. 创建 6 张 `daily_*_spend` 预聚合表（对齐 litellm 的 `LiteLLM_Daily*Spend`）
2. 实现请求完成时的 daily_spend 增量写入（内存队列 + 定时 batch upsert）
3. 后续 Stage 38 Usage 聚合端点直接从 daily_spend 表查询

## 背景

litellm 不直接从 `spend_logs` 表扫全量数据做 Usage 聚合。它有 6 张预聚合表：

| litellm 表名 | aigw 表名 | 分组键 | 用途 |
|-------------|----------|--------|------|
| `LiteLLM_DailyUserSpend` | `daily_user_spend` | `user_id` | Usage 按 user 过滤 |
| `LiteLLM_DailyTeamSpend` | `daily_team_spend` | `team_id` | `/team/daily/activity` |
| `LiteLLM_DailyOrganizationSpend` | `daily_organization_spend` | `organization_id` | `/organization/daily/activity` |
| `LiteLLM_DailyEndUserSpend` | `daily_end_user_spend` | `end_user_id` | 按最终用户 |
| `LiteLLM_DailyAgentSpend` | `daily_agent_spend` | `agent_id` | 按 agent |
| `LiteLLM_DailyTagSpend` | `daily_tag_spend` | `tag` + `request_id` | 按标签 |

每张表结构相同（以 `daily_user_spend` 为例）：

```sql
CREATE TABLE daily_user_spend (
    id TEXT PRIMARY KEY,
    user_id TEXT,
    date TEXT NOT NULL,                     -- YYYY-MM-DD
    api_key TEXT NOT NULL,
    model TEXT,
    model_group TEXT,
    custom_llm_provider TEXT,
    mcp_namespaced_tool_name TEXT,
    endpoint TEXT,
    prompt_tokens BIGINT DEFAULT 0,
    completion_tokens BIGINT DEFAULT 0,
    cache_read_input_tokens BIGINT DEFAULT 0,
    cache_creation_input_tokens BIGINT DEFAULT 0,
    spend REAL DEFAULT 0.0,
    api_requests BIGINT DEFAULT 0,
    successful_requests BIGINT DEFAULT 0,
    failed_requests BIGINT DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_user_spend_date ON daily_user_spend(date);
CREATE INDEX idx_daily_user_spend_user_date ON daily_user_spend(user_id, date);
```

## 多实例正确性

写入流程：请求完成 → 构造 daily_spend 记录 → 内存队列 → 定时 drain → batch upsert。

多实例下的关键保证是 **SQL 层面的 ON CONFLICT DO UPDATE 原子增量**：

```sql
INSERT INTO daily_user_spend (...)
VALUES (?, ?, ?, ?, ?)
ON CONFLICT (unique_composite_key)
DO UPDATE SET
    spend = daily_user_spend.spend + EXCLUDED.spend,
    prompt_tokens = daily_user_spend.prompt_tokens + EXCLUDED.prompt_tokens,
    completion_tokens = daily_user_spend.completion_tokens + EXCLUDED.completion_tokens,
    api_requests = daily_user_spend.api_requests + EXCLUDED.api_requests,
    successful_requests = daily_user_spend.successful_requests + EXCLUDED.successful_requests,
    failed_requests = daily_user_spend.failed_requests + EXCLUDED.failed_requests,
    updated_at = CURRENT_TIMESTAMP
```

`col = col + EXCLUDED.col` 在 PostgreSQL/MySQL/SQLite 层面都是原子操作。两个实例同时 upsert 同一行，数据库保证各自的增量正确累加，不会互相覆盖。不需要 Redis 协调，不需要 leader election。

## 验收标准

- [ ] 6 张 `daily_*_spend` 表在三数据库的 migration 全部建好
- [ ] `SpendLog` 写入时同步调用 `queue_daily_spend_update()` 写入内存队列
- [ ] 后台定时任务（每 10s）drain 队列、按 composite key 聚合、batch upsert
- [ ] `ON CONFLICT DO UPDATE` 正确实现整数累加
- [ ] `aigw-migrate` 支持从 litellm `LiteLLM_Daily*Spend` 表导入数据
- [ ] 新增列 `ttft_ms` 和 `completion_start_time`（非 litellm 原始字段，aigw 特有）
- [ ] BDD：daily spend record insert, batch upsert, concurrent upsert

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/migrations/sqlite/011_daily_spend.sql` | 新建 — 6 张表 DDL |
| `crates/aigw-core/migrations/postgres/011_daily_spend.sql` | 新建 |
| `crates/aigw-core/migrations/mysql/011_daily_spend.sql` | 新建 |
| `crates/aigw-core/src/models.rs` | 新增 `DailySpendLog` struct + SQLx FromRow |
| `crates/aigw-server/src/routes/chat.rs` | 请求完成路径调用 queue 写入 |
| `crates/aigw-server/src/routes/v1_messages.rs` | 同上 |
| `crates/aigw-core/src/db.rs` | 新增 `batch_upsert_daily_spend()` |
| `crates/aigw-core/src/daily_spend_queue.rs` | 新建 — 内存队列 + drain 逻辑 |

## 数据流

```
请求完成 (chat.rs / v1_messages.rs)
  → insert_spend_log()  (已有)
  → queue_daily_spend_update(tx)  (新增，非阻塞)
       ↓
  内存队列 (per entity type, asyncio-like bounded channel)
       ↓ (每 10s tokio::spawn)
  drain queue, 按 composite key 聚合
       ↓
  batch ON CONFLICT DO UPDATE
  写入 daily_*_spend 表
```

## 依赖

- Stage 34（spend_log 写入路径重构后——需要拿到正确的 status / tokens / spend 值才能写入 daily_spend）

## 风险

- **迁移号冲突**: 当前最新 migration 是 `010`，需要确认
- **ttft_ms / completion_start_time**: 这两列在 litellm `LiteLLM_Daily*Spend` 中不存在。Stage 34 新增后，考虑是否同步到 daily_spend 表（暂无需求，可后续扩展）
- **队列积压**: 如果 DB 写入慢于请求速率，队列可能积压。batch upsert + bounded channel backpressure
