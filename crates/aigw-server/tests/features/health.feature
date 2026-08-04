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

  # ━━━━ Stage 91: 模型健康检查探针 + spend_logs 留存 ━━━━

  Scenario: OpenAI 兼容模型探针命中 mock 上游返回 healthy 并留存 spend_log
    Given 健康检查 mock 上游已启动
    And 已配置 OpenAI 模型 "hc-openai-model" 指向健康检查 mock 上游
    When 发送 POST /model/health-check/all 请求
    Then 健康检查结果中 "hc-openai-model" 为 healthy
    And spend_logs 中存在 model="hc-openai-model" 且 call_type=health_check 且 status "success" 的记录

  Scenario: Anthropic-native 模型探针走 /v1/messages 路径返回 healthy
    Given 健康检查 mock 上游已启动
    And 已配置 Anthropic 模型 "hc-anthropic-model" 指向健康检查 mock 上游
    When 发送 POST /model/health-check/all 请求
    Then 健康检查结果中 "hc-anthropic-model" 为 healthy
    And spend_logs 中存在 model="hc-anthropic-model" 且 call_type=health_check 且 status "success" 的记录

  Scenario: mock 上游返回 500 时探针记为 unhealthy 且 spend_log 为 failure
    Given 健康检查 mock 上游已启动
    And 已配置 OpenAI 模型 "hc-fail-model" 指向健康检查 mock 上游
    And 健康检查 mock 上游 "/v1/chat/completions" 返回状态码 500
    When 发送 POST /model/health-check 请求查询模型 "hc-fail-model"
    Then 健康检查结果中 "hc-fail-model" 为 unhealthy
    And spend_logs 中存在 model="hc-fail-model" 且 call_type=health_check 且 status "failure" 的记录

  # ━━━━ Stage 98: 路由端点 BDD 补全 ━━━━

  Scenario: health_latest 返回最新检查记录
    Given 已确认有模型 "hl-test-model" 且健康检查表中有一条 status="healthy" 的记录
    When 发送 GET /health/latest 带 admin 认证请求
    Then 响应状态码为 200
    And 响应 body data 数组中包含 model_name 为 "hl-test-model" 且 status 为 "healthy" 的记录

  Scenario: health_latest 无记录时返回 data 数组但可能为空
    When 发送 GET /health/latest 带 admin 认证请求
    Then 响应状态码为 200
    And 响应 body 含 "data" 键

  Scenario: prometheus_metrics 返回期待格式
    When 发送 GET /metrics 请求
    Then 响应状态码为 200
    And Content-Type 文本包含 "text/plain"

  Scenario: health_metrics 返回 JSON metrics（admin 认证）
    When 发送 GET /health/metrics 带 admin 认证请求
    Then 响应状态码为 200
    And 响应 body 包含 uptime_seconds/db 等字段

  Scenario: health_metrics 无认证返回 401
    When 发送 GET /health/metrics 请求
    Then 响应状态码为 401
