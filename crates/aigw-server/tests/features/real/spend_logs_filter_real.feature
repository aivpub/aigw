@real_api @needs_upstream_db
Feature: spend_logs 的 call_type / parent_call_id 过滤跨三方言一致（Stage 137）

  列表查询与计数查询必须应用同一组条件，
  且 metadata 的 JSON 路径条件须在 SQLite / MySQL / PG 上行为一致。

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: call_type=search 过滤，列表与计数同口径
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入 1 条 LLM 行 + 1 条 search 行并按 "call_type=search" 过滤查询 spend logs
    Then 响应状态码为 200
    And 筛选结果的 count 与 total_count 相等
    And 筛选结果只包含 1 条 call_type 为 "search" 的行

  Scenario: parent_call_id 过滤走 JSON 路径，列表与计数同口径
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入 1 条 LLM 行 + 1 条 search 行并按 "parent_call_id=bdd-s137-llm" 过滤查询 spend logs
    Then 响应状态码为 200
    And 筛选结果的 count 与 total_count 相等
    And 筛选结果只包含 1 条 call_type 为 "search" 的行

  Scenario: 无匹配的 parent_call_id 返回空而非报错
    Given 上游 litellm 数据库连接已配置
    When 向 aigw 测试库灌入 1 条 LLM 行 + 1 条 search 行并按 "parent_call_id=does-not-exist" 过滤查询 spend logs
    Then 响应状态码为 200
    And 筛选结果的 count 与 total_count 相等
    And 筛选结果只包含 0 条 call_type 为 "search" 的行
