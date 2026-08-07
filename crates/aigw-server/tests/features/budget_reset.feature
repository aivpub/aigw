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

  Scenario: BudgetResetter execute 实际重置 spend 为 0 并设置新的 budget_reset_at
    # 创建一个 key 并直接修改 DB：设 spend=50、设 budget_reset_at 为过去时间（模拟已过期）
    Given 通过 API 创建 key "reset-exec" budget_duration="daily" max_budget=100
    And 将 key "reset-exec" 的 spend 设为 50 且 budget_reset_at 设为已过期
    # 触发 budget_reset 扫描并执行
    When 发送 POST admin jobs trigger budget_reset 扫描 key 类型
    And 等待 budget_reset job 执行完成
    # 验证 spend 已归零且 budget_reset_at 已更新为未来的时间
    Then key "reset-exec" 的 spend 应为 0
    And key "reset-exec" 的 budget_reset_at 不为空

  # ──── budget-reset stats 端点（供前端预算重置面板）────

  Scenario: budget-reset stats 返回 ready 计数与 preview 预览
    Given 通过 API 创建 key "stats-key" budget_duration="daily"
    And 将 key "stats-key" 的 spend 设为 1 且 budget_reset_at 设为已过期
    When 发送 GET admin budget-reset stats
    Then 响应状态码为 200
    And 响应 body 中 counts.key.ready 大于 0
    And 响应 body 中 ready_total 大于 0
    And 响应 body 中 preview  entity_type 为 "key"
    And 响应 body 中 next_tick_at 在未来
    And 响应 body 中 last_reset 为 null 或合法 job

  Scenario: 非 admin 访问 budget-reset stats 返回 401
    Given 非 admin 用户 token 已就绪
    When 非 admin 发送 GET admin budget-reset stats
    Then 响应状态码为 401
