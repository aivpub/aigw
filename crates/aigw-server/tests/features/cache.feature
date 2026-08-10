@mock
Feature: exact-match 响应缓存 — HIT/MISS + no-store + 计费 0 元（Stage 119）

  Scenario: 首次请求 MISS → 相同 body 二次请求 HIT
    Given mock 上游已启动
    And 已配置 model "cache-m" 指向 mock 上游
    And 一个普通 key "cache-key" 已生成且绑定模型 "cache-m"
    When 使用 key "cache-key" 发送 POST /chat/completions 请求用 model "cache-m"
    Then 响应状态码为 200
    And 响应头包含 X-Cache-Status "MISS"
    When 使用 key "cache-key" 发送 POST /chat/completions 请求用 model "cache-m"
    Then 响应状态码为 200
    And 响应头包含 X-Cache-Status "HIT"

  Scenario: no-store 绕过缓存（每次请求都 MISS + 上游被调）
    Given mock 上游已启动
    And 已配置 model "cache-ns" 指向 mock 上游
    And 一个普通 key "cache-ns-key" 已生成且绑定模型 "cache-ns"
    When 使用 key "cache-ns-key" 发送 POST /chat/completions 请求用 model "cache-ns" 带 cache no-store
    Then 响应状态码为 200
    And 响应头包含 X-Cache-Status "MISS"
    When 使用 key "cache-ns-key" 发送 POST /chat/completions 请求用 model "cache-ns" 带 cache no-store
    Then 响应状态码为 200
    And 响应头包含 X-Cache-Status "MISS"

  Scenario: cache-hit 计费 0 元（命中请求 SpendLog spend=0 + cached=1）
    Given mock 上游已启动
    And 已配置 model "cache-bill" 指向 mock 上游
    And 一个普通 key "cache-bill-key" 已生成且绑定模型 "cache-bill"
    When 使用 key "cache-bill-key" 发送 POST /chat/completions 请求用 model "cache-bill"
    Then 响应状态码为 200
    When 使用 key "cache-bill-key" 发送 POST /chat/completions 请求用 model "cache-bill"
    Then 响应头包含 X-Cache-Status "HIT"
    And SpendLog 中最近一条记录 spend 为 0
    And SpendLog 中最近一条记录 cached 为 1
