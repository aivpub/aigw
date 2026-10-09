@mock
Feature: 搜索 provider / 实例管理 — /admin/websearch/*（Stage 138）

  Background:
    Given 管理员已认证

  Scenario: 创建 provider 返回缺省单价 0.01 且为默认
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/websearch/providers 请求创建 provider
      """
      {"name": "searx-a", "kind": "searxng"}
      """
    Then 响应状态码为 200
    And 响应包含 name 字段值为 "searx-a"
    And 搜索 provider 单价为 0.01
    And 搜索 provider 为默认

  Scenario: 创建 provider 后添加实例并通过列表可见
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-list"
    When 发送 POST /admin/websearch/providers/{id}/instances 请求添加实例
      """
      {"name": "inst-1", "base_url": "http://127.0.0.1:9099"}
      """
    Then 响应状态码为 200
    And 搜索实例 has_api_key 为 false
    When 发送 GET /admin/websearch/providers 请求
    Then 响应状态码为 200
    And 列表首个 provider 的实例数为 1

  Scenario: 实例 api_key 加密落库且响应 redact
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-key"
    When 发送 POST /admin/websearch/providers/{id}/instances 请求添加实例
      """
      {"name": "inst-key", "base_url": "http://127.0.0.1:9099", "api_key": "sk-secret-abcdef"}
      """
    Then 响应状态码为 200
    And 搜索实例 has_api_key 为 true
    And 搜索实例 redacted_api_key 以 "****" 开头
    And 响应 JSON 不含明文 "sk-secret-abcdef"
    And 搜索实例落库 api_key 带 v2:gcm: 前缀

  Scenario: 更新实例不传 api_key 保留已存密钥
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-keep"
    And 已为该 provider 创建带 api_key 的实例
    When 发送 PUT /admin/websearch/instances/{id} 请求更新实例名称为 "inst-renamed"
    Then 响应状态码为 200
    And 搜索实例落库 api_key 与创建后一致

  Scenario: 不支持的 kind 返回 400
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/websearch/providers 请求创建 provider
      """
      {"name": "bad-kind", "kind": "tavily"}
      """
    Then 响应状态码为 400
    And 响应错误 type 为 "unsupported_provider_kind"
    And 响应错误 message 含 "tavily"
    And 响应错误 message 含 "searxng"

  Scenario: 未知 kind 返回 400 且提示 unknown
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/websearch/providers 请求创建 provider
      """
      {"name": "unknown-kind", "kind": "exa"}
      """
    Then 响应状态码为 400
    And 响应错误 type 为 "unsupported_provider_kind"
    And 响应错误 message 含 "unknown"

  Scenario: 重名 provider 返回 409
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-dup"
    When 发送 POST /admin/websearch/providers 请求创建 provider
      """
      {"name": "searx-dup", "kind": "searxng"}
      """
    Then 响应状态码为 409
    And 响应错误 type 为 "DUPLICATE_PROVIDER_NAME"

  Scenario: 删除默认 provider 返回 409
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-default"
    When 发送 DELETE /admin/websearch/providers/{id} 请求
    Then 响应状态码为 409

  Scenario: 批量删除跳过默认 provider 只删其余
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-first"
    And 已通过 API 创建搜索 provider "searx-second"
    When 发送 POST /admin/websearch/providers/batch-delete 请求删除两个 provider
    Then 响应状态码为 200
    And 批量删除结果中包含 1 个已删除 id
    And 批量删除结果中跳过 1 个 provider

  Scenario: 删除 provider 级联删除其实例
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-default-keep"
    And 已通过 API 创建搜索 provider "searx-cascade"
    And 已为该 provider 创建实例 "inst-cascade"
    When 发送 DELETE /admin/websearch/providers/{id} 请求
    Then 响应状态码为 200
    And 该 provider 的实例数在数据库中为 0

  Scenario: toggle provider 翻转 enabled
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-toggle"
    When 发送 POST /admin/websearch/providers/{id}/toggle 请求
    Then 响应状态码为 200
    And 搜索 provider enabled 为 false

  Scenario: 探测不可达实例写快照且不报 5xx
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-probe"
    And 已为该 provider 创建实例 "inst-dead"
    When 发送 POST /admin/websearch/instances/{id}/test 请求
    Then 响应状态码为 200
    And 探测结果 ok 为 false
    And 该实例的 probe_result 快照非空

  Scenario: 改单价后自动 reload 生效
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-price"
    When 发送 PUT /admin/websearch/providers/{id} 请求把单价改为 0.000012
    Then 响应状态码为 200
    And 搜索 provider 单价为 0.000012
    And 响应 reload 成功

  Scenario: DB 中的非法 kind 行不阻止 reload
    Given 数据库已初始化且已配置 master key
    And 已通过 API 创建搜索 provider "searx-good"
    And 数据库直插一个 kind 为 "tavily" 的搜索 provider 行
    When 发送 POST /admin/websearch/reload 请求
    Then 响应状态码为 200
    And 响应包含 status 字段值为 "ok"

  Scenario: 非 admin key 访问搜索管理接口返回 403
    Given 数据库已初始化且已配置 master key
    And 已生成普通 key "ws-nonadmin"（搜索场景）
    When 使用普通 key "ws-nonadmin" 发送 GET /admin/websearch/providers 请求
    Then 响应状态码为 403
