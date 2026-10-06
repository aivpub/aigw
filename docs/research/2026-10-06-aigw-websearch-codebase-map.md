# aigw 内建 web_search 工具 — 代码落地基线地图

> 调研日期: 2026-10-06
> 目标: 为「网关自己执行 web_search（调用外部搜索厂商）+ 调用日志 + 正确计费」这一规划特性，建立精确的代码落地基线。
> 方法: 只读代码审计，所有结论带 `path:line` 锚点。未执行任何 cargo 命令，未改动生产代码。
> 前置文档: `docs/research/2026-10-05-websearch-server-tool-support.md`（业界策略调研 + 本环境实测，已定案「丢弃 + 告警」，内建搜索列为独立 Phase）。

---

## 0. 一句话结论

| 维度 | 现状 |
|------|------|
| agentic loop | **完全不存在**。每个客户端请求严格单发上游（`chat.rs:1743` 只有一次 `.send()`），fallback 只是「换一个 deployment 重试同一请求」而非多轮对话。 |
| 服务端工具丢弃点 | `crates/aigw-core/src/adapter.rs:2553-2559`（`normalize_responses_tools` 的 `other =>` 分支，`tracing::warn!` 后落空）。 |
| SpendLog | 37 列单表 `spend_logs`，有 `call_type` 判别列 + `metadata` JSON 自由位，但**无 parent_call_id / 无子调用概念**。 |
| 按次计费 | **完全不存在**。全部走 token 单价；`ModalPricing` 虽名为「modal」但仍是 USD/1M **token**，且 `calc_spend_modal` 当前是 dead code。 |
| 搜索 provider 配置位 | 需在 `AigwConfig`（`config.rs:30`）新增顶层 section + `config_loader.rs` 新增装载逻辑；密钥沿用 `crypto::encrypt_litellm_value_gcm` 的 `v2:gcm:` 方案。 |
| 当前进度 | Phase 52 完成（Stage 131-132），总进度 136。新 Stage 应为 **Stage 134+**（Stage 133 已被 commit c7cd1f4 占用：Responses 原生直通）。 |

---

## A. 请求管线与工具处理

### A1. 三个端点的 axum 路由 → 上游 → 回流

#### 路由注册（全部在 `main.rs` 的单个 `Router` 链上）

| 端点 | 注册位置 | handler |
|------|---------|---------|
| `POST /v1/chat/completions` | `crates/aigw-server/src/main.rs:490-493` | `chat::chat_completions` |
| `POST /v1/messages` | `crates/aigw-server/src/main.rs:669-672` | `v1_messages::messages_handler` |
| `POST /v1/messages/count_tokens` | `crates/aigw-server/src/main.rs:676-679` | `v1_messages::count_tokens_handler` |
| `POST /v1/responses` | `crates/aigw-server/src/main.rs:680-684` | `responses::responses_handler` |

三者共享的 tower 层（`main.rs:684-719`）顺序（由内向外）：
`with_state` → `TraceLayer`（`RequestIdMakeSpan`）→ `SetRequestIdLayer`（UUID v7，`main.rs:698-701`）→ `PropagateRequestIdLayer`（回写 `x-call-id`，`main.rs:707-709`）→ CORS → 压缩 → `DefaultBodyLimit`。

> 关键：`SetRequestIdLayer` 产出的 UUID v7 就是 `SpendLog.call_id`（`main.rs:702-706` 注释明确 TD-006：`call_id == RequestId`）。**内建搜索若要新开 SpendLog 行，call_id 需要自己生成，不能复用同一个。**

#### `chat_completions` 完整请求路径（以 chat 为代表，三者结构同形）

| 步 | 内容 | 锚点 |
|----|------|------|
| 1 | 提取 `request_id`（从 `extensions` 取 `RequestId`） | `crates/aigw-server/src/routes/chat.rs:921-927` |
| 2 | OTEL traceparent 提取（noop when disabled） | `chat.rs:931-933` |
| 3 | 校验 `model` / `messages` 必填 + 非空 | `chat.rs:939-981` |
| 4 | 解析 `stream` 标志 | `chat.rs:982-985` |
| 5 | **鉴权**：`ChatAuth` extractor，先 Bearer 再 Cookie JWT | `chat.rs:601-618`（`from_request_parts`）/ `chat.rs:621`（`try_bearer_token`）/ `chat.rs:696`（`try_cookie_jwt`） |
| 6 | 模型权限检查（master key 绕过） | `chat.rs:1013` 起 |
| 7 | **deployment 解析**：`state.resolver.resolve(model)` → `Vec<Deployment>` | `chat.rs:1076` |
| 8 | 合并 key > team > global 的 `router_settings` → `effective_router` | `chat.rs:1083-1110`（`Router::from_merged`，`crates/aigw-core/src/router.rs:505`） |
| 9 | 响应缓存查询（Stage 119，HIT 则短路 + 零成本 SpendLog） | `chat.rs:1115`（`CacheControl::parse`）/ 命中分支 `chat.rs:1653-1716` |
| 10 | `fallback_order` 计算 + `pick_deployment` 选主 | `chat.rs:1137-1149`（`router.rs:494` 的 `fallback_order`） |
| 11 | **OAuth 反代分支**（`deployment.oauth` 存在时整条绕行） | `chat.rs:1152-1528` |
| 12 | **adapter 转换请求体**：`adapter.adapt_request(body, &deployment)` | `chat.rs:1534` |
| 13 | 拼上游 URL（`messages` vs `chat/completions`） | `chat.rs:1542-1551` |
| 14 | 提取 `end_user` / `session_id` / `requester_ip` / `user_agent` / `device_id` → `metadata` | `chat.rs:1553-1596` |
| 15 | **构建 HTTP client**：`state.router.build_retry_client()` | `chat.rs:1601`（实现在 `crates/aigw-core/src/router.rs:535-548`） |
| 16 | `max_parallel` 信号量许可 | `chat.rs:1609-1615` |
| 17 | 注入 auth header（`x-api-key`+`anthropic-version` 或 `Bearer`）+ `x-request-id` | `chat.rs:1617-1627` |
| 18 | **发送上游（唯一一次）** | `chat.rs:1743` |
| 19 | `report_success` / `report_failure` 更新 cooldown + 延迟 EWMA | `chat.rs:1848-1858` |
| 20a | 流式：Phase 1 占位 SpendLog INSERT → `tokio::spawn` 转发 + 收集 → Phase 2 UPDATE | `chat.rs:1987-2025`（INSERT）/ `chat.rs:2027-2046`（spawn）/ `chat.rs:2241`、`chat.rs:2313`（UPDATE） |
| 20b | 非流式：`upstream_resp.json()` → `calc_spend` → 单次 INSERT | `chat.rs:2443`（解析）/ `chat.rs:2570`（`calc_spend`）/ `chat.rs:2666`（INSERT） |

`responses_handler` 同形，锚点：`crates/aigw-server/src/routes/responses.rs:83`（入口）、`responses.rs:157`（stream 标志）、`responses.rs:227`（resolve）、`responses.rs:611`（`adapt_request`）、`responses.rs:632-647`（上游 path/url）、`responses.rs:714`（`client.post`）。

`messages_handler` 入口与常量：`crates/aigw-server/src/routes/v1_messages.rs`（文件头 `v1_messages.rs:1-6` 说明「协议转换到 OpenAI 上游经 adapter 层」）。

### A2. Responses→Chat adapter 的工具归一化 —— 服务端工具的精确丢弃点

**函数**：`ResponsesToChatCompletions::normalize_responses_tools`
**声明**：`crates/aigw-core/src/adapter.rs:2447-2449`
**doc comment（Stage 131 加入）**：`crates/aigw-core/src/adapter.rs:2435-2446`

```rust
    /// Normalize the Responses `tools` array into Chat Completions tools.
    ///
    /// Returns the converted tools plus the set of declared (surviving) tool
    /// names, which callers use to drop dangling `tool_choice` references.
    ///
    /// Sub2api/litellm-compatible behaviour:
    /// - `function`  → nested `{type, function:{...}}`
    /// - `namespace` → children flattened to `{ns}__{child}` functions
    /// - `custom`    → degraded to a single-string-input function
    /// - `tool_search` → same-named function proxy (cannot be renamed)
    /// - server-side tools (`web_search`, `code_interpreter`, `mcp`, ...) → dropped
    fn normalize_responses_tools(
        tools: &[Value],
    ) -> Result<(Vec<Value>, std::collections::HashSet<String>), AdapterError> {
```

**丢弃发生在 `match tool_type` 的 fallthrough 分支** —— `crates/aigw-core/src/adapter.rs:2553-2559`：

```rust
                other => {
                    tracing::warn!(
                        tool_type = %other,
                        tool_name = %name,
                        "dropping Responses API tool: no Chat Completions equivalent"
                    );
                }
            }
```

即：`match` 只有 `"function"`（`adapter.rs:2473`）、`"namespace"`（`adapter.rs:2488`）、`"custom"`（`adapter.rs:2525`）、`"tool_search"`（`adapter.rs:2536`）四个 case，`web_search` / `web_search_preview` / `code_interpreter` / `computer_use_preview` / `image_generation` / `shell` / `mcp` 全部落入 `other =>`，warn 后**不 push 到 `out`，也不插入 `declared`**，于是：
- 工具本身消失（`adapter.rs:2563` `Ok((out, declared))`）；
- 若 `tools` 归一化后为空数组，整个 `tools` 字段被 remove（`adapter.rs:2706-2710`）；
- 指向被丢弃工具的 `tool_choice` 也被级联清理（`adapter.rs:2713-2723`，调用 `normalize_tool_choice(&choice, &declared_tools)`）。

**调用点（唯一）**：`ResponsesToChatCompletions::adapt_request` —— `crates/aigw-core/src/adapter.rs:2700-2712`：

```rust
        // 1. Normalize tools — flatten namespaces, degrade custom/tool_search,
        //    drop server-side tools. Never reject the whole request.
        let mut declared_tools = std::collections::HashSet::new();
        if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()).cloned() {
            let (normalized, declared) = Self::normalize_responses_tools(&tools)?;
            declared_tools = declared;
            if normalized.is_empty() {
                obj.remove("tools");
            } else {
                obj.insert("tools".to_string(), Value::Array(normalized));
            }
        }
```

**第二个相关丢弃点（历史侧，不是请求侧）**：`input[]` 里的服务端**调用项**（上一轮的 `web_search_call` 回放）在 `items_to_messages`（Stage 132 新增的 item 分派）同样被跳过 —— `crates/aigw-core/src/adapter.rs:2210-2216`：

```rust
                other => {
                    // Server-side call items (`web_search_call`, `local_shell_call`,
                    // `file_search_call`, ...) have no Chat equivalent. Converting
                    // them would inject a stray message between an assistant's
                    // tool_calls and its replies, which strict upstreams reject.
                    tracing::debug!(item_type = %other, "skipping Responses item with no Chat equivalent");
                    pending_reasoning.clear();
                }
```

> 落地含义：实现内建搜索时，这两处都要改。请求侧需把 `web_search` 识别为「网关自执行工具」而非丢弃；历史侧需能把上一轮 `web_search_call` + 其结果重放回 messages（否则多轮上下文丢失）。

**BDD 现有锁定（会变红的测试）**：
- `crates/aigw-server/tests/features/responses.feature:105-111`（`/v1/responses bridge web_search tool dropped`，断言「上游收到的 tools 不含 `web_search_preview`」）
- `crates/aigw-server/tests/features/responses.feature:121-127`（同形，`code_interpreter`）
- 对应 step：`crates/aigw-server/tests/bdd_steps/responses_steps.rs:381-393`、`responses_steps.rs:395-407`
- 单测锁定：`crates/aigw-core/src/adapter.rs:4188-4191`（多种服务端工具全丢）、`adapter.rs:4206`、`adapter.rs:4219-4220`（`tool_choice` 级联清理）、`adapter.rs:4374`、`adapter.rs:4565-4572`（`web_search_call` item 跳过）、`adapter.rs:4666`、`adapter.rs:4689`

### A3. Anthropic `/v1/messages` 的工具处理 —— `web_search_20250305` 会在反序列化阶段就炸

Anthropic 侧请求体被反序列化为**强类型** struct，而非 `serde_json::Value`：

`crates/aigw-core/src/models.rs:1037-1062` —— `ClaudeMessageRequest`：

```rust
pub struct ClaudeMessageRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    pub max_tokens: i32,
    ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    ...
}
```

`crates/aigw-core/src/models.rs:1065-1071` —— `ClaudeToolDef`：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeToolDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}
```

**关键结论**：`input_schema` 是**非 Option 必填字段**，且 struct **没有 `type` 字段**。Anthropic 服务端工具的线上形态是：

```json
{"type": "web_search_20250305", "name": "web_search", "max_uses": 5}
```

—— 无 `input_schema`。因此：

1. **`AnthropicToOpenAI` 路径**（`crates/aigw-core/src/adapter.rs:225-233`）：
   ```rust
   fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
       let req: ClaudeMessageRequest = serde_json::from_value(body)
           .map_err(|e| AdapterError::Parse(format!("Invalid Claude request: {}", e)))?;
   ```
   带 `web_search_20250305` 的请求在这里 **`from_value` 直接失败**，整条请求以 `AdapterError::Parse` 被拒（返回 500 `adapter_error`，见 `chat.rs:1534-1540` 的 map_err）。**不是「丢弃 + 告警」，是硬失败** —— 这与 Responses 侧的优雅降级行为不一致。

2. **`AnthropicPassthrough` 路径**（`crates/aigw-core/src/adapter.rs:1613-1670`）：同样先 `from_value::<ClaudeMessageRequest>`（`adapter.rs:1619-1620`），再 `serde_json::to_value(&req)` 序列化回去（`adapter.rs:1660-1661`）。即便字段能解析，**round-trip 也会丢掉 `type` / `max_uses` 等未声明字段**（struct 无 `#[serde(flatten)]` 兜底，`models.rs` 全文无 `flatten`）。

3. 工具转换本体（Claude tools → OpenAI tools）在 `crates/aigw-core/src/adapter.rs:922-934`：
   ```rust
   let tools = req.tools.as_ref().map(|claude_tools| {
       claude_tools.iter().map(|ct| crate::models::ToolDef {
           tool_type: "function".to_string(),
           function: crate::models::ToolDefFunction {
               name: ct.name.clone(),
               description: ct.description.clone(),
               parameters: Some(ct.input_schema.clone()),
           },
       }).collect()
   });
   ```
   **无条件把每个工具打成 `function`**，没有任何服务端工具分支。

> 落地含义：Anthropic 侧要支持内建搜索，必须先给 `ClaudeToolDef` 加 `type: Option<String>` + `input_schema: Option<Value>`（或整体改为 untagged enum），否则连请求都收不下来。这是 Responses 侧没有的额外工作量。

### A4. agentic loop —— **不存在，一个都没有**

全仓扫描结论：

| 检查 | 结果 | 证据 |
|------|------|------|
| handler / adapter / oauth_pipeline 里有 `loop {}` | **零** | `grep -rn "loop {" crates/aigw-server/src/routes/ crates/aigw-core/src/adapter.rs crates/aigw-core/src/oauth_pipeline.rs` → 无输出 |
| `chat.rs` 中上游 `.send()` 次数 | **1 次** | `crates/aigw-server/src/routes/chat.rs:1743`（唯一）；`chat.rs:1616` 是 `client.post` 构建 |
| `fallback_order` 是否构成循环 | **否** | `crates/aigw-core/src/router.rs:494-501` 只返回按 priority 排序的 index 列表；`chat.rs:1137-1149` 只取 `.first()` 作首选兜底。`chat.rs:1133-1136` 的注释说「fallback loop over `fallback_order` is exercised below the send」，但 send 之后并无任何重发代码 —— 实际只有 `report_failure` 记 cooldown（`chat.rs:1852`），当前请求直接返回错误 |
| OAuth 401 重试是否多轮 | **否，仅 1 次重发同一请求** | `crates/aigw-core/src/oauth_pipeline.rs:334-362`：401 → `invalidate_and_refresh` → `.send()` 一次；仍 401 则报错。文件头 `oauth_pipeline.rs:14` 注明「retry once」 |
| `build_retry_client` 的重试 | **传输层重试，非对话轮次** | `crates/aigw-core/src/router.rs:535-548`：`ExponentialBackoff` + `RetryTransientMiddleware`，重发**完全相同的 body**，不读响应内容 |

**结论**：aigw 是严格的「单发代理」（request → adapt → 一次上游 → response/SSE）。要实现内建搜索，必须**从零引入**一个在单个客户端请求内部多次调用上游的循环，这是本特性最大的架构增量，也正是 `docs/research/2026-10-05-websearch-server-tool-support.md:171-184`（§3.2 可行性评估）表格里标注「**多轮 agentic loop … aigw 现状：零**」的那一行。

### A5. SSE 实现 —— 抽象、类型，以及能否中途合成事件

#### 两个抽象层

1. **`MessageAdapter` trait** —— `crates/aigw-core/src/adapter.rs:34-46`
   ```rust
   pub trait MessageAdapter: Send + Sync {
       fn client_protocol(&self) -> ClientProtocol;
       fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError>;
       fn adapt_response(&self, body: Value) -> Result<Value, AdapterError>;
       fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>>;
   }
   ```
   选择器：`select_adapter`（`adapter.rs:75-90`，按 `(ClientProtocol, ProviderType)` 二维 match）/ `select_responses_adapter`（`adapter.rs:104-113`，额外看 `deployment.supported_standard_types` 是否含 `"responses"` 决定原生直通）。

2. **`StreamAdapter` trait** —— `crates/aigw-core/src/adapter.rs:49-58`（**这是合成事件的关键钩子**）
   ```rust
   /// Streaming chunk-by-chunk converter.
   ///
   /// `next` is called **once per upstream byte chunk**, not in a drain loop: the
   /// chunk is fully consumed into events on that single call. Callers must not
   /// loop until `None` — a chunk that produced events always yields `Some` on the
   /// same input (the converter is not a queue), so a drain loop never terminates
   /// and the response stream hangs open.
   pub trait StreamAdapter: Send {
       fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>>;
       fn finish(&mut self) -> Option<Vec<u8>>;
   }
   ```

   实现清单：`PassthroughStream`（`adapter.rs:172`）、`AnthropicToOpenAIStream`（`adapter.rs:606`）、`AnthropicPassthroughStream`（`adapter.rs:1684`）、`OpenAIToAnthropicStream`（`adapter.rs:1828`）、**`ResponsesToChatCompletionsStream`（`adapter.rs:3051` 的 `next` / `adapter.rs:3229` 的 `finish`）**。

#### `ResponsesToChatCompletionsStream` —— 已经在"无中生有"地合成 Responses 事件

状态机 struct：`crates/aigw-core/src/adapter.rs:2934-2946`
```rust
struct ResponsesToChatCompletionsStream {
    response_id: String,
    model: String,
    created_sent: bool,
    done: bool,
    /// Monotonic `sequence_number` carried by every event payload.
    seq: u64,
    /// Set once the assistant message item has been announced.
    text_item_id: Option<String>,
    content_part_open: bool,
    text_accum: String,
    pending_usage: Option<Value>,
    tool_call_buf: Vec<ToolCallState>,
}
```
`ToolCallState`：`adapter.rs:2948-2958`（含 `item_id` / `call_id` / `name` / `arguments` / `item_open` / `done`）。
输出索引常量：`adapter.rs:2962` `const TEXT_OUTPUT_INDEX: usize = 0;`（工具调用占 index 之后，`output_index_for_tool` 在 `adapter.rs:2989-2991`）。

**事件发射原语** —— `crates/aigw-core/src/adapter.rs:2979-2987`：
```rust
    /// Serialize one event, stamping `type` and `sequence_number` into the
    /// payload — clients read the type from there, not from the `event:` line.
    fn sse(&mut self, event: &str, mut payload: Value) -> Vec<u8> {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("type".to_string(), json!(event));
            obj.insert("sequence_number".to_string(), json!(self.seq));
        }
        self.seq += 1;
        format!("event: {}\ndata: {}\n\n", event, payload).into_bytes()
    }
```

**已有的 item 生命周期合成器**（可直接作为 `web_search_call` item 的模板）：
- `ensure_text_item`（`adapter.rs:2993-3021`）：合成 `response.output_item.added`（message）+ `response.content_part.added`
- `ensure_tool_item`（`adapter.rs:3026-3049`）：合成 `response.output_item.added`（function_call）
- 完整事件序列契约在 `adapter.rs:2911-2932` 的注释里（`response.created` → `in_progress` → `output_item.added` → `content_part.added` → `output_text.delta`* → `output_text.done` → `content_part.done` → `output_item.done` → … → `response.completed` → `[DONE]`）

**答案：能。** 网关完全有能力在代理上游 delta 的同时、之前或之中插入额外事件 —— 这正是 `ResponsesToChatCompletionsStream` 当前的工作方式（上游发的是 Chat `delta`，客户端收到的是 Responses `output_item.added` 等，这些事件上游从未发送过，全部由 `self.sse(...)` 合成）。要加 `web_search_call`：
1. 在 `ResponsesToChatCompletionsStream` 加一个 `search_item_buf` 状态 + `ensure_search_item` 方法（仿 `ensure_tool_item`）；
2. 为搜索 item 预留一个 `output_index`（当前 `output_index_for_tool` 的 `TEXT_OUTPUT_INDEX + 1 + idx` 编号方案需调整，否则索引会和 function_call 撞）；
3. 发 `response.output_item.added`(type=`web_search_call`, status=`in_progress`) → 执行搜索 → 发 `response.output_item.done`(status=`completed`)。

#### 调用侧（handler 如何驱动 StreamAdapter）

`crates/aigw-server/src/routes/responses.rs:1021-1079`：
```rust
            let mut stream_adapter = adapter
                .stream_adapter()
                .expect("ResponsesToChatCompletions must provide a stream adapter");
            let mut pending_chunk: Vec<u8> = Vec::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        ...
                        // Convert the upstream chunk (Chat SSE) → Responses SSE
                        // events via the bridge stream adapter. One call per
                        // chunk: `next` drains the whole input, so looping until
                        // `None` on the same bytes would never terminate.
                        pending_chunk.extend_from_slice(&chunk);
                        if let Some(transformed) = stream_adapter.next(&pending_chunk) {
                            let _ = tx.send(transformed);
                        }
                        pending_chunk.clear();
                    }
                    Err(e) => { failure = Some((0, e.to_string())); break; }
                }
            }
            // Flush any final SSE events (response.completed + [DONE])
            if let Some(final_event) = stream_adapter.finish() {
                let _ = tx.send(final_event);
            }
```

客户端传输类型链：`tokio::sync::mpsc::UnboundedSender<Vec<u8>>`（`tx`）→ `tokio_stream::wrappers::UnboundedReceiverStream`（`responses.rs:471`）→ `axum::body::Body::from_stream`（`responses.rs:478`）→ 响应头 `text/event-stream`（`responses.rs:475`）。chat 侧同形：`chat.rs:1397-1405`。

> 关键约束（已被 commit 4bd85c7 / c3f360c 修过两次）：`StreamAdapter::next` **不是队列**，不能 drain-loop，且 `finish()` 必须在 `[DONE]` 之前发完 `response.completed`。内建搜索如果需要在循环中注入事件，必须**不打破这个「一 chunk 一调用」契约**，否则会复现死循环 / 提前断流 bug。

---

## B. SpendLog / 调用日志

### B1. 表结构（migration 文件）

三套 driver 各一份 DDL，列名完全一致、仅类型不同：

| driver | 建表文件 | 行 |
|--------|---------|-----|
| SQLite | `crates/aigw-core/migrations/sqlite/002_spend_logs.sql` | `:5` |
| PostgreSQL | `crates/aigw-core/migrations/postgres/002_spend_logs.sql` | `:5` |
| MySQL | `crates/aigw-core/migrations/mysql/002_spend_logs.sql` | `:5` |

DDL 头部注释自述「Maps to LiteLLM_SpendLogs table (column-compatible)」。

**002 基线列**（33 列）：

| 列 | SQLite | PostgreSQL | MySQL |
|---|---|---|---|
| `request_id`（建表时为 PK，023 改名） | TEXT PRIMARY KEY | TEXT NOT NULL, PK | VARCHAR(255) NOT NULL, PK |
| `call_type` | TEXT NOT NULL | TEXT NOT NULL | VARCHAR(255) NOT NULL |
| `api_key` | TEXT NOT NULL | TEXT NOT NULL | VARCHAR(255) NOT NULL |
| `spend` | REAL NOT NULL DEFAULT 0.0 | DOUBLE PRECISION NOT NULL DEFAULT 0.0 | DOUBLE NOT NULL DEFAULT 0.0 |
| `total_tokens` / `prompt_tokens` / `completion_tokens` | INTEGER NOT NULL DEFAULT 0 | INTEGER NOT NULL DEFAULT 0 | INTEGER NOT NULL DEFAULT 0 |
| `start_time` / `end_time` | DATETIME NOT NULL | TIMESTAMPTZ(3) NOT NULL | DATETIME(3) NOT NULL |
| `request_duration_ms` | INTEGER | INTEGER | INTEGER |
| `completion_start_time` | DATETIME | TIMESTAMPTZ(3) | DATETIME(3) |
| `model` | TEXT NOT NULL | TEXT NOT NULL | VARCHAR(255) NOT NULL |
| `model_id` / `model_group` / `custom_llm_provider` / `api_base` / `user` | TEXT | TEXT | VARCHAR(255) |
| `metadata` | BLOB | **JSONB** | JSON |
| `cache_hit` / `cache_key` | TEXT | TEXT | VARCHAR(255) |
| `request_tags` | BLOB | **JSONB** | JSON |
| `team_id` / `organization_id` / `end_user` / `requester_ip_address` | TEXT | TEXT | VARCHAR(255) |
| `messages` / `response` | BLOB | **JSONB** | JSON |
| `session_id` / `status` / `mcp_namespaced_tool_name` / `agent_id` | TEXT | TEXT | VARCHAR(255) |
| `proxy_server_request` | BLOB | **JSONB** | JSON |

**后续 ALTER 迁移**：

| migration | 新增列 | 文件锚点 |
|-----------|--------|---------|
| 021 | `body_archived BOOLEAN NOT NULL DEFAULT FALSE`、`parquet_path TEXT` | `crates/aigw-core/migrations/{sqlite,postgres,mysql}/021_spend_logs_body_archive.sql:4` |
| 023 | `request_id` → **改名 `call_id`**（aigw 网关 UUID v7，PK）；新增可空 `request_id TEXT`（上游 provider id，如 `msg_xxx` / `chatcmpl-xxx`）；加索引 `idx_spend_logs_request_id` | `.../postgres/023_rename_request_id_to_call_id.sql:12`、`.../mysql/023_rename_request_id_to_call_id.sql:11`、`.../sqlite/023_rename_request_id_to_call_id.sql` |
| 025 | `image_tokens BIGINT`（可空） | `crates/aigw-core/migrations/{sqlite,postgres,mysql}/025_image_tokens.sql:8` |

**全迁移后共 37 列**。

### B2. Rust struct

`crates/aigw-core/src/models.rs:146`（struct 声明；`models.rs:129` 的注释 `// spend_logs (24 columns, ...)` 已过期），derive `Debug, Clone, Serialize, Deserialize, FromRow`。

| 行 | 字段 | 类型 |
|---|---|---|
| `models.rs:147` | `call_id` | `String` |
| `models.rs:148` | `call_type` | `String` |
| `models.rs:149` | `api_key` | `String` |
| `models.rs:150` | `spend` | `f64` |
| `models.rs:151-153` | `total_tokens` / `prompt_tokens` / `completion_tokens` | `i32` |
| `models.rs:154-155` | `start_time` / `end_time` | `DateTime<Utc>` |
| `models.rs:156` | `request_duration_ms` | `Option<i32>` |
| `models.rs:157` | `completion_start_time` | `Option<DateTime<Utc>>` |
| `models.rs:158` | `model` | `String` |
| `models.rs:159-163` | `model_id` / `model_group` / `custom_llm_provider` / `api_base` / `user` | `Option<String>` |
| `models.rs:164` | `metadata` | `Option<serde_json::Value>` |
| `models.rs:165-166` | `cache_hit` / `cache_key` | `Option<String>` |
| `models.rs:167` | `request_tags` | `Option<serde_json::Value>` |
| `models.rs:168-171` | `team_id` / `organization_id` / `end_user` / `requester_ip_address` | `Option<String>` |
| `models.rs:172-173` | `messages` / `response` | `Option<serde_json::Value>` |
| `models.rs:174-177` | `session_id` / `status` / `mcp_namespaced_tool_name` / `agent_id` | `Option<String>` |
| `models.rs:178` | `proxy_server_request` | `Option<serde_json::Value>` |
| `models.rs:179` | `body_archived` | `bool` |
| `models.rs:180` | `parquet_path` | `Option<String>` |
| `models.rs:186` | `request_id` | `Option<String>` |
| `models.rs:191` | `image_tokens` | `Option<i32>` |

**无单独的 `NewSpendLog` / `InsertSpendLog` 变体** —— 读写复用同一 struct。
相关：`DailySpendLog`（`models.rs:198-218`）、`DailySpendKind` enum（`models.rs:221-229`）。

### B3. 写入路径

**DB facade**：
- `Database::insert_spend_log` 分发器：`crates/aigw-core/src/db.rs:3621`
- 后端实现：SQLite `db.rs:2317` / MySQL `db.rs:2752` / Postgres `db.rs:3201`
- `Database::update_spend_log` 分发器：`crates/aigw-core/src/db.rs:3630`

#### 非流式（chat）
| 场景 | 构造 | INSERT |
|------|------|--------|
| OAuth 分支成功 | `chat.rs:1459` | `chat.rs:1502` |
| 普通路径成功 | `chat.rs:2578` | `chat.rs:2666` |
| 失败（4xx/5xx） | `chat.rs:1914` / `chat.rs:2485` | `chat.rs:1954` / `chat.rs:2524` |
| 缓存 HIT（`spend: 0.0`、`cache_hit: Some("cached")`） | `chat.rs:1673` | `chat.rs:1711` |

#### 流式（两阶段 INSERT → UPDATE）
`chat.rs:1970` 的注释明确记录了这个设计。

| 阶段 | 位置 |
|------|------|
| Phase 1 占位 INSERT（`response: {"status":"streaming"}`、`status: "streaming"`） | `chat.rs:1987`（构造）→ `chat.rs:2025`（INSERT）；OAuth 变体 `chat.rs:1282` → `chat.rs:1320` |
| SSE 转发 + 累计（`tokio::spawn`） | `chat.rs:2027-2046` 起（`upstream_resp.bytes_stream()`） |
| Phase 2 UPDATE（最终 token / spend / 合并后 response） | `chat.rs:2241`、`chat.rs:2313`；OAuth 变体 `chat.rs:1379` |

> 注意：`status` 在 Phase 2 **不改写为 `success`**，流式记录的 status 一直是 `"streaming"`。

#### responses 路径（`call_type = "responses"`）
`crates/aigw-server/src/routes/responses.rs`：Phase 1 INSERT `responses.rs:360`→`:398` 与 `responses.rs:963`→`:1001`；Phase 2 UPDATE `responses.rs:1208`、`responses.rs:1275`；非流式 INSERT `responses.rs:538`→`:581`、`responses.rs:782`→`:827`、`responses.rs:894`→`:934`、`responses.rs:1442`→`:1482`。

#### commit 8cf8c12 —— 流式 SpendLog 只存最后一个 chunk

`git show 8cf8c12 --stat`：
```
 crates/aigw-server/src/routes/responses.rs         | 84 +++++++++++++++++++++-
 .../aigw-server/tests/bdd_steps/responses_steps.rs | 22 ++++++
 .../aigw-server/tests/features/responses.feature   |  1 +
```

根因（commit message 原文）：「`assembled_response` 在非空分支里只取 `chunk_jsons.last()`。流式 SSE 的最后一个 chunk 只带 `finish_reason`，delta 为空——文本全在之前的 chunk 里。」

修复：把 `let last = chunk_jsons.last().unwrap();` 替换为遍历全部 chunk 累加重建 assistant 消息（`delta.content` / `delta.reasoning_content` / 按 `index` 合并 `delta.tool_calls` 的 id/name/arguments / 记录 `finish_reason`），对齐 `chat.rs` 的 merge 逻辑。落库 JSON 形态从 `{"response": <last_chunk>, "streaming": true, ...}` 变为 `{"choices":[{"index":0,"message":{...},"finish_reason":...}], "streaming": true, "usage": {...}}`。当前代码位置：`crates/aigw-server/src/routes/responses.rs:1096` 起（`assembled_response` 构造）。

回归断言：`crates/aigw-server/tests/bdd_steps/responses_steps.rs:301` 起（`then_spendlog_stream_response_has_content`）。

> 落地含义：**内建搜索的流式路径有同类陷阱**。搜索调用的结果不会出现在上游 chunk 里（是网关自己产的），若沿用「从 chunk_jsons 重建 response」的逻辑，搜索记录会凭空消失。必须显式把搜索调用注入 `assembled_response` 或新开行。

#### 异步/批量写入器

`crates/aigw-core/src/daily_spend_queue.rs`：
- struct `DailySpendQueue`：`daily_spend_queue.rs:27`（持 `mpsc::UnboundedSender<PendingDailySpend>`）
- 入队 API `queue(&self, log: DailySpendLog)`：`daily_spend_queue.rs:84`
- 后台 flush 任务：`daily_spend_queue.rs:39-78` —— 首项唤醒 → drain 全部 → 按复合 key `(entity_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)` 聚合 → 批量 upsert 到 6 张 `daily_*_spend` 表（`ON CONFLICT DO UPDATE SET col = col + EXCLUDED.col`）→ sleep 10s
- 调用点：`chat.rs:2382`（流式 Phase 2 后）、`chat.rs:2727`（非流式成功后），responses 侧同形

### B4. 嵌套 / 子调用支持 —— **没有，需要新行或新表**

**可用于"挂载"的列**：

| 列 | 类型 | 现有用途 |
|---|---|---|
| `call_type` | TEXT/VARCHAR(255) | **判别列**。现有取值仅 `"completion"`（chat 全路径，见 `chat.rs:1461`、`chat.rs:1915`、`chat.rs:2583` 等）与 `"responses"`。schema 无约束，**可自由新增 `"web_search"` 这类取值** |
| `metadata` | JSONB/JSON/BLOB | 无 schema 约束的自由 JSON。现存键：`cache_read_tokens` / `cache_creation_tokens` / `cache_read_spend` / `cache_create_spend`（`chat.rs:2607` 起构造）、`image_tokens_source`、`user_agent` / `device_id`（`chat.rs:1585-1596`）、`cached`（`chat.rs:1667`） |
| `request_tags` | JSONB/JSON/BLOB | 任意 tag 数组 |
| `mcp_namespaced_tool_name` | TEXT | 标识被调用的 MCP 工具（已被 `daily_spend_queue` 的聚合 key 使用） |
| `agent_id` | TEXT | agent 上下文标识（当前全路径硬编码 `None`） |
| `session_id` | TEXT | 松散会话分组 |
| `request_id` | TEXT | **上游** provider 的 id（不是父请求 id） |

**明确不存在的**：`parent_call_id`、`parent_request_id`、子调用外键、任何 self-referencing FK。全部 migration 中无任何 FK 约束指向 `spend_logs` 自身。

**结论与两条可选路线**：

| 路线 | 做法 | 代价 |
|------|------|------|
| **A. 每次搜索一行**（推荐，litellm 风格） | 新 SpendLog 行，`call_type="web_search"`，自生成 `call_id`，用 `metadata.parent_call_id`（约定，非 schema）或 `session_id` 关联父 LLM 行 | 零 migration。但 `/spend/logs` 列表会混入搜索行（需前端加 `call_type` 过滤）；父子关联只是"约定"，无 DB 层保证；聚合/排行榜口径需重新审视（搜索行的 `model` 填什么？） |
| **B. 新 migration 加 `parent_call_id` 列 + 索引** | 027 迁移（三 driver 各一份），`SpendLog` struct 加字段，`insert_spend_log` 三份实现各改 SQL | 要动 `db.rs`（10452 行）里三份手写 INSERT 语句；但语义正确，可做 DB 层 JOIN，前端可做父子折叠 |

**不可行的路线**：把 N 次搜索塞进父行的 `metadata` JSON —— 虽然技术上能存，但无法按次计费归集、无法分页查询、无法独立统计，与 `/spend/logs` 现有分页模型冲突。

### B5. Admin API / 前端

#### HTTP 路由（注册于 `main.rs:591-618`）

| 方法 | 路径 | handler | 权限 | handler 行号 |
|------|------|---------|------|-------------|
| GET | `/spend/logs` | `spend::spend_logs` | 任意有效 key | `crates/aigw-server/src/routes/spend.rs:237` |
| GET | `/spend/keys` | `spend_keys` | 任意 key | `spend.rs:548` |
| GET | `/spend/users` | `spend_users` | 任意 key | `spend.rs:570` |
| GET | `/spend/tags` | `spend_tags` | 任意 key | `spend.rs:598` |
| GET | `/spend/models` | `spend_models` | 任意 key | `spend.rs:800` |
| GET | `/spend/providers` | `spend_providers` | 任意 key | `spend.rs:839` |
| GET | `/spend/model-groups` | `spend_model_groups` | 任意 key | `spend.rs:1009` |
| GET | `/global/spend` | `global_spend` | admin | `spend.rs:632` |
| GET | `/global/spend/logs` | `global_spend_logs` | admin | `spend.rs:649` |
| GET | `/global/spend/logs/{call_id}` | `global_spend_log_detail` | admin | `spend.rs:352` |
| GET | `/global/spend/keys` | `global_spend_keys` | admin | `spend.rs:765` |
| GET | `/global/spend/models` | `global_spend_models` | admin | `spend.rs:959` |
| GET | `/global/spend/providers` | `global_spend_providers` | admin | `spend.rs:854` |
| GET | `/global/spend/model-groups` | `global_spend_model_groups` | admin | `spend.rs:1048` |
| GET | `/global/spend/keys/rankings` | `global_spend_keys_rankings` | admin | `spend.rs:1345` |
| GET | `/global/spend/activity` | `global_spend_activity` | admin | `spend.rs:1107` |

#### 列表 DTO（`/spend/logs` + `/global/spend/logs`）

构造位置：`crates/aigw-server/src/routes/spend.rs:306-338`
外层信封：`{ data: [...], count, total_count, page, page_size, total_pages }`
每项字段：`call_id`、`request_id`、`call_type`、`api_key`、`key_name`（解析后的 alias）、`spend`、`total_tokens`、`prompt_tokens`、`completion_tokens`、`start_time`(RFC3339)、`end_time`(RFC3339)、`request_duration_ms`、`ttft_ms`（计算得出，非流式为 `None`）、`model`、`model_id`、`model_group`、`custom_llm_provider`、`api_base`、`user`、`team_id`、`organization_id`、`end_user`、`session_id`、`request_tags`、`metadata`、`cache_hit`、`cache_key`、`status`、`mcp_namespaced_tool_name`、`requester_ip_address`。

**列表端点刻意排除 `messages` / `response` / `proxy_server_request`** —— 由测试锁定：`crates/aigw-server/src/routes/spend.rs:1892`（"List endpoint must not include messages/response field"）。

#### 详情 DTO（`/global/spend/logs/{call_id}`）

构造位置：`crates/aigw-server/src/routes/spend.rs:495-529`
= 列表全字段 **+** `messages` + `response` + `proxy_server_request`。body blob 按 `body_archived` 标志从 DB 热路径或 Parquet 冷路径（经 `body_archiver`）取。

#### 前端

页面：`crates/aigw-frontend/src/pages/spend-logs/index.tsx`
- `SpendLogsPage` 导出：`index.tsx:1044`
- TS `SpendLog` 接口（列表）：`index.tsx:65-97`（含 `image_tokens`）
- TS `SpendLogDetail` 接口（抽屉）：`index.tsx:99-134`
- 表格列渲染 `<TableHeader>`：`index.tsx:1413-1454`

渲染列：`call_id`（截断 UUID）、`request_id`、`start_time`、**`call_type`（badge）**、`model`+`model_group` badge+image tokens 标记、`key_name`/`user`、`end_user`、`requester_ip_address`、`status`（`StatusBadge`）、`ttft_ms`、`request_duration_ms`、`prompt/completion tokens` + metadata 里的 cache tokens、`spend`。

详情抽屉 `DetailDrawer`：`index.tsx:631`。
数据源：列表调 `/global/spend/logs`（`index.tsx:1144`），详情调 `/global/spend/logs/{call_id}`（`index.tsx:1176`）。筛选：时间预设（15m/4h/24h/7d/custom）、call_id/request_id 搜索、model 下拉、status（`all`/`success`/`failure`/`streaming`）、min/max token。Live-tail 每 15s 刷新（`index.tsx:1042` `LIVE_TAIL_INTERVAL = 15_000`）。

**"web search call" 会出现在哪 / 需要改什么**：
若走 B4 路线 A（每次搜索一行），搜索记录会**自动出现在 `/spend/logs` 列表**，`call_type` 列显示 `web_search`。需要改的：
1. `index.tsx:1413-1454` 加一个 `call_type` 筛选器（否则搜索行污染 LLM 调用列表）；
2. 搜索行的 `model` / `prompt_tokens` / `completion_tokens` 列全为空或 0 —— 表格需容错（或改渲染为「搜索次数」）；
3. `status` 筛选的 `success`/`failure`/`streaming` 三态需补 `web_search` 语义；
4. 详情抽屉（`index.tsx:631`）的 token/cache 区块对搜索行无意义，需条件渲染；
5. 若要父子折叠展示，需后端提供关联字段（即 B4 路线 B 的 `parent_call_id`）+ 前端树形/嵌套行。

---

## C. 计费 / 定价

### C1. 定价数据模型

**存储位置**：DB 表 `proxy_models`，每行两个 JSON blob —— `model_info`（成本权威源）与 `litellm_params`（fallback）。**无内置价格表**（无 litellm `model_prices_and_context_window.json` 等价物）。

**运行期表示** —— `Deployment` struct 的定价字段，`crates/aigw-core/src/deployment.rs:27-61`：
```rust
    pub input_cost_per_token: Option<f64>,               // USD / input token
    pub output_cost_per_token: Option<f64>,              // USD / output token
    pub cache_read_input_token_cost: Option<f64>,        // USD / cache-read token
    pub cache_creation_input_token_cost: Option<f64>,    // USD / cache-creation token
    pub modal_pricing: Option<ModalPricing>,             // USD / 1M tokens by modality
```

`ModalPricing` —— `crates/aigw-core/src/models.rs:134-142`：
```rust
pub struct ModalPricing {
    pub image: Option<f64>,  // USD per 1M image input tokens
    pub audio: Option<f64>,  // USD per 1M audio input tokens
    pub video: Option<f64>,  // USD per 1M video input tokens
}
```
**注意：尽管叫 "modal"，单位仍是「每 100 万 token」，不是「每张图」或「每秒」。**

**价格提取**：`extract_pricing` —— `crates/aigw-server/src/routes/chat.rs:59-96`。先读 `model_info`，缺失则 fallback 到解密后的 `litellm_params`（`chat.rs:376` 注释说明）。读取键：`input_cost_per_token` / `output_cost_per_token` / `cache_read_input_token_cost` / `cache_creation_input_token_cost`。
`modal_pricing` 走另一条路：`extract_modal_pricing(&m.model_info)`，调用点 `crates/aigw-core/src/resolver.rs:301`、`resolver.rs:394`。

**config.yaml 侧**：`ModelParams`（`crates/aigw-core/src/config.rs:244-274`）只暴露 `input_cost_per_token`（`config.rs:261`）和 `output_cost_per_token`（`config.rs:265`）—— cache 层级定价只能走 DB。

### C2. 成本计算函数

**主函数** `calc_spend` —— `crates/aigw-server/src/routes/chat.rs:104-123`：
```rust
pub(crate) fn calc_spend(
    prompt_tokens: i32,
    completion_tokens: i32,
    input_cost: Option<f64>,
    output_cost: Option<f64>,
    cache_read_tokens: i32,
    cache_creation_tokens: i32,
    cache_read_cost: Option<f64>,
    cache_creation_cost: Option<f64>,
) -> f64 {
    let regular = 0.max(prompt_tokens - cache_read_tokens - cache_creation_tokens) as f64;
    let read_cost = cache_read_cost.unwrap_or(input_cost.unwrap_or(0.0));
    let create_cost = cache_creation_cost.unwrap_or(input_cost.unwrap_or(0.0));
    let base_input = input_cost.unwrap_or(0.0);
    regular * base_input
        + cache_read_tokens as f64 * read_cost
        + cache_creation_tokens as f64 * create_cost
        + completion_tokens as f64 * output_cost.unwrap_or(0.0)
}
```

token 口径：
- 普通 prompt：`(prompt - cache_read - cache_creation) * input_cost`
- cache-read：`cache_read * cache_read_cost`（缺失 fallback `input_cost`）
- cache-creation：`cache_creation * cache_creation_cost`（缺失 fallback `input_cost`）
- completion：`completion * output_cost`
- **reasoning token 不单独计价** —— `TokenDetails.reasoning_tokens` 存在于 `crates/aigw-core/src/models.rs:684`，但 `calc_spend` 把全部 completion token 一视同仁，reasoning 按 `output_cost_per_token` 计。

**Anthropic 归一化**：调 `calc_spend` 前先把 cache token 加回 prompt —— `crates/aigw-server/src/routes/chat.rs:1435-1448`（`effective_prompt = prompt_tokens + cache_read + cache_creation`，因 Anthropic 的 `input_tokens` 不含缓存部分）。responses 侧同形：`responses.rs:1080-1095`。

**`calc_spend_modal`** —— `crates/aigw-server/src/routes/chat.rs:139-158`。签名含 `modal_tokens: &[(&str, i32)]`、`modal_pricing: Option<&ModalPricing>`，把 USD/1M 除以 `1_000_000` 得每 token 单价。**标注 `#[allow(dead_code)]`（`chat.rs:138`）**，`chat.rs:137` 注释说明「Wired into the embeddings spend path once a request carries per-modality input tokens … TD-012b defers real-load wiring until such traffic exists」—— **当前不在任何活跃请求路径上**。

### C3. 非 token 计价单位 —— **完全不存在（本特性最核心的缺口）**

**定论：按次 / 按请求 / 按图 / 按秒 / 按字符的计价在任何活跃代码路径中都不存在。**

证据：

| 检查项 | 结果 |
|--------|------|
| `input_cost_per_request` / `output_cost_per_request` | 全仓零命中 |
| `input_cost_per_image` / `output_cost_per_image` | 全仓零命中 |
| `input_cost_per_second` / `input_cost_per_character` | 全仓零命中 |
| `per_call` / 任何 flat-fee 概念 | 全仓零命中 |
| `ModalPricing.image/audio/video` | **是 token 计价**（USD/1M token，用于 Gemini 多模态 embedding），不是按图/按秒 |
| `calc_spend_modal` | dead code（`chat.rs:138` `#[allow(dead_code)]`），未接线 |
| embeddings 路径（`crates/aigw-server/src/routes/embeddings.rs:531-540`） | 调 `calc_spend` 且 `completion_tokens = 0`，纯 prompt token 计价，**不叠加任何按次费用** |

**同时缺失：usage 里的搜索计数位**

| struct | 锚点 | 字段 |
|--------|------|------|
| OpenAI 风格 `Usage` | `crates/aigw-core/src/models.rs:665-676` | `prompt_tokens`、`completion_tokens`、`total_tokens`、`prompt_tokens_details`、`completion_tokens_details` |
| `TokenDetails` | `crates/aigw-core/src/models.rs:679-691` | `reasoning_tokens`、`audio_tokens`、`cached_tokens`、`accepted_prediction_tokens`、`rejected_prediction_tokens` |
| Anthropic `ClaudeUsage` | `crates/aigw-core/src/models.rs:1151-1161` | `input_tokens`、`output_tokens`、`cache_read_input_tokens`、`cache_creation_input_tokens` |

**`usage.server_tool_use.web_search_requests` 既不存在、不解析、也不保留。** 全仓无 `server_tool_use` / `web_search_requests` 标识符。Anthropic 上游若回传该字段，会在反序列化时被静默丢弃（struct 无该字段、无 `flatten` 兜底）。

> 落地含义：web_search 按「$X / 1000 次搜索」计价，必须**新引入**非 token 计价概念。好消息是落库侧零 schema 改动 —— `SpendLog.spend: f64`（`models.rs:150`）和各实体表的 `spend: f64` 列对金额来源单位完全中立。坏消息是：需要新增价格字段（`search_cost_per_1k` 之类）+ 新的计算函数 + 把结果并入现有 `spend_amount`，并决定是「搜索费记在父 LLM 行」还是「独立行」（两者对 `/spend/models` 之类按模型聚合的口径影响不同）。

### C4. 预算 / 限流 / spend 聚合如何消费成本

**成本增量（请求后异步 fire-and-forget）**：

trait 声明 —— `crates/aigw-core/src/db.rs:365-368`：
```rust
    async fn increment_key_spend(&self, token_hash: &str, cost: f64) -> Result<()>;
    async fn increment_user_spend(&self, user_id: &str, cost: f64) -> Result<()>;
    async fn increment_team_spend(&self, team_id: &str, cost: f64) -> Result<()>;
    async fn increment_org_spend(&self, org_id: &str, cost: f64) -> Result<()>;
```
实现：`db.rs:955`（key）/ `db.rs:964`（user）/ `db.rs:973`（team）/ `db.rs:982`（org）。
目标列：`virtual_keys.spend` / `users.spend` / `teams.spend` / `organizations.spend`。

调用点（chat）：`chat.rs:1505`（OAuth 非流式）、`chat.rs:2343-2351`（流式 Phase 2 后，四级一起）、`chat.rs:2676-2684`（非流式成功）。
embeddings 同形：`crates/aigw-server/src/routes/embeddings.rs:593-612`。

**预算检查（请求前）**：`check_entity` —— `crates/aigw-core/src/budget.rs:45-57`：
```rust
fn check_entity(entity_type: &str, spend: f64, max_budget: Option<f64>) -> Result<(), BudgetError> {
    ...
    if spend > limit { return Err(BudgetError::Exceeded { ... }); }
    Ok(())
}
```
多级顺序 key → user → team → organization：`crates/aigw-core/src/budget.rs:152-225`。

**日聚合**：`DailySpendQueue`（见 B3），6 张 `daily_*_spend` 表（`daily_user_spend` / `daily_team_spend` / `daily_organization_spend` / `daily_end_user_spend` / `daily_agent_spend` / `daily_tag_spend`），`daily_spend_queue.rs:1-11` 文件头说明，`ON CONFLICT DO UPDATE SET col = col + EXCLUDED.col`。

**让搜索费用计入预算所需改动（最小集）**：
1. 搜索费用必须在 `increment_*_spend` 调用**之前**算出并加进 `spend_amount`（`chat.rs:2676` / `responses.rs` 对应位置之前）；
2. 若走「独立 SpendLog 行」路线，需要**第二组** `increment_*_spend` 调用（否则日志有记录但预算不扣）；
3. `DailySpendLog`（`models.rs:198-218`）的聚合 key 含 `model` / `custom_llm_provider` —— 搜索调用这两个值填什么需定义（填 `"web_search"` / 搜索厂商名？），否则日聚合口径混乱；
4. **预算是「请求前检查、请求后增量」的最终一致模型** —— 搜索发生在请求**中途**（agentic loop 内），单次请求可能触发 N 次搜索。预算在循环开始时检查过一次，循环中途超预算无法拦截。需要在 loop 内加预算复检，或接受超支（并用 `max_agentic_loops` 这类上限兜底）。

---

## D. 配置与 provider 抽象

### D1. 配置加载

**顶层 struct** `AigwConfig` —— `crates/aigw-core/src/config.rs:30`：

| 字段 | 类型 | YAML key | 行 |
|---|---|---|---|
| `general_settings` | `Option<GeneralSettings>` | `general_settings` | `config.rs:32` |
| `model_list` | `Vec<ModelEntry>` | `model_list` | `config.rs:35` |
| `router_settings` | `Option<RouterSettings>` | `router_settings` | `config.rs:38` |
| `litellm_settings` | `Option<serde_json::Value>` | `litellm_settings` | `config.rs:41` |
| `environment_variables` | `Option<serde_json::Value>` | `environment_variables` | `config.rs:44` |
| `body_archive` | `Option<BodyArchiveConfig>` | `body_archive` | `config.rs:49` |
| `budget_reset` | `Option<BudgetResetConfig>` | `budget_reset` | `config.rs:55` |
| `cache` | `Option<CacheConfig>` | `cache` | `config.rs:60` |

`GeneralSettings`：`config.rs:105-163`（`master_key`、`database_url`、`custom_key_generate_length`、`disable_custom_api_keys`、`deployment_mode`、`metrics_buckets`、`compression`、`otel`、`request_body_limit_mb`、`alert_webhook`）。
`RouterSettings`：`config.rs:276-292`（`routing_strategy`、`allowed_fails`=3、`num_retries`=2、`cooldown_time`=30.0、`fallbacks`）。
`CacheConfig`：`config.rs:65-78`。

**来源与优先级：YAML + DB 混合，DB 优先**

1. `config.yaml` → `AigwConfig`（启动时）
2. `apply_environment_variables` 补环境变量（dotenvy 语义：shell env 永远胜出）—— `crates/aigw-core/src/config_loader.rs:101`
3. `seed_models_from_config` 把 `model_list` 写入 `proxy_models` 表 —— `crates/aigw-core/src/config_loader.rs:54`；**仅当该 `model_name` 的行不存在时才插**（`config_loader.rs:67` 的 `db.get_model_by_name` 判断）。经 admin API 创建的 DB 行**绝不被覆盖**
4. `build_router_config` 映射 `router_settings` → 运行期 `RouterConfig` —— `config_loader.rs:131`
5. `litellm_settings`（`drop_params` / `request_timeout` / `set_verbose`）被解析但**刻意未接线** —— `config_loader.rs:28` 有明确注释

**model deployment struct**：
- `ModelEntry`（容器）—— `config.rs:236`，字段 `model_name: String`（`config.rs:238`）、`litellm_params: ModelParams`（`config.rs:241`）
- **`model_info` 不是 `ModelEntry` 的字段** —— 它是 DB struct `ProxyModel` 上的 `serde_json::Value` 列，seed 时写成 `json!({})`（`config_loader.rs:79`）
- `ModelParams` —— `config.rs:244`，**全类型化 struct，不是 `HashMap<String, Value>`，无 `#[serde(flatten)]`**：`model`（`:246`）、`api_base`（`:248`）、`api_key`（`:250`）、`rpm`（`:252`）、`tpm`（`:254`）、`max_parallel_requests`（`:256`）、`input_cost_per_token`（`:261`）、`output_cost_per_token`（`:265`）、`tpm_limit`（`:270`）、`rpm_limit`（`:273`）
- 类型别名 —— `config.rs:305`：`pub type ModelInfo = ModelEntry; pub use ModelParams as LitellmParams;`
- seed 时 `ModelParams` 经 `serde_json::to_value(&entry.litellm_params)`（`config_loader.rs:77`）序列化进 `proxy_models.litellm_params` JSON 列

**运行期 `Deployment`**（resolver 的产物）—— `crates/aigw-core/src/deployment.rs:17` 起，与本特性相关的字段：
- `supported_standard_types: Vec<String>` —— `deployment.rs:52-56`，来自 `model_info.supported_standard_types`，含 `"responses"` 时走原生直通（提取函数 `crates/aigw-core/src/resolver.rs:473`，调用 `resolver.rs:300`、`resolver.rs:393`）
- `raw_params: Value` —— `deployment.rs:36`，解密后的完整 `litellm_params`
- `developer_role_passthrough: Option<bool>` —— `deployment.rs:47-50`（Stage 131 加的 `model_info` 开关，**可作为新 `model_info` 开关的实现模板**）

**config.example.yaml 顶层 section** —— `/Users/kofj/works/projects/github.com/aivpub/aigw/config.example.yaml`：
启用（未注释）：`model_list`（`:45`）、`router_settings`（`:87`）、`litellm_settings`（`:103`）。
注释态（可用）：`general_settings`、`budget_reset`、`cache`、`environment_variables`、`body_archive`。

**新增顶层 section（如 `search_providers:`）需要的改动**：
1. `crates/aigw-core/src/config.rs:30` 的 `AigwConfig` 加字段（约 `:60` 之后），配套新 struct（仿 `CacheConfig`（`config.rs:65-78`）或 `BodyArchiveConfig` 的写法）
2. `crates/aigw-core/src/config_loader.rs` 加启动装载逻辑（仿 `build_router_config`（`config_loader.rs:131`）或 `seed_models_from_config`（`config_loader.rs:54`））
3. `config.example.yaml` 加注释态示例块

**推荐放置位置的判断**：搜索 provider 有两类配置，建议分开：
- **全局 provider 清单 + 密钥**（Tavily / Perplexity 的 base_url + key + 定价）→ 新顶层 config section（如 `search_providers:`）或新 DB 表 + admin API（仿 `proxies` 表模式，`crates/aigw-server/src/routes/proxies.rs`）
- **每个 deployment 是否启用内建搜索 / 用哪个 provider / loop 上限** → `proxy_models.model_info` 的新键（完全复用 `supported_standard_types`（`deployment.rs:56`）/ `developer_role_passthrough`（`deployment.rs:50`）的既有套路，零 migration）

### D2. 出站 HTTP client 抽象

**主网关 client** —— `crates/aigw-core/src/router.rs:535-548`：
```rust
pub fn build_retry_client(&self) -> reqwest_middleware::ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(self.num_retries);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("failed to build reqwest client");
    ClientBuilder::new(client)
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}
```
调用点 `crates/aigw-server/src/routes/chat.rs:1601`。**每请求新建，无共享池**。超时硬编码 600s（`router.rs:542`）。重试 = `reqwest-middleware` + `reqwest-retry` 的 `ExponentialBackoff`，上限 `self.num_retries`（默认 2）。**无 proxy** —— 直连上游。

**proxy-aware client** —— `crates/aigw-core/src/probe.rs:26-33`：
```rust
pub fn build_proxy_client(proxy_url: &str, timeout: Duration) -> Result<reqwest::Client, String>
```
内部 `reqwest::Proxy::all(proxy_url)` + 调用方给定超时。使用方：proxy 探测端点（`crates/aigw-server/src/routes/proxies.rs:483`，10s）、OAuth pipeline（`crates/aigw-core/src/oauth_pipeline.rs:313`，600s）、Claude OAuth 换取（`crates/aigw-core/src/claude_oauth.rs:242`，60s）。

健康检查另有两个 ad-hoc `reqwest::Client::new()`：`crates/aigw-server/src/routes/health.rs:340`（15s）、`health.rs:402`（10s）。

**proxy 配置**：`proxies` 表存加密的 `proxy_url`（`crypto::encrypt_proxy_url`，`crates/aigw-core/src/crypto.rs:205`，NaCl）。CRUD 在 `crates/aigw-server/src/routes/proxies.rs`。`AigwConfig` / `RouterSettings` **没有** proxy 字段 —— proxy 是纯 DB 资源，挂在 OAuth credential 上。

**搜索厂商 client 该复用什么**：
- 模板选 `probe.rs:build_proxy_client`（`probe.rs:26`）—— 一个 `aigw-core` 里的自由函数，入参配置、返回 `reqwest::Client`
- 若需要重试，照搬 `router.rs:build_retry_client`（`router.rs:535`）的 `reqwest-middleware` + `reqwest-retry` 组合
- 若搜索厂商需要走代理出网（国内环境很可能需要），直接用 `build_proxy_client`
- **不要用** `crates/aigw-core/src/provider.rs` 的 `ProviderRegistry`（见 D4，是遗留/stub），也不要用 `crates/aigw-core/src/engine.rs`（那是后台异步任务队列：body archive / budget reset，不是出站 HTTP 抽象）

### D3. 密钥存储与解密

**加密函数** —— `crates/aigw-core/src/crypto.rs`：

| 函数 | 行 | 算法 | 输出格式 |
|---|---|---|---|
| `encrypt_litellm_value` | `crypto.rs:129` | NaCl XSalsa20-Poly1305（SecretBox） | Base64(nonce[24] ‖ ciphertext+tag) |
| **`encrypt_litellm_value_gcm`** | `crypto.rs:178` | AES-256-GCM | **`v2:gcm:`** + Base64(salt[16] ‖ nonce[12] ‖ ciphertext+tag) |
| `encrypt_proxy_url` | `crypto.rs:205` | 委托 `encrypt_litellm_value`（NaCl） | 同上 |

**解密函数**：

| 函数 | 行 | 说明 |
|---|---|---|
| `decrypt_litellm_value` | `crypto.rs:50` | **分发器**：检测 `v2:gcm:` 前缀 → `decrypt_gcm`；否则 NaCl |
| `decrypt_nacl`（私有） | `crypto.rs:69` | XSalsa20-Poly1305；key = `SHA-256(master_key)`（`derive_key`，`crypto.rs:22-29`） |
| `decrypt_gcm`（私有） | `crypto.rs:93` | AES-256-GCM；key = PBKDF2-HMAC-SHA256，**600,000 轮**（`crypto.rs:156-163`） |
| `decrypt_proxy_url` | `crypto.rs:210` | 委托 `decrypt_litellm_value` |
| `decrypt_json_fields` | `crypto.rs:285` | **递归**遍历 `serde_json::Value`，对每个 string 叶子尝试解密，非密文静默跳过 |

**master key 来源**：环境变量 `AIGW_MASTER_KEY`（运行期取自 `state.aigw_master_key`，用例见 `crates/aigw-server/src/routes/proxies.rs:510`），启动时由 CLI 或 `general_settings.master_key` 注入。

**请求时的上游 key 解密**：`resolve_upstream_params` 内 —— `crates/aigw-server/src/routes/chat.rs:347`（整 blob 加密场景，`decrypt_litellm_value`）/ `chat.rs:370`（字段级加密场景，`decrypt_json_fields`）。credential 路径同形 `chat.rs:443-459`。

**新厂商 API key 该怎么存**：放 `proxy_models.litellm_params` JSON 列，或 `credentials.credential_values` JSON 列，用 **`encrypt_litellm_value_gcm`**（`crypto.rs:178`；`crypto.rs:177` 的注释指出新写入应优先 GCM）。取用时 `decrypt_json_fields`（`crypto.rs:285`）会自动处理。**JSON blob 内加新键无需任何 schema 改动。** 若选新建独立 `search_providers` 表，则需 027 migration（三 driver）+ `db.rs` 的 CRUD。

### D4. provider 抽象

`crates/aigw-core/src/provider.rs` —— **没有 trait，是 struct-based registry**：
- `ProviderConfig`：`provider.rs:13`（`base_url`、`api_key`、`routing_strategy`、`allowed_fails`、`cooldown_secs`、`instances`）
- `ProviderInstance`：`provider.rs:33`（`url`、`weight`）
- `ProviderRegistry`：`provider.rs:49`（`providers: HashMap<String, ProviderConfig>`、`model_routing: HashMap<String, String>`）
- `get_provider(model)`：`provider.rs:108`（三步解析：`model_routing` 显式别名 → `/` 前缀 → 原名）
- `select_url`：`provider.rs:127`

**但这个模块是遗留/stub**：活跃请求路径（`chat.rs`）**不用** `ProviderRegistry`，而是走 `crates/aigw-core/src/deployment.rs` + DB `proxy_models` 表 + `Router`。`ProviderRegistry::default_with_env()`（`provider.rs:69`）直接读 `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` 环境变量，是 pre-DB 时期的早期接线。

**真正的 provider 分发**是 adapter 层的二维 match：`select_adapter(ClientProtocol, &ProviderType)` —— `crates/aigw-core/src/adapter.rs:75-90`，加上 `select_responses_adapter` 的 `supported_standard_types` 判断（`adapter.rs:104-113`）。

**搜索 provider 适不适合这个抽象：不适合。** 理由：
1. 活跃分发走 `proxy_models` + `resolve_upstream_params`，不走 `ProviderRegistry`
2. 语义不同：搜索 provider 不接收 OpenAI 格式 chat completions、不返回流式 token
3. `MessageAdapter` trait（`adapter.rs:34-46`）的四个方法（`client_protocol` / `adapt_request` / `adapt_response` / `stream_adapter`）对搜索厂商全部不适用

**建议**：新建独立抽象 —— 一个 `SearchProvider` trait（`query(&self, q: &str, opts) -> Result<SearchResults>`）+ 按厂商的实现 + 新 config section + 可能的新 route module，参照 `body_archive/`（`crates/aigw-core/src/body_archive/` 目录化模块：`config.rs` / `storage.rs` / `writer.rs` / `query.rs` / `cache.rs` / `mod.rs`）的组织方式。

---

## E. 测试基础设施

### E1. Mock BDD 机制

**Taskfile 测试任务** —— `/Users/kofj/works/projects/github.com/aivpub/aigw/Taskfile.yml`：

| task | 行 | 命令 | 关键 env |
|------|-----|------|---------|
| `test` | `:14-18` | `cargo test --workspace` | 无 |
| `bdd` | `:20-24` | `cargo test --test bdd -p aigw-server` | 无（纯 mock） |
| `bdd-real` | `:26-42` | 同上 | `.env` + `OPENAI_API_KEY`、`OPENAI_BASE_URL`、`AIGW_BASE_URL`（默认 `http://localhost:4000`）、`AIGW_MASTER_KEY`、**`AIGW_REAL_API=1`**。**不设 `AIGW_TEST_START_SERVER`，需外部已起 server** |
| `bdd-real-sqlite` | `:44-60` | 同上 | `AIGW_REAL_API=1` + **`AIGW_TEST_DB_DRIVER=sqlite`** + **`AIGW_TEST_START_SERVER=1`** + `AIGW_REAL_MODEL` + `AIGW_UPSTREAM_DB_URL` / `AIGW_UPSTREAM_ENCRYPT_KEY` |
| `bdd-real-pg` | `:62-73` | 同上 | 同上，`AIGW_TEST_DB_DRIVER=postgres` |
| `bdd-real-mysql` | `:75-85` | 同上 | 同上，`AIGW_TEST_DB_DRIVER=mysql`（不传 `AIGW_REAL_MODEL`） |
| `bdd-all` | `:87-92` | 先 mock 再 `AIGW_REAL_API=1` | 不设 `AIGW_TEST_START_SERVER` |
| `test-integration` | `:94-98` | `cargo test --workspace --features integration` | 无 |
| `bdd-coverage` | `:302-309` | `sh scripts/bdd-coverage` | 门禁 60% |

**框架与 World**：
- `cucumber` crate（`cucumber::World` derive）；入口 `#[tokio::main] main()` —— `crates/aigw-server/tests/bdd.rs:96`
- `TestWorld` struct —— `crates/aigw-server/tests/bdd.rs:13-55`（`state: Option<SharedState>`、`master_key`、`last_status`、`last_body`、`last_headers`、`created_keys`、`created_users`、`last_request_credential_name`）
- **串行执行** `max_concurrent_scenarios(1)` —— `bdd.rs:135`（mock upstream 用共享状态）

**feature 文件目录**：`crates/aigw-server/tests/features/`，发现调用 `bdd.rs:138` 的 `.filter_run("tests/features", ...)`。
文件清单：`adapter.feature`、`admin_jobs.feature`、`anthropic_native.feature`、`async_task.feature`、`auth.feature`、`body_archive_read.feature`、`body_archive_write.feature`、`budget_reset.feature`、`cache.feature`、`claude_oauth.feature`、`deleted_list.feature`、`embeddings.feature`、`end_to_end.feature`、`error_handling.feature`、`global.feature`、`health.feature`、`keys_permission.feature`、`keys.feature`、`messages.feature`、`migration.feature`、`model_access.feature`、`models.feature`、`proxies.feature`、`rate_limit.feature`、**`responses.feature`**、`router_settings.feature`、`router.feature`、`soft_budget.feature`、`spend_aggregation.feature`、`spend_end_user.feature`、`spend.feature` + `real/` 子目录。

### E2. Mock upstream —— 以及如何 mock 搜索厂商

`crates/aigw-server/tests/bdd_support/mock_upstream.rs`：

`MockUpstream` struct（`mock_upstream.rs:189`）包一个绑在临时端口（`TcpListener::bind("127.0.0.1:0")`）的 axum server，持 `Arc<MockState>`，内含两个 `Mutex` 集合：
- `requests: Arc<Mutex<Vec<RecordedRequest>>>` —— 记录每个入站请求（path / headers / body）
- `responses: Arc<Mutex<HashMap<String, MockResponse>>>` —— **按 path 为键**的罐头响应

**路由注册** —— `crates/aigw-server/tests/bdd_support/mock_upstream.rs:202-210`：
```rust
        let app = Router::new()
            .route("/v1/chat/completions", post(openai_handler))
            .route("/v1/messages", post(claude_handler))
            .route("/v1/responses", post(responses_handler))
            .route("/v1/embeddings", post(embeddings_handler))
            .route("/api/organizations", get(oauth_orgs_handler))
            .route("/v1/oauth/{org}/authorize", post(oauth_authorize_handler))
            .route("/v1/oauth/token", post(oauth_token_handler))
            .with_state(route_state);
```

**注册机制（按 PATH，不按 model）** —— `mock_upstream.rs:145-149`：
```rust
    pub fn set_response(&self, path: &str, response: MockResponse) {
        self.responses
            .lock()
            .unwrap()
            .insert(path.to_string(), response);
    }
```
便捷方法 `MockUpstream::set_response(path, status, body)` —— `mock_upstream.rs:240-252`。
其他变体：
- `set_sse_body(path, sse_body: Vec<u8>)` —— `mock_upstream.rs:257`（整段 SSE 字节流，流式场景）
- `set_sse_chunks(path, chunks: Vec<Vec<u8>>)` —— `mock_upstream.rs:278`（多 chunk SSE，每个元素作为独立 stream item 投递）
- `set_response_first_n(path, status, body, n)` —— `mock_upstream.rs:298`（一次性响应，命中 N 次后自动移除，用于 401→refresh 重试场景）

**要 mock 搜索厂商怎么做**：
1. 在 `mock_upstream.rs:202` 的 `Router::new()` 链上加路由（如 `.route("/search", post(search_handler))`）
2. 写对应 handler（照抄 `openai_handler` 的结构：先 record request，再查 `responses` map）
3. 场景里用现成的 `MockState::set_response(path, response)` 注册罐头结果
4. **零 model 路由**：分发纯按 HTTP path，所以 Tavily / Perplexity 可以各占一个 path，不需要额外机制
5. 多轮 loop 场景可用 `set_sse_chunks`（`mock_upstream.rs:278`）和 `set_response_first_n`（`mock_upstream.rs:298`）组合模拟「第一轮返工具调用、第二轮返最终答案」

**real-API 门禁** —— `crates/aigw-server/tests/bdd.rs:133-164`：
```rust
let real_api_mode = std::env::var("AIGW_REAL_API").as_deref() == Ok("1");
TestWorld::cucumber()
    ...
    .filter_run("tests/features", move |feature, _rule, scenario| {
        if scenario.tags.iter().any(|t| t == "skip") {
            return false;   // @skip 永不执行
        }
        if real_api_mode {
            feature.tags.iter().chain(scenario.tags.iter()).any(|t| t == "real_api")
        } else {
            let tags = ...;
            !tags.contains(&"needs_upstream_db")  // mock 模式跳过 @needs_upstream_db
        }
    })
```
- mock 模式：除 `@skip` 和 `@needs_upstream_db` 外全跑
- `AIGW_REAL_API=1`：**仅**带 `@real_api`（feature 级或 scenario 级）的场景跑

server 自动启动独立控制 —— `bdd.rs:111-123`：仅当 `AIGW_TEST_START_SERVER == "1"` **且** `TestDatabaseManager`（按 `AIGW_TEST_DB_DRIVER` 建）已创建时才调 `ServerGuard::start`。

### E3. 单测约定与数量

约定：**inline `#[cfg(test)]` mod**（标准 Rust 风格）。`crates/aigw-server/tests/` 下只有 BDD 集成测试（`bdd.rs`、`bdd_steps/`、`bdd_support/`、`features/`）加 `artifact_tests.rs`、`deployment_files_test.rs`、`dockerfile_test.rs`。

| crate | `#[test]` / `#[tokio::test]` 标注数（grep 近似） |
|-------|----------------------------------|
| `aigw-core` | ~564（约 40 个带测试的源文件） |
| `aigw-server` | ~181（约 22 个源文件） |
| `aigw-migrate` | ~47（约 11 个源文件） |

roadmap 口径（Stage 132 时）：「aigw-core 527 UT」「aigw-server 157 UT」；commit 8cf8c12 的验证记录：「task test aigw-core 530 passed；task bdd 285 场景（272 passed / 13 skipped）/ 1453 steps」。grep 数高于执行数（标注级 vs 执行级计数差异）。
测试最密集的单文件：`crates/aigw-core/src/adapter.rs`（~105 个标注）、`crates/aigw-core/src/db.rs`（~77 个）。

---

## F. RDD 流程产物

### F1. stage-roadmap.md 当前状态

`docs/stages/stage-roadmap.md`：
- 最后更新：2026-10-05（`stage-roadmap.md:4`）
- Phase 0-51 状态：`状态: **134/134 Stages 交付（ALL STAGES COMPLETE）**`（`stage-roadmap.md:11`）
- Phase 52：`✅ 完成（Stage 131-132，总进度 136）`（`stage-roadmap.md:12`）
- 进度条表格：`stage-roadmap.md:17-62`（ASCII bar，每 Phase 一行：填充条 + 百分比 + stage 数 + 标签），如：
  ```
  Phase 51:   ████████████████████ 100% (5/5 Stages) ✅ Claude OAuth 订阅反代 (Stage 126-130)  — **134/134 ALL STAGES COMPLETE**
  ```
- 每个 Phase 明细：H2 标题（带完成标记）+ 背景段落 + 表格 `| Stage | 状态 | 目标 | 类型 | 预估 |` + 依赖说明 + 关键决策（ADR 引用）

> **编号提示**：`stage-133.md` 文件**不存在**，但 Stage 133 编号**已被 commit c7cd1f4 使用**（「feat(responses): 原生直通 + 补齐 codex 合规 SSE 事件序列（Phase 52 / Stage 133）」）。因此新 stage 文档应为 **Stage 134**，且需补写 `stage-133.md` 或在 roadmap 中明确 133 的归属，否则编号出现文档空洞。

### F2. Stage 文档模板（stage-131 / stage-132 实证）

**元数据块**（文件顶部，**非 YAML front-matter，是 inline 粗体行**）：
```markdown
# Stage NNN: [标题]（Phase XX）

**所属**: Phase XX（描述）
**预估**: Xh（内容描述）
**依赖**: 描述 / 无
**状态**: ✅ 完成（内容）| 进行中
```
实证锚点：`docs/stages/stage-131.md:1-6`、`docs/stages/stage-132.md:1-6`。
stage-131 状态行原文（`stage-131.md:6`）：`**状态**: ✅ 完成（代码 + UT + BDD + 门禁全绿；未提交/未部署）`
stage-132 状态行原文（`stage-132.md:6`）：`**状态**: ✅ 完成（代码 + UT + BDD + 门禁全绿 + 真实端到端；未提交/未部署）`

**章节顺序（stage-131 为完整范式）**，锚点 `docs/stages/stage-131.md`：

| 章节 | 行 | 说明 |
|------|-----|------|
| `## 0. 实施结果（日期）` | `:10` | **仅 stage 完成后出现**，含汇总表 |
| `## 1. 目标` | `:33` | 下含 `### 验收标准`（`:44`）、`### 明确不做（边界）`（`:52`） |
| `## 2. 现状证据` | `:62` | 编号子节 `### 2.1 缺口 A — …`（`:66`）、`2.2`（`:74`）、`2.3`（`:87`）、`2.4`（`:100`）、`2.5`（`:111`）、`2.6 旁证`（`:122`） |
| `## 3. 方案` | `:130` | 编号子节 `### 3.1`（`:132`）含 `#### 3.1.1`（`:168`）、`#### 3.1.2`（`:183`）；`### 3.2`（`:193`）含 `#### 3.2.1/2/3`（`:197`/`:216`/`:232`）；`### 3.3`（`:243`）；`### 3.4`（`:260`） |
| `## 4. TDD 计划` | `:276` | `### 4.1 适配器级 UT`（`:278`）、`### 4.2 BDD`（`:299`）、`### 4.3 集成验证（人工）`（`:317`） |
| `## 5. 变更清单` | `:323` | 表格 `| 文件 | 改动 |` |
| `## 6. 回归验证` | `:344` | 编号命令列表 |
| `## 7. 门禁` | `:356` | `- [x]` / `- [ ]` checklist |
| `## 8. 风险与遗留` | `:367` | `### 8.1 风险`（`:369`，风险表）、`### 8.2 遗留（本 Stage 不做，登记 TD-017）`（`:378`） |

stage-132 用精简结构（小 stage），锚点 `docs/stages/stage-132.md`：
`## 1. 目标`（`:10`）、`## 2. 现象与证据`（`:16`）、`## 3. 方案`（`:33`，含 `### 3.1`（`:35`）/`### 3.2`（`:51`）/`### 3.3 设计参考`（`:57`））、`## 4. TDD`（`:61`）、`## 5. 回归验证`（`:78`）、`## 6. 门禁`（`:85`）、`## 7. 不做（边界）`（`:93`）。

**门禁（gate）典型条目**（stage-131 §7 / stage-132 §6）：
1. N 个新 UT 先 fail 后 pass（TDD 红绿）
2. Fixture / 回归 UT 通过
3. `task test` / `task bdd` / `task fmt` / `task lint` 全绿
4. BDD 场景改写/新增通过
5. `docs/11-next-steps.md` + `stage-roadmap.md` 回写
6. git commit（精确 add；`--signoff`）

**可直接套用的新 stage 骨架**：
```markdown
# Stage NNN: [主题]（Phase XX）

**所属**: Phase XX（描述）
**预估**: Xh（描述）
**依赖**: Stage NNN-1（描述）或 无
**状态**: 进行中

---

## 1. 目标

[主目标描述]

### 验收标准

- [ ] 验收项 1

### 明确不做（边界）

- 不做 X

---

## 2. 现状证据

### 2.1 缺口 A — 描述

[代码定位 path:line，实测证据]

---

## 3. 方案

### 3.1 子方案

[设计决策]

---

## 4. TDD 计划

### 4.1 适配器级 UT

| UT | 输入 | 期望 |
|----|------|------|

### 4.2 BDD

[feature 文件名 + 场景列表]

### 4.3 集成验证

---

## 5. 变更清单

| 文件 | 改动 |
|------|------|

---

## 6. 回归验证

1. `task test` 全绿
2. `task bdd` 全绿
3. `task fmt` / `task lint` green

---

## 7. 门禁

- [ ] N 个新 UT 先 fail 后 pass（TDD 红绿）
- [ ] `task test` / `task bdd` / `task fmt` / `task lint` 全绿
- [ ] `docs/11-next-steps.md` + `stage-roadmap.md` 回写
- [ ] git commit（精确 add；`--signoff`）

---

## 8. 风险与遗留

### 8.1 风险

| 风险 | 缓解 |
|------|------|

### 8.2 遗留（登记 TD）

- **TD-XXX**: 描述
```

### F3. 技术债 TD 条目格式与 web search 相关条目

`docs/12-technical-debt.md` 的 TD 条目格式：
```markdown
### TD-NNN: 标题 [✅ Resolved DATE if done]

- **Date**: YYYY-MM-DD
- **Priority**: P0/P1/P2/P3
- **Source**: 来源引用（可选）
- **Description**: 叙述
- **Impact**: 影响
- **Resolution**: ✅ 已实现（细节）| 待办文字
- **Target Phase**: Phase/Stage 引用 | 无固定排期
```
子项用表格：`| Sub-ID | 条目 | 优先级 | 描述 |`。

**TD-017 头部** —— `docs/12-technical-debt.md:197`：
```markdown
### TD-017: Codex Responses 多轮适配遗留项（Stage 131 后续）
```
- **Date**: 2026-10-05
- **Priority**: P2
- **Source**: Stage 131 调研（`docs/research/2026-10-05-codex-responses-bridge-gap.md`）+ 生产网关实测

**TD-017c 逐字引用** —— `docs/12-technical-debt.md:208`：

> | TD-017c | 服务端工具真实支持（`web_search` 等） | P3 | Stage 131 对无 chat 对应能力的服务端工具（`web_search` / `web_search_preview` / `code_interpreter` / `computer_use` / `image_generation` / `shell` / `mcp`）**丢弃 + 告警**。若产品需要**真实搜索能力**，需内建搜索执行（搜索后端 + 多轮 agentic loop + SSE 交互），是独立子系统——参考 litellm `WebSearchInterceptionLogger` + `max_agentic_loops`（`types/integrations/websearch_interception.py`）。调研：`docs/research/2026-10-05-websearch-server-tool-support.md` §3.2。**注意**：litellm 的「派生 `web_search_options`」路线在本环境**无实效**（参数被收下但不执行搜索，实测）。 |

**TD-017b 逐字引用（相邻，涉及 tool_choice，与本特性有交互）** —— `docs/12-technical-debt.md:207`：

> | TD-017b | `tool_choice` 的 `{type:"namespace"}` 形态 | P3 | Codex 会发 `tool_choice: {type:"function", name, namespace}`（带 namespace 限定）；Stage 131 只处理 `{type:"function"\|"namespace", name}` 的具名选择项随工具进出的清理，未做命名空间字段的剥离与降级。 |

同表其他条目：`TD-017a`（✅ Resolved 2026-10-05 / Stage 132，`input[].type` item 分派）、`TD-017d`（P3，Phase 41 适配器 UT 缺口剩余，含「streaming SSE 事件映射」）、`TD-017e`（✅ Resolved 2026-10-05 / Stage 132，Codex 多轮端到端验证）、`TD-017f`（P3，`developer_role_passthrough` 无自动嗅探）。

活跃条目中除 TD-017 外无其他提及「搜索」的技术债。

### F4. docs/11-next-steps.md 当前状态与既有设计笔记

`docs/11-next-steps.md`：Phase 52 完成，Stage 131-132 均于 2026-10-05 完成，总进度 136。

web search 相关提及：
- `docs/11-next-steps.md:17`：「…Codex 原生发送 namespace（multi_agent_v1，含 5 个嵌套 function）+ web_search + role="developer" 消息。」
- `docs/11-next-steps.md:33`：「服务端工具（web_search 等）处置定案：调研 `docs/research/2026-10-05-websearch-server-tool-support.md`（litellm 派生 web_search_options + 内建 agentic loop 子系统 / sub2api 丢弃 / new-api 透传）。本环境实测：web_search/web_search_preview/code_interpreter/computer_use_preview/mcp 透传全部 400；web_search_options 参数被收下（200）但不执行搜索（模型回复「我无法联网」）。→ 选「丢弃 + 告警」（与派生实效相同，但诚实且便于统计）；内建真实搜索列为独立 Phase（需搜索后端 + 多轮 agentic loop + SSE 交互）。」
- `docs/11-next-steps.md:48`：「遗留: tool_choice 的 {type:"namespace"} 形态（TD-017b）；内建搜索执行（TD-017c）；streaming SSE 事件映射 UT（TD-017d 剩余）。」

**已存在、不要重复做的调研**：

`docs/research/2026-10-05-websearch-server-tool-support.md` 是本特性的**权威前置设计参考**，已覆盖：
- `§1.1`（`:24`）litellm 的协议转换路线（派生 `web_search_options`）
- **`§1.2`（`:53`）litellm 真实搜索执行子系统的完整组件清单** —— `WebSearchInterceptionLogger`、搜索后端（`llms/tavily/search`、`llms/perplexity/search`、`llms/searchapi`）、`proxy/search_endpoints/search_tool_registry.py`、`search_tool_management.py`、`proxy/hooks/max_iterations_limiter.py`、`AnthropicServerToolUseBlock`（`server_tool_use` + `web_search_tool_result` 成对、共享 `srvtoolu_` 前缀 id）。配置形态：`litellm_settings.websearch_interception_params.{enabled_providers, search_tool_name, max_agentic_loops}`
- `§1.3`（`:83`）sub2api 静默丢弃（`chatcompletions_responses_bridge.go:851-853`）
- `§1.4`（`:100`）new-api 透传
- `§2`（`:119`）生产网关（9.135.87.221:4001）实测：工具形态全 400、`web_search_options` 收下但不搜索
- `§3.1`（`:160`）三条路线在本环境的实际效果
- **`§3.2`（`:171`）内建搜索可行性评估表** —— 逐项标注 aigw 现状（搜索后端「零」、多轮 agentic loop「零」、流式 SSE 与循环交互「需新增」、双向工具协议转换「部分可复用」、loop 上限 + 计费归集「需新增」、工具选择策略「需设计」），结论「这是一个完整子系统，不是桥接修复的一部分，建议作为独立 Phase 立项」
- `§3.3`（`:186`）与既有边界的一致性 —— 指出 `docs/research/2026-08-04-openai-responses-api-support.md:292` 曾定「内置工具上游原生支持，网关不实现」，本次实测推翻该前提，边界需重估
- `§4`（`:196`）引用表（litellm 工具分派 `litellm/responses/litellm_completion_transformation/transformation.py:1851-1912` 等）

另有 `docs/research/2026-10-05-codex-responses-bridge-gap.md`（Codex 桥接缺口分析，提及 `web_search` 作为必须丢弃的工具类型）。

**本文档的定位**：前者是「业界怎么做 + 要不要做」的决策调研；本文档是「aigw 代码里具体在哪改」的落地基线，两者互补不重叠。

---

## 实现难点与风险

以下是本次审计发现的 5 个最难集成点，按难度 × 风险排序。

### 难点 1：agentic loop 要从零引入，且与现有流式架构正交冲突（最难）

aigw 是严格单发代理 —— `chat.rs:1743` 是整条链路唯一的上游 `.send()`，全仓 handler/adapter/oauth_pipeline 无任何 `loop {}`。现有的三种「重试」全都不是对话轮次：`fallback_order`（`router.rs:494`）只排序候选、`build_retry_client`（`router.rs:535`）在传输层重发同一 body、OAuth 401 重试（`oauth_pipeline.rs:334-362`）只重发一次。

真正的困难在于**循环与流式的交互**。当前流式路径是「一个 `tokio::spawn` 任务从 `upstream_resp.bytes_stream()` 单向抽取 → 经 `StreamAdapter::next` 转换 → `tx.send()` 推给客户端」（`responses.rs:1034-1079`）。要在这个结构里插入「检测到搜索工具调用 → 暂停转发 → 执行搜索 → 构造新请求 → 再发一次上游 → 继续转发」，等于把单向管道改造成状态机驱动的多阶段管道。而 `StreamAdapter::next` 的契约（`adapter.rs:49-58`：「不是队列，不能 drain-loop」）已经两次被违反并导致线上 bug —— commit c3f360c「流式桥接死循环导致请求永不结束」、commit 4bd85c7「在 response.completed 之前就发 [DONE]，客户端提前断流」。在这个已被踩过两次坑的地方加一层循环，回归风险极高。

额外约束：`ResponsesToChatCompletionsStream` 的 `seq`（`adapter.rs:2940`）和 `output_index_for_tool`（`adapter.rs:2989-2991`，`TEXT_OUTPUT_INDEX + 1 + idx`）是单调/线性分配的。跨轮次复用同一个 stream adapter 实例时，第二轮的 `response.created` 不能重发、item index 不能与第一轮撞号 —— 需要重新设计编号方案。

### 难点 2：按次计费是全新计价维度，且预算模型不支持请求中途扣费

**计价侧**：全仓零按次计价能力。`calc_spend`（`chat.rs:104-123`）的八个参数全是 token 相关；`ModalPricing`（`models.rs:134-142`）尽管命名含 "modal"，单位仍是 USD/1M **token**；唯一看似相关的 `calc_spend_modal`（`chat.rs:139-158`）是 `#[allow(dead_code)]`。要支持「$X / 1000 次搜索」，需新增价格字段 + 新计算函数 + 决定费用归集位置。

**更难的是预算时序**。现有模型是「请求前 `check_entity`（`budget.rs:45-57`）读 `spend` 比 `max_budget`，请求后 `increment_*_spend`（`db.rs:365-368`）异步累加」—— 一个最终一致模型。而搜索发生在**请求中途**，单个请求可能触发 N 次搜索。预算在 loop 开始前检查过一次，循环中途超预算无法拦截。要么在 loop 每轮加预算复检（引入额外 DB 往返 × N，且 `increment_*_spend` 是 fire-and-forget 异步的，复检时读到的 `spend` 未必包含本请求已发生的搜索费），要么接受超支并用 `max_agentic_loops` 上限兜底。

**第三层**：`DailySpendLog`（`models.rs:198-218`）的日聚合复合 key 含 `model` / `custom_llm_provider`（`daily_spend_queue.rs:39-78` 的聚合逻辑），搜索调用这两个字段填什么必须先定义，否则 `/spend/models`、`/global/spend/providers`、`/global/spend/keys/rankings` 的口径全部失真。

### 难点 3：Anthropic `/v1/messages` 路径在反序列化阶段就硬失败（不是优雅丢弃）

`ClaudeToolDef`（`models.rs:1065-1071`）的 `input_schema: serde_json::Value` 是**非 Option 必填字段**，struct 也**没有 `type` 字段**，且全文件无 `#[serde(flatten)]` 兜底（`models.rs` grep `flatten` 零命中）。Anthropic 服务端工具的线上形态 `{"type":"web_search_20250305","name":"web_search","max_uses":5}` 缺 `input_schema`，于是：

- `AnthropicToOpenAI::adapt_request`（`adapter.rs:230-232`）的 `serde_json::from_value::<ClaudeMessageRequest>` **直接失败** → `AdapterError::Parse` → `chat.rs:1534-1540` 返回 500 `adapter_error`。**这与 Responses 侧「丢弃 + 告警」的优雅降级行为完全不同，是整条请求 500。**
- `AnthropicPassthrough::adapt_request`（`adapter.rs:1618-1661`）即便能解析，也因 round-trip（`from_value` → `to_value`）**丢掉所有未声明字段**。
- 工具转换本体（`adapter.rs:922-934`）无条件把每个工具打成 `tool_type: "function"`，零服务端工具分支。

落地含义：Anthropic 侧要支持内建搜索，必须先改 `models.rs` 的类型定义（加 `type: Option<String>` + `input_schema` 改 Option，或整体改 untagged enum）。这会波及所有现有 Anthropic 路径的 UT（`adapter.rs` 约 105 个测试标注里相当一部分涉及 Claude 结构），是一次有广泛回归面的类型改造 —— 而 Responses 侧完全不需要这一步（那边全程操作 `serde_json::Value`）。

### 难点 4：SpendLog 无父子关系，且流式日志重建逻辑会吞掉网关自产的记录

**结构缺口**：`spend_logs` 37 列中无 `parent_call_id` / `parent_request_id`，全部 migration 无自引用 FK。可用的「挂载点」只有判别列 `call_type`（`models.rs:148`，现仅 `"completion"` / `"responses"`，schema 无约束可扩展）、自由 JSON `metadata`（`models.rs:164`）、松散分组 `session_id`（`models.rs:174`）、当前全路径硬编码 `None` 的 `agent_id`（`models.rs:177`）。要做 DB 层父子关联，需 027 migration（三 driver 各一份）+ 改 `db.rs`（10452 行）里**三份手写 INSERT SQL**（`db.rs:2317` SQLite / `db.rs:2752` MySQL / `db.rs:3201` Postgres）。

**更隐蔽的陷阱**：commit 8cf8c12 刚修复的 bug 是「流式 SpendLog 只存最后一个 chunk」—— 当前 `assembled_response` 的重建逻辑（`responses.rs:1096` 起）是**遍历 `chunk_jsons` 累加**，而 `chunk_jsons` 只收集**上游发来的** chunk（`responses.rs:1033-1056` 的收集循环）。网关自己执行的搜索调用**不会出现在任何上游 chunk 里**，因此若沿用现有重建逻辑，搜索记录在流式路径上会**凭空消失**，重现「落库内容为空」的同类故障。必须显式把搜索调用注入 `assembled_response`，或新开独立行。

**前端连带改动**：`crates/aigw-frontend/src/pages/spend-logs/index.tsx:1413-1454` 的表格列（`model` / `prompt_tokens` / `completion_tokens` / `ttft_ms`）对搜索行全为空或 0，需容错渲染；`status` 筛选的三态（`success`/`failure`/`streaming`，`index.tsx` 筛选区）需补搜索语义；详情抽屉（`index.tsx:631`）的 token/cache 区块对搜索行无意义需条件渲染；且必须加 `call_type` 筛选器，否则搜索行污染 LLM 调用列表。

### 难点 5：搜索 provider 无任何可复用抽象，且需同时跨越三个配置层

**provider 抽象缺位**：`crates/aigw-core/src/provider.rs` 的 `ProviderRegistry`（`provider.rs:49`）是遗留 stub —— 活跃请求路径（`chat.rs`）完全不用它，而是走 `deployment.rs` + DB `proxy_models` + `Router`；它的 `default_with_env()`（`provider.rs:69`）还在直读 `OPENAI_API_KEY` 环境变量。真正的 provider 分发是 `select_adapter` 的二维 match（`adapter.rs:75-90`），而 `MessageAdapter` trait 的四个方法（`adapter.rs:34-46`：`client_protocol` / `adapt_request` / `adapt_response` / `stream_adapter`）对搜索厂商**全部不适用**。因此必须新建独立抽象，无现成 trait 可实现。

**HTTP client 也无共享基础设施**：`build_retry_client`（`router.rs:535-548`）每请求新建 `reqwest::Client`（无池化）、超时硬编码 600s、无 proxy 支持；proxy-aware 的 `build_proxy_client`（`probe.rs:26-33`）是另一个独立自由函数，无重试。搜索厂商很可能**同时需要代理出网和重试** —— 这两个能力当前分散在两个互不相干的函数里，需要新写一个组合版。

**配置要跨三层**：
1. **全局 provider 清单 + 密钥 + 定价** → 需在 `AigwConfig`（`config.rs:30`）加顶层 section（仿 `CacheConfig`（`config.rs:65-78`）），并在 `config_loader.rs` 加装载逻辑（仿 `build_router_config`（`config_loader.rs:131`））；或新建 DB 表 + admin API（仿 `proxies` 表 + `routes/proxies.rs`，972 行）
2. **每 deployment 的启用开关 / loop 上限** → `proxy_models.model_info` 新键（可零 migration 复用 `supported_standard_types`（`deployment.rs:56`）/ `developer_role_passthrough`（`deployment.rs:50`）的套路），但需同步改 `resolver.rs` 的提取函数（`resolver.rs:473` 附近）和 `Deployment` struct
3. **密钥加密** → 用 `crypto::encrypt_litellm_value_gcm`（`crypto.rs:178`，`v2:gcm:` 前缀 + PBKDF2 600k 轮），解密经 `decrypt_json_fields`（`crypto.rs:285`）自动处理；master key 来自 `AIGW_MASTER_KEY`

注意 `config_loader.rs:28` 的既有先例：`litellm_settings` 被解析但**刻意未接线** —— 新 config section 若只加结构不加运行期消费，会变成同类死配置。

---

## 附：关键锚点速查表

| 要做的事 | 去哪改 |
|---------|--------|
| 不再丢弃 `web_search` 工具 | `crates/aigw-core/src/adapter.rs:2553-2559`（`other =>` 分支） |
| 让上一轮 `web_search_call` 历史可回放 | `crates/aigw-core/src/adapter.rs:2210-2216` |
| 合成 `web_search_call` SSE 事件 | `crates/aigw-core/src/adapter.rs:2979-2987`（`sse` 原语）+ 仿 `adapter.rs:3026-3049`（`ensure_tool_item`） |
| 引入 agentic loop | `crates/aigw-server/src/routes/responses.rs:1034-1079`（流式）/ `responses.rs:714`、`crates/aigw-server/src/routes/chat.rs:1743`（非流式） |
| Anthropic 侧收下服务端工具 | `crates/aigw-core/src/models.rs:1065-1071`（`ClaudeToolDef`）+ `crates/aigw-core/src/adapter.rs:922-934` |
| 新增按次计价 | `crates/aigw-server/src/routes/chat.rs:104-123`（`calc_spend`）+ `crates/aigw-core/src/deployment.rs:27-61`（价格字段）+ `crates/aigw-server/src/routes/chat.rs:59-96`（`extract_pricing`） |
| 搜索费计入预算 | `crates/aigw-server/src/routes/chat.rs:2676-2684`（increment 四连）+ `crates/aigw-core/src/budget.rs:45-57`（检查） |
| 搜索调用落日志 | `crates/aigw-core/src/db.rs:3621`（`insert_spend_log`）+ `crates/aigw-core/src/models.rs:146`（struct）+ 可选 027 migration |
| 前端展示搜索调用 | `crates/aigw-frontend/src/pages/spend-logs/index.tsx:1413-1454`（表格列）/ `index.tsx:631`（详情抽屉） |
| 新增搜索 provider 配置 | `crates/aigw-core/src/config.rs:30`（`AigwConfig`）+ `crates/aigw-core/src/config_loader.rs:54`/`:131` |
| 搜索厂商密钥加解密 | `crates/aigw-core/src/crypto.rs:178`（GCM 加密）/ `crypto.rs:285`（递归解密） |
| 搜索厂商 HTTP client | 仿 `crates/aigw-core/src/probe.rs:26-33` + `crates/aigw-core/src/router.rs:535-548` |
| Mock 搜索厂商（测试） | `crates/aigw-server/tests/bdd_support/mock_upstream.rs:202-210`（加路由）+ `mock_upstream.rs:145-149`（注册响应） |
| 改写现有 BDD 断言 | `crates/aigw-server/tests/features/responses.feature:105-127` + `crates/aigw-server/tests/bdd_steps/responses_steps.rs:381-407` |
