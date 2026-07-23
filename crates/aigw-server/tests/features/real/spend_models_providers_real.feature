@real_api @needs_upstream_db
Feature: /spend/models + /spend/providers 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 下验证 model/provider 聚合
  因为 PG 版日期内联拼接与 SQLite/MySQL bind 参数行为差异大

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 /global/spend/models 按 model 聚合
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入多 model 的 spend_logs 并查询 /global/spend/models
    Then 响应状态码为 200
    And models 聚合按 model 分组且数值正确

  Scenario: /global/spend/models 支持日期过滤
    When 向 aigw 测试库灌入跨日期 spend_logs 并带日期查询 /global/spend/models
    Then 响应状态码为 200
    And models 聚合仅含日期范围内的数据

  Scenario: master-key 查询 /global/spend/providers 按 provider 聚合
    When 向 aigw 测试库灌入多 provider 的 spend_logs 并查询 /global/spend/providers
    Then 响应状态码为 200
    And providers 聚合按 provider 分组且空 provider 兜底为 unknown

  Scenario: /spend/models 需认证
    When 发送 GET /spend/models 请求（无认证）
    Then 响应状态码为 401

  Scenario: /global/spend/models 需管理员
    Given 通过 API 创建普通 key "mp-nonadmin"
    When 使用 key "mp-nonadmin" 发送 GET /global/spend/models 请求（real）
    Then 响应状态码为 403
