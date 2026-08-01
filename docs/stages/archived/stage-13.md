# Stage 13: config 表 + credentials 表 + 全量 Store PostgreSQL 支持

**创建日期**: 2026-07-06
**状态**: ✅ 完成
**优先级**: P0
**前置条件**: 无
**预估**: 6-8h

---

## 1. 目标

补齐 aigw 缺失的数据模型，并将所有 Store 的 PostgreSQL（及 MySQL）实现补全。

### 1.1 当前差距

| 表/Store | SQLite | PostgreSQL | MySQL |
|----------|--------|------------|-------|
| `LiteLLM_Config` / `config` | ❌ 不存在 | ❌ | ❌ |
| `credentials` | ❌ 不存在 | ❌ | ❌ |
| `KeyStore` | ✅ | ✅ | ✅ |
| `SpendLogStore` | ✅ | ✅ | ✅ |
| `ProxyModelStore` | ✅ | ❌ 仅 SQLite | ❌ |
| `CredentialsStore` | ❌ 不存在 | ❌ | ❌ |

### 1.2 `config` 表的必要性

litellm 将 `master_key` 持久化在 `LiteLLM_Config` 表（`param_name='general_settings'` 的 JSON 中）。aigw 当前没有对应的配置表，`master_key` 仅从环境变量 `MASTER_KEY` 读取，不落库。

Stage 15 迁移时需要将加密数据写入 aigw DB，aigw 必须有自己的 `master_key` 来加密。把 `master_key` 持久化到 `config` 表的好处：

- 与 litellm 数据结构对齐，`aigw-migrate` 可直接迁移 `LiteLLM_Config` → `config`
- aigw 启动时从 DB 读取 `master_key`（比环境变量更可靠）
- 支持未来更多的运行时配置持久化（`router_settings`、`litellm_settings` 等）

---

## 2. 交付

### 2.1 `config` 表

对齐 litellm 的 `LiteLLM_Config`：

| 列 | 类型 | 约束 |
|----|------|------|
| `id` | INTEGER | PK AUTOINCREMENT |
| `param_name` | TEXT | NOT NULL |
| `param_value` | TEXT (JSON) | NOT NULL |

存储行示例：
```
param_name = "general_settings"
param_value = {"master_key": "sk-aigw-master-key", "database_url": "postgres://..."}
```

- Migration SQL（SQLite + PostgreSQL）
- 启动时读取逻辑：`SELECT param_value FROM config WHERE param_name = 'general_settings'` → JSON 解析 → `master_key`
- 与 `AIGW_MASTER_KEY` 环境变量互补：环境变量为 override，DB 为 fallback

### 2.2 `credentials` 表

对齐 `LiteLLM_CredentialsTable`：

| 列 | 类型 | 约束 |
|----|------|------|
| `credential_id` | TEXT/UUID | PK |
| `credential_name` | TEXT | UNIQUE NOT NULL |
| `credential_values` | TEXT (JSON) | NOT NULL |
| `credential_info` | TEXT (JSON) | |
| `created_at` | TEXT (DATETIME) | NOT NULL |
| `created_by` | TEXT | |
| `updated_at` | TEXT (DATETIME) | NOT NULL |
| `updated_by` | TEXT | |

Migration SQL（SQLite + PostgreSQL）。

### 2.3 `CredentialsStore` trait + 全量 DB 实现

```rust
#[async_trait]
pub trait CredentialsStore {
    async fn insert_credential(&self, c: &Credential) -> Result<()>;
    async fn get_credential_by_name(&self, name: &str) -> Result<Option<Credential>>;
    async fn list_credentials(&self) -> Result<Vec<Credential>>;
    async fn update_credential(&self, name: &str, c: &Credential) -> Result<()>;
    async fn delete_credential(&self, name: &str) -> Result<()>;
}
```

实现：`SqlitePool` + `PgPool` + `MySqlPool`（与 `KeyStore`/`SpendLogStore` 模式一致）。

### 2.4 `ProxyModelStore` PostgreSQL + MySQL 实现

将 `proxy_models` 的 CRUD 从 `impl ProxyModelStore for SqlitePool` 扩展为 `for PgPool` + `for MySqlPool`。修改 `Database` enum 的 5 个 dispatch 方法，去掉 `only SQLite` 限制。

### 2.5 `Credential` 数据模型

`crates/aigw-core/src/models.rs` 新增：

```rust
pub struct Credential {
    pub credential_id: String,
    pub credential_name: String,
    pub credential_values: serde_json::Value,  // {api_key, api_base, api_version, ...}
    pub credential_info: serde_json::Value,
    pub created_at: String,
    pub created_by: Option<String>,
    pub updated_at: String,
    pub updated_by: Option<String>,
}
```

### 2.6 凭证管理 REST API

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/credentials` | 创建凭证 |
| `GET` | `/credentials` | 列表（支持 `?credential_name=xxx`） |
| `GET` | `/credentials/{credential_name}` | 查询单个 |
| `PUT` | `/credentials/{credential_name}` | 更新 |
| `DELETE` | `/credentials/{credential_name}` | 删除 |

### 2.7 `aigw-migrate` TABLE_MAPPINGS 扩展

```rust
("LiteLLM_Config", "config"),
("LiteLLM_CredentialsTable", "credentials"),
("LiteLLM_ProxyModelTable", "proxy_models"),
```

---

## 3. 门禁

- `config` 表 migration 正确执行，`general_settings` 写入与读取正确
- `credentials` 表 migration 正确执行（SQLite + PostgreSQL）
- 启动时从 `config` 表读取 `master_key` 正常（fallback 到 `MASTER_KEY` 环境变量）
- `CredentialsStore` 三个数据库后端 CRUD 测试通过
- `ProxyModelStore` PostgreSQL/MySQL 测试通过
- `/credentials/*` BDD 测试通过
- `aigw-migrate verify` 新表映射验证正确
