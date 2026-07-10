# Phase 13: 前端反馈改进 — Spend Logs / Usage / Users / Orgs / Playground

> **背景**: 用户使用反馈驱动的改进阶段。基于对 litellm admin UI 的分析和当前 aigw 前端/后端的差距评估，规划 6 个 Stage 的改进工作。

**日期**: 2026-07-10
**Phase**: 13
**触发**: 用户使用反馈（4 大问题领域）+ TTFT 实现差距调研

---

## 前置调研：TTFT 实现差距

### 结论：schema 里有，代码里没写

调研记录见内存 `[[ttft-implementation-gap]]`。

| 层 | 状态 | 根因 |
|----|------|------|
| Schema (`completion_start_time` 列) | ✅ 存在 | 从 litellm schema 迁移时带入 |
| Struct (`SpendLog.completion_start_time`) | ✅ 存在 | models.rs 有字段定义 |
| **写入时赋值** | ❌ 全是 None | 3 个插入点全部硬编码 `completion_start_time: None` |
| **Streaming 代理** | ❌ 没实现 | `chat.rs:728` 返回 stub JSON，注释写 "future work"；streaming 时根本不写 SpendLog |

### litellm 怎么做（参考）

1. **不存 `ttft_ms` 列** — 查询时 SQL 动态计算：`(completionStartTime - startTime) * 1000`
2. **首个 streaming chunk 到达时** `datetime.now()` → `completion_start_time`
3. **非 streaming** 设 `completionStartTime = endTime` 作为哨兵值，SQL 中 `CASE WHEN` 排除
4. API 排序用 SQL 表达式而非存储字段

### 要补什么（纳入 Stage 34）

1. 实现真正的 SSE streaming 代理（改写 `chat.rs` streaming 路径）
2. 首个 chunk 到达时捕获 `completion_start_time`
3. 非 streaming 路径设 `completion_start_time = end_time`
4. 查询时 SQL 计算 TTFT，API 响应返回 `ttft_ms`

---

## 反馈分析 → 差距评估

### 1. Spend Logs 页面

| 现状 | 目标 | 差距 |
|------|------|------|
| 无 Live Tail | 可开关 Live Tail (15s auto-refresh) | 缺 UI toggle + 轮询状态管理 |
| 无 Fetch 按钮 | 主动 Fetch 按钮（重 fetch 不重置 Live Tail） | 缺独立 fetch 控件 |
| 无分页（limit=100） | page/page_size 分页（默认 30），后端返回 total_pages | 前后端都缺分页 |
| 无 request_id 搜索 | request_id 搜索过滤 | 后端缺 request_id 参数 |
| 仅 start/end date | 时间预设（15分钟/4小时/24小时/7天/自定义），自定义时展开 date picker | 前端缺预设组件 |
| 5 列（Time/Model/Tokens/Cost/Status） | 增加 Type, Session ID, Request ID, TTFT, Duration, Key Name 等 | 前端列缺失 + 后端缺 TTFT/session_id 字段返回 |
| 无详情抽屉 | 点击行 → 右侧抽屉展示 request/response 详情 | 缺 Drawer 组件 + 后端缺详情端点 |
| 默认 limit=100 | 默认 page_size=30，可更改 | 前后端 adapt |

**litellm 参考**: `view_logs/index.tsx` + `LogsTableToolbar` + `LogDetailsDrawer` + `log_filter_logic.tsx`

### 2. Usage 页面

| 现状 | 目标 | 差距 |
|------|------|------|
| 调用 `/global/spend/logs` 计算 Period Spend | 不应该展示请求日志数据；使用 aggregated 聚合端点 | 后端缺 `/global/spend/daily-activity` 聚合端点 |
| `/spend/providers` 部分 provider 字段未解密 | 全部正确解密 | 后端 provider 解析逻辑可能缺 master_key 或 JSON_EXTRACT 失败 |
| 仅 Model Bar + Provider Pie | 增加按天 bar chart（3/7/30/自定义天） | 前端缺 daily 图表 + 后端缺 daily 聚合 |
| Total Requests 取 `/global/spend/logs` 的 total_count（受 limit 影响） | 全数据库总量（不限时间） | 后端缺总请求计数端点 |

### 3. Organizations + Users 页面

| 现状 | 目标 | 差距 |
|------|------|------|
| `/org/list` 可能不返回新建 org | 正确列出所有 org | 后端 bug 调查 |
| `/user/list` 无分页，全量返回 | page/page_size 分页（默认 10，可更改） | 前后端都缺分页 |

**litellm 参考**: `GET /user/list?page=X&page_size=Y` → `{ users, total, page, page_size, total_pages }`

### 4. Playground 页面

| 现状 | 目标 | 差距 |
|------|------|------|
| 单次对话（System + User → Response） | 多轮对话聊天界面 (Chat UI) | 完全重写 |
| 无对话历史 | 对话上下文持久化（localStorage/sessionStorage） | 缺 chat history |
| 简单 Settings sidebar | 自定义配置（多模型对比、tools/functions、MCP 工具） | 缺 advanced features |

**litellm 参考**: `ChatUI.tsx` + `ChatPage.tsx` — chat-like interface with model compare, MCP tools, conversation history

---

## Phase 13 Stage 规划

### 依赖图

```
Stage 34 (SSE streaming + completion_start_time + 后端分页/TTFT)
  ├── Stage 35 (daily_spend 聚合表迁移 + 定时写入)
  │     ├── Stage 36 (前端 Spend Logs 重构：Live Tail + 时间预设 + 分页 + 详情抽屉)
  │     └── Stage 38 (Usage 聚合端点 + 前端 Global 视图重构)
  │
Stage 37 (Users/Orgs 端到端修复 + Provider 解密)

Stage 39 (Playground 对话升级：Chat UI + 历史 + 上下文) — 独立
```

**Stage 34 + 37 可并行**（独立后端改动）。
**Stage 35 依赖 34**（daily_spend 写入需要在 spend_log 写入路径中触发）。
**Stage 36 依赖 34**（前端 TTFT 列/Live Tail/pagination 需要后端 streaming+分页；/global/spend/logs 来自 spend_logs 表）。
**Stage 38 依赖 35**（Usage activity 端点从 daily_spend 表查，避免扫全量 spend_logs）。
**Stage 39 独立**（纯前端改造）。

---

## Stage 明细

### Stage 34: 后端 SSE Streaming + completion_start_time + Spend Logs 增强

**预估**: 5h
**难度**: 高
**类型**: 后端

**目标**: 实现真正的 SSE 流式代理，捕获 `completion_start_time`，补齐 `/global/spend/logs` 的分页、过滤、TTFT 查询。

**后端改动**:

**A. SSE Streaming 代理实现** (`routes/chat.rs`):
1. 改写 streaming 路径（当前返回 stub JSON），实现真正的 SSE 字节流代理：
   - 使用 `reqwest::Response` 的 `bytes_stream()` 逐 chunk 读取上游
   - 用 `axum::response::Sse` 包装后转发给客户端
   - 首个 chunk 到达时记录 `completion_start_time = Utc::now()`
2. streaming 完成后正常写 SpendLog（含 `completion_start_time`）
3. 同样改造 `v1_messages.rs` 的 Claude streaming 路径

**B. `completion_start_time` 捕获**:
4. 非 streaming 路径：`completion_start_time = Some(end_time)`（对齐 litellm 哨兵值）
5. 在 3 处 SpendLog 构造点写入正确值（替代当前的 `None`）

**C. Spend Logs 查询增强** (`routes/spend.rs` + `db.rs`):
6. 新增查询参数：
   - `request_id: Option<String>` — request_id 搜索
   - `page: Option<i32>` — 页码（默认 1）
   - `page_size: Option<i32>` — 每页条数（默认 30）
7. DB 层：
   - `query_spend_logs_filtered()` — 添加 `request_id` filter + `OFFSET/LIMIT` 分页
   - `query_spend_logs_count()` — 所有过滤条件下的 total count
   - SQL: `WHERE request_id = ?` + `ORDER BY start_time DESC LIMIT ? OFFSET ?`
8. SQL 计算 TTFT（对齐 litellm）：
   - SQLite: `(julianday(completion_start_time) - julianday(start_time)) * 86400000 AS ttft_ms`
   - PG: `EXTRACT(EPOCH FROM (completion_start_time - start_time)) * 1000 AS ttft_ms`
   - MySQL: `TIMESTAMPDIFF(MICROSECOND, start_time, completion_start_time) / 1000 AS ttft_ms`
   - 仅当 `completion_start_time != end_time` 时有效（排除非 streaming 哨兵值）

**D. 响应体增强**:
```json
{
  "data": [{ ...spendlog_fields, "ttft_ms": 123.4 }],
  "count": 30,
  "total_count": 1523,
  "page": 1,
  "page_size": 30,
  "total_pages": 51
}
```

**SQL 变更**: 无 schema 变更，仅查询调整和代码逻辑修改。

**BDD**:
- SSE streaming proxying (chunks forwarded correctly)
- completion_start_time captured on first stream chunk
- TTFT returned in spend logs response (streaming rows)
- TTFT is null for non-streaming rows (sentinel value)
- spend logs pagination
- request_id filter

---

### Stage 35: daily_spend 聚合表迁移 + 定时写入

**预估**: 3.5h
**难度**: 中
**类型**: 后端

**目标**: 创建 6 张 `daily_*_spend` 预聚合表（对齐 litellm `LiteLLM_Daily*Spend`），实现内存队列 + 定时 batch upsert。后续 Usage activity 端点从 daily_spend 表查询而非扫全量 spend_logs。

**6 张表**: `daily_user_spend`, `daily_team_spend`, `daily_organization_spend`, `daily_end_user_spend`, `daily_agent_spend`, `daily_tag_spend`。所有表结构相同（仅 entity_id 列不同），UNIQUE 约束为 composite key：`(entity_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)`。

**多实例正确性**: 使用 `ON CONFLICT DO UPDATE SET col = col + EXCLUDED.col` 的原子增量语义。两个实例同时 upsert 同一行，数据库保证各自增量正确累加，不需要 Redis 协调。

**写入流程**: 请求完成 → `queue_daily_spend_update(tx)` → 内存队列 → 每 10s drain 聚合 → batch ON CONFLICT DO UPDATE。

**关键改动**:
- 3 份 migration（SQLite/PG/MySQL）
- `chat.rs` + `v1_messages.rs` 在 spend_log 写入后调用 queue
- `daily_spend_queue.rs` — 内存队列 + tokio::spawn 定时 drain
- `db.rs` — `batch_upsert_daily_spend()` 函数

**BDD**: daily spend record insert, batch upsert, concurrent upsert

### Stage 36: 前端 Spend Logs 重构

**预估**: 5h
**难度**: 高
**类型**: 前端

**目标**: 接近 litellm Spend Logs 体验的完整页面。

**前端改动** (`src/pages/spend-logs/index.tsx`):

1. **时间预设组件** — 替代裸 date picker:
   - 预设按钮组: 15 分钟, 4 小时, 24 小时, 7 天, 自定义
   - 选择"自定义"时展开 start/end date picker
   - 默认选中"24 小时"

2. **Live Tail 开关**:
   - Toggle Switch（默认关闭）
   - 开启时: `refetchInterval: 15_000`，仅对 page=1 生效
   - 绿色 banner: "Auto-refreshing every 15 seconds" + Stop 按钮
   - 状态写入 `sessionStorage`

3. **Fetch 按钮**:
   - 主动触发 `refetch()` 的手动刷新按钮
   - 与 Live Tail 互不干扰

4. **Request ID 搜索**:
   - Input 框 + 300ms debounce

5. **分页组件**:
   - "Showing X - Y of Z results" + "Page N of M"
   - Previous/Next 按钮
   - Page size selector (30/50/100)

6. **表格列增强**:
   - Time (sortable), Type (call_type badge), Status, Session ID, Request ID (copyable), TTFT, Duration, Key Name (api_key prefix), Model, Tokens, Cost
   - 移动端: 卡片布局适配关键字段

7. **详情抽屉** (右侧 Drawer):
   - 使用 shadcn/ui Sheet 组件
   - 点击表格行 → 打开抽屉
   - 展示: Request ID, Status, Model, Tokens (prompt/completion/total), Cost, TTFT, Duration, Start Time, End Time, Messages (request/response JSON), Tags, API Key, User, Team, Org
   - 可以从当前日志的 `messages` 和 `response` BLOB 字段展示（若后端返回）

8. **状态管理**:
   - Loading: Skeleton table rows
   - Empty: "No spend logs found" + 调整过滤建议
   - Error: 错误信息 + Retry

**BDD**: 
- spend-logs page load with 24h default
- time preset switching
- Live Tail toggle
- pagination navigation
- request_id search
- log detail drawer open/close
- mobile card list view

---

### Stage 37: Users/Orgs 端到端修复 + Provider 解密

**预估**: 4.5h
**难度**: 中
**类型**: 前后端

**目标**: 修复 Orgs list bug、Users 列表分页（后端+前端）、Provider 字段解密。

**后端改动**:

1. **`/org/list` 调查与修复**:
   - 检查 SQL 查询和 handler 逻辑
   - 可能原因: `organization_id` 字段使用 `REPLACE(UUID, '-', '')` 生成（无连字符），但前端可能期望完整 UUID
   - 也可能: 数据库事务未提交或 query invalidation 问题

2. **`/user/list` 分页支持**:
   - 新增 query params: `page: Option<i32>` (default 1), `page_size: Option<i32>` (default 10)
   - DB 层: 新增分页查询 + total count
   - 响应: `{ data: [...], total_count: N, page: 1, page_size: 10, total_pages: M }`

3. **`/spend/providers` provider 解密修复**:
   - 调查 `build_decrypted_provider_map()` 中 `aigw_master_key` 的可用性
   - 增强 plaintext JSON fallback 逻辑
   - 添加 master_key 缺失时的 WARN 日志

**前端改动**:

4. **`src/pages/users/index.tsx`** — 添加分页:
   - 新增 state: `page`, `pageSize` (default 10)
   - `useQuery` key 加入 `page` 和 `pageSize`
   - 分页控件: Previous/Next + Page N of M + page size selector (10/25/50)
   - 更新 `UserListResponse` 接口

5. **`src/pages/orgs/index.tsx`** — 验证修复:
   - 确认新建 org 后 `queryClient.invalidateQueries` 正确触发

**BDD**: 
- org create then list verification
- user list pagination (next page, page size change)
- spend/providers with encrypted credentials test

---

### Stage 38: Usage Overview 聚合 + 前端 Global 视图重构

**预估**: 5.5h
**难度**: 中
**类型**: 前后端

**目标**: 新增 `/global/spend/overview` 端点（参考 litellm `/user/daily/activity/aggregated`），一次性返回 metadata 汇总 + daily 按天数据。Usage 页面不再调用 `/global/spend/logs`。

**设计依据**: litellm 没有单独的 `/global/spend/total-requests` 端点。Usage 页面的 5 个指标卡片（Total Spend / Requests / Successful / Failed / Avg Cost / Total Tokens）全部来自 `/user/daily/activity/aggregated` 返回的 `metadata` 字段。Daily Spend 柱状图来自 `results` 字段。按 Global/Org/Team 分割通过对不同端点加 `user_id`/`org_id`/`team_id` 参数实现。

**关于 `/global/spend/logs`**: 保留。Spend Logs 页面（Stage 35）需要分页日志列表，这正是它的用途。只是 Usage 页面不应依赖它。

**后端改动**:

新增 `GET /global/spend/overview`:

Query params: `start_date`, `end_date`（必填），`user_id`（可选）

两个 SQL 并行执行（`tokio::join!`）:

1. metadata 汇总查询:
   ```sql
   SELECT COALESCE(SUM(spend), 0) AS total_spend,
          COUNT(request_id) AS total_requests,
          COUNT(CASE WHEN status='success' THEN 1 END) AS successful_requests,
          COUNT(CASE WHEN status='failure' THEN 1 END) AS failed_requests,
          COALESCE(SUM(total_tokens), 0) AS total_tokens,
          COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
          COALESCE(SUM(completion_tokens), 0) AS completion_tokens
   FROM spend_logs WHERE start_time BETWEEN ? AND ? [AND user = ?]
   ```

2. daily 按天查询:
   ```sql
   SELECT DATE(start_time) AS date,
          COALESCE(SUM(spend),0) AS spend,
          COALESCE(SUM(total_tokens),0) AS tokens,
          COUNT(request_id) AS requests
   FROM spend_logs WHERE start_time BETWEEN ? AND ? [AND user = ?]
   GROUP BY DATE(start_time) ORDER BY date ASC
   ```

响应格式:
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
    { "date": "2026-07-08", "spend": 1.2345, "tokens": 150000, "requests": 42 }
  ]
}
```

**前端改动** (`src/pages/usage/index.tsx`):

1. **移除 `/global/spend/logs` 依赖** — 删除 `logsData` query 和 `periodSpend` 计算
2. **单次请求取所有数据** — `GET /global/spend/overview` 返回 metadata + daily
3. **指标卡片** — Total Spend, Period Spend, Total Requests 改用 `metadata.*`
4. **新增 Daily Spend Bar Chart** — 数据源 `daily` 数组，X=date, Y=spend，tooltip 含 tokens+requests
5. **日期快捷选择** — 3天/7天/30天/自定义，默认 30 天
6. **保留现有图表** — Spend by Model bar chart（`/global/spend/models`），Spend by Provider pie（`/spend/providers`，依赖 Stage 36 解密修复）

**扩展方向（后续 Stage，本次不实现）**: 按 organization_id/team_id 过滤的 overview；breakdown 维度（按 model/provider 细分）。

**BDD**: overview endpoint, Usage page no /global/spend/logs calls, daily bar chart, date range switching

---

### Stage 39: Playground 对话升级

**预估**: 5h
**难度**: 高
**类型**: 前端

**目标**: 聊天式对话界面，支持多轮对话和上下文。

**前端改动** (`src/pages/playground/index.tsx`):

1. **Chat UI 布局**（参考 OpenWebUI / litellm ChatUI）:
   - 主区域: 消息列表（聊天气泡）+ 输入框
   - 侧边栏: 对话设置（可折叠）
   - 每个消息气泡: role icon + 内容（Markdown 渲染） + 复制按钮

2. **多轮对话支持**:
   - `useState<ChatMessage[]>` 管理消息历史
   - 每次发送时携带全部历史消息
   - 支持编辑已发送消息（修改后重新发送）
   - 支持删除消息（从历史中移除）

3. **对话设置侧边栏**:
   - Model 选择
   - Temperature slider
   - Max Tokens input
   - Top P, Frequency Penalty, Presence Penalty
   - System Prompt（全局，放在消息列表最前面）
   - Stop Sequences
   - Streaming toggle

4. **对话管理**:
   - New Chat 按钮（清空对话）
   - 对话历史（localStorage 持久化最近 N 条对话）
   - 对话重命名

5. **增强功能**:
   - Response 复制按钮
   - Token 用量显示（每条 assistant 消息下方）
   - Error retry
   - Stop generation（abort streaming）

6. **状态管理**:
   - Sending: 发送按钮变 Stop + spinner
   - Error: 错误消息 + Retry 按钮
   - Empty: "Start a conversation" 引导提示

**BDD**:
- playground page loads in chat layout
- multi-turn conversation (send, response, send again with context)
- streaming message display
- conversation history clear
- model selection
- parameter adjustment

---

## Stage 汇总

| Stage | 目标 | 类型 | 预估 | 优先级 | 依赖 |
|-------|------|------|------|--------|------|
| Stage 34 | SSE Streaming + completion_start_time + Spend Logs 增强 | 后端 | 5h | P0 | — |
| Stage 35 | daily_spend 聚合表迁移 + 定时写入 | 后端 | 3.5h | P0 | Stage 34 |
| Stage 36 | 前端 Spend Logs 重构（Live Tail+预设+抽屉） | 前端 | 5h | P0 | Stage 34 |
| Stage 37 | Users/Orgs 端到端修复 + Provider 解密 | 前后端 | 4.5h | P0 | — |
| Stage 38 | Usage 聚合端点 + 前端 Global 视图重构 | 前后端 | 5.5h | P1 | Stage 35 |
| Stage 39 | Playground 对话升级 | 前端 | 5h | P2 | — |

**总预估**: 28.5h（约 4 个工作日）

**并行策略**: Stage 34 + 37 可同时开始（不同文件）。
Stage 35 → 34 完成后。Stage 36 可与 35 并行。Stage 38 → 35 完成后（需要 daily_spend 表）。Stage 39 独立于所有其他 Stage。

## 验证标准

| 编号 | 标准 | 验证方式 |
|------|------|----------|
| V1 | SSE streaming 正确代理上游 chunk 到客户端 | BDD |
| V2 | completion_start_time 在首个 chunk 时正确捕获 | BDD |
| V3 | streaming 请求的 SpendLog 包含 ttft_ms（非 null） | BDD |
| V4 | 非 streaming 请求的 ttft_ms 为 null（哨兵值） | BDD |
| V5 | Spend Logs 页面具有 Live Tail 开关，开启后 15s 自动刷新 | BDD + 手动 |
| V6 | 时间预设按钮正确切换时间范围，自定义时显示 date picker | BDD |
| V7 | 分页组件正确显示页码、条数，可切换每页条数 | BDD |
| V8 | request_id 搜索能正确过滤日志 | BDD |
| V9 | 点击日志行弹出右侧抽屉，显示详情（含 TTFT） | BDD |
| V10 | Usage 页面不再调用 /global/spend/logs | Network mock BDD |
| V11 | Daily bar chart 正确渲染按天聚合数据 | BDD |
| V12 | Total Requests 显示全库总量 | 手动验证 |
| V13 | Provider 解密正确显示所有 provider 名称 | BDD + 手动 |
| V14 | 新建 org 后正确出现在组织列表 | BDD |
| V15 | Users 列表分页正常工作 | BDD |
| V16 | Playground 支持多轮对话，上下文正确传递 | BDD |
| V17 | Playground 聊天历史可清除 | BDD |
| V18 | 全部 BDD 测试通过（新增 ~25 scenarios） | CI |
