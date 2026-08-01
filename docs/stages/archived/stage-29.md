# Stage 29: 用户/组织/团队管理前端页面

**创建日期**: 2026-07-08
**状态**: ⏳ 待开始
**优先级**: P1
**前置条件**: Stage 25（BDD）+ Stage 26（登录安全，获取 user_role）+ Stage 27（移动端适配）
**预估**: 6-8h

---

## 1. 目标

在前端暴露后端已有的 15 个用户/组织/团队 CRUD 端点，提供完整的管理页面。侧边栏新增 Users、Orgs、Teams 导航项。

---

## 2. 设计决策

### 2.1 页面结构

```
/dash/users   — 用户列表 + 新建/编辑/删除
/dash/orgs    — 组织列表 + 新建/编辑/删除
/dash/teams   — 团队列表 + 新建/编辑/删除 + 成员管理
```

每个页面与 Keys 页面保持一致的交互模式：表格/卡片列表 → 搜索 → 新建按钮 → 表单 Dialog → 删除确认 Dialog。

### 2.2 数据流

```
API 端点                页面          操作
GET  /user/list    →   Users 列表   → 展示所有用户
POST /user/new     →   新建表单     → 提交创建
GET  /user/info    →   编辑表单     → 预填数据
PUT  /user/update  →   编辑表单     → 提交更新
DELETE /user/delete →  删除确认     → 删除用户

（Org/Team 同理）
```

### 2.3 字段设计

**Users 页面字段（列表 + 表单）：**

| 字段 | 列表列 | 表单可见 | 必填 | 说明 |
|------|--------|---------|------|------|
| user_id | ✓ | 自动生成 | - | UUID v7 |
| user_email | ✓ | ✓ | ✓ | 标识（也用于登录） |
| password | - | ✓ | 创建时 | scrypt 哈希 |
| user_role | ✓ | ✓ | - | proxy_admin / internal_user 等 |
| user_alias | ✓ | ✓ | - | 显示名称 |
| organization_id | - | ✓ | - | 下拉选择组织 |
| team_id | - | ✓ | - | 下拉选择团队 |
| max_budget | ✓ | ✓ | - | 最大预算 |
| tpm_limit | - | ✓ | - | Token/分钟限制 |
| rpm_limit | - | ✓ | - | 请求/分钟限制 |

**Orgs 页面字段：**

| 字段 | 列表列 | 表单必填 | 说明 |
|------|--------|---------|------|
| organization_id | ✓ | 自动生成 | UUID v7 |
| organization_alias | ✓ | ✓ | 显示名称 |
| budget_id | ✓ | - | 预算 ID |
| metadata | - | - | JSON（高级） |

**Teams 页面字段：**

| 字段 | 列表列 | 表单必填 | 说明 |
|------|--------|---------|------|
| team_id | ✓ | 自动生成 | UUID v7 |
| team_alias | ✓ | ✓ | 显示名称 |
| organization_id | ✓ | ✓ | 下拉选择 |
| members | - | ✓ | 多选（user_id 列表） |
| admins | - | ✓ | 多选（user_id 列表） |
| max_budget | ✓ | - | 最大预算 |
| blocked | ✓ | - | 是否禁用 |

### 2.4 关联字段处理

对于 `organization_id` 字段和 `members`/`admins` 字段：
- 表单中下拉框，通过 `GET /org/list` 和 `GET /user/list` 获取选项
- 使用 TanStack Query 的 `useQuery` 缓存选项数据

---

## 3. 交付

### 3.1 新增文件

```
crates/aigw-frontend/src/
  pages/
    users/index.tsx          # [NEW] 用户列表 + CRUD
    orgs/index.tsx            # [NEW] 组织列表 + CRUD
    teams/index.tsx           # [NEW] 团队列表 + CRUD + 成员管理
  components/layout/
    sidebar.tsx               # [MODIFY] 添加 Users/Orgs/Teams 导航
  tests/features/
    users.feature             # [NEW] 用户管理 BDD
    orgs.feature              # [NEW] 组织管理 BDD
    teams.feature             # [NEW] 团队管理 BDD
```

### 3.2 导航结构更新

```typescript
const navItems = [
  { to: "/dash/home", label: "Home", icon: LayoutDashboard },
  { to: "/dash/keys", label: "Keys", icon: Key },
  { to: "/dash/models", label: "Models", icon: Box },
  // 新增
  { to: "/dash/users", label: "Users", icon: Users },
  { to: "/dash/orgs", label: "Orgs", icon: Building2 },
  { to: "/dash/teams", label: "Teams", icon: Users2 },
];
```

### 3.3 BDD 场景（核心）

**users.feature:**
```gherkin
Feature: 用户管理
  Scenario: 查看用户列表
    Given 管理员已登录
    And API 返回 3 个用户
    When 访问 "/dash/users"
    Then 显示 3 个用户

  Scenario: 创建新用户
    Given 管理员已登录
    When 点击 "New User"
    And 填写 user_email "test@example.com"
    And 填写 password "secret123"
    And 选择 user_role "internal_user"
    And 点击 "Create"
    Then 用户列表刷新
    And 新用户出现在列表中

  Scenario: 编辑用户
    Given 管理员已登录
    When 点击某用户的编辑按钮
    And 修改 user_alias 为 "Updated Name"
    And 点击 "Save"
    Then 用户 alias 更新

  Scenario: 删除用户
    Given 管理员已登录
    When 点击某用户的删除按钮
    And 确认删除
    Then 用户从列表消失
```

---

## 4. 门禁

- [ ] `/dash/users` 用户列表正确展示（含 email、role、alias、budget）
- [ ] 新建用户：email + password + role + org_id + team_id 全部可设
- [ ] 编辑用户：可以修改 email、alias、role、limits
- [ ] 删除用户：确认后删除成功
- [ ] `/dash/orgs` 组织 CRUD 全功能
- [ ] `/dash/teams` 团队 CRUD + members/admins 多选
- [ ] [R-G-R] users.feature、orgs.feature、teams.feature 全部通过
- [ ] 移动端卡片布局正常（复用 Stage 27 的 ResponsiveCardList）
- [ ] 侧边栏新增导航项正确高亮
- [ ] 仅 admin 角色可见（通过 `user_role` 控制，非 admin 隐藏导航项）
