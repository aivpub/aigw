# 前端技术选型调研报告

**项目**: aigw (AI Gateway)
**日期**: 2026-07-05
**状态**: 调研完成

---

## 1. 调研目标

为 aigw 前端管理控制台确认技术选型。以 litellm 前端为参考基线，评估 shadcn/ui + Tailwind CSS v4 方案能否满足复刻需求。

---

## 2. LiteLLM 前端工程分析

### 2.1 技术栈总览

| 维度 | 技术 |
|------|------|
| 框架 | Next.js 16.2.6 (App Router, static export) |
| 语言 | TypeScript 5.9.3 |
| 构建 | Turbopack (默认) / Webpack (备选) |
| 样式 | Tailwind CSS v4.3.2 |
| 组件库 | Ant Design 5.29.3 + Radix UI 1.6.1 |
| 图表 | Tremor v3.18.7（基于 D3 自研引擎） |
| 图标 | Lucide React + Heroicons |
| 状态管理 | TanStack React Query 5 |
| 表格 | TanStack React Table 8 |
| 测试 | Vitest + Playwright |
| 部署 | Next.js 静态导出 → Python 后端托管 |

### 2.2 litellm 的三层 UI 架构

litellm 不是单用一套 UI 库，而是三层分工：

```
Tailwind CSS v4         ← 全局样式底层（间距、颜色微调、排版）
├── Ant Design 5        ← 复杂交互组件（表单验证、弹窗、高级表格、通知、日期选择器）
└── Tremor v3           ← 图表 + 仪表盘布局（BarChart、AreaChart、DonutChart、Card、TabGroup）
```

### 2.3 分工明细

| 场景 | 用哪个 | 为什么 |
|------|--------|--------|
| 图表（用量趋势、消费占比、缓存命中率） | Tremor | AntD 没有原生图表组件 |
| 仪表盘布局（卡片、网格、标签页导航） | Tremor | Card、Grid、TabGroup 开箱即用的美观样式 |
| 复杂表单（登录、设置、新建 Key、SSO 配置） | AntD | Form.Item 声明式校验、动态字段，Tremor 只有裸 Input |
| 弹窗/抽屉（编辑表单、删除确认） | AntD | Modal、Drawer，Tremor 没有弹窗系统 |
| 高级表格（审计日志、用户列表、可排序/筛选/分页） | AntD | Table 企业级能力（ColumnsType、Summary、展开行、行选择） |
| Toast 通知（操作成功/失败反馈） | AntD | message.success() / notification API，Tremor 没有 |
| 日期范围选择（用量筛选） | AntD | DatePicker.RangePicker，Tremor 仅有值类型 DateRangePickerValue |
| 间距和颜色微调 | Tailwind | 当两个库的默认间距不够时，用 utility class 兜底（p-8、gap-2、text-green-600） |

### 2.4 litellm 使用的 4 种 Tremor 图表

| Tremor 组件 | litellm 中使用量 | 典型场景 |
|---|---|---|
| BarChart | ~30 处 | 每日消费额、请求量、模型/Key 排名、缓存命中率 |
| AreaChart | ~9 处 | Token 消耗趋势、成功/失败请求、提示缓存指标 |
| LineChart | 少量 | 端点随时间变化的趋势 |
| DonutChart | 少量 | 按 Provider 的消费构成占比 |

### 2.5 litellm 中 Tremor 与 AntD 的实际混用

`settings.tsx` 是典型代表，同时导入两套组件：

```typescript
import { Button, Card, Grid, SelectItem, Switch, Tab, TabGroup, ... } from "@tremor/react";
import { Button as Button2, Form, Input, Modal, Select, Typography } from "antd";
```

- Tremor 的 `TabGroup` 驱动页面导航布局
- AntD 的 `Form`/`Modal` 处理表单交互
- 冲突处理：AntD 的 Button 别名为 `Button2`

---

## 3. shadcn/ui + Tailwind CSS v4 图表能力评估

### 3.1 shadcn/ui 图表的架构

shadcn/ui 不自己实现图表引擎，而是在 Recharts 上做**样式封装**。

```
npm install recharts             ← 真正的图表引擎
npx shadcn@latest add chart     ← shadcn 的包装层（一个 chart.tsx 文件）
```

**chart.tsx 导出 5 个辅助组件**：

| 组件 | 作用 |
|------|------|
| `ChartContainer` | 响应式容器，管理 CSS 变量和 `chartConfig` |
| `ChartTooltip` | Tooltip 触发 |
| `ChartTooltipContent` | Tooltip 内容渲染 |
| `ChartLegend` | 图例 |
| `ChartStyle` | CSS 变量注入（用于服务端样式） |

图表自身（`BarChart`、`LineChart` 等）直接从 Recharts 导入，shadcn 不做二次封装。

### 3.2 与 Tremor 的图表能力对比

| 图表类型 | Tremor v3 | shadcn/ui + Recharts | 结论 |
|---------|-----------|---------------------|------|
| 柱状图（含堆叠、水平布局） | BarChart | BarChart | 完全覆盖 |
| 面积图（含堆叠、渐变填充） | AreaChart | AreaChart + SVG linearGradient | 完全覆盖 |
| 折线图（含多条线、数据标签） | LineChart | LineChart + LabelList | 完全覆盖 |
| 环形图（占比） | DonutChart | PieChart + innerRadius | 完全覆盖 |
| 雷达图 | — | RadarChart | Recharts 额外提供 |
| 径向条形图 | — | RadialBarChart | Recharts 额外提供 |
| 散点图 | — | ScatterChart | Recharts 额外提供 |

**litellm 使用的 4 种图表全部可 1:1 复刻。**

### 3.3 API 风格差异

Tremor 是声明式 props 风格：

```tsx
<BarChart
  data={chartData}
  index="endpoint"
  categories={["success", "failed"]}
  colors={["green", "red"]}
  stack={true}
/>
```

shadcn/ui + Recharts 是组合式 JSX 风格：

```tsx
<ChartContainer config={chartConfig}>
  <BarChart data={chartData}>
    <CartesianGrid vertical={false} />
    <XAxis dataKey="endpoint" />
    <ChartTooltip content={<ChartTooltipContent />} />
    <Bar dataKey="success" fill="var(--color-success)" stackId="a" />
    <Bar dataKey="failed" fill="var(--color-failed)" stackId="a" />
  </BarChart>
</ChartContainer>
```

差异本质：**Tremor 用 prop 数组声明多条数据线；Recharts 用多个 `<Bar>` / `<Line>` 子组件**。迁移时需要在渲染前做一次 categories → children 的转换，工作量不大。

### 3.4 Tailwind CSS v4 兼容性

shadcn/ui v4 **原生支持** Tailwind CSS v4：
- 不需要 `tailwind.config.js`（v4 用 CSS 文件配置）
- 使用 `@theme inline` 指令注入 `--chart-1` ~ `--chart-5` CSS 变量
- litellm 的 `globals.css` 已经有相同结构的 CSS 变量，证明该模式成熟可用

---

## 4. shadcn/ui 对 Ant Design 的组件覆盖度

### 4.1 可直接替换的组件（80%+）

| 类别 | AntD 组件 | shadcn/ui 对等方案 |
|------|----------|-------------------|
| 表单 | Form, Form.Item, Input, Select, Checkbox, Radio, Switch | React Hook Form + Zod + Form/Input/Select/Checkbox/RadioGroup/Switch |
| 弹窗 | Modal, Drawer | Dialog, Sheet |
| 表格 | Table（基础排序/筛选/分页） | TanStack Table + DataTable |
| 通知 | message, notification | Sonner toast |
| 基础组件 | Button, Badge, Card, Tabs, Accordion, Tooltip, Popover, Dropdown, Progress, Skeleton, Alert, Pagination, Breadcrumb | 均有直接对应组件 |
| 图标 | @ant-design/icons | Lucide React |
| 日期选择 | DatePicker | react-day-picker + Calendar + Popover |

### 4.2 需要额外构建的组件（15%）

| AntD 组件 | 缺失原因 | 替代方案 | 预估工作量 |
|----------|---------|---------|-----------|
| TreeSelect / Tree | shadcn 无树组件 | react-arborist 或递归 Radix Collapsible 自建 | 1-2 天 |
| Cascader | 无级联选择器 | 嵌套 Dialog/Select 或第三方库 | 1-2 天 |
| MultiSelect | 无多选下拉 | Command + Popover 组合实现 | 0.5 天 |
| Upload（完整拖拽上传） | 仅有 Attachment + Progress 原语 | react-dropzone + shadcn 组件 | 1 天 |
| TimePicker | 无时间选择器 | react-time-picker 或原生 input[type=time] | 0.5 天 |
| Transfer（穿梭框） | 无穿梭框 | 两个列表 + 按钮组合 | 0.5-1 天 |
| ColorPicker | 无颜色选择器 | react-colorful (2KB) | 0.5 天 |

### 4.3 aigw 场景不需要的组件

由于 aigw 的边界比 litellm 小很多（目标是 litellm proxy 最小兼容替代），以下 AntD 组件 aigw 用不到：
- TreeSelect/Cascader/Transfer — 无层级组织架构需求
- Upload — 无文件上传场景
- Mentions — 无 @提及需求
- QRCode/Watermark/Affix/Anchor/Tour — 管理控制台不需要

---

## 5. 方案对比

### 5.1 litellm 方案：Tailwind + Tremor + AntD（三库混用）

**优点**：
- 开箱即用，功能覆盖最全
- AntD 的 Table、Form 企业级成熟度高
- Tremor 的图表美观度好

**缺点**：
- 三套 API，学习成本和认知负担高
- 组件冲突需要别名解决（Button vs Button2）
- Bundle 体积大：Tremor (D3) + AntD (CSS-in-JS) + Tailwind
- AntD v5 用 CSS-in-JS 运行时，与 Tailwind 哲学冲突

### 5.2 aigw Stage 4 方案：Tailwind + shadcn/ui

**优点**：
- **一套组件库覆盖 litellm 双库 90% 的功能**
- shadcn/ui 代码在项目中，完全可定制
- 基于 Radix UI 原语，无障碍标准高
- Tailwind v4 原生支持，无冲突
- Bundle 体积小，tree-shake 彻底
- 与 Rust 后端 "pay for what you use" 理念一致

**缺点**：
- 高级 Table（内联编辑、列缩放）需要手写 TanStack Table UI 层
- 图表语法比 Tremor 繁琐（组合式 vs 声明式）
- React Hook Form + Zod 的表单写法比 AntD Form 多些模板代码

### 5.3 最终推荐

**维持 Stage 4 的技术选型：React + TypeScript + Vite + shadcn/ui + Recharts + TanStack Query**

理由：
1. aigw 功能边界明确（Key 管理、用量统计、模型查看），shadcn/ui 完全覆盖
2. 不需要 litellm 那样厚重的 AntD Table/Form/Tree 能力
3. 单套组件库维护成本远低于三库混用
4. Recharts 覆盖 litellm 全部 4 种 Tremor 图表类型
5. Tailwind v4 + shadcn/ui v4 兼容性已验证

---

## 6. 与 aigw Stage 4 规划的对照更新

Stage 4 规划（`docs/stages/stage-4-frontend-plan.md`）选的技术栈与本次调研结论一致，以下做补充确认：

| 项目 | Stage 4 规划 | 调研结论 |
|------|------------|---------|
| 框架 | React + TypeScript + Vite | 维持 |
| 组件库 | shadcn/ui (Radix + Tailwind) | 维持，已验证可覆盖 litellm 功能 |
| 图表 | Recharts | 维持，覆盖 Tremor 全部 4 种图表 |
| 状态管理 | TanStack Query + Zustand | 维持 |
| 部署 | Vite 静态文件 → rust-embed | 维持 |
| 图标 | 未指定 | 推荐 Lucide React（与 shadcn/ui 默认一致） |
| Toast | 未指定 | 推荐 Sonner（shadcn/ui 默认） |
| 表单 | 未指定 | 推荐 React Hook Form + Zod（shadcn/ui 默认） |
| 日期选择 | 未指定 | 推荐 react-day-picker + shadcn Calendar Popover |

---

## 7. 风险与注意事项

1. **Recharts vs Tremor 语法差异**：从 litellm 迁移参考时，需要将 Tremor 声明式 props 转换为 Recharts 组合式 JSX。概念映射清晰，主要是机械翻译。

2. **TanStack Table UI 层**：shadcn 的 DataTable 提供了排序/筛选/分页的基础实现，但如果需要更高级的表格功能（如内联编辑），需要参考 TanStack Table 文档自行构建 UI。

3. **litellm 代码参考边界**：参考 litellm 的前端页面布局和功能设计是合理的，但不应直接复制其 Tremor/AntD 混用的架构。aigw 用 shadcn/ui 单一方案实现更简洁的架构。

4. **静态导出 vs SSR**：aigw 的前端不需要 Next.js。Vite SPA + rust-embed 静态文件服务足以满足管理控制台的需求。

---

## 8. 调研来源

- LiteLLM 代码库: `~/works/projects/github.com/BerriAI/litellm/ui/litellm-dashboard/`
- shadcn/ui 官方文档: https://ui.shadcn.com/docs/components/chart
- Recharts 官方文档: https://recharts.org
- 社区对比文章:
  - [DhiWise: Shadcn UI vs Ant Design](https://www.dhiwise.com/post/shadcn-ui-vs-ant-design-a-comprehensive-comparison-for-2025)
  - [Reddit r/ExperiencedDevs: Ant Design vs shadcn](https://www.reddit.com/r/ExperiencedDevs/comments/1hd7y7y/ant_design_vs_shadcn_part_2/)
  - [Reddit: Replacing Ant Design with shadcn/ui](https://www.reddit.com/r/reactjs/comments/16fj5cx/whats_your_experience_with_replacing_ant_design/)
  - [Builder.io: Should you choose shadcn/ui for enterprise?](https://www.builder.io/blog/shadcn-ui-enterprise)
  - [Vercel Blog: shadcn/ui vs Ant Design 2025](https://vercel.com/blog/shadcn-ui-vs-ant-design-2025)
