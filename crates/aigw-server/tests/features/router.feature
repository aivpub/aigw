@mock
Feature: Router 智能路由 — weighted / cooldown / merge（Stage 118）

  Scenario: weighted 路由按 weight 比例分发到多个 upstream
    Given mock 上游已启动
    And 已配置 model "router-w" 且上游 model 为 "upstream-heavy" 指向 mock 上游
    And 已配置 model "router-w" 且上游 model 为 "upstream-light" 指向 mock 上游
    And 一个普通 key "router-w-key" 已生成且绑定模型 "router-w"
    And 更新 model "router-w" 的上游 "upstream-heavy" weight=10
    When 使用 key "router-w-key" 发送 POST /chat/completions 请求用 model "router-w"
    Then 响应状态码为 200
    And mock 上游收到的请求中 upstream_model "upstream-heavy" 的数量大于 "upstream-light"

  Scenario: cooldown 后同 deployment 不再被选中（429 触发排除）
    Given mock 上游已启动
    And 已配置 model "router-cd" 且上游 model 为 "upstream-cd-a" 指向 mock 上游
    And 已配置 model "router-cd" 且上游 model 为 "upstream-cd-b" 指向 mock 上游
    And 一个普通 key "router-cd-key" 已生成且绑定模型 "router-cd"
    And 更新 key "router-cd-key" 的 router 设置 allowed_fails=1 cooldown_time=60
    And mock 上游 "/v1/chat/completions" 返回状态码 429
    When 使用 key "router-cd-key" 发送 POST /chat/completions 请求用 model "router-cd"
    And 使用 key "router-cd-key" 发送 POST /chat/completions 请求用 model "router-cd"
    Then mock 上游收到的请求中包含两个不同 upstream_model
