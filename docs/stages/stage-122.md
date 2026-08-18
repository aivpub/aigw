# Stage 122: 代理服务管理 — 后端 CRUD（Phase 50）

**所属**: Phase 50（代理服务管理）
**预估**: 10h（migration ×3 + model + db store ×3 + 路由 + in-use 守卫 + 加密 + UT/BDD）
**依赖**: 无
**状态**: ⏳ 待开始

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

- [ ] Migration 027 三方言应用通过（sqlite/pg/mysql）
- [ ] proxies CRUD + in-use 守卫 + proxy_url 加密/redact 全绿
- [ ] mock BDD proxies.feature 全绿;real BDD 三后端 CRUD 全绿
- [ ] fmt + clippy `-D warnings` green;既有基线无回归
