# Phase 35: Core Entity Soft-Delete — 独立归档表实现

**项目**: aigw (litellm Rust 最小兼容替代)  
**创建日期**: 2026-07-28  
**前置**: Stage 87（Spend Logs UI 双 id + 模糊搜索）  
**参考**: litellm `LiteLLM_DeletedTeamTable` / `LiteLLM_DeletedVerificationToken` 模式；aigw 现有 `deleted_keys` 实现

---

## 背景

当前 aigw 的五张核心表中，仅 `virtual_keys` 有删除归档（`deleted_keys` 独立表 + `KeyStore::delete_key` 的「先归档后硬删」流程）。`teams`、`users`、`organizations`、`proxy_models` 四张表全部硬删除，`DELETE FROM` 直接移除数据，无审计追溯能力。

litellm 采用「独立归档表」模式处理软删除：为 `LiteLLM_TeamTable` 和 `LiteLLM_VerificationToken` 分别新建 `LiteLLM_DeletedTeamTable` 和 `LiteLLM_DeletedVerificationToken`，归档表完整镜像源表所有列 + `deleted_at` + `deleted_by` 审计列。删除时先 INSERT 归档表，再 DELETE 源表。

aigw 的 `deleted_keys`（`010_deprecated_keys.sql`）已实现同样的模式，`KeyStore::delete_key()`（`db.rs:564`）三方言统一流程：读取 → 归档 → 硬删。本次扩展将这一模式覆盖到剩余四张表。

### 核心约束

- **不改源表结构**：不在 `teams`/`users`/`organizations`/`proxy_models` 上加 `deleted_at` 列
- **不改现有 API 响应格式**：DELETE 端点返回与现有完全一致（向后兼容）
- **不改变现有 trait 接口签名**：`TeamStore::delete_team` 等方法签名不变，仅实现内部改为 tombstone-then-delete
- **审计字段**：新增 `deleted_at`（必填），`deleted_by` 留到后续 Phase 由 auth middleware 注入

---

## Stage 拆分

| Stage | 名称 | 类型 | 预估 |
|-------|------|------|------|
| Stage 88 | 后端 — 归档表迁移 + DB 层 + API 路由 + 测试 | 后端+测试 | 12h |
| Stage 89 | 前端 — 删除确认增强 + 已删除视图 + E2E 验证 | 前端+测试 | 6h |

**Phase 35 合计**: 18h，2 Stages

---

## Stage 88: 后端 — 归档表迁移 + DB 层 + API 路由 + 测试

### 核心预期

四张 `deleted_*` 归档表的完整后端实现：迁移脚本（三方言）、Store trait 的 tombstone-then-delete 改造、归档列表查询端点、UT + BDD 覆盖。

### 实现方案

#### 1. DB 迁移

**迁移文件**: `024_deleted_tables.sql`（三方言各一份）

创建 4 张归档表，每张完整镜像源表所有列 + `deleted_at` 审计列。PK 使用自增 `id`（避免源 ID 重复问题——同一 team_id 可能被删后重建再删），源 ID 降级为普通索引列。

```sql
-- 示例：MySQL deleted_teams
CREATE TABLE IF NOT EXISTS deleted_teams (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    -- 完整镜像 teams 表所有列 (team_id, team_alias, organization_id, ...直到 allow_team_guardrail_config)
    deleted_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_deleted_teams_team_id (team_id),
    INDEX idx_deleted_teams_deleted_at (deleted_at)
);

-- deleted_users — 同上模式
-- deleted_organizations — 同上模式
-- deleted_models — 同上模式 (model_id 做索引)
```

> **关于 PK 设计**: `deleted_keys` 用 `token` 做 PK，因为 token hash 天然唯一。但 `team_id`/`user_id` 可能被删后重建——用源 ID 做主键重复。改用自增 `id` 做主键、源 ID 做索引。litellm 的 `LiteLLM_DeletedTeamTable` 正是这样（`id String @id @default(uuid())`）。

`db.rs` 的 `data_cleanse()`（line ~241）的 `blocked_tables` 数组需新增 4 张 `deleted_*` 表名。

#### 2. DB 层（db.rs）

**修改 `delete_*` 实现**：参考 `KeyStore::delete_key()`（`db.rs:564`）三步流程，对 `TeamStore`/`UserStore`/`OrganizationStore`/`ModelStore` 的 delete 方法全部改为：

```
1. 查询源表获取完整行（不存在 → 返回 Ok 幂等）
2. INSERT INTO deleted_* 归档表
3. DELETE FROM 源表
```

涉及：4 个 trait × 3 方言 = 12 处修改，加 8 条 INSERT SQL 常量（MySQL/SQLite 共用 `?` 占位符常量，PG 独立 `$1` 占位符常量）。

**新增 `list_deleted_*` 方法**：4 个 trait 各加一个方法 + 3 方言实现。

**关键细节**：
- MySQL bool 字段绑定用 `as i8`（参考 `db.rs:804`）
- 不用事务包裹（与现有 `delete_key` 一致——INSERT 失败行保留在源表，偏安全）
- `deleted_at` 由数据库 DEFAULT 填充，不在 INSERT 列中显式传值

#### 3. API 路由

**现有端点无感知切换**：trait 方法签名不变，`team.rs`/`user.rs`/`org.rs`/`models.rs` 的四个 DELETE handler 代码完全不动。

**新增归档列表端点**：

| 端点 | Handler | 文件 |
|------|---------|------|
| `GET /team/deleted` | `team_deleted_list` | `routes/team.rs` |
| `GET /user/deleted` | `user_deleted_list` | `routes/user.rs` |
| `GET /org/deleted` | `org_deleted_list` | `routes/org.rs` |
| `GET /model/deleted` | `model_deleted_list` | `routes/models.rs` |

全部 admin-only（`require_admin`），第一版不分页全量返回。`main.rs:338-410` 注册 4 条新路由。

**新增 models.rs struct**：`DeletedTeam`/`DeletedUser`/`DeletedOrganization`/`DeletedModel` 各一个（derive `Serialize, FromRow`），或复用现有 struct + 追加 `deleted_at` 字段。

#### 4. 测试

**UT**（`db.rs` `#[cfg(test)]` 模块）：
- `test_delete_team_soft` — 源表空 + 归档有记录 + deleted_at 非空
- `test_delete_user_soft` / `test_delete_org_soft` / `test_delete_model_soft` — 同上
- `test_delete_idempotent` — 重复删除不报错
- `test_list_deleted_*` — 归档列表查询

**BDD**（`tests/bdd_steps/`）：
- `team_steps.rs` / `user_steps.rs` / `org_steps.rs` / `model_steps.rs` 各新增 step
- `When I delete the {entity}` → DELETE 调用
- `Then the {entity} should be in deleted {entities}` → GET /entity/deleted 验证
- 对应 `.feature` 文件

**集成验证**：SQLite 内存 DB 全部通过；MySQL/Postgres 通过 feature flag 运行；验证迁移脚本在全新 DB 正确创建归档表。

### 涉及文件

| # | 文件 | 操作 |
|---|------|------|
| 1 | `crates/aigw-core/migrations/mysql/024_deleted_tables.sql` | 新建 |
| 2 | `crates/aigw-core/migrations/postgres/024_deleted_tables.sql` | 新建 |
| 3 | `crates/aigw-core/migrations/sqlite/024_deleted_tables.sql` | 新建 |
| 4 | `crates/aigw-core/src/db.rs` | 新增 8 个 INSERT 常量 + 修改 12 个 delete 实现 + 新增 12 个 list_deleted 实现 + 新增 `blocked_tables` 条目 + UT |
| 5 | `crates/aigw-core/src/models.rs` | 新增 4 个 Deleted* struct |
| 6 | `crates/aigw-server/src/routes/team.rs` | 新增 `GET /team/deleted` handler |
| 7 | `crates/aigw-server/src/routes/user.rs` | 新增 `GET /user/deleted` handler |
| 8 | `crates/aigw-server/src/routes/org.rs` | 新增 `GET /org/deleted` handler |
| 9 | `crates/aigw-server/src/routes/models.rs` | 新增 `GET /model/deleted` handler |
| 10 | `crates/aigw-server/src/main.rs` | 注册 4 条新路由 |
| 11 | `crates/aigw-server/tests/bdd_steps/team_steps.rs` | BDD step |
| 12 | `crates/aigw-server/tests/bdd_steps/user_steps.rs` | BDD step |
| 13 | `crates/aigw-server/tests/bdd_steps/org_steps.rs` | BDD step |
| 14 | `crates/aigw-server/tests/bdd_steps/model_steps.rs` | BDD step |
| 15 | `crates/aigw-server/tests/features/` | 新增 .feature 文件 |

### 门禁

- `task test` 全部通过（三方言）
- `task lint` 无警告
- BDD 全部通过（三后端 SQLite/PG/MySQL）

---

## Stage 89: 前端 — 删除确认增强 + 已删除视图 + E2E

### 核心预期

1. 五个管理页面（keys/teams/users/orgs/models）删除按钮统一增强确认弹窗
2. 每个管理页面新增"已删除"Tab，展示归档记录
3. Playwright BDD 覆盖删除 → 已删除 Tab 可见的完整交互流

### 实现方案

#### 1. 删除确认增强

**现状**: `keys/index.tsx` 已有 `deleteOpen` state + `deleteMutation` + Dialog。`models/DeleteConfirm.tsx` 有可复用组件。teams/users/orgs 需要对齐。

**统一模式**（每个页面）:
- 删除按钮 → 打开确认 Dialog（复用 `models/DeleteConfirm.tsx` 或提取为通用组件）
- 确认后调用 DELETE API → toast 提示 → 刷新列表
- 文案：「确定要删除 {name} 吗？删除后可在"已删除"中查看历史记录。」

#### 2. 已删除视图

每个管理页面用 `<Tabs>` 切换两个视图：
- "活跃" Tab：现有列表（代码不动）
- "已删除" Tab：`apiGet('/{entity}/deleted')` → 表格展示

归档表格列包含：源实体关键字段 + `deleted_at`。只读展示，不提供编辑/恢复按钮（恢复留到后续 Phase）。

#### 3. E2E 验证

Playwright BDD 场景覆盖：
- 删除确认弹窗出现 → 确认 → toast 成功提示
- 切换"已删除"Tab → 表格显示被删记录 + `deleted_at` 列
- 重复删除同一实体 → 幂等，不报错

### 涉及文件

| # | 文件 | 操作 |
|---|------|------|
| 1 | `crates/aigw-frontend/src/pages/keys/index.tsx` | 新增"已删除"Tab（复用现有 delete 逻辑） |
| 2 | `crates/aigw-frontend/src/pages/teams/index.tsx` | 删除确认 + 已删除 Tab |
| 3 | `crates/aigw-frontend/src/pages/users/index.tsx` | 删除确认 + 已删除 Tab |
| 4 | `crates/aigw-frontend/src/pages/orgs/index.tsx` | 删除确认 + 已删除 Tab |
| 5 | `crates/aigw-frontend/src/pages/models/index.tsx` | 已删除 Tab（复用现有 `DeleteConfirm.tsx`） |
| 6 | Playwright BDD 场景文件 | 新增 E2E 场景 |

### 门禁

- `npm run bdd` Playwright 测试通过
- 五个管理页面均可切换"已删除"Tab 查看归档记录

---

## 依赖关系

```
Stage 88 (后端) → Stage 89 (前端)
```

后端先行落地（API + 测试），前端基于已就绪的接口契约独立开发。

---

## 不纳入 Phase 35 的项目（后续 Phase）

| 项目 | 原因 |
|------|------|
| 恢复（restore）功能 | 需额外 UI + API + 冲突处理逻辑 |
| 归档表数据保留策略（TTL） | 运维策略，独立 SOP 文档 |
| `deleted_by` 审计列 | 需 auth middleware 注入用户信息 |
| cascade 级联软删除 | 业务语义复杂（删 org 时下级 team/key 的归档策略） |
| 归档表定期清理 cron job | 独立运维功能 |
