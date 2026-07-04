Feature: 健康检查
  Scenario: 健康总览
    When 发送 GET /health 请求
    Then 响应状态码为 200
    And 响应包含 status 字段

  Scenario: liveness 探针
    When 发送 GET /health/liveliness 请求
    Then 响应状态码为 200

  Scenario: readiness 探针
    When 发送 GET /health/readiness 请求
    Then 响应状态码为 200
