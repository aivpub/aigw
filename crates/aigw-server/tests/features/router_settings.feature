@mock
Feature: Router Settings — key/team 级别覆盖

  Scenario: patch_key 设置 key 级别 router_settings
    Given 已存在 key "patch-test-key"
    When 使用 master-key 发送 PATCH key patch-test-key 的 router_settings 设置 cooldown_time=30
    Then 响应状态码为 200
    And key patch-test-key 的 router_settings cooldown_time 为 30

  Scenario: patch_key 无认证返回 401
    When 不认证发送 PATCH key some-key 的 router_settings 设置 cooldown_time=30
    Then 响应状态码为 401

  Scenario: patch_team 设置 team 级别 router_settings
    Given 已存在 team "patch-team-1"
    When 使用 master-key 发送 PATCH team patch-team-1 的 router_settings 设置 num_retries=2
    Then 响应状态码为 200
    And team patch-team-1 的 router_settings num_retries 为 2

  Scenario: patch_team 无认证返回 401
    When 不认证发送 PATCH team some-team 的 router_settings 设置 num_retries=2
    Then 响应状态码为 401
