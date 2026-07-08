# Stage 32: Spend Logs 独立页面

**Phase**: 12 — 前端导航重构 + Playground
**状态**: ⏳ 待开始
**预估**: 1.5h

---

## 目标

从 Usage Dashboard 拆分 Spend Logs 到独立页面 `/dash/spend-logs`，支持日期筛选、模型过滤、分页。

## 验收标准

- [ ] `/dash/spend-logs` 页面加载成功
- [ ] 日期范围筛选（start_date / end_date），默认当前月
- [ ] Model 过滤（可选下拉）
- [ ] 分页或加载更多
- [ ] 表格列: Time, Model, Tokens, Cost, Status
- [ ] 30s 自动刷新
- [ ] 移动端 card list 布局
- [ ] Loading / Empty / Error 三态覆盖
- [ ] BDD: page load, date filter, model filter, mobile view

## 关键文件

| 文件 | 操作 |
|------|------|
| `src/pages/spend-logs/index.tsx` | 新建：独立 spend logs 页面 |
| `src/App.tsx` | 新增路由 |

## API

```
GET /global/spend/logs?start_date=X&end_date=Y&limit=100&model=gpt-4
```

## 组件状态

| 状态 | 展示 |
|------|------|
| Loading | Skeleton 行 |
| Empty | "No spend logs found" + 调整日期范围提示 |
| Error | "Failed to load spend logs" + Retry 按钮 |
| Data | 表格或移动端 card list |

## 依赖

- Stage 31（路由 + 侧边栏）

## 输出

- [ ] `src/pages/spend-logs/index.tsx`
- [ ] BDD feature + steps
