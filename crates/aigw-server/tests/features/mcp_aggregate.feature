@mock
Feature: MCP 聚合端点 — /mcp（Stage 144）

  Background:
    Given 管理员已认证
    And 数据库已初始化且已配置 master key
    And 已生成聚合场景的普通 key

  # ── 合并与前缀 ────────────────────────────────────────────

  Scenario: 两个上游的工具合并到同一个目录且带各自的前缀
    Given 已启动聚合上游 "alpha" 提供工具 "search,read"
    And 已启动聚合上游 "beta" 提供工具 "write"
    When 用 master key 向聚合端点发送 tools/list
    Then 响应状态码为 200
    And 聚合目录工具数为 3
    And 聚合目录包含工具 "alpha__read"
    And 聚合目录包含工具 "alpha__search"
    And 聚合目录包含工具 "beta__write"
    And 聚合目录不含工具 "search"

  Scenario: 工具名含下划线时按第一个双下划线拆分
    Given 已启动聚合上游 "gh" 提供工具 "get_user_issue"
    When 用 master key 调用聚合工具 "gh__get_user_issue"
    Then 响应状态码为 200
    And 上游 "gh" 依次收到 MCP 方法 "tools/call"
    And 上游 "gh" 收到的工具名为 "get_user_issue"

  # ── 调用路由 ──────────────────────────────────────────────

  Scenario: 前缀决定调用落到哪个上游
    Given 已启动聚合上游 "alpha" 提供工具 "search,read"
    And 已启动聚合上游 "beta" 提供工具 "write"
    When 用 master key 调用聚合工具 "beta__write"
    Then 响应状态码为 200
    And 上游 "beta" 依次收到 MCP 方法 "tools/call"
    And 上游 "alpha" 没有收到任何 MCP 请求

  Scenario: 未知前缀是 404 -32601 而不是 500
    Given 已启动聚合上游 "alpha" 提供工具 "search,read"
    When 用 master key 调用聚合工具 "ghost__search"
    Then 响应状态码为 404
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32601

  Scenario: 没有前缀的工具名是 404 -32601
    Given 已启动聚合上游 "alpha" 提供工具 "search,read"
    When 用 master key 调用聚合工具 "search"
    Then 响应状态码为 404
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32601

  # ── 同名工具 ──────────────────────────────────────────────

  Scenario: 两个上游提供同名工具时前缀让它们同时可见
    Given 已启动聚合上游 "alpha" 提供工具 "search"
    And 已启动聚合上游 "beta" 提供工具 "search"
    When 用 master key 向聚合端点发送 tools/list
    Then 响应状态码为 200
    And 聚合目录工具数为 2
    And 聚合目录包含工具 "alpha__search"
    And 聚合目录包含工具 "beta__search"

  # ── user_required 的列表侧与调用侧 ────────────────────────

  Scenario: user_required 且未配置个人 token 时该上游被静默剔除
    Given 已启动要求个人凭据的聚合上游 "vault"
    And 已启动聚合上游 "alpha" 提供工具 "search,read"
    When 用普通 key 向聚合端点发送 tools/list
    Then 响应状态码为 200
    And 聚合目录工具数为 2
    And 聚合目录不含工具 "vault__secret"

  Scenario: 补上个人 token 后该上游的工具出现
    Given 已启动要求个人凭据的聚合上游 "vault"
    When 该普通 key 为上游 "vault" 设置个人 token
    Then 响应状态码为 200
    When 用普通 key 向聚合端点发送 tools/list
    Then 响应状态码为 200
    And 聚合目录工具数为 1
    And 聚合目录包含工具 "vault__secret"

  Scenario: user_required 且无个人 token 时显式调用是 403
    Given 已启动要求个人凭据的聚合上游 "vault"
    When 用普通 key 调用聚合工具 "vault__secret"
    Then 响应状态码为 403
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32003

  # ── ACL ───────────────────────────────────────────────────

  Scenario: ACL 拒绝只封调用，列表仍然照给
    Given 已启动聚合上游 "alpha" 提供工具 "search,read"
    And 管理员把 "alpha" 的工具 "read" 对当前普通 key 设为禁止
    When 用普通 key 向聚合端点发送 tools/list
    Then 响应状态码为 200
    And 聚合目录包含工具 "alpha__read"
    When 用普通 key 调用聚合工具 "alpha__read"
    Then 响应状态码为 403
    And 响应 JSON 路径 "error.code" 的 JSON 数字为 -32003
    And 上游 "alpha" 没有收到工具调用

  # ── 调用日志 ──────────────────────────────────────────────

  Scenario: 聚合调用写下的日志能按 alias 查回来
    Given 已启动聚合上游 "alpha" 提供工具 "search,read"
    And 已启动聚合上游 "beta" 提供工具 "write"
    When 用 master key 调用聚合工具 "beta__write"
    Then 响应状态码为 200
    And 最近一条聚合调用的 server_alias 为 "beta"
    And 最近一条聚合调用的 method 为 "tools/call"
    When 管理员按 server_alias "beta" 查询调用日志
    Then 响应状态码为 200
    And 调用日志 total_count 为 1
    And 调用日志第一行 tool_name 为 "write"
