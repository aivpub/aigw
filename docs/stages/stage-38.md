# Stage 38: Usage 聚合端点 + 前端 Global 视图重构

**Phase**: 13 — 前端反馈改进
**状态**: ⏳ 待开始
**预估**: 5.5h

---

## 目标

1. 新增 3 个聚合端点替换 `/global/spend/logs` 依赖
2. 前端 Usage Global 视图基于正确数据源，增加按天 Bar Chart

## 设计依据：litellm API 结构

litellm 用**多条独立路由**服务不同分组维度（背后是不同的物化汇总表）：

| 端点 | 分组维度 | 物化表 |
|------|---------|--------|
| `/user/daily/activity/aggregated` | Global / 按 user | `LiteLLM_DailyUserSpend` |
| `/team/daily/activity` | 按 team | `LiteLLM_DailyTeamSpend` |
| `/organization/daily/activity` | 按 org | `LiteLLM_DailyOrganizationSpend` |

所有端点返回一致的 `SpendAnalyticsPaginatedResponse { results + metadata }`。

aigw 没有物化汇总表，直接从 `spend_logs` 聚合。按 litellm 模式拆分为 3 条独立路由，WHERE 条件不同但响应格式统一。

### `/global/spend/logs` 还有存在的必要吗？

**有。** Spend Logs 页面（Stage 35）需要分页的日志列表数据，这正是 `/global/spend/logs`（Stage 34 增强版）的用途。只是 **Usage 页面不应该调用它**——Usage 应该用聚合端点。

## 验收标准

- [ ] `GET /global/spend/activity?start_date=X&end_date=Y` 返回 Global 聚合数据
- [ ] `GET /global/spend/activity?start_date=X&end_date=Y&user_id=U` 支持按 user 过滤
- [ ] 支持按 `team_id` 过滤
- [ ] 支持按 `organization_id` 过滤
- [ ] 响应包含 `metadata` 和 `daily`（统一结构）
- [ ] Usage 页面不再调用 `/global/spend/logs`
- [ ] Usage 页面的 Period Spend + Total Requests 卡改用 activity 数据
- [ ] 新增 Daily Spend Bar Chart
- [ ] 日期快捷选择：3天/7天/30天/自定义天数，默认 30 天
- [ ] BDD：activity endpoint, Usage 页面无 logs 调用, daily bar chart

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-server/src/routes/spend.rs` | 新增 `global_spend_activity` handler |
| `crates/aigw-core/src/db.rs` | 新增 activity 聚合查询（metadata + daily） |
| `src/pages/usage/index.tsx` | 重构数据源 + 新增 daily bar chart |
| `crates/aigw-server/src/main.rs` | 注册 `/global/spend/activity` 路由 |

## 后端改动

### 新增 `GET /global/spend/activity`

**Query params**:

```rust
pub struct ActivityQuery {
    pub start_date: String,            // 必填 YYYY-MM-DD
    pub end_date: String,              // 必填 YYYY-MM-DD
    pub user_id: Option<String>,       // 可选
    pub team_id: Option<String>,       // 可选
    pub organization_id: Option<String>, // 可选
}
```

三个分组维度的 filter 之间是 AND 关系，不传即为不分组（全局）：

| 不传任何 filter | 全局视图 |
| `user_id=X` | 按指定用户过滤 |
| `team_id=X` | 按指定 team 过滤 |
| `organization_id=X` | 按指定 org 过滤 |

**两条 SQL 并行执行**（`tokio::join!`）:

1. **metadata 查询** — 指定时间范围内的汇总指标:
   ```sql
   SELECT
       COALESCE(SUM(spend), 0) AS total_spend,
       COUNT(request_id) AS total_requests,
       COUNT(CASE WHEN status = 'success' THEN 1 END) AS successful_requests,
       COUNT(CASE WHEN status = 'failure' THEN 1 END) AS failed_requests,
       COALESCE(SUM(total_tokens), 0) AS total_tokens,
       COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
       COALESCE(SUM(completion_tokens), 0) AS completion_tokens
   FROM spend_logs
   WHERE start_time >= ? AND start_time <= ?
     [AND user = ?]
     [AND team_id = ?]
     [AND organization_id = ?]
   ```

2. **daily 查询** — 按天聚合:
   ```sql
   SELECT
       DATE(start_time) AS date,
       COALESCE(SUM(spend), 0) AS spend,
       COALESCE(SUM(total_tokens), 0) AS tokens,
       COUNT(request_id) AS requests
   FROM spend_logs
   WHERE start_time >= ? AND start_time <= ?
     [AND user = ?]
     [AND team_id = ?]
     [AND organization_id = ?]
   GROUP BY DATE(start_time)
   ORDER BY date ASC
   ```

**响应格式**（所有分组维度统一结构）:

```json
{
  "metadata": {
    "total_spend": 15.6789,
    "total_requests": 423,
    "successful_requests": 401,
    "failed_requests": 22,
    "total_tokens": 2300000,
    "prompt_tokens": 1400000,
    "completion_tokens": 900000
  },
  "daily": [
    { "date": "2026-07-08", "spend": 1.2345, "tokens": 150000, "requests": 42 },
    { "date": "2026-07-09", "spend": 2.3456, "tokens": 230000, "requests": 67 }
  ]
}
```

### 分组维度的路由设计

| 路由 | 分组维度 | 本 Stage |
|------|---------|----------|
| `/global/spend/activity` | Global + user/team/org 过滤 | ✅ 实现 |
| `/organization/spend/activity` | 同结构 + 必须传 org_id | 后续 |
| `/team/spend/activity` | 同结构 + 必须传 team_id | 后续 |

三端点共享同一个 handler 函数，仅通过路由区分默认 filter 语义。后续 `/organization/spend/activity` 和 `/team/spend/activity` 在本 Stage 先注册但返回 "not implemented" 占位，由后续 Phase 前端 EntityView 切换器补齐。

## 前端改动

```
移除:
  const { data: logsData } = useQuery(["global-spend-logs", ...])    ← 不再调用
  const periodSpend = useMemo(() => reduce(logs...), [logs])         ← 删除

改为:
  const { data: activity } = useQuery(["global-spend-activity", startDate, endDate])
  const metadata = activity?.metadata
  const dailyData = activity?.daily ?? []

Show cards:
  Total Spend       ← metadata.total_spend
  Period Spend      ← metadata.total_spend（指定时间范围内）
  Total Requests    ← metadata.total_requests（时间范围内全量，不受 limit 影响）
  Successful        ← metadata.successful_requests
  Failed            ← metadata.failed_requests
  Total Tokens      ← metadata.total_tokens (可展开 prompt/completion)

新增图表:
  Daily Spend Bar Chart:
    data = dailyData
    X: date, Y: spend
    Tooltip: date + spend + tokens + requests

日期选择:
  预设按钮组: 3D / 7D / 30D / 自定义
  默认 30D
```

## 后续 Stage 扩展点

- `/organization/spend/activity` — 必须传 `organization_id`
- `/team/spend/activity` — 必须传 `team_id`
- 前端 `EntityViewSelect` 组件 — 下拉切换 Global/Organization/Team，配合实体选择器

## 依赖

- Stage 34（spend_logs 数据写入）
- Stage 35（daily_spend 聚合表 — activity 端点从 daily_spend 查询而非扫全量 spend_logs）

## 风险

- SQLite `DATE()` 函数与 PG/MySQL 的兼容性
- `status` 字段实际值格式确认（`"success"`/`"failure"` 还是其他？）
- 日期时区：`start_time` 存 UTC，按 UTC 日期分组
