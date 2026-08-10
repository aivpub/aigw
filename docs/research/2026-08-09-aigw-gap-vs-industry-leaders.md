# aigw vs 业界领先 AI 网关 —— 差距分析与长期规划建议

> 调研日期：2026-08-09/10 ｜ 编译：最终综合 agent
> 输入：`/tmp/aigw-gap-research/` 下 10 篇笔记（00 基线 + 11/12/13 litellm + 21/22/23 国际 + 31/32 中国 + 41 Rust）+ 本仓库代码抽查核验
> 目标：aigw（Rust + axum + sqlx，litellm 最小兼容替代）长期规划输入，每条建议可落地到「特性 + 参考实现 + 工作量」
> 版本基线：litellm v1.97.0（commit ecba48d，源码浅克隆核对）

---

## 1. 执行摘要

### 1.1 aigw 现状定位

aigw 已完成 116/116 Stages + Phase 45，v0.2.0+109 commits，是一个**功能完整的最小代理型网关**：四类入站协议（OpenAI Chat/Anthropic Messages/Responses/Embeddings）全部代理 + 双向协议转换、三方言数据库（SQLite/PG/MySQL 各 25 迁移）、两级通用上游接入、三级缓存差异化计费 + 全链路 SpendLog + TTFT、S3/Parquet ZSTD Body Archive 生产化、Prometheus/OTEL 可观测底座、12+ 页前端控制台、litellm 数据双向迁移工具。**单二进制 ~20MB 部署形态在开源网关里独树一帜。**

但与 litellm（1.97.0，149 provider / 11863 行 Router / 16948 行 proxy_server / 70 张表）及业界领导者（Portkey/OpenRouter/Cloudflare/Envoy AI Gateway/Higress/agentgateway/aisix）相比，aigw 的差距分两类：

- **A 类「代码在但运行时未接线」（最致命、成本最低）**：RPM/TPM 限流、多级预算 `check_budget_multi`、soft_budget 告警、`max_parallel_requests`、`usage-based/latency-based` 路由、`report_failure/success` cooldown、`merge_router_overrides`、`TenantAuth` SaaS 隔离——**这些模块都已实现且有 UT，但请求路径零调用点，生产上不生效**。这是本次调研反复出现的最大欠账。
- **B 类「明确缺失」（需要投入建设）**：guardrails/内容安全、缓存层（exact-match + 语义）、路由智能（7+ 策略）、企业级（SSO/RBAC/审计）、告警通道（Slack/企微/邮件）、MCP、WebSocket、OpenAPI 覆盖、中国市场特性（MCP 托管/token 限流/模型市场）。

### 1.2 Top 差距（一句话）

1. **未接线的能力比缺失的能力更危险**——限流/多级预算/cooldown/fallback 全部是「看起来有、实际没有」，企业采购 demo 一测即穿。
2. **AI 语义层整体空白**——guardrails（0 vs litellm 50 hooks / 商业云 6 类过滤）、缓存（无，所有竞品标配）、语义路由（未接线）三连缺。
3. **企业级治理缺失**——SSO/RBAC/审计日志（0 vs litellm 完整矩阵 + 商业云 IAM/CloudTrail）。
4. **中国市场差异点未打出**——Anthropic 双协议 + 三级缓存计费 + 单二进制自托管已是差异化，但 MCP（中国市场 2026 标配）、token 级限流、模型市场缺失。
5. **OpenAPI/可观测/前端**覆盖不全（19/92 端点入 spec、14 vs litellm 47 指标），影响企业集成与可观测卖点。

### 1.3 一句话结论

> **aigw 的护城河是「litellm 数据/API 兼容 + 单二进制 + 三方言 DB + 高精度计费」，当前最优性价比动作是把已写的代码接上请求路径（A 类），再按「先精确缓存与路由接线、后 guardrails 最小集、再企业级治理」的顺序补齐 B 类，同时把中国市场特性（Anthropic 协议 + 缓存计费）变成可营销卖点。**

---

## 2. aigw 基线（引用 00-aigw-baseline.md）

### 2.1 已有（确认在请求路径/运行时生效）

| # | 能力 | 证据 |
|---|---|---|
| 1 | 客户端协议：`/v1/chat/completions`、`/v1/models`、`/v1/messages`、`/v1/responses`、`/v1/embeddings`（四端点含 Azure 别名）全部代理；SSE 流式双向转换（OpenAI↔Anthropic、Responses↔Chat） | adapter.rs 4370 行 + 5 适配器 |
| 2 | 数据库三方言：SQLite+PostgreSQL+MySQL 全支持（每方言 25 迁移，Cross-DB BDD） | migrations/{sqlite,postgres,mysql} |
| 3 | 上游：OpenAI-compatible（通用）+ Anthropic native，5 协议适配器 | select_adapter 矩阵 |
| 4 | 部署：单静态二进制(~20MB) + rust-embed 前端 + Docker/Compose 三形态 + amd64/arm64 | Dockerfile + compose |
| 5 | Spend 全链路：两阶段流式 SpendLog、三级缓存计费、image_tokens、daily_*_spend 6 维聚合、上游 request_id 对账、TTFT | chat.rs / spend.rs / daily_spend_queue.rs |
| 6 | 单级 key 预算检查（四 handler 内联）+ 请求后异步实体 spend 增量 + Budget Reset 周期任务 | budget.rs + async_task.rs |
| 7 | Body Archive 生产化：S3/FS Parquet ZSTD 冷存储（写分片+读 footer 缓存+Admin API+前端 Jobs 页） | body_archive/ |
| 8 | 可观测：JSON 日志、Prometheus 14 指标、OTEL 5 层 span、健康检查全套、x-call-id 响应头 | metrics.rs / otel_tracing.rs |
| 9 | 前端 12+ 页面 + i18n 中英 + Playground 多模态图片 + SpendLog 详情/CSV + Budgets/Jobs 页 | App.tsx |
| 10 | 鉴权：Virtual Key + Master Key + JWT/scrypt 管理登录 + key 模型白名单 + require_admin | auth.rs |
| 11 | 多租户数据模型 + org/team/user/budget/credential CRUD + 软删除归档 | models.rs |
| 12 | aigw-migrate：litellm↔aigw 双向迁移 + aigw↔aigw sync + precheck/verify/rollback | migrate crate |
| 13 | HTTP 重试（reqwest-retry 指数退避）+ 响应压缩 + 请求体上限 32MiB | chat/v1_messages/responses/embeddings 四 handler |
| 14 | 测试体系：~815 UT + mock BDD 233 + fe-bdd 342 + real BDD 三后端 41-47 + BDD 覆盖率门禁 | Taskfile |

### 2.2 缺失或「代码在但运行时未接线」（差距重点）

| # | 项 | 性质 | 关键证据 |
|---|---|---|---|
| M1 | RPM/TPM 限流 | **未接线** | `RateLimiter`（252 行 token bucket）+ `enforce_limits` 零调用点；`main.rs:223` 创建后仅注入 AppState，路由层无 `.check(` |
| M2 | 多级预算 `check_budget_multi`（key→user→team→org）+ soft_budget + alert webhook | **未接线** | 仅被未接线的 `enforce_limits` 引用；实际只生效单级 key max_budget |
| M3 | `max_parallel_requests` | 字段无执行 | 字段存储于 key/budget 表，无信号量 |
| M4 | usage/latency-based 路由 | **未接线** | `Router::pick_deployment` 仅 SimpleShuffle + cooldown 过滤；Strategy 枚举其余值/`select_instance`/`report_failure/success`/`merge_router_overrides` 均未接入请求路径 |
| M5 | SaaS 租户隔离 | 未接线 | `TenantAuth`/`TenantIdentity` 定义但无路由使用 |
| M6 | OpenAPI spec 覆盖不全 | 覆盖缺失 | 19/92 端点入 spec（openapi.rs expected_endpoints）；Swagger UI 依赖 unpkg CDN |
| M7 | 端点缺失 | 缺失 | `/global/spend/users`（litellm 有）；chat 无 Azure 别名端点；无 `/v1/images|audio|moderations|files|assistants|threads|fine_tuning` |
| M8 | 计费/模态剩余 | 部分 | 视频 token 估算 + Playground 视频输入（TD-011a SKIPPED）；embeddings 按模态计费未接线（TD-012b） |
| M9 | Key 自动轮换 | 字段无执行 | auto_rotate/rotation_interval/key_rotation_at 存储但无逻辑 |
| M10 | 出站告警单一 | 缺失 | 仅 soft_budget webhook 一种；无 Slack/Email/企微模板与去重 |
| M11 | 长期路线未交付 | 缺失 | K8s Operator/Helm、PG 高可用迁移、BodyArchive 日 compaction/生命周期/监控指标 |
| M12 | NG 清单（明确不做） | 明确不做 | Redis 语义缓存(NG4)、SSO/OAuth(NG1)、Guardrails/Policy(NG2)、MCP(NG3)、Adaptive Router(NG5)、WebSocket(NG7)、Workflow(NG8)、30+ provider handler(NG9) |

---

## 3. 竞品画像

> 每个产品给「一张表 + 3-5 条关键特性/独特优势」。规模数据为调研时点快照。

### 3.1 litellm（BerriAI，aigw 的参照实现）

| 维度 | 值 |
|---|---|
| 语言/许可 | Python（proxy）+ Rust 核心（官方 tagline 已改为 "fastest, litest AI Gateway. Rust core"），MIT + 商业版 |
| 规模 | 55,955 stars；proxy_server.py 16948 行、Router 11863 行、149 provider 枚举、133 llms 子目录、70 张表、47 个 Prometheus metric、guardrails 128 py/50 hooks |
| 定位 | 全功能企业 proxy + 可观测 + 治理全家桶 |

关键特性：
1. **7 种路由策略 + 分型 fallback/retry**：simple-shuffle/least-busy/usage-based(v1/v2)/latency/cost/lar1 + routing_groups + weighted + `context_window_fallbacks`/`content_policy_fallbacks`/order 级 fallback + 流中 fallback（MidStreamFallbackError + chunk 重建）+ 按异常类型的 `retry_policy`/`allowed_fails_policy`。
2. **预算/限流/花费体系最完整**：Redis 原子 Lua 跨 pod counter（`spend:key/team/user/org:` 等）、预算预扣（`reserve_budget_for_request` 估计最大成本预扣 + 对账 + 流中断兜底）、`budget_duration` 周期窗口 + `budget_reset_at`、soft/projected 告警、`budget_throttle` 超预算降速、7 张 daily spend 表含 savings 字段。
3. **计费引擎最深**：2988 条内置价格表 + service-tier/`_above_128k..512k` 阈值/`tiered_pricing`/reasoning 独立价/regional uplift/batch 减半/discount-margin + 三级缓存 + 分模态（image/video/audio/reasoning）。
4. **缓存矩阵**：memory/Redis/Redis-Cluster/DualCache/S3/GCS/Azure-blob/disk + 语义缓存（Redis/Valkey/Qdrant）+ `cache={use-cache,no-store,ttl,s-maxage}` + prompt caching 透传 + cache-hit 计费 0 元。
5. **企业级**：SSO（SAML/Google/Microsoft/Okta 通用 OAuth）+ SCIM、RBAC 角色矩阵（auth_checks ~4000 行）、Audit Log 表 + 13 挂载点、Slack/PagerDuty/email 告警（21 类型）、guardrails 50 hooks + CustomLogger 64 hook 集成框架。

### 3.2 Portkey（AI Gateway + Control Plane，已被 Palo Alto Networks 收购）

| 维度 | 值 |
|---|---|
| 语言/许可 | TypeScript，开源 MIT + 托管 SaaS + 企业私有部署 |
| 规模 | 12.7k stars；45+ providers / 1600+ 模型；声称每天 100 亿+ tokens |
| 定位 | AI Gateway + Observability + Guardrails + Governance + Prompt Management 一体 |

关键特性：
1. **Data/Control Plane 分离架构**：网关容器留客户 VPC（路由/计量/访问控制/guardrail），控制面 SaaS 托管 dashboard/config；1 分钟 heartbeat 配置同步 + 离线可用（缓存 TTL 7 天）+ 出站匿名指标。
2. **嵌套策略路由**：fallback target 可嵌套「负载均衡/条件路由/另一 fallback」；自动重试至 5 次 + fallback 限 `on_status_codes`。
3. **语义缓存**：余弦相似度 0.95 阈值 + Milvus/Pinecone + embedding provider（Enterprise + 自托管），简单 exact-match 缓存全计划。
4. **guardrails**：50+ 预置检查（regex/JSON-schema/代码检测/提示注入）+ PII 脱敏 + 30+ 第三方集成 + FAIL 返回 246/446 自定义码驱动 fallback。
5. **治理**：虚拟 key、RBAC（user/workspace/key）、SSO/SCIM、PII、SOC2/HIPAA、KMS；MCP Gateway 统一 auth + tool 全量日志。

**教训**：独立网关厂商终局被安全平台收购——纯网关差异化弱，控制面 + 治理能力才是收购价值。

### 3.3 OpenRouter（纯托管聚合器）

| 维度 | 值 |
|---|---|
| 语言/许可 | 核心闭源 SaaS；仅开源 TS/Python/Go SDK |
| 规模 | 400+ 模型 / 70-90+ providers / 200 万亿+ 月 tokens / 1000 万+ 用户；融资 $1.13 亿 |
| 定位 | "largest and most popular AI gateway"，一个端点买所有模型 |

关键特性：
1. **provider 级路由最精细**：价格反平方加权 + `order`/`only`/`ignore` 白黑名单 + `max_price` 每请求价格上限 + 吞吐/延迟百分位偏好 + `:nitro`/`:floor` 模型后缀 + ZDR 强制。
2. **Auto Router 全局路由**：~30 类任务分类 + spend-share 排名 + cost/quality 约束 + session_id/prompt-cache 指纹粘性。
3. **usage/cost 透明**：每响应 `usage.cost` + `cost_details.upstream_inference_cost`（透明 margin）；Analytics API ≤2 维度，metric 含 cache_hit_rate/tokens_reasoning。
4. **组织 guardrail**：预算限额（日/周/月 reset）、模型/提供者 allowlist、ZDR、PII NLP 检测、提示注入 regex；多 guardrail 冲突「更严格者胜」。
5. **credit 商业模式**：推理 0 markup，收 credit 购买手续费 5.5%（Stripe）/5%（crypto）+ BYOK 5% 抽成。

**教训**：路由 + 计费/信用 + 用量分析可做成独立托管商业模式，关键在模型/价格目录是实时数据资产。

### 3.4 Cloudflare AI Gateway（边缘 AI 网关）

| 维度 | 值 |
|---|---|
| 语言/许可 | 托管 SaaS（依托 300+ 边缘节点） |
| 规模 | 26 家 provider；统一 OpenAI 兼容 3 类端点 |
| 定位 | 开发者优先的边缘流量层（统一端点 + 缓存 + 限流 + 可观测 + DLP） |

关键特性：
1. **边缘 exact-match 缓存**：cache key = SHA-256(provider+endpoint+model+auth+body)，`cf-aig-cache-status` HIT/MISS 头；语义缓存官方明确"规划中"。
2. **结构化动态路由**：图式节点编排（Conditional/Percentage/Model/Rate Limit/Budget Limit），预算超限→显式 fallback 节点（非自动重试）。
3. **DLP 数据防泄漏**（2025 新增）：入站 prompt + 出站 response 扫描，Flag（放行+日志）或 Block（400），流式先缓存再扫描。
4. **跨 provider 统一可观测**：Requests/Token/Costs/Errors/Cached Responses + GraphQL 查询（`aiGatewayRequestsAdaptiveGroups`）。
5. **限流弱项**：仅请求数维度（interval+limit+technique），无 per-token/per-tenant RPM/TPM、无 IAM/RBAC。

**启示**：SaaS 多租户流量/成本层的好模板，但企业合规纵深弱（边缘即公网）。

### 3.5 Envoy AI Gateway（envoyproxy/ai-gateway，K8s 原生控制面）

| 维度 | 值 |
|---|---|
| 语言/架构 | 控制面 Go + 数据面 Envoy + **ExtProc（Rust sidecar）** 承担全部 AI 语义 |
| 规模 | v1.0.0 GA（2026-06-23）、1.9k stars、16 家 provider |
| 定位 | Envoy Gateway 之上的 AI 控制面（AIGatewayRoute/InferencePool） |

关键特性：
1. **token 计数→限流/配额/计费闭环**（aigw 最值得抄的设计）：ExtProc 从响应 usage 提取 5 类 token → dynamic metadata → Redis 全局限流 → 429；QuotaPolicy 用 CEL `costExpression` 做 token 预算 + shadowMode。
2. **provider fallback priority 排序**：numAttemptsPerPriority + priority（0 主 1 备），后端不健康自动切换。
3. **模型虚拟化 modelNameOverride**：一个统一模型名映射多 provider 不同命名 + 50/50 权重 + `x-ai-eg-model`/`x-tenant-id` 路由 header。
4. **prompt caching 透传**：统一 `cache_control:{"type":"ephemeral"}` 翻译成 Anthropic/Vertex/Bedrock 原生（≥1024 token 生效）。
5. **MCPRoute + InferencePool**：MCP 工具路由/CEL 授权；GPU/KV cache/LoRA 感知推理路由。

**启示**：Envoy/K8s 生态 aigw 不该抄；但「usage→限流/配额闭环」与「priority fallback」是 aigw Router 该补的。

### 3.6 Kong AI Gateway（插件市场式）

| 维度 | 值 |
|---|---|
| 语言/架构 | Kong（Nginx/OpenResty）+ 30+ Lua AI 插件 |
| 规模 | 44k stars（Kong 本体）；18 家 provider |
| 定位 | 通用 API 网关 + AI 插件（大部分 Enterprise） |

关键特性：
1. **ai-rate-limiting-advanced 成本级限流**：`(prompt×input_cost+completion×output_cost)/1e6` 计费公式接限流——与 aigw `calc_spend` 同构，aigw 可把 spend 结果接到 RPM/TPM。
2. **ai-prompt-guard（免费）**：regex allow/deny + 零宽字符/双向控制字符检测（prompt injection）——aigw 最小 guardrail 的直接蓝本。
3. **ai-prompt-template**：`{{variable}}` 填空模板 + `{template://}` 引用 + `allow_untemplated_requests=false` 强制。
4. **ai-semantic-cache / ai-rag-injector / ai-semantic-prompt-guard**：embedding 相似度 + Redis-VSS/pgvector；embeddings/vectordb 配置「部分共享」模式值得抄。
5. **ai-proxy-advanced 负载均衡**：round-robin/consistent-hashing/least-connections/lowest-latency(EWMA)/lowest-usage/semantic + failover_criteria。

### 3.7 agentgateway（solo-io 系，Rust 单二进制，kgateway 的 AI 主线）

| 维度 | 值 |
|---|---|
| 语言/许可 | Rust + K8s controller，Linux Foundation，4.3k stars |
| 规模 | 单二进制 30MB 内存、165k QPS、sub-ms；独立 standalone 模式 + K8s Gateway API |
| 定位 | "first complete connectivity solution for Agentic AI"：LLM+MCP+A2A+Inference 四合一 |

关键特性：
1. **P2C（Power of Two Choices）负载均衡 + priority 级 failover**（错误/429 自动切换）——零依赖 Rust 易实现。
2. **guardrail 5 层**：regex + 内置 PII / OpenAI Moderation / AWS Bedrock Guardrails / Google Model Armor / 自定义 webhook；请求+响应双侧 defense-in-depth。
3. **token 限流/预算**：`tokens`+`requests` 双维度，响应完成后扣减（流式等完整流），`x-ratelimit-limit/remaining/reset` 头。
4. **CEL 模板/transform**：`json().with()` 富化（含 request.headers/jwt）+ CEL 请求/响应 body 改写。
5. **standalone 模式证明**：轻量 AI 网关不做 K8s 也能立足——与 aigw 单二进制 onprem 完全同路。

### 3.8 Higress（阿里，CNCF Sandbox，中国 AI 网关定义者）

| 维度 | 值 |
|---|---|
| 语言/架构 | Istio + Envoy + **Wasm 插件**，9.1k stars，CNCF Sandbox |
| 规模 | ai-proxy 38 provider；社区版免费 + 企业托管版（99.95% SLA） |
| 定位 | 一个部署同时是 AI 网关 + K8s ingress + 微服务网关 + 安全网关 |

关键特性：
1. **ai-proxy**：统一三条路径（`/v1/chat/completions`、`/v1/embeddings`、`/v1/messages`）+ 自动协议探测转换 + modelMapping 前缀通配（`"gpt-4-*": "qwen-max"` + `"*"` 兜底）。
2. **ai-token-ratelimit**：**token 级限流**（非请求数），Redis 状态，依赖 ai-statistics 从响应 usage 提取 token——中国市场 token 限流标准。
3. **ai-cache**：Redis 响应缓存，GJSON PATH 取最后一条用户消息做 key，流式+非流式都缓存，`x-higress-skip-ai-cache` 绕过。
4. **McpBridge CRD + HTTP-to-MCP 转换**：把上游聚合服务暴露为 MCP Server，openapi-to-mcp 工具。
5. **ai-agent/ai-rag/ai-security-guard/ai-data-masking**：网关层 ReAct Agent 引擎 + RAG + 内容安全 + 数据脱敏插件链。

**启示**：中国市场 MCP 已从「要不要」变「必须有」；token 级限流 + 语义缓存是标配。

### 3.9 new-api（one-api 继任者，中国中转站事实标准）

| 维度 | 值 |
|---|---|
| 语言/许可 | Go 单二进制，AGPLv3 + 商用授权 |
| 规模 | ~45k stars / 11k forks / 2.5M Docker pulls（活跃，2026-08-08 更新） |
| 定位 | 面向中国用户的 API 中转站（用户自助额度 + 充值 + 渠道 + 分组倍率） |

关键特性：
1. **用户自助额度体系**：钱包余额 + 兑换码 + 在线支付（EPay/Stripe/Creem/Waffo）+ 订阅套餐 + 签到 + 邀请返利。
2. **channel 渠道模型**：渠道 CRUD/权重/优先级/自动禁用（成功率阈值 0.8）/余额查询/多 Key 轮询/标签。
3. **分组定价**：ModelRatio/CompletionRatio/CacheRatio/GroupRatio/ModelPrice + **Tiered 账单表达式 DSL**（`tier("standard", p*3+c*15+cr*0.3+...)`）。
4. **模型市场**：从 basellm/llm-metadata 同步模型/倍率预设，models.dev 价格。
5. **任务型渠道**：MJ/Suno/视频（Sora/Kling/即梦）+ Realtime WS + 图片/音频。

**对比结论**：aigw 的 per-key RPM/TPM 令牌桶与三级缓存计费比 new-api 强；new-api 的充值/渠道/分组倍率是「中转站生意」不是网关产品，aigw 不该做（31 号笔记结论）。

### 3.10 api7/aisix（与 aigw 最同构的 Rust 竞品）

| 维度 | 值 |
|---|---|
| 语言/许可 | **Rust 单二进制**，Apache-2.0，92 stars（596 commits，pre-1.0） |
| 规模 | 165 e2e 测试（426 case）；declarative `resources.yaml` 热重载 + etcd 多副本 |
| 定位 | API7（APISIX 原班人马）Rust AI 网关 |

关键特性：
1. **6 路由策略 + 语义路由**：round-robin/加权 sticky/canary/least-cost/least-latency/**semantic embedding 相似度路由**。
2. **guardrails 全家桶**：keyword/regex/PII/Presidio/Lakera/OpenAI Moderation/Bedrock/Azure/Aliyun/火山。
3. **精确匹配响应缓存**（内存+Redis）+ Anthropic 自动 prompt caching。
4. **限流**：RPS/RPM/RPH/RPD + TPM/TPD + 并发，caller key×model×policy AND 组合，Redis 分布式。
5. **MCP + A2A 网关 + ensemble 模型**（fan-out + judge）。

**结论**：aisix 与 aigw 同构（Rust 单二进制 + 统一端点 + 观测/限流/guardrail/缓存）；aigw 相对优势在 litellm schema 兼容 + 三方言 DB + 计费精度 + 前端；aisix 相对优势在 guardrail/cache/semantic routing 已落地——**这两块正是 aigw 的 B 类缺口，互相印证了建设优先级**。

### 3.11 其他 Rust 生态相关项（41 号笔记要点）

- **tensorzero（11.7k stars）**：Rust 核心 LLMOps 平台，2026-06-12 被 owner 归档转向付费产品，留下「Rust LLM 网关平台化」真空，可作为 aigw 长期评测/优化层蓝本。
- **litellm-rs（104 stars）**：litellm 灵感 Rust 网关 + 库，60+ providers、默认 prompt-injection guardrails、**语义缓存明确 "config-rejected"（未实现）**——佐证语义缓存在 Rust 生态是共同缺口。
- **async-openai（~2k stars）**：Rust OpenAI SDK，BYOT 支持 OpenAI 兼容端点——「面向 OpenAI 兼容上游通用接入」是 Rust 生态主流，与 aigw NG9 一致。
- **agentgateway / aisix / clewdr / gproxy / smg / Helicone ai-gateway**：Rust 单二进制网关路线验证者。

---

## 4. 差距分析矩阵

维度 ×（aigw vs litellm vs 领导者）+ 差距等级（无/小/中/大）。

| 维度 | aigw 现状（证据） | litellm 现状 | 业界领导者（代表） | 差距等级 |
|---|---|---|---|---|
| **协议覆盖** | OpenAI Chat/Messages/Responses/Embeddings 全代理 + 5 适配器 + SSE 双向转换 | 全协议 + Realtime/Images/Audio/Files/Assistants/Batches/Vector Stores 全功能 | 全功能（Portkey/Cloudflare/Envoy） | **中**（chat/embeddings 对齐，缺 images/audio/moderations/realtime/fine_tuning + chat Azure 别名） |
| **Provider 覆盖** | OpenAI-compat + Anthropic 两类通用接入（NG9 明确不做 30+） | 149 provider / 133 llms 子目录 / 26 JSON provider 注册表 / per-provider transformation | Portkey 45+、OpenRouter 90+、Higress 38 | **小→中**（策略性收窄，NG9 有据；但缺 per-provider 参数白名单 `get_supported_openai_params`） |
| **路由智能** | 仅 SimpleShuffle 生效；usage/latency/weighted/report_failure/cooldown/merge_overrides **未接线** | 7 策略 + routing_groups + weighted + 分型 fallback/retry + 流中 fallback + cooldown 全链自动 | Envoy priority failover + P2C、OpenRouter 价格加权 + max_price | **大**（代码在但运行时失效，最致命） |
| **缓存** | 无缓存层（仅计费解析 cache tokens；NG4 长期路线） | memory/Redis/Cluster/Dual/S3/disk + 语义缓存（Redis/Valkey/Qdrant）+ cache 控制头 + prompt caching 透传 | Portkey semantic、Cloudflare 边缘 exact-match、Higress ai-cache | **大**（全竞品标配，aigw 为 0） |
| **预算配额** | 单级 key 预算生效；多级 `check_budget_multi`/soft_budget/alert **未接线**；`max_parallel_requests` 无执行；Budget Reset 已接线 | Redis 跨 pod counter + 预算预扣 + 周期窗口 + throttle + 模型级预算 + provider 预算路由 | Azure Quota Tiers RPM/TPM、Envoy QuotaPolicy、Cloudflare Budget Limit | **大**（预算模型全但接线缺口；限流维度粗） |
| **可观测性** | Prometheus 14 指标 + OTEL 5 层 span + TTFT + x-call-id + JSON 日志 | 47 metric family + OTEL v2（SPAN_REGISTRY 8 类 + presets/mappers）+ 服务健康指标 | Cloudflare Costs/TTFT、Bedrock identity.arn 日志、agentgateway OTEL+OpenInference | **中**（底座有，缺 budget/guardrail/cache/MCP 指标 + `gen_ai.*` 语义 + dashboards） |
| **企业级（RBAC·SSO·审计·guardrail）** | 管理端 JWT/scrypt + require_admin；无 SSO/RBAC 矩阵/审计日志/guardrails | SSO(SAML/OAuth/SCIM) + RBAC 角色矩阵 + AuditLog 表 13 挂载 + guardrails 50 hooks | 商业云 IAM/Entra + CloudTrail/审计 + Bedrock Guardrails/Azure Content Filter | **大**（0 vs 完整矩阵；企业采购红线） |
| **网关形态** | 单二进制 ~20MB + rust-embed 前端 + Docker/Compose | Python 全家桶（重） | agentgateway/aisix 单二进制、Portkey Data/Control Plane 分离 | **小**（aigw 优势项；SaaS 控制面形态未做） |
| **集成生态** | 0 个日志集成（无 CustomLogger 机制） | 195 py integrations / 16 一等集成 / CustomLogger 64 hook | Portkey OTEL 11 家 tracing + 30+ guardrail 集成 | **大**（Langfuse/LangSmith/Datadog 等企业刚需） |
| **性能** | Rust + 单二进制，代理热路径快（~20MB） | Python（正 Rust 化，官方 8ms P95 @1k RPS） | agentgateway sub-ms、aisix、rtk 等 Rust 生态验证 | **小**（aigw 优势项，但 litellm 也在 Rust 化，纯性能不持久） |
| **开源治理/许可** | 仓库无 LICENSE 文件？需核对；friendly 单团队 | MIT + 商业版双许可；55k stars 生态 | 各家 Apache-2.0 / AGPLv3 / Linux Foundation | **小→中**（stars/社区不可比，但作为商业决策需定 license + CLA） |

**等级分布**：无差距 3 项（网关形态/性能/协议主体）、中差距 3 项（协议覆盖/可观测性/Provider）、**大差距 5 项（路由智能/缓存/预算配额/企业级/集成生态）**。

---

## 5. 方向建议——三时间窗

> 每条：为什么（gap+证据）、做什么、参考实现、工作量预估、优先级。
> 工作量按「一个熟练 Rust 开发者专注开发」人-周（pw）估算，含 UT。

### 5.1 短期（1-2 月）：接线 + 补最痛能力 —— 把「看起来有」变成「真有用」

**S1. 接线 RPM/TPM 限流 + 多级预算 + soft_budget 告警（P0，最高优先）**
- 为什么：M1/M2 是本次调研反复出现的最大欠账——企业采购 demo 一测即穿；litellm 在 auth 阶段硬编码全链执行（auth_checks.py:504-620 并发 gather 预算检查）。
- 做什么：① 在 4 个 handler（chat/v1_messages/responses/embeddings）请求入口挂 `RateLimiter.check` + `enforce_limits`；② `check_budget_multi`（key→user→team→org）接到同一入口；③ soft_budget 命中时发 webhook（`alerts.rs` 已实现）；④ `max_parallel_requests` 用 `tokio::sync::Semaphore` 每 deployment 加信号量。
- 参考实现：litellm `auth_checks.py:504-712`、`router.py:2886-2905`（Semaphore）；bdd_steps/rate_limit_steps.rs 已有测试脚手架。
- 工作量：2-3 pw。先接线 + 全链路 UT（bdd_steps 已有基础），再补 concurrency 测试。

**S2. 接线 Router 智能路由 + cooldown/fallback（P0）**
- 为什么：M4——`usage-based-routing-v2`/`latency-based-routing`/`report_failure/success`/`merge_router_overrides` 全部实现但请求路径零调用，cooldown 实际不触发（11 号笔记 §2.3：litellm 通过 failure_callback 全链自动接线）。
- 做什么：① `report_failure/report_success` 挂到上游返回路径，cooldown 状态真实推进；② `merge_router_overrides`（key>team>global）接入请求入口；③ `weighted`（按 weight/rpm/tpm 加权随机，对标 simple_shuffle.py:29-60）落地；④ fallback：实现「按错误类型触发」（429/5xx/context-window/content-policy）的 priority 排序 fallback（对标 Envoy provider-fallback / agentgateway priority failover）。
- 参考实现：litellm `router_strategy/simple_shuffle.py`、`cooldown_handlers.py`；Envoy provider-fallback 文档。
- 工作量：3-4 pw。S1+S2 合计 6-7 pw，是 1-2 月主战场。

**S3. exact-match 响应缓存（P1）**
- 为什么：M 缓存=0，全竞品标配；litellm `caching/` 有完整矩阵，最轻先行项是精确匹配。
- 做什么：① 内存缓存（moka 已在依赖）+ 可选 Redis 后端；② cache key = 参数拼接哈希（对标 litellm `Cache.get_cache_key`）；③ 响应头 `X-Cache-Status: HIT/MISS` + 支持 `cache={"use-cache","no-store","ttl"}` 控制；④ **流式组装后入缓存**（对标 `_add_streaming_response_to_cache`）;⑤ cache-hit 计费 0 元（litellm cache_hit 时 `response_cost=0.0`）——aigw `calc_spend` 已解析 cache tokens，直接复用。
- 参考实现：litellm `caching/caching.py`、`caching_handler.py`；Higress ai-cache（GJSON PATH 取末条 user 消息做 key）。
- 工作量：2-3 pw。

**S4. OpenAPI 覆盖补齐 + 本地 Swagger UI（P2）**
- 为什么：M6——19/92 端点入 spec，企业集成（SDK 生成）受阻；Swagger UI 依赖 unpkg CDN（内网不可用）。
- 做什么：① 把管理端点（keys/orgs/teams/users/budgets/spend/jobs/router-settings）补进 openapi.rs；② Swagger UI 静态资源 rust-embed 进二进制（去 CDN）。
- 工作量：1-2 pw。

**S5. 出站告警通道扩展（P2）**
- 为什么：M10——仅 soft_budget webhook 一种；litellm 21 种告警类型 + Slack/email/PagerDuty。
- 做什么：① 告警类型枚举扩展（budget/user/team/org/cooldown/outage）；② Slack/企微/Email 模板各一 + 简单去重（滑动窗口）；③ 从 `alerts.rs` 抽象 AlertDispatcher。
- 参考实现：litellm `SlackAlerting/slack_alerting.py`（1811 行）+ `budget_alert_types.py`。
- 工作量：2 pw。

### 5.2 中期（3-6 月）：AI 语义层 + 企业治理 + 中国市场差异点

**M1. Guardrails 最小集：regex 提示注入 + PII 脱敏 + 可选 Moderation hook（P0）**
- 为什么：NG2 长期路线但企业红线——商业云 6 类内容过滤 + litellm 50 hooks + 中国市场 Higress ai-security-guard 全有；aigw 0。
- 做什么：① regex allow/deny + 零宽字符/双向控制字符检测（对标 Kong ai-prompt-guard 免费版，Rust 易实现）；② 内置 PII（SSN/邮箱/手机正则，参考 litellm presidio recognizer 列表的确定性子集）；③ 预检 hook + 响应后 hook 架构（为第三方预留）；④ 按 aisix 模式预留外部审核 API 集成（OpenAI Moderation/Bedrock Guardrails 纯 HTTP 调用）。
- 参考实现：Kong `ai-prompt-guard`；litellm `enterprise_hooks/openai_moderation.py`；aisix guardrails 集成。
- 工作量：3-4 pw（确定性层）+ 外部 API 集成 1 pw。

**M2. Redis 分布式层：跨实例限流 + 预算 counter + 共享缓存（P0）**
- 为什么：41 号笔记 §3.2——SaaS 多实例（Stage 6）需分布式；litellm 全部 Redis 原子 Lua + 跨 pod counter；aigw 内存 token bucket 多实例失效。
- 做什么：① 引入 `redis` crate；② RPM/TPM counter + `max_parallel_requests` 迁 Redis（保留自研语义，governor 不建模 litellm 语义，41 号笔记结论）；③ 预算 counter（`spend:key/team/user/org:` 模式）+ `get_current_spend` 权威 DB floor 校验（对标 proxy_server.py:2242）；④ 缓存后端接 Redis。
- 参考实现：litellm `parallel_request_limiter_v3.py`（原子 Lua）、`budget_reservation.py`。
- 工作量：4-5 pw。

**M3. MCP 最小可行（P1）**
- 为什么：中国市场 MCP 已普及（Higress/百炼/千帆/七牛/智谱全有）+ 国际 Envoy/Kong/agentgateway/Portkey 全有；aigw NG3 建议重估。
- 做什么：最小版 = ① MCP SSE/streamable-http 透传（axum `ws` + tokio-tungstenite，NG7 WebSocket 一并低成本达成）；② 工具级 ACL（参考 agentgateway MCP 工具 RBAC）；③ 前端 MCP 配置页。
- 参考实现：agentgateway MCP 文档、Higress McpBridge、lx0758/AI-Gateway `/mcp/v1`。
- 工作量：3-4 pw。

**M4. 审计日志 + RBAC 权限矩阵（P1）**
- 为什么：企业级缺口最大项之一——商业云 IAM/CloudTrail 全有，litellm AuditLog 表 + 13 挂载点；aigw grep audit 0 结果。
- 做什么：① `audit_logs` 表（before/after JSON + actor）挂 key/team/org/user/budget/model CRUD（对标 LiteLLM_AuditLog）；② `user_role` 字符串升级为 per-resource permission（至少 admin/viewer/org_admin/team_admin，对标 litellm `_types.py:103` 角色枚举）。
- 参考实现：litellm `management_helpers/audit_logs.py`、`auth_checks.py`。
- 工作量：审计 2-3 pw + RBAC 3-4 pw。

**M5. 结构化 LLM 可观测：`gen_ai.*` 语义 + 指标扩展（P2）**
- 为什么：OTEL 已有 5 层 span 但无 GenAI 语义约定；Prometheus 14 vs litellm 47。
- 做什么：① span 补 `gen_ai.client.token.usage`/`gen_ai.server.request.duration`/`time_to_first_token`/`time_per_output_token`（对标 Envoy metrics / OTEL GenAI semconv）；② 指标扩展：budget remaining（key/team/org）、cache hits/misses、guardrail 计数、deployment RPM/TPM limit。
- 参考实现：litellm `prometheus.py`（47 metric）、`integrations/otel/`。
- 工作量：2-3 pw。

**M6. SSO/OIDC 重新评估（P2，可选差异化）**
- 为什么：NG1 永久跳过但企业采购频繁问；litellm 用通用 OAuth 兼容任何 IdP。
- 做什么：OIDC/OAuth（Keycloak/Okta/GitHub）先行（对标 litellm `ui_sso.py` generic OAuth）；SAML 视客户再议。
- 工作量：3-4 pw。

**M7. `/global/spend/users` 补齐 + 视频 token 估算 + embeddings 模态计费接线（P2）**
- 为什么：M7/M8——litellm 基线端点缺失 + TD-011a/TD-012b 剩余。
- 做什么：① 补 `/global/spend/users`；② 视频 token 估算（参考 litellm `default_video_cost_calculator`）+ Playground 视频输入；③ embeddings 模态计费接线（`calc_spend_modal` 已有纯数学 + UT）。
- 工作量：1-2 pw。

### 5.3 长期（6-12 月）：差异化护城河 + 平台化

**L1. 语义缓存 + 语义路由（差异化机会，P1）**
- 为什么：Rust 生态共同缺口（litellm-rs "config-rejected"、aisix 仅精确匹配、Cloudflare "规划中"）；aigw 已有 `/v1/embeddings` 基础设施可复用。
- 做什么：① fastembed（本地 ONNX embedding）→ Redis/pgvector 相似度比较 → 语义缓存（对标 Portkey semantic，threshold 0.95）；② 语义路由（embedding 相似度选 deployment，对标 aisix/Envoy）。
- 参考实现：`fastembed` crate + litellm `redis_semantic_cache.py`。
- 工作量：6-8 pw（含向量库选型压测）。

**L2. Data/Control Plane 分离（Stage 6 云服务蓝图，P1）**
- 为什么：Portkey/Helicone/Cloudflare 全部演进到控制面形态；aigw `DeploymentMode::SaaS` + `TenantAuth` 已定义但未接线。
- 做什么：① `TenantAuth` 接入路由（SaaS 租户隔离真正生效）；② 网关出站 1 分钟配置同步 + 离线可用 + 匿名指标推送（对标 Portkey hybrid 架构，TTL 7 天缓存）；③ 控制面 SaaS dashboard/配置/密钥/预算/guardrail 策略。
- 参考实现：Portkey `docs/self-hosting/hybrid-deployments/architecture`。
- 工作量：8-12 pw（分阶段）。

**L3. Prompt 模板 + 评测/实验（P2）**
- 为什么：Portkey Prompt Studio / Helicone prompts 已定义标准；TensorZero 归档留蓝本。
- 做什么：① Playground 升级 Prompt Studio（版本化 + `{{var}}` 模板 + 生产 Prompt API，对标 Kong ai-prompt-template）；② LLM-as-judge 评测（参考 TensorZero eval 设计）。
- 工作量：模板 3-4 pw + 评测 4-6 pw。

**L4. K8s Operator/Helm + PG 高可用 + BodyArchive 生命周期（P2）**
- 为什么：M11 长期路线未交付；企业交付面（Higress ComputeNest 一键 / 托管 SLA）。
- 做什么：① Helm chart + K8s Operator（对标 litellm helm chart）；② BodyArchive 日 compaction + S3 生命周期 + 监控指标（LT-Body*）；③ PG 生产高可用迁移文档。
- 工作量：6-8 pw。

**L5. 中国市场特性打包（P2，营销向）**
- 为什么：32 号笔记——Anthropic 双协议已是 aigw 差异化但未作为卖点。
- 做什么：① README/兼容矩阵强调「Claude Code 直连 + 国内双协议平台（混元/DeepSeek/七牛）对接」；② 中国 provider 基线文档（百炼 compatible-mode、方舟 endpoint ID、混元 stop 语义差异、智谱 temperature 0.6）；③ per-provider 参数差异在适配器文档化。
- 工作量：1 pw + 持续维护。

---

## 6. 差异化机会

### 6.1 Rust 网关自身（41 号笔记 + 23 号笔记交叉印证）

1. **性能 + 单二进制 + 低资源**：~20MB 单二进制、rust-embed 前端、SQLite 默认——agentgateway（30MB/165k QPS/sub-ms）、clewdr（15MB/<10MB RAM）、aisix（单二进制）已验证路线成立；Helicone 自托管要 6 组件（Worker/Jawn/Supabase/ClickHouse/Minio/Web），aigw 一键 Docker 完胜。
2. **litellm 数据/API 兼容是独有资产**：41 号笔记 §3.1 结论——没有任何 Rust 网关做 litellm schema/管理 API 兼容；aigw-migrate 双向迁移 + 表名解耦是护城河（charter §6）。
3. **边缘级能力**：Cloudflare 语义缓存"规划中"、aisix 仅精确匹配、litellm-rs 拒绝——**语义缓存/语义路由是 Rust 网关可领先的空白**（L1）。
4. **高精度计费**：三级缓存差异化 + 分模态 + image_tokens + TTFT，比 one-api 无缓存感知计费、比 Portkey/OpenRouter 仅 cost 聚合级都细（22 号笔记 §4.1）。

### 6.2 中国市场特性

1. **Anthropic 协议是 2026 中国底线**（混元/DeepSeek/七牛/智谱全部双协议，面向 Claude Code）——aigw 已有，且是 new-api 系（无 Anthropic 原生）没有的强点（32 号笔记结论）。
2. **三级缓存差异化计费正是为 DeepSeek/方舟 cache-hit 50-120x 价差设计**——这是对 one-api 型网关的营销点。
3. **中国市场 MCP 已普及**——补 MCP 最小透传即可对齐 Higress/百炼/七牛（M3）。
4. **中国市场计费预期 = 预算 + 缓存感知 + 免费路由**——aigw 预算体系 + 三级缓存计费已有，补「按 cost 的 usage-based 路由」即实现「自动选最便宜可用模型」（32 号笔记 §4.2）。

### 6.3 不该做的（避免浪费，多个笔记共识）

- 30+ provider 原生 handler（NG9，所有中国平台都是 OpenAI/Anthropic 兼容，通用适配器覆盖）。
- 用户自助充值/兑换码/在线支付（中转站生意非网关产品）。
- Envoy/K8s 生态本身、InferencePool GPU 感知路由、Gateway API（单二进制定位无需）。
- Kong 企业化插件模式、纯 Rust ML 内容审核训练（生态不成熟，走外部 API + regex 层）。

---

## 7. 风险与约束

1. **人力瓶颈（最大风险）**：aigw 是单团队维护 ~122 个 .rs 文件 + 前端 23 页面 + 三方言迁移。S1+S2 短期 6-7 pw、中期 M1-M7 合计 18-24 pw、长期 L1-L5 合计 28-36 pw——需按 P0→P2 严格排序，避免「接线 5% 收益的完美主义」。
2. **litellm 追赶成本**：litellm 55k stars、每日高频提交（HEAD 合 35773 个 PR）；aigw 作为"最小兼容替代"不可能全量追赶，**必须坚持 charter NG 收窄**——每次 litellm schema/API 变化都要评估是否跟进（aigw-migrate 表名解耦已降低迁移成本）。
3. **维护负担（Provider/协议漂移）**：上游 provider 参数差异（混元 stop 语义、智谱 temperature、百炼 tools+stream 限制）需持续文档化；litellm 每次上游变化 aigw 需验证协议转换正确性。
4. **许可与治理**：仓库 LICENSE 状态需核对；若面向商业/中国客户分发，需明确 Apache-2.0 或 MIT 并补 CONTRIBUTING/CLA（41 号笔记「开源治理」维度）。
5. **「接线代码」的隐蔽风险**：S1/S2 涉及把已实现但从未在请求路径跑过的模块上线，**必须补真实 BDD（real BDD 三后端）+ 并发/TOCTOU 测试**（12 号笔记明确指出 aigw 只在请求前读 DB spend 列、TOCTOU 注释自己承认并发窗口）。
6. **语义缓存/向量库的运维复杂度**（L1）：引入 Redis/pgvector 后，跨实例一致性、embedding 模型版本、相似度阈值调参都会成为长期运维负担，需压测后决定。

---

## 8. 证据与来源

### 8.1 本地调研笔记（全部经代码/文档核验）

| 笔记 | 内容 | 关键引用 |
|---|---|---|
| `00-aigw-baseline.md` | aigw 功能基线（116/116 Stages 完成） | 已有/缺失速览 §附 |
| `11-litellm-core.md` | litellm provider/Router/proxy 管道/流式/工具/模型管理 | router.py:410-418、auth_checks.py:504、pattern_match |
| `12-litellm-cost-spend.md` | litellm 定价/预算/限流/花费/缓存 | budget_reservation.py:147、caching/、model_prices 2988 条 |
| `13-litellm-enterprise.md` | litellm 企业版 + 可观测 + 集成 + guardrails | 694k py LOC、50 guardrail hooks、SlackAlerting 21 类型 |
| `21-intl-k8s.md` | Envoy AI Gateway / Kong / agentgateway / APISIX / aisix | token→限流闭环、priority fallback、guardrail 分层 |
| `22-intl-control-plane.md` | Portkey / Helicone / OpenRouter | Data/Control Plane 分离、语义缓存、guardrail 更严格者胜 |
| `23-intl-edge-cloud.md` | Cloudflare / Bedrock / Azure / Vertex / 国内商业平台 | DLP、Quota Tiers、内容过滤、IAM/审计 |
| `31-china-one-api.md` | one-api / new-api 深挖（源码级） | channel/ModelRatio/钱包充值/分组倍率 |
| `32-china-others.md` | Higress + 国内平台网关形态 | ai-token-ratelimit、MCP 标配、双协议底线 |
| `41-rust.md` | Rust 网关生态盘点 + 借力点评估 | tensorzero 归档、litellm Rust 化、fastembed/governor/redis-rs |

### 8.2 本仓库代码核验（本报告抽查）

- `crates/aigw-server/src/main.rs:223,303` —— `RateLimiter::new()` 创建并注入 AppState，**无调用点**（M1）
- `crates/aigw-core/src/budget.rs:152` —— `check_budget_multi` 定义，**生产代码 0 调用**（M2）
- `crates/aigw-server/src/openapi.rs:12` —— "The spec covers all 19 endpoints"（M6）
- `crates/aigw-server/src/main.rs` 92 条 `.route(` vs openapi 19 端点（M6）
- `crates/aigw-server/src/main.rs:481` —— 仅有 `/spend/users`，**无 `/global/spend/users`**（M7）

### 8.3 外部来源（竞品画像逐条 URL）

详见各笔记尾部来源清单，这里给出可复核的骨架：
- litellm 源码：`/tmp/aigw-gap-research/litellm`（HEAD=ecba48d）+ https://github.com/BerriAI/litellm
- Envoy AI Gateway：https://aigateway.envoyproxy.io/ ；https://github.com/envoyproxy/ai-gateway
- Kong：https://developer.konghq.com/ai-gateway/ ；https://github.com/Kong/kong
- agentgateway：https://agentgateway.dev/ ；https://github.com/agentgateway/agentgateway
- Portkey：https://github.com/Portkey-AI/gateway ；https://portkey.ai/docs/self-hosting/hybrid-deployments/architecture ；PAN 收购 https://www.paloaltonetworks.com/company/press/2026/
- OpenRouter：https://openrouter.ai/docs ；https://github.com/OpenRouterTeam
- Helicone：https://github.com/Helicone/helicone ；Mintlify 收购 https://www.mintlify.com/blog/mintlify-acquires-helicone
- Cloudflare AI Gateway：https://developers.cloudflare.com/ai-gateway/
- AWS Bedrock：https://docs.aws.amazon.com/bedrock/latest/userguide/guardrails.html
- Azure Foundry：https://learn.microsoft.com/en-us/azure/foundry/ ；Quotas https://learn.microsoft.com/en-us/azure/foundry/openai/quotas-limits
- Higress：https://higress.cn/en/ai-gateway ；ai-proxy https://higress.cn/en/docs/latest/user/plugins/ai/api-provider/ai-proxy/
- aisix：https://github.com/api7/aisix
- one-api / new-api：https://github.com/songquanpeng/one-api ；https://github.com/QuantumNous/new-api
- tensorzero / litellm-rs / async-openai / fastembed / governor / redis-rs / opentelemetry-rust：41 号笔记 §4

---

## 附：行动清单速查（供 Stage 规划）

| 时间窗 | 项 | 优先级 | 工作量 | 一句话 |
|---|---|---|---|---|
| 短期 | S1 限流+多级预算+max_parallel 接线 | P0 | 2-3 pw | 最大欠账，企业 demo 一测即穿 |
| 短期 | S2 Router 智能路由+cooldown/fallback 接线 | P0 | 3-4 pw | usage/latency 策略代码在但运行时失效 |
| 短期 | S3 exact-match 响应缓存 | P1 | 2-3 pw | 全竞品标配，aigw 为 0 |
| 短期 | S4 OpenAPI 覆盖 + 本地 Swagger | P2 | 1-2 pw | 19/92 端点，企业 SDK 集成受阻 |
| 短期 | S5 出站告警通道扩展 | P2 | 2 pw | 仅 1 种 webhook vs litellm 21 类型 |
| 中期 | M1 guardrails 最小集（regex+PII+Moderation hook） | P0 | 4-5 pw | 企业红线，参考 Kong ai-prompt-guard |
| 中期 | M2 Redis 分布式层（限流/counter/缓存） | P0 | 4-5 pw | SaaS 多实例必需 |
| 中期 | M3 MCP 最小透传 + WebSocket | P1 | 3-4 pw | 中国市场 2026 标配 |
| 中期 | M4 审计日志 + RBAC 矩阵 | P1 | 5-7 pw | 合规刚需 |
| 中期 | M5 `gen_ai.*` 可观测语义 + 指标扩展 | P2 | 2-3 pw | 对齐 OTEL GenAI semconv |
| 中期 | M6 SSO/OIDC 重估 | P2 | 3-4 pw | 企业采购高频问 |
| 中期 | M7 `/global/spend/users` + 视频 token + embeddings 计费 | P2 | 1-2 pw | 收尾 TD-011a/012b |
| 长期 | L1 语义缓存 + 语义路由 | P1 | 6-8 pw | Rust 生态空白，差异化机会 |
| 长期 | L2 Data/Control Plane 分离（Stage 6） | P1 | 8-12 pw | Portkey 蓝图 |
| 长期 | L3 Prompt 模板 + 评测 | P2 | 7-10 pw | TensorZero 蓝本 |
| 长期 | L4 K8s/Helm + BodyArchive 生命周期 + PG HA | P2 | 6-8 pw | 企业交付面 |
| 长期 | L5 中国市场特性打包（营销向） | P2 | 1 pw | Anthropic 双协议 + 缓存计费卖点 |
