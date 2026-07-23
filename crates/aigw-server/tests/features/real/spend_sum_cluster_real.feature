@real_api @needs_upstream_db
Feature: SUM 聚合簇 + 应用层 keys 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 下验证简单 SUM 与应用层聚合
  因为 /spend/users 的 "user" 引号列名、/spend/tags 的 LIKE 转义存在跨 DB 差异

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 /global/spend 返回总 spend
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入若干 spend_logs 并查询 /global/spend
    Then 响应状态码为 200
    And global spend 等于灌入总额

  Scenario: 使用带 user 的 key 查询 /spend/users
    Given 通过 API 创建带 user_id 的 key "sum-user-key"
    When 向 aigw 测试库灌入该 user 的 spend_logs 并使用该 key 查询 /spend/users
    Then 响应状态码为 200
    And spend/users 返回该 user 的累计 spend

  Scenario: master-key 查询 /spend/tags 按 tag 匹配
    When 向 aigw 测试库灌入带 request_tags 的 spend_logs 并查询 /spend/tags?tag=important
    Then 响应状态码为 200
    And spend/tags 返回匹配 tag 的累计 spend

  Scenario: master-key 查询 /global/spend/keys 应用层聚合
    When 向 aigw 测试库灌入多 key 的 spend_logs 并查询 /global/spend/keys
    Then 响应状态码为 200
    And global/spend/keys 应用层聚合结果正确
