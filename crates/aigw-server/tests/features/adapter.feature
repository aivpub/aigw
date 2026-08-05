Feature: Provider 适配转换
  作为网关
  我需要 OpenAI 和 Claude 协议间的双向转换
  以便客户端可以用 OpenAI 格式调用 Claude 模型

  Scenario: OpenAI 请求转换为 Claude 请求
    Given 一个 OpenAI ChatCompletion 请求
      """
      {"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]}
      """
    When 通过适配器转换为 Claude 请求
    Then Claude 请求的 model 为 "gpt-4"
    And Claude 请求包含 messages 数组
    And Claude 请求的 max_tokens 为 1024

  Scenario: OpenAI 系统消息转为 Claude system 字段
    Given 一个包含系统消息的 OpenAI 请求
      """
      {"model": "gpt-4", "messages": [{"role": "system", "content": "Be helpful"}, {"role": "user", "content": "Hi"}]}
      """
    When 通过适配器转换为 Claude 请求
    Then Claude 请求的 system 字段为 "Be helpful"
    And Claude 请求的 messages 只包含 user 消息

  Scenario: Claude 响应转换为 OpenAI 响应
    Given 一个 Claude Messages 响应
    When 通过适配器转换为 OpenAI 响应
    Then OpenAI 响应的 object 为 "chat.completion"
    And OpenAI 响应包含 choices 数组
    And OpenAI 响应包含 usage 信息

  Scenario: Claude 到 OpenAI 请求转换
    Given 一个 Claude Messages 请求
      """
      {"model": "claude-sonnet", "messages": [{"role": "user", "content": "What is Rust?"}], "max_tokens": 512}
      """
    When 通过适配器转换为 OpenAI 请求
    Then OpenAI 请求的 model 为 "claude-sonnet"
    And OpenAI 请求的 max_tokens 为 512

  Scenario: OpenAI 到 Claude 响应转换
    Given 一个 OpenAI ChatCompletion 响应
    When 通过适配器转换为 Claude 响应
    Then Claude 响应的 type 为 "message"
    And Claude 响应的 role 为 "assistant"

  Scenario: reasoning_content 在 OpenAI-Claude-OpenAI 往返转换中保留
    Given 一个包含 reasoning_content 的 OpenAI 响应
    When 响应通过 OpenAI->Claude->OpenAI 往返转换
    Then 往返后的 OpenAI 请求中 assistant 消息的 reasoning_content 为 "analyzing step by step"

  Scenario: DeepSeek thinking 模式流式 Delta 保留 reasoning_content
    Given 一个包含 reasoning_content 的 SSE Delta chunk
      """
      {"id":"chatcmpl-001","object":"chat.completion.chunk","created":1,"model":"deepseek-v4-flash","choices":[{"index":0,"delta":{"role":"assistant","content":"","reasoning_content":"Let me analyze this"},"finish_reason":null}]}
      """
    When 解析该 Delta chunk
    Then delta.reasoning_content 为 "Let me analyze this"

  Scenario: Usage token details 在序列化/反序列化中保留
    Given 一个包含 prompt_tokens_details 和 completion_tokens_details 的 Usage 结构
    When Usage 结构序列化后再反序列化
    Then cached_tokens 值为 80
    And reasoning_tokens 值为 20
