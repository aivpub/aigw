@mock
Feature: Soft Budget — 软预算命中放行 + 硬预算拒绝（Stage 117）

  Scenario: soft_budget 命中但未超硬预算 → 请求放行（200）
    Given mock 上游已启动
    And 已配置 model "gpt-4" 指向 mock 上游
    And 一个普通 key "soft-pass-key" 已生成且绑定模型 "gpt-4"
    And key "soft-pass-key" 的 spend=80 soft_budget=50 max_budget=100
    When 使用 key "soft-pass-key" 发送 POST /chat/completions 请求用 model "gpt-4"
    Then 响应状态码为 200

  Scenario: 硬预算超限（spend >= max_budget）→ 429 budget_exceeded
    Given mock 上游已启动
    And 已配置 model "gpt-4" 指向 mock 上游
    And 一个普通 key "hard-reject-key" 已生成且绑定模型 "gpt-4"
    And key "hard-reject-key" 的 spend=150 soft_budget=50 max_budget=100
    When 使用 key "hard-reject-key" 发送 POST /chat/completions 请求用 model "gpt-4"
    Then 响应状态码为 429
    And 响应 body 包含 entity_type "key"
