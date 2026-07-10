Feature: Spend 用量聚合查询
  作为网关管理员
  我需要按模型和提供商聚合查询用量数据
  以便监控不同维度的消耗情况

  Scenario: /spend/models 需要认证
    When 发送 GET /spend/models 请求（无认证）
    Then 响应状态码为 401

  Scenario: /spend/providers 需要认证
    When 发送 GET /spend/providers 请求（无认证）
    Then 响应状态码为 401

  Scenario: /global/spend/models 需要管理员权限
    Given 一个普通 key "spend-user" 已生成
    When 使用 key "spend-user" 发送 GET /global/spend/models 请求
    Then 响应状态码为 403

  Scenario: /global/spend/providers 需要管理员权限
    Given 一个普通 key "spend-prov-user" 已生成
    When 使用 key "spend-prov-user" 发送 GET /global/spend/providers 请求
    Then 响应状态码为 403

  Scenario: /spend/models 正常返回聚合数据
    Given 一个普通 key "spend-model-user" 已生成
    When 使用 key "spend-model-user" 发送 GET /spend/models 请求
    Then 响应状态码为 200
    And 响应 JSON 包含 "data" 字段

  Scenario: /spend/providers 正常返回聚合数据
    Given 一个普通 key "spend-prov2-user" 已生成
    When 使用 key "spend-prov2-user" 发送 GET /spend/providers 请求
    Then 响应状态码为 200
    And 响应 JSON 包含 "data" 字段

  Scenario: /global/spend/models 管理员可查所有数据
    When 使用 master-key 发送 GET /global/spend/models 请求
    Then 响应状态码为 200
    And 响应 JSON 包含 "data" 字段

  Scenario: /global/spend/providers 管理员可查所有数据
    When 使用 master-key 发送 GET /global/spend/providers 请求
    Then 响应状态码为 200
    And 响应 JSON 包含 "data" 字段

  Scenario: /spend/logs 支持 model 过滤参数
    Given 一个普通 key "spend-filter-user" 已生成
    When 使用 key "spend-filter-user" 发送 GET /spend/logs 请求带 model 过滤
    Then 响应状态码为 200

  Scenario: /spend/logs 支持 start_date/end_date 过滤参数
    Given 一个普通 key "spend-date-user" 已生成
    When 使用 key "spend-date-user" 发送 GET /spend/logs 请求带时间过滤
    Then 响应状态码为 200

  Scenario: /spend/logs 响应包含分页元数据
    Given 一个普通 key "spend-paginate-user" 已生成
    When 使用 key "spend-paginate-user" 发送 GET /spend/logs 请求带 page=1&page_size=10
    Then 响应状态码为 200
    And 响应 JSON 包含 "page" 字段值为 1
    And 响应 JSON 包含 "page_size" 字段值为 10
    And 响应 JSON 包含 "total_count" 字段
    And 响应 JSON 包含 "total_pages" 字段

  Scenario: /spend/logs 响应包含请求耗时和TTFT字段
    Given 一个普通 key "spend-ttft-user" 已生成
    When 使用 key "spend-ttft-user" 发送 GET /spend/logs 请求带 page=1&page_size=10
    Then 响应状态码为 200
    And 响应 JSON 包含 "data" 字段

  Scenario: /spend/logs 支持 request_id 过滤
    Given 一个普通 key "spend-rid-user" 已生成
    When 使用 key "spend-rid-user" 发送 GET /spend/logs 请求带 request_id 过滤
    Then 响应状态码为 200

  Scenario: /global/spend/logs 分页和 request_id 过滤
    Given 一个普通 key "gsl-paginate-user" 已生成
    When 使用 master-key 发送 GET /global/spend/logs 请求带 page=1&page_size=5&request_id=nonexistent
    Then 响应状态码为 200
    And 响应 JSON 包含 "total_pages" 字段
