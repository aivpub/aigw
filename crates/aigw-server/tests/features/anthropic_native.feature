Feature: Anthropic Native 上游适配
  作为网关
  我需要支持 Anthropic Native 上游（非 OpenAI-Compatible）
  以便 /v1/messages 和 /v1/chat/completions 端点都能路由到 Anthropic Messages API

  Scenario: select_adapter 返回 AnthropicPassthrough
    When 使用 ClientProtocol Anthropic 和 ProviderType AnthropicNative 选择适配器
    Then 适配器已选择
    And 适配器的 client_protocol 为 Anthropic

  Scenario: select_adapter 返回 OpenAIToAnthropic
    When 使用 ClientProtocol OpenAI 和 ProviderType AnthropicNative 选择适配器
    Then 适配器已选择
    And 适配器的 client_protocol 为 OpenAI

  Scenario: AnthropicPassthrough adapt_request 直通 body
    Given 一个 AnthropicPassthrough 适配器
    And 一个 Anthropic Native Deployment "claude-sonnet-4-20250514"
    When adapt_request 传入 Anthropic Messages 请求
      """
      {"model":"claude-sonnet","max_tokens":100,"messages":[{"role":"user","content":"hello"}]}
      """
    Then 请求 body 不变且 model 已替换

  Scenario: AnthropicPassthrough adapt_response 直通响应
    Given 一个 AnthropicPassthrough 适配器
    When adapt_response 传入 Anthropic Messages 响应
      """
      {"id":"msg_001","type":"message","role":"assistant","content":[{"type":"text","text":"hello"}],"model":"claude","stop_reason":"end_turn","usage":{"input_tokens":5,"output_tokens":3}}
      """
    Then 响应不变
