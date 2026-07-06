@mock
Feature: 迁移数据运行时验证

  Scenario: 创建并查询 credential
    Given 一个 credential "mig-cred-1" 已创建
    When 使用 master key 查询 credential "mig-cred-1"
    Then 响应状态码为 200
    And 响应 JSON 字段 "credential_name" 值为 "mig-cred-1"

  Scenario: 创建并列表查询 credential
    Given 一个 credential "mig-cred-2" 已创建
    When 使用 master key 发送 GET credential list 请求
    Then 响应状态码为 200
    And 响应 JSON 列表中应包含 credential 名称为 "mig-cred-2"

  Scenario: 更新 credential 后验证变更
    Given 一个 credential "mig-cred-3" 已创建
    When 使用 master key 更新 credential "mig-cred-3" 的 api_key 为 "updated-key"
    And 使用 master key 查询 credential "mig-cred-3"
    Then 响应 JSON 字段 "credential_values" 包含 "updated-key"

  Scenario: 删除 credential 后不可见
    Given 一个 credential "mig-cred-4" 已创建
    When 使用 master key 删除 credential "mig-cred-4"
    And 使用 master key 查询 credential "mig-cred-4"
    Then 响应状态码为 404
