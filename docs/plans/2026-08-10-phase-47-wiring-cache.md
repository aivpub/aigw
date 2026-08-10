# Phase 47 规划：A 类接线 + exact-match 缓存（S1+S2+S3）

**日期**: 2026-08-10
**背景**: 差距调研（`docs/research/2026-08-09-aigw-gap-vs-industry-leaders.md`）确认 aigw 最大欠账是 **A 类「代码在但运行时未接线」**——RPM/TPM 限流、多级预算 `check_budget_multi`、`soft_budget` 告警、`max_parallel_requests`、Router 智能路由（usage/latency/weighted/cooldown/fallback/merge_overrides）全部已实现且有 UT，但请求路径零调用点，生产上不生效。已逐条在仓库代码核实（`enforce_limits` 仅 test 调用、`check_budget_multi` 仅 `#[cfg(test)]`、`select_instance`/`merge_router_overrides` 仅测试模块）。**企业采购 demo 一测即穿。**

其次 **B 类「缓存=0」**：exact-match 响应缓存是全部竞品标配（litellm caching 矩阵 / Portkey / Cloudflare 边缘 / Higress ai-cache），aigw 为零。

本 Phase 规划三项（S1+S2+S3）收敛为 3 Stage 串行交付。

---

## 1. 现状核实结论（逐条带代码证据）

| 项 | 现状 | 关键证据 |
|----|------|----------|
| **RPM/TPM 限流** | 实现未接线 | `RateLimiter`（rate_limiter.rs 252 行，token bucket）+ `enforce_limits`（middleware/rate_limit.rs:126 完整实现含多级预算+RPM/TPM）零请求路径调用；`main.rs` 创建注入 AppState 后仅测试用 |
| **多级预算 `check_budget_multi`** | 实现未接线 | `BudgetEnforcer::check_budget_multi`（budget.rs:152，key→user→team→org）生产零调用；实际只生效单级 key `max_budget` 内联检查 |
| **`soft_budget` 告警** | 实现未接线 | `aigw_core::alerts`（alerts.rs 146 行，AlertDispatcher + webhook）已实现（TD-007），仅被未接线的 enforce_limits 引用 |
| **`max_parallel_requests`** | 字段无执行 | key/budget 表字段存储，无信号量 |
| **Router 智能路由** | 仅 SimpleShuffle 生效 | `Router::pick_deployment`（router.rs）仅 shuffle+cooldown 过滤；Strategy 枚举其余值（usage-based/latency-based）+ `select_instance`/`report_failure`/`report_success`/`merge_router_overrides` 均未接入请求路径；cooldown 实际不触发 |
| **weighted 路由** | 未实现 | 无按 weight/rpm/tpm 加权随机 |
| **fallback** | 无 | 无按错误类型（429/5xx/context-window/content-policy）的 priority 排序 fallback |
| **exact-match 缓存** | 无（=0） | 无缓存层；仅计费解析 cache tokens；`calc_spend` 已支持 cache 三级计费（可复用做 cache-hit 0 元） |
| **OpenAPI 覆盖** | 19/92 端点 | openapi.rs expected_endpoints；Swagger UI 依赖 unpkg CDN |

## 2. Stage 拆分（3 Stage，串行依赖，共 40h）

### Stage 117：A 类接线核心 — 请求入口挂限流 + 多级预算 + 告警 + max_parallel（S1）— 16h

**目标**: 把已实现的 `enforce_limits`（含 `check_budget_multi` + RPM/TPM）+ `soft_budget` 告警 + `max_parallel_requests` 信号量接到 4 个 LLM handler 请求入口，让 A 类最大欠账在生产生效。

| 子项 | 内容 | 落点 |
|------|------|------|
| **① RPM/TPM 限流接线** | 4 handler（chat/v1_messages/responses/embeddings）入口调 `enforce_limits(&db, &rate_limiter, &identity, token_estimate)`；`token_estimate` 从请求 body `max_tokens`/输入估算（无则 0 只查 RPM+预算） | 4 handler 共享 guard 函数 |
| **② 多级预算接线** | `enforce_limits` 内已含 `check_budget_multi`（key→user→team→org）——接线后自动生效；`KeyIdentity` 由认证层完整填充（user_id/team_id/org_id 已在 auth 解析） | 复用 enforce_limits |
| **③ soft_budget 告警** | `check_budget_multi` 命中 `soft_budget`（spent > soft_budget < max_budget）时调 `alerts::AlertDispatcher` 发 webhook（已实现）；记录日志；`spent >= max_budget` → 429/403 拒绝 | budget.rs 告警分支 + enforce_limits |
| **④ max_parallel_requests** | `tokio::sync::Semaphore` 每 deployment（按 api_base+model 分桶）信号量；`Deployment`/key 的 max_parallel_requests 值控制许可数；超限 429 `x-ratelimit` 头语义 | router.rs / handler 上游调用段 |
| **⑤ 竞态修复（12 号笔记）** | 预算检查前置 + spend 递增后置间的 TOCTOU 窗口：检查时读当前 spend 列 → 通过后请求 → 完成后异步增量。改为「请求前读 spend + 完成后写回」保持现状但**文档化窗口** + `check_budget_multi` 用 DB 事务读快照 | budget.rs / spend 写入 |
| **⑥ BDD + 并发测试** | 真实验证：RPM/TPM 超限 429、多级预算拒绝、soft_budget 告警 webhook、max_parallel 排队 | bdd_steps + real BDD 三后端 |

**TDD 预估**: core 8-10 UT + handler 4 UT + mock BDD 4-5 + real BDD 三后端 3-4 场景。

### Stage 118：Router 智能路由接线 + weighted + cooldown/fallback（S2）— 14h

**目标**: 让已声明的策略真实生效 + 补 weighted 与错误类型 fallback。

| 子项 | 内容 | 落点 |
|------|------|------|
| **① report_failure/report_success 接线** | 上游返回路径（成功/失败/超时）调 `Router::report_*`，cooldown 状态真实推进；失败计数（429/401/408/404/5xx）→ 超过 allowed_fails → cooldown 排除（对标 litellm cooldown_handlers.py） | router.rs + chat 上游调用 |
| **② merge_router_overrides 接线** | 请求入口按 key>team>global 优先级合并 router_settings（`merge_router_overrides` 已实现）→ 路由选择用合并后配置 | handler 入口 + router.rs |
| **③ weighted 路由** | 按 deployment weight / rpm / tpm 加权随机选择（对标 litellm simple_shuffle.py:29-60 `random.choices` 归一化） | router.rs `pick_deployment` |
| **④ usage/latency 变体** | `UsageBased`（按 5 分钟窗口 RPM/TPM 剩余）与 `Latency`（EWMA 响应时间）`pick_deployment` 分支——Stage 116 已声明变体，此处实现真实决策 | router.rs |
| **⑤ 错误类型 fallback** | 实现 priority 排序 fallback：候选 deployment 列表按 priority 分组，当前失败按错误类型（429/5xx/context-window/content-policy）触发切换到下一组（对标 Envoy provider-fallback / agentgateway priority failover）；最多 N 次尝试 | router.rs fallback 模块 |
| **⑥ 前端 RouterSettings 启用** | `routing_strategy` 下拉启用 usage-based/latency-based（当前 disabled），加 weight/rpm/tpm 输入 | router-settings/index.tsx |

**TDD 预估**: router 10-12 UT + handler 2-4 + mock BDD 4-6 + fe-bdd 2 场景。

### Stage 119：exact-match 响应缓存（S3）— 10h

**目标**: 落地精确匹配响应缓存（内存 moka + 可选 Redis），全部竞品标配能力。

| 子项 | 内容 | 落点 |
|------|------|------|
| **① 缓存层** | `aigw_core::cache`：moka LRU 内存后端 + `CacheBackend` trait（预留 Redis）；key = 参数拼接哈希（provider+endpoint+model+auth+body，对标 litellm `Cache.get_cache_key`） | 新模块 core/cache/ |
| **② 读写路径** | 非流式响应组装后入缓存；流式响应 `stream_chunk_builder` 组装后入缓存（对标 litellm `_add_streaming_response_to_cache`）；缓存 TTL 默认 60s（`cache={"ttl"}` 可覆盖） | chat.rs 上游返回段 |
| **③ cache 控制** | 请求 `cache={"use-cache","no-store","ttl"}` 解析；响应头 `X-Cache-Status: HIT/MISS` | handler 入口 + 响应 |
| **④ cache-hit 计费 0 元** | 缓存命中时 `response_cost=0`（litellm 行为）；`calc_spend` 已有 cache 三级计费逻辑复用 | spend.rs / calc_spend |
| **⑤ config 接线** | `config.yaml` 增 `cache: {enabled, backend: memory|redis, ttl_seconds, max_entries}`；boot 构建注入 AppState | config.rs + main.rs |
| **⑥ BDD** | HIT/MISS 头、no-store 绕过、ttl 过期、cache-hit 计费 0 元 | bdd_steps |

**TDD 预估**: cache 8-10 UT + handler 2-4 + mock BDD 4-5。

---

## 3. 交付顺序决策

- **Stage 117 优先**：A 类最大欠账（企业 demo 一测即穿），纯后端接线 + 现成测试脚手架，立即释放价值。
- **Stage 118 次之**：依赖 117 的 guard/身份上下文；Router 增强独立可测。
- **Stage 119 最后**：缓存是新能力，作为接线完成后第一个新能力补齐。

## 4. 后续跟进（Stage 117-119 完成后）

- **中期（3-6 月）**：M1 guardrails 最小集（regex 提示注入 + PII 脱敏 + Moderation hook）、M2 Redis 分布式层（跨实例限流/预算 counter/共享缓存）、M3 MCP 最小透传、M4 审计日志 + RBAC、M5 `gen_ai.*` 可观测语义。
- **长期**：L1 语义缓存 + 语义路由（fastembed 本地 ONNX → Redis/pgvector，Rust 生态空白差异化）、L2 Data/Control Plane 分离、L3 Prompt 模板 + 评测、L4 K8s/Helm + BodyArchive 生命周期。

## 5. 验收标准（跨 Stage）

- `task test`（aigw-core + aigw-server UT 全绿）+ `task bdd` mock BDD 全绿 + `task bdd-real-*` 三后端全绿
- **Stage 117**：RPM/TPM 超限 429、多级预算（key→user→team→org）逐级拒绝、soft_budget 触发 webhook、max_parallel 排队 429（BDD 断言）
- **Stage 118**：cooldown 排除（失败 N 次后命中被跳过）、weighted 路由命中率、usage/latency 选择生效、fallback 切换（429→下一优先组）、前端下拉启用
- **Stage 119**：`X-Cache-Status: HIT/MISS`、no-store 绕过、TTL 过期、cache-hit 计费 0 元（BDD 断言 + `calc_spend` 复用）
- fmt + lint green；cargo check 无 warning

## 6. 参考实现（对齐调研证据）

| 子项 | 参考 |
|------|------|
| RPM/TPM + 多级预算接线点 | litellm `auth_checks.py:504-712`（auth 阶段全链执行）+ `common_request_processing.py:1623` |
| max_parallel Semaphore | litellm `router.py:2886-2905`（asyncio.Semaphore 包调用） |
| weighted 路由 | litellm `router_strategy/simple_shuffle.py:29-60` |
| cooldown/fallback | litellm `cooldown_handlers.py` + `router_strategy/*`；Envoy provider-fallback / agentgateway priority failover |
| exact-match 缓存 | litellm `caching/caching.py` + `caching_handler.py`；Higress ai-cache（末条 user 消息做 key） |
| cache-hit 计费 0 元 | litellm cache_hit → `response_cost=0.0` |
