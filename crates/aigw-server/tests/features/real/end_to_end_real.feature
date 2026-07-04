@real_api
Feature: 真实 API 端到端代理验证

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: OpenAI SDK 调用 aigw /v1/chat/completions 返回成功
    Given 通过 API 创建普通 key "real-openai-user"
    When 使用 key "real-openai-user" 发送 POST /chat/completions 请求到真实上游
    Then 响应状态码为 200
    And 响应包含 choices[0].message.content

  Scenario: 真实 API 调用记录写入 spend_logs
    Given 通过 API 创建普通 key "real-spend-user"
    When 使用 key "real-spend-user" 发送 POST /chat/completions 请求到真实上游
    Then 响应状态码为 200
    And /spend/logs 包含本次调用记录
    And 记录的 tokens > 0

  Scenario: 缺少 API key 时返回 401
    When 使用 invalid key 发送 POST /chat/completions 请求到真实上游
    Then 响应状态码为 401

  Scenario: 无效 model 名称返回错误
    Given 通过 API 创建普通 key "real-bad-model-user"
    When 使用 key "real-bad-model-user" 发送 POST /chat/completions 请求使用 model "nonexistent-model"
    Then 响应状态码为 400 或 404 或 422

  Scenario: 流式请求返回 SSE 事件流
    Given 通过 API 创建普通 key "real-stream-user"
    When 使用 key "real-stream-user" 发送 POST /chat/completions stream=true 请求到真实上游
    Then 响应状态码为 200
    And 响应包含多个 SSE chunk

  Scenario: max_tokens 参数正常传递并生效
    Given 通过 API 创建普通 key "real-tokens-user"
    When 使用 key "real-tokens-user" 发送 POST /chat/completions 请求包含 max_tokens=50
    Then 响应状态码为 200
    And 响应 completion_tokens <= 50
