# Stage 31: 侧边栏分组重构 + Usage 重命名

**Phase**: 12 — 前端导航重构 + Playground
**状态**: ⏳ 待开始
**预估**: 2h

---

## 目标

对齐 litellm admin UI 侧边栏 5 组结构，重命名 Dashboard→Usage，移除 `/dash/home` 路由。

## 验收标准

- [ ] 侧边栏分 3 组（AI GATEWAY / OBSERVABILITY / ACCESS CONTROL），每组灰色大写标题
- [ ] AI GATEWAY 组: Virtual Keys, Models, Playground（Virtual Keys 置顶）
- [ ] OBSERVABILITY 组: Usage, Spend Logs
- [ ] ACCESS CONTROL 组: Users, Teams, Organizations
- [ ] `/dash/home` 路由删除，`/dash` 默认 redirect → `/dash/usage`
- [ ] `pages/dashboard/` → `pages/usage/`，Usage 页只保留概览卡 + 图表（移除 Spend Logs 表）
- [ ] "Keys" → "Virtual Keys", "Orgs" → "Organizations"
- [ ] 分组标题在侧边栏折叠时隐藏（与 litellm 一致）
- [ ] BDD 测试适配新路由名

## 关键文件

| 文件 | 操作 |
|------|------|
| `src/components/layout/sidebar.tsx` | 重写：分组结构 + grey section headers |
| `src/App.tsx` | 路由：`/dash/home` → `/dash/usage`，`/dash` redirect → `/dash/usage` |
| `src/pages/dashboard/index.tsx` | 移动至 `src/pages/usage/index.tsx`，移除 spend logs |
| `tests/features/*.feature` | 更新路由引用 |
| `tests/steps/*.ts` | 更新路由引用 |

## 侧边栏分组设计（对齐 litellm leftnav.tsx）

```
┌─────────────────────────┐
│  aigw Admin             │
│─────────────────────────│
│  AI GATEWAY             │  ← 灰色小标题 10px #6b7280
│    🔑 Virtual Keys      │
│    📦 Models            │
│    🎮 Playground        │
│                         │
│  OBSERVABILITY          │
│    📈 Usage             │
│    📋 Spend Logs        │
│                         │
│  ACCESS CONTROL         │
│    👤 Users             │
│    👥 Teams             │
│    🏢 Organizations     │
└─────────────────────────┘
```

## 依赖

- 无

## 输出

- [ ] 分组 sidebar 组件
- [ ] 新路由配置
- [ ] Usage 页面（瘦身后）
- [ ] BDD 测试更新
