# Stage 130 Review Log

**Review Type**: Design
**Review Date**: 2026-08-24
**Reviewer**: aigw-core (claude)
**Stage**: Stage 130 — Phase 51 收尾（real BDD + 安全审计 + 文档）

---

## Review Summary

Stage 130 是纯收尾/验证 Stage：无新功能代码（TD-015a 是既有 OAuth 分支错误体透传修复，非新增），核心是 **real BDD 三后端 OAuth 凭证 CRUD 覆盖** + **安全审计 8 项复核** + **ADR-034 / roadmap / next-steps 文档回写**。设计文档 `docs/stages/stage-130.md` 结构完整、范围清晰。预检发现 3 处需细化/修正的执行层问题，均在 Gate 3 解决。

## Findings

### F1（High → Gate 3 解决）: mock 上游不可达，real BDD OAuth 交换场景需走"预种子 + 只读重放"而非实时交换

- **位置**: stage-130.md §2.1 + 2.2；预检 `claude_oauth_steps.rs` / `mock_upstream.rs` / `ServerGuard`（server.rs）
- **描述**: `AIGW_OAUTH_MOCK_BASE` / `AIGW_ANTHROPIC_MOCK_BASE` 读取点在 aigw-core（claude_oauth.rs:52 / oauth_pipeline.rs:55），`feature = "test"` 门控。real BDD 启动的 aigw **二进制**（`aigw-server` 正常依赖 `aigw-core` **不带** `test` feature，dev-dependency 的 `test` feature 只用于 bdd 测试 crate 内联 handler 调用），且 `ServerGuard::start` 未透传这两个变量。**实时 exchange 在 real 模式必然失败（403）**。
- **方案**：real feature 场景改为 **全 HTTP CRUD + 只读重放**——OAuth 凭证通过 `/credential/new`预加密明文 token 值，明文仅用于测试，非真实密钥）预种子；随后经 `/credential/list` / `/credential/info` 断言 `***` redact + 字段保留；`/credential/delete` 断言删除。in-use 守卫 + probe 快照复用 Stage 125 real BDD 既有模式（credential_new 引用 proxy_id → 删代理 409 / 探测 200|500）。exchange/refresh/401 重试等实时路径**已在 mock BDD 充分覆盖**，real 层只验证 CRUD + 加密落库 + redact + in-use，完全命中 stage-130 §2.1 验收目标。
- **处置**: 设计文档 §2.1 明确标注该实现约束。

### F2（High → Gate 3 解决）: 安全审计"日志无明文 token/cookie"需在 tracing 层补 redact 助手

- **位置**: stage-130.md §2.2 审计项"日志"
- **描述**: 审计清单要求 OAuth 交换/刷新/自愈路径日志不打 token/cookie 明文。预检确认 aigw-core 这些路径的 `tracing::error!` 均为红action 级封装错误（`PipelineError`），不含 token/cookie 明文；handler 层 error_msg 透传上游错误体。**唯一可注入点**：OAuth 分支 `format!("Upstream returned {}: {}", status, error_body)` 将上游错误体原文拼入客户端响应（chat.rs:1241/1392、responses.rs:343/486、v1_messages.rs:540/693）——上游错误体**可能**包含鉴权失败 detail 但**不含** access_token（token 只在 Authorization 头）。审计判定：OAuth 分支收窄为「仅透传上游错误 message，不拼原始 body」（TD-015a 修复），非 OAuth 分支保持既有全库模式不扩。
- **处置**: TD-015a 作为本 Stage 唯一代码改动。

### F3（Medium → Gate 3 解决）: 审计"DB 无明文"需要在 BDD 里加 DB 层只读断言，而非仅依赖代码路径审查

- **位置**: stage-130.md §2.2 "sk-ant-sid cookie 落库必须加密"
- **描述**: 仅靠代码审查 `build_oauth_credential_values` 用 `encrypt_litellm_value`（v2:gcm:）加密 token 三件套不构成"DB 无明文"的**可复现验证**。需在 real feature 里对预种子 OAuth 凭证做 **DB 层直读断言**：`credential_values` 序列化不含 `sk-ant-`/明文 secret 子串，且含 `v2:gcm:` 前缀。
- **处置**: real feature 加「凭证密文落库」场景。

## AI Pre-Filter Results

- **过滤（非发现）**: 无 "exchange 走 mock 上游" 的无效建议——mock 不可达已由 F1 覆盖。
- **过滤（低价值）**: 审计项 "proxy_url 存储加密" 复用 Stage 125 已验收（proxies 表 + `encrypt_proxy_url` + redact），不在 Stage 130 重复验证清单外扩。

## Resolution Summary

**Total Findings**: 3
**Fixed**: 3（F1 real 场景约束 / F2 TD-015a 修复 / F3 DB 直读断言）
**Deferred**: 0
**Wont Fix**: 0

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes

---

## Code Review（Stage 130, Gate 4）

**Review Type**: Code
**Review Date**: 2026-08-24
**Reviewer**: aigw-core (claude)

### Files Reviewed
- `crates/aigw-core/src/crypto.rs`（新增 `encrypt_litellm_value_gcm` + 2 UT）
- `crates/aigw-server/src/routes/credentials.rs`（`credential_new` OAuth 落库加密 + 2 UT）
- `crates/aigw-server/src/routes/chat.rs`（`upstream_error_message` + TD-015a 3 处接线 + 1 UT）
- `crates/aigw-server/src/routes/{v1_messages,responses,embeddings}.rs`（TD-015a 错误体收窄）
- `crates/aigw-server/tests/bdd_steps/real_oauth_steps.rs` + `features/real/claude_oauth_crud.feature` + `mod.rs`

### Findings

#### F4（High → Fixed）: `/credential/new` 对 OAuth 凭证不加密 token 三件套

- **位置**: credentials.rs `credential_new`（Stage 130 安全审计第 1/2 项的直接缺口）
- **描述**: `/credential/new` 直接 `insert_credential(body.credential_values.clone())`，`anthropic_oauth` 凭证的 access/refresh/session_key 以明文落库。`oauth_exchange` 已加密，但通用创建路径遗漏。real BDD DB 直探针立即暴露（`got: {"access_tokensk-ant-access-real",...}`）。
- **修复**: `credential_new` 对 `type=="anthropic_oauth"` 走 `encrypt_oauth_credential_values`（AES-256-GCM `v2:gcm:` 逐字段加密，经 `aigw-core::crypto::encrypt_litellm_value_gcm`）+ 2 UT + real BDD 探针断言。三后端 58/58 全绿。

#### F5（High → Fixed）: TD-015a 错误体透传未收窄

- **位置**: chat/v1_messages/responses/embeddings 的 OAuth 分支 `format!("Upstream returned {}: {}", status, error_body)` 把上游错误体原文拼给客户端。
- **修复**: 新增 `chat::upstream_error_message(status, body)`——只提取上游 `error.message`（Anthropic object / string 双形状），parse 失败退化为 `Upstream returned HTTP {status}`；4 handler 全部接线（含 embeddings 非 OAuth 分支保持 message 提取 + 不扩）。+1 UT。

#### F6（Medium → Fixed）: `credential_values` 解密对明文敏感字段的鉴别

- **位置**: `CredentialResponse::decrypt_credential_values` / `decrypt_json_fields` 对明文 `sk-ant-*` 值 try-decrypt 失败后原样保留——响应 redact 仍会 mask（`redact_oauth_credential_values` 按 key 掩码），但 DB 明文是根因，F4 已消除。加 UT 锁定 `rotate_json_fields` 不吞明文（防未来 regress）。

#### F7（Info → Documented）: 上游错误体落 spend_logs（TD-015c）保留

- 上游响应原文写入 `spend_logs.response` 是全库既有模式；access_token 只走 Authorization 头、永不进 body。保持原样，TD-015c 继续 P3。

### Triangulation Verification
- **每项 findings 独立以代码定位验证**（非仅评审模型共识）：F4 由 real BDD DB 直读探针实证（3 后端复现明文→修复后 `v2:gcm:`）；F5 由 grep 全库 13 处 `Upstream returned` + 逐一确认 OAuth 分支接线；F6 由新增 UT + decrypt 路径代码审查。
- **基线回归**：mock BDD 278（265 pass / 13 skip）、real BDD 三后端 58/58 × 3、aigw-core 502 + aigw-server 157 + aigw-migrate 27 UT、fmt + clippy `-D warnings` green。

### Resolution Summary
**Total Findings**: 4
**Fixed**: 3（F4 / F5 / F6）
**Documented**: 1（F7 → TD-015c 维持 P3）
**Deferred**: 0

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes
