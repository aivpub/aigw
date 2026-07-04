Feature: Global 用量查询
  Scenario: 查询全局用量需要 admin 权限
    When 发送 GET /global/spend 请求（无认证）
    Then 响应状态码为 401
