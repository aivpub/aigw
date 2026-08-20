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

- **real BDD pg/mysql 未跑**：本地 Docker daemon（OrbStack）未启动，`testcontainers` PG/MySQL 容器无法创建（`pool timed out while waiting for an open connection`）。sqlite 真实后端 53/53 全绿。PG/MySQL 环境恢复后补跑。
- 新 BDD mock 场景 `claude_oauth.feature` 非 `@real_api`，不进入 real BDD 覆盖范围。

## 5. 未做（登记长期路线）

TLS 指纹模拟、tool 名混淆、dateline 归一化、1h cache TTL 注入、metadata.user_id 注入、完整三块伪装 —— 与 Stage 128 §2.7 一致。
