@mock
Feature: OpenAI Responses API Passthrough — /v1/responses

  Scenario: Non-streaming /v1/responses passthrough
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-user" 已生成
    When 使用 key "resp-user" 发送 POST /v1/responses 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"
    And 响应 JSON 中 "output" 数组长度大于 0
    And mock 上游收到请求

  Scenario: Streaming /v1/responses SSE passthrough
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-stream-user" 已生成
    When 使用 key "resp-stream-user" 发送 POST /v1/responses 流式请求
    Then 响应状态码为 200
    And 响应 Content-Type 包含 "text/event-stream"
    And mock 上游收到请求

  Scenario: /v1/responses 流式响应包含 Responses SSE 事件
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-stream-events" 已生成
    When 使用 key "resp-stream-events" 发送 POST /v1/responses 流式请求
    Then 响应状态码为 200
    And 响应原始流包含 "response.created" 事件
    And 响应原始流包含 "response.output_text.delta" 事件
    And 响应原始流包含 "response.completed" 事件

  Scenario: /v1/responses 流式请求在有限时间内完成（回归：桥接转换不得死循环）
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And mock 上游 chat 返回多帧流式响应
    And 一个普通 key "resp-stream-term" 已生成
    When 使用 key "resp-stream-term" 发送 POST /v1/responses 流式请求
    Then 响应状态码为 200
    And 响应原始流包含 "response.output_text.delta" 事件
    And 响应原始流包含 "response.completed" 事件
    And 响应原始流中 "response.completed" 出现在 "data: [DONE]" 之前

  Scenario: /v1/responses with input string (not array)
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-str-user" 已生成
    When 使用 key "resp-str-user" 发送 POST /v1/responses 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"

  Scenario: /v1/responses missing model returns 400
    Given 一个普通 key "resp-no-model" 已生成
    When 使用 key "resp-no-model" 发送 POST /v1/responses 请求不带 model
    Then 响应状态码为 400
    And 响应 JSON "error.type" 为 "invalid_request_error"
    And 响应 JSON "error.message" 包含 "model"

  Scenario: /v1/responses missing input returns 400
    Given 一个普通 key "resp-no-input" 已生成
    When 使用 key "resp-no-input" 发送 POST /v1/responses 请求不带 input
    Then 响应状态码为 400
    And 响应 JSON "error.type" 为 "invalid_request_error"
    And 响应 JSON "error.message" 包含 "input"

  Scenario: /v1/responses spend log is recorded with usage
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-spend-user" 已生成
    When 使用 key "resp-spend-user" 发送 POST /v1/responses 请求
    Then 响应状态码为 200
    And SpendLog 中最近一条记录的 call_id 非空
    And SpendLog 中最近一条记录的 prompt_tokens 大于 0
    And SpendLog 中最近一条记录的 completion_tokens 大于 0

  # ── TD-006: 客户端从响应头直取 call_id 对账 ──

  Scenario: 响应头 x-call-id 回写网关调用 ID（TD-006）
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-callid" 已生成
    When 使用 key "resp-callid" 发送 POST /v1/responses 请求
    Then 响应状态码为 200
    And 响应头包含 x-call-id 且匹配 SpendLog call_id

  # ── Stage 102: Bridge mode scenarios ──

  Scenario: /v1/responses bridge with instructions
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-instr" 已生成
    When 使用 key "resp-instr" 发送带 instructions 的 /v1/responses 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"

  Scenario: /v1/responses bridge with function tools
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-tools" 已生成
    When 使用 key "resp-tools" 发送带 function tools 的 /v1/responses 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"

  Scenario: /v1/responses bridge web_search tool dropped
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-ws" 已生成
    When 使用 key "resp-ws" 发送带 web_search_preview tool 的 /v1/responses 请求
    Then 响应状态码为 200
    And 上游收到的 tools 不含 "web_search_preview"

  Scenario: /v1/responses bridge tool call in response
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-tc" 已生成
    When 使用 key "resp-tc" 发送带 function tools 的 /v1/responses 请求含工具调用响应
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"

  Scenario: /v1/responses bridge code_interpreter tool dropped
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-ci" 已生成
    When 使用 key "resp-ci" 发送带 code_interpreter tool 的 /v1/responses 请求
    Then 响应状态码为 200
    And 上游收到的 tools 不含 "code_interpreter"

  Scenario: /v1/responses bridge namespace tool flattened
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-ns" 已生成
    When 使用 key "resp-ns" 发送带 namespace tool 的 /v1/responses 请求
    Then 响应状态码为 200
    And 上游收到的 tools 含 "multi_agent_v1__spawn_agent"

  Scenario: /v1/responses bridge developer role mapped
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-dev" 已生成
    When 使用 key "resp-dev" 发送带 developer role 的 /v1/responses 请求
    Then 响应状态码为 200
    And 上游收到的 messages 不含 role "developer"

  Scenario: /v1/responses bridge developer passthrough
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游 且 developer_role_passthrough 为 true
    And 一个普通 key "resp-devpass" 已生成
    When 使用 key "resp-devpass" 发送带 developer role 的 /v1/responses 请求
    Then 响应状态码为 200
    And 上游收到的 messages 含 role "developer"

  Scenario: /v1/responses bridge codex-shaped request
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-codex" 已生成
    When 使用 key "resp-codex" 发送 Codex 形状的 /v1/responses 请求
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"

  Scenario: /v1/responses bridge multi-turn tool history
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-mturn" 已生成
    When 使用 key "resp-mturn" 发送带 tool 历史的 /v1/responses 请求
    Then 响应状态码为 200
    And 上游收到的 messages 含 role "tool"
    And 上游收到的 messages 含 role "assistant"
