@mock
Feature: 代理服务管理 CRUD — /admin/proxies/*（Stage 122）

  Background:
    Given 管理员已认证

  Scenario: 创建代理并返回加密/redact 的 proxy_url
    Given 数据库已初始化且已配置 master key
    When 发送 POST /admin/proxies 请求创建代理
      """
      {"name": "proxy-1", "proxy_url": "http://user:secret@1.2.3.4:8080"}
      """
    Then 响应状态码为 200
    And 响应包含 name 字段值为 "proxy-1"
    And 响应 proxy_url 字段已 redact 不包含明文密码

  Scenario: 列表分页展示代理（含探测快照字段）
    Given 数据库已初始化且已配置 master key
    And 已创建代理 "proxy-a" 带探测快照
    When 发送 GET /admin/proxies 请求
    Then 响应状态码为 200
    And 响应中的 data 包含 1 个代理

  Scenario: 获取代理详情
    Given 数据库已初始化且已配置 master key
    And 已创建代理 "proxy-detail"
    When 发送 GET /admin/proxies/{id} 请求
    Then 响应状态码为 200
    And 响应包含 name 字段值为 "proxy-detail"

  Scenario: 更新代理
    Given 数据库已初始化且已配置 master key
    And 已创建代理 "proxy-update"
    When 发送 PUT /admin/proxies/{id} 请求更新名称为 updated-name
    Then 响应状态码为 200
    And 响应包含 name 字段值为 "updated-name"

  Scenario: 删除代理
    Given 数据库已初始化且已配置 master key
    And 已创建代理 "proxy-delete"
    When 发送 DELETE /admin/proxies/{id} 请求
    Then 响应状态码为 200
    And 响应包含 status 字段值为 "deleted"

  Scenario: 删除被凭证引用的代理返回 409 PROXY_IN_USE
    Given 数据库已初始化且已配置 master key
    And 已创建代理 "proxy-in-use"
    And 已存在凭证 "oauth-cred" 引用该代理
    When 发送 DELETE /admin/proxies/{id} 请求
    Then 响应状态码为 409
    And 响应错误 type 为 "PROXY_IN_USE"

  Scenario: 非 admin key 访问代理接口返回 403
    Given 数据库已初始化且已配置 master key
    And 已生成普通 key "proxy-nonadmin"（代理场景）
    When 使用普通 key "proxy-nonadmin" 发送 GET /admin/proxies 请求
    Then 响应状态码为 403

  Scenario: 批量删除代理（in-use 跳过）
    Given 数据库已初始化且已配置 master key
    And 已创建代理 "batch-a"
    And 已创建代理 "batch-b"
    When 发送 POST /admin/proxies/batch-delete 请求删除两个代理
    Then 响应状态码为 200
    And 批量删除结果中包含 2 个已删除 id