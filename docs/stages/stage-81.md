# Stage 81: 前端 Jobs 管理页面

**Phase**: 30 — Body Archive 冷存储
**优先级**: P0
**状态**: ⏳ 待开始
**预估**: 10h
**前置**: Stage 80（Admin API 就绪）

---

## 背景

Stage 78-80 完成了 AsyncTask + Engine 框架和 Body Archive 全链路。本阶段在前端 Admin Settings 下新增 "Jobs" 页面——按 step_type 分 Sub-Tab 管理所有异步工作，Archive 是第一个。budget_reset 等新工作类型接入时只需加一个 Sub-Tab。

## 目标

1. Settings → Jobs Tab，按 step_type 分 Sub-Tab
2. Archive Sub-Tab：统计卡片、手动触发、Job 历史、Job 详情
3. 通用 JobList / JobDetail 组件适配所有 step_type
4. **TDD**：4 BDD × 3 viewports

## 验收标准

### 页面框架

- [ ] Settings → Jobs Tab，Sub-Tab 从 `GET /admin/jobs` 动态去重 step_type
- [ ] 默认选中 body_archive Sub-Tab

### Archive — 统计卡片

- [ ] `GET /admin/archive/stats`：archive_enabled、last_archive_at、累计存储/释放
- [ ] `GET /admin/jobs/stats`：loops + pending/running/stale + 24h 完成/失败
- [ ] 30s 自动刷新

### Archive — 手动触发

- [ ] Start/End 日期选择器
- [ ] 预估 Steps 数量展示
- [ ] Trigger 按钮 → `POST /admin/jobs/trigger` → 成功展示 job_id → 跳转详情

### Job 历史（通用组件）

- [ ] 列表：Job ID（截断）、trigger_type、status（颜色标记）、进度、created_at
- [ ] 状态颜色：running=蓝色动画、completed=绿色、failed=红色
- [ ] 分页，默认按 created_at DESC
- [ ] 点击行 → 展开 Job Detail

### Job 详情（通用组件）

- [ ] Summary：total/completed/failed Steps
- [ ] Steps 表格：step_key、status 图标（✅🔄⏳❌）、payload、result 格式化
- [ ] Logs：最新 50 条、level 过滤（info/warn/error）、时间倒序
- [ ] running Job 每 10s 轮询刷新
- [ ] body_archive 的 result 格式化（bytes → MB, ms → s）
- [ ] 其他 step_type 的 result 通用 JSON 展示

### 其他 Sub-Tab

- [ ] 切换 → 查询对应 step_type → 统计卡片 + JobList + JobDetail
- [ ] 暂无 Job 时显示占位信息

## 页面布局（body_archive Sub-Tab）

```
┌─────────────────────────────────────────────────────────────┐
│  Jobs                                          [Settings ←] │
├─────────────────────────────────────────────────────────────┤
│  [Body Archive] [Budget Reset]                              │
│  ─────────────────────────────────────────────────────────  │
│                                                             │
│  ┌─ Body Archive ─────────────────────────────────────────┐ │
│  │  ● Enabled    Last: 2026-07-24 17:00                   │ │
│  │  450K rows / 75 GB    120 GB freed    800 pending       │ │
│  │  ─────────────────────────────────────────────────────── │ │
│  │  Engine: 6 loops (3 replicas × 2)                       │ │
│  │  Queue: 3 pending · 2 running · 0 stale                 │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Manual Trigger ───────────────────────────────────────┐ │
│  │  Start: [2026-07-22]  End: [2026-07-24]               │ │
│  │  Estimated: 48 steps    [Trigger Archive]               │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Job History ──────────────────────────────────────────┐ │
│  │  ┌────────────────────────────────────────────────────┐ │ │
│  │  │ ID                   │ Trigger  │ Status │ Prog   │ │ │
│  │  │ archive-20260724-.. │ manual   │ 🟢     │ 8/24   │ │ │
│  │  │ cron-body_archive-.. │ cron    │ ✅     │ 1/1    │ │ │
│  │  └────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────┘ │
│                                                             │
│  ┌─ Detail: archive-20260724-a1b2c3 ──────────────────────┐ │
│  │  Summary: 24 steps, 8 done, 0 failed                    │ │
│  │  ┌──────────────────────────────────────────────────┐   │ │
│  │  │ Step Key            │ St │ Rows  │ Size  │ Dur   │   │ │
│  │  │ hour=2026-07-22T00 │ ✅ │ 200   │ 35MB  │ 3.1s  │   │ │
│  │  │ hour=2026-07-22T01 │ 🔄 │ -     │ -     │ -     │   │ │
│  │  └──────────────────────────────────────────────────┘   │ │
│  │  Logs [info ▾]:                                          │ │
│  │  [info] 17:30:05 Step hour=2026-07-22T00 completed      │ │
│  └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## 关键文件

| 文件 | 操作 |
|------|------|
| `crates/aigw-frontend/src/pages/settings/jobs/index.tsx` | 新增：Jobs 页面 + Sub-Tab 路由 |
| `crates/aigw-frontend/src/pages/settings/jobs/stats-card.tsx` | 新增：通用统计卡片（loops + queue） |
| `crates/aigw-frontend/src/pages/settings/jobs/archive-tab.tsx` | 新增：Archive Sub-Tab |
| `crates/aigw-frontend/src/pages/settings/jobs/trigger-card.tsx` | 新增：手动触发卡片 |
| `crates/aigw-frontend/src/pages/settings/jobs/job-list.tsx` | 新增：通用 Job 列表 |
| `crates/aigw-frontend/src/pages/settings/jobs/job-detail.tsx` | 新增：通用 Job 详情 |
| `crates/aigw-frontend/src/pages/settings/jobs/placeholder-tab.tsx` | 新增：其他 step_type 占位 |
| `crates/aigw-frontend/src/lib/api/jobs.ts` | 新增：jobs API client |
| `crates/aigw-frontend/tests/features/jobs.feature` | 新增：BDD |

## 自动刷新

Job Detail 在 status=running 时每 10s 轮询。统计卡片每 30s 刷新。

## 测试要求

- **BDD 1**：Settings → Jobs → Archive Tab → 统计卡片 → 格式化数字
- **BDD 2**：日期 → Trigger → job_id → 展开 Detail
- **BDD 3**：Job History → running 行 → Steps 表格/状态图标 → Logs 过滤
- **BDD 4**：切换到 Budget Reset Sub-Tab → 占位 + 统计卡片正确
- 3 viewports：desktop / tablet / mobile

## 依赖

- Stage 80（所有 admin API 就绪）

## 不做

- 非 admin 用户视图
- 存储文件浏览器
- Parquet 在线预览
- Job 取消操作
- Dashboard Job 概览 widget
