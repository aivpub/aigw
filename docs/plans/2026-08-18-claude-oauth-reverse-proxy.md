# Claude OAuth 订阅反代 + 代理服务管理 — 总体规划

> 规划日期:2026-08-18 | 用途:Phase 50/51 实施蓝图
> 参考实现:`~/works/play/sub2api`(Go,代理管理 + Claude OAuth 反代)
> 技术调研:`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md`
> 状态:⏳ 规划完成,待实施(本文档仅规划,不实施)

---

## 1. 一句话目标

aigw 分两个 Phase 补齐两块能力:

1. **Phase 50 代理服务管理**:系统配置中新增代理服务管理(CRUD + 出口检测 + 质量检测),参考 sub2api 的 `proxies` 表 + 探测服务实现。
2. **Phase 51 Claude Code OAuth 订阅反代**:凭证管理支持 `sk-ant-sid***` cookie 换取 access/refresh token,凭证绑定代理 IP,模型解析到 OAuth 凭证时经代理出口以 Bearer access_token 打到 Anthropic `/v1/messages`,对 CC 订阅账号默认注入 billing header(凭证可配置注入提示词)。

**核心主张**:代理先行(Phase 50),OAuth 反代(Phase 51)建立在代理之上——凭证绑代理、cookie→token 交换走代理出口、质量检测都依赖代理管理。

---

## 2. 用户决策记录(2026-08-18 确认)

| 决策点 | 选项 | 选定 | 理由 |
|--------|------|------|------|
| 提示词注入默认形态 | 最小化 billing 块 / 完整 CC 三块 / 全局可切换 | **最小化 billing 块** | Anthropic 身份 gate 只需 `system[0]` 是 billing 块即通过(实测,见 `docs/research`);billing 块 0 token 成本、服务端剥离。凭证可另行配置 inject_prompt 追加提示词 |
| TLS 指纹模拟 | 本阶段纳入 / 推迟 / 永久不做 | **推迟到后续阶段** | 本阶段 HTTP 层伪装(UA/Stainless 头/body)已够初步可用;Rust 侧 rquest/自定义 rustls ClientHello 成本高,登记长期路线 |
| Token 刷新策略 | 仅 refresh / cookie 保活 / **三层自愈** | **三层自愈** | access(8h)+refresh(30 天轮换)+cookie **三者都存**;请求临期优先 refresh 刷新,refresh 失效自动回退 cookie 重走 3 步,再失效标记 needs_reauth + 告警 |
| 凭证存储 | 扩展 credentials 表 / 独立账号表 / credentials+薄状态表 | **扩展 credentials 表** | credential_values JSON 加 OAuth 结构化字段,proxy_models 经现有 `litellm_credential_name` 引用。零新表,复用现有加密/redact/aigw-migrate |
| 端点范围 | 仅原生 / **含 chat/responses 转换** / 仅 messages | **含 chat/responses 转换** | 任何入站协议,只要解析到 OAuth 凭证就统一走 OAuth 反代管线(经代理→注入→Bearer 打 /v1/messages);非 OAuth 部署原样不动。embeddings → 400(Anthropic 无 embedding 端点) |
| proxies 表形态 | 细字段拆分 / **整串 proxy_url** | **整串 proxy_url** | reqwest 天然吃 `scheme://user:pass@host:port` 字符串;proxy_url 整串 AES-GCM 加密落库,比 sub2api 明文存 password 进步 |
| 检测结果存储 | 多快照列 / **单 JSON 字段** | **单 JSON 字段** | `probe_result` JSON 承载延迟/出口/分数/等级/逐项;status 顶层列仅用于过滤 |
| 质量检测 CF 项 | — | **加入 `claude_oauth` 目标** | 生产实测请求被 CF 拦截;按 sub-check 的 CF 识别逻辑在质量检测中探测 `claude.ai/api/organizations` 这条最敏感路径 |

---

## 3. 依赖关系

```
Phase 50 (Stage 122-125) 代理服务管理 ──► Phase 51 (Stage 126-130) OAuth 反代
      │                                            ▲
      │   proxy_url 加密/解密 / 代理出口客户端 /    │
      │   质量检测(含 CF challenge)                │ 凭证绑代理 / 交换走代理 / 反代走代理出口
      └────────────────────────────────────────────┘
```

**Phase 51 强依赖 Phase 50**:OAuth 凭证 `proxy_id` 引用、cookie→token 交换走代理出口、反代管线代理出口、质量检测的 claude_oauth 目标全部建立在 proxies 表 + 代理 HTTP 客户端之上。

---

## 4. Phase 50:代理服务管理(Stage 122-125,~44h)

### 4.1 背景

sub2api 的代理管理(CRUD + 出口/质量检测)已生产验证,是 OAuth 反代的底座。aigw 当前零代理能力——reqwest 客户端无代理配置、无 proxies 表、无检测端点。

### 4.2 数据模型(精简版)

`proxies` 表(Migration 027 × 3 方言):

```sql
CREATE TABLE proxies (
    id           INTEGER PRIMARY KEY,          -- 三方言各自类型(INTEGER/BIGSERIAL/BIGINT)
    name         TEXT NOT NULL,
    proxy_url    TEXT NOT NULL,                -- 整串加密落库(master_key AES-GCM,v2:gcm: 前缀),如 socks5://user:pass@host:1080
    status       TEXT NOT NULL DEFAULT 'active',  -- active / inactive / expired
    expires_at   TEXT,                         -- NULL=永不过期;status=expired 由它派生
    probe_result TEXT NOT NULL DEFAULT '{}',   -- 检测快照 JSON(修订2)
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**明确不做**(与 sub2api 差异):不拆分 protocol/host/port/username/password 细字段;不做 fallback_mode/backup_proxy_id/expiry_warn_days(过期回退机制 v1 不做,登记长期路线)。

### 4.3 probe_result JSON 形态

```json
{
  "latency_ms": 320,
  "exit_ip": "1.2.3.4", "country": "香港", "country_code": "HK",
  "region": "Hong Kong", "city": "Hong Kong",
  "score": 88, "grade": "B",
  "overall_status": "healthy" | "warn" | "failed" | "challenge",
  "items": [
    {"target": "base_connectivity", "status": "pass", "latency_ms": 320, "message": "代理出口连通正常"},
    {"target": "openai", "status": "pass", "http_status": 401, "message": "HTTP 401（目标可达）"},
    {"target": "anthropic", "status": "pass", "http_status": 400, "message": "HTTP 400（目标可达）"},
    {"target": "claude_oauth", "status": "pass", "http_status": 200, "message": "claude.ai 无 challenge，OAuth 路径可达"},
    {"target": "gemini", "status": "warn", "http_status": 429, "message": "目标返回 429，可能存在频控"},
    {"target": "grok", "status": "pass", "http_status": 401}
  ],
  "last_check_at": "2026-08-18T08:00:00Z"
}
```

`status` 顶层列用于过滤;延迟/分数/国家展示从 `probe_result` 内存解析(admin 列表量小,不做 JSON 列跨方言排序)。PostgreSQL 用 JSONB,SQLite/MySQL 用 TEXT。

### 4.4 API(`/admin/proxies/*`,复用 `require_admin`)

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/admin/proxies` | 分页 + 过滤(protocol 由 proxy_url scheme 派生 / status / search / sort) |
| GET | `/admin/proxies/all` | 全部 active,下拉用(凭证绑定代理、前端 Select) |
| POST | `/admin/proxies` | 创建;proxy_url 加密;创建后 `tokio::spawn` 异步探测 |
| GET | `/admin/proxies/{id}` | 详情(含解密后 URL,响应 redact password) |
| PUT | `/admin/proxies/{id}` | 更新(整串替换);同步触发异步重探测 |
| DELETE | `/admin/proxies/{id}` | 删除;**in-use 守卫**:JSON 扫描 credential_values.proxy_id,有引用 → 409 |
| POST | `/admin/proxies/{id}/test` | 出口检测(IP + 延迟),结果存 probe_result |
| POST | `/admin/proxies/{id}/quality` | 质量检测(逐项 + 分数 + 等级),结果存 probe_result |
| POST | `/admin/proxies/{id}/toggle` | active ↔ inactive |
| POST | `/admin/proxies/batch-test` | 批量出口检测 |
| POST | `/admin/proxies/batch-quality` | 批量质量检测 |
| POST | `/admin/proxies/batch-delete` | 批量删除(in-use 跳过 + skipped 列表) |

### 4.5 检测服务

**出口探测**:经代理 GET `ip-api.com/json`(备选 `api64.ipify.org`)→ 出口 IP/国家/城市/延迟;两个 URL 优先级探测,全部失败报错。

**质量目标**(复刻 sub2api `runProxyQualityTarget` + 修订 3 新增 CF 项):

| target | URL | 判定 |
|--------|-----|------|
| base_connectivity | ip-api(出口) | pass=fail 出口信息获取成功 |
| openai | `https://api.openai.com/v1/models` | 401/200 pass;429 warn;其余 fail |
| anthropic | `https://api.anthropic.com/v1/messages` | 401/405/404/400 pass;429 warn;其余 fail |
| **claude_oauth** | `https://claude.ai/api/organizations` | 见下(CF challenge 检测) |
| gemini | `https://generativelanguage.googleapis.com/$discovery/rest?version=v1beta` | 200 pass;429 warn |
| grok | `https://api.x.ai/v1/models` | 401 pass;429 warn |

**claude_oauth CF 检测**(按 sub-check `classifyStep1Error` + 响应体嗅探):
- CF 签名命中(`just a moment` / `cf-ray` / `cf-mitigated` / `challenges.cloudflare.com` / `attention required` / 返回 HTML 即 `invalid character '<'`)→ `status="challenge"`,message **"Cloudflare 正在挑战该代理 IP — 请更换出口节点(住宅/干净池),cookie 本身未评估"**
- 200 + JSON → `pass`
- 403 非 CF / 401 → `fail`(区分 message)
- 计入 `challenge` 计数 → 分数扣 30、等级降级

**计分**:`100 - warn×10 - fail×22 - challenge×30`(下限 0)→ 等级 A(≥90)/B(≥75)/C(≥60)/D(≥40)/F(<40)。

**CF challenge 工具**:复用 sub2api `httputil.IsCloudflareChallengeResponse`(状态码 + 头 + body)与 `ExtractCloudflareRayID` 语义。

### 4.6 reqwest 代理支持

`reqwest` 加 `socks` feature(workspace 依赖),`Client::builder().proxy(Proxy::all(proxy_url)?)`;支持 http/https/socks5/socks5h。**配置客户端与默认重试客户端分离**:代理客户端由 Pipeline 单独构造(带代理 + 超时),不混入 `build_retry_client`。

### 4.7 前端

Settings 分组新增「Proxies」页(`/dash/proxies`):表格(名称/出口 IP·国家/延迟/分数等级/状态/到期)、创建/编辑对话框(名称 + proxy_url + expires_at)、Test/Quality 按钮 + 分数徽章 + 逐项展开、批量测试/质量/删除、status 徽章 + toggle;i18n 全量。

---

## 5. Phase 51:Claude OAuth 订阅反代(Stage 126-130,~60h)

### 5.1 凭证扩展(扩展 credentials 表,零新表)

`credential_values` JSON 新增 OAuth 结构化字段(敏感字段加密落库):

```json
{
  "type": "anthropic_oauth",
  "access_token": "<enc>", "refresh_token": "<enc>", "session_key": "<enc>",
  "expires_at": 1752900000, "proxy_id": 3, "inject_prompt": null,
  "org_uuid": "...", "account_uuid": "...", "email_address": "...",
  "status": "active" | "needs_reauth", "last_error": null
}
```

- 敏感字段(access_token/refresh_token/session_key)用 master_key AES-GCM 加密落库(与 proxy_url 同机制)
- proxy_id/inject_prompt/org_uuid/status 明文(用于 resolver/列表/告警)
- proxy_models 经现有 `litellm_credential_name` 引用 → resolver 判定 `type=="anthropic_oauth"` 即 OAuth 部署
- 响应 redact:现有 `SensitiveCredentialKeys` 需补 `access_token`/`refresh_token`/`session_key`(查证现有列表是否覆盖,不足则补)

### 5.2 Cookie→Token 交换(Stage 126)

`POST /credential/oauth/exchange`(admin),body `{session_key, proxy_id?, inject_prompt?, name}`:

**Step 1** — `GET https://claude.ai/api/organizations`(cookie `sessionKey`)= `<sk-ant-sid>`,走绑定代理 → org UUID(多 org 优先 `raven_type=="team"`)

**Step 2** — `POST https://claude.ai/v1/oauth/{org}/authorize`(PKCE S256 + state,body 含 client_id `9d1c250a-e61b-44d9-88ed-5944d1962f5e` / redirect_uri / scope / code_challenge)→ auth code(从 redirect_uri 解析)

**Step 3** — `POST https://platform.claude.com/v1/oauth/token`(grant `authorization_code`,code_verifier,state)→ `{access_token, refresh_token, expires_in(28800=8h), organization.uuid, account.uuid/email_address}`

- 全程走绑定代理(reqwest client 带 proxy_url)
- scope:`user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload`
- OAuth HTTP 客户端独立构造(带代理 + 60s 超时 + UA 伪装,不混入网关重试客户端)

### 5.3 Token 生命周期 + 自愈(Stage 127,三层)

**存储**:access + refresh + cookie 三者都存(加密)。

**请求路径获取有效 access_token**(进程内锁 + 内存 token 缓存,key = credential_id):
1. 内存缓存命中 → 直接用
2. access 临期(3min 窗口)或缓存未命中 → **refresh_token 刷新**(`POST platform.claude.com/v1/oauth/token`,grant `refresh_token`,带代理)→ 写回缓存 + DB(合并保留旧字段,写 `_token_version` 防陈旧)
3. refresh 失败 `invalid_grant` → **自动回退存储 cookie 重走 3 步**(Section 5.2)→ 自愈,无需人工
4. cookie 也失效 → `status="needs_reauth"` + 复用现有 `alert_webhook` 告警 + 请求返回 401

**管线内 401**:强制刷新一次 + 重试;再 401 → 按上述自愈链。

**多实例**:进程内锁 + 内存缓存,单实例足够;分布式锁(Redis)推迟 M2 分布式层。

### 5.4 反代管线(Stage 128)

**判定**:resolver 解析到 OAuth 凭证 → `Deployment` 带 `oauth` 引用(access_token 获取器 + proxy_url + inject_prompt)。**任何入站协议,只要目标是 OAuth 部署**,统一:

1. 解析有效 access_token(缓存→刷新→cookie 自愈)
2. 统一打向 `https://api.anthropic.com/v1/messages`
3. 头部:`Authorization: Bearer <access_token>`(非 x-api-key)+ CC 伪装头(`User-Agent: claude-cli/2.1.220 (external, cli)`、`X-Stainless-*`、`x-app: cli`、`anthropic-version: 2023-06-01`、`anthropic-beta: oauth-2025-04-20, claude-code-20250219`)
4. **Billing 块注入(默认最小化)**:`system[0]` = `x-anthropic-billing-header: cc_version=<ver>.<fp>; cc_entrypoint=cli;`
   - 指纹算法(字节对齐 sub2api/Parrot):SALT `"59cf53e54c78"` + 首条 user 消息 text 的 chars[4,7,20](不足补 `'0'`)→ `SHA256(SALT+chars+version)` hex[:3]
   - billing 块**不带 cache_control**,0 token 成本、服务端剥离
   - 客户端原 system 块保留在后(`[System Instructions]` 降级为消息对可选,本阶段只做最小注入,原 system 不动)
   - **若凭证配置了 inject_prompt,追加为额外 block**(gate 只查 block[0],之后不受限)
5. 出口:reqwest client 带凭证绑定代理(http/socks5)
6. 协议转换:`/v1/chat/completions` → 现有 `OpenAIToAnthropic`;`/v1/responses` → 现有 `ResponsesToChat` 转 Anthropic 格式 → 同一管线
7. `/v1/messages/count_tokens` → Bearer + `token-counting-2024-11-01` beta
8. `/v1/embeddings` → **400**(Anthropic 无 embedding 端点,OAuth 凭证不可用)

**不做(登记长期路线)**:TLS 指纹模拟(uTLS/rquest ClientHello 伪装);tool 名混淆;dateline 归一化;1h cache TTL 注入——均为 sub2api 完整伪装链的增强项,本阶段只做身份 gate 必需的最小集合。

### 5.5 前端(Stage 129)

CredentialsTab 新增 OAuth 入口:粘贴 sk-ant-sid + 代理下拉(`/admin/proxies/all`)+ 注入提示词 textarea + 提交;列表展示 token 到期时间、needs_reauth 徽章、Refresh/Re-auth 按钮;i18n 全量。

---

## 6. 测试策略

| 层 | 覆盖 |
|----|------|
| core UT | proxies store ×3 方言、proxy_url 加解密、检测计分/等级、CF 识别、billing 指纹(字节对齐)、token 缓存/刷新/自愈、resolver OAuth 判定 |
| handler UT | /admin/proxies CRUD + in-use 守卫、/credential/oauth/exchange、反代管线 401 刷新重试 |
| mock BDD | proxies.feature(CRUD/test/quality/批量/in-use 409)、claude_oauth.feature(exchange + 反代注入 + 代理出口 + chat 转换 + count_tokens + embeddings 400 + needs_reauth) |
| 前端 BDD | proxies 页 + CredentialsTab OAuth 入口,× 3 viewports |
| real BDD | 三后端(sqlite/pg/mysql)proxies CRUD + 检测快照落库 |

**Mock 上游**:新增 Anthropic OAuth 模拟(claude.ai organizations/authorize + platform.claude.com token + api.anthropic.com messages),供 BDD 场景走真实管线。

---

## 7. 交付文档清单

```
docs/plans/2026-08-18-claude-oauth-reverse-proxy.md              ← 本文档(总体规划)
docs/research/2026-08-18-sub2api-proxy-oauth-reference.md        ← 参考实现技术调研
docs/stages/stage-122.md ~ stage-130.md                          ← 9 份 Stage 设计文档
docs/stages/stage-roadmap.md                                     ← Phase 50/51 追加(125→134)
docs/11-next-steps.md                                            ← 下一步更新
docs/08-autonomous-decisions.md                                  ← ADR-033(代理表+加密)/ ADR-034(OAuth 注入默认)
```

---

## 8. 修订记录

| 版本 | 日期 | 内容 |
|------|------|------|
| v1.0 | 2026-08-18 | 初稿:两 Phase 9 Stage,代理先行 + OAuth 反代 + 三层 token 自愈 + 最小化 billing 注入 |
| v1.1 | 2026-08-18 | 用户 4 项修订:① proxies 表不拆细字段,改整串 proxy_url 加密落库;② 检测结果收单 JSON 字段 probe_result;③ 质量检测加 claude_oauth 目标(CF challenge 检测,参考 sub-check);④ 其余规划无异议 |
