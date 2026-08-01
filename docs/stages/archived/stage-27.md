# Stage 27: 移动端适配

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P1
**前置条件**: Stage 25（前端 BDD 基础设施）
**预估**: 3-4h

---

## 1. 目标

全页面响应式改造，确保 Dashboard、Keys、Models、Login 以及后续用户管理页面在 375px ~ 1280px 范围内可用。不是"打磨"，是功能交付标准。

---

## 2. 设计决策

### 2.1 改造策略

原则：**移动端用卡片替代表格，用堆叠替代并排**。不改后端，只改前端布局。

| 页面 | 桌面端 (> 1024px) | 平板 (768-1024px) | 手机 (< 768px) |
|------|-------------------|-------------------|-----------------|
| Dashboard | 2x2 图表网格 | 1x 堆叠图表 | 单列堆叠 |
| Keys 列表 | 表格 | 表格（横向滚动） | 卡片列表 |
| Models 列表 | 表格 | 表格（横向滚动） | 卡片列表 |
| Login | Card max-w-sm | Card max-w-sm | Card w-full |
| 表单弹窗 | Dialog 居中 | Dialog max-h-[90vh] | 全屏 Dialog (Sheet 风格) |

### 2.2 组件级别的改造

**DataTable → ResponsiveCardList：**
```
桌面: <table> 完整 8 列
 └─ 手机: 每个 row → 一张 <Card>，关键字段以 label/value 对展示
```

**Dialog 全屏模式：**
```
桌面: max-w-lg, 圆角, 居中
 └─ 手机: w-full h-full, m-0, rounded-none（从底部滑出效果）
```

**Recharts 响应式：**
```tsx
<ResponsiveContainer width="100%" height={isMobile ? 200 : 300}>
  <BarChart data={data} margin={isMobile ? { left: 0, right: 0 } : { left: 16, right: 16 }}>
    ...
  </BarChart>
</ResponsiveContainer>
```

### 2.3 断点定义

```
sm:  640px  (手机横屏)
md:  768px  (平板)
lg:  1024px (小桌面)
xl:  1280px (大桌面)
```

使用 Tailwind 响应式前缀（`sm:`, `md:`, `lg:`），不引入额外断点库。

---

## 3. 交付

### 3.1 文件修改

| 文件 | 改动 |
|------|------|
| `src/pages/keys/index.tsx` | 表格行 → `<Card>` 移动端布局；Dialog fullscreen |
| `src/pages/models/index.tsx` | 同上，可展开行在移动端的展示 |
| `src/pages/dashboard/index.tsx` | 图表 grid 响应式；卡片堆叠；日期筛选器换行 |
| `src/pages/login.tsx` | Card `mx-4` + `w-full` |
| `src/components/layout/shell.tsx` | Shell 内容区 padding 调整 |
| `src/components/ui/data-table.tsx` | [NEW] 响应式 Table/Card 切换组件 |

### 3.2 ResponsiveCardList 组件设计

```tsx
interface ResponsiveCardListProps<T> {
  data: T[];
  columns: ColumnDef<T>[];
  renderCard: (item: T) => React.ReactNode;  // 移动端卡片渲染
  isLoading?: boolean;
  emptyMessage?: string;
}

// 使用:
// <div className="hidden md:block"><DataTable ... /></div>
// <div className="md:hidden"><CardList ... /></div>
```

### 3.3 各页面移动端布局规格

**Keys 页面 (移动端):**
```
┌────────────────────┐
│ 搜索框              │
├────────────────────┤
│ ┌────────────────┐ │
│ │ Key Name: prod │ │
│ │ Alias: gpt-key │ │
│ │ Models: gpt-4  │ │
│ │ Budget: $100   │ │
│ │ Status: Active │ │
│ │ [Edit] [Delete]│ │
│ └────────────────┘ │
│ ┌────────────────┐ │
│ │ ...             │ │
│ └────────────────┘ │
└────────────────────┘
```

**Dashboard (移动端):**
```
┌────────────────────┐
│ [Date Range Picker] │
├────────────────────┤
│ ┌──────┐ ┌──────┐  │
│ │Spend │ │Keys  │  │
│ │$42.50│ │  12  │  │
│ └──────┘ └──────┘  │
│ ┌─────────────────┐ │
│ │ Spend Over Time │ │
│ │   (chart 200h)  │ │
│ └─────────────────┘ │
│ ┌─────────────────┐ │
│ │ Spend by Model  │ │
│ │   (chart 200h)  │ │
│ └─────────────────┘ │
│ ┌─────────────────┐ │
│ │ Recent Logs     │ │
│ │   (table/cards) │ │
│ └─────────────────┘ │
└────────────────────┘
```

---

## 4. 门禁

- [ ] [R-G-R] mobile.feature BDD 场景全部通过（375px viewport）
- [ ] 每个页面在 375px / 768px / 1280px 下可正常浏览和操作
- [ ] 所有弹窗/对话框在移动端可正常使用（不超出屏幕）
- [ ] 图表在移动端可读（不挤压）
- [ ] 侧边栏滑出/关闭手感流畅
- [ ] text truncation 无溢出（长 key name、长 model name 等）
