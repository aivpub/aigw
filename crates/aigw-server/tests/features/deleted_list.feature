@mock
Feature: Deleted Entity Lists — 软删除实体归档查询

  Background:
    Given 数据库中已有 "team-1" 和 "team-2" 两个正常 team

  Scenario: GET /team/deleted 返回已删除 team
    Given team "team-x" 已被软删除
    When 使用 admin 认证发送 GET /team/deleted 请求
    Then 响应状态码为 200
    And "team-x" 在返回的 deleted 列表中
    And "team-1" 不在返回结果中

  Scenario: GET /model/deleted 返回已删除 model
    Given model "model-archived" 已被软删除
    When 使用 admin 认证发送 GET /model/deleted 请求
    Then 响应状态码为 200
    And "model-archived" 在返回的 deleted 列表中

  Scenario: GET /user/deleted 返回已删除 user
    Given user "deleted-user" 已被软删除
    When 使用 admin 认证发送 GET /user/deleted 请求
    Then 响应状态码为 200
    And "deleted-user" 在返回的 deleted 列表中

  Scenario: GET /org/deleted 返回已删除 org
    Given org "old-org" 已被软删除
    When 使用 admin 认证发送 GET /org/deleted 请求
    Then 响应状态码为 200
    And "old-org" 在返回的 deleted 列表中
