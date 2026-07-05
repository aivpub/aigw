@real_api
Feature: SDK 兼容性验证

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: OpenAI SDK 错误格式兼容
    Given 通过 API 创建普通 key "compat-err-user"
    When 发送无 messages 字段的请求经 aigw 到真实上游
    Then 响应状态码为 400 或 500
    And 错误格式与 OpenAI 官方一致
    And 错误包含 "error" 和 "type" 字段

  Scenario: Claude SDK 协议格式兼容验证
    Given 通过 API 创建普通 key "compat-claude-user"
    When 使用 OpenAI SDK 调用默认模型经 aigw
    Then 响应状态码为 200
    And 客户端收到 OpenAI 协议格式的响应

  Scenario: 缺少 API key 时 SDK 收到标准错误
    When 不带 Authorization 头发送请求
    Then 响应状态码为 401
    And 错误 type 是 "authentication_error"
