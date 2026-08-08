# OpenAI Embeddings API 代理支持 — 调研报告

**日期**: 2026-08-08
**状态**: Accepted（用户已答复 5 项决策，规划为 Phase 44；2026-08-08 由原 Phase 45 重编号）
**作者**: Claude Code Agent（6 路 subagent：4 研究 + 1 代码审计 + 1 综合）

---

## 1. 结论

**aigw 应当实现 OpenAI-compatible Embeddings 代理（`POST /v1/embeddings`）。** 这是"litellm 最小兼容替代"定位的前置要求，工程成本 ~1 Stage（8-10h）即可落地 Passthrough。用户已确认：

1. **已有部分 Embedding 应用流量**，并希望尝试更多 Embedding 应用 → 现在交付。
2. **有本地和托管的 embedding 模型**（vLLM/BGE/Qwen3/Ollama + OpenAI text-embedding-3）→ 薄 OpenAI-compatible Passthrough 已覆盖。
3. **排在在途 P1 收尾之后** → 规划为 **Phase 44**（2026-08-08 由原 Phase 45 重编号：原"Phase 44 在途 P1 收尾"是无 Stage 的待办桶，降级为无 Phase 号待办，Embeddings 承接 Phase 44 编号）。
4. **四种风格端点都需要** → `/v1/embeddings` + `/embeddings` + `/engines/{model}/embeddings`（Azure legacy）+ `/openai/deployments/{model}/embeddings`（Azure）。
5. **health.rs embedding-mode 探测** → 非阻塞（记技术债）。

---

## 2. 调研证据

### 2.1 LiteLLM 把 Embeddings 当一等公民（aigw 参照实现）

| 维度 | LiteLLM 行为 | 来源 |
|------|-------------|------|
| 端点路径 | `/v1/embeddings` + `/embeddings` + `/engines/{model}/embeddings` + `/openai/deployments/{model}/embeddings`（Azure），全部 OpenAI SDK 兼容 | `litellm/proxy/proxy_server.py` L9545-9567 |
| call_type | 代理侧 `"aembedding"`，SDK 直调 `"embedding"`；chat=`"completion"`，responses=`"responses"` | `litellm/types/utils.py` CallTypes 枚举 |
| 计费 | **仅 `prompt_tokens × input_cost_per_token`**，`output_cost_per_token=0`（cost map 中 124 个 embedding 模型） | `litellm/cost_calculator.py` |
| usage | `prompt_tokens` + `total_tokens`（total==prompt，completion_tokens=0），spend_logs 两列都存 | `litellm/spend_tracking/spend_tracking_utils.py` |
| 管道 | 与 chat **完全一致**：auth → budget → TPM/RPM → pre/post hooks → spend-log，**无浅路径** | `litellm/proxy/proxy_server.py` embeddings handler |
| Provider | OpenAI/Azure/Bedrock(titan/nova/cohere/twelvelabs)/Cohere/Voyage/Gemini/Vertex/Mistral/NVIDIA NIM/HF/Nebius + `openai/`-前缀本地兼容 | `docs.litellm.ai/docs/embedding/supported_embedding` |
| 状态 | OSS 一等公民，无 Enterprise gate，无弃用 | — |

**对 aigw 的含义**：litellm 兼容的网关若缺 embeddings 端点，等于功能不完整。

### 2.2 市场趋势：Embeddings/RAG 未衰退，仍在增长

- **Anthropic《Building Effective Agents》**：retrieval、tools、memory 是"增强 LLM"三大增广，检索未被 agent 取代。
- **Anthropic《Effective Context Engineering》**："许多 AI 原生应用采用基于 embedding 的推理前检索"；上下文窗口再大也受 context pollution 影响，检索仍是补充。
- **a16z**：上下文窗口越大，embedding 管道**越重要**（大上下文计算成本高）。
- **OpenAI**：text-embedding-3-small/large/ada-002 仍按 input token 计费，真实计费负载。
- **embedding 模型 2025-2026 仍在加速迭代**：Qwen3-Embedding（2025-06，MTEB 多语言 SOTA）、gemini-embedding-2（多模态）、BGE-M3。
- ⚠️ 市场数据（RAG 市场 ~32% CAGR、"68% Fortune 500"）来自分析师/厂商，方差大，仅作方向性参考。

### 2.3 竞品网关：leader 级必配，非普适 table-stakes

| 支持（一等公民） | 缺失 |
|---|---|
| LiteLLM、Kong AI Proxy（GA 3.11）、Portkey、**new-api**（中国） | Cloudflare AI Gateway（2025-06 后一直未发）、Helicone、Azure APIM |

**含义**：aigw 对标 LiteLLM/Kong/Portkey/new-api，embeddings 属 leader 级 parity；对标 Cloudflare/Helicone 则非必须。

### 2.4 Embedding 模型现状（2026）

| 模型 | 端点 | 输入上限 | 计费 |
|------|------|---------|------|
| OpenAI text-embedding-3-small/large/ada-002 | `/v1/embeddings`（原生） | 8,192 tokens | input token 计费，dimensions 不改变价格，批量数组支持 |
| Gemini gemini-embedding-2（多模态） | `:embedContent`（原生，**非** OpenAI 格式） | 8,192 tokens（跨模态共享） | 按模态单价（text $0.20 / image $0.45 / audio $6.50 / video $12.00 per 1M） |
| Qwen3-Embedding（0.6B/4B/8B） | vLLM `openai/` 兼容 `/v1/embeddings` | 32K context | 本地推理，OpenAI 兼容 |
| BGE / NV-Embed | vLLM `/v1/embeddings` | — | 本地推理 |

**含义**：`openai/`-前缀 Passthrough 覆盖 OpenAI 托管 + 本地 vLLM/BGE/Qwen3。Gemini/Cohere 原生格式翻译属于差异化层，本阶段不做。

---

## 3. aigw 代码现状审计

### 3.1 现状（已验证）

- **Zero embedding 代码**：`crates/` 与 frontend grep `embedding` 无任何匹配。
- 路由仅：`/v1/chat/completions`、`/v1/models`、`/v1/messages`、`/v1/responses`（main.rs L388-522）。
- `call_type` 自由文本列（002_spend_logs.sql L7），新字符串端到端零 schema 变更。

### 3.2 可原样复用（无 gap）

| 组件 | 位置 | 说明 |
|------|------|------|
| `ChatAuth` | chat.rs L440-472（responses.rs 已 re-export） | 认证提取，原样复用 |
| `resolve_key_model_list` | chat.rs L645-760 | 模型权限/哨兵展开 |
| `calc_spend` | chat.rs L103-122 | 已按 prompt-only 计费（completion=0 → 零输出成本） |
| `OpenAIPassthrough` | adapter.rs L112-141 | adapt_request 只改 model + 注入 stream_options（embeddings 永不流式），adapt_response 恒等透传 |
| `ModelResolver::resolve` | resolver.rs L40-106 | 任意 model_name → Vec<Deployment> |
| `Router::pick_deployment` | router.rs L262-288 | 负载均衡 |
| `/v1/models` | chat.rs L2038-2103 | 已列出任意 proxy_models 行（model_info.mode 透传），embedding 模型天然可见 |
| 前端 call_type badge | spend-logs/index.tsx L687/L1445/L1526 | `call_type` 原样渲染，无映射表，`"embedding"` 直接显示 |

### 3.3 缺口清单（按优先级）

| # | 缺口 | 工作量 | 说明 |
|---|------|--------|------|
| 1 | `routes/embeddings.rs` 新 handler + main.rs 注册 + mod.rs | **L** | responses.rs 的非流式克隆 |
| 2 | **adapter 陷阱**：`select_adapter(OpenAI, AnthropicNative)` → `OpenAIToAnthropic` 会破坏 embedding body | S | handler 必须硬选 `OpenAIPassthrough`/OpenAICompatible，或拒绝 AnthropicNative 部署 |
| 3 | `responses.rs` 的 `extract_prompt_tokens`/`extract_total_tokens` 私有 | S | 提升 `pub(crate)` 复用（embeddings usage 即 `{prompt_tokens, total_tokens}`） |
| 4 | SpendLog `call_type="embedding"`（区别于 completion/responses） | S | 自由文本，零 schema |
| 5 | mock_upstream.rs `/v1/embeddings` handler + `embeddings.feature` + `embeddings_steps.rs` | M | ~11 场景 |
| 6 | openapi.rs `embeddings_spec()` + `expected_endpoints` 18→19 | S | 现仅 chat/models 有 spec |
| 7 | 前端 `OutputCard.tsx` `parseOutput` 加 `data[]` 分支 | M | 否则 embedding 响应详情抽屉空态 |
| 8 | 文档：charter L91/L204、roadmap Phase 44、next-steps | S | 现明确排除 embeddings |
| 9 | health.rs 探测按 `model_info.mode` 分支 | S | **非阻塞**（用户确认） |

---

## 4. 用户决策记录

| # | 决策 | 结论 |
|---|------|------|
| 1 | 是否有 embedding/RAG 流量 | **有，想尝试更多 Embedding 应用** → 现在交付 |
| 2 | 本地/托管 embedding 模型 | **都有** → 薄 OpenAI-compatible Passthrough 覆盖 |
| 3 | 排期 | **在途 P1 收尾之后** → 规划为 **Phase 44**（原 Phase 45，2026-08-08 重编号） |
| 4 | 端点风格 | **四种都需要** → `/v1/embeddings` + `/embeddings` + 两个 Azure 别名 |
| 5 | health.rs embedding-mode 探测 | **非阻塞** → 记技术债 |

---

## 5. 建议 scope

**薄 OpenAI 兼容 Passthrough**（不做协议翻译）：
- `openai/`-前缀覆盖 OpenAI 托管 + vLLM/BGE/Qwen3 本地推理
- 不做 Gemini `:embedContent` / Cohere `/v2/embed` 翻译（Envoy 刚合并的差异化层，等真实 RAG 负载再上）
- 计费 = `prompt_tokens × input_cost_per_token`，`output_cost=0`
- call_type = `"embedding"`

---

## 6. 来源

- https://docs.litellm.ai/docs/embedding/supported_embedding （LiteLLM embeddings 文档）
- https://docs.litellm.ai/docs/providers/openai （LiteLLM OpenAI 文档）
- https://docs.litellm.ai/docs/providers/openai_compatible
- https://docs.litellm.ai/docs/providers/bedrock
- https://developers.openai.com/api/docs/guides/embeddings （OpenAI embeddings 指南）
- https://ai.google.dev/gemini-api/docs/embeddings （Gemini embeddings）
- https://docs.vllm.ai/en/latest/models/pooling_models.html （vLLM OpenAI 兼容 embeddings）
- https://www.anthropic.com/research/building-effective-agents
- https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents
- https://a16z.com/emerging-architectures-for-llm-applications/
- https://www.databricks.com/glossary/retrieval-augmented-generation-rag
- https://developer.konghq.com/plugins/ai-proxy/ （Kong llm/v1/embeddings）
- https://github.com/envoyproxy/ai-gateway/pull/2114 （Envoy OpenAI→GCP/Gemini embedding 翻译，2026-06-08 合并）
- https://docs.newapi.pro/en/docs/api/ai-model/embeddings/createembedding （new-api embeddings）
- https://github.com/QuantumNous/new-api （README Embedding Interface）
- https://developers.cloudflare.com/ai-gateway/ （Cloudflare AI Gateway 无 embeddings）

> LiteLLM 源码（proxy_server.py / cost_calculator.py / spend_tracking_utils.py）经 WebFetch 逐行核实。
