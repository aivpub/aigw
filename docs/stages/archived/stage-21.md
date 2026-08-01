# Stage 21: 前端工程搭建

**创建日期**: 2026-07-08
**完成日期**: 2026-07-08
**状态**: ✅ 完成
**优先级**: P1
**前置条件**: Phase 8 完成（Stage 18-20）
**预估**: 2-3h

---

## 1. 目标

初始化前端项目（Vite + React + shadcn/ui），通过 rust-embed 集成到 aigw-server 二进制。

---

## 2. 交付

### 2.1 技术栈（已确认）

| 维度 | 选型 |
|------|------|
| 框架 | React + TypeScript + Vite |
| 组件库 | shadcn/ui（Radix UI + Tailwind CSS v4） |
| 图表 | shadcn/ui chart（基于 Recharts） |
| 状态管理 | TanStack Query + Zustand |
| 表单 | react-hook-form + zod |
| 图标 | Lucide React |
| Toast | Sonner |
| 部署 | Vite SPA → rust-embed |

### 2.2 项目结构

```
crates/aigw-frontend/
  src/
    main.tsx
    App.tsx
    components/
      ui/           # shadcn/ui components
      layout/       # Sidebar, Header
    pages/
      dashboard/    # Stage 23
      keys/         # Stage 22
      models/       # Stage 24
    lib/
      api.ts        # API client
      utils.ts
  public/
  index.html
  package.json
  vite.config.ts
  tsconfig.json
  tailwind.config.ts
```

### 2.3 rust-embed 集成

- 在 `aigw-server` 中通过 `rust-embed` 宏嵌入前端 `dist/` 产物
- `GET /admin` 返回 `index.html`
- `GET /admin/*` 返回对应静态资源（JS/CSS/图片）
- 对于 SPA 路由（`/admin/keys` 等），后端都返回 `index.html`，由 React Router 处理

### 2.4 构建流程

- `npm run build` → `dist/` 产物
- `cargo build -p aigw-server` 时 rust-embed 将 `dist/` 嵌入二进制
- Makefile 新增 `task frontend-build` 串联两个步骤

---

## 3. 门禁

- `npm run dev` 前端独立启动正常
- `GET /admin` 返回前端页面
- `GET /admin/keys` SPA 路由正常（不返回 404）
- 不影响现有 API 端点
