Feature: Spend 查询
  Scenario: 查询 spend logs 需要认证
    When 发送 GET /spend/logs 请求（无认证）
    Then 响应状态码为 401

  Scenario: 查询 spend keys 需要认证
    When 发送 GET /spend/keys 请求（无认证）
    Then 响应状态码为 401

  Scenario: 查询详情端点需要认证
    When 发送 GET /global/spend/logs/missing-id 请求（无认证）
    Then 响应状态码为 401

  Scenario: 详情端点不存在的 request_id 返回 404
    Given 一个支出记录 "test-bdd-req-001" 已入库
    When 使用 master-key 发送 GET /global/spend/logs/nonexistent-id 请求
    Then 响应状态码为 404

  Scenario: 详情端点有效的 request_id 返回 200
    Given 一个支出记录 "test-bdd-req-002" 含 body 已入库
    When 使用 master-key 发送 GET /global/spend/logs/test-bdd-req-002 请求
    Then 响应状态码为 200
    And 响应 body 包含 "messages" 和 "response" 字段

  Scenario: 列表端点不含 body 字段
    Given 一个支出记录 "test-bdd-req-bodyless" 含 body 已入库
    When 使用 master-key 发送 GET /global/spend/logs 请求带 page_size=10
    Then 响应状态码为 200
    And 响应 data 不含 "messages" 和 "response" 字段
