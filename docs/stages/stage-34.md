# Stage 34: SSE Streaming + completion_start_time + Spend Logs 增强

**Phase**: 13 — 前端反馈改进 + SSE Streaming + TTFT
**状态**: ⏳ 待开始
**预估**: 5h

---

## 目标

1. 实现真正的 SSE streaming 代理（替代当前构造空 JSON 返回）
2. 在首个 streaming chunk 到达时捕获 `completion_start_time`
3. 补齐 `/global/spend/logs` 的分页、request_id 过滤、TTFT 查询

## 背景

当前 `chat.rs:728-763` 和 `v1_messages.rs:271-284` 的 streaming 路径并未代理上游 SSE 流。

实际行为：
- 发送请求到上游后，校验状态码，然后**丢弃上游响应**
- 构造一个空 JSON 对象返回给客户端：
  - `chat.rs`：返回 `chat.completion.chunk`，`delta.content` 为空字符串
  - `v1_messages.rs`：返回 `message`，`content` 为空数组，`usage` 全 0
- 注释中写道 "*The actual streaming proxy is handled by the proxy.rs module*"，但 `proxy.rs` 中并不存在对应的代理逻辑
- streaming 路径不创建 SpendLog，因此 `completion_start_time` 从未被捕获

同时 `completion_start_time` 列虽在 schema 中存在，但所有非 streaming 插入点也硬编码为 `None`（见 `[[ttft-implementation-gap]]`）。

litellm 的方式：
- 首个 chunk 到达时 `datetime.now()` → `completionStartTime`
- 非 streaming 时 `completionStartTime = endTime`（哨兵值）
- 查询时 SQL 计算 TTFT：`(completionStartTime - startTime) * 1000`
- SQL `CASE WHEN` 排除哨兵值

## 验收标准

- [ ] `stream: true` 的 chat completions 请求正确代理上游 SSE chunk 到客户端
- [ ] 首个 chunk 到达时 `completion_start_time` 被记录
- [ ] streaming 完成后正确创建 SpendLog（含 `completion_start_time`）
- [ ] 非 streaming 路径 `completion_start_time = Some(end_time)`（哨兵值）
- [ ] `stream: true` 的 `/v1/messages` 请求同样改造
- [ ] `/global/spend/logs` 支持 `request_id` 查询参数
- [ ] `/global/spend/logs` 支持 `page` / `page_size` 分页（默认 page=1, page_size=30）
- [ ] SQL 查询返回 `ttft_ms`（streaming 行有值，非 streaming 行为 null）
- [ ] 响应包含 `page, page_size, total_pages, total_count`
- [ ] BDD: SSE streaming proxying, completion_start_time capture, TTFT in response, pagination

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/chat.rs` | 重写 streaming 路径（第 728-763 行） |
| `crates/aigw-server/src/routes/v1_messages.rs` | 重写 streaming 路径（第 271-284 行） |
| `crates/aigw-server/src/routes/spend.rs` | 新增 query params: request_id, page, page_size |
| `crates/aigw-core/src/db.rs` | 扩展 query_spend_logs_filtered + query_spend_logs_count |

## 技术方案

### A. SSE Streaming 代理

```
上游 reqwest Response (stream)
  → bytes_stream() 逐 chunk 读取
  → 首个非空 chunk: 记录 completion_start_time = Utc::now()
  → axum::response::Sse 包装 (data: + \n\n 格式)
  → 流完成后写 SpendLog（含 completion_start_time）
```

### B. completion_start_time 策略

| 请求类型 | completion_start_time 值 |
|----------|--------------------------|
| streaming (有 chunk) | 首个 chunk 到达时间 |
| streaming (无 chunk/错误) | end_time（哨兵） |
| non-streaming | end_time（哨兵） |

### C. SQL TTFT 计算

**SQLite**:
```sql
CASE WHEN completion_start_time IS NOT NULL 
     AND completion_start_time != end_time
THEN (julianday(completion_start_time) - julianday(start_time)) * 86400000.0
ELSE NULL END AS ttft_ms
```

**PostgreSQL**:
```sql
CASE WHEN completion_start_time IS NOT NULL 
     AND completion_start_time != end_time
THEN EXTRACT(EPOCH FROM (completion_start_time - start_time)) * 1000
ELSE NULL END AS ttft_ms
```

### D. 分页响应格式

```json
{
  "data": [{ "...": "...", "ttft_ms": 123.4 }],
  "count": 30,
  "total_count": 1523,
  "page": 1,
  "page_size": 30,
  "total_pages": 51
}
```

## 依赖

- 无（后端独立改动）

## 风险

- **SSE streaming 代理是最危险的改动**：此前从未实现真正的流式代理。需要处理上游断连、chunk 格式变化、错误注入等问题。
- 需要确保非 streaming 路径的现有 BDD 测试不受影响。
