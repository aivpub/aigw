# Stage 122: 代理服务管理 — 后端 CRUD（Phase 50）

**所属**: Phase 50（代理服务管理）
**预估**: 10h（migration ×3 + model + db store ×3 + 路由 + in-use 守卫 + 加密 + UT/BDD）
**依赖**: 无
**状态**: ✅ 完成（2026-08-18）

---

## 1. 目标

在系统配置中新增**代理服务管理**后端能力：`proxies` 表（Migration 027 × 3 方言）+ CRUD 路由 `/admin/proxies/*` + 创建后异步探测 + in-use 守卫（代理被凭证引用时禁止删除）。前端见 Stage 124。

参考实现：`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §1（sub2api `proxies` 表 + `admin_proxy.go`）。

## 2. 数据模型（精简版）

`proxies` 表——**不拆细字段**，整串 `proxy_url` 加密落库（用户决策）：

```sql
CREATE TABLE proxies (
    id           INTEGER PRIMARY KEY,          -- SQLite INTEGER / PG BIGSERIAL / MySQL BIGINT AUTO_INCREMENT
    name         TEXT NOT NULL,
    proxy_url    TEXT NOT NULL,                -- 整串加密落库(master_key AES-GCM, v2:gcm: 前缀)
    status       TEXT NOT NULL DEFAULT 'active',  -- active / inactive / expired
    expires_at   TEXT,                         -- NULL=永不过期;status=expired 由它派生
    probe_result TEXT NOT NULL DEFAULT '{}',   -- 检测快照 JSON(Stage 123 填充,本 Stage 默认为空对象)
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_proxies_status ON proxies(status);
```

**明确不做**：不拆分 protocol/host/port/username/password；不做 fallback_mode/backup_proxy_id/expiry_warn_days（过期回退登记长期路线）。

## 3. 方案

### 3.1 Rust model（`crates/aigw-core/src/models.rs`）

```rust
pub struct Proxy {
    pub id: i64,
    pub name: String,
    pub proxy_url: String,       // 加密密文（DB 存密文；响应侧由 handler 解密 + redact）
    pub status: String,          // active/inactive/expired
    pub expires_at: Option<String>,
    pub probe_result: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
```

### 3.2 db.rs store（× 3 方言）

- `create_proxy` / `get_proxy_by_id` / `list_proxies`(分页 + status/search/sort 过滤) / `update_proxy` / `delete_proxy` / `list_active_proxies`(下拉用) / `count_proxies_by_proxy_url`(查重可选)
- 排序：`created_at desc` 默认；search 匹配 `name LIKE` 与 `proxy_url LIKE`
- 方言差异：占位符 `?`/`$N`、分页 `LIMIT ? OFFSET ?`/`LIMIT $1 OFFSET $2`、`datetime('now')`/`NOW()` 默认值——按现有 3 方言惯例

### 3.3 proxy_url 加密（`crates/aigw-core/src/crypto.rs`）

- 复用现有 `AES-GCM v2:gcm:` 机制（`encrypt_litellm_value` / `decrypt_litellm_value`），proxy_url 与 litellm credential 同强度
- 新增 helper：`encrypt_proxy_url` / `decrypt_proxy_url`（薄封装，标记用途清晰）
- **响应 redact**：列表/详情响应中 proxy_url 返回 `scheme://user:***@host:port` 形态（password 掩码），完整解密仅服务端使用

### 3.4 路由（`crates/aigw-server/src/routes/proxies.rs`，新建）

| 方法 | 路径 | handler | 说明 |
|------|------|---------|------|
| GET | `/admin/proxies` | `list_proxies` | 分页 + `status`/`search`/`sort_by`/`sort_order` 过滤;响应含 probe_result 解析出的 exit_ip/country/latency/score/grade 透出 |
| GET | `/admin/proxies/all` | `list_all_proxies` | 全部 active,下拉用(凭证绑定 + 前端 Select) |
| POST | `/admin/proxies` | `create_proxy` | body `{name, proxy_url, expires_at?}`;proxy_url 加密;创建后 `tokio::spawn` 异步探测(Stage 123 接线,本 Stage 预留) |
| GET | `/admin/proxies/{id}` | `get_proxy` | 详情(解密 + redact password) |
| PUT | `/admin/proxies/{id}` | `update_proxy` | 整串替换 proxy_url(重新加密);同步触发异步重探测(Stage 123) |
| DELETE | `/admin/proxies/{id}` | `delete_proxy` | **in-use 守卫**(见 3.5),被引用 → 409 |
| POST | `/admin/proxies/batch-delete` | `batch_delete_proxies` | 批量;逐条 in-use 跳过 + `{deleted_ids, skipped:[{id, reason}]}` |

- 全部 `SpendAuth` + `require_admin`（复用 `routes/spend.rs:221`）
- 路由注册：main.rs `/admin/proxies` 段

### 3.5 in-use 守卫

删除/批量删除前，扫描 `credentials.credential_values` JSON 中的 `proxy_id` 字段是否等于目标 id：

- SQLite/MySQL：`SELECT credential_name FROM credentials WHERE json_extract(credential_values, '$.proxy_id') = ?`（SQLite）/ `JSON_EXTRACT`（MySQL）
- PostgreSQL：`SELECT credential_name FROM credentials WHERE credential_values->>'proxy_id' = $1`
- 命中 → 409 `PROXY_IN_USE`（含引用凭证名），拒绝删除
- Stage 126 落 OAuth 凭证结构后生效；本 Stage 实现扫描逻辑，当前无 OAuth 凭证 → 恒不命中

## 4. TDD 计划

### 4.1 core UT（`crates/aigw-core/src/db.rs` tests / `tests/proxies.rs`）

- `test_proxy_crud_roundtrip`：create → get → update → list → delete
- `test_proxy_list_filters`：status/search/sort 过滤 + 分页
- `test_proxy_delete_in_use`：credentials 含 proxy_id 引用 → 拒绝
- `test_proxy_url_encrypt_decrypt`：proxy_url 加解密 roundtrip + 密文不含明文
- `test_proxy_delete_not_found_idempotent`

### 4.2 handler UT（`crates/aigw-server/src/routes/proxies.rs` tests）

- `test_proxy_create_masks_password`：响应 redact
- `test_proxy_delete_in_use_409`
- `test_proxy_require_admin_403`

### 4.3 mock BDD（`features/proxies.feature` 新建）

- 创建代理 / 列表分页过滤 / 详情 / 更新 / 删除 / 批量删除 in-use 跳过 / 非 admin 403

## 5. 验收标准

- [x] Migration 027 三方言应用通过（sqlite/pg/mysql）
- [x] proxies CRUD + in-use 守卫 + proxy_url 加密/redact 全绿
- [x] mock BDD proxies.feature 全绿;real BDD 三后端 CRUD 全绿（Stage 125 补 real BDD）
- [x] fmt + clippy `-D warnings` green;既有基线无回归

---

## 6. 实现记录（2026-08-18 ✅）

### 6.1 交付清单

- **Migration 027 ×3**：`crates/aigw-core/migrations/{sqlite,postgres,mysql}/027_proxies.sql`——`proxies` 表（id/name/proxy_url/status/expires_at/probe_result/created_at/updated_at + idx_proxies_status）；SQLite `INTEGER PRIMARY KEY AUTOINCREMENT`、PG `BIGSERIAL`、MySQL `BIGINT AUTO_INCREMENT`；`probe_result TEXT NOT NULL DEFAULT '{}'`。
- **`Proxy` model**（models.rs）+ `CreateProxyRequest`/`UpdateProxyRequest` 请求体；lib.rs re-export `Proxy`。
- **crypto.rs**：`encrypt_proxy_url` / `decrypt_proxy_url`（薄封装 `v2:gcm:`）+ `redact_proxy_url`（`scheme://user:***@host:port`，无 `@`/无 `://` 原样返回）+ 5 UT。
- **`ProxyStore` trait** ×3 方言：`create_proxy`（返回新 id）/ `get_proxy_by_id` / `list_proxies`（分页 + status/search/sort 过滤，SQLite `?1/?2` 复用、PG `$1::text IS NULL OR ...` + ILIKE、MySQL `CONCAT('%',?,'%')`）/ `count_proxies` / `list_active_proxies` / `update_proxy` / `delete_proxy` / `proxy_in_use_by_credentials`（SQLite `json_extract(credential_values,'$.proxy_id')`、MySQL `JSON_EXTRACT`、PG `credential_values->>'proxy_id'`）/ `credentials_referencing_proxy`。lib.rs re-export `ProxyStore`。
- **`routes/proxies.rs`**（新建）+ `routes/mod.rs` + `main.rs` 注册 4 组路由：`/admin/proxies`（GET list + POST create）、`/admin/proxies/all`、`/admin/proxies/batch-delete`、`/admin/proxies/{id}`（GET/PUT/DELETE）。全部 `SpendAuth` + `require_admin`。
- **in-use 守卫**：DELETE / batch-delete 前置 `proxy_in_use_by_credentials` → 409 `PROXY_IN_USE` + `referenced_by:[credential_name]`。
- **proxy_url 加密**：create/update 时 `encrypt_proxy_url`（AIGW_MASTER_KEY 缺失 → 500 config_error）；响应 `ProxyResponse` 解密 + `redact_proxy_url` 掩码（`probe_result` 透出 exit_ip/country/country_code/latency_ms/score/grade 供 Stage 124 列表）。
- **异步探测预留**：`spawn_async_probe`（`tokio::spawn`，Stage 123 替换为真实出口+质量探测写快照；请求路径不阻塞）。
- **测试**：5 core UT（crypto 加密/redact + db CRUD roundtrip / list filters / list_active / in-use / delete idempotent）+ 3 handler UT（create masks password / delete in-use 409 / non-admin 403）+ **proxies.feature 8 BDD 场景**（create+redact / list+快照 / detail / update / delete / in-use 409 / non-admin 403 / batch-delete）。

### 6.2 验证

- aigw-core **468 UT**（+10：crypto 5 + db proxy 5）全绿；aigw-server **152 UT**（+3 handler）全绿。
- mock BDD **257 场景（244 pass / 13 skip body_archive / 0 fail）**——+8 proxies.feature 场景，skip 数保持 13 不变（body_archive_read 7 + body_archive_write 6）。
- `task test` / `task fmt` / `task lint` 全绿；`task build`（workspace debug）通过。
- ALL_TABLES 表清单加 `proxies`（`test_all_23_tables_exist_after_migration` 通过）。

### 6.3 实现偏差

- **batch-delete 场景名引用**：BDD step `已创建代理 "batch-a"/"batch-b"` 同时存 `proxy:{name}` 键供批量删除定位 id（最初因 `created_keys` 存 id 字符串拼进 JSON 产生 422，修复为解析 i64）。
- **cucumber 参数转义**：`{id}` 在 `#[when(expr)]` 中需 `\{id\}` 转义（`{` 是 cucumber 参数语法）。
- **`sqlx::query_as(sql)`**（String 字面量）：clippy `needless_borrow` 要求传值（`&sql` 被报 E0308——`query_as` 接受 `&str`，String 字面量直接传；格式化的 String 仍需 `&sql`）。
- **Step 语义冲突**：`一个普通 key {string} 已生成` 与 spend_steps 重复 → 代理专用 step 改名 `已生成普通 key {string}（代理场景）`。

### 6.4 边界

- **不做**：real BDD 三后端 proxies CRUD → **Stage 125 收尾**补；`/test` `/quality` `/batch-*` `/toggle` → **Stage 123**；前端 ProxiesPage → **Stage 124**；fallback/expiry 回退 → 长期路线。
- 过期状态派生：`status=expired` 由 `expires_at` 派生，本 Stage 不实现自动派生逻辑（Stage 123 探测时顺带刷新）。
