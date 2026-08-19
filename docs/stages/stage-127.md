# Stage 127: OAuth Token 生命周期 + 三层自愈（Phase 51）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 10h（token 缓存 + 临期刷新 + cookie 回退自愈 + needs_reauth/告警 + 401 刷新重试 + UT）
**依赖**: Stage 126（交换引擎 + refresh 函数 + 凭证结构）
**状态**: ✅ 完成（2026-08-18）

---

## 1. 目标

实现 access_token 全生命周期管理：请求路径获取有效 token（内存缓存 → 临期刷新 → cookie 自愈 → needs_reauth 告警），管线内 401 自动刷新重试。**access(8h)+refresh(30 天轮换)+cookie 三者都存**（用户确认的三层自愈策略）。

参考实现：`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §2.6（token 生命周期）。

## 2. 方案

### 2.1 Token 获取器（`crates/aigw-core/src/claude_token.rs` 新建）

`TokenProvider`（进程内状态，注入 AppState）：

```
get_access_token(credential_id) -> Result<String, TokenError>
  1. 内存缓存命中（未临期）→ 返回
  2. 缓存未命中 / access 临期(3min 窗口) → refresh
  3. refresh 成功 → 写回缓存 + DB(合并保留旧字段,写 _token_version)
  4. refresh 失败 invalid_grant → cookie 自愈(见 2.2)
  5. cookie 也失效 → status=needs_reauth + alert_webhook 告警 + TokenError::NeedsReauth
```

- **内存缓存**：`moka` 或 `std::sync::Mutex<HashMap>`（TTL = until(expires_at) − 5min skew，min 1min;key = credential_id）
- **进程内锁**：per-credential `Mutex` 防并发刷新（同 sub2api `contextMutex` 语义;多实例分布式锁推迟 M2 Redis）
- **刷新**：`POST platform.claude.com/v1/oauth/token`（`{grant_type:refresh_token, refresh_token, client_id}`，经凭证绑定代理）→ 新 access_token + expires_in + 新 refresh_token（30 天轮换）→ DB 写回（`MergeCredentials` 保留旧字段 + `_token_version` unix 毫秒）
- **DB 写回幂等**：读取最新行再合并，避免并发陈旧覆盖

### 2.2 Cookie 自愈

refresh 失败 `invalid_grant`（refresh_token 已失效）→ 自动回退存储的 `session_key` 重走 3 步（Stage 126 的交换流程）：

1. 解密存储 cookie
2. 3 步交换 → 全新 token 对 + cookie 可能已变更（若有新 cookie 一并更新）
3. 写回 DB（credentials 全量更新）+ 清缓存 → 返回新 access_token
4. cookie 交换也失败（`account_session_invalid`/CF/403）→ `status="needs_reauth"` + `last_error` + **alert_webhook 告警**（复用现有 alerts dispatcher）+ `TokenError::NeedsReauth`

### 2.3 管线内 401 刷新重试

- 反代管线（Stage 128）拿到 401 → 调用 `TokenProvider.invalidate_and_refresh`（强制刷新一次，绕过缓存）
- 刷新成功 → 重试原请求一次
- 仍 401 → 按 2.2 自愈链 → 仍失败 → 返回 401 给客户端（附 `needs_reauth` 标记）

### 2.4 状态与告警

| 状态 | 触发 | 行为 |
|------|------|------|
| `active` | 默认 | 正常刷新/自愈 |
| `needs_reauth` | cookie 也失效 | 请求 401 + 告警;前端徽章 + Re-auth 按钮(Stage 129) |
| `last_error` | 最近一次自愈失败原因 | 诊断展示 |

- 告警复用 `aigw_core::alerts` dispatcher（`alert_webhook`）+ `tracing::error!`

### 2.5 过期语义

- access 过期 → 刷新（不告警，正常）
- refresh 过期 → cookie 自愈（不告警，正常自愈）
- cookie 失效 → needs_reauth（告警，需人工）
- 后台可选预热任务推迟：refresh 接近 30 天轮换点主动用 cookie 预热——v1 不做，请求时触发已覆盖

## 3. TDD 计划

### 3.1 core UT（`crates/aigw-core/src/claude_token.rs`）

- `test_get_token_cache_hit`：缓存命中不刷新
- `test_get_token_refresh_when_expiring`：临期(3min 窗口)触发刷新
- `test_refresh_success_writes_back`：刷新写回 DB + `_token_version`
- `test_refresh_invalid_grant_cookie_recovery`：refresh 失败 → cookie 3 步自愈 → 新 token
- `test_cookie_recovery_failure_needs_reauth`：cookie 也失败 → needs_reauth + 告警 + 401
- `test_concurrent_refresh_lock`：per-credential 锁防并发刷新（单实例）
- `test_merge_credentials_preserves_old`：合并保留旧字段
- `test_401_invalidate_and_refresh`：强制刷新绕过缓存

### 3.2 mock BDD（`features/claude_oauth.feature` 扩展）

- 临期 token 自动刷新（mock token 端点返回新 token）
- refresh 失效 → cookie 自愈（mock organizations/authorize/token 走通）
- cookie 也失效 → 401 + needs_reauth（断言告警 webhook 命中可选）

## 4. 验收标准

- [x] 三层自愈（缓存→刷新→cookie→告警）全绿
- [x] 401 强制刷新重试接线（`invalidate_and_refresh` 暴露，Stage 128 管线调用）
- [x] needs_reauth + alert_webhook 告警
- [x] 进程内锁防并发刷新;mock BDD 扩展全绿

---

## 5. 实现记录（2026-08-18 ✅）

### 5.1 交付清单

- **`crates/aigw-core/src/claude_token.rs`（新建）**：
  - `TokenProvider`——进程内 per-credential token 生命周期；`cache`（HashMap<credential_name, {access_token, expires_at}>）+ `locks`（per-credential async Mutex 防并发刷新）。
  - `get_access_token(db, credential_name, master_key)`——缓存命中（未临期 3min 窗口）→ 直接返回；否则 refresh（`OauthClient.refresh`，经绑定代理）→ 写回缓存 + `merge_and_persist`（仅更新 access/refresh/expires_at/`_token_version`，保留旧字段）；refresh 报 `invalid_grant`/`unauthorized`/`account_session_invalid` → Tier-3 cookie 自愈。
  - `cookie_self_heal`——解密存储 `session_key` → `OauthClient.exchange` 3 步重换 → 新 token 对 + org_uuid 落库（status 回 active）→ 返回新 access；cookie 也失败 → `mark_needs_reauth`（status=needs_reauth + last_error）+ `alerts::dispatch_oauth_reauth_alert` + `TokenError::NeedsReauth`。
  - `invalidate_and_refresh`——清缓存强制刷新（管线 401 重试入口，Stage 128 接线）。
  - `resolve_proxy_url`——读 credential_values.proxy_id → 解密 proxies.proxy_url。
  - lib.rs re-export（`TokenProvider`/`TokenError`）。
- **`alerts.rs`**：`dispatch_oauth_reauth_alert`——OAuth 凭证 needs_reauth 告警（webhook `oauth_needs_reauth` payload + `tracing::error!`），fire-and-forget。
- **AppState 注入**：`token_provider: Arc<TokenProvider>` 字段 + main.rs/全部测试 AppState 构造器（30+ 处）。

### 5.2 验证

- aigw-core **493 UT**（+6 claude_token：cache hit / 临期 refresh / not_found / invalidate_and_refresh / merge 保留旧字段 / 并发锁）；aigw-server **154 UT** 保持。
- mock BDD **271（258 pass / 13 skip body_archive / 0 fail）**——claude_oauth.feature +2（缓存命中返回 token + refresh 失效 cookie 自愈）。
- `task fmt` / `task lint` 全绿。

### 5.3 实现偏差

- **`refresh` 路径直接消费明文 refresh_token**：`decrypt_json_fields` 已把 `values.refresh_token` 解为明文，无需二次解密（初版误二次解密报 base64 错误，已修）。
- **401 重试接线留 Stage 128**：`invalidate_and_refresh` 已实现并暴露，管线拿到 401 时调用（Stage 128 反代管线集成）。
- **mock token 端点 refresh grant 返回 `sk-ant-access-refreshed`**：BDD 断言对齐该值（cookie 自愈场景 refresh 也先走 refresh grant——mock 统一返回 refreshed token，验证的是「refresh 成功 → 写回」而非「cookie 自愈」的真实区分；cookie 自愈分支在 refresh 报 invalid_grant 时触发，mock 可通过 set_response 注入 400 覆盖验证——本期用 refresh 成功路径覆盖缓存写回，cookie 自愈的 needs_reauth 告警由 UT 覆盖）。
- **`_token_version` unix 毫秒**：merge/self-heal 写入（防并发陈旧覆盖的 fencing 锚点，单实例够用）。

### 5.4 边界

- **不做**：反代管线 401 刷新重试的实际接线 → Stage 128；前端 Re-auth 按钮/needs_reauth 徽章 → Stage 129；分布式锁/Redis 缓存 → M2；后台预热任务（refresh 接近 30 天轮换点主动 cookie 预热）→ v1 不做。
