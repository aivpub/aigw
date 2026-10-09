@real_api
Feature: 搜索 provider / 实例管理 real BDD — 三后端 CRUD + 加密落库 + 守卫（Stage 138）

  Background:
    Given AIGW_REAL_API=1 且 API keys 已配置

  Scenario: 创建 provider 与实例并列表可见
    Given 通过 API 创建搜索 provider "real-searx-a"
    And 通过 API 为该 provider 添加实例 "real-inst-a"
    When 通过 API 查询搜索 provider 列表
    Then 搜索 provider 列表包含 "real-searx-a"
    And 该 provider 列表项含实例 "real-inst-a"

  Scenario: 实例 api_key 加密落库且响应 redact
    Given 通过 API 创建搜索 provider "real-searx-key"
    When 通过 API 为该 provider 添加带 api_key "sk-real-secret-9f2c" 的实例 "real-inst-key"
    Then 搜索实例响应 has_api_key 为 true
    And 搜索实例响应 redacted_api_key 以 "****" 开头
    And 实例 api_key 落库带 v2:gcm: 前缀且无明文
    And real 搜索实例详情响应无明文 "sk-real-secret-9f2c"

  Scenario: 更新实例名称
    Given 通过 API 创建搜索 provider "real-searx-upd"
    And 通过 API 为该 provider 添加实例 "real-inst-upd"
    When 通过 API 更新搜索实例 "real-inst-upd" 名称为 "real-inst-renamed"
    Then 搜索 provider 列表包含实例 "real-inst-renamed"

  Scenario: 删除默认 provider 返回 409
    Given 通过 API 创建搜索 provider "real-searx-guard" 且 failover_order 为 -999
    When 通过 API 删除搜索 provider "real-searx-guard"
    Then real 搜索 provider 删除返回 409

  Scenario: 删除非默认 provider 后其实例一并消失
    Given 通过 API 创建搜索 provider "real-searx-keep"
    And 通过 API 创建搜索 provider "real-searx-cascade"
    And 通过 API 为该 provider 添加实例 "real-inst-cascade"
    When 通过 API 删除搜索 provider "real-searx-cascade"
    Then real 搜索 provider 删除返回 200
    And 该 provider 的实例在库中已不存在
    And 该 provider 在库中已不存在

  Scenario: toggle provider 状态
    Given 通过 API 创建搜索 provider "real-searx-toggle"
    When 通过 API toggle 搜索 provider "real-searx-toggle"
    Then real 搜索 provider toggle 返回 200
