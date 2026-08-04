@mock
Feature: Rate Limiting — RPM/TPM 超限返回 429

  Scenario: RPM 超限返回 429
    Given key "rpm-limit-key" 的 rpm_limit=2
    And 过去 1 分钟内已使用 key "rpm-limit-key" 发送 2 个请求
    When 使用 key "rpm-limit-key" 的 enforce_limits（token_estimate=0）
    Then enforce_limits 返回 LimitError::RateLimited
    And 错误类型是 rate_limited

  Scenario: TPM 超限返回 429
    Given key "tpm-limit-key" 的 tpm_limit=100
    And 过去 1 分钟内已使用 key "tpm-limit-key" 消费 100 tokens
    When 使用 key "tpm-limit-key" 的 enforce_limits（token_estimate=100）
    Then enforce_limits 返回 LimitError::RateLimited
    And 错误类型是 rate_limited

  Scenario: 未超限正常通过
    Given key "normal-key" 的 rpm_limit=100
    When 使用 key "normal-key" 的 enforce_limits（token_estimate=0）
    Then enforce_limits 返回 OK 不触发限制
