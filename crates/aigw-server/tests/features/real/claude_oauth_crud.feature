@real_api @needs_upstream_db
Feature: Claude OAuth 凭证 real BDD — 三后端 CRUD + 加密落库 + in-use 守卫（Stage 130）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 创建 OAuth 凭证并列表/详情可见（token 三件套 redact）
    Given 通过 API 创建 OAuth 凭证 "real-oauth-a" 带 cookie "sk-ant-sid-real-a"
    When 通过 API 查询凭证列表
    Then 凭证列表包含 "real-oauth-a"
    And 凭证响应 token 三件套已 redact 为 ***
    When 通过 API 查询凭证详情 "real-oauth-a"
    Then 凭证详情 token 三件套已 redact 为 ***

  Scenario: OAuth 凭证密文落库（DB 无明文，只读断言）
    Given 通过 API 创建 OAuth 凭证 "real-oauth-enc" 带 cookie "sk-ant-sid-real-enc"
    When 通过 API 查询凭证详情 "real-oauth-enc"
    Then real 凭证 DB 直读 credential_values 含 v2:gcm: 前缀且无明文 sk-ant- 子串

  Scenario: 删除 OAuth 凭证成功
    Given 通过 API 创建 OAuth 凭证 "real-oauth-del" 带 cookie "sk-ant-sid-real-del"
    When 通过 API 删除凭证 "real-oauth-del"
    Then real 凭证删除返回 200

  Scenario: 删除被 OAuth 凭证引用的代理返回 409
    Given 通过 API 创建代理 "real-oauth-proxy" 使用 URL "http://user:secret@1.2.3.4:8080"
    And 通过 API 创建 OAuth 凭证 "real-oauth-inuse" 引用该代理
    When 通过 API 删除代理 "real-oauth-proxy"
    Then real 代理删除返回 409 PROXY_IN_USE

  Scenario: 代理出口检测写 probe_result 快照（不可达出口容忍）
    Given 通过 API 创建代理 "real-oauth-probe" 使用 URL "http://user:secret@127.0.0.1:1"
    When 通过 API 触发出口检测 "real-oauth-probe"
    Then real 代理探测返回 200 或 500
