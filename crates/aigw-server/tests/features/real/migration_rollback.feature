@real_api
Feature: 回滚迁移验证（aigw → litellm 反向同步）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置
    And 上游 litellm 数据库连接已配置

  Scenario: 回滚 plain tables 到上游 litellm
    When 从 aigw 回滚所有 plain tables 到上游 litellm
    Then 回滚同步成功无报错
    And 回滚后 plain tables 与源 aigw 行数一致

  Scenario: 回滚 credentials 表到上游 litellm（含密钥轮转）
    When 从 aigw 回滚 credentials 表到上游 litellm
    Then 回滚同步成功无报错
    And 回滚后 credentials 表与源 aigw 行数一致
