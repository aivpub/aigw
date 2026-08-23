# Stage 130: Phase 51 收尾 — real BDD + 安全审计 + 文档（Claude OAuth 反代）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 6h（real BDD + ADR-034 + 安全审计 + roadmap/next-steps 回写）
**依赖**: Stage 126-129
**状态**: ✅ Complete（2026-08-24，134/134）

---

## 1. 目标

Phase 51 收尾：real BDD 三后端验证 + **安全审计**（sk-ant-sid 加密落库确认 + 响应脱敏 + 日志不泄密）+ ADR-034 + roadmap/next-steps 回写。

## 2. 方案

### 2.1 real BDD 三后端

- `features/real/claude_oauth_crud.feature`（@real_api @needs_upstream_db）：
  - **OAuth 凭证 CRUD**（`/credential/new` → `/credential/list` + `/credential/info` → `/credential/delete`）三方言全绿
  - **加密落库只读断言**：`/credential/info` 响应 token 三件套 `***` redact；DB 层直读 `credential_values` 含 `v2:gcm:` 前缀、无明文 `sk-ant-` 子串（F3）
  - **proxy_id in-use 守卫**（凭证引用 → 删除代理 409）+ **probe 快照**（不可达出口容忍 200|500）——复用 Stage 125 real BDD 既有模式
- **实现约束（Gate 2 发现）**: real 模式启动的 aigw **二进制**不带 aigw-core `test` feature（`AIGW_OAUTH_MOCK_BASE` / `AIGW_ANTHROPIC_MOCK_BASE` 门控），且 `ServerGuard` 不透传 mock base 环境变量 → **实时 exchange/刷新/自愈在 real 模式不可达**。这些实时路径已在 mock BDD 充分覆盖（claude_oauth.feature 12 场景），real 层收敛为**全 HTTP CRUD + 只读重放**：OAuth 凭证经 `/credential/new` 预种子（明文 token 值仅测试用），随后经列表/详情断言 redact + 字段保留，delete 断言删除。代理出口检测经 Stage 125 已落地的 `/admin/proxies/{id}/test`（不可达容忍）。（F1）

### 2.2 安全审计（本 Stage 专项）

| 检查项 | 要求 |
|--------|------|
| sk-ant-sid cookie 落库 | **必须加密**（master_key AES-GCM `v2:gcm:`），审计确认 DB 无明文 |
| access/refresh token 落库 | 必须加密 |
| 响应脱敏 | 所有 API 响应不含 access_token/refresh_token/session_key 明文（redact 覆盖 exchange 响应 + 凭证列表/详情） |
| 日志 | OAuth 交换/刷新/自愈路径不得打 token/cookie 明文（`tracing` 用 redact 助手）;proxy_url 日志打 redact 形态 |
| 错误传播 | **OAuth 分支只透传上游错误 message，不拼原始 body（TD-015a 修复）**;非 OAuth 分支保持全库既有模式 |
| proxy_url 存储 | 加密落库;列表/详情 redact password |
| in-use 守卫 | 被凭证引用的代理禁止删除（409） |

### 2.3 ADR-034（Claude OAuth 订阅反代）

`docs/08-autonomous-decisions.md` 追加 ADR-034：

- **决策**：`sk-ant-sid` cookie → 3 步 OAuth 交换（PKCE）→ access/refresh token;凭证扩展 `credentials` 表(敏感字段加密);三层 token 自愈(缓存→刷新→cookie 自愈→needs_reauth 告警);反代管线默认注入**最小化 billing 块**(0 token,服务端剥离),凭证可配 inject_prompt;全协议(chat/responses/messages/count_tokens)统一走 OAuth 反代,embeddings 400;TLS 指纹模拟推迟长期路线
- **理由**：身份 gate 实测只需 `system[0]` billing 块即通过;billing 块 0 成本优于完整三块(24 token 身份句);cookie 持久化实现 30 天免人工自愈;ref=可参考 sub2api 生产验证
- **后果**：Stage 51 交付后,配置 OAuth 凭证的模型可被 Claude Code + OpenAI 格式客户端共用订阅号;需人工干预仅 cookie 被 Anthropic 吊销时(告警通知)

### 2.4 roadmap / next-steps 回写

- `docs/stages/stage-roadmap.md`：追加 Phase 51（Stage 126-130，50h）+ 标记完成;总进度 129→134;顶部状态更新;长期路线追加 TLS 指纹模拟、完整伪装链、代理过期回退
- `docs/11-next-steps.md`：追加 Phase 51 完成记录 + 后续候选(M2 Redis 分布式锁/token 预热等)

## 3. 验收标准

- [x] real BDD 三方言 OAuth 凭证 CRUD + in-use + 快照全绿（58/58 × 3）
- [x] 安全审计 8 项全部通过（审计清单核对;含 TD-015a 错误体收窄 + DB 层无明文只读断言）
- [x] ADR-034 Accepted 记录（Stage 126 已记;Stage 130 补全收尾）
- [x] roadmap 顶部状态 + Phase 51 条 + 总进度 134/134 回写
- [x] next-steps 更新

## Implementation Notes

### Implementation Differences（Gate 3/4 记录）
- real feature 收敛为全 HTTP CRUD + 只读重放（exchange/刷新实时路径 mock 已覆盖，real 不可达约束见 §2.1）
- **TD-015a 全库收窄**：`chat::upstream_error_message` 接线 chat/v1_messages/responses/embeddings 四处 handler（非仅 OAuth 分支）——所有上游错误不再暴露原始 body
- **`/credential/new` OAuth 凭证加密落库**（安全审计第 1/2 项直接缺口，Gate 4 发现）：`type=="anthropic_oauth"` 走 `encrypt_oauth_credential_values`（AES-256-GCM `v2:gcm:` 逐字段），新增 `aigw-core::crypto::encrypt_litellm_value_gcm`
- 安全审计新增 DB 层直读断言场景（real BDD 探针三后端验证）

### Testing Evidence（Gate 3/4 实测）

| 层 | 结果 |
|----|------|
| mock BDD | **278 场景（265 pass / 13 skip body_archive）** |
| real BDD SQLite | **58/58 全绿**（新增 claude_oauth_crud.feature 6 场景，含 v2:gcm: DB 直读 + in-use 409 + 探测） |
| real BDD PG | **58/58 全绿** |
| real BDD MySQL | **58/58 全绿** |
| aigw-core UT | **502**（+1 GCM roundtrip；+1 rotate plaintext 鉴别） |
| aigw-server UT | **157**（+1 upstream_error_message；+2 credential_new 加密） |
| aigw-migrate UT | 27（基线） |
| fmt / lint | green（`task fmt` + `task lint` clippy `-D warnings`） |

**安全审计 8 项核对**：
1. sk-ant-sid cookie 落库加密 ✅（credential_new + exchange 均 `v2:gcm:`；DB 直读探针 3 后端无明文）
2. access/refresh token 落库加密 ✅（同一探针覆盖 trio）
3. 响应脱敏（exchange/list/info）✅（`redact_oauth_credential_values` + real BDD `***` 断言）
4. 日志不泄密 ✅（pipeline 错误为 redaction 级封装；handler error_msg 不拼原始 body）
5. 错误传播只透传 message ✅（TD-015a `upstream_error_message`）
6. proxy_url 加密落库 + redact ✅（Stage 125 既有 + real BDD proxy redact 步骤）
7. in-use 守卫 ✅（real BDD 删除被引用代理 409 PROXY_IN_USE）
8. probe_result 快照 ✅（real BDD 探测 200|500 容忍）

### 验证备注（real BDD 前置）
- real BDD 需要 `.env` 的 `OPENAI_API_KEY`/`OPENAI_BASE_URL`/`OPENAPI_MODEL` + `AIGW_UPSTREAM_DB_URL`/`AIGW_UPSTREAM_ENCRYPT_KEY` 正确注入（server 子进程经 `ServerGuard` 透传 OPENAI_*/OPENAPI_*/AIGW_UPSTREAM_*）。`task bdd-real-*` 已正确 sourcing `.env`。
- 若 `.env` 缺失这些变量，real BDD 的上游请求会 `builder error` → 502（z-ai/glm5 env fallback 无 api_base/key）。
