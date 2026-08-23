# Stage 129 Review Log

**Review Type**: Design + Code
**Review Date**: 2026-08-24
**Reviewer**: Claude（多模型评审）
**Stage**: Stage 129（CredentialsTab OAuth 前端 + 后端 refresh 端点）

---

## Review Summary

Stage 129 交付完整：后端 `POST /credential/oauth/refresh` 端点 + 前端 OAuth 凭证入口（对话框/状态徽章/Refresh/Re-auth）+ i18n + 前后端 BDD。设计评审聚焦 §2.2 refresh 端点的纳入（Stage 127 仅暴露 core 无 HTTP 端点 → 本 Stage 补齐）与安全（token 三件套永不落响应）。代码评审聚焦 redact 完整性与错误传播不泄漏凭证。

---

## Findings

### Design Review

| # | Severity | Finding | Status |
|---|----------|---------|--------|
| D1 | High | §2.2 原本写"若 Stage 127 未提供独立端点,先做触发式"——设计收敛为正式交付 `POST /credential/oauth/refresh`（前端 Refresh 按钮必须可调） | ✅ 设计已更新（§2.2 + 设计收敛注） |
| D2 | Medium | §2.1 凭证列表展示 last_error——需确认该字段永不携带 token 内容 | ✅ 已核实：`mark_needs_reauth` 写入的是 `OauthError.kind + message`（静态/分类文案），不含凭据 |

### Code Review

| # | Severity | Finding | Status |
|---|----------|---------|--------|
| C1 | Critical | `claude_token.rs cookie_self_heal` 对已解密的 `session_key` 二次 `decrypt_litellm_value` → base64 Invalid padding → cookie 自愈永远不可达（Config 错误替代 NeedsReauth） | ✅ 修复：直接用 `values["session_key"]`（`get_access_token` 已解密）。409 BDD 场景暴露此隐藏缺陷 |
| C2 | High | `oauth_refresh` 端点失败分支需返回 `kind=oauth_refresh_failed` 以便前端区分 409 | ✅ 已返回 `{error:{kind:"oauth_refresh_failed"}}` + 3 个 mock BDD 场景覆盖 |
| C3 | Medium | `oauth_refresh` 响应需完整 redact token 三件套 | ✅ `redact_oauth_credential_values` 在成功路径应用 + BDD "敏感字段已 redact" 断言 |
| C4 | Low | 前端 last_error 渲染 `slice(0,48)`——避免超长错误撑爆行 | ✅ 已截断 |

### Security Review（独立审计）

| 项 | 结论 |
|----|------|
| access/refresh/session_key 落响应 | ✅ 全部 `redact_oauth_credential_values` → `***` |
| session_key 落日志 | ✅ `credentials.rs` 仅 warn "does not start with sk-ant-"（不打印值）；exchange/refresh 无 token 日志 |
| 错误传播泄漏 | ✅ `OauthError.message` 均为分类文案（401/403/429/cf_challenge 等），不内嵌 secret |
| last_error 落库 | ✅ 同为 OauthError 文案，无凭据 |
| 前端 DOM | ✅ 仅渲染 status/expires_at/proxy 名/last_error 截断；token 三件套无任何 `t()`/state 暴露 |
| 测试 mock | ✅ 仅测试文件含 fake sk-ant 值（mock 数据，非真实） |

---

## Resolution Summary

**Total Findings**: 7
**Fixed**: 7
**Deferred**: 0
**Wont Fix**: 0

**All Critical Fixed**: Yes
**All High Priority Addressed**: Yes

**Test Evidence**: 后端 mock BDD 278 场景（265 pass / 13 skip）+ 前端 fe-bdd 387 pass / 3 skip（0 fail）；fmt + lint + fe-build + fe-lint 全绿。
