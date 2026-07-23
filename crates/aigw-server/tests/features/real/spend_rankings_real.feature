@real_api @needs_upstream_db
Feature: /global/spend/keys/rankings 多 DB 端到端
  作为网关管理员
  我需要在 SQLite/PG/MySQL 三种 DB 下验证 keys 排名聚合
  以防跨 DB SQL 方言差异（如 PG 的 GROUP BY 严格模式）导致线上报错

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: master-key 查询 keys/rankings 返回按 spend 降序的排名
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入两条已知 spend_logs 并查询 keys/rankings
    Then 响应状态码为 200
    And keys/rankings 首条 total_spend 最大且 key_alias 已回填

  Scenario: 无认证访问 keys/rankings 返回 401
    When 不携带 Authorization 发送 GET /global/spend/keys/rankings 请求
    Then 响应状态码为 401

  Scenario: 普通用户访问 keys/rankings 返回 403
    Given 通过 API 创建普通 key "rank-nonadmin"
    When 使用 key "rank-nonadmin" 发送 GET /global/spend/keys/rankings 请求
    Then 响应状态码为 403
