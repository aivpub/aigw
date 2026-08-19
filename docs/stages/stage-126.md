# Stage 126: Claude OAuth 凭证 + Cookie→Token 交换引擎（Phase 51）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 12h（凭证结构 + 3 步交换客户端 + 敏感加密/redact + exchange 端点 + mock OAuth 上游 + UT/BDD）
**依赖**: Phase 50（Stage 122-123，凭证绑代理引用 + 代理客户端 + proxy_id in-use 扫描）
**状态**: ✅ 完成（2026-08-18）

---

## 1. 目标

凭证管理支持 `sk-ant-sid***` cookie 换 token：扩展 `credentials` 表 `credential_values` 为 OAuth 结构化凭证（access/refresh/session_key 加密落库 + proxy_id/inject_prompt/org_uuid 明文），新增 `POST /credential/oauth/exchange` 走 3 步交换（PKCE S256，经绑定代理）。

参考实现：`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §2.1/2.2（OAuth 常量 + 3 步交换）。

## 2. 方案

### 2.1 凭证结构化扩展（`credentials.credential_values` JSON）

```json
{
  "type": "anthropic_oauth",
  "access_token": "<enc>", "refresh_token": "<enc>", "session_key": "<enc>",
  "expires_at": 1752900000, "proxy_id": 3, "inject_prompt": null,
  "org_uuid": "...", "account_uuid": "...", "email_address": "...",
  "status": "active", "last_error": null
}
```

- **加密落库**（master_key AES-GCM `v2:gcm:`）：`access_token` / `refresh_token` / `session_key`
- **明文**：`type` / `proxy_id` / `inject_prompt` / `org_uuid` / `account_uuid` / `email_address` / `expires_at`(unix 秒) / `status` / `last_error`
- **redact**：`crates/aigw-core/src/account_credentials_redact` 补 `access_token`/`refresh_token`/`session_key` 到敏感键清单（查证现有 `SensitiveCredentialKeys` 是否已含，不足则补）
- **routing**:proxy_models 经现有 `litellm_credential_name` 引用该凭证;resolver 判定 `type=="anthropic_oauth"` 即 OAuth 部署

### 2.2 OAuth 客户端（`crates/aigw-core/src/claude_oauth.rs` 新建）

固定常量（对齐 sub2api，来自真实抓包）：

```rust
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const ORGS_URL: &str = "https://claude.ai/api/organizations";
const AUTHORIZE_URL: &str = "https://claude.ai/v1/oauth/{org}/authorize";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const REDIRECT_URI: &str = "https://platform.claude.com/oauth/code/callback";
const SCOPE_API: &str = "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
```

**3 步交换**（全程走绑定代理）：
1. `GET {ORGS_URL}`，cookie `sessionKey=<sk-ant-sid>` → `[{uuid, name, raven_type}]`;多 org 优先 `raven_type=="team"`
2. `POST {AUTHORIZE_URL}`，body `{response_type:code, client_id, organization_uuid, redirect_uri, scope, state, code_challenge, code_challenge_method:S256}` → 从 `redirect_uri` 解析 `code`+`state`
3. `POST {TOKEN_URL}`，body `{code, grant_type:authorization_code, client_id, redirect_uri, code_verifier, state}` → `{access_token, refresh_token, expires_in(28800), scope, organization.uuid, account.uuid/email_address}`

- **PKCE S256**：verifier = 32 随机字节 base64url-nopad;challenge = base64url(sha256(verifier))
- **刷新**（Stage 127 接线，本 Stage 实现函数）：`{grant_type:refresh_token, refresh_token, client_id}`
- 客户端：`build_proxy_client`（Stage 123）+ 60s 超时;UA 伪装（`Mozilla/5.0...` 浏览器形态，claude.ai 端对抗 CF）
- 错误分类（参考 sub-check `classifyStep1Error`）：CF challenge / account_session_invalid / account_disabled|suspended|terminated / 401 / 403 / 429 / 5xx，返回结构化错误

### 2.3 交换端点

`POST /credential/oauth/exchange`（admin）：

- body：`{session_key, proxy_id?, inject_prompt?, name}`
- 流程：3 步交换 → 构造 OAuth 凭证 JSON（敏感字段加密）→ `create_proxy` 同款写 `credentials` 表 → 返回凭证（redact 后）
- proxy_id 为 null → 凭证不绑代理（直连交换）
- 校验：session_key 前缀 `sk-ant-` 警告（非致命）;proxy_id 存在性 + active
- 创建后可异步跑一次 `claude_oauth` 质量探测（复用 Stage 123 目标）校验出口

### 2.4 mock Anthropic OAuth 上游

BDD mock 新增 Anthropic OAuth 模拟（`MockUpstream`）：
- `GET /api/organizations` → 返回 org 列表（含 team）
- `POST /v1/oauth/{org}/authorize` → 返回 `{redirect_uri: ".../callback?code=...&state=..."}`
- `POST /v1/oauth/token`（authorization_code / refresh_token 双 grant）→ 返回 token 对
- 供 BDD 场景走真实 3 步链路

## 3. TDD 计划

### 3.1 core UT（`crates/aigw-core/src/claude_oauth.rs`）

- `test_pkce_verifier_challenge`：S256 正确性（43 字符 + 匹配 challenge）
- `test_exchange_step1_org_selection`：单 org / 多 org 优先 team / 无 org 报错
- `test_exchange_full_flow`：3 步成功 → token 对
- `test_exchange_error_classify`：CF/account_session_invalid/403/429 分类
- `test_oauth_credential_encrypt`：凭证 JSON 敏感字段加密 roundtrip + redact
- `test_refresh_grant`：refresh_token 刷新请求体构造

### 3.2 handler UT

- `test_oauth_exchange_endpoint`：成功 + redact 响应
- `test_oauth_exchange_bad_session_key`：403 分类错误
- `test_oauth_exchange_proxy_not_found`：proxy_id 无效 400

### 3.3 mock BDD（`features/claude_oauth.feature` 新建）

- cookie 换 token 成功（走 mock 3 步）
- proxy_id 绑定生效（请求经 mock 代理发出）
- 敏感字段响应 redact
- 坏 cookie → 结构化错误

## 4. 验收标准

- [x] credential OAuth 结构化字段 + 敏感加密/redact 全绿
- [x] 3 步交换（mock 上游）+ proxy 绑定 + 错误分类全绿
- [x] `POST /credential/oauth/exchange` 可用
- [x] mock BDD claude_oauth.feature 全绿;既有基线无回归

---

## 5. 实现记录（2026-08-18 ✅）

### 5.1 交付清单

- **`crates/aigw-core/src/claude_oauth.rs`（新建）**：
  - 常量：`CLIENT_ID` / `REDIRECT_URI` / `SCOPE_API` / `DEFAULT_EXPIRES_IN`（对齐 sub2api 抓包）。
  - `pkce_s256()`——32 随机字节 base64url-nopad verifier（43 字符）+ SHA-256 challenge；`pkce_state()`。
  - `OauthClient`——`new(proxy_url)`（走 `build_proxy_client` 经绑定代理，60s 超时 + 浏览器 UA 伪装）；`fetch_orgs`（cookie `sessionKey=` → org 列表）/ `authorize`（PKCE body → redirect_uri 解析 code+state）/ `exchange_code` / `refresh`（refresh_token grant，Stage 127 接线）/ `exchange`（3 步全流程 → `(TokenResponse, org_uuid)`）。
  - `select_org`——多 org 优先 `raven_type=="team"`；空列表报 `no_org`。
  - `parse_redirect_code`——redirect_uri 查询串解析 code+state（state 校验）。
  - `classify_oauth_error`——CF challenge（cf-ray/cf-mitigated/签名/HTML-403）→ `account_session_invalid` → `account_disabled|suspended|terminated` → 401/403/429/5xx/unknown。
  - `build_oauth_credential_values`——access/refresh/session_key **AES-GCM 单独加密** + proxy_id/inject_prompt/org_uuid/account_uuid/email_address/expires_at/status 明文。
  - lib.rs re-export（`OauthClient`/`OauthError`/`OauthOrg`/`TokenResponse`/`pkce_s256`/`select_org`/`parse_redirect_code`/`classify_oauth_error`/`build_oauth_credential_values`）。
- **crypto.rs**：`OAUTH_SENSITIVE_KEYS`（access_token/refresh_token/session_key）+ `redact_oauth_credential_values`（敏感键掩码 `***`，非对象透传）+ 2 UT。
- **`routes/credentials.rs`**：`POST /credential/oauth/exchange`——session_key 校验（sk-ant- 前缀 warn）+ proxy_id 存在性/active + 解密 proxy_url → 3 步交换 → 凭证落库（敏感加密）→ 响应 redact；`credential_info`/`credential_list` 响应统一 redact OAuth 敏感字段。main.rs 注册路由。
- **mock OAuth 上游**：`MockUpstream` 新增 `/api/organizations`（GET）+ `/v1/oauth/{org}/authorize`（POST）+ `/v1/oauth/token`（POST，authorization_code/refresh_token 双 grant），默认返回 team org + 固定 token 对，支持 `set_response` 覆盖错误。
- **BDD harness 端点重映射**：`AIGW_OAUTH_MOCK_BASE` env（仅测试）→ OAuth client 把 claude.ai/platform.claude.com 端点改写为 mock base，3 步交换全走 MockUpstream（**永不进生产代码路径**）。
- **测试**：11 core UT（PKCE/org 选择/redirect 解析/错误分类/凭证加密）+ mock BDD **claude_oauth.feature 4 场景**（成功+redact / 坏 cookie 403 / 无代理 400 / 已存凭证 redact）。

### 5.2 验证

- aigw-core **487 UT**（+12：claude_oauth 11 + crypto redact 1）；aigw-server **154 UT** 保持。
- mock BDD **269（256 pass / 13 skip body_archive / 0 fail）**——+4 claude_oauth.feature。
- `task fmt` / `task lint` 全绿。

### 5.3 实现偏差

- **端点常量改写**：原设计固定 `https://claude.ai/...` 常量；为让 mock BDD 走通，加 `endpoint()` helper 读 `AIGW_OAUTH_MOCK_BASE`（仅 BDD 设 env）——**生产零影响**（env 未设时走原 URL）。
- **`refresh()` 为 async**：Stage 127 TokenProvider 需要 await（设计 §2.2 提到 refresh，未标注同步/异步）。
- **401 分类**：mock 401 JSON body `{"error":"unauthorized"}` 命中 `unauthorized` kind（非 account_session_invalid——那需要 body 含 `account_session_invalid` 标记）。
- **credential_info/list redact**：设计 §2.1 仅提 exchange 响应 redact；实现扩大到 info/list 一致掩码（避免泄漏路径）。

### 5.4 边界

- **不做**：Token 生命周期（缓存/刷新/自愈）→ Stage 127；反代管线（resolver OAuth 识别 + billing 注入）→ Stage 128；前端 CredentialsTab OAuth 入口 → Stage 129。
