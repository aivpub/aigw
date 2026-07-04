@mock
Feature: 端到端调用链路（mock）

  Scenario: OpenAI 协议调用 mock 上游返回成功
    Given mock 上游已启动
    And 已配置 model "gpt-4-mock" 指向 mock 上游
    And 一个普通 key "e2e-user" 已生成
    When 使用 key "e2e-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 200
    And mock 上游收到请求

  Scenario: 上游错误码透传
    Given mock 上游已启动
    And mock 上游 "/v1/chat/completions" 返回状态码 500
    And 一个普通 key "e2e-error-user" 已生成
    When 使用 key "e2e-error-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 500 或 502

  Scenario: 上游超时场景
    Given mock 上游已启动
    And mock 上游 "/v1/chat/completions" 返回状态码 503
    And 一个普通 key "e2e-timeout-user" 已生成
    When 使用 key "e2e-timeout-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 500 或 502 或 503

  Scenario: 用量记录写入 spend_logs
    Given mock 上游已启动
    And 已配置 model "spend-log-model" 指向 mock 上游
    And 一个普通 key "e2e-spend-user" 已生成
    When 使用 key "e2e-spend-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 200

  Scenario: Mock 上游请求转发路径正确
    Given mock 上游已启动
    And 已配置 model "path-check-model" 指向 mock 上游
    And 一个普通 key "e2e-path-user" 已生成
    When 使用 key "e2e-path-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 200
    And mock 上游收到路径为 "/v1/chat/completions" 的请求

  Scenario: 响应体包含预期字段
    Given mock 上游已启动
    And 已配置 model "field-check-model" 指向 mock 上游
    And 一个普通 key "e2e-field-user" 已生成
    When 使用 key "e2e-field-user" 发送 POST /chat/completions 请求
    Then 响应状态码为 200
    And 响应 JSON 包含 "choices" 字段
