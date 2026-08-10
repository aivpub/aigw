@real_api @needs_upstream_db
Feature: 多级 BudgetEnforcer — key→user→team→org 逐级检查

  作为网关管理员
  我需要在 SQLite/PG/MySQL 三种 DB 下验证多级预算检查
  以确保任意层级超限都能正确返回 429 并携带 entity_type 标识

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置


  Scenario: 多级检查——key 未超但 user 超了
    Given 数据库中有 user "budget-ml-u1" max_budget=10 spend=9.5 和 key "budget-ml-k1" max_budget=100 关联该 user
    When 为该 user "budget-ml-u1" 增加 spend 1.0 使 user 累计达到 10.5
    And 使用 key "budget-ml-k1" 发送 chat 请求 cost=1.0
    Then 响应状态码为 429
    And 响应 body 包含 entity_type "user"


  Scenario: 多级检查——team 级拒绝
    Given 数据库中有 team "budget-ml-t1" max_budget=5 spend=4.8 和 key "budget-ml-k2" 关联该 team
    When 为该 team "budget-ml-t1" 增加 spend 0.5 使 team 累计达到 5.3
    And 使用 key "budget-ml-k2" 发送 chat 请求 cost=0.5
    Then 响应状态码为 429
    And 响应 body 包含 entity_type "team"


  Scenario: 多级检查——全通过
    Given 数据库中有 key "budget-ml-k3" max_budget=100 和 user "budget-ml-u3" max_budget=200 和 team "budget-ml-t3" max_budget=500
    When 使用 key "budget-ml-k3" 发送 chat 请求 cost=1.0
    Then 响应状态码为 200


  Scenario: org 级检查——JOIN budgets 表
    Given 数据库中有 org "budget-ml-o1" budget_id="budget-ml-b1" spend=19.5 和 budget "budget-ml-b1" max_budget=20
    And 有关联该 org 的 team "budget-ml-t4" 和 key "budget-ml-k4"
    When 为该 org "budget-ml-o1" 增加 spend 1.0 使 org 累计达到 20.5
    And 使用 key "budget-ml-k4" 发送 chat 请求 cost=1.0
    Then 响应状态码为 429
    And 响应 body 包含 entity_type "organization"


  @skip
  Scenario: 完整链路——spend 更新 → reset → 恢复
    Given 通过 API 创建 key "budget-ml-cycle" budget_duration="1h" max_budget=10
    When 持续发送请求使 key spend 超过 10.0
    Then 响应状态码为 429
    When 发送 POST admin jobs trigger budget_reset 扫描 key 类型
    And 等待 budget_reset job 执行完成
    And 使用 key "budget-ml-cycle" 发送 chat 请求 cost=1.0
    Then 响应状态码为 200

  @skip

  Scenario: 历史用量聚合——team 和 org 维度的 SUM(spend)
    Given 数据库中有多条 spend_logs 跨不同 team 和 org
    When 调用 get_spend_by_team 和 get_spend_by_org
    Then 返回的 SUM(spend) 与预期一致
