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

  Scenario: /v1/responses bridge web_search tool rejected
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-ws" 已生成
    When 使用 key "resp-ws" 发送带 web_search_preview tool 的 /v1/responses 请求
    Then 响应状态码为 400
    And 响应 JSON "error.message" 包含 "web_search_preview"
    And 响应 JSON "error.message" 包含 "not supported"

  Scenario: /v1/responses bridge tool call in response
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-tc" 已生成
    When 使用 key "resp-tc" 发送带 function tools 的 /v1/responses 请求含工具调用响应
    Then 响应状态码为 200
    And 响应 JSON 中 "object" 为 "response"

  Scenario: /v1/responses bridge code_interpreter tool rejected
    Given mock 上游已启动
    And 已配置 model "gpt-4o" 指向 mock 上游
    And 一个普通 key "resp-ci" 已生成
    When 使用 key "resp-ci" 发送带 code_interpreter tool 的 /v1/responses 请求
    Then 响应状态码为 400
    And 响应 JSON "error.message" 包含 "code_interpreter"
