@mock
Feature: 错误处理

  Scenario: 请求缺少 model 字段返回 400
    Given 一个普通 key "err-no-model" 已生成
    When 使用 key "err-no-model" 发送 POST /chat/completions 缺少 model
    Then 响应状态码为 400

  Scenario: 请求缺少 messages 字段返回 400
    Given 一个普通 key "err-no-msgs" 已生成
    When 使用 key "err-no-msgs" 发送 POST /chat/completions 缺少 messages
    Then 响应状态码为 400

  Scenario: 请求 messages 数组为空返回 400
    Given 一个普通 key "err-empty-msgs" 已生成
    When 使用 key "err-empty-msgs" 发送 POST /chat/completions messages 为空
    Then 响应状态码为 400

  Scenario: 请求 body 为无效 JSON 返回 400
    Given 一个普通 key "err-bad-json" 已生成
    When 使用 key "err-bad-json" 发送 POST /chat/completions 无效 JSON
    Then 响应状态码为 400

  Scenario: 无效 API key 返回 401
    When 使用 invalid key 发送 GET /key/info 请求
    Then 响应状态码为 401

  Scenario: 缺少 Authorization 头返回 401
    When 不携带 Authorization 发送 GET /key/info 请求
    Then 响应状态码为 401

  Scenario: 上游 500 错误透传
    Given mock 上游已启动
    And mock 上游 "/v1/chat/completions" 返回状态码 500
    And 一个普通 key "err-upstream" 已生成
    When 使用 key "err-upstream" 发送 POST /chat/completions 请求
    Then 响应状态码为 500 或 502

  Scenario: 上游 429 限流透传
    Given mock 上游已启动
    And mock 上游 "/v1/chat/completions" 返回状态码 429
    And 一个普通 key "err-ratelimit" 已生成
    When 使用 key "err-ratelimit" 发送 POST /chat/completions 请求
    Then 响应状态码为 429
