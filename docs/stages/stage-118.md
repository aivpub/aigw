# Stage 118: Router 智能路由接线 + weighted + cooldown/fallback（S2）

**所属**: Phase 47（A 类接线 + 缓存）
**预估**: 14h（后端 + 测试 + 前端）
**依赖**: Stage 117（guard/身份上下文）
**状态**: ⏳ 待开始

---

## 1. 目标

让已声明但未接入请求路径的 Router 能力真实生效，并补 weighted 与错误类型 fallback：

1. **report_failure/report_success 接线** — cooldown 状态真实推进（当前 cooldown 实际不触发）
2. **merge_router_overrides 接线** — key>team>global 三级配置合并进请求路径
3. **weighted 路由** — 按 deployment weight/rpm/tpm 加权随机
4. **usage/latency 变体** — 真实负载决策（Stage 116 已声明 `usage-based-routing-v2`/`latency-based-routing` 变体）
5. **错误类型 fallback** — priority 排序 + 按 429/5xx/context-window/content-policy 切换
6. **前端启用** — RouterSettings `routing_strategy` 下拉解锁 usage/latency

> 差距报告 P0 第二项——策略代码在但运行时失效，cooldown 从不触发。

## 2. 现状证据（已核实）

| 项 | 现状 | 证据 |
|----|------|------|
| `pick_deployment` | 仅 SimpleShuffle + cooldown 过滤 | `router.rs`（626 行） |
| Strategy 其余值 | 声明但无真实决策 | `Strategy` 枚举 + FromStr（Stage 116 补 usage/latency 变体） |
| `report_failure`/`report_success` | 实现但请求路径零调用 | router.rs |
| `merge_router_overrides` | 实现但未接入 | router.rs |
| weighted | 无 | 无按 weight/rpm/tpm 加权随机 |
| fallback | 无 | 无按错误类型的 priority fallback |

## 3. 方案

### 3.1 report_failure / report_success 接线

上游调用返回路径（成功/失败/超时）调用 `Router::report_success`/`report_failure(instance, error_kind)`：

- 失败计数触发条件（对齐 litellm `cooldown_handlers.py`）：429 / 401 / 408 / 404 / 5xx 才计入；非这些（400 业务错）不计。
- 超过 `allowed_fails`（默认 1-3，配置化）→ deployment 进入 cooldown（时间 `cooldown_time` 秒，默认 30s）→ 期间 `pick_deployment` 排除。
- 成功重置失败计数。

### 3.2 merge_router_overrides 接线

请求入口按 key>team>global 优先级合并 `router_settings`（`merge_router_overrides` 已实现）→ 合并后的 `routing_strategy`/`weights`/`fallbacks` 用于本次路由选择。

### 3.3 weighted 路由

`pick_deployment` 在 SimpleShuffle 分支内按 weight/rpm/tpm 加权随机（对标 litellm `simple_shuffle.py:29-60` `random.choices` 归一化）：
- `weights` 未配置时回退均匀随机。
- weight 为 0 的 deployment 不参与。

### 3.4 usage / latency 变体

`pick_deployment` 按 Strategy 分支：
- **UsageBased**：按 5 分钟窗口 `RPM/TPM 剩余比例`（spend 表 + key 的 rpm/tpm_limit）选余量最大者。
- **Latency**：按 EWMA 响应时间（`report_*` 记录）选最小者；无样本时回退 SimpleShuffle。

### 3.5 错误类型 fallback

- 候选 deployment 列表按 `priority`（0 主 1 备，对标 Envoy provider-fallback）分组。
- 当前实例失败按错误类型（429 / 5xx / context-window / content-policy）触发切换到同组下一候选或下一 priority 组。
- 最多 N 次尝试（默认 3，配置化）。
- 流式场景：已产内容则抛原始错误（对标 litellm `MidStreamFallbackError`），未产内容走 fallback。

### 3.6 前端

`router-settings/index.tsx` 的 `routing_strategy` 下拉 `usage-based-routing`/`latency-based-routing` 选项从 `disabled` 改为可启用；补 weight/rpm/tpm 输入（Deployment 级）。

## 4. 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `crates/aigw-core/src/router.rs` | 修改 | weighted/usage/latency 分支 + report_* 接线 + fallback 模块 |
| `crates/aigw-core/src/router_strategy.rs`（如存在）| 修改 | 策略实现 |
| `crates/aigw-server/src/routes/chat.rs` 等 | 修改 | report_* + merge_router_overrides 调用 + fallback 循环 |
| `crates/aigw-frontend/src/pages/router-settings/index.tsx` | 修改 | 解锁 usage/latency + weight 输入 |
| `crates/aigw-frontend/src/i18n/locales/{en,zh-CN}.json` | 修改 | 新增 weight/rpm/tpm label |

## 5. TDD

- **router UT**（10-12）：weighted 加权随机命中率（statistical）/ 零权重排除 / report_failure 触发 cooldown / cooldown 期间排除 / 429 vs 400 计数差异 / usage 变体选余量最大 / latency 变体选 EWMA 最小 / 无样本回退 / fallback priority 分组切换 / 错误类型触发 / 最多 N 次尝试 / merge_overrides 优先级 key>team>global。
- **handler UT**（2-4）：fallback 循环 + report_* 调用。
- **mock BDD**（4-6）：cooldown 排除（失败 N 次后命中被跳过）/ weighted 路由命中 / usage 选择 / fallback 切换 429→下一优先组。
- **fe-bdd**（2 场景 × 3 viewports）：下拉启用 usage/latency + weight 输入。

## 6. 验收标准

- [ ] `task test` / `task bdd` / `task fe-bdd` 全绿
- [ ] `task fmt` / `task lint` 全绿
- [ ] cooldown 真实排除（BDD 断言失败后命中跳过）；weighted 命中率符合权重；usage/latency 选择生效；fallback 429→下一优先组
- [ ] 前端下拉启用 + weight 输入可用

## 7. 参考实现

- litellm `router_strategy/simple_shuffle.py:29-60`（weighted `random.choices`）
- litellm `cooldown_handlers.py:61`（429/401/408/404/5xx 才 cooldown）
- litellm `router.py:2155`（`FallbackStreamWrapper` 流中 fallback）
- Envoy provider-fallback（priority 排序）/ agentgateway priority failover
