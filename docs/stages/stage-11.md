# Stage 11: Usage 用量查询增强（BDD 驱动）

**Status**: Planning
**Phase**: Phase 5 — 最小化后端补齐（RGR 驱动）
**预估工时**: 2-3h
**依赖**: Stage 7（BDD 框架）、Stage 8（proxy_models 表，用于 provider 聚合 JOIN）

## Goal

增强 Usage 用量查询能力，新增按 model / provider 维度聚合端点，增强 `/spend/logs` 过滤参数。对齐 litellm proxy 的 spend 查询接口。

> **范围说明**：原 Stage 11 包含 `/v1/key/*` 别名端点，经核实 litellm 源码（`litellm/proxy/management_endpoints/key_management_endpoints.py` + `proxy_server.py:15638` 无 prefix 挂载）确认 litellm **没有** `/v1/key/*` 路由，key 管理只走 `/key/*`。为保持 litellm 兼容性，aigw 保持 `/key/*` 单一前缀，删除别名部分。本 Stage 仅保留 Usage 增强。

## 删除范围（不做）

- ~~`/v1/key/*` 别名端点~~ — litellm 无此路由，aigw 不引入
- ~~v1_keys.rs / v1_keys.feature / v1_keys_steps.rs~~ — 不需要

## Usage 用量增强

### 新增端点

1. **GET /spend/models** — 按 model 聚合用量
   ```json
   {"data":[{"model":"gpt-4","total_tokens":1234,"total_spend":5.67,"requests":10}]}
   ```
2. **GET /global/spend/models** — 全局按 model 聚合
3. **GET /spend/providers** — 按 provider 聚合（基于 `proxy_models.model_params.provider`）

### 增强 /spend/logs
- 新增 `model` 过滤参数
- 新增 `provider` 过滤参数
- 新增 `start_date` / `end_date` 时间范围参数

### 数据来源
- `spend_logs` 表现有字段：`model` 已有，`provider` 需从 `proxy_models.model_params.provider` JOIN 获取
- 若 `spend_logs` 无 `provider` 列，通过 `model_name` 反查 `proxy_models` 表

## 关键交付件

1. `crates/aigw-server/src/routes/spend_models.rs` — `/spend/models` `/global/spend/models`
2. `crates/aigw-server/src/routes/spend_providers.rs` — `/spend/providers`
3. `crates/aigw-core/src/repositories/spend_repo.rs` — 增强查询（model/provider/date 过滤 + 聚合）
4. `tests/bdd/features/spend_enhanced.feature` — 用量聚合 BDD 场景
5. `tests/bdd/steps/spend_enhanced_steps.rs`

## BDD 场景

### spend_enhanced.feature

```gherkin
@mock
Feature: 用量查询增强

  Scenario: 按 model 聚合查询
    Given 已存在 gpt-4 的 5 条记录和 claude-3 的 3 条记录
    When 发送 GET /spend/models
    Then 响应包含 2 个 model 聚合
    And gpt-4 的 requests 为 5
    And claude-3 的 requests 为 3

  Scenario: 全局按 model 聚合
    Given 已存在用量记录
    When 发送 GET /global/spend/models
    Then 响应包含全局聚合

  Scenario: 按 provider 聚合查询
    Given 已存在 openai provider 的 4 条记录
    When 发送 GET /spend/providers
    Then 响应包含 openai 聚合

  Scenario: spend_logs 按 model 过滤
    Given 已存在 gpt-4 和 claude-3 各若干记录
    When 发送 GET /spend/logs?model=gpt-4
    Then 响应仅包含 gpt-4 记录

  Scenario: spend_logs 按时间范围过滤
    Given 已存在 2026-07-01 和 2026-07-03 的记录
    When 发送 GET /spend/logs?start_date=2026-07-02&end_date=2026-07-04
    Then 响应仅包含 2026-07-02 之后的记录

  Scenario: spend_logs 按 provider 过滤
    Given 已存在 openai 和 anthropic 各若干记录
    When 发送 GET /spend/logs?provider=openai
    Then 响应仅包含 openai provider 记录

  Scenario: 鉴权生效
    When 未带 Bearer token 发送 GET /spend/models
    Then 响应状态码为 401
```

## RGR 循环

1. **Red**: 写 `spend_enhanced.feature`（7 场景）→ 失败
2. **Green**: 实现 `/spend/models` `/global/spend/models` `/spend/providers` + 增强 `/spend/logs` → 逐场景通过
3. **Refactor**: 提取查询过滤条件构建到 `spend_repo::filter_builder`

## 验收标准

- [ ] `spend_enhanced.feature` ≥ 7 个 Scenario 全部通过
- [ ] `/spend/models` `/global/spend/models` `/spend/providers` 可用
- [ ] `/spend/logs` 支持 model/provider/date 过滤
- [ ] provider 聚合通过 JOIN proxy_models 获取
- [ ] 鉴权生效

## 风险

| 风险 | 缓解 |
|------|------|
| provider 聚合需 JOIN | spend_logs 无 provider 字段时，通过 model_name 反查 proxy_models |
| 时间范围跨方言 | SQLite/MySQL/PG 日期函数差异，使用 sqlx 编译期检查 |
