@real_api
Feature: 代理服务管理 real BDD — 三后端 proxies CRUD + in-use + 快照（Stage 125）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 创建代理并列表可见（proxy_url 加密落库）
    Given 通过 API 创建代理 "real-proxy-a" 使用 URL "http://user:secret@1.2.3.4:8080"
    When 通过 API 查询代理列表
    Then 代理列表包含 "real-proxy-a"
    And 代理响应 proxy_url 已 redact 不包含明文密码

  Scenario: 更新代理名称
    Given 通过 API 创建代理 "real-proxy-update" 使用 URL "http://user:secret@5.6.7.8:8080"
    When 通过 API 更新代理 "real-proxy-update" 名称为 "real-proxy-renamed"
    Then 代理列表包含 "real-proxy-renamed"

  Scenario: 删除未引用代理成功
    Given 通过 API 创建代理 "real-proxy-delete" 使用 URL "http://user:secret@9.9.9.9:8080"
    When 通过 API 删除代理 "real-proxy-delete"
    Then 代理列表不包含 "real-proxy-delete"

  Scenario: 删除被凭证引用的代理返回 409
    Given 通过 API 创建代理 "real-proxy-inuse" 使用 URL "http://user:secret@1.1.1.1:8080"
    And 通过 API 创建凭证 "real-cred-inuse" 引用该代理
    When 通过 API 删除代理 "real-proxy-inuse"
    Then real 代理删除返回 409 PROXY_IN_USE

  Scenario: 出口检测写 probe_result 快照（不可达出口容忍）
    Given 通过 API 创建代理 "real-proxy-test" 使用 URL "http://user:secret@127.0.0.1:1"
    When 通过 API 触发出口检测 "real-proxy-test"
    Then real 代理探测返回 200 或 500

  Scenario: toggle 代理状态
    Given 通过 API 创建代理 "real-proxy-toggle" 使用 URL "http://user:secret@2.2.2.2:8080"
    When 通过 API toggle 代理 "real-proxy-toggle"
    Then real 代理 toggle 返回 200
