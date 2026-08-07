# aigw -- 下一步行动

**上次更新**: 2026-08-08
**当前阶段**: Phase 41 ⏳ 待开始（Stage 101-102 OpenAI Responses API，22h）；Phase 42 ✅ 完成（Stage 103-105 Playground 多模态图片，34.5h）；Phase 43 ⏳ 待开始（Stage 106-108 Image Token Usage Tracking，28h）

---

## 当前状态：103/108 Stages（98-100 + 103-105 已完成；101-102 + 106-108 待开始）

**2026-08-07**: Phase 40 全部完成（Stage 98-100 ✅）。Phase 41 规划落定——两 Stage 渐进交付（Passthrough 8h + Bridge 14h）。Phase 42 规划落定——Playground 多模态图片 3 Stage（Backend 6.5h + Frontend 16h + Render/Docs 12h），三路 subagent 并发实测代码改动量。**Phase 42 全部完成：Stage 103 ✅（`cd576dc`）+ Stage 104 ✅（`0f4868f`）+ Stage 105 ✅（图片渲染 + SpendLog 详情 + 文档，全量 frontend BDD 312 pass）。** Phase 43 规划落定——Image Token Usage Tracking 3 Stage（上游优先解析 + fallback 客户端估算），基于阿里云 DashScope 文档 + litellm/OpenRouter/OneAPI 源码调研确认：Qwen 返回 image_tokens（最完整），OpenAI/Anthropic 不返回，主流网关均不做预计算——aigw 将是行业差异化功能。

**2026-08-08**: 预算重置 cron 界面重构（预算重置 UI）——`GET /admin/budget-reset/stats` 端点（counts/preview/last_reset/next_tick_at）+ BudgetResetStatsCard（真实待重置数 / 上次重置 / 诚实 next-tick 倒计时）+ BudgetResetPreview（分实体明细 + 即将重置列表）+ BudgetResetTriggerDialog（范围选择 → 预览确认 → POST 后跳转 job 详情）+ job 表 trigger 列本地化 + job-detail formatStepResult 渲染 budget_reset 结果。TDD: 4 新 core UT + 2 后端 real BDD 场景 + 3 前端 BDD 场景 × 3 viewports。全部绿色（core 371 + bdd 215 + fe 87 jobs + 42 dashboard/i18n）。

**待办**:
1. **Phase 41 Stage 101**（P1, 8h）：`POST /v1/responses` Passthrough — 新建 handler + 路由注册 + Usage 双 fallback + TDD: 6 UT + 6 BDD
2. **Phase 41 Stage 102**（P1, 14h）：Responses→Chat 协议桥接 — `ResponsesToChatCompletions` 适配器 + 流式 SSE 事件映射 + TDD: 19 UT + 6 BDD
3. **Phase 43 Stage 106**（P1, 10h）：Image Token Engine — aigw-core 上游解析器 + fallback 估算 + header parser + 15 UT
4. **Phase 43 Stage 107**（P1, 10h）：Handler 集成 + Migration 025 + 8 BDD
5. **Phase 43 Stage 108**（P1, 8h）：前端展示 + Real API BDD + Docs
6. Phase 30（Stage 78-81）代码已落地 + Phase 31 修复完成，待一并回写为 ✅
4. TD-006 客户端 call_id 响应头回写
5. TD-007 soft_budget 告警通道（tracing::warn → webhook/email/Prometheus alert）
6. TD-009a/b/e 图片压缩 + 超大图 body 防御 + 外链渲染（视使用量触发）
7. 长期路线 LT-BodyMetrics/LT-BodyCompact/LT-BodyLifecycle 视数据量触发

---

## Phase 40: BDD Coverage Enhancement ✅ 完成

**2026-08-05 回写**: 以下 git log 确认 Stage 98-100 全部实际完成，roadmap 此前未同步——本次文档修正。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 98 | 路由端点 BDD 补全 — health 追加 5 场景 + router_settings.feature 新建 4 场景 + deleted_list.feature 新建 4 场景。共 13 mock BDD，3 个 feature 文件 | 测试 | 12h | ✅ 完成 |
| Stage 99 | 内部模块 + middleware 补测 — daily_spend_queue UT ×7 + rate_limiter 429 BDD ×3 + auth_gateway UT ×4 + rate_limit middleware UT ×5。共 19 测试 | 测试 | 14h | ✅ 完成 |
| Stage 100 | aigw-migrate 高级功能 BDD — precheck.feature ×4 + verify.feature ×2 + advanced.feature ×3 + cursor.feature ×2。共 11 real BDD（SQLite 全量 + PG/MySQL 选 5） | 测试 | 10h | ✅ 完成 |

**相关 commits**:
```
46e4c32 test(migrate): Stage 100 aigw-migrate pre-check + verify UT 6 tests
0888185 test(core): Stage 99 Part C+D — auth_gateway already has 6 UT + rate_limit 5 UT
3069dfd test(bdd): Stage 99 Part B — rate_limiter BDD 3 scenarios
8ccbba6 test(core): Stage 99 Part A — daily_spend_queue 7 UT
f191758 test(bdd): Stage 98 路由端点 BDD 补全 — 13 new mock BDD scenarios
2d31e97 docs(phase-40): add BDD coverage enhancement Stage 98-100 design docs
```

**设计文档**: `docs/plans/2026-08-03-bdd-coverage-enhancement-phase-39.md`、`docs/stages/stage-98~100.md`、`docs/research/2026-08-03-bdd-coverage-audit.md`

---

## Phase 41: OpenAI Responses API 接入 ⏳（22h，2 Stages）

**背景**: OpenAI 于 2025 年推出 Responses API（`POST /v1/responses`）。`/v1/responses` 上游生态极窄（仅 OpenAI + litellm），绝大多数 provider 只支持 `/v1/chat/completions`。分两阶段渐进交付：101 先做 Passthrough 让端点可用，102 加 Responses→Chat 协议转换覆盖所有上游。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 101 | POST /v1/responses Passthrough — 新建 handler + 路由注册 + ClientProtocol::Responses + Usage 双 fallback + 流式透传。TDD: 6 UT + 6 BDD | 后端+测试 | 8h | ⏳ 待开始 |
| Stage 102 | Responses→Chat 协议桥接 — ResponsesToChatCompletions 适配器（MessageAdapter + StreamAdapter）+ 非流式字段映射 + 流式 SSE 事件映射 + handler 集成。TDD: 19 适配器 UT + 6 BDD | 后端+测试 | 14h | ⏳ 待开始 |

**依赖**: Stage 101 → 102（101 落地端点骨架 + ClientProtocol::Responses，102 在此基础上加适配器转换，独立测试验收）。

**关键决策**:
- **先 Passthrough 后 Bridge，分开验收**：两个 Stage 独立可测，101 验证端点→认证→SpendLog 链路正确，102 验证协议转换正确。
- **Stage 101 复用 OpenAIPassthrough**：适配器层零改动（只换 model + 注 stream_options），handler 层处理校验字段（`input`/`messages`）、上游路径（`responses`/`chat/completions`）、Usage 解析（`input_tokens`/`prompt_tokens` fallback）。
- **Stage 102 新增独立适配器**：`ResponsesToChatCompletions` 实现完整的请求/响应/流式转换。
- **显式丢弃字段**：`reasoning`、`previous_response_id`/`conversation`、非 function 工具（400 拒绝）。

**设计文档**: `docs/stages/stage-101.md` + `docs/research/2026-08-04-openai-responses-api-support.md`

---

## 项目里程碑

```
Phase 0-4:  ████████████████████ 100% (6/6)  ✅
Phase 5:    ████████████████████ 100% (6/6)  ✅
Phase 7:    ████████████████████ 100% (5/5)  ✅
Phase 8:    ████████████████████ 100% (3/3)  ✅
Phase 9:    ████████████████████ 100% (4/4)  ✅
Phase 11:   ████████████████████ 100% (6/6)  ✅
Phase 12:   ████████████████████ 100% (3/3)  ✅
Phase 13:   ████████████████████ 100% (6/6)  ✅
Phase 14:   ████████████████████ 100% (4/4)  ✅
Phase 15:   ████████████████████ 100% (3/3)  ✅
Phase 16:   ████████████████████ 100% (3/3)  ✅
Phase 17:   ████████████████████ 100% (3/3)  ✅
Phase 18:   ████████████████████ 100% (2/2)  ✅
Phase 19:   ████████████████████ 100% (2/2)  ✅
Phase 20:   ████████████████████ 100% (2/2)  ✅
Phase 21:   ████████████████████ 100% (2/2)  ✅
Phase 22:   ████████████████████ 100% (2/2)  ✅
Phase 23:   ████████████████████ 100% (2/2)  ✅
Phase 24:   ████████████████████ 100% (1/1)  ✅
Phase 25:   ████████████████████ 100% (1/1)  ✅
Phase 26:   ████████████████████ 100% (3/3)  ✅
Phase 27:   ████████████████████ 100% (3/3)  ✅ 全栈质量修复 + Usage 图表增强
Phase 28:   ████████████████████ 100% (1/1)  ✅ 安全与质量加固
Phase 29:   ████████████████████ 100% (4/4)  ✅ Cross-DB BDD Hardening
Phase 30:   ░░░░░░░░░░░░░░░░░░░░   0% (0/4)  ⚠️ 代码已落地，Phase 31 修复完成待一并标记 ✅
Phase 31:   ████████████████████ 100% (3/3)  ✅ Stage 82-84 全部完成
Phase 32:   ████████████████████ 100% (1/1)  ✅ request_id → call_id 改名 + 上游对账链路（Stage 85）
Phase 33:   ████████████████████ 100% (1/1)  ✅ aigw↔aigw 多表只读增量同步（Stage 86）
Phase 34:   ████████████████████ 100% (1/1)  ✅ 售后对账链路收尾（Stage 87）
Phase 35:   ████████████████████ 100% (2/2)  ✅ Core Entity Soft-Delete (Stage 88-89)
Phase 36:   ████████████████████ 100% (1/1)  ✅ Upstream Cache Detection & Billing (Stage 90)
Phase 38:   ████████████████████ 100% (3/3)  ✅ UI 多语言 i18n 支持 (Stage 91-93)
Phase 39:   ████████████████████ 100% (4/4)  ✅ Budget Reset 周期任务 + 配置 (Stage 94-97)
Phase 40:   ████████████████████ 100% (3/3)  ✅ BDD Coverage Enhancement (Stage 98-100)
Phase 41:   ░░░░░░░░░░░░░░░░░░░░   0% (0/2)  ⏳ OpenAI Responses API 接入 (Stage 101-102)
Phase 42:   ████████████████████ 100% (3/3)  ✅ Playground 多模态图片 (Stage 103-105)
```

---

## Phase 42: Playground 多模态图片 ⏳（2026-08-07，34.5h）

**背景**: 用户要给 Playground 增加图片能力，让 qwen3.5-vl 等多模态模型在 playground 中识别图片。代码审计确认后端多模态转换部分就绪（`claude_message_to_openai` 正确生成 `data:{media_type};base64,{data}`），但 `openai_message_to_claude` 反向有 bug（硬编码 image/jpeg + 完整 data URL 塞入 data 字段），前端 Playground 仅纯文本。三路 subagent 并发实测收敛为 3 Stage。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 103 | 多模态适配修复（`openai_message_to_claude` data URL 解析 + `/v1/models` 暴露 model_info.mode）+ 6 BDD。TDD: 8 UT + 6 BDD | 后端+测试 | 6.5h | ✅ 完成（cd576dc） |
| Stage 104 | Playground 图片输入（上传/粘贴/预览 + 双端点多模态序列化 + 独立 sessionStorage 持久化 + RASTER_MIME 守卫）+ 新增 /v1/messages mock + 请求体捕获。TDD: 8 E2E × 3 viewports = 24 执行 | 前端 | 16h | ✅ 完成 |
| Stage 105 | 图片渲染（Playground 气泡 + log-viewer extractImages/ImageThumbnails + OutputCard Responses output[] 分支）+ SpendLog 详情 3 UT + 文档收尾。TDD: 3 UT + 5 E2E × 3 viewports = 15 执行 | 全栈+文档 | 12h | ✅ 完成 |

**依赖**: Stage 103 → 104（发送图片依赖反向转换正确 + 模式字段）；Stage 104 → 105（渲染依赖图片数据模型就绪）。

**设计文档**: `docs/stages/stage-103.md` / `stage-104.md` / `stage-105.md`

---

## 测试目标

| 层 | 当前 |
|---|------|
| 后端单元 | ≥ 290 tests（含 Stage 99 daily_spend_queue 7 + auth_gateway 4 + rate_limit 5） |
| mock BDD | ≥ 191 scenarios（含 Stage 98 13 new） |
| real BDD | ≥ 47 SQLite / ≥ 41 PG / ≥ 41 MySQL（含 Stage 100 11 new） |

---

## 优先级排序

| 优先级 | 目标 | 状态 |
|--------|------|------|
| ✅ | Phase 40 BDD Coverage Enhancement | ✅ 完成（2026-08-03） |
| P1 | Phase 41 Stage 101 POST /v1/responses Passthrough | ⏳ 待开始 |
| P1 | Phase 41 Stage 102 Responses→Chat 协议桥接 | ⏳ 待开始 |
| ✅ | Phase 42 Stage 103 多模态适配修复 + 模型模式暴露 | ✅ 完成（cd576dc） |
| ✅ | Phase 42 Stage 104 Playground 图片输入 | ✅ 完成 |
| ✅ | Phase 42 Stage 105 图片渲染 + SpendLog 详情 + 文档 | ✅ 完成 |
| P2 | Phase 30 backfill 标记 | 待处理 |
| P2 | TD-006 客户端 call_id 响应头回写 | 待处理 |
| P2 | TD-007 soft_budget 告警通道 | 待处理 |
| P2 | TD-009a/b/e 图片压缩 + 超大图 body 防御 + 外链渲染 | 待处理（视使用量） |
