# Stage 84: 前端 Jobs 页面生产化重构（路由化 + 分页 + 矛盾检测 + a11y）

**Phase**: 31 — Body Archive 生产化
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 8h
**前置**: Stage 82（后端配置失联 + 状态机已修复，前端兜底依赖后端 summary.running + result 字段）

---

## 背景

前端审计确认 8 个用户反馈问题全部成立（详见 stage-roadmap.md Phase 31 背景与下方逐条定论）：

- Q1 job 卡 pending（后端根因但前端未兜底）
- Q2 logs 独立空区块未按 step 关联
- Q3 steps 假阳性 completed（前端无 result 列 + 矛盾检测）
- Q4 tab 含下划线 + Disabled 仍可触发
- Q5 Manual Trigger 独占行
- Q6 列表无分页
- Q7 详情页冗余 tab + 标题差 + Steps 无分页
- Q8 子页面不可 URI 直达

当前 `crates/aigw-frontend/src/pages/jobs.tsx` 602 行单文件巨石组件，原型级质量。本 Stage 把修复工作重新规划为 8h（原 14h 偏高 2-5 倍），按 subagent 并发实测下调；前端 BDD 用 Playwright mock API 不需后端，可多 subagent 并发 3 viewports。

## 目标（对应 8 问题）

1. **路由化**：`App.tsx` 加 `/dash/jobs/:jobId` 子路由；`jobs.tsx` 内 tab/selectedJob 改 `useSearchParams`；URI 直达 / 刷新 / 分享 / 后退（Q8）
2. **Tab 美化**：`STEP_LABELS` 映射（`body_archive` → "Body Archive"）+ fallback `replace(/_/g," ")`；卡片标题 + Trigger 按钮都用 label（Q4）
3. **Manual Trigger 同行**：删除独立 Card，按钮挪到 `TabsList` 右侧 flex 同行（Q5）
4. **Archive Disabled 联动**：Trigger 按钮 `disabled={archiveStats && !archiveStats.archive_enabled}` + tooltip（Q4）
5. **列表表格化 + 分页**：换 `<Table>`；加 page state + `<Pagination>`；后端 list response 加 `total` 字段（`jobs.rs:169-173`）；Overview 与 per-type 复用同一 `JobList` 组件（Q6）
6. **详情页去冗余**：独立路由隐藏外层 `TabsList`；标题改 `{step_type} · {trigger_type} · {created_at}`；Steps 加分页（pageSize=20）+ Payload/Result/Duration 列（Q7）
7. **Logs 关联 step**：Logs 表加 Step Key 列；Steps 每行可展开折叠面板按 `step_key` 分组；running 轮询条件改 `summary.running>0 || summary.pending>0`（Q2）
8. **矛盾检测 + 兜底**：`displayStatus = summary.running>0 ? 'running' : job.status`；`step.status==='completed' && result.rows_archived===0` → 灰色 "completed (no-op)"；错误 toast 替换 silent fail；a11y 键盘导航 + StatusBadge `aria-label`（Q1/Q3）

## 前端规划与交互流程

路由化页面拆分，两层级路由 + URL searchParams 驱动状态：

```
┌─────────────────────────────────────────────────────────────────────┐
│  /dash/jobs                                                         │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │ [Overview] [Body Archive] [Budget Reset]    [Trigger ▾] ←Q5  │   │
│  └──────────────────────────────────────────────────────────────┘   │
│  ┌─ Body Archive Overview ──────────────────────────────────────┐   │
│  │ ● Enabled   Archived: 450K rows / 75GB   Pending: 800 rows   │   │
│  └──────────────────────────────────────────────────────────────┘   │
│  ┌─ Recent Jobs (Table + Pagination) ───────────────────────────┐   │
│  │ ID │ Step │ Trigger │ Status │ Progress │ Created    │       │   │
│  │ job-abc │ body_archive │ manual │ 🟢 │ 8/24 │ 07-25 14:00 │   │   │
│  │ job-def │ body_archive │ cron   │ ✅ │ 1/1  │ 07-25 13:00 │   │   │
│  │                    ◀ 1 2 3 ... ▶  (page=1)                   │   │
│  └──────────────────────────────────────────────────────────────┘   │
└─────────────────────────│ 点击 job 行 / Enter / Space (a11y) ────────┘
                          ▼ useSearchParams({job: id}) → history.push
┌─────────────────────────────────────────────────────────────────────┐
│  /dash/jobs/:jobId            ← Q8 URI 直达/刷新/分享/后退           │
│  ← Back to list                                                     │
│  body_archive · manual · 2026-07-25 14:00        🟢 running  ←Q7    │
│  ┌─ Summary ───────────────────────────────────────────────────┐    │
│  │ Total: 24  Completed: 8  Failed: 0  Pending: 16             │    │
│  │ ████████░░░░░░░░░░░░  33%                                    │    │
│  └──────────────────────────────────────────────────────────────┘    │
│  ┌─ Steps (Table, pageSize=20, 分页) ──────────────────────────┐    │
│  │ Step Key       │ Status │ Result        │ Duration │ ▾     │    │
│  │ hour=..T14     │ ✅     │ 200 rows/35MB │ 3.1s     │ [▾]   │    │
│  │ hour=..T15     │ 🔄     │ -             │ -        │       │    │
│  └──────────────────────────────────────────────────────────────┘    │
│         ▼ 点击 [▾] 展开该 step 的 logs (Q2 按 step 关联)             │
│  ┌─ Logs for hour=..T14 ───────────────────────────────────────┐    │
│  │ [info] 14:00:05 step started                                 │    │
│  │ [info] 14:00:08 queried 200 rows                             │    │
│  │ [info] 14:00:09 parquet written 35MB                         │    │
│  │ [info] 14:00:09 step completed                               │    │
│  └──────────────────────────────────────────────────────────────┘    │
│                                                                      │
│  Trigger 按钮 disabled 当 archive_enabled=false (Q4 联动)            │
│  step completed + result.rows_archived=0 → 灰色 "no-op" (Q3 矛盾)    │
└─────────────────────────────────────────────────────────────────────┘
```

交互流程：
1. 访问 /dash/jobs → 加载 Overview + Recent Jobs（分页，URL 带 ?page=N）
2. 点 job 行 → navigate('/dash/jobs/{id}')，URL 更新，可刷新/分享/后退
3. 详情页隐藏外层 Tab（Q7 去冗余），running 时每 10s 轮询
4. 点 step [▾] → 就地展开该 step 的 logs（Q2 按 step_key 过滤）
5. Trigger 按钮（Q5 同行）→ 弹 Dialog 选日期 → POST → 跳新 job 详情
6. Archive Disabled 时 Trigger 禁用 + tooltip（Q4 联动）

关键交互点（对应 8 问题）：Q8 路由化、Q5 Trigger 同行、Q6 列表分页、Q7 详情去冗余、Q2 Logs 按 step、Q3 矛盾检测、Q4 Disabled 联动、Q1 displayStatus 兜底。

## TDD 红绿流程（核心）

本 Stage 强制走严格 TDD 红绿循环：先写失败测试（跑红）→ 重构实现至测试通过（转绿）→ 发现错误及时修复并重跑。Red 阶段所有 BDD 场景使用 Playwright mock API（不依赖真实后端），可多 subagent 并发跑 3 viewports。

### Red 阶段（先写失败测试，Playwright BDD mock API）

在 `crates/aigw-frontend/tests/features/jobs.feature` 补齐如下 11 个场景，先跑红确认当前实现失败：

1. **BDD 路由直达**：访问 `/dash/jobs/{id}` 直达详情页
   - 当前失败：无子路由，回 overview
2. **BDD 详情页刷新**：详情页刷新后仍显示同一 job
   - 当前失败：state 丢失
3. **BDD 浏览器后退**：从详情回到列表
   - 当前失败：无 history
4. **BDD 列表分页**：列表 >50 条显示分页控件 + 翻页请求带 page 参数
   - 当前失败：写死 limit=50 无分页
5. **BDD Tab 标签**：Tab 标签显示 "Body Archive" 不含下划线
   - 当前失败：硬编码 `body_archive`
6. **BDD Archive Disabled**：Archive Disabled 时 Trigger 按钮 disabled
   - 当前失败：纯展示可点
7. **BDD 矛盾检测**：step completed + `result.rows_archived=0` 显示灰色 no-op badge
   - 当前失败：无 result 列
8. **BDD Logs 按 step**：Logs 表显示 Step Key 列 + 按 step 折叠
   - 当前失败：无 Step Key 列
9. **BDD Manual Trigger 同行**：Manual Trigger 按钮与 `TabsList` 同行
   - 当前失败：独占 Card
10. **BDD 详情页去冗余**：详情页不显示外层 `TabsList`
    - 当前失败：冗余显示
11. **BDD 键盘导航**：job 行键盘 Enter/Space 可触发详情
    - 当前失败：无 tabIndex/onKeyDown

执行：`task fe-bdd -- --grep @jobs` 应全红（需先在另一终端 `task dev` 启 server；fe-bdd 内部即 npx playwright test）。

### Green 阶段（实现至测试通过）

逐条对应目标 8 条展开，最小化实现使测试转绿：

- 拆分为 `jobs.tsx` 主文件 + `jobs/job-list.tsx` + `jobs/job-detail.tsx` + `lib/api/jobs.ts`
- `App.tsx` 加 `/dash/jobs/:jobId` 子路由
- `jobs.tsx` tab/selectedJob 改 `useSearchParams`
- `STEP_LABELS` + fallback `replace(/_/g," ")`
- Manual Trigger 按钮挪到 `TabsList` 右侧 flex 同行
- Trigger 按钮 `disabled={archiveStats && !archiveStats.archive_enabled}`
- `<Table>` + `<Pagination>` + 后端 list response 加 `total`
- 详情页独立路由 + 隐藏外层 `TabsList` + Steps 分页 + Payload/Result/Duration 列
- Logs 表加 Step Key 列 + 按 step 折叠面板
- `displayStatus` 矛盾检测 + 灰色 no-op badge + 错误 toast + a11y `aria-label` + tabIndex/onKeyDown

执行：`task fe-bdd` 应全绿；发现的错误及时修复并重跑。

## BDD + real BDD 验证

### BDD（Playwright mock API，3 viewports）

- 3 viewports：desktop / tablet / mobile
- 上述 11 个场景全绿
- 多 subagent 并发跑（前端 BDD 不需后端，mock API 即可）

### real BDD（AIGW_REAL_API=1，对真实后端）

- 验证分页请求带 page 参数
- 验证 trigger disabled 返回 409 前端正确展示
- 验证冷数据回源 body 显示

### 实际执行

- `task fe-bdd`（playwright-bdd）全绿（3 viewports）
- `task fe-lint` 无错误
- 发现的错误及时修复并重跑至全绿
- a11y 验证：键盘导航 + axe-core 无 critical 违规

## 验收标准

- [ ] Red → Green 全绿（11 个 BDD 场景）
- [ ] 3 viewports BDD 通过（desktop / tablet / mobile）
- [ ] real BDD 通过（分页 / 409 / 冷数据回源）
- [ ] a11y：键盘导航 + axe-core 无 critical 违规
- [ ] 发现的错误已及时修复并重跑通过
- [ ] 8 个用户反馈问题（Q1-Q8）逐条解决

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/App.tsx` | 加 `/dash/jobs/:jobId` 子路由 |
| `crates/aigw-frontend/src/pages/jobs.tsx` | 拆分：路由化 + `useSearchParams` + 美化 + 兜底 |
| `crates/aigw-frontend/src/pages/jobs/job-list.tsx` | 新增：通用 Table + Pagination `JobList` |
| `crates/aigw-frontend/src/pages/jobs/job-detail.tsx` | 新增：独立详情页（去冗余 tab + 分页 Steps + Logs 按 step）|
| `crates/aigw-frontend/src/lib/api/jobs.ts` | 新增：jobs API client（含分页）|
| `crates/aigw-server/src/routes/jobs.rs` | list response 加 `total` 字段（`jobs.rs:169-173`）|
| `crates/aigw-frontend/tests/features/jobs.feature` | 补 11 个场景对应 BDD |

## 测试要求

- **BDD（mock API）**：11 个场景 × 3 viewports，全绿
- **real BDD**：分页 / 409 / 冷数据回源，全绿
- **a11y**：键盘导航 + axe-core 无 critical
- **TDD**：Red 先写失败测试 → Green 实现至通过 → 错误及时修复重跑

## 页面布局（body_archive Sub-Tab，Green 阶段后）

```
┌─────────────────────────────────────────────────────────────┐
│  Jobs                                          [Settings ←] │
├─────────────────────────────────────────────────────────────┤
│  [Body Archive] [Budget Reset]        [Trigger Archive ↗]  │
│  ─────────────────────────────────────────────────────────  │
│                                                             │
│  ┌─ Body Archive · manual · 2026-07-24 17:00 ─────────────┐ │
│  │  ● Enabled    Last: 2026-07-24 17:00                   │ │
│  │  450K rows / 75 GB    120 GB freed    800 pending       │ │
│  │  Engine: 6 loops (3 replicas × 2)                       │ │
│  │  Queue: 3 pending · 2 running · 0 stale                 │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Job History ──────────────────────────────────────────┐ │
│  │  ┌────────────────────────────────────────────────────┐ │ │
│  │  │ ID                   │ Trigger  │ Status │ Prog   │ │ │
│  │  │ archive-20260724-.. │ manual   │ 🟢     │ 8/24   │ │ │
│  │  │ cron-body_archive-.. │ cron    │ ✅     │ 1/1    │ │ │
│  │  └────────────────────────────────────────────────────┘ │ │
│  │  ◀ 1 2 3 ▶  (page=2, total=124)                        │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

详情页 `/dash/jobs/{jobId}` 独立路由，无外层 TabsList，Steps 表加 Payload/Result/Duration 列并分页，Logs 按 step_key 折叠分组。

## 依赖

- Stage 82（后端配置失联 + 状态机已修复，summary.running + result 字段可用）

## 不做

- 非 admin 用户视图
- 存储文件浏览器
- Parquet 在线预览
- Job 取消操作
- Dashboard Job 概览 widget
