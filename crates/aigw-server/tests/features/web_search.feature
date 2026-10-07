@mock
Feature: 内建 web search 接线 — 三入口触发 + prompt 注入 + 降级（Stage 135）

  Background:
    Given mock 上游已启动

  Scenario: Chat web_search_options 触发搜索并注入
    Given 已配置 model "ws-chat" 指向 mock 上游
    And 一个普通 key "ws-chat-key" 已生成且绑定模型 "ws-chat"
    And mock 搜索后端已启动且 web_search 已配置
    When 使用 key "ws-chat-key" 发送带 web_search_options 的 POST /chat/completions 请求用 model "ws-chat"
    Then 响应状态码为 200
    And 搜索后端收到 1 次请求
    And mock 上游收到的请求 body 含 "Rust Async Book"
    And mock 上游收到的请求 body 不含 "web_search_options"

  Scenario: Responses web_search tool 触发搜索并注入
    Given 已配置 model "ws-resp" 指向 mock 上游
    And 一个普通 key "ws-resp-key" 已生成且绑定模型 "ws-resp"
    And mock 搜索后端已启动且 web_search 已配置
    When 使用 key "ws-resp-key" 发送带 web_search tool 的 POST /v1/responses 请求用 model "ws-resp"
    Then 响应状态码为 200
    And 搜索后端收到 1 次请求
    And mock 上游收到的请求 body 含 "Rust Async Book"
    And mock 上游收到的 tools 不含 "web_search"

  Scenario: Anthropic web_search_20250305 不再 500 并注入
    Given 已配置 model "ws-msg" 指向 mock 上游
    And 一个普通 key "ws-msg-key" 已生成且绑定模型 "ws-msg"
    And mock 搜索后端已启动且 web_search 已配置
    When 使用 key "ws-msg-key" 发送带 web_search_20250305 的 POST /v1/messages 请求用 model "ws-msg"
    Then 响应状态码为 200
    And 搜索后端收到 1 次请求
    And mock 上游收到的请求 body 含 "Rust Async Book"

  Scenario: 未配置 web_search 时带搜索工具仍成功
    Given 已配置 model "ws-none" 指向 mock 上游
    And 一个普通 key "ws-none-key" 已生成且绑定模型 "ws-none"
    And web_search 未配置
    When 使用 key "ws-none-key" 发送带 web_search_options 但不带 web_search 配置的 POST /chat/completions 请求用 model "ws-none"
    Then 响应状态码为 200
    And mock 上游收到请求

  Scenario: 搜索后端 500 时请求降级成功
    Given 已配置 model "ws-500" 指向 mock 上游
    And 一个普通 key "ws-500-key" 已生成且绑定模型 "ws-500"
    And mock 搜索后端已启动且 web_search 已配置
    And 搜索后端返回状态码 500
    When 使用 key "ws-500-key" 发送带 web_search_options 的 POST /chat/completions 请求用 model "ws-500"
    Then 响应状态码为 200
    And mock 上游收到的请求 body 不含 "Rust Async Book"

  Scenario: 搜索后端超时时请求降级成功
    Given 已配置 model "ws-to" 指向 mock 上游
    And 一个普通 key "ws-to-key" 已生成且绑定模型 "ws-to"
    And 搜索后端延迟 400 毫秒且 web_search 超时为 100 毫秒
    When 使用 key "ws-to-key" 发送带 web_search_options 的 POST /chat/completions 请求用 model "ws-to"
    Then 响应状态码为 200
    And mock 上游收到的请求 body 不含 "Rust Async Book"

  Scenario: 触发搜索的请求不命中 exact-match 缓存
    Given 已配置 model "ws-cache" 指向 mock 上游
    And 一个普通 key "ws-cache-key" 已生成且绑定模型 "ws-cache"
    And mock 搜索后端已启动且 web_search 已配置
    When 使用 key "ws-cache-key" 发送带 web_search_options 的 POST /chat/completions 请求用 model "ws-cache"
    Then 响应状态码为 200
    When 使用 key "ws-cache-key" 发送带 web_search_options 的 POST /chat/completions 请求用 model "ws-cache"
    Then 响应状态码为 200
    And 搜索后端收到 2 次请求

