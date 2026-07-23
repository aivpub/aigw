# Stage 77: Spend Logs Body 字段分离 —— 列表移除 + 详情端点

**Phase**: 26 — 可观测性 (Observability)
**优先级**: P1
**状态**: ✅ 完成
**完成日期**: 2026-07-23
**预估**: 5h
**前置**: Stage 45 已规划但未落地

---

## 背景

Stage 45 规划了「列表接口移除 body + 新增详情端点按需获取」方案，但当前代码只做了折中——`page_size <= 50` 时有条件返回 `messages`/`response`。这导致：

- 默认 page_size=30 时，列表响应中仍包含大量 blob 数据（单个请求可达几十 KB）
- 没有独立的详情端点，抽屉关闭后数据不可单独刷新
- SQL 查询始终 SELECT 全部 32 列（包括 body），数据库层无优化

## 目标

1. **两个列表端点** (`GET /spend/logs`、`GET /global/spend/logs`) 永久移除 `messages` / `response` 字段
2. **新增详情端点** `GET /global/spend/logs/{request_id}` 按需返回完整 body
3. **前端交互**：点击行 → 抽屉立即打开（显示已有元信息） → 自动 fetch 详情 → body 区域 Skeleton 加载 → 成功显示 / 失败展示错误+重试
4. **测试覆盖**：UT + BDD 全量覆盖

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-core/src/db.rs` | 修改：trait + 3 后端 impl + Database 通用方法 + UT |
| `crates/aigw-server/src/routes/spend.rs` | 修改：新增 handler + 移除列表 body + UT |
| `crates/aigw-server/src/main.rs` | 修改：新增路由 |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 修改：新增 detail query + enrichedLog + loading/error 状态 |
| `crates/aigw-frontend/tests/features/spend-logs.feature` | 修改：新增 scenario |
| `crates/aigw-frontend/tests/steps/spend-logs.steps.ts` | 修改：新增 step 定义 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | 修改：更新 mock 数据 + 新增 detail mock |

## 验收标准

- [x] `GET /spend/logs` 返回的 JSON 不包含 `messages` / `response` 字段
- [x] `GET /global/spend/logs` 返回的 JSON 不包含 `messages` / `response` 字段
- [x] `GET /global/spend/logs/{request_id}` 返回 200 + 完整 body（含 messages、response、proxy_server_request）对于有效的 request_id
- [x] `GET /global/spend/logs/{request_id}` 返回 404 对于不存在的 request_id
- [x] `GET /global/spend/logs/{request_id}` 返回 401 无认证
- [x] 前端抽屉点击行后自动 fetch 详情端点
- [x] 加载过程中 body 区域显示 Skeleton 骨架屏
- [x] 加载失败时 body 区域显示错误 + 重试按钮
- [x] 加载成功后 body 区域显示 InputCard/OutputCard 可视化
- [x] 所有现有测试继续通过（全量 workspace 通过）

## 技术方案

### DB 层

在 `SpendLogStore` trait 新增 `get_spend_log_by_request_id(&self, request_id: &str) -> Result<Option<SpendLog>>`，三个后端统一 delegate 到 `Database` 的通用方法（single match dispatch + `sqlx::query_as::<_, SpendLog>` + `fetch_optional`）。

### 后端

- 新增 `global_spend_log_detail` handler，鉴权使用 `SpendAuth` + `require_admin(&auth)?`
- 从 `spend_logs` 和 `global_spend_logs` 两个 handler 的 JSON 序列化中移除 `messages`/`response`
- 路由注册**先参数后精确**：`/global/spend/logs/{request_id}` 在 `/global/spend/logs` 之前

### 前端

- 点击行 → `setSelectedLog(log); setDrawerOpen(true)`（抽屉立即展示元信息）
- `useQuery` 在 `selectedLog` 非 null 时自动 fetch `/global/spend/logs/{request_id}`
- 合并 detail 数据到 `enrichedLog`，传给 `DetailDrawer`
- body 区域三种状态：loading（Skeleton）、error（错误消息+Retry）、success（InputCard/OutputCard）

## 依赖

- 无外部依赖
- 不影响其他模块

## 风险

- 路由注册顺序：参数路由必须在精确路由之前，否则 `/global/spend/logs/abc-123` 被 `/global/spend/logs` 误捕获 ✅ 已在 main.rs 和 test_app() 中以正确顺序注册
- UT `test_app()` 需同步注册新路由 ✅ 已更新
- 前端 `queryKey` 绑定 `selectedLog?.request_id`，切换行自动重新 fetch ✅ 通过 `detailRequestId` 状态 + `enabled` 控制

## 实现记录

**完成日期**: 2026-07-23

**改动摘要**:

| 文件 | 改动 |
|------|------|
| `crates/aigw-core/src/db.rs` | `SpendLogStore` trait 新增 `get_spend_log_by_request_id`；3 后端 impl（SqlitePool/MySqlPool/PgPool）；`Database` enum dispatch |
| `crates/aigw-server/src/routes/spend.rs` | 新增 `global_spend_log_detail` handler；`spend_logs` 和 `global_spend_logs` 移除 `messages`/`response` 字段；test 新增 4 个 UT |
| `crates/aigw-server/src/main.rs` | 注册 `/global/spend/logs/{request_id}` 路由（在 `/global/spend/logs` 之前） |
| `crates/aigw-frontend/src/pages/spend-logs/index.tsx` | 拆分为 `SpendLog`（列表，无 body）+ `SpendLogDetail`（含 body）；`DetailDrawer` 新增 loading/error/retry 状态；`useQuery` 按需 fetch 详情；`enrichedLog` 合并数据 |
| `crates/aigw-frontend/tests/steps/api-mocks.ts` | 新增 detail mock 端点 + sampleDetailLog1/2 |

**测试结果**: 全量 workspace 测试通过（502 passed, 0 failed）。新增 4 个 UT：详情 401/404/200 + 列表不含 body。
