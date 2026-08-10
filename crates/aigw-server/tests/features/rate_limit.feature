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

  # ── Stage 117: HTTP 级行为验证（接线后 guard 真实生效）──

  Scenario: HTTP 级 RPM 超限返回 429 + x-ratelimit 头
    Given mock 上游已启动
    And 已配置 model "gpt-4" 指向 mock 上游
    And 一个普通 key "http-rpm-key" 已生成且绑定模型 "gpt-4"
    And 更新 key "http-rpm-key" 的 rpm_limit=2
    When 使用 key "http-rpm-key" 发送 POST /chat/completions 请求用 model "gpt-4"
    And 使用 key "http-rpm-key" 发送 POST /chat/completions 请求用 model "gpt-4"
    And 使用 key "http-rpm-key" 发送 POST /chat/completions 请求用 model "gpt-4"
    Then 响应状态码为 429
    And 响应头包含 x-ratelimit-limit

  Scenario: HTTP 级 master key 全链直通不触发限流
    Given mock 上游已启动
    And 已配置 model "gpt-4" 指向 mock 上游
    When 使用 master-key 发送 POST /chat/completions 请求用 model "gpt-4"
    Then 响应状态码为 200
