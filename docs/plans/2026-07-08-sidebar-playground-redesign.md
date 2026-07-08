# Phase 12: 前端导航重构 + Playground 实现计划

> **For Claude:** 使用 superpowers:executing-plans 按任务逐步实施。

**目标:** 对齐 litellm admin UI 侧边栏分组结构，新增 Usage/SpendLogs/Playground 页面

**架构:** 前端 React 路由 + 组件重构，不动后端 API

**技术栈:** React + TypeScript + shadcn/ui + Recharts + react-router-dom v7

---

## Phase 12 路线图

| Stage | 目标 | 预估 |
|-------|------|------|
| Stage 31 | 侧边栏分组重构 + Dashboard→Usage 重命名 + Keys→Virtual Keys | 2h |
| Stage 32 | Spend Logs 独立页面 | 1.5h |
| Stage 33 | Playground Chat 调试页 | 2.5h |

### Stage 31: 侧边栏分组重构 + Usage 重命名

**变更清单:**

1. 侧边栏 (sidebar.tsx): 引入分组结构，每组灰色大写标题 (10px, #6b7280, letter-spacing)，与 litellm leftnav.tsx 风格一致

   ```
   AI GATEWAY
     🔑 Virtual Keys     /dash/keys
     📦 Models            /dash/models
     🎮 Playground        /dash/playground

   OBSERVABILITY
     📈 Usage             /dash/usage
     📋 Spend Logs        /dash/spend-logs

   ACCESS CONTROL
     👤 Users             /dash/users
     👥 Teams             /dash/teams
     🏢 Organizations     /dash/orgs
   ```

2. 路由重构 (App.tsx):
   - `/dash/home` → `/dash/usage`（重命名）
   - `/dash` 默认 redirect → `/dash/usage`
   - 新增 `/dash/spend-logs`、`/dash/playground` 路由占位

3. 页面文件重命名:
   - `pages/dashboard/index.tsx` → `pages/usage/index.tsx`
   - 移除 spend logs 表格，只保留概览卡 + 图表

4. 标签重命名:
   - "Keys" → "Virtual Keys"
   - "Orgs" → "Organizations"

5. BDD 测试更新: 适配新路由名

### Stage 32: Spend Logs 独立页面

**新文件:** `src/pages/spend-logs/index.tsx`

**功能:**
- 日期范围筛选 (start_date / end_date)
- Model 过滤 (可选)
- 分页（limit/offset，简单分页或加载更多）
- 表格列: Time, Model, Tokens, Cost, Status（复用现有 spend logs 表格组件）
- 移动端 card list 布局（复用现有 mobile responsive 模式）
- 30s 自动刷新 (tanstack-query refetchInterval)

**API:** `GET /global/spend/logs?start_date=X&end_date=Y&limit=100`

### Stage 33: Playground Chat 调试页

**新文件:** `src/pages/playground/index.tsx`

**功能:**
- 模型选择 dropdown（从 `/v1/models` 获取）
- System Prompt 输入区（可选，textarea）
- User Message 输入区（textarea）
- 参数控制: Temperature (slider 0-2), Max Tokens (number input)
- Send 按钮 + Response 展示区（支持 streaming / non-streaming toggle）
- 调用 `/v1/chat/completions`（使用 admin session 鉴权）
- loading/error/empty 三态覆盖
- 移动端堆叠布局

**依赖:** 后端 `/v1/chat/completions` 已就绪

---

## 依赖关系

```
Stage 31 (侧边栏重构 + 路由变更)
  ├── Stage 32 (Spend Logs 独立页)
  └── Stage 33 (Playground 页)
```

Stage 31 需先完成（路由+侧边栏基础设施），Stage 32/33 可并行。

## 实施步骤

### Task 1: Stage 31 — 侧边栏分组重构

- [ ] 重命名 `pages/dashboard/` → `pages/usage/`，文件名 `index.tsx`（内容后续瘦身）
- [ ] 更新 `App.tsx` 路由: `/dash/home` → `/dash/usage`, `/dash` redirect → `/dash/usage`
- [ ] 新增 `/dash/spend-logs`、`/dash/playground` 路由占位（返回 "Coming soon" placeholder）
- [ ] 重写 `sidebar.tsx`: 3 分组结构，灰色大写标题
- [ ] 标签: "Keys" → "Virtual Keys", "Orgs" → "Organizations"
- [ ] Usage 页瘦身: 移除 spend logs 表格，只保留概览卡 + 图表
- [ ] 更新 BDD step 中的路由引用
- [ ] 运行 BDD 测试确认

### Task 2: Stage 32 — Spend Logs 独立页

- [ ] 创建 `src/pages/spend-logs/index.tsx`
- [ ] 日期过滤 + 分页 + 表格
- [ ] 移动端 card list
- [ ] 30s 自动刷新
- [ ] Mock empty/loading/error 三态
- [ ] BDD scenarios: spend-logs page load, filter by date, filter by model, mobile view, empty state

### Task 3: Stage 33 — Playground 页

- [ ] 创建 `src/pages/playground/index.tsx`
- [ ] 模型选择 + System/User 消息输入
- [ ] Temperature + Max Tokens 控件
- [ ] Send + Response 展示
- [ ] Streaming toggle
- [ ] 移动端堆叠布局
- [ ] Mock empty/loading/error 三态
- [ ] BDD scenarios: playground page load, send message, streaming response, model select, empty state

---

## 实现完成后的状态

```
侧边栏: 3 组 8 项
路由: /dash/usage, /dash/spend-logs, /dash/playground, /dash/keys, /dash/models, /dash/users, /dash/teams, /dash/orgs
页面: Usage (图表), Spend Logs (新), Playground (新), Virtual Keys, Models, Users, Teams, Organizations
BDD: 新增 6-8 场景 (Playground + Spend Logs)
```
