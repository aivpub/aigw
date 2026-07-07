# Stage 23: 用量 Dashboard

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P1
**前置条件**: Stage 21 完成
**预估**: 4-6h

---

## 1. 目标

实现用量 Dashboard：总 spend 卡片、按 model/provider 聚合图表、spend logs 表格。

---

## 2. 交付

### 2.1 Dashboard 布局

```
┌────────────────────────────────────────────┐
│  📊 本月支出          📊 总计支出           │
│  ¥ 1,234.56          ¥ 45,678.90          │
├────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐       │
│  │  按 Model     │  │  按 Provider  │       │
│  │  BarChart    │  │  DonutChart  │       │
│  └──────────────┘  └──────────────┘       │
├────────────────────────────────────────────┤
│  日期范围: [DatePicker] ~ [DatePicker]     │
│  ┌──────────────────────────────────────┐  │
│  │ Spend Logs Table                     │  │
│  │ 时间 │ Key │ Model │ Tokens │ Cost   │  │
│  │ ...  │ ... │ ...   │ ...    │ ...    │  │
│  └──────────────────────────────────────┘  │
└────────────────────────────────────────────┘
```

### 2.2 功能

| 功能 | 说明 |
|------|------|
| 支出卡片 | 本月/总计 spend 统计，通过 `/spend/models` + `/global/spend/models` 聚合 |
| Model 柱状图 | 按 model 分组用量（BarChart），可堆叠显示 token 数/cost |
| Provider 环形图 | 按 provider 消费占比（DonutChart） |
| Spend Logs 表格 | TanStack Table，支持按时间、key、model 排序，分页加载 |
| 日期筛选 | `react-day-picker` + Calendar Popover，start_date/end_date 过滤 |

### 2.3 UI 组件

- `Card` — 统计卡片
- `ChartContainer` + `BarChart` + `DonutChart` — shadcn/ui chart（Recharts）
- `DataTable` — spend logs 表格
- `Calendar` + `Popover` — 日期选择器
- `Skeleton` — 加载占位

### 2.4 API 对接

- `GET /spend/models` — 按 model 聚合
- `GET /spend/providers` — 按 provider 聚合
- `GET /spend/logs?start_date=...&end_date=...&model=...` — 日志查询
- `GET /global/spend/models` — 管理员全局聚合

### 2.5 路由

`/admin/dashboard` — Dashboard 主页（默认首页）

---

## 3. 门禁

- 卡片正确显示 spend 数据
- 图表渲染正常（BarChart 和 DonutChart）
- Spend logs 表格支持排序和分页
- 日期筛选后图表和表格同步刷新
- 无数据时显示空状态提示
