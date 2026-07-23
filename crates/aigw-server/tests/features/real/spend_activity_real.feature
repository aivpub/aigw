@real_api @needs_upstream_db
Feature: /global/spend/activity 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 三种 DB 下验证 activity 聚合
  因为该接口三 DB 占位符/日期转换/动态过滤方言差异最大且零覆盖

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 activity 返回 metadata 和 daily
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入跨天 spend_logs 并查询 activity
    Then 响应状态码为 200
    And activity metadata 7 个字段数值正确
    And activity daily 按天分组且数值正确

  Scenario: activity 支持 user_id 过滤
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入不同 user 的 spend_logs 并带 user_id 查询 activity
    Then 响应状态码为 200
    And activity metadata 仅统计该 user 的数据

  Scenario: activity 支持 team_id 过滤
    When 向 aigw 测试库灌入不同 team 的 spend_logs 并带 team_id 查询 activity
    Then 响应状态码为 200
    And activity metadata 仅统计该 team 的数据

  Scenario: activity 无认证返回 401
    When 不携带 Authorization 发送 GET /global/spend/activity 请求
    Then 响应状态码为 401

  Scenario: 普通用户访问 activity 返回 403
    Given 通过 API 创建普通 key "act-nonadmin"
    When 使用 key "act-nonadmin" 发送 GET /global/spend/activity 请求
    Then 响应状态码为 403
