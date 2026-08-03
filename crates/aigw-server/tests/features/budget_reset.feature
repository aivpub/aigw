@real_api
Feature: Budget Reset Flow
  验证预算周期到期后 spend 自动重置的完整流程

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 创建带 budget_duration 的 key 后 budget_reset_at 初始为 NULL
    When 发送 POST key generate 创建 key "budget-test" budget_duration="24h"
    Then 响应状态码为 200
    And key budget_reset_at 为空 "budget-test"

  Scenario: budget_duration=NULL 的 key budget_reset_at 也为空
    When 发送 POST key generate 创建无 budget_duration 的 key "budget-no-reset"
    Then 响应状态码为 200
    And key budget_reset_at 为空 "budget-no-reset"

  Scenario: 查询 key 信息时 budget_duration 字段可见
    Given 通过 API 创建 key "budget-info" budget_duration="daily"
    When 发送 GET key info 查询 key "budget-info"
    Then 响应状态码为 200
    And 响应 body 中 budget_duration 为 "daily"

  Scenario: BudgetResetter 扫描能找到 budget_duration+NUL 的 key
    Given 通过 API 创建 key "budget-scan" budget_duration="daily"
    When 发送 POST admin jobs trigger budget_reset 扫描 key 类型
    Then 响应状态码为 200
    And 响应 body 中 total_steps 大于 0
