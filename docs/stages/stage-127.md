# Stage 127: OAuth Token 生命周期 + 三层自愈（Phase 51）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 10h（token 缓存 + 临期刷新 + cookie 回退自愈 + needs_reauth/告警 + 401 刷新重试 + UT）
**依赖**: Stage 126（交换引擎 + refresh 函数 + 凭证结构）
**状态**: ⏳ 待开始

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

- [ ] 三层自愈（缓存→刷新→cookie→告警）全绿
- [ ] 401 强制刷新重试接线
- [ ] needs_reauth + alert_webhook 告警
- [ ] 进程内锁防并发刷新;mock BDD 扩展全绿
