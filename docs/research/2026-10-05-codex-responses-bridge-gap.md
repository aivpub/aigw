# Codex → Responses→Chat 桥接兼容性缺口 — 行业调研

> 调研日期:2026-10-05 | 用途:Phase 52（Stage 131）设计输入
> 触发:Codex CLI 0.157.1 接 aigw `/v1/responses` 全量报错
> 参考仓库(全部直接读源码,非第三方转述):
> - litellm `~/works/projects/github.com/kofj/litellm` @ `168a0055a2`
> - new-api `~/works/projects/github.com/kofj/new-api` @ `a63364d1`
> - sub2api `~/works/play/sub2api` @ `f8f0f07f6`
> - aigw `~/works/projects/github.com/aivpub/aigw` @ `d712732`
> 外部来源:OpenAI 官方文档(WebFetch,见文末引用)
> 实测环境:aigw `http://9.135.87.221:4001`(生产网关)+ Codex CLI 0.157.1 / 0.92.0

---

## 0. 一句话结论

**aigw 的 Responses→Chat 桥接对工具是「硬拒绝」,而 litellm / new-api / sub2api 三家全部是「转换或降级」——aigw 是四家里唯一一个遇到 Codex 原生工具就 400 的。** 缺口共 4 处,其中 3 处会让 Codex 完全不可用,1 处是既存路径的真实上游 400(被 mock BDD 掩盖)。

---

## 1. 现象

Codex CLI 0.157.1 配置 `wire_api = "responses"` 指向 aigw:

```
ERROR: {"error":{"message":"Unsupported: tool type 'namespace' is not supported in
Responses→Chat bridge. Only 'function' tools are supported.","type":"invalid_request_error"}}
```

抓包(Codex 发往 aigw 的 `/v1/responses` 请求体,`tools` 数组):

| # | type | name |
|---|------|------|
| 0-3 | `function` | `exec_command` / `write_stdin` / `request_user_input` / `view_image` |
| 4 | **`namespace`** | **`multi_agent_v1`**(内含 5 个嵌套 function:`close_agent`/`resume_agent`/`send_input`/`spawn_agent`/`wait_agent`) |
| 5-7 | `function` | `get_goal` / `create_goal` / `update_goal` |
| 8 | **`web_search`** | — |

`input` 数组(消息):

```
[ {type:"message", role:"system",     content:[{type:"input_text", text:"<20KB>"}]},
  {type:"message", role:"developer",  content:[{type:"input_text", text:"<permissions 指令>"}]},
  {type:"message", role:"user",       content:[{type:"input_text", ...}]}, ... ]
```

---

## 2. 缺口 A:非 function 工具被硬拒绝

### 2.1 aigw 现状

`crates/aigw-core/src/adapter.rs:2005-2019`(`ResponsesToChatCompletions::adapt_request`):

```rust
// 1. Validate tools — only function type is supported
if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
    for tool in tools {
        let tool_type = tool.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match tool_type {
            "function" => {} // allowed
            other => {
                return Err(AdapterError::Unsupported(format!(
                    "tool type '{}' is not supported in Responses→Chat bridge. Only 'function' tools are supported.",
                    other
                )));
            }
        }
    }
}
```

**遇到任何非 function 工具直接 400 整请求失败。** BDD 还把这条行为固化为验收标准(`responses.feature:93` "web_search tool rejected"、`:110` "code_interpreter tool rejected")。

### 2.2 litellm(`litellm/responses/litellm_completion_transformation/transformation.py:1851-1912`)

逐类型分派,无一失败路径:

```python
def _responses_tool_to_chat_form(tool):
    tool_type = tool.get("type")
    if tool_type == "mcp":                       # → 原样透传
        return ResponsesToolChatForm(chat_tools=(cast(OpenAIMcpServerTool, tool),), ...)
    if tool_type in ("web_search_preview", "web_search"):   # → 派生成 web_search_options 参数
        return ResponsesToolChatForm(chat_tools=(), web_search_options=OpenAIWebSearchOptions(...))
    if tool_type == "function":                  # → 扁平转嵌套
        chat_completion_tool = {
            "type": "function",
            "function": {"name": ..., "description": ..., "parameters": ..., "strict": ...},
        }
        ...
    if tool_type == "namespace":                 # → 拍平子工具为 "{ns}__{name}"
        return ResponsesToolChatForm(chat_tools=..._namespace_chat_tools(tool), ...)
    if tool_type == "custom":                    # → 转 function
        converted = convert_custom_tool_to_function_tool(tool)
        return ResponsesToolChatForm(chat_tools=() if converted is None else (converted,), ...)
    if tool_type in ("computer_use", "image_generation", "shell"):
        verbose_logger.warning(                    # → 丢弃 + 告警
            "Dropping Responses API tool of type '%s': it has no Chat Completions "
            "equivalent and the target provider would reject the request.", tool_type)
        return ResponsesToolChatForm(chat_tools=(), web_search_options=None)
    return ResponsesToolChatForm(chat_tools=(cast(ChatToolParam, tool),), ...)  # → 兜底透传
```

**四种处置:转嵌套 / 派生参数 / 拍平 / 丢弃带告警 / 兜底透传。零 400。**

namespace 拍平实现(`:1805-1827`):

```python
def _namespace_chat_tools(tool):
    namespace = str(tool.get("name") or "")
    namespace_tools = tool.get("tools")
    if isinstance(namespace_tools, Sequence) ...:
        return tuple(
            chat_tool for raw_tool in namespace_tools
            if (chat_tool := ..._build_ns_chat_tool(namespace, namespace_description, raw_tool, True)) is not None
        )
```
其中 `chat_tool_name = f"{namespace}__{tool_name}"`(`:1790`),并有重名冲突检测 `_validate_namespace_name_collisions`(`:1830-1848`)——顶层 function 名与拍平名冲突时**抛 ValueError**(而非静默覆盖)。

### 2.3 new-api(`service/relayconvert/internal/oai_responses/to_oai_chat_req.go:321-357`)

```go
for _, tool := range tools {
    toolType := strings.TrimSpace(common.Interface2String(tool["type"]))
    if toolType == "function" {
        out = append(out, dto.ToolCallRequest{          // → 扁平转嵌套
            Type: "function",
            Function: dto.FunctionRequest{
                Name:        strings.TrimSpace(common.Interface2String(tool["name"])),
                Description: common.Interface2String(tool["description"]),
                Parameters:  tool["parameters"],
            },
        })
        continue
    }
    rawTool, err := common.Marshal(tool)
    if err != nil { return nil, err }
    out = append(out, dto.ToolCallRequest{              // → 其余类型原样透传为 Custom
        Type:   toolType,
        Custom: rawTool,
    })
}
```
new-api **不丢不拒,一律透传**,把「认不认」交给上游。

### 2.4 sub2api(`backend/internal/pkg/apicompat/chatcompletions_responses_bridge.go:800-860`)

最接近 aigw 的目标形态(同为 Codex 场景打磨过):

```go
case "custom":
    // codex 0.14x 的核心执行工具 exec 即为 custom 类型;丢弃它会让模型
    // 无法执行任何命令,必须降级为 function 工具透传。
    out = append(out, ChatTool{Type: "function", Function: &ChatFunction{
        Name: tool.Name, Description: tool.Description,
        Parameters: json.RawMessage(customToolInputSchema)}})
case "tool_search":
    // 代理不能改名(codex 的模型侧按 tool_search 这个名字调用)...
    out = append(out, toolSearchProxyChatTool())
case "namespace":
    flattened, err := namespaceChildrenToChatTools(tool, topLevel, flatOwner)  // → 拍平
    if err != nil { return nil, err }
    out = append(out, flattened...)
}
// 其余类型(web_search、image_generation 等服务端工具)在 chat 上游没有
// 对应能力,维持丢弃。
```

配套:`responsesToolChoiceToChatToolChoice(raw, declared)`(`:944`)——**丢弃指向已丢工具的 `tool_choice`**,注释明确「指向未声明工具的 tool_choice 会被 chat 上游 400 拒绝」。命名同样用 `"<namespace>__"` 前缀,冲突**报错**而非静默覆盖(`responses_namespace.go:67-75`)。

### 2.5 横向对比

| 仓库 | 非 function 工具 | namespace | 冲突处理 | 失败路径 |
|------|-----------------|-----------|---------|---------|
| **aigw** | **400 整请求失败** | **400** | — | 有 |
| litellm | 派生/丢弃带告警/透传 | 拍平 `ns__name` | ValueError | 仅重名 |
| new-api | 透传(Custom) | 透传 | — | 仅 marshal 失败 |
| sub2api | 降级/代理/丢弃 | 拍平 `ns__name` | error | 仅重名 |

---

## 3. 缺口 B:`role: "developer"` 未处理

### 3.1 OpenAI 规范(官方文档)

> "`developer` messages are instructions provided by the application developer, prioritized ahead of user messages."
> "using the `instructions` parameter is 'roughly equivalent' to passing a `developer` role message in the input array"

即:**`developer` 与 `instructions` 语义等价**(aigw 已支持 `instructions` → 首位 system,`adapter.rs:2028-2035`)。也就是说 codex 的 `developer` 消息与 aigw 已处理的 `instructions` 是同一类东西,只是入口不同。

### 3.2 三家参考实现**完全一致**:developer → system 纯重命名

**litellm**(`litellm/llms/base_llm/base_utils.py:207-224` + 调用点 `litellm/main.py:5389`):

```python
def map_developer_role_to_system_role(messages):
    """Translate `developer` role to `system` role for non-OpenAI providers."""
    new_messages = []
    for m in messages:
        if m["role"] == "developer":
            verbose_logger.debug("Translating developer role to system role for non-OpenAI providers.")
            new_messages.append({"role": "system", "content": m["content"]})
        else:
            new_messages.append(m)
    return new_messages
```
调用点(`main.py:5381-5389`)对所有 `custom_llm_provider in LlmProviders` 生效;`translate_developer_role_to_system_role` 被 OpenAI/Azure 覆写——**即非 OpenAI 系 provider 一律降级为 system**。

**new-api**——把二者当同一个 role 分支处理(`to_oai_responses_req.go:129`、`to_claude_messages_req.go:301`、`to_gemini_chat_req.go:297`):

```go
if role == "system" || role == "developer" {   // 归入 instructions
case "system", "developer":                    // → system
```

**sub2api**(`chatcompletions_responses_bridge.go:689-697`):

```go
func chatCompletionsBridgeRole(role string) string {
    trimmed := strings.TrimSpace(role)
    if trimmed == "" { return "user" }
    if strings.EqualFold(trimmed, "developer") {
        return "system"
    }
    return role
}
```

**结论:developer→system 是行业一致做法,无分歧。**

### 3.3 aigw 现状:零处理

`grep -rn '"developer"' crates/**/*.rs` **无任何命中**。`ChatMessage.role` 是无约束 `String`(`crates/aigw-core/src/models.rs:574`),`developer` 反序列化合法,`ResponsesToChatCompletions` 直接透传(`adapter.rs:1977` 原样拷贝 role)。上游 MaaS 不认 → 400。

---

## 4. 缺口 C:developer→system 会触发 Qwen 严格模板(必须走折叠)

### 4.1 关键交互

Codex 的消息形状是 **`[system(instructions), developer, user...]`**。若对 developer 做**朴素重命名**,输出变成 **`[system, system, user...]`**——两个 system,第二个在 index 1。

实测(生产网关 `9.135.87.221:4001`)的后果差异:

```
两个 system 开头          -> 200     ← deepseek 系:看不出来
system 在 user 之后       -> 200     ← deepseek 系:看不出来
developer 在开头(单条)     -> 400     ← 当前 bug
developer 在 user 之后     -> 400
```

**在 deepseek/glm 上游,多 system 不报错,所以这个坑在本网关上「测不出来」。** 但 Qwen 系严格 Jinja 模板要求 `role="system"` **必须且仅能**位于 index 0,否则模板 parser 抛异常 —— aigw 已为此实现过专门机制(Phase 21 / Stage 60),见 `docs/plans/2026-07-16-system-message-normalization.md`:

> Qwen 系列(qwen2 / qwen2.5 / qwen3 / qwen3.5 / qwen-max)的官方 Jinja chat template 强制:
> 若 `messages` 出现 `role="system"`,必须**且仅**位于 index 0,否则模板 parser 生成阶段抛异常。

### 4.2 aigw 已有机制但**未接入 Responses 桥接**

`crates/aigw-core/src/adapter.rs:305-330`(`ChatTemplateCompat` + auto-sniff by model name contains "qwen")与 `:345-394`(`fold_extra_systems_into_adjacent_user`,post-condition "role='system' only appears at index 0",尾随 reminder 追加到末条 user 或合成新 user turn,并有 `debug_assert` 兜底)。

**但折叠只在 `AnthropicToOpenAI::adapt_request` 被调用**(`adapter.rs:170-183`):

```rust
// Stage 60: System message normalization for strict chat templates
let compat = resolve_chat_template_compat(deployment);
let oai_req = match compat {
    ChatTemplateCompat::Strict => {
        let messages = fold_extra_systems_into_adjacent_user(oai_req.messages);
        ChatCompletionRequest { messages, ..oai_req }
    }
    ChatTemplateCompat::Loose => oai_req,
    ...
};
```

**`ResponsesToChatCompletions::adapt_request`(`adapter.rs:1995-2066`)完全没有接入**——它只在 `:2028-2035` 把 `instructions` 插成 index 0 system,之后原样透传 role。

对应地,new-api 有 `ensureClaudeMessagesStartWithUser`(`to_claude_messages_req.go:304`),litellm 有 `map_system_message_pt`(`factory.py:92-122`,针对 `supports_system_message=False`)——**两家参考实现都在转换侧做了「不能让 system 乱跑」的归一化**。

---

## 5. 缺口 D:扁平 function 工具从未转成嵌套格式(既存路径的真实 400)

### 5.1 实测

Responses 规范的 function 工具是**扁平**的;Chat Completions 要**嵌套**。生产网关实测:

```
扁平 {"type":"function","name":"foo","description":"x","parameters":{...}}
  -> 400  The request parameter tools[0].function.name is invalid or missing.

嵌套 {"type":"function","function":{"name":"foo",...}}
  -> 200
```

### 5.2 aigw 现状

`adapt_request` **只校验、不转换**——`"function" => {}` 分支直接放行,工具对象原样带进上游 chat 请求。**即 aigw 目前「允许」的 function 工具,打到真实上游同样会 400。**

这个缺陷被 mock BDD 掩盖:`responses.feature:85` "bridge with function tools" 断言 200,但 mock upstream 不校验 tool 形状。**litellm(`:1876-1896`)/ new-api(`:334-343`)/ sub2api 三家都在此处显式构建嵌套结构。**

---

## 6. 缺口 E:`input` 的 content part 类型 `input_text` 未转换

Respondes `input[]` 的 content part 用 `{type:"input_text"}`;Chat Completions 要 `{type:"text"}`。实测:

```
content=[{"type":"input_text","text":"hi"}]  -> 400
content=[{"type":"text","text":"hi"}]        -> 200
```

aigw `input_to_messages`(`adapter.rs:1974-1980`)对 `content` 做 `item.get("content").cloned()` 原样拷贝,不做 part 级映射 → 400。

另注:`input[]` 元素带 `type` 字段(`"message"` / `"function_call"` / `"function_call_output"`),aigw **完全忽略 `type`**,一律当 message 处理。单轮 Codex 不触发;多轮(tool 调用回填)会走偏。**建议本 Stage 只做 part 映射,item type 分派单独立项**(见 §8)。

---

## 7. 旁证:同一网关的 chat 路径也不行(排除「改用 chat completions」方案)

曾评估「让 Codex 降级到 `wire_api = "chat"` 直连 `/v1/chat/completions`,绕开 bridge」。实测 **Codex 0.92.0**(最后一个仍支持 chat wire 的版本,`codex-cli 0.57`+ 起 chat 已废弃、0.157.1 完全移除;OpenAI 官方 [discussion #7782](https://github.com/openai/codex/discussions/7782) 确认 2026-02 移除):

```
msg[0] role=system     -> 200
msg[1] role=developer  -> 400   ← 同一个拦路虎
msg[2] role=user       -> 200
```

**`developer` role 在 chat 路径同样 400。** 即:改用 chat completions 并不能省掉网关侧修复,反而多欠一笔 Codex 降级债(跨 65 个版本)。**故修复点必须在网关。**

---

## 8. 结论与设计取向

### 8.1 必做(阻塞 Codex 可用)

| # | 缺口 | 取向 | 对齐依据 |
|---|------|------|---------|
| A | namespace 工具 | 拍平为 `{ns}__{child}` function;重名报错 | litellm / sub2api 一致 |
| A | 服务端工具(web_search/search/code_interpreter/computer_use/image_generation/shell) | **丢弃 + 日志告警**;同步清理指向已丢工具的 `tool_choice` | litellm / sub2api 一致 |
| B | `developer` role | → `system` | 三家一致 |
| C | 多 system | **接入既有 `fold_extra_systems_into_adjacent_user`**(Strict 时) | aigw 既有机制 + Phase 21 决策 |
| D | 扁平 function | 转嵌套 `{type:function, function:{...}}` | 三家一致 |
| E | `input_text` part | → `{"type":"text"}` | 三家一致 |

### 8.2 关键设计取舍

1. **丢弃 vs 透传 vs 报错**:选**丢弃带告警**(litellm/sub2api 路线)。理由:这些工具在 chat 上游无对应能力,透传必然被上游 400(new-api 的透传路线在本环境会把失败推给上游,用户体验更差),报错则直接废掉 Codex。
2. **developer 处置**:选**重命名 + 走 fold**,而非「重命名 + 合并进首位 system」。理由:codex 的 developer 是**权限/沙箱策略**指令(高优先级),合并进首位 system 保真度更高;但若为省事走 fold,内容会进 user turn 的 `<system-reminder>`——**降权**。**建议合并进首位 system(`instructions` 语义等价),避免降权。** 此点为设计决策待确认项。
3. **是否复用 `ChatTemplateCompat`**:建议**复用**(auto-sniff qwen),不新增机制。但需注意:即使 Loose 模式,「两个 system」在部分上游仍是风险——**建议对 developer 合并进首位 system(而非新增 system),从根上不产生第二个 system**,与 compat 模式解耦。
4. **BC 破坏**:`responses.feature:93/110` 两条「non-function tool rejected」场景需**改写**为「丢弃 + 后续请求成功」。BDD 基线会变。

### 8.3 单独立项(本 Stage 不做)

- `input[]` 的 `item.type` 分派(`function_call` / `function_call_output` / `reasoning` …)——多轮 Codex 必需,但超出「单轮可用」目标,建议 TD 登记后独立 Stage。
- `tool_choice` 中 `{type:"namespace"}` 形态的降级(Codex 也会发 `tool_choice: {type:"function", name, namespace}`)。

---

## 引用

- OpenAI Responses API tools(`namespace` 为规范 tool type,Go SDK `ToolParamOfNamespace`):https://developers.openai.com/api/docs/guides/tools
- OpenAI `developer` role 语义(与 `instructions` 等价):https://developers.openai.com/api/docs/guides/text
- Codex `wire_api = "chat"` 移除讨论:https://github.com/openai/codex/discussions/7782
- aigw 既有决策:`docs/plans/2026-07-16-system-message-normalization.md`(Phase 21 / Stage 60 折叠算法与取舍)
