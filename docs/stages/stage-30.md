# Stage 30: Dashboard 数据接入 + 移动端图表

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P2
**前置条件**: Stage 25（BDD）+ Stage 27（移动端）+ Stage 28（Key UX）
**预估**: 3-4h

---

## 1. 目标

将 Dashboard 从静态占位数据改为接入真实 `/spend/*` 和 `/global/spend/*` API 数据，同时确保图表在移动端可读。

---

## 2. 现状

当前 `src/pages/dashboard/index.tsx` (424 行) 已有基础支出卡片和图表结构，但数据来源需要验证和加强：

- 支出概览卡片：已接入部分 `/spend/*` 端点
- 图表：Recharts `ResponsiveContainer` 已使用
- 日期筛选：已实现

**需要改进：**
1. 数据加载状态（skeleton）、空状态（empty message）、错误状态（error + retry）
2. 图表在移动端高度/间距调整
3. Spend Logs 表格在移动端改为卡片
4. 数据轮询或手动刷新

---

## 3. 交付

### 3.1 数据接入完整性

| 数据区 | 当前状态 | 目标 |
|--------|---------|------|
| 总支出卡片 | `/global/spend` | ✓ 已有，确认 loading/error 状态 |
| 周期支出卡片 | `/spend/tags` | 同上 |
| 活跃 Keys 数 | `/global/spend/keys` | 接入 |
| 按模型统计 | `/spend/models` | ✓ 已有 |
| 按 Provider 统计 | `/spend/providers` | 接入 |
| 最新消费日志 | `/spend/logs` | ✓ 已有 |

### 3.2 状态覆盖

每个数据区实现 4 种状态：
```
loading  → Skeleton 占位
empty    → "No data" 提示 + 引导
error    → 错误信息 + Retry 按钮
data     → 图表/卡片
```

使用 React Suspense 或 TanStack Query 的 `isLoading`/`isError`/`isEmpty`。

### 3.3 移动端图表调整

```tsx
// Dashboard 响应式 grid
<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
  <SpendOverTimeChart />
  <SpendByModelChart />
  <SpendByProviderChart />
  <RecentSpendLogs />
</div>

// Chart wrapper
function ChartCard({ title, children }: Props) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm md:text-base">{title}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="h-[200px] md:h-[300px]">
          {children}
        </div>
      </CardContent>
    </Card>
  );
}
```

### 3.4 Spend Logs 移动端卡片

```
桌面端: 5 列表格（Time, Model, Key, Tokens, Cost）
手机端: 每行 → Card
  ┌────────────────────┐
  │ Model: gpt-4       │
  │ Time: 07-08 14:30  │
  │ Tokens: 1,234      │
  │ Cost: $0.42        │
  └────────────────────┘
```

---

## 4. 门禁

- [ ] 所有数据区接入真实 API
- [ ] Loading / Empty / Error 状态全部覆盖
- [ ] [R-G-R] dashboard.feature 包含 loading 和 error 场景
- [ ] 移动端图表可读（图表高度 ≥ 200px，不挤压）
- [ ] Spend Logs 移动端卡片布局正常
- [ ] `task fe-bdd` 所有 scenario 通过
