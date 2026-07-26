@real_api
Feature: 协议转换端到端验证
  # 验证 /v1/messages (Anthropic 客户端协议) → resolver env-var fallback →
  # AnthropicToOpenAI adapter → OpenAI 格式 → 上游 litellm → 响应转回 Anthropic 格式。
  # 与 end_to_end_real.feature 共用同一套 env var (AIGW_REAL_MODEL / OPENAPI_MODEL)。

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置
    Given 上游 litellm 数据库连接已配置

  Scenario: Anthropic 客户端 /v1/messages 返回 Anthropic Messages 格式响应
    Given 通过 API 创建普通 key "real-an2oa-user"
    When 使用 key "real-an2oa-user" 发送 POST /v1/messages 请求用默认模型
    Then 响应状态码为 200
    And 响应为 Anthropic Messages 格式（type=message, role=assistant）
    And 响应包含 content 数组

  Scenario: Anthropic 客户端 /v1/messages stream 返回 SSE 事件流
    Given 通过 API 创建普通 key "real-stream-an-user"
    When 使用 key "real-stream-an-user" 发送 POST /v1/messages stream=true 请求用默认模型
    Then 响应状态码为 200
    And 流式响应包含 Anthropic SSE 事件（message_start）
