# sub2api 代理管理 + Claude OAuth 反代 — 参考实现技术调研

> 调研日期:2026-08-18 | 用途:Phase 50/51 设计输入
> 参考仓库:`~/works/play/sub2api`(Go + ent ORM + gin),HEAD `67fa331f0`
> 结论来源:直接读源码(backend/internal + backend/ent/schema),非第三方转述

---

## 1. 代理管理(sub2api `proxies`)

### 1.1 Schema(`backend/ent/schema/proxy.go`,表 `proxies`)

```go
name, protocol(http/https/socks5...), host, port, username, password,
status(active/inactive/expired), expires_at(可空), 
fallback_mode(none/proxy/direct), backup_proxy_id(自引用), expiry_warn_days(默认7),
TimeMixin(created_at/updated_at) + SoftDeleteMixin(deleted_at)
```

**URL 构造**(`service/proxy.go:42`):`url.URL{Scheme: protocol, Host: net.JoinHostPort(host, port)}`,含 `user:pass` → `scheme://user:pass@host:port`。

### 1.2 CRUD(`service/admin_proxy.go`)

- `ListProxies`/`ListProxiesWithAccountCount`:分页 + protocol/status/search/sort 过滤 + LEFT JOIN account count
- `CreateProxy`:校验 fallback_mode(proxy 必须有 backup)、expiry_warn_days ≥ 0;创建后 **异步 probe**(`go s.probeProxyLatency`)
- `UpdateProxy`:backup 不能是自身;整字段透传 expires/fallback
- `DeleteProxy`:**in-use 守卫** `CountAccountsByProxyID`,>0 报 `ErrProxyInUse`
- `BatchDeleteProxies`:逐条 in-use 跳过 + `{DeletedIDs, Skipped[{ID, Reason}]}`
- `TestProxy` / `CheckProxyQuality`:见 1.4
- 另有 `GetProxyAccounts`、`CheckProxyExists`(host/port/auth 查重)

### 1.3 出口探测(`repository/proxy_probe_service.go`)

- 探测 URL 按优先级:`http://ip-api.com/json/?lang=zh-CN`(解析 ip-api)备选 `http://api64.ipify.org?format=json`(ipify)
- 经代理 GET(10s 超时,`httpclient.GetClient(Options{ProxyURL, ...})`),响应体限 1MB
- 解析:ip-api 返回 `status/city/region/regionName/country/countryCode/query(IP)`;返回 `ProxyExitInfo{IP, City, Region, Country, CountryCode}` + latencyMs
- 两个 URL 都失败才报错(某些 AI API 专用代理只放行特定域名)

### 1.4 质量检测(`service/admin_proxy.go:CheckProxyQuality`)

`ProxyQualityCheckResult{Score, Grade(A/B/C/D/F), ExitIP, Country, CountryCode, BaseLatencyMs, PassedCount/WarnCount/FailedCount/ChallengeCount, CheckedAt, Items[]}`。

`proxyQualityTargets`(无鉴权探测,白名单状态码判 pass):

| target | URL | 白名单状态码 |
|--------|-----|--------------|
| openai | `https://api.openai.com/v1/models` | 401 |
| anthropic | `https://api.anthropic.com/v1/messages` | 401/405/404/400 |
| gemini | `https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta` | 200 |
| grok | `https://api.x.ai/v1/models` | 401 |

- 429 → warn;Cloudflare challenge(`httputil.IsCloudflareChallengeResponse(status, headers, body)` + `ExtractCloudflareRayID`)→ **challenge**
- **计分**:`100 - warn×10 - fail×22 - challenge×30`,下限 0
- **等级**:A≥90 / B≥75 / C≥60 / D≥40 / F
- 结果存快照(Redis `proxy:latency:{id}` + DB 列),列表读取 attach

### 1.5 代理客户端(`internal/pkg/httpclient/pool.go`)

- `Options{ProxyURL, Timeout, InsecureSkipVerify, ValidateResolvedIP, AllowPrivateHosts}`
- `proxyurl.Parse` 支持 http/https/socks5/socks5h;`proxyutil.ConfigureTransportProxy` 写 transport.Proxy
- **安全控制**:`insecure_skip_verify` 不允许;URL allowlist 校验解析后 IP(禁私网)

### 1.6 过期回退

`proxy_expiry_*`:代理过期 → 按 fallback_mode(none/proxy→backup_proxy_id/direct)切换,`proxy_fallback_origin_id` 记录原代理供手动回退。**aigw v1 不做,登记长期路线**。

---

## 2. Claude OAuth 反代

### 2.1 常量(`internal/pkg/oauth/oauth.go`)

```
ClientID     = "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
AuthorizeURL = "https://claude.com/cai/oauth/authorize"
TokenURL     = "https://platform.claude.com/v1/oauth/token"
RedirectURI  = "https://platform.claude.com/oauth/code/callback"
ScopeAPI     = "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload"
ScopeInference = "user:inference"
SessionTTL   = 30min
```

PKCE:S256,verifier = 32 随机字节 base64url-nopad(43 字符),challenge = base64url(sha256(verifier))。

### 2.2 Cookie 交换 3 步(`repository/claude_oauth_service.go`)

**Step 1** `GET https://claude.ai/api/organizations`,cookie `sessionKey=<sk-ant-sid>` → `[{uuid, name, raven_type(null|team)}]`,多 org 优先 team。

**Step 2** `POST https://claude.ai/v1/oauth/{org}/authorize`(cookie 同,body `{response_type:code, client_id, organization_uuid, redirect_uri, scope, state, code_challenge, code_challenge_method:S256}`)→ `{redirect_uri}` 解析出 `code` + `state`。

**Step 3** `POST https://platform.claude.com/v1/oauth/token`(body `{code, grant_type:authorization_code, client_id, redirect_uri, code_verifier, state}`)→ `{access_token, token_type, expires_in(28800=8h), refresh_token, scope, organization.uuid, account.uuid/email_address}`。

**刷新** 同端点 `{grant_type:refresh_token, refresh_token, client_id}`。

**代理**:每步接受 proxyURL,`createReqClient` 构造带代理的 req/v3 客户端。

### 2.3 TLS 指纹(sub2api 特有,commit 7af776b7d / 67fa331f0)

- `createReqClient`:`req/v3` `ImpersonateSafari()`(默认)/Firefox/Chrome/none,env `CLAUDE_OAUTH_IMPERSONATE`
- 原因:Cloudflare 对 Chrome JA3 在 claude.ai 打 403 managed-challenge;Safari/Firefox 稳定通过
- 上游 API TLS:uTLS `HelloCustom` + 自建 ClientHello,复刻 Node.js/Claude CLI(J A3 `44f88fca...`,JA4 `t13d1714h1...`),17 套件、扩展序含 ECH、GREASE
- **aigw 推迟**:Rust 侧需 rquest/自定义 rustls ClientHello,成本高,登记长期路线

### 2.4 身份 gate 与 billing 注入(核心机制)

**身份 gate**(`docs/claude-oauth-identity-gate.md`,实测矩阵):OAuth 凭证打 `/v1/messages` 必须 `system[0]` 字节级匹配 **billing block** 或 **身份句**之一,否则 429 `rate_limit_error`。必须在首块、整块精确、与 stream 无关;缺失 `anthropic-beta: oauth-2025-04-20` → 401。

**Billing block**(`service/gateway_billing_block.go`):
- 形态 `x-anthropic-billing-header: cc_version={ver}.{fp}; cc_entrypoint=cli;`
- 指纹 `computeClaudeCodeFingerprint`(与 Parrot `src/transform/cc_mimicry.py` 字节对齐):首条 user 消息纯文本 chars[4,7,20](不足补 `'0'`)→ `SHA256("59cf53e54c78" + chars + version)` hex[:3]
- SALT `"59cf53e54c78"`;`CLICurrentVersion = "2.1.220"`
- **不带 cache_control**(与真实 CLI 一致);服务端剥离,0 billed tokens
- `syncBillingHeaderVersion`:按出站 UA 版本重写 cc_version

**注入三块默认**(`gateway_claude_oauth_body.go:887-965`):`[billing, 身份句 "You are Claude Code...", 扩展 prompt(1541 字, cache_control ephemeral 5m)]`;客户端原 system 降级为 `[System Instructions]` 消息对。**aigw 用户决策:只做最小化 billing 块(0 token),身份句/扩展 prompt 不做默认**(gate 只查 block[0])。

**可配置项**(`domain_constants.go:564-568`):`enable_claude_oauth_system_prompt_injection`(默认 true)、`claude_oauth_system_prompt`、`claude_oauth_system_prompt_blocks`(JSON 块数组)。**aigw 简化为凭证级 `inject_prompt`**(零全局设置)。

### 2.5 反代头部管线(`service/gateway_upstream_request.go:119-189`)

- OAuth → `Authorization: Bearer <access_token>`(API-key 才用 x-api-key)
- `shouldMimicClaudeCode = IsOAuth && !isClaudeCode`(UA `^claude-cli/` + metadata.user_id 判定真实 CC)→ 强制覆盖 `User-Agent: claude-cli/2.1.220 (external, cli)`、全部 `X-Stainless-*`、`x-app: cli`、`anthropic-dangerous-direct-browser-access: true`、`Accept: application/json`
- `anthropic-beta`:OAuth+mimic → 全 CC beta 集(`claude-code-20250219, oauth-2025-04-20, interleaved-thinking-2025-05-14, ...`),客户端 beta 丢弃
- 客户端头白名单透传(非 mimic 路径):9 个 exact 头;mimic 路径**完全跳过**客户端头转发(避免 x-stainless-* 冲突)
- 元数据:metadata.user_id 注入(JSON `{device_id, account_uuid, session_id}`,session_id 确定性派生)+ `X-Claude-Code-Session-Id` 头

### 2.6 Token 生命周期(sub2api)

- **缓存**:Redis `claude:account:{id}`,TTL = until(expires_at) − 5min skew,min 1min
- **刷新**:临期 3min 窗口内 `RefreshIfNeeded`:进程内 mutex + Redis 分布式锁(60s TTL)+ DB 重读(防陈旧 refresh_token)+ `_token_version` 防 fencing;`invalid_grant` race-recovery 重读 DB
- **CanRefresh**:anthropic oauth/setup-token;`NeedsRefresh`:time.Until(expires_at) < window
- **expires_at 存 unix 秒字符串**;刷新保留旧字段(`MergeCredentials`)
- **aigw 单实例简化**:进程内锁 + 内存缓存;分布式锁推迟 M2 Redis

### 2.7 sub-check(CF 检测参考,`cmd/sub-check/main.go`)

生产诊断工具:sessionKey → 3 步换 token → 最小 live probe(`POST api.anthropic.com/v1/messages`,haiku-4-5,max_tokens 1,"ping",Bearer + oauth beta)。

**verdict 分类**:200 ok / 401 token 拒(scope stripped/revoked/region)/ 403 banned / 429 rate-limited 但可用 / 5xx 上游不稳定。

**CF 识别**(`classifyStep1Error`,须在 403 分支前):`just a moment` / `cf-ray` / `cf-mitigated` / `challenges.cloudflare.com` / `attention required` / `invalid character '<'`(HTML 非 JSON)→ **"Cloudflare 正在挑战该代理 IP — 请更换出口节点(住宅/干净池)。cookie 本身未评估。"**

**其他分类**:`account_session_invalid`→cookie 已吊销;`account_disabled/suspended/terminated`→账号被封;401→cookie 过期;403→cookie/geo/IP block;429→限流。

---

## 3. aigw 复用 vs 推迟清单

| 能力 | 复用(本阶段) | 推迟(长期路线) |
|------|:---:|:---:|
| proxies 表 CRUD + in-use 守卫 | ✅(精简为整串 proxy_url) | — |
| 出口探测(ip-api/ipify 经代理) | ✅ | — |
| 质量检测目标 + 计分/等级 | ✅ | — |
| CF challenge 检测(sub-check) | ✅(新增 claude_oauth 目标) | — |
| 代理过期回退(fallback/backup) | ❌ | 长期路线 |
| OAuth 3 步交换(PKCE) | ✅ | — |
| refresh_token 刷新 + token 缓存 | ✅(进程内锁+内存) | Redis 分布式锁(M2) |
| cookie 自愈(cookie 保活) | ✅(三层策略,优于 sub2api 人工重贴) | — |
| billing 块注入(指纹算法) | ✅(默认最小化单块) | 完整三块伪装可选增强 |
| TLS 指纹模拟(uTLS/rquest) | ❌ | 长期路线 |
| 完整伪装链(tool 混淆/dateline/1h TTL/metadata) | ❌(metadata.user_id 可选) | 长期路线 |
| 提示词注入配置 | ✅(凭证级 inject_prompt) | 全局 blocks JSON 配置 |
