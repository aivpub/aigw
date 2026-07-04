Feature: Spend 查询
  Scenario: 查询 spend logs 需要认证
    When 发送 GET /spend/logs 请求（无认证）
    Then 响应状态码为 401

  Scenario: 查询 spend keys 需要认证
    When 发送 GET /spend/keys 请求（无认证）
    Then 响应状态码为 401
