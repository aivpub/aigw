Feature: Claude /v1/messages 端点
  作为网关
  我需要支持 Anthropic 原生的 /v1/messages 端点
  以便客户端可以用 Claude 协议调用任何上游

  Scenario: 缺少 anthropic-version header 返回 400
    When 发送 POST /v1/messages 请求未带 anthropic-version header
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 400
    And 错误 type 为 "invalid_request_error"

  Scenario: 缺少 max_tokens 报错
    When 发送 POST /v1/messages 请求
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}]}
      """
    Then 响应状态码为 400
    And 错误信息包含 "max_tokens"

  Scenario: 缺少 model 报错
    When 发送 POST /v1/messages 请求
      """
      {"messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 400
    And 错误信息包含 "model"

  Scenario: messages 空数组报错
    When 发送 POST /v1/messages 请求
      """
      {"model":"claude-3","messages":[],"max_tokens":100}
      """
    Then 响应状态码为 400
    And 错误信息包含 "messages"

  Scenario: 未认证请求被拒绝
    When 发送 POST /v1/messages 请求未带认证
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 401
    And 错误 type 为 "authentication_error"

  Scenario: Anthropic 错误格式检查
    When 发送 POST /v1/messages 请求
      """
      {}
      """
    Then 响应状态码为 400
    And 响应体为 Anthropic 错误格式

  Scenario: x-api-key 认证通过（master key 不会报 401）
    When 发送 POST /v1/messages 请求带 x-api-key 认证
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码不为 401

  Scenario: Bearer token 认证通过（master key 不会报 401）
    When 发送 POST /v1/messages 请求带 Bearer 认证
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码不为 401

  Scenario: 模型不存在返回 400
    Given 已配置 model "claude-opus-4-8" 在数据库中
    When 发送 POST /v1/messages 请求带认证 model="nonexistent-model-xyz"
    Then 响应状态码为 400
    And 错误 type 为 "invalid_request_error"

  Scenario: /v1/messages Claude image block 转 OpenAI content-parts 透传
    Given mock 上游已启动
    And 已配置 model "gpt-4o-img" 指向 mock 上游
    And 一个普通 key "msg-image-user" 已生成
    When 使用 key "msg-image-user" 发送带图片的 POST /v1/messages 请求用 model "gpt-4o-img"
    Then 响应状态码为 200
    And mock 上游收到的 /v1/messages 请求 body 含 image_url 图片 parts
