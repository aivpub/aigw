# Stage 128 Review Log — Claude OAuth 反代管线

**Stage**: 128（Phase 51）
**日期**: 2026-08-20
**状态**: ✅ 完成（Gate 4/5 通过）

---

## 1. 交付总览

| 模块 | 变更 |
|------|------|
| `crates/aigw-core/src/deployment.rs` | `Deployment` 新增 `oauth: Option<OAuthDeployment>`；`OAuthDeployment` struct（credential_id / proxy_url / inject_prompt） |
| `crates/aigw-core/src/resolver.rs` | `resolve_one` OAuth 判定：credential `type=="anthropic_oauth"` → 填 `oauth`（解密 proxy_url + inject_prompt） |
| `crates/aigw-core/src/oauth_pipeline.rs` | **新建** — billing 指纹（字节对齐 sub2api/Parrot）+ `inject_billing_block` + `adapt_to_anthropic`（chat/responses 转换）+ `apply_cc_headers` + `send`（Bearer + 代理出口 + 401 刷新重试）+ `OauthTarget`（Messages/CountTokens）+ `AIGW_ANTHROPIC_MOCK_BASE`测试端点重映射 |
| `crates/aigw-core/src/lib.rs` | 注册 `oauth_pipeline` 模块 |
| `crates/aigw-server/src/routes/chat.rs` | `/v1/chat/completions` OAuth 分支：`adapt_to_anthropic(OpenAI)` → billing 注入 → 管线 send → 流式/非流式 SpendLog |
| `crates/aigw-server/src/routes/v1_messages.rs` | `/v1/messages` OAuth 分支（原生 passthrough + billing 注入）+ 新建 `count_tokens_handler`（OAuth → token-counting beta / 非 OAuth passthrough） |
| `crates/aigw-server/src/routes/responses.rs` | `/v1/responses` OAuth 分支：`adapt_to_anthropic(Responses)` → billing → 管线 |
| `crates/aigw-server/src/routes/embeddings.rs` | OAuth 凭证 → **400**「Anthropic OAuth 凭证不支持 embeddings」 |
| `crates/aigw-server/src/main.rs` | 注册 `/v1/messages/count_tokens` 路由 |
| BDD | `claude_oauth.feature` +4 场景（messages Bearer+billing / chat 转换 / embeddings 400 / 401 刷新重试）；`mock_upstream.rs` 加 `set_response_first_n` 一次性响应；`claude_oauth_steps.rs` +7 步骤 |

## 2. 测试结果

| 层 | 结果 |
|----|------|
| aigw-core lib UT | **500 passed**（+7 Stage 128，503 running） |
| aigw-server lib UT | **154 passed** |
| mock BDD | **275 scenarios（262 passed / 13 skip body_archive）** |
| real BDD sqlite | **53/53 passed** |
| fmt / lint / doctor / build | 全绿 |

## 3. 关键设计点

- **Billing 指纹**：`SHA256("59cf53e54c78" + chars[4,7,20] + version)[:3]`，字节对齐 sub2api `gateway_billing_block.go` / Parrot `cc_mimicry.py`。
- **billing 块注入**：无条件重写 `system[0]`（含覆盖客户端自带 billing 块），客户端原 system 保留在后，`inject_prompt` 追加为末尾 block（gate 只查 block[0]）。
- **CC 伪装**：OAuth 管线不转发客户端 x-stainless-*/UA 头（避免与注入值冲突判第三方）。
- **401 刷新重试**：管线内 401 → `TokenProvider::invalidate_and_refresh` → 重试一次。
- **测试端点重映射**：`AIGW_ANTHROPIC_MOCK_BASE` 仅 BDD harness 设置，生产零影响（与 Stage 126 `AIGW_OAUTH_MOCK_BASE` 同模式）。

## 4. 例外/环境

- **real BDD pg/mysql**：首跑因 OrbStack daemon 未启动失败（容器 Exited），拉起后 `docker compose -f docker-compose.test.yml up -d postgres mysql` 修复，**53/53 × 2 全绿**。
- 新 BDD mock 场景 `claude_oauth.feature` 非 `@real_api`，不进入 real BDD 覆盖范围。

## 5. Gate 4 代码评审（多模型 + 独立安全审计）

### 5.1 评审方法

- **Agent A（一般代码评审）**：发现 H1-H4/M1-M4 等，逐个读 commit diff 核实。
- **Agent B（专门安全审计）**：凭据泄漏 / 指纹注入 / 代理出口 / 401 重试 / embeddings 覆盖 / env 守卫 6 维度。
- **Agent C（独立验证）**：对 A/B 的 High/Critical 发现逐条用源码验证，输出 CONFIRMED/REFUTED/PARTIAL + 精确文件:行。

### 5.2 已修复（真实缺陷）

| 缺陷 | 严重性 | 修复 | commit |
|------|--------|------|--------|
| OAuth 流式 SpendLog 双 INSERT 同 call_id → 真实用量不落库 | High | Phase-2 改 `update_spend_log` | `432305b` |
| responses OAuth 零计费 | High | 补齐流式 + 非流式 SpendLog | `432305b` |
| 三处 OAuth 分支 span guard 跨 await（sharded.rs panic 同类） | High | OAuth 分支入口 `drop(_resolve_enter)` | `432305b` |
| mock base env 无生产守卫（凭据改道攻击者） | Medium | `feature="test"` 门控 + aigw-core test feature | `0d62ffa`/`285b1d6`/`2fc1d89` |
| 401 重试仍被上游拒不标 needs_reauth（运维告警缺口） | Medium | `send()` retry 仍 401 → `mark_needs_reauth` | `0d62ffa` |

### 5.3 验证后降级/不修

- **billing 指纹 12-bit 碰撞**（审计标 High）→ 验证 REFUTED：fp 只发上游、与账单归属无关、SALT 本就公开，sub2api/Parrot 上游设计固有。登记 TD-015b。
- **SpendLog 明文 body 落库**（审计标 High）→ 验证 PARTIAL：access_token 只走 header 永不进 body，非凭据泄漏；上游响应原文落库全库既有。登记 TD-015c。
- **proxy_url 经 reqwest error 进日志**（标 Medium）→ 验证 PARTIAL：reqwest Display 不含代理 userinfo。不修。
- **上游错误体透传**（全库既有）→ 纳入 Stage 130 审计项，登记 TD-015a。

### 5.4 产品决策待定

- **H4**：chat/responses 响应侧协议转换（OpenAI SDK 兼容 vs native response）— TD-015d。
- **M1**：count_tokens 认证语义（x-api-key / Bearer）— TD-015e。

## 6. 未做（登记长期路线）

TLS 指纹模拟、tool 名混淆、dateline 归一化、1h cache TTL 注入、metadata.user_id 注入、完整三块伪装 —— 与 Stage 128 §2.7 一致。
