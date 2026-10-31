@real_api
Feature: MCP 日志保留期 real BDD — 035/036 迁移与三方言分批删除（Stage 150/152）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 035 迁移在本方言上建出拒绝审计表
    Then 真库 mcp_auth_rejection 表存在

  Scenario: 036 迁移在本方言上建出调用 body 表
    Then 真库 mcp_call_body 表存在

  Scenario: 一批就能删完时三张表的过期行都清掉
    Given 真库保留期场景在三张表各写入 2 行过期与 1 行新鲜记录
    When 真库保留期 worker 以 batch_size=5000 执行一次清理
    Then 真库中过期记录已全部删除、1 行新鲜记录仍在

  Scenario: batch_size=1 时仍把 5 行积压删干净
    Given 真库保留期场景在三张表各写入 5 行过期与 0 行新鲜记录
    When 真库保留期 worker 以 batch_size=1 执行一次清理
    Then 真库中过期记录已全部删除、0 行新鲜记录仍在

  Scenario: call_log_days=0 只删拒绝记录
    Given 真库保留期场景在三张表各写入 2 行过期与 1 行新鲜记录
    When 真库保留期 worker 配置 call_log_days=0 执行一次清理
    Then 真库中过期调用日志与 body 仍在而过期拒绝记录已删除
