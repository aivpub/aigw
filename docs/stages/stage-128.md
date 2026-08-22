# Stage 128: Claude OAuth 反代管线（Phase 51）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 14h（resolver/Deployment OAuth 识别 + CC 头 + billing 注入 + 代理出口 + 协议转换接线 + count_tokens + embeddings 400 + BDD）
**依赖**: Stage 126/127（凭证 + TokenProvider）
**状态**: ✅ 完成（2026-08-20，代码 + 评审 + 验证全绿）

---

## 1. 目标

**任何入站协议**，只要 `ModelResolver` 解析到 OAuth 凭证（`type=="anthropic_oauth"`），统一走 OAuth 反代管线：经凭证绑定代理 → 注入 billing 块（默认最小化）→ `Authorization: Bearer <access_token>` 打到 Anthropic `/v1/messages`。非 OAuth 部署原样不动。

参考实现：`docs/research/2026-08-18-sub2api-proxy-oauth-reference.md` §2.4/2.5（身份 gate + 头部管线 + billing 注入）。

## 2. 方案

### 2.1 OAuth 部署判定

- `Deployment` 新增可选字段：
  ```rust
  pub struct Deployment {
      // ...existing...
      /// OAuth 反代引用:Some 时该部署走 OAuth 反代管线
      pub oauth: Option<OAuthDeployment>,
  }
  pub struct OAuthDeployment {
      pub credential_id: String,
      pub proxy_url: Option<String>,     // 凭证绑定代理(Stage 126)
      pub inject_prompt: Option<String>, // 凭证配置的注入提示词
  }
  ```
- `ModelResolver::resolve`：解析到 `litellm_credential_name` 且凭证 `type=="anthropic_oauth"` → 填 `oauth`（解密 proxy_url + inject_prompt）;否则 `None`
- 入口判定：`deployment.oauth.is_some()` → OAuth 反代管线;否则原路径

### 2.2 反代管线（`crates/aigw-core/src/oauth_pipeline.rs` 新建）

```
handle(client_protocol, deployment, body):
  1. access_token = TokenProvider.get_access_token(credential_id)   // 缓存→刷新→cookie 自愈
  2. target = "https://api.anthropic.com/v1/messages"               // 统一上游
  3. body 按需转换: OpenAIToAnthropic(chat) / ResponsesToChat(responses) / 原样(messages)
  4. 注入 billing 块(2.3) + inject_prompt
  5. headers: Authorization: Bearer <token> + CC 伪装头(2.4)
  6. reqwest client 带 proxy_url(凭证绑定代理) → POST
  7. 401 → TokenProvider.invalidate_and_refresh → 重试一次
  8. 流式 SSE 透传(与现有 stream 路径一致)
```

### 2.3 Billing 块注入（默认最小化,用户决策）

- 无条件重写 `system[0]` 为 billing 块（即使客户端已发 CC 风格 system——对齐 sub2api 无条件覆盖逻辑,防「有 CC prompt 无 billing block」不一致判第三方）
- 形态：`x-anthropic-billing-header: cc_version={ver}.{fp}; cc_entrypoint=cli;`
- 指纹 `compute_claude_code_fingerprint(body, version)`（**字节对齐 sub2api/Parrot**）：
  1. 首条 role=user 消息纯文本(首块 text)
  2. chars = text[4], text[7], text[20](不足补 '0')
  3. `SHA256("59cf53e54c78" + chars + version)` hex[:3]
- version 常量 `CLI_CURRENT_VERSION = "2.1.220"`
- **不带 cache_control**(与真实 CLI 一致);0 token 成本、服务端剥离
- **客户端原 system 块保留在后**（本阶段最小注入,不做 [System Instructions] 消息对降级）
- **inject_prompt**：凭证配置了则追加为额外 block(在 billing 之后;gate 只查 block[0])

### 2.4 CC 伪装头（对齐 sub2api 头部管线最小集合）

| Header | 值 |
|--------|-----|
| `Authorization` | `Bearer <access_token>`（OAuth 不用 x-api-key） |
| `anthropic-version` | `2023-06-01` |
| `anthropic-beta` | `oauth-2025-04-20, claude-code-20250219` |
| `User-Agent` | `claude-cli/2.1.220 (external, cli)` |
| `x-app` | `cli` |
| `X-Stainless-Lang` / `Package-Version` / `OS` / `Arch` / `Runtime` / `Runtime-Version` | `js` / `0.94.0` / `Linux` / `arm64` / `node` / `v24.3.0` |
| `X-Stainless-Retry-Count` / `Timeout` | `0` / `600` |
| `Accept` | `application/json` |
| `anthropic-dangerous-direct-browser-access` | `true` |

**mimic 路径跳过客户端头转发**（对齐 sub2api：转发客户端 x-stainless-*/UA 会与注入头冲突判第三方）——OAuth 管线不转发客户端头，全部用注入值。

### 2.5 协议转换接线

- `/v1/messages` → 原样(Anthropic 原生)
- `/v1/chat/completions` → 现有 `OpenAIToAnthropic` 转 Anthropic 格式
- `/v1/responses` → 现有 `ResponsesToChat` → 再转 Anthropic 格式
- 转换后统一进 2.2 管线(billing 注入 + Bearer + 代理出口)
- **count_tokens**：`/v1/messages/count_tokens` → Bearer + `token-counting-2024-11-01` beta(注入 billing 块可省:count_tokens 无身份 gate,但为一致性仍走管线)
- **embeddings**：解析到 OAuth 凭证 → **400**「Anthropic OAuth 凭证不支持 embeddings」(Anthropic 无 embedding 端点)

### 2.6 流式

- 流式 SSE 逐帧透传(与现有 messages/chat 流式路径一致);上游 SSE 事件不做协议转换(messages 原生),转换协议由 2.5 的 adapter 产出 Anthropic 格式 SSE

### 2.7 不做（登记长期路线）

TLS 指纹模拟、tool 名混淆、dateline 归一化、1h cache TTL 注入、metadata.user_id 注入、完整三块伪装——sub2api 完整伪装链的增强项,本阶段只做身份 gate 必需的最小集合。

## 3. TDD 计划

### 3.1 core UT

- `test_billing_fingerprint_byte_aligned`：已知 body → 期望 fp（与 sub2api/Parrot 对齐向量）
- `test_billing_block_injected_first`：system[0] = billing 块;原 system 保留在后
- `test_inject_prompt_appended`：凭证 inject_prompt 追加为后续 block
- `test_oauth_deployment_detection`：resolver type==anthropic_oauth → oauth 填值;否则 None
- `test_pipeline_401_retry_once`：401 → 强制刷新 → 重试
- `test_pipeline_embeddings_400`
- `test_pipeline_proxy_egress`：请求经凭证绑定代理发出

### 3.2 mock BDD（`features/claude_oauth.feature` 扩展）

- messages 走 OAuth 反代:Bearer + billing 块 + 代理出口断言
- chat/completions → Anthropic 转换 + 同一管线
- count_tokens Bearer + token-counting beta
- embeddings OAuth → 400
- 401 → 刷新重试成功 / 仍失败 → 401

## 4. 验收标准

- [x] resolver OAuth 识别 + 反代管线全绿
- [x] billing 块默认最小化注入 + 指纹字节对齐
- [x] chat/responses 转换接线 + count_tokens + embeddings 400
- [x] 代理出口生效;mock BDD 扩展全绿;既有基线无回归

---

## 5. Implementation Notes

### 5.1 交付 commit（11 个）

| Commit | 内容 |
|--------|------|
| `beffd97` | OAuth 反代管线核心（billing 指纹 + 协议转换 + CC 伪装 + 401 刷新重试 + Deployment.oauth + resolver 识别）+ 7 UT |
| `fb1f6b8` | 四入口接线（chat/v1_messages/responses/embeddings）+ count_tokens 端点 |
| `fdf5f2a` | claude_oauth.feature 反代 4 场景 + mock 一次性响应 |
| `8968b50` / `8f46b59` | review log + health.rs oauth 字段 |
| `77dc2eb` | root span guard 跨 await 显式 drop（修 sharded.rs 偶发 panic） |
| `8e40e0c` | span guard 风险注释 |
| `432305b` | **Gate 4 修复**：OAuth 流式计费双 INSERT 主键冲突 + responses 零计费 + OAuth 分支 span guard |
| `0d62ffa` | **Gate 4 修复**：401 后 needs_reauth + mock env 生产守卫 |
| `285b1d6` / `2fc1d89` | **Gate 4 修复**：aigw-core test feature 门控 + claude_oauth mock env 守卫 |

### 5.2 与设计的偏差

- **响应侧未做协议转换**：chat/responses 只做请求侧 `adapt_to_anthropic`，响应直接返回 Anthropic 原生结构（文档 §2.6 记"客户端接收 native response"）。若需 OpenAI SDK 兼容应补响应侧 `adapt_response`——**产品决策待定**（H4）。
- **count_tokens 认证**：用 Bearer-only `ChatAuth`，Anthropic 客户端用 x-api-key 会 401——**产品决策待定**（M1）。
- **流式计费修正**：初版 OAuth 流式 Phase-2 误用 `insert_spend_log`（同 call_id 主键冲突 → 真实用量不落库），Gate 4 评审发现后改 `update_spend_log`。
- **responses OAuth 计费补齐**：初版 responses OAuth 分支零计费，Gate 4 补齐流式 + 非流式 SpendLog。

### 5.3 验证基线（Gate 3-5）

- aigw-core **500 UT** / aigw-server **154 UT**
- mock BDD **275（262 pass / 13 skip body_archive / 0 fail）**，含 Stage 128 反代 4 场景
- real BDD 三端 sqlite/pg/mysql **53/53 × 3**
- fe-build / fe-lint / fe-bdd **369 pass**
- fmt + clippy -D warnings 全绿

### 5.4 安全审计（Gate 4）

- **已修**：mock base env 生产守卫（feature 门控）、401 后 needs_reauth、上游错误体透传（全库既有模式，纳入 Stage 130 审计项）
- **降级不修**：billing 指纹 12-bit 碰撞（威胁模型不成立，sub2api/Parrot 上游设计固有）、SpendLog 明文 body 落库（access_token 只走 header 永不进 body，全库既有模式）
- **待定**：H4 响应侧协议转换、M1 count_tokens 认证语义

### 5.5 不做（登记长期路线）

TLS 指纹模拟、tool 名混淆、dateline 归一化、1h cache TTL 注入、metadata.user_id 注入、完整三块伪装（Stage 128 §2.7）。
