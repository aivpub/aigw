@mock
Feature: 端到端调用链路（mock）

  Scenario: OpenAI 协议调用 mock 上游返回成功
    Given mock 上游已启动
    And 已配置 model "gpt-4-mock" 指向 mock 上游
    And 一个普通 key "e2e-user" 已生成
    When 使用 key "e2e-user" 发送 POST /chat/completions 请求用 model "gpt-4-mock"
    Then 响应状态码为 200
    And mock 上游收到请求

  Scenario: 上游错误码透传
    Given mock 上游已启动
    And 已配置 model "gpt-4-mock" 指向 mock 上游
    And mock 上游 "/v1/chat/completions" 返回状态码 500
    And 一个普通 key "e2e-error-user" 已生成
    When 使用 key "e2e-error-user" 发送 POST /chat/completions 请求用 model "gpt-4-mock"
    Then 响应状态码为 500 或 502

  Scenario: 上游超时场景
    Given mock 上游已启动
    And 已配置 model "gpt-4-mock" 指向 mock 上游
    And mock 上游 "/v1/chat/completions" 返回状态码 503
    And 一个普通 key "e2e-timeout-user" 已生成
    When 使用 key "e2e-timeout-user" 发送 POST /chat/completions 请求用 model "gpt-4-mock"
    Then 响应状态码为 500 或 502 或 503

  Scenario: 用量记录写入 spend_logs（成功）
    Given mock 上游已启动
    And 已配置 model "spend-log-model" 指向 mock 上游
    And 一个普通 key "e2e-spend-user" 已生成
    When 使用 key "e2e-spend-user" 发送 POST /chat/completions 请求用 model "spend-log-model"
    Then 响应状态码为 200
    And spend_logs 表中存在 model="spend-log-model" 且 status="success" 的记录

  Scenario: 用量记录写入 spend_logs（失败）
    Given 一个普通 key "e2e-spend-fail-user" 已生成
    When 使用 key "e2e-spend-fail-user" 发送 POST /chat/completions 请求用 model "nonexistent-model-xyz"
    Then 响应状态码为 400
    And spend_logs 表中存在 model="nonexistent-model-xyz" 且 status 包含 "failure" 的记录

  Scenario: Mock 上游请求转发路径正确
    Given mock 上游已启动
    And 已配置 model "path-check-model" 指向 mock 上游
    And 一个普通 key "e2e-path-user" 已生成
    When 使用 key "e2e-path-user" 发送 POST /chat/completions 请求用 model "path-check-model"
    Then 响应状态码为 200
    And mock 上游收到路径为 "/v1/chat/completions" 的请求

  Scenario: 响应体包含预期字段
    Given mock 上游已启动
    And 已配置 model "field-check-model" 指向 mock 上游
    And 一个普通 key "e2e-field-user" 已生成
    When 使用 key "e2e-field-user" 发送 POST /chat/completions 请求用 model "field-check-model"
    Then 响应状态码为 200
    And 响应 JSON 包含 "choices" 字段

  Scenario: spend_logs model 字段记录上游模型名而非代理名
    Given mock 上游已启动
    And 已配置 model "proxy-name" 且上游 model 为 "upstream-real-model" 指向 mock 上游
    And 一个普通 key "e2e-upstream-model-user" 已生成
    When 使用 key "e2e-upstream-model-user" 发送 POST /chat/completions 请求用 model "proxy-name"
    Then 响应状态码为 200
    And spend_logs 中 model 字段值为 "upstream-real-model"

  Scenario: chat 图片透传（OpenAI image_url parts 原样到上游）
    Given mock 上游已启动
    And 已配置 model "gpt-4o-mock" 指向 mock 上游
    And 一个普通 key "e2e-image-user" 已生成
    When 使用 key "e2e-image-user" 发送带图片的 POST /chat/completions 请求用 model "gpt-4o-mock"
    Then 响应状态码为 200
    And mock 上游收到的请求 body 保留 image_url 图片 parts
