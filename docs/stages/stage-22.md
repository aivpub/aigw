# Stage 22: Key 管理页面

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P1
**前置条件**: Stage 21 完成
**预估**: 3-4h

---

## 1. 目标

实现 Key 管理页面：列表、搜索、创建、编辑、删除、复制 API key。

---

## 2. 交付

### 2.1 页面功能

| 功能 | 说明 |
|------|------|
| Key 列表 | TanStack Table 展示：alias、key（脱敏）、models、budget、expires、spend |
| 搜索 | 按 key_alias 实时过滤 |
| 创建 | Dialog 表单：key_alias、models、max_budget、budget_duration、metadata、team_id |
| 编辑 | Dialog 预填当前值，更新 key_alias、models、max_budget 等 |
| 删除 | 确认 Dialog，删除后列表刷新 |
| API key 复制 | 创建成功后显示完整 key，一键复制，仅显示一次 |
| 显示/隐藏 | 列表中 key 字段默认脱敏（`sk-xxx...xxx`），toggle 显示完整 |

### 2.2 UI 组件

- `DataTable` — shadcn/ui 数据表格（排序、分页）
- `Dialog` — 创建/编辑/删除表单弹窗
- `Form` + `Input` + `Select` — react-hook-form + zod 校验
- `Sonner` toast — 操作成功/失败反馈
- `CopyButton` — 一键复制

### 2.3 API 对接

- `GET /key/list` — 获取列表
- `POST /key/generate` — 创建
- `POST /key/update` — 更新
- `POST /key/delete` — 删除
- `GET /key/info?key=...` — 查询单个

### 2.4 路由

`/admin/keys` — Key 管理主页

---

## 3. 门禁

- 列表正确展示所有 key
- 搜索过滤正常工作
- 创建/编辑/删除操作后列表刷新
- API key 创建后显示完整，复制按钮可复制
- 列表中的 key 默认脱敏
