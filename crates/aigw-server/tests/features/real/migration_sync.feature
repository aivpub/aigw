@real_api
Feature: 上游数据同步验证

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置
    And 上游 litellm 数据库连接已配置

  Scenario: 同步 plain tables（不含加密字段）
    When 从上游同步所有 plain tables 到 aigw
    Then 同步成功无报错
    And organizations 表行数 >= 0
    And teams 表行数 > 0
    And 所有 plain tables 与上游行数一致

  Scenario: 同步 credentials 表（含密钥轮转）
    When 从上游同步 credentials 表到 aigw
    Then 同步成功无报错
    And credentials 表行数 > 0
    And credentials 表行数与上游一致

  Scenario: 同步 proxy_models 表（含密钥轮转）
    When 从上游同步 proxy_models 表到 aigw
    Then 同步成功无报错
    And proxy_models 表行数 > 0
    And proxy_models 表行数与上游一致

  Scenario: 同步 spend_logs 表（限制 10 条）
    When 从上游同步 spend_logs 表到 aigw（限制 10 条）
    Then 同步成功无报错
    And spend_logs 表行数为 10
