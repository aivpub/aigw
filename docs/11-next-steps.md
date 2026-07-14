# aigw -- 下一步行动

**上次更新**: 2026-07-14
**当前阶段**: Phase 17 代理转发架构重构（P1）

---

## 当前状态：Phase 17 代理转发架构重构（Stages 50-52）

### 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅ 项目基础设施 + 功能对等 + 部署就绪
Phase 5:    ████████████████████ 100% (6/6)  ✅ 最小化后端 + BDD 测试
Phase 7:    ████████████████████ 100% (5/5)  ✅ 生产 litellm 迁移
Phase 8:    ████████████████████ 100% (3/3)  ✅ 生产化基础（日志/多租户/健康检查）
Phase 9:    ████████████████████ 100% (4/4)  ✅ 前端管理控制台
Phase 11:   ████████████████████ 100% (6/6)  ✅ 前端质量加固 + 安全达标
Phase 12:   ████████████████████ 100% (3/3)  ✅ 前端导航重构 + Playground
Phase 13:   ████████████████████ 100% (6/6)  ✅ 前端反馈改进（Stages 34-39）
Phase 14:   ████████████████████ 100% (4/4)  ✅ /v1/messages 接口修复（Stages 40-43）
Phase 15:   ████████████████████ 100% (3/3)  ✅ 反馈改进（Stages 44-46）
Phase 16:   ████████████████████ 100% (3/3)  ✅ Playground 增强（Stages 47-49）
Phase 17:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3)  🔄 代理转发架构重构（Stages 50-52）
```

### 测试状态

| 层 | 框架 | 通过 |
|---|------|------|
| 后端单元 | libtest | 316 tests |
| 后端 BDD | cucumber-rust | 92 scenarios (353 steps) |
| 前端 BDD | Playwright + playwright-bdd | 108 tests (36 scenarios × 3 viewports) |

---

## 优先级排序

| 优先级 | Phase | 目标 | 原因 |
|--------|-------|------|------|
| **P1** | Phase 17 (Stages 50-52) | 代理转发架构重构 | 消除 chat.rs/v1_messages.rs 重复逻辑，为后续 Provider/Router 扩展打基础 |
| P2 | LT-Router | Router 负载均衡（多 deployment 选择 + cooldown） | 多实例 upstream 需求 |
| P2 | LT-Usage | Usage 多视角聚合（Global/Team/Org/Key） | 前端用户反馈 |
| P3 | LT-Native | Anthropic 原生上游适配 | 需直接调 Anthropic Messages API |

---

## Phase 17: 代理转发架构重构（P1，预估 12h）

当前 `chat.rs` 和 `v1_messages.rs` 各自独立 resolve upstream（~230 行重复逻辑），`DefaultAdapter` 写死单一实现且不支持 tool_use/tool_result 转换，`provider_registry`/`router_state` 在 AppState 中定义但从未使用。

| Stage | 目标 | 预估 |
|-------|------|------|
| Stage 50 | ModelResolver + Deployment — 新建 `deployment.rs` + `resolver.rs`，迁移 `resolve_upstream_params` 为 `ModelResolver::resolve() → Vec<Deployment>` | 3h |
| Stage 51 | MessageAdapter + tool 转换 — `MessageAdapter` trait + `OpenAIPassthrough` + `AnthropicToOpenAI`（含 tool_use/tool_result ↔ tool_calls）+ `select_adapter()` | 5h |
| Stage 52 | Handler 瘦身 — chat.rs / v1_messages.rs 通用逻辑下沉到 ModelResolver + MessageAdapter | 3h |

**依赖关系**: Stage 50 → 51 → 52（串行，渐进式重构）。预估 12h。

**TDD 要求**: 每个 Stage 先写测试（UT + BDD scenario），RED → GREEN → REFACTOR 循环，测试全部通过后才可 commit。

**设计文档**: `docs/plans/2026-07-13-arch-refactor-plan.md`

### 新增核心组件

| 组件 | 命名 | 职责 |
|------|------|------|
| 模型解析层 | `ModelResolver` | model_name → Vec<Deployment>（查 proxy_models、解密、解析 credential、提取定价） |
| 消息格式转换 | `MessageAdapter` trait | OpenAI Chat ↔ Anthropic Messages 双向转换（含 tool_use/tool_result ↔ tool_calls） |
| 流式转换器 | `StreamAdapter` trait | SSE chunk 逐块转换（`&mut self` 维护跨 chunk 状态如 tool_use index） |
| 上游配置值对象 | `Deployment` | 纯值：api_base / api_key / upstream_model / provider_type / 定价 |

---

## 后续路线

| ID | 主题 | 优先级 | 触发条件 |
|----|------|--------|---------|
| LT-Router | Router 负载均衡（多 deployment 选择 + cooldown + fallback） | P2 | 多实例 upstream 需求 |
| LT-Usage | Usage 多视角聚合（Global/Team/Org/Key 下拉框 + 饼图联动） | P2 | 前端用户反馈 |
| LT-Native | Anthropic 原生上游适配（OpenAIToAnthropic + AnthropicPassthrough） | P3 | 需直接调 Anthropic Messages API |
| LT-Redis | Redis 缓存 + 性能优化 | P2 | QPS > 1000 |
| LT-Observ | Observability (Prometheus + OTEL) | P2 | 生产环境部署 |
| LT-SSO | SSO/OAuth 鉴权 | P3 | 企业客户需求 |
| LT-PG | PostgreSQL 生产级支持 | P2 | 多实例 + 高可用 |
| LT-K8s | Kubernetes Operator | P3 | 云原生需求 |

---

## 技术债

| 编号 | 状态 | 说明 |
|------|------|------|
| TD-001~007 | ✅ | 已解决 |

## ADR 记录

| 编号 | 决策 | 日期 |
|------|------|------|
| ADR-013 | `/v1/messages` 接口审计 — 7 bugs（2 CRITICAL） | 2026-07-11 |
| ADR-014 | 当前无 Provider 适配架构 — 仅单一 DefaultAdapter | 2026-07-11 |
| ADR-015 | 架构重构优先于功能增强 — Phase 17 替换为 ModelResolver + MessageAdapter | 2026-07-14 |

## 当前审核中的文件（未提交）

```
新增文件:
  docs/plans/2026-07-13-arch-refactor-plan.md  (架构重构方案，v2)
  docs/plans/2026-07-14-roadmap-update-plan.md  (本次 roadmap 更新方案)

修改文件:
  docs/stages/stage-roadmap.md
  docs/11-next-steps.md
  docs/08-autonomous-decisions.md

删除文件:
  docs/stages/stage-50.md   (旧 Usage 多视角聚合)
  docs/stages/stage-51.md   (旧 Usage 多视角聚合)
```
