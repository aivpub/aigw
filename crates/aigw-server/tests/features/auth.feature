@mock
Feature: 授权 (Authorization)
  作为 API 网关
  我需要验证请求的授权
  以确保只有合法用户能访问受保护的资源

  Scenario: 无 Bearer Token 返回 401
    When 不携带 Authorization 发送 GET /key/list 请求
    Then 响应状态码为 401

  Scenario: 无效 Token 返回 401
    When 使用 invalid key 发送 GET /key/list 请求
    Then 响应状态码为 401

  Scenario: 普通 key 无法访问管理接口
    Given 一个普通 key "auth-regular" 已生成
    When 使用 key "auth-regular" 发送 GET /key/list 请求
    Then 响应状态码为 403

  Scenario: Master key 拥有完整访问权限
    When 使用 master-key 发送 GET /key/list 请求
    Then 响应状态码为 200

  Scenario: 有效 key 可以访问非管理接口
    Given 一个普通 key "auth-self" 已生成
    When 使用 key "auth-self" 发送 GET /spend/models 请求
    Then 响应状态码为 200
    And 响应 JSON 包含 "data" 字段
