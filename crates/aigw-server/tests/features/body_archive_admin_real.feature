@real_api
Feature: Body Archive Admin API — Stage 82 生产化验收

  # ━━━━ Stage 82: trigger enabled=false → 409 ━━━━
  # 验证 P0-3/P0-6：配置失联或 disabled 时 trigger 端点必须拒绝，
  # 而不是用 default config 创建 Job 导致假阳性执行。
  # ServerGuard 启动时不传 body_archive config，enabled 默认 false。

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: body_archive 未配置时 trigger 返回 409 Conflict
    When 使用 master-key 发送 POST /admin/jobs/trigger 请求
      """
      {"step_type": "body_archive", "payload": {"start_date": "2026-07-22T00:00:00+00:00", "end_date": "2026-07-23T00:00:00+00:00"}}
      """
    Then 响应状态码为 409

  Scenario: 未认证 trigger 返回 401
    When 不携带 Authorization 发送 POST /admin/jobs/trigger 请求
      """
      {"step_type": "body_archive", "payload": {}}
      """
    Then 响应状态码为 401

  Scenario: trigger 未知 step_type 返回 404
    When 使用 master-key 发送 POST /admin/jobs/trigger 请求
      """
      {"step_type": "nonexistent_step_type", "payload": {}}
      """
    Then 响应状态码为 404
