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

  # ━━━━ Stage 85: call_id / upstream request_id 双列 ━━━━
  # 核心预期:任意 SpendLog 都能用上游 request_id 对账;搜索同时命中 call_id 与 request_id。
  Scenario: SpendLog 同时返回 call_id 与上游 request_id
    Given 一条含上游 request_id 的支出记录 "gw-call-001" 已入库
    When 使用 master-key 发送 GET /global/spend/logs/gw-call-001 请求
    Then 响应状态码为 200
    And 响应 body 的 call_id 为 "gw-call-001"
    And 响应 body 的 request_id 为 "msg_upstream_001"

  Scenario: 搜索参数 request_id 同时匹配 call_id 与上游 request_id
    Given 一条含上游 request_id 的支出记录 "gw-call-001" 已入库
    And 一条含上游 request_id 的支出记录 "gw-call-002" 已入库
    When 使用 master-key 发送 GET /global/spend/logs 请求搜索 request_id 为 "gw-call-002"
    Then 响应状态码为 200
    And 响应 data 包含 call_id 为 "gw-call-002" 的记录
    When 使用 master-key 发送 GET /global/spend/logs 请求搜索 request_id 为 "msg_upstream_001"
    Then 响应状态码为 200
    And 响应 data 包含 call_id 为 "gw-call-001" 的记录
