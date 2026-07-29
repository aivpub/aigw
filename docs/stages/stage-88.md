# Stage 88: 四表软删除 — 后端全链路（迁移 + DB 层 + API + 测试）

**Phase**: 35 — Core Entity Soft-Delete
**优先级**: P1
**状态**: ⏳ 待开始
**预估**: 12h
**前置**: Stage 87（Spend Logs UI 双 id + 模糊搜索）
**参考**: litellm `LiteLLM_DeletedTeamTable` / `LiteLLM_DeletedVerificationToken` 独立归档表模式；aigw 现有 `deleted_keys` 实现（`db.rs:564`）

---

## 核心预期

**teams、users、organizations、proxy_models 四张表的 DELETE 操作从「纯硬删除」改为「先归档到独立 deleted_* 表再硬删」，同时提供归档列表查询 API。现有 DELETE 端点行为无感知切换（响应格式不变），新增 4 条归档列表端点。**

---

## 背景

当前 `virtual_keys` 已有软删除（`deleted_keys` 归档表 + `KeyStore::delete_key()` tombstone-then-delete），但 `teams`/`users`/`organizations`/`proxy_models` 全是 `DELETE FROM` 硬删除。一旦误删或需要审计追溯，数据不可恢复。

litellm 为 `TeamTable` 和 `VerificationToken` 各建了 `Deleted*` 独立归档表（完整镜像源表列 + `deleted_at` + `deleted_by`）。aigw 的 `deleted_keys` 已走通这条路。本 Stage 将该模式覆盖到剩余四表。

### 不改的边界

- 源表不加 `deleted_at` 列——独立归档表物理隔离
- 现有 DELETE 端点响应格式不变——向后兼容
- Trait 方法签名不变——路由层零改动
- `deleted_by` 审计列留到后续 Phase（需 auth middleware 注入）

---

## 实现

### ① DB 迁移 — `024_deleted_tables.sql` × 3 方言

每张归档表完整镜像源表所有列，追加 `deleted_at` 审计列。PK 用自增 `id`（源 ID 可能被删后重建再删导致主键冲突，`deleted_keys` 的 `token` 是 hash 天然唯一没这个问题）。

```sql
-- MySQL 示例: deleted_teams
CREATE TABLE IF NOT EXISTS deleted_teams (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    -- teams 表所有列 (team_id, team_alias, organization_id, ...allow_team_guardrail_config)
    deleted_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_deleted_teams_team_id (team_id),
    INDEX idx_deleted_teams_deleted_at (deleted_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- deleted_users / deleted_organizations / deleted_models — 同上模式
```

四张表：`deleted_teams`、`deleted_users`、`deleted_organizations`、`deleted_models`（model_id 做索引）。

**SQLite 变体**: `id INTEGER PRIMARY KEY AUTOINCREMENT`，`DATETIME`，`BLOB` for JSON  
**PG 变体**: `id BIGSERIAL PRIMARY KEY`，`TIMESTAMPTZ(3)`，`JSONB`

`db.rs:241` 的 `data_cleanse()` `blocked_tables` 数组追加四张 `deleted_*` 表名。

---

### ② DB 层 — Store trait tombstone-then-delete 改造

参考 `KeyStore::delete_key()`（`db.rs:564`）三步流程，修改 4 个 trait × 3 方言 = 12 处实现：

```
1. SELECT * FROM {table} WHERE {pk} = ?  → 获取完整行
2. 不存在 → 返回 Ok(())  幂等
3. INSERT INTO deleted_{table} (...) VALUES (...)  → 归档
4. DELETE FROM {table} WHERE {pk} = ?
```

**新增 SQL 常量**（8 条）：4 表 × 2 占位符风格（`?` 用于 MySQL/SQLite 共用 + `$N` 用于 PG）。参考 `INSERT_DELETED_KEY_SQLITE`（`db.rs:424`）格式。

**新增 `list_deleted_*` 方法**：4 个 trait 各加一个 `async fn list_deleted_{entity}s(&self) -> Result<Vec<Deleted{Entity}>>` + 三方言实现（`SELECT * FROM deleted_{table} ORDER BY deleted_at DESC`）。

**关键细节**：
- MySQL bool 列绑定用 `as i8`（参考 `db.rs:804` 的 `soft_budget_cooldown_bool() as i8`）
- `deleted_at` 不显式传值，由 DB DEFAULT 填充
- 不用事务包裹（与 `delete_key` 一致——INSERT 失败则行保留在源表，偏安全）

---

### ③ models.rs — 新增 4 个 Deleted* struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeletedTeam {
    pub id: i64,
    pub team_id: String,
    pub team_alias: Option<String>,
    // ... 其余 teams 表所有列
    pub deleted_at: DateTime<Utc>,
}
// DeletedUser / DeletedOrganization / DeletedModel — 同上
```

> **备选方案**: 复用现有 `Team`/`User`/`Organization`/`ProxyModel` struct + 追加 `deleted_at` 字段（因归档表多了 `id` 和 `deleted_at`，列前缀匹配可能不完美）。优先用独立 struct 保证映射清晰。

---

### ④ API 路由 — 现有端点不动 + 新增 4 条归档列表端点

**现有端点零改动**（trait 方法签名不变）：

- `DELETE /team/delete` — `team.rs:236`
- `DELETE /user/delete` — `user.rs:288`
- `DELETE /org/delete` — `org.rs:204`
- `DELETE /model/delete` — `models.rs:279`

**新增端点**（`main.rs:338-410` 注册）：

| 端点 | Handler | 文件 |
|------|---------|------|
| `GET /team/deleted` | `team_deleted_list` | `routes/team.rs` |
| `GET /user/deleted` | `user_deleted_list` | `routes/user.rs` |
| `GET /org/deleted` | `org_deleted_list` | `routes/org.rs` |
| `GET /model/deleted` | `model_deleted_list` | `routes/models.rs` |

全部 admin-only（`require_admin`），第一版不分页全量返回 `Vec<DeletedEntity>`。

**Handler 签名**（示例）：

```rust
/// GET /team/deleted — list archived (soft-deleted) teams
pub async fn team_deleted_list(
    State(state): State<SharedState>,
    SpendAuth(auth): SpendAuth,
) -> Result<Json<Vec<DeletedTeam>>, (StatusCode, Json<Value>)> {
    require_admin(&auth)?;
    state.db.list_deleted_teams().await.map(Json).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("{}", e)})))
    })
}
```

---

### ⑤ 测试 — UT + BDD

**UT**（`db.rs` `#[cfg(test)]` 模块，SQLite 内存 DB）：
- `test_delete_team_soft` — 源表空、归档有记录、deleted_at 非空
- `test_delete_user_soft` / `test_delete_org_soft` / `test_delete_model_soft` — 同上
- `test_delete_idempotent` — 重复删除同一实体不报错
- `test_list_deleted_teams` — 归档列表查询返回正确记录

**BDD**（`tests/bdd_steps/`）：
- 4 个 step 文件各新增 `When I delete the {entity}` + `Then the {entity} should be in deleted {entities}` step
- 对应 `.feature` 文件
- mock BDD + 三后端 real BDD（mysql/pg/sqlite）

**集成验证**：
- `task test` 全部通过（三方言）
- 迁移脚本在全新 DB 正确创建四张归档表
- MySQL/Postgres 通过 feature flag 运行

---

## 涉及文件

| # | 文件 | 操作 |
|---|------|------|
| 1 | `crates/aigw-core/migrations/mysql/024_deleted_tables.sql` | 新建 |
| 2 | `crates/aigw-core/migrations/postgres/024_deleted_tables.sql` | 新建 |
| 3 | `crates/aigw-core/migrations/sqlite/024_deleted_tables.sql` | 新建 |
| 4 | `crates/aigw-core/src/db.rs` | 新增 8 SQL 常量 + 改 12 delete 实现 + 加 12 list_deleted 实现 + blocked_tables + UT |
| 5 | `crates/aigw-core/src/models.rs` | 新增 DeletedTeam/DeletedUser/DeletedOrganization/DeletedModel struct |
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

## 门禁

- `task test` 全部通过
- `task lint` 无警告
- BDD mock + real 三后端全部通过

---

## 关键决策

- **归档表 PK 用自增 `id` 而非源 ID**：team_id/user_id 可能被删后重建再删，用源 ID 做主键会冲突。`deleted_keys` 用 `token` 做 PK 是因为 token hash 天然唯一，但这个假设对其他实体不成立。litellm 的 `LiteLLM_DeletedTeamTable` 正是用 UUID `id` 做主键、`team_id` 做索引。
- **幂等不报 404**：行不存在时直接返回 Ok，不报错。与 `delete_key` 行为一致。
- **不用事务**：与 `delete_key` 保持一致。INSERT 失败则行留在源表（偏安全），DELETE 失败则两表都有记录（可接受，不会丢数据）。补齐事务放到后续 Phase。
- **不分页**：归档表数据量不大，第一版全量返回。
