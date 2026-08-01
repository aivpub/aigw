# Stage 71: Usage 页面图表增强 — Daily 堆叠 + Top Keys/Models 排行榜

**Phase**: 27 — 全栈质量修复 + Usage 页面图表增强
**状态**: ✅ 完成
**预估**: 8h
**完成日期**: 2026-07-22
**依赖**: Stage 69 ✅ (提供 /activity 扩展字段 + /keys/rankings 端点)

---

## 目标

对比 litellm 的 Global Usage：
1. Daily Trend token 区分 prompt vs completion（堆叠 bar）
2. Daily Trend requests 区分 success vs failed（堆叠 bar）
3. 新增 Top Virtual Keys 排行榜
4. Top Models 新增排行榜视图 + Provider 图表独立 Tab 状态

---

## Part A — Daily Trend 堆叠柱状图 (3h)

**`crates/aigw-frontend/src/pages/usage/index.tsx`**

### 1.1 类型扩展

```typescript
interface DailyRow {
  date: string;
  spend: number;
  tokens: number;
  requests: number;
  prompt_tokens: number;       // new
  completion_tokens: number;   // new
  successful_requests: number; // new
  failed_requests: number;     // new
}
```

### 1.2 Tokens 模式 — 堆叠 bar

当 `dailyChartMode === "tokens"`:

```tsx
<Bar dataKey="prompt_tokens" name="Prompt" fill="#94a3b8" stackId="tokens" radius={[0,0,0,0]} />
<Bar dataKey="completion_tokens" name="Completion" fill="#3b82f6" stackId="tokens" radius={[4,4,0,0]} />
```

### 1.3 Requests 模式 — 堆叠 bar

当 `dailyChartMode === "requests"`:

```tsx
<Bar dataKey="successful_requests" name="Success" fill="#22c55e" stackId="requests" radius={[0,0,0,0]} />
<Bar dataKey="failed_requests" name="Failed" fill="#ef4444" stackId="requests" radius={[4,4,0,0]} />
```

### 1.4 Spend 模式

保持不变，单 bar

### 1.5 Tooltip 增强

**分类显示 + 总数**。Tooltip 始终在顶部显示当日总数，然后按分类拆解：

**Spend 模式**:
```
2026-07-20
  Total: $1.2345
```

**Tokens 模式** — 堆叠 bar，tooltip 显示分类 + 总数:
```
2026-07-20
  Prompt:      5.1K
  Completion:  3.1K
  ─────────────
  Total:       8.2K tokens
```

**Requests 模式** — 堆叠 bar，tooltip 显示分类 + 总数:
```
2026-07-20
  Success:     42
  Failed:      3
  ─────────────
  Total:       45 requests
```

实现:
```tsx
<Tooltip
  formatter={(value, name) => {
    if (chartMode === "tokens") return [fmtTokens(value), name];
    if (chartMode === "requests") return [value, name];
    return [fmtSpend(value), "Spend"];
  }}
  labelFormatter={(label) => {
    const item = dailyChartData.find(d => d.date === label);
    if (!item) return label;
    if (chartMode === "tokens") {
      return `${label}\n  Prompt: ${fmtTokens(item.prompt_tokens)}  |  Completion: ${fmtTokens(item.completion_tokens)}\n  Total: ${fmtTokens(item.tokens)}`;
    }
    if (chartMode === "requests") {
      return `${label}\n  Success: ${item.successful_requests}  |  Failed: ${item.failed_requests}\n  Total: ${item.requests}`;
    }
    return `${label}  |  ${fmtSpend(item.spend)}`;
  }}
/>
```

> Recharts Tooltip 的 `labelFormatter` 返回字符串，用 `\n` 换行即可实现多行显示。

**排行榜 tooltip**（Top Keys / Top Models）同样显示分类 + 总数:
- hover 某行显示: key_alias + 对应指标值 (spend/tokens/requests) + 占比百分比

---

## Part B — Top Virtual Keys 排行榜 (2.5h)

### 2.1 API 调用

```typescript
const { data: keyRankingsData } = useQuery<KeyRankingResponse>({
  queryKey: ["key-rankings", startDate, endDate],
  queryFn: () => apiGet(`/global/spend/keys/rankings?start_date=${encodeURIComponent(startDate)}&end_date=${encodeURIComponent(endDate)}&limit=10`),
  refetchInterval: 30_000,
});
```

### 2.2 UI

在 Daily Trend 卡片下方新增 "Top Virtual Keys" 卡片：

```
┌──────────────────────────────────────────┐
│ Top Virtual Keys           [💰 📊 📋]     │
│                                          │
│ #1  my-key          ████████  $12.3456  │
│ #2  production      ████      $8.9012   │
│ #3  dev-test        ██        $3.4567   │
│ ...                                      │
└──────────────────────────────────────────┘
```

- 排名序号 + key_alias（优先）或 api_key 截断
- 迷你进度条（宽度 = value / max × 100%）
- Tab 切换 spend / tokens / requests

### 2.3 空状态 + 加载态

- 无数据: "No data available"
- Skeleton 占位

---

## Part C — Top Models 排行榜 + 图表 Tab 状态独立 (2.5h)

### 3.1 "Spend by Model" 卡片双模式

Chart / Rank Tab 切换:

```tsx
<Tabs value={modelViewMode} onValueChange={setModelViewMode}>
  <TabsTrigger value="chart">📊 Chart</TabsTrigger>
  <TabsTrigger value="ranking">📋 Rank</TabsTrigger>
</Tabs>
```

Ranking 视图: 复用 Top Keys 的排名列表模式，数据来源 `modelChartData`，按 spend/tokens/requests 排序

### 3.2 图表 Tab 状态独立化

当前问题: Daily Trend、Model、Provider 三个图表共享一个 `chartMode` 状态，切换一处影响所有。

修正:
```typescript
const [dailyChartMode, setDailyChartMode] = useState<ChartMode>("spend");
const [providerChartMode, setProviderChartMode] = useState<ChartMode>("spend");
// modelViewMode 控制 Chart/Rank 切换
```

### 3.3 布局调整

新布局:
```
Metric Cards (6 卡片)
Daily Trend (全宽)
Top Virtual Keys (全宽)
Spend by Model + Spend by Provider (lg:grid-cols-2)
```

---

## 测试 (5 BDD × 3 viewports = 15 tests)

| # | 场景 |
|---|------|
| 1 | Daily Trend tokens — prompt + completion 堆叠 bar 分层可见 |
| 2 | Daily Trend requests — success + failed 堆叠 bar 分层可见 |
| 3 | Top Virtual Keys — 排名列表按 spend DESC 显示 |
| 4 | Top Virtual Keys Tab 切换 — 点击 tokens/requests tab 指标切换 |
| 5 | Top Models — 切换到 Rank Tab 看到排名列表 |

---

## 门禁

- [ ] `npm run build` 通过
- [ ] 前端 BDD: 129 → 144 tests
- [ ] Daily Trend: tokens 堆叠 prompt/completion, requests 堆叠 success/failed
- [ ] Top Virtual Keys: 排行榜按 spend DESC, tabs 可切换
- [ ] Top Models: Chart/Rank Tab 切换正常
- [ ] 各图表 Tab 状态独立，互不影响
- [ ] 响应式: 3 种 viewport 验收通过
