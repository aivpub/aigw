# Phase 15: 第二轮用户反馈改进 — Models / Playground / Spend Logs / Migrate

> **注意**: 本文档原为 Phase 14。2026-07-11 重编号：Phase 14 改为 `/v1/messages` 修复（最高优先级），原 Phase 14 内容移入 Phase 15。
>
> **背景**: Phase 13 完成后基于用户使用反馈提出的新一轮改进需求。

**日期**: 2026-07-10（规划）/ 2026-07-11（重编号）
**Phase**: 15（原 Phase 14）
**触发**: 用户使用反馈（4 个改进领域）

---

## 反馈分析 → 差距评估

### 1. Models 页面 — 成本列缺失

| 现状 | 目标 | 差距 |
|------|------|------|
| 展示 Provider、Model Type、Status、Active | 增加一列 Cost，上排 Input Cost、下排 Output Cost（Per Million Tokens） | 前端 ModelItem 未读取 `model_info.input_cost_per_token` / `output_cost_per_token` |

**设计**: 表格新增 Cost 列，渲染为两行紧凑文本：
```
$0.xxxx (Input)
$0.xxxx (Output)
```
单位为 Per 1M Tokens（`x 1,000,000` 格式化），方便与模型官方定价对比。若模型无定价数据则显示 "—"。

---

### 2. Playground 页面 — Markdown + 气泡边框 + 富统计信息

| 现状 | 目标 | 差距 |
|------|------|------|
| 简单 `whitespace-pre-wrap` 文本渲染 | **Markdown 渲染（含 streaming 增量）** — 代码块、粗体、列表等 | 需 Markdown 解析器（可选 `react-markdown` + `remark-gfm`） |
| 无气泡边框 | 消息气泡带边框效果，边框底部显示统计信息（token 费用、消耗量）和操作按钮 | 需重构气泡底部栏（`rounded-lg border` + 底部小字 + 按钮） |
| 无流式 Markdown 增量渲染 | streaming 模式下 Markdown 随 chunk 到达逐段更新 | 流式拼接 raw text，实时传给 Markdown 渲染器 |

**消息气泡设计**:
```
┌─────────────────────────────────────┐
│ [role icon] 角色标签                │
│                                     │
│ ## Markdown 渲染内容                 │
│ 带代码高亮、粗体等                   │
│                                     │
│ ─────────────────────────────────── │
│ 1,234 tokens  │  $0.042  │  📋 复制 │  ← 底部状态栏
│ 重新生成  ❌  删除                   │
└─────────────────────────────────────┘
```

**功能清单**:
- 气泡边框（不同 role 不同颜色：system 紫色、user 蓝色、assistant 绿色）
- 底部栏：Token 计数（`prompt / completion`）、费用（`spend`）、复制内容按钮
- 重新生成按钮（对该消息重新发送）
- streaming 增量 Markdown 渲染
- 代码块语法高亮（`react-syntax-highlighter` 等）

---

### 3. Spend Logs 页面 — 详情抽屉增强 + 导出 + 布局优化

| 现状 | 目标 | 差距 |
|------|------|------|
| 抽屉仅展示 Basic Info、Tokens、Timestamps、API Key | **全部可用字段**：提示词内容、响应内容（Markdown/JSON 渲染）、请求参数（model/temperature/max_tokens 等）、模型信息（model_id/model_group/pricing）、时间戳、tools/functions 等 | 后端未返回 `messages` 和 `response` blob 字段；抽屉需 Markdown/JSON 视图 |
| 无导出功能 | CSV / Excel 导出按钮 | 前端 fetch data → 生成 CSV/Excel Blob → 触发下载 |
| Request ID / Model 输入框较宽 | 缩短与 Time Range 行对齐 | CSS 调整 |

**详情抽屉新增内容**:
```
┌─ Request Details ──────────────────────┐
│ Basic Info: Request ID / Status / Type  │
│ Model: model_name (model_group)         │
│ Cost: $x.xxxx (input $x / output $x)    │
│ Tokens: prompt 1,234 / completion 567   │
│ Timing: TTFT 234ms / Duration 1.2s      │
│ Timestamps: start → end → completion    │
│ ─────────────────────────────────────── │
│ 📝 Messages (Request)                   │
│ ┌ role: system ───────────────────────┐ │
│ │ You are a helpful assistant.        │ │
│ └─────────────────────────────────────┘ │
│ ┌ role: user ─────────────────────────┐ │
│ │ What is Rust?                       │ │
│ └─────────────────────────────────────┘ │
│ ─────────────────────────────────────── │
│ 🤖 Response                            │
│ ┌─────────────────────────────────────┐ │
│ │ Rust is a systems programming       │ │
│ │ language ...                        │ │
│ └─────────────────────────────────────┘ │
│ ─────────────────────────────────────── │
│ 🔧 Request Parameters                  │
│ model: gpt-4, temperature: 0.7,        │
│ max_tokens: 4096, stream: true          │
│ ─────────────────────────────────────── │
│ 🏷️ Metadata                            │
│ API Key: sk-xxx, User: xxx, Team: xxx  │
│ Tags: [...]  │  Session: xxx           │
│ Tools: [...]  │  Tool Calls: [...]      │
└────────────────────────────────────────┘
```

**导出功能设计**:
- 按钮位置：Toolbar（与 Request ID 搜索 + Fetch 同一行）
- 格式：CSV（默认）、Excel（可选，使用 `xlsx` 库）
- 导出范围：当前筛选条件对应的全部数据（不受分页限制，或可选 max 条数 = total_count）
- 文件名：`spend-logs-{startDate}-{endDate}.csv`
- 实现：后端新增 `GET /global/spend/logs?format=csv&limit=N` 端点（推荐），或前端 `fetch all pages → CSV on client`

**布局优化**:
- Request ID 搜索框宽度：`w-36` → `w-44`（与 Model filter 同宽）
- Time Range 行与 filter 行间距减小，紧凑布局

---

### 4. aigw-migrate — 选择性跳过字段

| 现状 | 目标 | 差距 |
|------|------|------|
| 全量迁移所有例 | 可通过 CLI 参数指定跳过的列，如 `--skip-columns messages,response` 跳过 spend_logs 表中的 body 字段 | 需新增列层面过滤逻辑 |

**设计**:
```
aigw-migrate remote-import \
  --skip-columns spend_logs.messages,spend_logs.response \
  --source-url postgres://...
```

或更简洁的用法：
```
aigw-migrate remote-import \
  --skip-body   # 预设：跳过 messages, response（常用场景）  
  --skip-columns spend_logs.metadata   # 跳过指定列
```

**实现要点**:
| 参数 | 说明 |
|------|------|
| `--skip-columns <table.col,...>` | 逗号分隔的表名.列名，跳过指定列的迁移 |
| `--skip-body` | 快捷预设 = `--skip-columns spend_logs.messages,spend_logs.response` |
| 缺失列处理 | 跳过写入目标列（INSERT 时排除该列，或写 NULL/DEFAULT） |
| 验证 | 迁移完成后输出 "N columns skipped (messages, response)" |

---

## Phase 15 Stage 规划（重编号后）

> 2026-07-11 重编号：原 Phase 14 → Phase 15，Stage 40-43 → Stage 44-46（Models/Spend/Migrate）
> Stage 41 Playground Markdown 移入 Phase 16 Stage 49。

### 依赖图

```
Stage 44 (Models Cost 列) — 独立前端
Stage 45 (Spend Logs 抽屉增强 + 导出 + 布局) — 前后端
Stage 46 (aigw-migrate --skip-columns) — 独立后端
```

全部独立，可并行开发。

| Stage | 目标 | 类型 | 预估 | 优先级 | 依赖 |
|-------|------|------|------|--------|------|
| Stage 44 | Models 页面 Cost 列（Input/Output Per M） | 前端 | 2h | P1 | — |
| Stage 45 | Spend Logs 抽屉完整内容 + 导出 + 布局调整 | 前后端 | 5h | P0 | — |
| Stage 46 | aigw-migrate --skip-columns / --skip-body 选择性迁移 | 后端 | 3h | P1 | — |

**总预估**: 15h（约 2 个工作日）

---

## 验证标准

| 编号 | 标准 | 验证方式 |
|------|------|---------|
| V1 | Models 页面展示 Cost 列（Input / Output Per M） | BDD |
| V2 | 定价缺失时显示 "—" | BDD |
| V3 | Playground 响应支持 Markdown 渲染（代码块、粗体、列表） | BDD |
| V4 | Playground streaming 模式下 Markdown 增量渲染 | BDD |
| V5 | 消息气泡带边框 + 底部统计栏（tokens/cost/copy） | BDD |
| V6 | Spend Logs 抽屉展示 messages（提示词）和 response（响应内容） | BDD |
| V7 | Spend Logs 抽屉展示 request parameters 和 metadata | BDD |
| V8 | CSV 导出按钮 → 下载文件含正确列和数据 | BDD |
| V9 | Request ID / Model 输入框与 Time Range 行对齐 | 手动 |
| V10 | aigw-migrate --skip-body 跳过 messages, response 列 | 单元测试 |
| V11 | aigw-migrate --skip-columns table.col 正确跳过指定列 | 单元测试 |
