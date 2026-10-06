# 网关侧「自己执行搜索」架构调研 — 服务端工具模拟 / agentic loop

> 调研日期:2026-10-06 | 用途:TD-017c（服务端工具真实支持）独立 Phase 立项输入
> 前置:`docs/research/2026-10-05-websearch-server-tool-support.md`(协议层处置三策略 + 生产网关实测)
> 本篇定位:**不重复协议转换**,专攻「网关自己发起搜索」的架构——hook 点、循环驱动、流式、客户端契约、**计费归集**
> 方法:直接读源码(litellm `168a0055a2` / sub2api `f8f0f07f6` / new-api `a63364d1`)+ 官方文档 WebFetch
> 标注约定:**已验证**=有源码路径+行号或官方文档 URL;`未验证`=未能取得一手证据

---

## 0. 一句话结论

**调研覆盖 13 个网关,结论是「网关自己执行搜索」已成主流而非个例:7 家有 web-search 专用特性(litellm、sub2api、Higress、Portkey、Vercel、LangDB、OpenRouter),6 家透传,aigw 当前的「丢弃」属最保守一档。** 业界「网关自执行」有三种成熟形态,而非两种:**(A)「短路」**——请求里只有 web_search 工具时根本不调模型,网关搜完直接合成一个假的 assistant 回复返回(litellm、sub2api);**(B)「agentic loop」**——把 web_search 换成普通 function 工具发给模型,模型回 tool_call 后网关执行搜索、回灌、再请求一轮,上限默认 3(litellm、LangDB);**(C)「prompt 注入」**——发往上游之前先搜,把结果改写进最后一条 user 消息或 system 消息,模型只调一次(Higress `ai-search`、Portkey `exa/online`)。另 Vercel 在网关内执行 server tool 并**刻意隐藏中间态**(`finish_reason:"stop"`,不回传 `tool_calls` 与原始结果),Cloudflare 则**明确声明不做 provider-agnostic 的搜索抽象**。

**形态 C 是本环境最优先的候选**:它对上游**零能力要求**(不需要 function calling、不需要服务端工具)、保留模型综合能力、流式几乎无改动、且**不需要向客户端合成任何服务端工具块**——从而完全绕开下面两个硬约束。

**两个决定设计上限的硬约束(本次调研关键发现)**:① Anthropic 的 `web_search_tool_result.encrypted_content` 是**服务端签发的不透明 blob,网关无法伪造,且多轮回放时会被逐字节校验**(缺失/改动 → 400)——故「网关自搜 + 合成 Anthropic 服务端工具块」**在协议上根本不可能保真**,litellm/sub2api 都只能用附加键(`snippet`/`page_content`)兜底;② 正因如此,自造块一旦被客户端(Claude Code)回放就会污染后续请求,**必须在发往上游前按 id 前缀剥离**(sub2api 用 `srvtoolu_ws_` 标记自造块)——此配套缺失会导致「第一轮能搜、第二轮整个会话 400」。**形态 C 不产生自造块,故两个约束均不适用。**

**流式**:形态 A 手写固定事件序列(sub2api),形态 B **一律降级为非流式跑完循环再重建 SSE**(litellm `FakeAnthropicMessagesStreamIterator`;Bifrost Agent Mode 索性明确不支持流式,Vercel / LangDB 的流式支持均 `未验证`),形态 C 只需在上游 SSE 里注入 references(Higress)。**没有任何一家做「边循环边真流式」——这是业界普遍难点。**

**计费**:四种呈现方式——litellm **单独一条 SpendLog 行**(`call_type="asearch"`,按 `input_cost_per_query` 计价,24 个 provider 真实价目见 §2.8)、new-api **附加费并进同一条行**(`other.web_search_price`)、Vercel **回传进响应体** `provider_metadata.gateway.gatewayToolCalls`、sub2api / Higress / Portkey **完全不计费**。

**对 aigw 的设计建议见 §5.3 / §5.6 —— 推荐阶段 1 做形态 C(prompt 注入)+ 计费方案 1(独立 SpendLog 行)+ 方案 4(回传 `usage.server_tool_use.web_search_requests`)**,阶段 2 再按需补短路(A,服务 Claude Code 的独立搜索子请求)与 agentic loop(B);**(B) 的硬前提「上游 MaaS 支持网关注入的 function 工具往返」尚未实测**(§5.9)。

---

## 1. 横向对照表

### 1.1 策略 × 流式 × 计费

| 网关 | 策略 | 真实执行? | 流式支持 | 计费归集 | 证据 |
|------|------|----------|---------|---------|------|
| **litellm**(短路 A) | **真实执行**(不调模型) | ✅ | N/A(短路返非流式 dict,由上层包装) | **独立 SpendLog 行**(`call_type="asearch"`) | `websearch_interception/handler.py:246-384` |
| **litellm**(agentic loop B) | **真实执行**(调模型 N+1 次) | ✅ | ✅ **伪流式**(`stream=True`→内部转 False→跑完→重建 SSE) | 独立行(搜索)+ 每轮模型各一行 | `handler.py:1297-1351`、`llm_http_handler.py:5582-5619` |
| **litellm**(协议派生) | 派生 `web_search_options` | ❌(靠上游) | 透传 | 并入同行(`search_context_cost_per_query`) | `tool_call_cost_tracking.py:531-566` |
| **sub2api** | **真实执行(短路 A)** | ✅ | ✅ **手写 5 事件 SSE** | **不计费**(`Usage: ClaudeUsage{}`) | `gateway_websearch_emulation.go:143-236` |
| **sub2api**(Responses 桥接) | 丢弃 | ❌ | — | — | `chatcompletions_responses_bridge.go:851-853` |
| **new-api** | 透传 + **按上游回报计费** | ❌(靠上游) | 透传 | **附加费并入同行**(`other.web_search_*`) | `service/text_quota.go:84-120`、`relay-claude.go:234` |
| **Higress `ai-search`** | **真实执行(prompt 注入 C)** | ✅ | ✅(SSE 注入 references) | `未验证`(搜索内容按普通 prompt token 计,搜索 $ 不追踪) | `plugins/wasm-go/extensions/ai-search/main.go` |
| **Higress `ai-agent`** | **真实执行(通用 ReAct loop)** | ✅ | ✅ | `未验证` | 同仓 `ai-agent`,`maxIterations` 默认 15 |
| **Portkey** | **真实执行(prompt 注入 C)** | ✅ | ✅ | `未验证`(仅 `execution_time`) | OSS `plugins/exa/online.ts` |
| **Vercel AI Gateway** | **真实执行(网关内 server tool)** | ✅ | 裸 HTTP server tool 是否支持 `未验证` | ✅ **明码标价** + 回传 `provider_metadata.gateway.gatewayToolCalls` | `vercel.com/docs/ai-gateway/.../web-search` |
| **Cloudflare AI Gateway** | **透传**(明确不做抽象) | ❌ | 标准代理 | 按上游费率走 Unified Billing,**不另收搜索费** | `developers.cloudflare.com/ai-gateway/usage/web-search/` |
| **Kong AI Gateway** | **不支持**(`tools` 透传) | ❌ | ✅ | 保留 analytics | `docs.konghq.com/hub/kong-inc/ai-proxy/` |
| **Apache APISIX** | **不支持**(`tools` 转换后透传) | ❌ | ✅ | `llm_tool_count` 等 | `apisix.apache.org/docs/apisix/plugins/ai-proxy/` |
| **Bifrost** | **真实执行(进程内 Go agent loop,通用 MCP 工具)**;但**无内建搜索后端**;且**唯一会把 `web_search` 跨 provider 归一化**的网关 | `mcp/agent.go`(`executeAgent`:`for depth < maxAgentDepth`);`bifrost.go:858-859`/`:960-961` 无条件进入;`schemas/responses.go:2987` `normalizeResponsesToolType()` | ❌ **流式明确不支持**(`bifrost.go:871`/`:973` 流式路径无 agent 分支,仅注入工具定义) | ✅ **跨轮累加** `MergeBifrostLLMUsage`;`ResponsesServerSideToolUsageDetails{WebSearchCalls}` |
| **OpenRouter** | **真实执行**(server tool / plugin) | ✅ | ✅(Responses:annotations 在 `response.completed` 一次性给) | 按引擎 per-request 计价 + `usage.server_tool_use.web_search_requests` | 见 §3 |
| **LangDB** | **真实执行**(内建 websearch MCP,网关自己托管并执行) | `"mcp_servers":[{"name":"websearch","type":"in-memory"}]`(裸 HTTP,无需 SDK) | `未验证` | 响应 `usage.cost`;搜索是否并入 `未验证` | `docs.langdb.ai/api-reference/create-chat-completion/`、`/getting-started/working-with-mcps/` |
| **Helicone** | **透传**(全样本里最纯粹的透传) | 官方:"**By default, we simply proxy your LLM requests directly to the provider**" | SSE 中继 | 计费是核心产品但**无搜索可计**("0% markup") | `docs.helicone.ai/references/availability`;`ai-gateway/src/` 模块树无 mcp/tools/agent |
| **aigw(现状)** | **丢弃 + warn** | ❌ | — | — | `adapter.rs:2446`、Stage 131 |

> **关键纠正(相对前篇)**:前篇 §1.3 判定「sub2api 只丢弃,不执行」。该结论对**Responses→Chat 桥接路径**成立,但 sub2api 在 **Anthropic `/v1/messages` 原生路径**上另有一套完整的 web search emulation 子系统(`gateway_websearch_emulation.go` + `internal/pkg/websearch/` 共 8 文件),含 Brave/Tavily 两 provider、Redis 配额加权负载均衡、SSE 构造、历史块剥离。**即「真实执行」阵营实为 litellm + sub2api + Higress + Portkey + Vercel + OpenRouter 六家,远不止 litellm 一家。**

### 1.2 三种「真实执行」形态对比(非两种)

> **本次调研的结构性发现**:除 litellm/sub2api 的两种形态外,Higress 与 Portkey 采用了**第三种更简单的形态——「先搜 + 改写 prompt + 单次模型调用」**,它对上游**零能力要求**(不需要 function calling、不需要服务端工具),是本环境最容易落地的一种。

| 维度 | (A) 短路 short-circuit | (B) agentic loop | **(C) prompt 注入 search-then-inject** |
|------|----------------------|------------------|----------------------------------|
| 触发条件 | tools **只有** web_search | 含 web_search(可混合) | 任意(Higress 按 `web_search_options` 触发) |
| **上游模型调用次数** | **0** | **N+1** | **1** |
| **上游能力要求** | 无 | **需 function calling** | **无** ✅ |
| 搜索时机 | 收到请求后 | 模型要求后 | **发给上游前** |
| 结果去向 | 直接合成回复 | 回灌为 tool_result | **改写进 prompt / system 消息** |
| 文本回复来自 | 网关拼接 | 模型生成 | **模型生成** ✅ |
| 回复质量 | 低(结果列表) | 高 | **高**(模型读了结果再答) |
| 实现复杂度 | 低 | 高 | **低** ✅ |
| 流式 | 手写固定事件 | 降级+重建 | **几乎无改动**(上游流式原样透传,仅需注入 references) |
| 客户端契约 | 需合成服务端工具块 | 需剥除内部工具 | **无需合成任何服务端工具块** ✅ |
| 实现者 | litellm、sub2api | litellm | **Higress `ai-search`、Portkey `exa/online`** |

**litellm 对 (A)/(B) 取舍的明确注释**(`handler.py:282-292`)——这是 aigw 选型时最该权衡的一段话:

```python
# Only short-circuit for providers whose Anthropic Messages agentic loop
# does not run web_search itself. Providers that have a
# BaseAnthropicMessagesConfig which handles web search natively (bedrock,
# vertex_ai, azure_ai, anthropic) already perform the search plus a
# follow-up LLM synthesis step; short-circuiting those would skip that
# synthesis and return raw search text — a regression for existing users.
```

即:**短路会跳过「模型综合」这一步,返回原始搜索文本——litellm 认为这对已有用户是倒退,故只对确实不支持的 provider 启用。** 而**形态 C 恰好绕开了这个取舍**:它既不需要上游支持服务端工具,又保留了模型综合。

**形态 C 的两个实现证据:**

**Higress `ai-search`**(`plugins/wasm-go/extensions/ai-search/main.go`,wasm-go 插件)——**改写最后一条 user 消息**:

```go
prompt := strings.Replace(config.promptTemplate, "{search_results}", searchResults, 1)
// → sjson.SetBytes(body, fmt.Sprintf("messages.%d.content", queryIndex), prompt)
// → proxywasm.ReplaceHttpRequestBody(modifiedBody)
```

- 搜索引擎:**google / bing / quark(阿里云 IQS)/ arxiv / elasticsearch**,硬编码端点如 `https://customsearch.googleapis.com/customsearch/v1`、`https://api.bing.microsoft.com/v7.0/search`、`https://export.arxiv.org/api/query`、`https://cloud-iqs.aliyuncs.com/search/genericSearch`(ACS3-HMAC-SHA256 签名);
- 配置:`searchFrom[]`(type/serviceName/apiKey/count/start/timeoutMillisecond)、`needReference`、`referenceFormat`、`referenceLocation`(head|tail)、`promptTemplate`(**必须含 `{search_results}` + `{question}`**,可选 `{cur_date}`)、`searchRewrite`;
- **触发方式与 litellm 的「派生」殊途同归**:`defaultEnable:false` 时**仅当请求体带 `web_search_options` 才启用**,且 `search_context_size` 的 low/medium/high → **1/3/5 条搜索 query**。即它把 OpenAI 的原生参数**当作自己的开关**来用;
- **多引擎并行 + 按 `result.Link` 去重**,总超时 = `max(timeoutMillisecond)` + 处理时间;
- `searchRewrite` 会**先调一次 LLM** 判断是否需要搜索并把 query 改写成关键词(多一跳 LLM);
- **流式支持**:`wrapper.ProcessStreamingResponseBody` + `UnifySSEChunk`,按 `\n\n` 切分把 references 注入 SSE;非流式则改 `choices.0.message.content`;
- 计费:`未验证` ——`ai-search` 自身不做计费;`ai-statistics` 只记上游 usage 的 input/output token,**故注入的搜索内容按普通 prompt token 计费,搜索 API 的 $ 成本不被追踪**。

**Portkey `exa/online`**(`plugins/exa/online.ts` + `manifest.json`,在 `plugins/index.ts` 注册为 `exa: { online: exaonline }`)——**注入 system 消息**:

- 网关自己请求 `const BASE_URL = 'https://api.exa.ai/search'`,再 `insertSearchResults`:追加到已有 system 消息,或 `unshift` 一条新 system 消息,内容包在 `<web_search_context>...</web_search_context>` 里;
- manifest:`"type":"transformer"`、`"supportedHooks":["beforeRequestHook"]`;参数 `prefix`/`suffix`/`numResults`/`includeDomains`/`excludeDomains`/`timeout`;
- **仅支持输入侧 guardrail**(官方原话):"it adds web search results to your request before it reaches the LLM, which is why only `before_request_hooks` are supported";
- 流式:官方称 `/chat/completions`、`/completions`、`/embeddings`、`/messages` 的 hook 结果均支持流式;
- Tavily 也在文档中提供但**不在 OSS `plugins/index.ts`** → 闭源/企业版;
- 计费:`未验证`(无独立搜索费用行,仅 `hook_results` 里记 `execution_time`)。

**另一个独立的 gateway 内 ReAct loop 实现**:Higress **`ai-agent`** 插件 = 网关内完整 ReAct 循环(`llm.maxIterations` **默认 15**,工具用 OpenAPI 描述,支持流式与非流式)——**通用工具执行,非 web-search 专用**,但证明「网关内跑 agentic loop」在 wasm 网关里也是可行的。

### 1.3 其他网关横向调研结果

| 网关 | 策略 | 证据 | 流式 | 计费 |
|------|------|------|------|------|
| **Higress `ai-search`** | **真实执行(形态 C)** | `plugins/wasm-go/extensions/ai-search/`(main.go + engine/{google,bing,quark,arxiv,elasticsearch}) | ✅ `ProcessStreamingResponseBody` 注入 references | `未验证`——搜索内容按普通 prompt token 计,搜索 API $ 成本**不追踪** |
| **Higress `ai-agent`** | **真实执行(通用 ReAct loop)** | 同仓 `ai-agent` 插件,`llm.maxIterations` 默认 15 | ✅ | `未验证` |
| **Portkey** | **真实执行(形态 C)** | OSS `plugins/exa/online.ts`、`plugins/exa/manifest.json`、`plugins/index.ts` | ✅(官方声明 hook 结果支持流式) | `未验证`(仅 `execution_time`) |
| **Vercel AI Gateway** | **真实执行(网关内执行 server tool)** | `vercel.com/docs/ai-gateway/models-and-providers/web-search`;`tools:[{"type":"vercel:exa_search",...}]` 走裸 HTTP,无需 SDK | SDK `streamText` 可用;**裸 HTTP server tool 是否支持 `stream:true` `未验证`** | ✅ **明码标价**:Perplexity $5/1k、Exa $7/1k(含 10 条,超出 +$1/1k)、Parallel $5/1k、Browserbase Fetch $1/1k(+代理/抽取各 $3/1k);次数与聚合成本在 `choices[0].message.provider_metadata.gateway.gatewayToolCalls` |
| **Cloudflare AI Gateway** | **透传**(明确拒绝做抽象) | `developers.cloudflare.com/ai-gateway/usage/web-search/` | 标准代理 | ✅ 按上游费率走 Unified Billing,**网关不另收搜索费** |
| **Kong AI Gateway** | **不支持 web search**;`tools` 透传 | `docs.konghq.com/hub/kong-inc/ai-proxy/`(web_search 零命中);`ai-rag-injector` 仅向量库(Redis Vector / pgvector),**非 web** | ✅ | 保留 analytics + 成本计算 |
| **Apache APISIX** | **不支持 web search**;`tools` 格式转换后透传 | `apisix.apache.org/docs/apisix/plugins/ai-proxy/`(web_search 零命中);`ai-rag` 仅 Azure OpenAI embeddings + Azure AI Search,**非 web** | ✅(SSE) | `llm_tool_count`/`llm_has_tool_calls`/`llm_stream` 等字段 |
| **Bifrost** | **真实执行(进程内 Go agent loop,通用 MCP 工具)**;**无内建搜索后端**;**选择性**执行(白名单外的工具原样回客户端) | `mcp/agent.go` `executeAgent`:`for depth < maxAgentDepth` → `extractToolCalls` → `executeToolFunc` → `makeLLMCall`;并行 goroutine 扇出(`agent.go:292`);白名单外 `agent.go:366` "returning them immediately without continuing the loop" | ❌ **流式不支持**(流式路径仅注入工具定义,不执行) | ✅ 跨轮累加 `MergeBifrostLLMUsage` |
| **LangDB** | **真实执行(内建 websearch MCP,形态近 B)** | 裸 HTTP `"mcp_servers":[{"name":"websearch","type":"in-memory"}]`;官方 "MCP tools are treated just like normal function calls **inside LangDB**" | `未验证` | `usage.cost`,搜索是否并入 `未验证` |
| **Helicone** | **透传** | 官方 "By default, we simply proxy your LLM requests directly to the provider";`ai-gateway/src/` 无 mcp/tools/agent 模块 | SSE 中继 | 无搜索可计 |
| **open-webui**(客户端对照) | 应用侧执行 + RAG 注入 | `backend/open_webui/retrieval/web/`(searxng.py、tavily.py … + `main.py get_filtered_results`) | — | — |

**⚠️ 一类必须区分清楚的「假阳性」——RAG ≠ web search:**

| 网关 | 功能 | 实质 |
|------|------|------|
| Kong `ai-rag-injector` | 向量检索注入 | **仅 Redis Vector Search / pgvector**,非 web |
| APISIX `ai-rag` | 向量检索注入 | **仅 Azure OpenAI embeddings + Azure AI Search**,非 web |
| Cloudflare **AI Search** | 独立产品 | **与 AI Gateway 是两个产品**,不可混记 |

**→ 这三个在形态上与 Higress/Portkey 的「注入 prompt」一致,但数据源是自有向量库而非公网搜索。调研时极易误记为「网关支持 web search」。**

**三条可引用的官方表态:**

**Cloudflare —— 明确不做抽象**(`developers.cloudflare.com/ai-gateway/usage/web-search/`):

> AI Gateway proxies native web search tools from supported providers... **Search runs on the upstream provider**; AI Gateway applies its standard features — logging, caching, rate limiting, and guardrails — to the request.
> AI Gateway does **not** provide a provider-agnostic web search abstraction.
> Web search requests are billed at the upstream provider's web-search rates and flow through Unified Billing... **AI Gateway does not charge a separate web-search fee.**

**Vercel —— 明确自己执行且隐藏中间态**(`vercel.com/docs/ai-gateway/models-and-providers/web-search`):

> AI Gateway executes the search, adds the results to the model context, and returns the final answer in the same response.
> AI Gateway executes server tools internally. The final Chat Completions response has `finish_reason: "stop"` and **does not include client-facing `tool_calls` or raw search results**.

**Bifrost —— 网关内 loop 与流式互斥**(`docs.getbifrost.ai/mcp/agent-mode`,并经源码 `bifrost.go:871`/`:973` 证实流式路径无 agent 分支):

> Agent Mode is not compatible with streaming operations (`chat_stream` and `responses_stream`)... requires complete responses before proceeding to the next iteration.

**→ 这与 litellm「把流式降级成非流式再重建」是同一个物理约束的两种应对:Bifrost 选择直接不支持,litellm 选择降级+重建。aigw 应选 litellm 路线(对客户端无感),或干脆走形态 C(无此约束)。**

**两处对 aigw 尤其有价值的设计点:**
1. **Vercel 的 `config` 覆盖语义**——server tool 的 `config` 字段是 **developer 默认值,覆盖模型生成的值**,官方说明这是**防 prompt 注入**的措施。即「域名白名单/结果条数」这类安全参数不能让模型自己决定。
2. **Vercel 把搜索次数与聚合成本放进响应体 `provider_metadata.gateway.gatewayToolCalls`**——这是 §5.6 之外的**第四种计费呈现方式**(回传给客户端而非只落库),可与方案 1/3 叠加。

### 1.4 全样本最终归类(13 个网关)

| 类别 | 成员 | 数量 |
|------|------|------|
| **真实执行 — web search 专用特性** | litellm(A+B)、sub2api(A)、**Higress `ai-search`(C)**、**Portkey `exa/online`(C)**、**Vercel AI Gateway**、**LangDB**、OpenRouter | **7** |
| **真实执行 — 通用工具 loop,非 web search 专用特性** | Higress `ai-agent`(ReAct,maxIterations 15)、**Bifrost**(进程内 Go loop,**无内建搜索后端**,不支持流式) | 2 |
| **透传** | Cloudflare AI Gateway(**明确拒绝做抽象**)、Kong `ai-proxy`、APISIX `ai-proxy`、Helicone、new-api | 5 |
| **丢弃** | sub2api(Responses 桥接)、**aigw(现状)** | 2 |
| RAG 但非 web(**勿误记**) | Kong `ai-rag-injector`、APISIX `ai-rag`、Cloudflare AI Search | 3 |
| 客户端侧对照 | open-webui(+ Cherry Studio / LobeChat / NextChat,`未验证`) | — |

**结论性判断:「网关自己执行搜索」已是主流而非个例(7 家有专用特性),且其中最简单的形态 C 有 2 家独立实现。** aigw 当前的「丢弃」在横向对比中属于最保守的一档(仅与 sub2api 的桥接路径同类)。

**两个值得单独记住的极端立场**(aigw 定位时的两端参照):

| 立场 | 代表 | 原话/证据 |
|------|------|----------|
| **拒绝抽象** | Cloudflare AI Gateway | "AI Gateway does **not** provide a provider-agnostic web search abstraction" —— 只做可观测/缓存/限流,搜索归上游 |
| **主动归一化** | **Bifrost** | `schemas/responses.go:2987` `normalizeResponsesToolType()`:`"web_search_20250305"` → `"web_search"`,并在 `providers/gemini/responses.go:4192` 把 canonical `web_search` 映到 Gemini 的 `googleSearch`——**全样本中唯一跨 provider 归一化 `web_search` 的网关** |

**→ Bifrost 的归一化思路对 aigw 直接可用**:即使阶段 1 不执行搜索,也可以先把 `web_search` / `web_search_preview` / `web_search_20250305` 等变体**归一成一个内部 canonical 类型**,再决定执行或丢弃——这让后续接入形态 C 时入口层无需再改。

---

## 2. litellm 拦截子系统深潜

**包结构**(`litellm/integrations/websearch_interception/`,共 2720 行):

| 文件 | 行数 | 职责 |
|------|------|------|
| `ARCHITECTURE.md` | — | **官方架构说明(含 mermaid 时序图)**,本节主要依据 |
| `handler.py` | 1879 | `WebSearchInterceptionLogger`:短路 + 三个 API surface 的 loop |
| `transformation.py` | 479 | tool_use 检测、tool_result 构造、native block 构造 |
| `tools.py` | 293 | 标准工具定义 + 四种格式检测 |
| `types/integrations/websearch_interception.py` | 49 | 配置 TypedDict + `AnthropicServerToolUseBlock` |

### 2.1 Hook 点在请求生命周期的确切位置

**两个 hook,一前一后:**

| Hook | 时机 | 作用 | 位置 |
|------|------|------|------|
| `async_pre_call_deployment_hook` | **上游请求发出前** | 把 native `web_search_20250305` 换成普通 function 工具 | `handler.py:386-420` |
| `async_should_run_agentic_loop` + `async_run_agentic_loop` | **上游响应回来后** | 检测 tool_call → 执行搜索 → 回灌 → 再请求 | `handler.py`,由 `llm_http_handler.py:5621` 编排 |

**前置转换的必要性**(`ARCHITECTURE.md:65-67`):

> **Without Interception**: Bedrock would receive native tool → try to execute natively → return `web_search_tool_result_error` with `invalid_tool_input`
> **With Interception**: LiteLLM converts → Bedrock returns tool_use → LiteLLM executes search → Returns final answer ✅

即**必须在发出前把服务端工具降级成普通 function 工具**,否则上游会尝试原生执行并报错。这正是 aigw 当前「丢弃」路线要改成「替换」的那一步。

**注入的 function 工具**(`tools.py:15-34`,常量 `LITELLM_WEB_SEARCH_TOOL_NAME = "litellm_web_search"`,`constants.py:590`):

```python
{
    "name": "litellm_web_search",
    "description": "Search the web for information...",
    "input_schema": {
        "type": "object",
        "properties": {"query": {"type": "string", "description": "The search query"}},
        "required": ["query"],
    },
}
```

**四种入参格式的检测**(`tools.py:118-293`,`ARCHITECTURE.md:35-42`):

| 格式 | 示例 | 检测方式 | 前向兼容 |
|------|------|---------|---------|
| LiteLLM 标准 | `name="litellm_web_search"` | 名称精确匹配 | — |
| Anthropic 原生 | `type="web_search_20250305"` | **`type.startswith("web_search_")`** | ✅ 自动支持 `web_search_2026` 等未来版本 |
| Claude Code CLI | `name="web_search"` + `type="web_search_20250305"` | 名称 + 类型 | ✅ |
| 遗留 | `name="WebSearch"` | 名称匹配 | — |

`tools.py:152`:`return tool_type == "web_search" or tool_type.startswith("web_search_")`

### 2.2 agentic loop 的驱动与安全栏

**编排在 `llm_http_handler.py:5621-5740`**(`_call_agentic_completion_hooks`),流程:

```
上游响应 → 遍历 litellm.callbacks
         → callback.async_should_run_agentic_loop(response, tools, ...)   # 门禁,返回 (bool, tool_calls)
         → [安全栏 _check_agentic_loop_safety]                             # 在 try/except 之外
         → callback.async_run_agentic_loop(...) / async_build_agentic_loop_plan(...)
         → _maybe_wrap_in_fake_stream(result)                              # 流式重建
```

**门禁与安全栏分离的设计约束**(`:5685-5686`,原注释):

```python
# Safety guards must run OUTSIDE the callback try/except — they are
# bounded-loop / cycle-break rails, not callback bugs.
```

即:**callback 自身的异常被 `except Exception` 吞掉(坏回调不该让整个请求失败),但循环上限/成环检测必须能中断派发。** 这是 aigw 实现时容易写错的地方。

**两条安全栏**(`:5201-5223`):

```python
fingerprint: Final = BaseLLMHTTPHandler._fingerprint_agentic_tools(tool_calls)
if fingerprint in fingerprints:
    raise AgenticLoopSafetyError("Agentic loop detected repeated tool-call fingerprint; aborting rerun")
if depth >= max_loops:
    raise AgenticLoopSafetyError(f"Exceeded max_agentic_loops={max_loops} for model={model}")
```

1. **指纹成环检测**:`json.dumps(tool_calls, sort_keys=True)`(`:5226-5230`)——**模型连续两轮请求完全相同的搜索就断环**。
2. **深度上限**:`DEFAULT_MAX_AGENTIC_LOOPS = 3`(`litellm_core_utils/agentic_loop_settings.py:24`)。

**上限可配置的两个层级 + 一条安全约束**(`ARCHITECTURE.md:216-241`):

```yaml
litellm_settings:
  websearch_interception_params:
    enabled_providers: ["bedrock"]
    max_agentic_loops: 5          # 特性级
model_list:
  - model_name: claude-sonnet-4-5
    litellm_params:
      max_agentic_loops: 5         # 部署级,优先于特性级
```

> Clients cannot set it. `max_agentic_loops` is on the proxy's untrusted-field list, so a request body that carries it is ignored and one request can never drive an unbounded number of upstream model calls.

**客户端不能设上限**——这是必须照抄的安全设计(否则一个请求能驱动无限次上游调用)。且**配置在加载期校验**,非法值直接拒绝启动而不是运行时才炸。

**撞上限时的收尾**(`ARCHITECTURE.md:243-247`,行为被 `test_websearch_agentic_loop_cap.py` 固化):

> the turn ends there and the client gets the last response back with the internal `litellm_web_search` tool call removed and `stop_reason: end_turn`. The client never declared that tool, so leaving the block in would hand it a tool call it has no way to answer.

测试断言(`:188-250`):`INTERNAL_TOOL_NAME not in _tool_use_names(result)`、`result["stop_reason"] == "end_turn"`、`_block_types(result) == ["server_tool_use", "web_search_tool_result", "text"]`,且「只剩被拒调用」时 `result["content"] == []`。

**→ 对 aigw 的直接含义:注入的内部工具名绝不能泄漏给客户端。** 客户端没声明过它,返回一个它无法应答的 tool_call 会让会话卡死。

### 2.3 三个 API surface 的差异(重要:不对等)

| surface | loop 实现 | 撞上限行为 | 流式 | native block 回灌 |
|---------|----------|-----------|------|------------------|
| **Anthropic `/v1/messages`** | `_execute_agentic_loop` / plan | **优雅收尾**(改写成 `end_turn`) | ✅ 伪流式重建 | ✅ `server_tool_use` + `web_search_tool_result` |
| **`/v1/chat/completions`** | 独立一份 `chat_completion_agentic_loop.py`(338 行) | **仍然 raise** | `未验证` | ❌ 无等价块 |
| **`/v1/responses`** | `async_build_responses_agentic_loop_plan` | **仍然 raise** + 内部调用泄漏 | 未包装 | ❌ **不合成 `web_search_call`** |

**官方自陈的缺口**(`ARCHITECTURE.md:256-259`):

> Two other surfaces do not get that treatment yet. `/v1/responses` returns its own shape that the finalizer does not rewrite, so it still hands back the internal call. And `/v1/chat/completions` runs its own copy of these rails in `litellm_core_utils/chat_completion_agentic_loop.py`, which still raises rather than ending the turn.

**Responses 路径的回灌形态**(`handler.py:1168-1185`)——注意它用的是**普通 function_call 对**,不是 `web_search_call`:

```python
followup_items: Final = [
    item
    for tool_call, search_text in zip(tool_calls, search_texts)
    for item in (
        {"type": "function_call", "call_id": tool_call.get("call_id"),
         "name": LITELLM_WEB_SEARCH_TOOL_NAME, "arguments": tool_call.get("arguments", "")},
        {"type": "function_call_output", "call_id": tool_call.get("call_id"), "output": search_text},
    )
]
```

**→ 即 litellm 在 Responses 侧并未给出 OpenAI 原生 `web_search_call` 的保真回放。** aigw 若要支持 Codex(走 Responses),这块没有现成参照,需自行设计(见 §5.4)。

### 2.4 Chat Completions 路径的结果回灌

`handler.py:1745-1746` 的注释点明了协议不对等:

```python
# Chat-completion path only needs text — OpenAI tool_result format
# has no equivalent of Anthropic's web_search_tool_result block.
```

回灌为标准三段式(`transformation.py:380-396`):`messages + [assistant(tool_calls)] + [{"role":"tool","tool_call_id":...,"content": 搜索文本}]`。

搜索并行执行(`handler.py:1741-1743`):`await asyncio.gather(*search_tasks, return_exceptions=True)`——**一轮里多个 tool_call 并发搜索**,单个失败降级为 `f"Search failed: {result}"` 文本而非中断整请求。

### 2.5 流式策略:降级 + 重建(而非真流式)

`ARCHITECTURE.md:265-277` + `llm_http_handler.py:5582-5619`:

1. 请求带 `stream=True` + web_search;
2. 发请求前 `anthropic_messages()` 检测到拦截启用 → **`stream=True` 改成 `stream=False`**,并在 `logging_obj.model_call_details` 打标 `websearch_interception_converted_stream=True`;
3. 非流式跑完整个 loop;
4. 结束后 `_maybe_wrap_in_fake_stream` 读该标记,把最终 dict 包进 `FakeAnthropicMessagesStreamIterator` 重建 Anthropic SSE。

**理由(官方)**:

> Server-side agentic loops require consuming full responses to detect tool_use

**门禁在 `api_surface == "anthropic_messages"`**(`:5634-5636`)——只有 Anthropic surface 会被包装,Responses 原样返回。

另有一个性能优化值得照抄(`:5156-5184`,`_has_agentic_completion_hook`):**没有任何 callback 覆写 `async_should_run_agentic_loop` 时,跳过「为了调 hook 而缓冲+重建整个 SSE 响应」的包装器。** 即未启用搜索时流式零额外开销。

### 2.6 推理模型的交互坑:max_tokens vs thinking.budget_tokens

`handler.py:1255-1279`(`_resolve_max_tokens`)+ `test_websearch_thinking_constraint.py`:

```python
if max_tokens <= budget_tokens:
    adjusted: Final = math.ceil(budget_tokens) + 1024
```

**后续轮次的 `max_tokens` 必须严格大于 `thinking.budget_tokens`,否则 Anthropic API 直接拒绝。** loop 里每多一轮都要重新满足这个约束。aigw 若接推理模型(deepseek-r1 / glm thinking),同类约束需验证。

### 2.7 `/search` 端点 + search_tool_registry

| 组件 | 位置 | 说明 |
|------|------|------|
| 搜索端点 | `proxy/search_endpoints/endpoints.py:17-40` | `POST /v1/search`、`/search`、`/v1/search/{search_tool_name}`、`/search/{tool}` 四个路由 |
| 工具列表 | 同上 `:241-253` | `GET /v1/search/tools`、`/search/tools` |
| 注册表 | `search_tool_registry.py`(10470 B) | 管理已配置搜索工具 |
| 管理 API | `search_tool_management.py`(23621 B) | CRUD |

**端点对齐 Perplexity Search API 规范**(`endpoints.py:47-49`):

```
Follows the Perplexity Search API spec:
https://docs.perplexity.ai/api-reference/search-post
```

`search_tool_name` 可走 URL path 或 body,**推荐走 path 以保持 body 与 Perplexity 兼容**。

**21 个搜索后端**(`litellm/llms/*/search`):`apiserpent`、`azure`、`bedrock`、`brave`、`dataforseo`、`duckduckgo`、`exa_ai`、`fastcrw`、`firecrawl`、`google_pse`、`linkup`、`nimble`、`parallel_ai`、`perplexity`、`searchapi`、`searxng`、`serper`、`tavily`、`tinyfish`、`you_com`(+ `base_llm/search` 抽象)。

**provider 选择三级兜底**(`handler.py:1638-1672`,`ARCHITECTURE.md:287-290`):指定名匹配 → 取 router 第一个 → 兜底 `perplexity`。

**鉴权**:搜索工具有独立的 key / team 级授权(`handler.py:1557-1580`):`can_key_call_search_tool` + `can_team_call_search_tool`。

### 2.8 ⭐ 计费:搜索费如何进 SpendLog(本节为 aigw 最关键输入)

litellm 对「搜索费」有**两条完全独立的路径**,对应「谁执行了搜索」:

#### 路径 1:网关自己执行 → **独立 SpendLog 行**

`_execute_search` 调用的是**顶层 API** `litellm.asearch()`(`handler.py:1513-1524`):

```python
result: Final = (
    await litellm.asearch(query=query, search_provider=search_provider, **_NO_ASEARCH_NAMED, **search_kwargs)
    if search_metadata is None
    else await litellm.asearch(
        query=query, search_provider=search_provider,
        litellm_metadata=search_metadata,       # ← 关键:带上归属元数据
        **_NO_ASEARCH_NAMED, **search_kwargs,
    )
)
```

因为是顶层调用,它**走完整的 logging 管线 → 产生自己的 SpendLog 行**,`call_type` 为 `CallTypes.asearch = "asearch"`(`types/utils.py:396-397`)。

**归属元数据的构造**(`handler.py:1582-1602`),注释直接说明了「为什么必须带」:

```python
@staticmethod
def _build_search_request_metadata(user_api_key_auth, search_tool_name) -> Mapping[str, object]:
    """
    Spend-tracking metadata for the intercepted search, so its provider cost is logged
    and billed against the key/user/team that made the originating LLM request instead
    of being dropped by the proxy's spend hook for lack of an owner.
    """
    user_api_key_metadata = LiteLLMProxyRequestSetup.get_sanitized_user_information_from_key(...)
    return {
        **user_api_key_metadata,
        "model_group": search_tool_name,     # ← 搜索工具名占用 model_group 字段
        "user_api_key": user_api_key_auth.api_key,
        "user_api_key_auth": user_api_key_auth,
    }
```

**→ 两个可直接照搬的设计点:**
1. **不带归属元数据,这条搜索花费会被 spend hook 以「无主」为由丢弃**;
2. **`model_group` 字段复用为搜索工具名**——即在 SpendLog 里搜索行长得像「model_group=my-tavily-tool、call_type=asearch」。

**计价函数**(`litellm/search/cost_calculator.py:31-85`),按查询数计价 + 支持阶梯:

```python
def search_provider_cost_per_query(model, custom_llm_provider=None, number_of_queries=1, optional_params=None):
    """Returns (input_cost, output_cost) where input_cost = queries * cost_per_query"""
    model_info = get_model_info(model=model, custom_llm_provider=custom_llm_provider)
    tiered_pricing = model_info.get("tiered_pricing")
    if tiered_pricing and isinstance(tiered_pricing, list):
        max_results = (optional_params or {}).get("max_results", 10)
        for tier in tiered_pricing:
            range_min, range_max = tier["max_results_range"]
            if range_min <= max_results <= range_max:
                cost_per_query = tier["input_cost_per_query"]
                break
        else:
            cost_per_query = tiered_pricing[-1]["input_cost_per_query"]
    else:
        cost_per_query = float(model_info.get("input_cost_per_query") or 0.0)
    total_cost = number_of_queries * cost_per_query
    return (total_cost, 0.0)                      # output_cost 恒为 0
```

接线点 `cost_calculator.py:585-593`(`call_type in ("search","asearch")`)与 `:1512-1540`。**`number_of_queries` 从 `query` 是否为 list 推导**(`:1516-1522`)。

**真实价格表**(`model_prices_and_context_window.json`,`mode: "search"`,单位 USD/query):

| provider | `input_cost_per_query` | 备注 |
|----------|----------------------|------|
| `searxng/search`、`you_com/search`、`duckduckgo/search`、`tinyfish/search`、`agentcore/search` | **0.0** | 自建/免费 |
| `apiserpent/search` | 0.0006 | 最低付费 |
| `serper/search` | 0.001 | |
| `parallel_ai/search-fast`、`search-turbo` | 0.001 | |
| `dataforseo/search` | 0.003 | |
| `perplexity/search`、`nimble/search`、`google_pse/search`、`parallel_ai/search(-pro)` | 0.005 | |
| `linkup/search` | 0.00587 | |
| `tavily/search` | **0.008** | `search-advanced` 0.016 |
| `bing_grounding/search` | 0.035 | 最贵 |
| `linkup/search-deep` | 0.05867 | |
| `exa_ai/search` | **阶梯**:0.005(≤25 结果)/ 0.025(26-100) | `tiered_pricing` |
| `firecrawl/search` | **阶梯**:0.00166(1-10)/ 0.00332(11-20)/ … | |

#### 路径 2:上游模型原生执行 → **并入同一条行**

`litellm_core_utils/llm_cost_calc/tool_call_cost_tracking.py:531-566`:

```python
@staticmethod
def get_cost_for_web_search(web_search_options=None, model_info=None) -> float:
    search_context_raw = model_info.get("search_context_cost_per_query")
    search_context_pricing = search_context_raw or SearchContextCostPerQuery()
    if web_search_options.get("search_context_size", None) == "low":
        return search_context_pricing.get("search_context_size_low", 0.0)
    elif ... == "medium": ...
    elif ... == "high": ...
    return StandardBuiltInToolCostTracking.get_default_cost_for_web_search(model_info)   # 默认取 medium
```

按 `search_context_size` 三档计价,**无该参数时默认按 medium**(`:552-566`,官方注释引 `https://platform.openai.com/docs/pricing#web-search`)。

检测是否真的搜了:Chat 侧看 `annotations[].type == "url_citation"`,Responses 侧看 `response_includes_output_type(response, "web_search_call")`(`:505-522`)。

价格表里 **307 个模型**带 `search_context_cost_per_query`,例如全部 Bedrock Anthropic 条目:

```json
"anthropic.claude-opus-4-5-20251101-v1:0": {
  "search_context_cost_per_query": {
    "search_context_size_low": 0.01, "search_context_size_medium": 0.01, "search_context_size_high": 0.01
  }
}
```

**即 Anthropic 系三档同价 $0.01/query = $10/1k searches**,与 Anthropic 官方公开价一致(§4 交叉验证)。

#### 一条踩过的坑:follow-up 复用 logging 对象会吞掉首轮花费

`handler.py:1281-1295`:

```python
"""Build kwargs for the follow-up call, excluding internal keys.

``litellm_logging_obj`` MUST be excluded so the follow-up call creates
its own ``Logging`` instance via ``function_setup``.  Reusing the
initial call's logging object triggers the dedup flag
(``has_logged_async_success``) which silently prevents the initial
call's spend from being recorded — the root cause of the
SpendLog / AWS billing mismatch.
"""
```

**→ aigw 实现 loop 时必须让每轮上游调用各自独立计费,不可共用一个 logging/spend 上下文。** 这是 litellm 实际修过的 bug(SpendLog 与真实账单不符),直接继承该教训。

### 2.9 litellm 计费模型小结

| 场景 | SpendLog 行数 | call_type | 计价依据 |
|------|-------------|-----------|---------|
| 网关执行 1 次搜索 + 2 轮模型 | **3 行**(1 搜索 + 2 模型) | `asearch` / `acompletion` ×2 | `input_cost_per_query` / token |
| 上游原生搜索 | **1 行** | `acompletion` | token + `search_context_cost_per_query` |

---

## 3. OpenRouter 的 plugin / server-tool 模型

> 数据来源:`https://openrouter.ai/docs/llms-full.txt`(官方机器可读文档全量 dump,HTTP 200,4.2 MB,每段带 `Source:` 原始 URL)+ 渲染页交叉验证。注:该站对 `WebFetch` 返回 SSL 错误,需 `curl -X GET`。

### 3.1 ⚠️ plugin 与 `:online` 已被标记弃用

本调研发现一处**与任务前提不同的事实**:`plugins: [{id:"web"}]` 与 `:online` 后缀**均已正式弃用**,取代者是 **server tool `openrouter:web_search`**。

> **Deprecated** — The `:online` variant and the web search plugin are deprecated. Use the [`openrouter:web_search` server tool] instead.
> — `https://openrouter.ai/docs/guides/features/plugins`

两种形态目前都仍可用。

### 3.2 请求形态

**旧 plugin 形态**(`https://openrouter.ai/docs/guides/features/plugins/web-search`,逐字):

```json
{
  "model": "openai/gpt-5.2:online",
  "plugins": [
    {
      "id": "web",
      "engine": "parallel",
      "mode": "turbo",
      "max_results": 1,
      "search_prompt": "Some relevant web results:",
      "include_domains": ["example.com", "*.substack.com"],
      "exclude_domains": ["reddit.com"]
    }
  ]
}
```

参数表(`https://openrouter.ai/docs/api_reference/responses/web-search`):`id`(必填 `"web"`)、`engine`(`native`/`exa`/`firecrawl`/`parallel`,省略=auto)、`max_results`(**1-25,默认 5**;Perplexity 1-20)、`include_domains`/`exclude_domains`(支持 `*.substack.com` 通配)。plugin 页另有 `mode`、`search_prompt`、`x_search`、`enabled`。

**默认 `search_prompt`(逐字)**——值得注意它**显式要求模型用 markdown 链接引用,并以域名命名**:

```
A web search was conducted on `date`. Incorporate the following web search results into your response.

IMPORTANT: Cite them using markdown links named using the domain of the source.
Example: [nytimes.com](https://nytimes.com/some-page).
```

**新 server tool 形态**(`https://openrouter.ai/docs/guides/features/server-tools/web-search`):

```json
{
  "type": "openrouter:web_search",
  "parameters": {
    "engine": "exa",
    "max_results": 5,
    "max_total_results": 20,
    "search_context_size": "medium",
    "allowed_domains": ["example.com"],
    "excluded_domains": ["reddit.com"]
  }
}
```

字段改名:`include_domains`/`exclude_domains` → `allowed_domains`/`excluded_domains`。新增 `max_uses`、`max_characters`(1-100000)、`user_location`。另有顶层 `max_tool_calls` 限制 server-tool 预算:**省略时默认 30 步,也是上限**。

**`:online` 语义存在文档自相矛盾**:plugin/model-variants 两页说它等价于 `{"model":"openrouter/auto","plugins":[{"id":"web"}]}`(**会换掉模型**),而 Plugins 总览页说它等价于保留原模型 + web plugin。`未验证` 哪个权威(未做实活调用),**实际语义大概率是后者**。

一个对网关有用的行为(`.../model-variants/online`):

> If your application already provides the `web_search` tool (e.g. OpenAI's built-in web search tool type), OpenRouter automatically recognizes it and hoists it to the `openrouter:web_search` server tool.

**即 OpenRouter 会把客户端发来的 OpenAI 原生 `web_search` 工具自动「提升」为自己的 server tool** —— 这正是 aigw 可以采取的入口兼容策略。

### 3.3 响应 annotations:两个 API 形状不同(踩坑点)

**Chat Completions —— 嵌套在 `url_citation` 子对象下,位于 `choices[0].message.annotations`**(逐字):

```json
{
  "message": {
    "role": "assistant",
    "content": "Here's the latest news I found: ...",
    "annotations": [
      {
        "type": "url_citation",
        "url_citation": {
          "url": "https://www.example.com/web-search-result",
          "title": "Title of the web search result",
          "content": "Content of the web search result",
          "start_index": 100,
          "end_index": 200
        }
      }
    ]
  }
}
```

OpenRouter 明确声明它把**所有模型(含 Perplexity / OpenAI Online 等原生搜索模型)的结果统一归一化成 OpenAI 的这一个 annotation schema**。注:`content` 字段是 **OpenRouter 的扩展**(装 Exa highlights),OpenAI 原生 spec 无此字段(见 §4.1 交叉验证)。多段摘录以 `[...]` 分隔。

**Responses API —— 扁平,且没有 `title`**,位于 `output[].content[].annotations`:

```json
{ "type": "url_citation", "url": "https://example.com/article", "start_index": 0, "end_index": 50, "content": "Excerpt..." }
```

**→ 两个 API 的 annotation 形状不一致(嵌套+有 title vs 扁平+无 title),适配器必须分开处理。**

### 3.4 计价:per-request,不是「$4/1000 结果」

**任务描述里的「$4 per 1000 results」在现行文档中不存在**(`grep` `$4 per`/`0.004`/`per 1000 results` 均无命中)——标记 `未验证 / 疑为过期`。现行为**按引擎 per-request**(`.../server-tools/web-search` 逐字表):

| 引擎 | 计价 |
|------|------|
| **Exa** | Instant/Fast/Auto **$0.007/request**;Deep Lite/Deep $0.012;Deep Reasoning $0.015。**含前 10 条结果,超出每条 +$0.001** |
| **Parallel** | Turbo/Fast $0.001;Basic/Advanced $0.005。含前 10 条,超出每条 +$0.001 |
| **Perplexity** | $0.005/request |
| **Firecrawl** | 直接消耗你的 Firecrawl 额度,OpenRouter 不收费(搜索 2 credits/10 结果 + 每结果 5 credits) |
| **Native** | 原样透传上游 provider 价格 |

> All pricing is in addition to standard LLM token costs for processing the search result content.

**计价不对称值得注意**:计价「含前 10 条」,但默认 `max_results=5`,**故默认配置永远吃不到超额费**。

`web_search_options` 无独立费率,是原生搜索的 context-size 控制,计费按 provider 透传。

### 3.5 搜索后端:Exa 被官方点名

> For other models, the web search plugin is powered by [Exa](https://exa.ai). It uses their "auto" method (a combination of keyword search and embeddings-based web search) ... For each result, OpenRouter requests Exa highlights — extractive excerpts drawn from the page that Exa selects as most relevant to the search query, **sized adaptively (typically ~2,000–4,000 characters per result)**.

另点名 Firecrawl / Parallel / Perplexity。**Brave、Serper 全文无提及。**

`search_context_size` → Exa `contents.highlights.maxCharacters`:`low`=5,000 / `medium`=15,000 / `high`=30,000 字符每结果。**这是一份现成的「片段截断预算」参考值**(见 §5.5)。

引擎解析:`auto`(默认)→ 上游支持原生则用原生,否则 Exa。

### 3.6 流式与计费回报

**Responses API 流式有官方示例**(`.../api_reference/responses/web-search`):annotations **不增量下发**,从终态 `response.completed` 事件里读:

```javascript
if (parsed.type === 'response.completed') {
  const annotations = parsed.response?.output
    ?.find(o => o.type === 'message')
    ?.content?.find(c => c.type === 'output_text')
    ?.annotations || [];
}
```

**Chat Completions SSE 的 annotation 下发方式:`未验证`** —— 全量文档无 `delta.annotations` 示例,`StreamingChoice` 类型也只声明 `{content, role?, tool_calls?}`。

**usage 里回报搜索次数**(`.../server-tools/web-search`):

```json
{ "usage": { "input_tokens": 105, "output_tokens": 250, "server_tool_use": { "web_search_requests": 2 } } }
```

成本字段(`.../api_reference/overview`)有 `cost`、`cost_details.server_tool_cost`(文档示例是 shell sandbox 计时,**web search 是否落这里 `未验证`**)。确认会把搜索费单列的两处:Logs UI 明细("a cost breakdown including ... **web search**, web fetch, and file processing charges")与 Broadcast 导出(`web_search_engine`、`num_search_results` 字段,理由是"they select or multiply a rate")。`GET /api/v1/generation` 的确切字段表 `未验证`(页面超时,无公开 OpenAPI)。

---

## 4. 线格式附录(aigw 必须对客户端还原的形状)

> 本节由并发子代理读官方文档取得,并与 litellm 本地类型定义 / new-api 价格表交叉验证。
> **方法说明**:`platform.openai.com` / `developers.openai.com` 对 WebFetch 返回 403/500,OpenAI 部分经真实浏览器读取官方文档页(每段均确认 `location.href` 为官方 URL);Anthropic 部分经 WebFetch 成功取得。

### 4.0 ⚠️ 三处与常识不符的事实更正

| # | 旧认知 | **现行事实** | 来源 |
|---|-------|------------|------|
| 1 | OpenAI web search $25/1k | **canonical `web_search` 为 $10/1k**;$25/1k **仅存于 `web_search_preview` + 非推理模型**(代价换来「搜索内容 token 免费」) | OpenAI pricing 页 |
| 2 | `gpt-4o-search-preview` 是 chat 搜索入口 | **已弃用,2026-07-23 停服**;现为 **`gpt-5-search-api`** | OpenAI tools-web-search(chat mode) |
| 3 | Anthropic `web_search_20250305` 是唯一版本 | 已有**三个版本**:`20250305`(基础)/ `20260209`(动态过滤)/ `20260318`(`response_inclusion` 控制) | Anthropic web-search-tool |

### 4.1 OpenAI Chat Completions `web_search_options` + `annotations`

**请求**——注意 Chat 路径是**模型路径而非可选工具**(「always searches before responding」):

```json
{
  "model": "gpt-5-search-api",
  "web_search_options": {},
  "messages": [{"role": "user", "content": "What was a positive news story from today?"}]
}
```

带 `user_location` 时是**双层嵌套**(`type` 与 `approximate` 对象同级):

```json
{
  "model": "gpt-5-search-api",
  "web_search_options": {
    "user_location": {
      "type": "approximate",
      "approximate": {"country": "GB", "city": "London", "region": "London"}
    }
  }
}
```

- `search_context_size`:枚举 `low`|`medium`|`high`,默认 `medium`;官方声明它「不设定精确 token 数,也不保证来源/引用条数」。
- `country` 为 ISO 3166-1 alpha-2;`timezone` 为 IANA ID;`city`/`region` 自由文本。
- Chat 路径**不支持**:域名过滤、完整来源列表、`external_web_access`、`return_token_budget`。

**响应 `annotations` —— 嵌套形态**(官方示例逐字):

```json
{
  "index": 0,
  "message": {
    "role": "assistant",
    "content": "the model response is here...",
    "refusal": null,
    "annotations": [
      {
        "type": "url_citation",
        "url_citation": {
          "end_index": 985,
          "start_index": 764,
          "title": "Page title...",
          "url": "https://..."
        }
      }
    ]
  },
  "finish_reason": "stop"
}
```

**litellm 类型定义独立交叉验证**(`litellm/types/llms/openai.py:684-704`)——确认嵌套,且**无 `content` 字段**:

```python
class ChatCompletionAnnotationURLCitation(TypedDict, total=False):
    end_index: int;  start_index: int;  title: str;  url: str

class ChatCompletionAnnotation(TypedDict, total=False):
    type: Literal["url_citation"]                  # Always `url_citation`.
    url_citation: ChatCompletionAnnotationURLCitation
```

**→ OpenRouter 的 `url_citation.content` 字段是其私有扩展**(§3.3),aigw 若求 OpenAI 保真则不应添加。

### 4.2 OpenAI Responses `web_search` + `web_search_call`

**请求——canonical 名称是 `web_search`**(官方逐字):

> "For new Responses API integrations, use `{ "type": "web_search" }`. The earlier `web_search_preview` tool remains available for legacy integrations, but it does not support newer controls such as `filters`, `external_web_access`, and `return_token_budget`."

```json
{"model": "gpt-6-astra", "tools": [{"type": "web_search"}], "input": "what was a positive news story from today?"}
```

可选字段全集:

```json
{
  "type": "web_search",
  "search_context_size": "low",
  "user_location": {"type": "approximate", "country": "GB", "city": "London", "region": "London"},
  "filters": {
    "allowed_domains": ["pubmed.ncbi.nlm.nih.gov"],
    "blocked_domains": ["reddit.com", "quora.com"]
  },
  "external_web_access": false,
  "return_token_budget": "unlimited",
  "search_content_types": ["image", "text"],
  "image_settings": {"max_results": 3, "caption": true}
}
```

- ⚠️ **`user_location` 在此是扁平的**(`country`/`city`/`region`/`timezone` 直挂)——**与 Chat Completions 的双层嵌套不一致**。
- `filters`:`allowed_domains` 与 `blocked_domains` 各上限 100;不带 scheme;含子域。Responses 专有。
- `external_web_access`:默认 `true`;`false` = 只读缓存。`web_search_preview` 忽略此字段。
- `return_token_budget`:仅 `"default"` / `"unlimited"`(数字与 null 被拒);限 GPT-5+ 推理模型。

**响应输出项 —— `ws_` 前缀 + 带 `action` 对象 + 扁平 annotations**(官方逐字):

```json
[
  {
    "type": "web_search_call",
    "id": "ws_67c9fa0502748190b7dd390736892e100be649c1a5ff9609",
    "status": "completed",
    "action": {"type": "search", "query": "latest news about AI"}
  },
  {
    "id": "msg_67c9fa077e288190af08fdffda2e34f20be649c1a5ff9609",
    "type": "message",
    "status": "completed",
    "role": "assistant",
    "content": [
      {
        "type": "output_text",
        "text": "On March 6, 2025, several news...",
        "annotations": [
          {
            "type": "url_citation",
            "start_index": 2606,
            "end_index": 2758,
            "url": "https://...",
            "title": "Title..."
          }
        ]
      }
    ]
  }
]
```

- `id` 前缀 **`ws_`**(已验证)。
- **确实带 `action` 对象**:`action.type` ∈ `search`(通常含 `query`,**这一种才产生工具调用费**)/ `open_page` / `find_in_page`(后两者限推理模型)。
- ⚠️ **annotation 字段在此是扁平的**(`type`/`start_index`/`end_index`/`url`/`title`),**与 Chat Completions 的嵌套完全相反**。

**SSE 事件名——五个全部按名验证**(`https://developers.openai.com/api/reference/resources/responses/streaming-events`,逐字):

```
event: response.web_search_call.in_progress
data: {"type":"response.web_search_call.in_progress","output_index":0,"item_id":"ws_123","sequence_number":0}

event: response.web_search_call.searching
data: {"type":"response.web_search_call.searching","output_index":0,"item_id":"ws_123","sequence_number":0}

event: response.web_search_call.completed
data: {"type":"response.web_search_call.completed","output_index":0,"item_id":"ws_123","sequence_number":0}
```

每个事件恰含四字段:`item_id`/`output_index`/`sequence_number`/`type`。加上通用 `response.output_item.added` / `response.output_item.done`(aigw 已于 Stage 133 实现该序列)。

**litellm 枚举独立交叉验证**(`litellm/types/llms/openai.py:1515-1517`)三个事件名一致。

> `未验证`:官方未给出把 `response.output_item.added` 与三个 `web_search_call.*` 交织的完整有序 trace。自然顺序(`output_item.added` → `in_progress` → `searching` → `completed` → `output_item.done`)属**推断**。

### 4.3 OpenAI 官方价格(已更正)

来源:`https://developers.openai.com/api/docs/pricing`(Tools 表,逐字)

| 工具行 | 价格 | 搜索内容 token |
|-------|------|--------------|
| Web search(所有模型) | **$10.00 / 1k calls** | 按模型价另计 |
| Image Web search(所有模型) | **$10.00 / 1k calls** | 按模型价另计 |
| Web search preview(推理模型,含 gpt-5 / o 系) | **$10.00 / 1k calls** | 按模型价另计 |
| Web search preview(非推理模型) | **$25.00 / 1k calls** | **免费** |

> 「搜索内容 token」官方定义:"tokens retrieved from the search index and fed to the model alongside your prompt to generate an answer."

特例(成本建模需注意):`gpt-4o-mini` 与 `gpt-4.1-mini` 走非 preview web search 时,搜索内容 token 按**固定 8,000 input tokens / call** 计。

**交叉验证**:new-api 价格表(`setting/operation_setting/tools.go:23-33`)独立给出 `web_search`=**10.0**、`web_search_preview`=10.0、`gpt-4o*`/`gpt-4.1*` 系=**25.0**(USD/1k)——与官方表一致,且其 `gpt-4o*` 特例正对应「非推理模型 $25」。

### 4.4 Anthropic `server_tool_use` / `web_search_tool_result`

**请求**(官方逐字,注释为官方自带):

```json
{
  "type": "web_search_20250305",
  "name": "web_search",

  // Optional: Limit the number of searches per request
  "max_uses": 5,

  // Optional: Only include results from these domains.
  // Use allowed_domains or blocked_domains, not both.
  "allowed_domains": ["example.com", "trusteddomain.org"],

  // Optional: Never include results from these domains
  "blocked_domains": ["untrustedsource.com"],

  // Optional: Localize search results
  "user_location": {
    "type": "approximate",
    "city": "San Francisco",
    "region": "California",
    "country": "US",
    "timezone": "America/Los_Angeles"
  }
}
```

- `allowed_domains` 与 `blocked_domains` **同时传 → 400**。
- `user_location.type` 必须为 `approximate`,且至少给一个子字段。
- `allowed_callers`:`20250305` 默认 `["direct"]`;**`20260209`+ 默认 `["code_execution_20260120"]`——默认行为翻转为在 code execution 内运行**,若 aigw 对接新版本字符串需注意。

**响应**(官方完整示例逐字):

```json
{
  "role": "assistant",
  "content": [
    {"type": "text", "text": "I'll search for when Claude Shannon was born."},
    {
      "type": "server_tool_use",
      "id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
      "name": "web_search",
      "input": {"query": "claude shannon birth date"}
    },
    {
      "type": "web_search_tool_result",
      "tool_use_id": "srvtoolu_01WYG3ziw53XMcoyKL4XcZmE",
      "content": [
        {
          "type": "web_search_result",
          "url": "https://en.wikipedia.org/wiki/Claude_Shannon",
          "title": "Claude Shannon - Wikipedia",
          "encrypted_content": "EqgfCioIARgBIiQ3YTAwMjY1Mi1mZjM5LTQ1NGUtODgxNC1kNjNjNTk1ZWI3Y...",
          "page_age": "April 30, 2025"
        }
      ]
    },
    {"text": "Based on the search results, ", "type": "text"},
    {
      "text": "Claude Shannon was born on April 30, 1916, in Petoskey, Michigan",
      "type": "text",
      "citations": [
        {
          "type": "web_search_result_location",
          "url": "https://en.wikipedia.org/wiki/Claude_Shannon",
          "title": "Claude Shannon - Wikipedia",
          "encrypted_index": "Eo8BCioIAhgBIiQyYjQ0OWJmZi1lNm..",
          "cited_text": "Claude Elwood Shannon (April 30, 1916 – February 24, 2001) was an American mathematician, ..."
        }
      ]
    }
  ],
  "id": "msg_a930390d3a",
  "usage": {
    "input_tokens": 6039,
    "output_tokens": 931,
    "server_tool_use": {"web_search_requests": 1}
  },
  "stop_reason": "end_turn"
}
```

**`usage.server_tool_use`**:

```json
{"usage": {"input_tokens": 105, "output_tokens": 6039,
           "cache_read_input_tokens": 7123, "cache_creation_input_tokens": 7345,
           "server_tool_use": {"web_search_requests": 1}}}
```

**错误形态——装在 200 响应体里,且 `content` 退化为单对象**:

```json
{
  "type": "web_search_tool_result",
  "tool_use_id": "srvtoolu_a93jad",
  "content": {"type": "web_search_tool_result_error", "error_code": "max_uses_exceeded"}
}
```

⚠️ **结构多态**:成功时 `content` 是**数组**,出错时是**裸对象**;「搜到但零结果」是**空数组**而非错误。解析器必须按类型分支。

错误码共 **6 个**(比任务描述多 2 个):`too_many_requests` / `invalid_tool_input` / `max_uses_exceeded` / `query_too_long` / **`request_too_large`**(通常是域名过滤列表过长)/ `unavailable`。另:若组织在 Console 全局禁用 web search,则返回真正的 **400 `invalid_request_error`**,而非体内错误码。

**citations**:`type: "web_search_result_location"`,四字段 `cited_text`/`url`/`title`/`encrypted_index` 全部验证。web search 的 citations **恒开**;`cited_text` 上限 **150 字符**,且 `cited_text`/`title`/`url` **不计入 input/output token**。

**流式**(官方逐字):

```
event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"srvtoolu_xyz789","name":"web_search"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"latest quantum computing breakthroughs 2025\"}"}}

// Pause while search executes

event: content_block_start
data: {"type":"content_block_start","index":2,"content_block":{"type":"web_search_tool_result","tool_use_id":"srvtoolu_xyz789","content":[{"type":"web_search_result","title":"...","url":"https://example.com"}]}}
```

即:query 以 `input_json_delta` 增量流出;**结果块整块装在 `content_block_start` 里(无 delta)**;**搜索执行期间有真实墙钟暂停**——对反代的读超时是实际风险。

**价格**:官方逐字 —— "Web search is available on the Claude API for **$10 per 1,000 searches**, plus standard token costs for search-generated content."(与任务描述一致,已验证;且与 litellm 价格表 `search_context_size_* = 0.01` 交叉一致)

计费叠加规则:"Web search results retrieved throughout a conversation are counted as input tokens, in search iterations executed during a single turn **and in subsequent conversation turns**." 每次搜索算一次(不论结果条数);**出错的搜索不计费**。

#### ⭐⭐ 两个对 aigw 致命的约束

**(1) `encrypted_content` 必须逐字节原样回传,否则 400**

官方多轮约束:必须把 assistant content blocks **包括 `encrypted_content` 在内逐字节原样**回传;缺失或被改动则请求以 **400 validation error** 失败。

而 `encrypted_content` 是 **Anthropic 服务端签发的不透明 blob,网关无法伪造**。litellm 的处理与原注释(`transformation.py:415-422`):

> The spec'd shape carries page text only in `encrypted_content`, an opaque server-issued blob that **we cannot mint**. ... So the snippet is carried in an additive `snippet` key alongside the spec fields. `encrypted_content` stays empty rather than holding plaintext, **which would assert encryption semantics that do not hold**.

litellm 产出(`:436-457`)用 `encrypted_content: ""` + 附加 `snippet` 键;sub2api(`gateway_websearch_emulation.go:365-382`)则**完全不输出 `encrypted_content`**,用附加键 `page_content`:

```go
block := map[string]string{"type": "web_search_result", "url": r.URL, "title": r.Title}
if r.Snippet != "" { block["page_content"] = r.Snippet }      // ← 键名与 litellm 的 snippet 不同
if r.PageAge  != "" { block["page_age"]     = r.PageAge }
```

**→ 结论:任何「网关自己搜」的实现都无法做到 Anthropic 规范保真。** 两家独立实现都靠附加键,且键名不统一。更关键的是:**由于真实 Anthropic 会在回放时校验 `encrypted_content`,网关自造的块一旦被客户端回放就会出问题——这正是 §5.7「历史投毒」必须剥离自造块的根本原因**(不只是上游不认,而是语义上根本无法满足)。

**(2) id 前缀硬约束**

litellm 类型定义(`types/integrations/websearch_interception.py:17-27`)与注释(`handler.py:999-1003`):

> The pair is what Anthropic's spec requires: a bare result block, or one keyed by the model's `toolu_...` id instead of a `srvtoolu_...` one, is rejected on replay ("String should match pattern '^srvtoolu_'") ...

必须用 `srvtoolu_` 前缀,且 `server_tool_use.id == web_search_tool_result.tool_use_id`。

#### 另一个无 OpenAI 对应物的状态机:`pause_turn`

长搜索可能返回 `stop_reason: "pause_turn"`,要求把暂停的 assistant 消息原样回传以继续。且**若 web search 与客户端工具在同一并行批次被调用,则返回 `stop_reason: "tool_use"`,搜索要到下一次请求才执行**——这是一个有状态续传要求,OpenAI 侧无对应概念。aigw 若做 Anthropic 侧保真需单独处理。

### 4.5 Agent 框架的 loop 上限与截断实践(佐证 §5.5)

> ⚠️ **本小节证据等级最低**:除 Anthropic 一项外,子代理对 LangGraph / OpenAI Agents SDK / Tavily / LlamaIndex / vLLM 的所有取数**均因网络阻断失败**,数值来自模型训练知识,**全部标注 `未验证`**,仅作数量级参照,不作为设计依据。

| 框架 | loop 上限 | 机制 | 验证状态 |
|------|----------|------|---------|
| LangGraph | 25 super-steps | `recursion_limit`,超限抛 `GraphRecursionError` | `未验证` |
| LangChain `AgentExecutor`(legacy) | 15 | `max_iterations` | `未验证` |
| LlamaIndex `ReActAgent`/`FunctionAgent` | ~10(旧文档为 20) | `max_iterations` | `未验证`(数值有分歧,用前须复核) |
| OpenAI Agents SDK | 10 | `Runner.run(max_turns=)`,超限抛 `MaxTurnsExceeded` | `未验证` |
| **Anthropic web search** | **无 loop 上限**,只有每工具 `max_uses` | `max_uses`,**官方不设默认值**(省略则模型自主决定) | ✅ 已验证 |

Anthropic 官方对搜索次数的经验值(已验证):"simple factual queries typically use 1–3 searches; comparative or multi-entity research can use 10 or more."

**token 膨胀抑制(Tavily,`未验证`)**:`include_raw_content` 默认 `false`(只回 snippet)——这是最关键的开关,置 `true` 会回灌全文;`search_depth` 默认 `"basic"`(`"advanced"` 耗 2 credits);`max_results` 默认 5、上限 20。

**vLLM(`未验证`)**:**无任何服务端搜索集成**,tool calling 纯用户态——调用方自行执行工具并回灌。即「自建栈给 OSS 模型加搜索」这件事,vLLM 本身不提供,必须在网关或应用层做。**这反向印证 aigw 做网关侧搜索的定位是合理的**(自建推理栈不会帮你做这件事)。

**横向结论**:业界 loop 上限集中在 **10-25**,而 litellm 专为「网关侧搜索」设的上限是 **3**(§2.2)。差异合理——框架上限要容纳多工具长链路推理,网关侧搜索只需容纳「搜一次、不够再搜一次」,**aigw 取 3 与 litellm 对齐即可,不必向框架的 10-25 靠。**

---

## 5. 对 aigw 的设计含义

### 5.1 aigw 现状基线

| 项 | 现状 | 位置 |
|----|------|------|
| 服务端工具处置 | **丢弃 + `tracing::warn`** | `adapter.rs:2446`(Stage 131) |
| Anthropic 侧 `server_tool_use` | **零处理**(grep 无命中) | — |
| 搜索后端 | **零** | — |
| HTTP 客户端 / 出口代理 | `reqwest` + `reqwest::Proxy::all`(probe/router 已用) | `probe.rs:27`、`router.rs:536-545` |
| deployment 级开关模式 | 已有(从 `model_info` 读) | `deployment.rs:44-47` |
| agentic loop | **零**(单发代理) | — |
| `call_type` 取值 | `completion` / `embedding` / `responses` / `function` | `chat.rs` 等 |
| 计费函数 | `calc_spend(prompt, completion, …)` 纯 token | `chat.rs:104-123` |
| 定价来源 | `proxy_models.model_info` **自由 JSON** | `models.rs:781` |
| SpendLog 列 | 已有 `model_group` / `metadata` / `call_type`;有 `image_tokens` 专列先例 | `models.rs:146-191`、迁移 `025_image_tokens.sql` |

**三个有利条件**:
1. **`model_info` 是自由 JSON** → 加 `input_cost_per_query` **不需要迁移**;
2. **`image_tokens` 已立下先例**——「专列 + source tag 进 metadata」(`models.rs:188-191`:`Value source: upstream → client-side estimate → NULL`,`metadata.image_tokens_source` 存 `"upstream"|"estimated"`),**搜索次数可照抄这个模式**;
3. **`Deployment` 已有「从 `model_info` 读开关」的成熟模式**(`deployment.rs:44-47`:`chat_template_compat`、`developer_role_passthrough`)→ 搜索开关(`websearch_mode` 等)可同构新增,**零迁移、零新机制**。另 `probe.rs:27` 已有 `reqwest::Proxy::all` 用法,搜索后端的出口代理可直接复用(对标 sub2api 的按账号代理)。

### 5.2 搜索执行该放在哪一层

aigw 的三条协议路径各自独立(`chat.rs` 4263 行 / `responses.rs` 1975 行 / `v1_messages.rs` 2405 行),且**各自都有 `insert_spend_log` 调用点(chat.rs 单文件 8 处)**。

| 方案 | 说明 | 评价 |
|------|------|------|
| 各路由内联 | 在三个路由各写一遍 | ❌ 三份重复,litellm 正因此踩坑(§2.3 三 surface 行为不一致) |
| **独立模块 + 三路由接线** | `aigw-core` 新增 `websearch` 模块,暴露「检测 / 执行 / 注入」三个纯函数,路由只负责调用与计费 | ✅ **推荐**。与 `adapter.rs` 现有纯函数风格一致(`normalize_responses_tools` 等) |
| 中间件层 | axum layer 拦截 | ❌ 需反序列化/重序列化整个 body,且拿不到 deployment 上下文 |

**设计 C 的接线位置最简单**:在 `adapt_request` 之后、上游请求发出之前——即 sub2api `gateway_forward.go:99` 与 Higress `onHttpRequestBody` 的同一个位置。**纯函数签名可以是 `fn inject_search_results(body: &mut Value, results: &[SearchResult], template: &str)`**,与现有 `merge_developer_into_system` / `normalize_content_parts` 同构,**可直接复用现有的请求体改写测试模式**。

**litellm 的反面教材值得明说**:它把 loop 分散实现在 `handler.py`(Anthropic)+ `chat_completion_agentic_loop.py`(Chat)+ `async_build_responses_agentic_loop_plan`(Responses)三处,导致**撞上限行为在三个 surface 不一致**(一个优雅收尾、两个抛错并泄漏内部工具名)——这是 aigw 应当从第一天就避免的。

### 5.3 四个候选设计(C / A / B / D)

#### 设计 C:prompt 注入(search-then-inject)—— ⭐ 推荐作为阶段 1

**触发**:请求带 `web_search` 工具 或 `web_search_options` 参数(Higress 用后者,与 litellm「派生」路线正好接上——**aigw 现有的丢弃逻辑只需改成「丢弃 + 置搜索标记」**)。
**行为**:发往上游**之前**执行搜索 → 把结果按模板注入最后一条 user 消息(Higress)或 system 消息(Portkey)→ **照常单次调用上游** → 可选地在响应里追加 references。

| 维度 | 评价 |
|------|------|
| 工作量 | **最小**——纯请求体改写 + 一个搜索 HTTP 客户端;**无循环、无流式重建、无服务端工具块合成** |
| 上游依赖 | **零** ✅(不需要 function calling,不需要服务端工具;本环境已验证上游接受普通 messages) |
| 回复质量 | **高**(模型读了搜索结果再答,保留综合能力) |
| 流式 | **几乎无改动**——上游流式原样透传;若要加 references,按 Higress 的 `UnifySSEChunk` 在尾部注入 |
| 客户端契约 | **最诚实**——不合成任何假的 `server_tool_use`/`web_search_call`,因而 §4.4 的 `encrypted_content` 不可伪造问题与 §5.7 的历史投毒问题**均不存在** |
| 计费 | 干净:1 条搜索行(方案 1)+ 1 条正常模型行 |
| 代价 | ① **模型不能自主决定是否搜索/搜什么**(除非像 Higress 那样先调一次 LLM 做 `searchRewrite`,多一跳);② 注入内容计入 prompt token(成本可见但不可避免);③ 客户端拿不到结构化 citations(除非自行在响应里追加) |

**依据**:Higress `ai-search`(`main.go` prompt 改写 + 5 引擎 + 流式注入)与 Portkey `exa/online`(`plugins/exa/online.ts` system 注入)**两家独立实现**;Higress 的 `web_search_options.search_context_size` → 1/3/5 条 query 的映射可直接照抄,**让 aigw 对客户端呈现的参数语义与 OpenAI 一致**。

#### 设计 A:短路(short-circuit)—— 阶段 2(服务 Claude Code)

**触发**:请求 tools **只含** web_search(litellm `handler.py:308` / sub2api `isOnlyWebSearchToolInBody`)。
**行为**:不调上游模型;执行搜索;合成 `server_tool_use` + `web_search_tool_result` + `text` 三块返回。

| 维度 | 评价 |
|------|------|
| 工作量 | 小——无循环;SSE 为固定 5 事件(aigw 已有构造能力) |
| 上游依赖 | **零** |
| 客户端覆盖 | **Claude Code / Claude Desktop 的独立搜索子请求**(litellm 与 sub2api 都专为此场景而建) |
| 回复质量 | 低(模板化列表,无模型综合) |
| 计费 | 干净:**只有搜索费,没有模型费** |
| 代价 | **必须合成 Anthropic 服务端工具块** → 踩 §4.4(`encrypted_content` 无法伪造)+ §5.7(历史投毒,须按 id 前缀剥离)两个坑 |

**价值点**:这是唯一能让 **Claude Code 的 WebSearch 按钮真正可用**的设计——那是一个独立的 `/v1/messages` 子请求,形态 C 对它不适用(它本身不需要模型综合)。

#### 设计 B:agentic loop —— 阶段 3(按需)

**行为**:发请求前把 web_search 换成内部 function 工具 → 模型回 tool_call → 执行搜索 → 回灌 → 再请求,上限 N 轮。

| 维度 | 评价 |
|------|------|
| 工作量 | **大**(循环 + 指纹防环 + 上限 + 流式降级重建 + 每轮独立计费) |
| 上游依赖 | **要求上游支持 function calling**——**本环境未实测,是选型阻塞项**(§5.9) |
| 回复质量 | 最高(模型可多轮追问式搜索) |
| 计费 | 复杂:1 条搜索行 + N+1 条模型行 |
| 必须照抄的安全栏 | ① `max_agentic_loops` 默认 3;② **客户端不可覆盖该值**;③ 指纹成环断环;④ 撞上限时剥除内部工具名并改 `stop_reason=end_turn`;⑤ **每轮独立 logging/spend 上下文**(§2.8 末的 litellm bug) |

#### 设计 D(兜底):维持丢弃 + 诚实告知

保持现状,但在响应里明确告知搜索不可用而非静默。工作量极小,仅避免「客户端以为搜了」的误判。

#### 推荐路线

| 阶段 | 设计 | 理由 |
|------|------|------|
| **1** | **C(prompt 注入)** | 上游零依赖、保留模型综合、流式几无改动、**不触碰两个硬约束**;业界两家独立验证 |
| 2 | **A(短路)** | 唯一能让 Claude Code WebSearch 可用;但需同时实现 §5.7 历史剥离 |
| 3 | B(agentic loop) | 质量最高但工作量最大,且上游 function calling 能力需先实测 |

### 5.4 流式策略

**业界无一例外:不做「边循环边真流式」。** 三种形态各自的流式方案:

| 形态 | 方案 | 来源 | 说明 |
|------|------|------|------|
| **C(prompt 注入)** | **上游流式原样透传** | Higress `ai-search` | 请求体在发出前已改写完毕,**上游 SSE 不需要任何干预**;若要追加 references,用 `ProcessStreamingResponseBody` + `UnifySSEChunk` 按 `\n\n` 切分注入 |
| A(短路) | 手写固定事件序列 | sub2api `writeWebSearchStreamResponse` | 5 个事件:`message_start` → `server_tool_use`(start+stop) → `web_search_tool_result`(start+stop) → `text`(start+delta+stop) → `message_delta`+`message_stop`。**写完即 flush,无需缓冲** |
| B(loop) | 降级非流式 + 重建 | litellm `_maybe_wrap_in_fake_stream` | `stream=True` → 内部改 False → 跑完 loop → 用最终结果重建 SSE。(Bifrost Agent Mode 索性明确不支持流式) |

**→ 形态 C 的流式成本几乎为零,这是它作为阶段 1 的又一个理由。**

sub2api 的一个细节值得照抄(`writeWebSearchStreamResponse:221-232`):**用 `[]func() error` 顺序执行,任一写失败就 break 并 warn**,而不是让客户端断连后继续写。

**一个真实流式风险(Anthropic 官方已明示)**:原生 web search 在流式里**搜索执行期间存在真实墙钟暂停**(官方示例注释 `// Pause while search executes`,§4.4)。形态 C 的暂停发生在**首字节之前**(还没开始回 SSE),故只影响 TTFB 而不会造成「流中途静默」——**比形态 A/B 更安全**。sub2api 设 `X-Accel-Buffering: no`(`setSSEHeaders`)即为应对此类问题。

**各 surface 的保真度差异(必须明确告知产品)**:

| 客户端协议 | 形态 C 的呈现 | 形态 A/B 需合成的形状 | 保真度 |
|-----------|-------------|-------------------|-------|
| Anthropic `/v1/messages` | 普通 text 回复(含 markdown 链接) | `server_tool_use` + `web_search_tool_result` + `citations` | A/B **中**——`encrypted_content` 无法伪造(§4.4),须附加键;回放需剥离。**C 无此问题** |
| OpenAI Responses | 普通 `output_text` | `web_search_call`(`ws_` 前缀 + `action`)+ 扁平 `url_citation` | A/B **中**——形状可构造,但 litellm 未做此保真,无业界参照(§2.3)。**C 无此问题** |
| OpenAI Chat Completions | 普通 content | 嵌套 `url_citation` annotations | **高**——形状简单且无不可伪造字段,**保真成本最低**;C 也可选择性追加 |

**→ 形态 C 用「markdown 链接引用」替代结构化 citations,这正是 OpenRouter 默认 `search_prompt` 的做法**(§3.2:"Cite them using markdown links named using the domain of the source")——**有业界先例,不是降级妥协。** 若后续要补结构化 citations,**从 Chat Completions 的 `annotations` 开始**(唯一无不可伪造字段的协议)。

### 5.5 护栏:max-iteration 与截断

| 护栏 | 业界值 | 来源 | 验证 |
|------|-------|------|------|
| loop 上限 | **3**(默认),可配到 5 | litellm `agentic_loop_settings.py:24` | ✅ |
| 客户端能否改上限 | **不能**(untrusted-field list) | litellm `ARCHITECTURE.md:236-237` | ✅ |
| 成环断路 | tool_calls JSON 指纹重复即断 | litellm `llm_http_handler.py:5219-5220` | ✅ |
| 结果条数默认 | **5** | sub2api `webSearchDefaultMaxResults=5`;OpenRouter `max_results` 默认 5;Tavily 默认 5 | ✅(前二) |
| 单结果字符预算 | Exa highlights **~2000-4000 字符**;`search_context_size` low/medium/high = **5000/15000/30000** | OpenRouter 文档 | ✅ |
| Anthropic 每请求搜索次数 | `max_uses`,**官方无默认值**;经验值「简单事实 1-3 次,多实体研究 10+」 | Anthropic 官方 | ✅ |
| server-tool 总步数 | 30(OpenRouter `max_tool_calls` 默认即上限) | OpenRouter 文档 | ✅ |
| Agent 框架 loop 上限 | 10-25(LangGraph 25 / LangChain 15 / Agents SDK 10) | 见 §4.5 | `未验证` |

**选值建议**:**loop 上限取 3,与 litellm 对齐**,不向 Agent 框架的 10-25 靠——框架上限要容纳多工具长链路推理,网关侧搜索只需容纳「搜一次、不够再搜一次」(§4.5 末)。

**token 膨胀的三个现成抓手**:
1. **`max_results` 默认 5**(而非 10+)——三家一致;
2. **只回灌 snippet 不回灌全文**——litellm `format_search_response`(`transformation.py:459-479`)就是 `Title/URL/Snippet` 三行拼接,**不含正文**;Tavily 侧对应 `include_raw_content=false`(默认,`未验证`);
3. **单结果字符上限**——可直接采用 OpenRouter 的 `search_context_size` 三档语义(5000/15000/30000 字符/结果),既有现成语义又能直接映射客户端传来的同名参数。

**另一条必须纳入护栏的成本事实**(Anthropic 官方,§4.4):搜索结果**在当轮的多次搜索迭代中、以及后续所有会话轮次中,都持续按 input token 计费**。即 loop 轮数不只放大搜索费,**还会把搜索结果反复计入后续每一轮的 input token**——这是「上限取 3 而非 10」的另一个经济理由。

### 5.6 ⭐ 搜索调用在 SpendLog 里应该长什么样 —— 四个方案

aigw 现有 `SpendLog` 可用字段:`call_type`、`model`、`model_group`、`custom_llm_provider`、`spend`、`prompt_tokens`/`completion_tokens`/`total_tokens`、`metadata`(JSON)、`call_id`(PK, UUID v7)、`session_id`。

#### 方案 1:独立行(litellm 路线)

一次搜索 = **一条新 SpendLog 行**。

```
call_id      = <新 UUID v7>
call_type    = "search"                       # 新增取值
model        = "tavily/search"                # provider/endpoint
model_group  = "<搜索工具名>"                   # litellm 即如此复用该字段
custom_llm_provider = "tavily"
spend        = number_of_queries × input_cost_per_query
prompt_tokens = completion_tokens = total_tokens = 0
session_id   = <与触发它的 LLM 请求同一 session>   # 用于串联
metadata     = {"search_query_count": 1, "parent_call_id": "<LLM 请求的 call_id>"}
```

| | |
|---|---|
| ✅ 优点 | 账目最清晰(搜索费与模型费可独立聚合/出账);复用现有 `insert_spend_log`,**零迁移**;与 litellm 列兼容(aigw 的 schema 本就对齐 litellm);天然支持「一次请求多轮搜索」=多行 |
| ⚠️ 代价 | 行数膨胀(一个用户请求可能产出 2-4 行);`total_tokens=0` 的行会影响「平均 token」类统计口径;需要 `parent_call_id` 才能回溯归属 |
| 适配 | **设计 A 与 B 都适用**;是三者里唯一能干净表达「搜索独立于模型」的 |

#### 方案 2:附加费并入同行(new-api 路线)

搜索费作为 surcharge 加到**触发它的那条 LLM 行**的 `spend` 上,次数与单价记进 `metadata`。

new-api 的实现(`service/text_quota.go:109-117` + `:435-443`):

```go
summary.ClaudeWebSearchCallCount = ctx.GetInt("claude_web_search_requests")
if summary.ClaudeWebSearchCallCount > 0 {
    summary.ClaudeWebSearchPrice = operation_setting.GetToolPrice("web_search")
    surcharge = surcharge.Add(decimal.NewFromFloat(summary.ClaudeWebSearchPrice).
        Div(decimal.NewFromInt(1000)).Mul(dGroupRatio).Mul(dQuotaPerUnit).
        Mul(decimal.NewFromInt(int64(summary.ClaudeWebSearchCallCount))))
}
// ...
other["web_search"] = true
other["web_search_call_count"] = summary.WebSearchCallCount
other["web_search_price"]      = summary.WebSearchPrice
```

aigw 对应形态:

```
spend    = token 费 + (search_count × cost_per_query)
metadata = {"web_search": true, "web_search_call_count": 2, "web_search_price": 0.008}
```

| | |
|---|---|
| ✅ 优点 | **行数不变**(一请求一行,现有报表/聚合口径完全不受影响);与 litellm 的「上游原生搜索」路径口径一致(§2.8 路径 2);实现最省(只改 `calc_spend` + metadata) |
| ⚠️ 代价 | 搜索费被 token 费淹没,**无法独立聚合**(除非扫 metadata JSON);`spend` 不再等于 `tokens × 单价`,对账时易困惑;**多轮搜索的明细丢失**(只剩次数) |
| 适配 | **设计 A 不适用**(短路路径没有 LLM 调用,无「同一行」可挂);仅适合设计 B 或「上游原生搜索」计费 |

> ⚠️ **这是方案 2 的致命限制**:设计 A(短路)根本不产生模型调用,没有宿主行。若选设计 A 必须用方案 1 或 3。

#### 方案 3:专列 + source tag(aigw `image_tokens` 自有先例)

加一个 `search_count: Option<i32>` 列(迁移 `028_search_count.sql`),计费并入 `spend`,来源标记进 `metadata.search_count_source`(`"gateway"` = 网关自己搜的 / `"upstream"` = 上游回报的 `web_search_requests`)。

```
spend                 = token 费 + 搜索费
search_count          = 2
metadata.search_count_source = "gateway" | "upstream"
```

| | |
|---|---|
| ✅ 优点 | **可索引、可直接 SQL 聚合**(不必扫 JSON);**一个列同时覆盖两种来源**(网关自执行 / 上游原生回报)——这正是 aigw 未来会同时面对的两种情形;与 `image_tokens` 模式一致,**认知成本低、有先例可循** |
| ⚠️ 代价 | **需要三方言迁移**(pg/mysql/sqlite);前端 Spend Logs 列表/抽屉需加展示;搜索费仍与 token 费混在 `spend` 里(除非再加 `search_spend` 列) |
| 适配 | 设计 A(行里只有搜索费,token 全 0)与 B 都适用;**与方案 1 可叠加**(独立行 + 该行带 `search_count`) |

#### 方案 4(补充):回传进响应体(Vercel 路线)

把搜索次数与聚合成本放进响应体,让客户端可见。Vercel 的位置是 `choices[0].message.provider_metadata.gateway.gatewayToolCalls`;Anthropic/OpenRouter 的标准位置是 `usage.server_tool_use.web_search_requests`(§4.4)。

| | |
|---|---|
| ✅ 优点 | 客户端可自行核账;**`usage.server_tool_use.web_search_requests` 是 Anthropic/OpenRouter 的官方字段,填它等于协议保真**;与落库方案不冲突 |
| ⚠️ 代价 | 只是呈现,不替代落库 |
| 适配 | **应与方案 1 或 3 叠加**,而非替代 |

#### 推荐

| 阶段 | 推荐 | 理由 |
|------|------|------|
| 阶段 1(设计 C prompt 注入) | **方案 1(独立行)+ 方案 4(回传 usage)** | 零迁移、账目清晰;搜索与模型天然是两次独立调用,各自一行最自然;同时填 `usage.server_tool_use.web_search_requests` 做协议保真 |
| 阶段 2(设计 A 短路) | **方案 1** | 短路路径没有宿主行可挂,**方案 2 不可用** |
| 阶段 3(设计 B loop) | **方案 1 + 方案 3 叠加** | 搜索独立成行,同时用 `search_count` 专列承载「上游原生回报次数」,两种来源统一口径 |

**无论选哪个,litellm 踩过的坑必须避免**(§2.8 末):**每轮上游调用要有独立的 logging/spend 上下文**,否则首轮花费会被去重标记静默吞掉,导致 SpendLog 与真实账单不符。

**另一条 Vercel 的安全设计值得照抄**:server tool 的 `config` 字段是 **developer 默认值,覆盖模型生成的值**,官方说明这是**防 prompt 注入**措施。即 `allowed_domains` / `max_results` 这类安全参数**不能让模型(或客户端请求体)自由决定**——与 litellm 把 `max_agentic_loops` 放进 untrusted-field list 是同一个思路(§2.2)。

### 5.7 多轮会话的「历史投毒」——sub2api 的血泪教训(仅设计 A/B 适用)

> ⚠️ **适用范围**:本节仅适用于**会向客户端合成服务端工具块**的设计(A 短路 / B loop)。**设计 C(prompt 注入)不产生自造块,完全不受此问题影响**——这是推荐 C 作为阶段 1 的核心理由之一。

**问题**:网关合成的 `server_tool_use` / `web_search_tool_result` 块会被客户端(Claude Code)**原样回放**在下一轮请求的历史里。这会同时触发**两个**独立故障:

1. **上游不认**:上游 MaaS(DeepSeek/Kimi/GLM)只接受 text/thinking/image/tool_use/tool_result,见到 `server_tool_use` 一律 400;
2. **真 Anthropic 也不认**:§4.4 已验证,真实 Anthropic 会在回放时**逐字节校验 `encrypted_content`**,而网关自造的块里该字段是空的或缺失的 → 400 validation error。

**即自造块在任何上游都无法安全回放——这不是上游兼容性问题,而是语义上的根本不可能。**

sub2api 为此专门写了 `FilterWebSearchHistoryBlocks`(`gateway_websearch_block_filter.go:26-45`,原注释):

> 1. Emulation-synthesized blocks — `server_tool_use` / `web_search_tool_result` whose tool-use ID carries `webSearchToolUseIDPrefix` — are fabricated locally by the web-search emulation. **No upstream ever issued them, so clients replaying the conversation (e.g. Claude Code) poison every follow-up request.** They are stripped for all upstreams.
> 2. For passback-required upstreams (DeepSeek/Kimi/GLM …) **all** `server_tool_use` / `web_search_tool_result` blocks are stripped: these upstreams only accept text/thinking/image/tool_use/tool_result and **reject anything else with 400 "invalid value: `server_tool_use`"**.

实现要点:
- **靠 id 前缀区分自造与真实**(`webSearchToolUseIDPrefix = "srvtoolu_ws_"`,注意比 Anthropic 的 `srvtoolu_` 多了 `ws_` 标记)——这是一个**可直接照抄的巧妙设计**:自造块打上可识别前缀,回放时精确剥离;
- 剥离后内容为空的消息补占位文本(`"(content removed)"` / `"(assistant content removed)"`),避免空 content 被上游拒;
- 搜索上下文不丢失——因为**合成的 assistant turn 末尾总有一段 text 摘要**,剥离块后摘要仍在。

接线点:`gateway_forward.go:347` 与 `gateway_anthropic_passthrough.go:87`,**在请求发往上游前执行**。

**→ 这是 aigw 实现设计 A 或 B 都必须同时做的配套,否则「第一轮能搜、第二轮整个会话 400」。** 前篇与本篇任务描述均未覆盖此点,属本次调研的关键增量发现。**设计 C 无需此配套。**

### 5.8 开关与多后端治理(sub2api 的可照抄部分)

sub2api 的 emulation 有一套完整的开关与后端治理,比 litellm 的「全局 callback + enabled_providers」更贴近 aigw 的多租户模型:

**五级开关判定链**(`gateway_websearch_emulation.go:54-81`):

```
manager 已初始化 → 请求只含 web_search 工具 → 全局开关开 → 账号级模式 → 渠道级开关
                                                      ├ "enabled"  强制开
                                                      ├ "disabled" 强制关
                                                      └ "default"  跟随渠道配置
```

**→ aigw 对应物**:全局 config → key/team 级 → deployment 级(`model_info.websearch_mode`,复用 §5.1 第 3 条的既有模式)。litellm 另有搜索工具的 key/team 授权(`handler.py:1557-1580`),与 aigw 现有 key→model 授权同构。

**多搜索后端治理**(`internal/pkg/websearch/manager.go`):

| 机制 | 实现 | 对 aigw 的价值 |
|------|------|--------------|
| 配额加权负载均衡 | `selectByQuotaWeight`:剩余配额越多优先级越高;`quota_limit=0`(未设限)权重 0 排最后 | 多搜索 key 轮换 |
| 配额原子预留 + 失败回滚 | Redis Lua `quotaIncrScript`(INCR + 首次设 TTL),搜索失败则 DECR 回滚 | 防超额 |
| 配额周期 | 按 `subscribed_at` 月度重置,无订阅日期则 31 天 + 24h buffer | 对齐 provider 计费周期 |
| Redis 不可用降级 | `tryReserveQuota` 记 warn 并**放行**(不因 Redis 挂掉而拒服务) | 可用性优先 |
| 代理不可用故障转移 | `ErrProxyUnavailable` → `UpstreamFailoverError` 触发账号切换 | 复用 aigw 既有 proxies 机制 |

**→ 阶段 1 不必做全套。** 最小可用只需「单后端 + key 配置」;配额加权 LB 可在多 key 场景出现后再补。但 **Redis 不可用时放行而非拒服务** 这个取舍值得第一天就定下来。

### 5.9 实施前必须先实测的三件事

| # | 待测 | 为什么关键 | 阻塞哪个设计 |
|---|------|-----------|------------|
| 1 | 上游 MaaS 是否支持 **function calling**(**网关自行注入**的 tools + tool_calls 往返) | **设计 B 的硬前提**。Stage 132 已验证「Codex 客户端声明的工具」经 aigw 到上游返 200,但需确认**网关注入**的工具同样被模型正确调用 | **B** |
| 2 | 上游是否接受历史里的 `server_tool_use` 块 | 决定 §5.7 剥离是「全剥」还是「只剥自造」。sub2api 实测 DeepSeek/Kimi/GLM **全部 400** | **A / B** |
| 3 | 选定搜索后端的可达性与配额 | `searxng/search` 在 litellm 价格表里是 **0.0**(自建),成本最低;Serper $0.001/query、Tavily $0.008/query;Higress 另提供 quark(阿里云 IQS)可选 | **全部** |

**注意:设计 C 不被第 1、2 项阻塞**——它只改请求体 messages 的文本内容,不引入任何工具声明或非常规内容块,**用本环境已验证可用的能力即可落地**。这是推荐它作为阶段 1 的决定性理由。

**第 3 项是唯一的真实前置依赖**(任何设计都需要一个搜索后端 + key)。

---

## 6. 引用与验证状态

### 6.1 源码(本地直读,已验证)

| 来源 | 位置 | 验证内容 |
|------|------|---------|
| litellm 架构说明 | `litellm/integrations/websearch_interception/ARCHITECTURE.md` | 短路/loop 两形态、流式降级重建、loop 上限语义与三 surface 不对等、provider 选择兜底 |
| litellm 短路实现 | `handler.py:246-384` | 触发条件(tools 全为 web_search)、native block 合成、合成响应形状 |
| litellm 短路取舍注释 | `handler.py:282-292` | 「短路会跳过模型综合,对原生支持的 provider 是倒退」 |
| litellm loop 编排 | `llm_http_handler.py:5621-5740` | 两 hook(门禁 + 执行)、安全栏在 try/except 之外 |
| litellm 安全栏 | `llm_http_handler.py:5201-5230` | 指纹成环 + 深度上限 |
| litellm loop 默认上限 | `litellm_core_utils/agentic_loop_settings.py:24` | `DEFAULT_MAX_AGENTIC_LOOPS = 3` |
| litellm 流式重建 | `llm_http_handler.py:5582-5619` | `FakeAnthropicMessagesStreamIterator`、只对 anthropic surface 生效 |
| litellm 流式零开销优化 | `llm_http_handler.py:5156-5184` | 无 agentic callback 时跳过 SSE 缓冲重建 |
| litellm 注入工具定义 | `tools.py:15-34`、`constants.py:590` | `litellm_web_search` 标准工具 |
| litellm 四格式检测 | `tools.py:118-293` | `startswith("web_search_")` 前向兼容 |
| litellm native block 构造 | `transformation.py:398-457` | `encrypted_content` 无法伪造 → 留空 + 附加 `snippet` 键 |
| litellm id 前缀硬约束 | `handler.py:999-1003` | 非 `srvtoolu_` 前缀回放被 400 |
| litellm Anthropic 块类型 | `types/integrations/websearch_interception.py:11-49` | 配对要求、配置 TypedDict |
| litellm 结果文本格式 | `transformation.py:459-479` | Title/URL/Snippet 三行,不含正文 |
| litellm Chat 回灌 | `transformation.py:380-396`、`handler.py:1745-1746` | OpenAI 无 `web_search_tool_result` 等价块 |
| litellm Responses 回灌 | `handler.py:1168-1185` | 用 `function_call`/`function_call_output` 对,**不合成 `web_search_call`** |
| litellm 并行搜索 | `handler.py:1741-1743` | `asyncio.gather(..., return_exceptions=True)` |
| litellm thinking 约束 | `handler.py:1255-1279` | `max_tokens > budget_tokens + 1024` |
| **litellm 搜索计费元数据** | `handler.py:1582-1602` | 不带归属元数据则被 spend hook 丢弃;`model_group` 复用为搜索工具名 |
| **litellm 搜索顶层调用** | `handler.py:1513-1524` | `litellm.asearch()` → 独立 SpendLog 行 |
| **litellm logging 去重坑** | `handler.py:1281-1295` | 复用 logging_obj 会吞掉首轮花费(SpendLog/账单不符的根因) |
| **litellm 搜索计价** | `litellm/search/cost_calculator.py:31-85` | `input_cost_per_query` + `tiered_pricing`(按 `max_results` 分档) |
| litellm 计价接线 | `cost_calculator.py:585-593`、`:1512-1540` | `call_type in ("search","asearch")` |
| **litellm 原生搜索计价** | `llm_cost_calc/tool_call_cost_tracking.py:531-566` | `search_context_cost_per_query` 三档,缺省按 medium |
| litellm 搜索检测(计费用) | `tool_call_cost_tracking.py:505-522` | Chat 看 `url_citation`、Responses 看 `web_search_call` |
| litellm 价格表 | `model_prices_and_context_window.json` | 24 个 `mode:"search"` 条目(见 §2.8 表);307 个模型带 `search_context_cost_per_query`;Anthropic 系 $0.01/query |
| litellm CallTypes | `types/utils.py:396-397` | `search` / `asearch` |
| litellm SpendLogs schema | `schema.prisma:628-667` | 与 aigw 列对齐确认 |
| litellm `/search` 端点 | `proxy/search_endpoints/endpoints.py:17-60`、`:241-253` | 4 个 POST 路由 + 2 个 GET;对齐 Perplexity Search API |
| litellm 搜索鉴权 | `handler.py:1557-1580` | key / team 级搜索工具授权 |
| litellm provider 兜底 | `handler.py:1638-1672` | 指定名 → 首个 → perplexity |
| litellm OpenAI annotation 类型 | `types/llms/openai.py:684-704` | **嵌套 `url_citation`,无 `content` 字段** |
| litellm Responses SSE 事件名 | `types/llms/openai.py:1515-1517` | 三个 `response.web_search_call.*` |
| litellm loop 上限测试 | `tests/.../test_websearch_agentic_loop_cap.py:188-253` | 剥除内部工具名、`stop_reason=end_turn`、块序列断言 |
| litellm 短路测试 | `tests/.../test_websearch_short_circuit.py:25-184` | 混合工具不短路、错 provider 不短路、搜索失败返错误文本 |
| **sub2api 搜索模拟** | `backend/internal/service/gateway_websearch_emulation.go`(全文) | 短路实现、SSE 5 事件、非流式 JSON、`page_content` 附加键 |
| **sub2api 历史块剥离** | `gateway_websearch_block_filter.go`(全文) | 自造块 id 前缀 `srvtoolu_ws_`、回放投毒、上游 400、空内容占位 |
| sub2api 接线点 | `gateway_forward.go:99-101`、`:347`;`gateway_anthropic_passthrough.go:87` | 上游请求前拦截 |
| sub2api provider 管理 | `internal/pkg/websearch/manager.go`、`types.go`、`tavily.go`、`brave.go` | Redis 配额加权 LB + 预留/回滚、代理不可用故障转移、`defaultMaxResults=5` |
| sub2api 开关层级 | `gateway_websearch_emulation.go:54-81` | manager 存在 → 仅 web_search → 全局开关 → 账号(enabled/disabled/default)→ 渠道 |
| **new-api 搜索附加费** | `service/text_quota.go:84-120`、`:435-443` | surcharge 并入同行 + `other.web_search_*` |
| new-api 上游次数提取 | `relay/channel/claude/relay-claude.go:234-235` | `usage.server_tool_use.web_search_requests` → ctx |
| **new-api 工具价格表** | `setting/operation_setting/tools.go:23-33` | `web_search`=**10.0**、`web_search_preview`=10.0、`gpt-4o*`/`gpt-4.1*` 系=**25.0**(USD/1k) |
| new-api 类型定义 | `dto/claude.go:185-192`、`:600`;`dto/openai_request.go:86`、`:834` | 仅类型,无执行(无任何搜索后端 HTTP 客户端,grep 确认) |
| aigw 现状 | `adapter.rs:2446`;`models.rs:146-191`、`:781`;`chat.rs:104-123`;`deployment.rs:17-47`;`probe.rs:27`;迁移 `002`/`025` | 丢弃策略、SpendLog 列、`model_info` 自由 JSON、`image_tokens` 先例、deployment 开关模式、`reqwest::Proxy` |
| aigw TD 账本 | `docs/12-technical-debt.md:208`(TD-017c) | 本调研的立项目标 |

### 6.1b 其他网关源码 / 文档(由子代理取证)

> 取证环境说明:该子代理环境下 `WebFetch` 返回 400、`github.com` 与 `raw.githubusercontent.com` 对 curl 返回 403/422,**源码经浏览器访问 raw URL 后读 innerText 取得**,非 git clone。故行号不可引,仅引文件路径与关键代码片段。

| 来源 | 位置 | 验证内容 |
|------|------|---------|
| **Higress `ai-search`** | `plugins/wasm-go/extensions/ai-search/`(`main.go` + `engine/{google,bing,quark,arxiv,elasticsearch}`) | **形态 C 真实执行**:`strings.Replace(config.promptTemplate, "{search_results}", ...)` + `sjson.SetBytes(body, "messages.N.content", prompt)` + `ReplaceHttpRequestBody`;5 引擎硬编码端点;`web_search_options.search_context_size` → 1/3/5 query;`ProcessStreamingResponseBody` + `UnifySSEChunk` 流式注入;多引擎并行 + 按 `result.Link` 去重;`searchRewrite` 先调 LLM |
| **Higress `ai-agent`** | 同仓 `ai-agent` 插件 | 网关内完整 ReAct loop,`llm.maxIterations` **默认 15**,OpenAPI 描述工具,支持流式/非流式 |
| **Higress `ai-statistics`** | 同仓 | 仅记上游 usage 的 input/output token → 搜索 $ 成本不被追踪(`未验证` 推论依据) |
| **Portkey `exa/online`** | `plugins/exa/online.ts`、`plugins/exa/manifest.json`、`plugins/index.ts` | **形态 C 真实执行**:网关请求 `https://api.exa.ai/search`,`insertSearchResults` 注入 system 消息,内容包在 `<web_search_context>`;manifest `"type":"transformer"`、`"supportedHooks":["beforeRequestHook"]`;参数 prefix/suffix/numResults/includeDomains/excludeDomains/timeout;Tavily 在文档但不在 OSS index → 闭源 |
| **Vercel AI Gateway** | `vercel.com/docs/ai-gateway/models-and-providers/web-search` | 裸 HTTP `tools:[{"type":"vercel:exa_search","config":{...}}]`;6 种 `vercel:*` server tool;**官方原话「AI Gateway executes the search ... does not include client-facing `tool_calls` or raw search results」**;`provider_metadata.gateway.gatewayToolCalls` 回传次数+成本;`config` 为 developer 默认值覆盖模型生成值(防注入);per-request 价格 Perplexity $5/1k、Exa $7/1k(+$1/1k 超额)、Parallel $5/1k |
| **Cloudflare AI Gateway** | `developers.cloudflare.com/ai-gateway/usage/web-search/` | **官方明确「Search runs on the upstream provider」+「does not provide a provider-agnostic web search abstraction」+「does not charge a separate web-search fee」**;各 provider 激活方式表 |
| **Kong AI Gateway** | `docs.konghq.com/hub/kong-inc/ai-proxy/` | web_search 零命中;`ai-rag-injector` 仅 Redis Vector / pgvector(**非 web**);function calling 支持;流式支持 |
| **Apache APISIX** | `apisix.apache.org/docs/apisix/plugins/ai-proxy/` | web_search 零命中;`ai-rag` 仅 Azure OpenAI embeddings + Azure AI Search(**非 web**);支持 SSE + tool use 透传;`llm_tool_count` 等指标 |
| **Bifrost** | **源码 `github.com/maximhq/bifrost/core@v1.11.2`**(经 `proxy.golang.org` 取得,绕开 github 封锁):`mcp/agent.go`、`mcp/exec.go`、`bifrost.go`、`schemas/responses.go`;文档 `docs.getbifrost.ai/mcp/{overview,agent-mode}` | **真实执行(进程内 loop)**:`executeAgent` 的 `for depth < maxAgentDepth` → `extractToolCalls` → `executeToolFunc` → `makeLLMCall`;`bifrost.go:858-859`/`:960-961` **无条件进入**(opt-in 的是「哪些工具可自动跑」`tools_to_auto_execute` 默认 none,**不是 loop 是否存在**);白名单外工具 `agent.go:366` 立即回客户端;并行 goroutine `agent.go:292`;经 mark3labs/mcp-go 自行拨 MCP transport(`exec.go`)。**流式不支持**:`bifrost.go:871`/`:973` 流式路径直接 `handleStreamRequest` 无 agent 分支,仅 `:5979-5981` 注入工具定义。**无内建搜索后端**:全模块 grep `tavily\|serper\|duckduckgo\|searxng\|brave_search\|api.exa.ai\|customsearch.googleapis\|api.bing.microsoft` **零命中**。**唯一跨 provider 归一化 `web_search`**:`schemas/responses.go:2987` `normalizeResponsesToolType()`(`"web_search_20250305"` → `"web_search"`),`providers/gemini/responses.go:4192` 映到 `googleSearch`。计费:`MergeBifrostLLMUsage` 跨轮累加 + `ResponsesServerSideToolUsageDetails{WebSearchCalls}` + `NormalizeProviderCost()` |
| **open-webui**(客户端对照) | `backend/open_webui/retrieval/web/`(searxng.py、tavily.py … + `main.py get_filtered_results`) | 应用侧「搜索 + 塞 prompt」= 与 Higress/Portkey 同模式,但执行在聊天应用而非网关 |
| **LangDB** | `docs.langdb.ai/api-reference/create-chat-completion/`、`/getting-started/working-with-mcps/`、`/guides/using-llms/connecting-llms-to-the-web-with-real-time-search-tools/`、`/features/mcp-support/` | **真实执行**:裸 HTTP `"mcp_servers":[{"name":"websearch","type":"in-memory"}]`,客户端发一次拿最终答案、**从不收到 tool_calls**;官方 "MCP tools are treated just like normal function calls **inside LangDB**";可接外部 MCP(如 Smithery Exa)经 `extra_body`;`usage.cost` 字段 |
| **Helicone** | `docs.helicone.ai/references/availability`;`github.com/Helicone/ai-gateway` README + `src/` 模块树;`src/endpoints/openai/chat_completions.rs`、`src/endpoints/mappings.rs` | **透传**:官方 "Selective Business Logic ... **By default, we simply proxy your LLM requests directly to the provider**";`src/` 无 mcp/tools/agent 模块(对照 Bifrost 有 `core/mcp/{agent,exec,toolmanager}.go`);`type RequestBody = CreateChatCompletionRequest`(直接复用 async_openai 类型),`tools` 为不透明透传字段,`mappings.rs` 仅做端点形状映射无工具类型翻译 |

### 6.2 官方文档(WebFetch / curl,已验证)

| URL | 验证内容 |
|-----|---------|
| `https://openrouter.ai/docs/llms-full.txt` | 官方机器可读文档全量 dump(4.2 MB),本节所有 OpenRouter 结论的一手来源 |
| `https://openrouter.ai/docs/guides/features/plugins` | **plugin 与 `:online` 已弃用**;`Plugin` 类型 |
| `https://openrouter.ai/docs/guides/features/plugins/web-search` | plugin 请求 JSON、默认 `search_prompt`、Chat annotations 形状、**Exa 为后端 + highlights ~2000-4000 字符** |
| `https://openrouter.ai/docs/guides/features/server-tools/web-search` | `openrouter:web_search` 形态、按引擎 per-request 计价表、`search_context_size` → 5000/15000/30000 字符、`usage.server_tool_use.web_search_requests` |
| `https://openrouter.ai/docs/api_reference/responses/web-search` | Responses annotations **扁平无 title**、流式从 `response.completed` 读 annotations、plugin 参数表 |
| `https://openrouter.ai/docs/api_reference/overview` | `cost_details.server_tool_cost`、`server_tool_use` 类型;`StreamingChoice` 无 annotations 字段 |
| `https://openrouter.ai/docs/api_reference/parameters` | `web_search_options` 为顶层 map 参数 |
| `https://openrouter.ai/docs/guides/routing/model-variants/online` | `:online` 语义(与 plugin 页矛盾)、**客户端 `web_search` 工具自动 hoist** |
| `https://openrouter.ai/docs/guides/features/logs` | Logs UI 成本明细含 web search 单列 |
| `https://openrouter.ai/docs/guides/features/broadcast` | 导出字段 `web_search_engine` / `num_search_results` / `native_server_tool_use.*` |
| `https://openrouter.ai/docs/guides/overview/multimodal/pdfs` | `file-parser` plugin(对照:annotations 是按 `type` 多态的数组) |
| `https://developers.openai.com/api/docs/guides/tools-web-search?api-mode=chat` | Chat `web_search_options` 形状(`user_location` **双层嵌套**)、`search_context_size` 语义、**`gpt-5-search-api` 取代已弃用的 `gpt-4o-search-preview`(2026-07-23 停服)**、嵌套 `url_citation` 响应示例 |
| `https://developers.openai.com/api/docs/guides/tools-web-search?api-mode=responses` | **canonical 名为 `web_search`**(`web_search_preview` 为 legacy 且不支持 `filters`/`external_web_access`/`return_token_budget`)、`web_search_call` 的 `ws_` 前缀 + `action` 对象、**扁平 annotations**、`user_location` 在此为扁平 |
| `https://developers.openai.com/api/reference/resources/responses/streaming-events` | 五个 SSE 事件名逐字验证 + 各事件四字段 payload |
| `https://developers.openai.com/api/docs/pricing` | **web search $10/1k(canonical)**;$25/1k 仅限 `web_search_preview`+非推理模型(换来内容 token 免费);`gpt-4o-mini`/`gpt-4.1-mini` 按固定 8000 input tokens/call |
| `https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool` | 三个版本字符串、完整请求/响应/usage/错误/citations/流式示例、**$10 per 1,000 searches**、**`encrypted_content` 必须逐字节原样回传否则 400**、错误时 `content` 退化为单对象、6 个错误码、`cited_text` 上限 150 字符且不计 token、`pause_turn` 状态机、`max_uses` 无默认值 |
| `https://docs.perplexity.ai/api-reference/search-post` | litellm `/search` 端点对齐的规范(经 litellm 源码注释引用) |

### 6.3 `未验证` 清单

| # | 未验证项 | 影响 |
|---|---------|------|
| 1 | OpenRouter「$4 per 1000 results」—— 现行文档无此费率,现为 per-request($0.007 Exa auto) | 任务描述中的前提疑为过期 |
| 2 | OpenRouter `:online` 是否会把模型换成 `openrouter/auto`(两页文档自相矛盾) | 不影响 aigw 设计 |
| 3 | OpenRouter **Chat Completions SSE** 的 annotation 下发方式(无 `delta.annotations` 任何示例) | aigw 若要在 chat 流式回传 citations,无业界参照 |
| 4 | OpenRouter web search 费用是否落在 `cost_details.server_tool_cost` 还是并入 `cost` | 不影响 aigw 设计 |
| 5 | `GET /api/v1/generation` 的确切字段表(页面超时,无公开 OpenAPI) | 不影响 aigw 设计 |
| 6 | litellm `/v1/chat/completions` 路径的流式与拦截交互行为 | 若 aigw 要在 chat 路径做 loop,需自行设计 |
| 7 | **上游 MaaS 是否支持网关注入的 function 工具往返**(设计 B 硬前提) | **阻塞设计 B 选型,须实测**(见 §5.8) |
| 8 | **上游 MaaS 是否接受历史里的 `server_tool_use` 块** | 决定 §5.7 剥离范围,须实测 |
| 9 | ~~LangDB / Helicone 的策略~~ | ✅ **已验证**(LangDB=真实执行 / Helicone=透传,见 §1.3) |
| 9a | LangDB `mcp_servers[].type` 的 `"in-memory"` 取值 | **搜索指南中已验证,但 API reference 的 schema enum 只列 `[ws, sse]`**——内建变体文档不全 |
| 9b | Higress / Portkey / Vercel / Bifrost / LangDB 的**计费落库形态** | 均无独立搜索费用记录的证据,标 `未验证`;Vercel 有明码价格但落库形态未验证 |
| 9c | Vercel 裸 HTTP `vercel:*` server tool 与 LangDB MCP loop 是否支持 `stream:true` | 两家文档均未述。**对照:Bifrost Agent Mode 明确不支持**——提示「网关内 loop + 流式」是业界普遍难点 |
| 9d | §1.3 其他网关的源码行号 | 取证经浏览器读 innerText(环境限制,见 §6.1b),**仅文件路径可引,行号不可引** |
| 9e | Helicone 仓库「全文无 web_search」 | GitHub code search 需鉴权,结论由**模块树 + README + 文档**推断,**非穷尽 grep** |
| 10 | ~~Anthropic / OpenAI 官方价格与线格式~~ | ✅ **已验证**(§4 全节直读官方文档,并与 litellm 价格表 / new-api 价格表三方交叉一致;Anthropic $10/1k 三方确认) |
| 11 | §4.5 的 LangGraph / LangChain / LlamaIndex / OpenAI Agents SDK / Tavily / vLLM **全部数值**(取数被网络阻断,来自模型训练知识) | 仅作数量级参照;**LlamaIndex 的 10 vs 20 有分歧,用前必须复核**。不影响 aigw 设计(loop 上限已定锚 litellm 的 3) |
| 12 | OpenAI Responses 把 `response.output_item.added` 与三个 `web_search_call.*` 交织的完整有序 trace(官方未给 worked example) | aigw 若做 Responses 保真,事件顺序需实测确认 |
| 13 | Anthropic 新版本 `web_search_20260209` / `20260318` 的具体线格式差异(仅验证了版本字符串与 `allowed_callers` 默认值翻转) | 若 aigw 对接新版本需补调研 |
