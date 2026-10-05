# 业界网关 web_search / 服务端工具支持调研 — 后续规划参考

> 调研日期:2026-10-05 | 用途:Phase 52（Stage 131）决策输入 + 后续「内建搜索」Phase 规划参考
> 方法:直接读源码(litellm / sub2api / new-api) + 生产网关 `9.135.87.221:4001` 实测
> 关联:`docs/research/2026-10-05-codex-responses-bridge-gap.md`(同批调研,桥接兼容性)

---

## 0. 一句话结论

**三家参考网关对服务端工具(web_search 等)有三种不同策略,但只有 litellm 真正「内建执行搜索」——那是一个带搜索后端 + agentic loop 的独立子系统,不是桥接转换的一环。sub2api 与 new-api 都只是「转换」,不执行。** aigw 当前上游 MaaS 既不能透传服务端工具(实测 400),也不能让模型真正联网(实测 `web_search_options` 被忽略),故任何「转换路线」在本环境都只能得到「静默失去搜索能力」的结果。

---

## 1. 三家策略对照

| 网关 | `web_search` / `web_search_preview` | `code_interpreter` | 其他(`computer_use` / `image_generation` / `shell`) |
|------|-----------------------------------|-------------------|------------------------------------------------|
| **litellm** | **派生**为 `web_search_options` 顶层参数(条件性) | **透传** | **丢弃** + 告警 |
| **sub2api** | **丢弃** | **丢弃** | **丢弃** |
| **new-api** | **透传**(转 `Custom` 原始 JSON) | **透传** | **透传** |
| **aigw(现状)** | **400 拒绝** | **400 拒绝** | **400 拒绝** |

### 1.1 litellm — 唯一「真实支持」的一家

`litellm/responses/litellm_completion_transformation/transformation.py:1855-1865`:

```python
if tool_type == "web_search_preview" or tool_type == "web_search":
    return ResponsesToolChatForm(
        chat_tools=(),                       # 不进 tools
        web_search_options=OpenAIWebSearchOptions(
            search_context_size=..., user_location=...,
        ),                                    # 派生为顶层参数
    )
```

**条件性丢弃**(`:271-286`,`_should_drop_derived_web_search_options`):

```python
"""A Responses ``web_search`` built-in tool is derived into a ``web_search_options`` param.
When the resolved provider/model does not support it (e.g. Bedrock Anthropic, where only
Nova maps it to a nova_grounding systemTool), the derived param is dropped here instead of
raising UnsupportedParamsError downstream. Providers that support it keep it untouched."""
supported_params = get_supported_openai_params(model=model, custom_llm_provider=...)
return supported_params is not None and "web_search_options" not in supported_params
```

即:**按目标 provider 的 `get_supported_openai_params` 决定派生还是丢弃**——这是 litellm 的 provider 能力注册表在起作用。

**drop 列表精确是 `("computer_use", "image_generation", "shell")`**(`:1905`);`code_interpreter` **不在** drop 列表(`grep -c '"code_interpreter"'` 该文件 **0 命中**),也**无专门分支**,落最后兜底 `return ResponsesToolChatForm(chat_tools=(cast(ChatToolParam, tool),))` = 原样透传。

### 1.2 litellm 的真实搜索执行 — 独立子系统

**关键区分**:上面 §1.1 只是「协议转换」;litellm 另有一套**真正执行搜索**的机制,与本 Stage 目标完全不同:

`litellm/types/integrations/websearch_interception.py`:

```python
"""
Configuration parameters for WebSearchInterceptionLogger.
    litellm_settings:
      websearch_interception_params:
        enabled_providers: ["bedrock"]           # 哪些上游需要拦截
        search_tool_name: "my-perplexity-search"
        max_agentic_loops: 5                     # 一次请求内允许几轮后续模型调用
"""
```

配套组件:

| 组件 | 位置 | 作用 |
|------|------|------|
| 搜索后端 provider | `litellm/llms/tavily/search`、`llms/perplexity/search`、`llms/searchapi` | 实际发起搜索 |
| 搜索工具注册表 | `litellm/proxy/search_endpoints/search_tool_registry.py` | 管理已配置的搜索工具 |
| 搜索工具管理 API | `litellm/proxy/search_endpoints/search_tool_management.py` | CRUD |
| 拦截 hook | `WebSearchInterceptionLogger` | 拦截 → 执行 → 回灌 |
| agentic loop 上限 | `litellm/proxy/hooks/max_iterations_limiter.py` | 限流防失控 |
| Anthropic 侧类型 | `AnthropicServerToolUseBlock`(`server_tool_use` + `web_search_tool_result` 成对、共享 `srvtoolu_` 前缀 id) | 回程构造 |

**本质**:把服务端工具**拦截下来,网关自己执行搜索,把结果喂回模型,再要一轮**——一个 agentic loop。这与 aigw 现有「单发代理」架构(request → upstream → response)有本质差异。

### 1.3 sub2api — 丢弃

`backend/internal/pkg/apicompat/chatcompletions_responses_bridge.go:851-853`:

```go
		}
		// 其余类型（web_search、image_generation 等服务端工具）在 chat 上游没有
		// 对应能力，维持丢弃。
	}
	return out, nil
```

switch 只有 `function` / `custom` / `tool_search` / `namespace` 四个 case,**无 `web_search` 分支** → 静默丢弃(无日志)。

`tool_choice` 同步丢弃(`:944`):
> 服务端工具（web_search 等）的选择项随工具本身丢弃——指向未声明工具的 tool_choice 会被 chat 上游 400 拒绝。

### 1.4 new-api — 透传

`service/relayconvert/internal/oai_responses/to_oai_chat_req.go:345-356`:

```go
		rawTool, err := common.Marshal(tool)
		if err != nil { return nil, err }
		out = append(out, dto.ToolCallRequest{
			Type:   toolType,
			Custom: rawTool,
		})
```

非 `function` 一律透传为 `Custom`,把「认不认」交给上游。

**注**:new-api 另有 `web_search_options` 作为**顶层参数**的处理(`dto/openai_request.go:86` + `relay/channel/openai/adaptor.go`),但那是 chat 协议原生字段,**与 Responses `tools[]` 里的 `web_search` 是两条独立路径**——后者仍走 Custom 透传。

---

## 2. 生产网关实测(9.135.87.221:4001)

### 2.1 工具形态

```
tools=[{type:"web_search"}]            -> 400
tools=[{type:"web_search_preview"}]    -> 400
tools=[{type:"code_interpreter",...}]  -> 400
tools=[{type:"computer_use_preview"}]  -> 400
tools=[{type:"mcp",...}]               -> 400
```

**透传路线(new-api / litellm 兜底)在本上游直接 400**——不可用。

### 2.2 参数形态

```
web_search_options={search_context_size:"low"}                      -> 200
web_search_options={search_context_size,user_location:{...}}         -> 200
```

**参数被接受,但未启用搜索**(下节实测)。

**→ 结论:litellm 的派生路线在本环境「能过协议、但无实效」。**

### 2.3 派生是否真的开启搜索 — 实测否定

同一联网问题对比(模型 `tokenhub/deepseek-v4-flash`):

```
无 web_search_options  -> (空响应)
带 web_search_options  -> "I can't search the live web from this environment,
                            so I can't provide today's latest news or cite a source URL."
```

**模型明确表示无法联网。** 即 `web_search_options` 在本上游是「收下即忽略」。

---

## 3. 对 aigw 的规划含义

### 3.1 三条路线在本环境的实际效果

| 路线 | 协议结果 | 真实搜索能力 | 业界对齐 | 量级 |
|------|---------|-------------|---------|------|
| 透传(原样发上游) | **400** | — | new-api / litellm 兜底 | 不可用 |
| 派生 `web_search_options` | 200 | **无**(参数被忽略) | litellm | 几行 |
| 丢弃 + 告警 | 200 | 无 | sub2api | 几行 |
| **内建执行搜索** | 200 | **有** | litellm `WebSearchInterceptionLogger` | **独立 Phase** |

**「派生」与「丢弃」在本环境实效相同**——都是静默失去搜索能力。差别仅在:派生会让上游请求里留一个骗人的参数,丢弃 + 告警会在网关日志留下明确痕迹(便于统计「有多少请求本来要搜索」,作为是否值得内建搜索的决策依据)。

### 3.2 内建搜索的可行性评估(供后续规划)

若产品决定支持,参考 litellm 的最小闭环:

| 需要 | aigw 现状 | 备注 |
|------|----------|------|
| 搜索后端(Tavily / Perplexity / SerpAPI / Bing)+ key + 计费接入 | **零** | 只有 `reqwest`,无任何搜索 provider |
| **多轮 agentic loop**(拿到 tool_call → 执行 → 回灌 → 再请求) | **零** | aigw 是单发代理,无循环 |
| 流式 SSE 与循环的交互 | 需新增 | 每轮都要处理流式,复杂度翻倍 |
| 双向工具协议转换(`server_tool_use` ↔ `tool_call` 配对) | 部分(现有 tool 转换可复用) | litellm 用 `srvtoolu_` 前缀 id 配对 |
| loop 上限 + 计费归集 | 需新增 | litellm `max_agentic_loops` |
| 工具选择策略(何时触发搜索) | 需设计 | 模型自主 vs 强制 |

**评估:这是一个完整子系统,不是桥接修复的一部分。** 参照 litellm 的组件清单(§1.2),建议作为独立 Phase 立项,而非塞进 Stage 131。

### 3.3 与既有项目边界的一致性

`docs/research/2026-08-04-openai-responses-api-support.md:292` 曾明确:

> | 内置工具(web_search, code_interpreter) | 上游原生支持,网关不实现 |

该判断在**当初**成立(假设上游原生支持);**本次实测表明本环境上游并不支持**,故若产品需要,该边界需要重新评估。

---

## 4. 引用

| 来源 | 位置 |
|------|------|
| litellm 工具分派 | `litellm/responses/litellm_completion_transformation/transformation.py:1851-1912` |
| litellm web_search 派生 | 同上 `:1855-1865` |
| litellm 派生条件丢弃 | 同上 `:271-286` |
| litellm 内建搜索拦截 | `litellm/types/integrations/websearch_interception.py` |
| litellm 搜索后端 | `litellm/llms/{tavily,perplexity,searchapi}/search` |
| sub2api 丢弃 | `backend/internal/pkg/apicompat/chatcompletions_responses_bridge.go:851-853` |
| sub2api tool_choice 清理 | 同上 `:944` |
| new-api 透传 | `service/relayconvert/internal/oai_responses/to_oai_chat_req.go:345-356` |
| new-api `web_search_options` 顶层参数 | `dto/openai_request.go:86`、`relay/channel/openai/adaptor.go` |
| OpenAI Responses tools(namespace 等) | https://developers.openai.com/api/docs/guides/tools |
