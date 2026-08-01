# Stage 65: 设置中心 + Models 多 Tab + Credential 管理

**Phase**: 24 — 管理控制台完善
**状态**: ⏳ 待开始
**预估**: 5h
**依赖**: Stage 64

---

## 目标

1. **侧边栏 SETTINGS 分组** — 新增分组，Router Settings 从 ACCESS CONTROL 迁入
2. **Router Settings 三 Tab** — Global / Keys / Teams 三级独立配置
3. **Models 页面三 Tab** — Model Groups / Credentials / Health
4. **Credential 管理前端** — 列表 + 创建/编辑/删除（后端 CRUD 已就绪）
5. **Health Tab 集成** — 现有 `pages/health/index.tsx` 搬入 + 对接 `/health/metrics`

---

## Part A — 侧边栏 SETTINGS 分组 (0.5h)

### 1.1 sidebar.tsx

```tsx
// 新增 SETTINGS 分组（插入在 ACCESS CONTROL 之前）
{
  title: "SETTINGS",
  items: [
    { to: "/dash/router-settings", label: "Router Settings", icon: Shuffle },
  ],
},
```

从 ACCESS CONTROL 组移除 Router Settings 条目。导入 `Shuffle` 从顶行 `lucide-react` 已有。

---

## Part B — Router Settings 三 Tab (2h)

### 2.1 改造 `pages/router-settings/index.tsx`

现有页面只有一个 Global 表单卡片。改为 Tabs 结构：

```
┌──────────────────────────────────────────────┐
│  Router Settings                             │
│  ┌─────────┬────────┬───────┐                │
│  │ Global  │ Keys   │ Teams │                │
│  └─────────┴────────┴───────┘                │
│                                              │
│  [当前 Global 表单内容]                        │
│                                              │
│  (Keys Tab → 下拉选择 Key + 表单)              │
│  (Teams Tab → 下拉选择 Team + 表单)           │
└──────────────────────────────────────────────┘
```

| Tab | 数据源 | 读 | 写 |
|-----|--------|----|----|
| Global | `GET /router/settings` | 页面加载时 | `PUT /router/settings` → toast "热生效" |
| Keys | `GET /key/list` | 选 key 后显示其 `router_settings` | `PATCH /key/{token}/router/settings` |
| Teams | `GET /team/list` | 选 team 后显示其 `router_settings` | `PATCH /team/{id}/router/settings` |

**Key/Team Tab 交互**:
- 顶部放一个 Searchable Select 展示 key/team 列表
- 选中后加载该对象的 `router_settings`（从 list 返回的对象中读取）
- Key list 返回对象含 `token` 和 `router_settings` 字段
- Team list 返回对象含 `team_id` 和 `router_settings` 字段
- 未选中时显示 "Select a key/team to configure"

**提取共用**: 三个 Tab 的表单部分（routing_strategy / num_retries / allowed_fails / cooldown_time）抽取为 `<RouterSettingsForm>` 子组件，三个 Tab 共用。

---

## Part C — Models 页面三 Tab + Credential 管理 (2h)

### 3.1 Models 页面改造

当前 `pages/models/index.tsx`（561 行）顶部加 `<Tabs>`：

```
┌──────────────────────────────────────────────┐
│  Models                                      │
│  ┌──────────────┬─────────────┬────────┐     │
│  │ Model Groups │ Credentials │ Health │     │
│  └──────────────┴─────────────┴────────┘     │
│                                              │
│  [Tab 内容]                                   │
└──────────────────────────────────────────────┘
```

- **Model Groups Tab**: 全部现有模型管理代码（列表 + 搜索 + 展开 + 创建/编辑/删除），页面标题 "Model Groups"，`<h1>` 改文字
- **Credentials Tab**: 见 3.2
- **Health Tab**: 见 3.3

### 3.2 Credential 管理页面（Tab 内）

后端已就绪：`POST /credential/new`, `GET /credential/list`, `GET /credential/info`, `PUT /credential/update`, `DELETE /credential/delete`。

**列表列**:

| 列 | 来源 |
|----|------|
| Credential Name | `credential_name` |
| Provider | `credential_values` JSON 中的 `custom_llm_provider` |
| API Base | `credential_values` JSON 中的 `api_base`（截断显示） |
| Info | `credential_info`（简要展示） |
| Actions | 编辑 / 删除 |

**创建/编辑 Dialog**: 复用 ModelDialog 的 dialog 骨架模式：

- `credential_name` — Input
- `credential_values` — Textarea（JSON，含 api_base / api_key / custom_llm_provider）
- `credential_info` — Textarea（JSON，可选元信息如 provider 描述）

**credential_values 中的 api_key 掩码**: 列表不展示完整 api_key，显示 `sk-***{后4位}`。

**删除确认**: 同 Models DeleteConfirm 模式。

**文件**:
- `pages/models/index.tsx` — 加 Tabs 壳 + 导入 CredentialsTab / HealthTab
- `pages/models/CredentialsTab.tsx` — 新建，~200 行
- `pages/models/CredentialDialog.tsx` — 新建，~150 行

### 3.3 Health Tab

现有 `pages/health/index.tsx`（102 行）代码搬入，作为 Health Tab。

**扩展**: 对接 `/health/metrics`（admin only），展示更丰富数据：

```tsx
// 现有 /health 返回
{ status, uptime_seconds, db: { size, connections }, counts: {...}, version }

// 补充 /health/metrics 调用
GET /health/metrics → { pool_size, idle, key_count, model_count, uptime_seconds }
```

**文件**: `pages/models/HealthTab.tsx` — 搬入 + 扩展 ~120 行。

---

## Part D — 前端的路由清理 (0.5h)

- `App.tsx`: 移除独立的 `/dash/health` 路由（合并进 Models Tab 后不再需要独立页面）
- 确认 `pages/health/index.tsx` 被 HealthTab 替代后可标记废弃或删除
- Models 路由保持不变（`/dash/models`）

---

## 测试

| 类型 | # | 场景 |
|------|---|------|
| BDD | 1 | Credentials: 创建 → 列表可见 → 编辑 → 删除 |
| BDD | 2 | Models 页面三 Tab 切换正常，Tab 状态保持 |
| BDD | 3 | Router Settings: Keys Tab 选 Key → 设 allowed_fails → 保存 → 重开值保持 |
| 手动 | — | Health Tab 展示 /health 和 /health/metrics 数据 |

> 后端无新增 UT（所有 API 端点已有 UT 覆盖）。

---

## 门禁

- [ ] `npm run build` 前端构建通过
- [ ] `cargo test` 全量 UT 回归通过
- [ ] BDD: 97 → 100 scenarios 全部通过
- [ ] 前端 BDD: 114 → 123 tests（3 新增 scenarios × 3 viewports）
- [ ] 手动验证：侧边栏 SETTINGS 分组可见、Router Settings 三 Tab 切换正常、Credentials CRUD 端到端
