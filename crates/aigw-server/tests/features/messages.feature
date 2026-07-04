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

  Scenario: x-api-key 认证通过
    When 发送 POST /v1/messages 请求带 x-api-key 认证
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 200 或 404

  Scenario: Bearer token 认证通过
    When 发送 POST /v1/messages 请求带 Bearer 认证
      """
      {"model":"claude-3","messages":[{"role":"user","content":"hi"}],"max_tokens":100}
      """
    Then 响应状态码为 200 或 404

  Scenario: 模型不存在返回 404
    Given 已配置 model "existing-model" 在数据库中
    When 发送 POST /v1/messages 请求带认证 model="nonexistent-model"
    Then 响应状态码为 404
    And 错误 type 为 "not_found_error"
