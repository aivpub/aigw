# Stage 19: 多租户管理 API

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P1
**前置条件**: Stage 17 完成（可与 Stage 18 并行）
**预估**: 4-6h

---

## 1. 目标

补齐 `/org/*`、`/team/*`、`/user/*` 完整 CRUD，为前端管理控制台提供后端 API。

---

## 2. 交付

### 2.1 端点清单

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/org/new` | 创建组织 |
| `GET` | `/org/info?organization_id=...` | 查询组织详情 |
| `PUT` | `/org/update` | 更新组织 |
| `DELETE` | `/org/delete` | 删除组织 |
| `GET` | `/org/list` | 列出所有组织 |
| `POST` | `/team/new` | 创建团队 |
| `GET` | `/team/info?team_id=...` | 查询团队详情 |
| `PUT` | `/team/update` | 更新团队 |
| `DELETE` | `/team/delete` | 删除团队 |
| `GET` | `/team/list` | 列出所有团队 |
| `POST` | `/user/new` | 创建用户 |
| `GET` | `/user/info?user_id=...` | 查询用户详情 |
| `PUT` | `/user/update` | 更新用户 |
| `DELETE` | `/user/delete` | 删除用户 |
| `GET` | `/user/list` | 列出所有用户 |

### 2.2 技术方案

- **BDD 驱动开发** — 先写 `.feature` 场景，再实现
- 基于现有 `Store` trait 扩展 `OrganizationStore`、`TeamStore`、`UserStore`
- 复用现有 SQLite/MySQL/PostgreSQL 三数据库实现
- 鉴权：管理员（master_key）可操作所有 CRUD，普通 key 仅可查询

### 2.3 数据模型

| 表 | 核心字段 |
|----|---------|
| `organizations` | `organization_id`, `organization_alias`, `budget_id`, `models`, `spend`, `max_budget` |
| `teams` | `team_id`, `team_alias`, `organization_id`, `budget_id`, `models`, `spend`, `max_budget` |
| `users` | `user_id`, `user_alias`, `user_email`, `organization_id`, `team_id`, `budget_id`, `models`, `spend`, `max_budget` |

### 2.4 BDD 场景示例

```gherkin
Scenario: 管理员创建组织
  Given 使用 master-key
  When 发送 POST /org/new 请求体 {"organization_alias": "my-org"}
  Then 响应状态码为 200
  And 响应 JSON 包含 "organization_id" 字段

Scenario: 普通 key 无法创建组织
  Given 一个普通 key "user-key" 已生成
  When 使用 key "user-key" 发送 POST /org/new 请求
  Then 响应状态码为 403

Scenario: 列出所有组织
  Given 使用 master-key
  When 发送 GET /org/list 请求
  Then 响应状态码为 200
  And 响应 JSON 包含 "data" 数组
```

---

## 3. 门禁

- 15 端点全部通过 BDD 测试（通过三数据库）
- 管理员 CRUD 全部可用
- 普通 key 写操作返回 403
- 响应格式与 litellm 兼容
