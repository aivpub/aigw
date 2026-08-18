# Stage 130: Phase 51 收尾 — real BDD + 安全审计 + 文档（Claude OAuth 反代）

**所属**: Phase 51（Claude OAuth 订阅反代）
**预估**: 6h（real BDD + ADR-034 + 安全审计 + roadmap/next-steps 回写）
**依赖**: Stage 126-129
**状态**: ⏳ 待开始

---

## 1. 目标

Phase 51 收尾：real BDD 三后端验证 + **安全审计**（sk-ant-sid 加密落库确认 + 响应脱敏 + 日志不泄密）+ ADR-034 + roadmap/next-steps 回写。

## 2. 方案

### 2.1 real BDD 三后端

- `features/real/claude_oauth_crud.feature`（@real_api @needs_upstream_db）：
  - OAuth 凭证 CRUD + exchange 三方言全绿
  - proxy_id in-use 守卫（凭证引用 → 删除代理 409）
  - probe_result 快照 + needs_reauth 状态
- exchange/刷新/自愈走 mock OAuth 上游（`@real_api` 不依赖真实 Anthropic，避免外部依赖 flake）;代理出口检测经 mock IP 服务

### 2.2 安全审计（本 Stage 专项）

| 检查项 | 要求 |
|--------|------|
| sk-ant-sid cookie 落库 | **必须加密**（master_key AES-GCM `v2:gcm:`），审计确认 DB 无明文 |
| access/refresh token 落库 | 必须加密 |
| 响应脱敏 | 所有 API 响应不含 access_token/refresh_token/session_key 明文（redact 覆盖 exchange 响应 + 凭证列表/详情） |
| 日志 | OAuth 交换/刷新/自愈路径不得打 token/cookie 明文（`tracing` 用 redact 助手）;proxy_url 日志打 redact 形态 |
| 错误传播 | 上游错误 body 只透传 message,不含凭证 |
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

- [ ] real BDD 三方言 OAuth 凭证 CRUD + in-use + 快照全绿
- [ ] 安全审计 8 项全部通过（审计清单核对）
- [ ] ADR-034 Accepted 记录
- [ ] roadmap 顶部状态 + Phase 51 条 + 总进度 134/134 回写
- [ ] next-steps 更新
