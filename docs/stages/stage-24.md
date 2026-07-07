# Stage 24: Model 管理页面

**创建日期**: 2026-07-08
**完成日期**: 2026-07-08
**状态**: ✅ 完成
**优先级**: P2
**前置条件**: Stage 21 完成
**预估**: 2-3h

---

## 1. 目标

实现 proxy_models 列表查看和详情展示。

---

## 2. 交付

### 2.1 页面功能

| 功能 | 说明 |
|------|------|
| 模型列表 | TanStack Table：model_name、provider、modality、created_at、updated_at |
| 搜索 | 按 model_name 实时过滤 |
| 详情展开 | 点击行展开详情：litellm_params（JSON 格式化展示） |
| 状态标签 | Badge 显示 active/inactive 状态 |

### 2.2 UI 组件

- `DataTable` — 列表
- `Sheet` / 展开行 — 详情
- `Badge` — 状态标签
- `Input` — 搜索

### 2.3 API 对接

- `GET /model/info` — 模型列表
- 单个模型详情从列表数据中提取

### 2.4 路由

`/admin/models` — Model 管理主页

---

## 3. 门禁

- 模型列表正确展示所有 proxy_models
- 搜索过滤正常
- 详情展开显示 litellm_params（JSON 格式化）
- 空列表状态正常
