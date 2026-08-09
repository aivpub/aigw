# aigw -- 下一步行动

**上次更新**: 2026-08-09
**当前阶段**: **Phase 44 ✅ + 在途 P1 收尾 ✅ 全部完成（116/116 Stages + TD-004/006/007 Resolved）；Phase 45 ⏳ 规划（Stage 113-115 技术债清理，28h）**

---

## 当前状态：116/116 Stages + 在途 P1 收尾全完成（剩余为技术债）

**2026-08-09（四期）**: **Phase 44 全部完成（Stage 110-112 ✅）——OpenAI Embeddings API 代理。** Stage 110（`41d0223`）后端四端点：新建 `routes/embeddings.rs`（responses.rs 非流式子集：ChatAuth→校验 model+input→resolver+router→硬选 OpenAIPassthrough→上游 `/embeddings`→透传→SpendLog `call_type="embedding"` + prompt-only 计费）+ openapi `embeddings_spec()`（endpoints 18→19）+ mock `/v1/embeddings` handler + 6 UT + 11 BDD。**实现修正**：axum `Path<Option<String>>` 在无参数路由会 500 → 拆 `embeddings_handler`（无 Path wrapper）+ `embeddings_handler_with_path`（Azure 别名 Path<String>），共享 `embeddings_handler_inner`。Stage 111（`4637062`）前端：OutputCard `data[]` 分支（向量维度 + 8 维截断预览 + usage grid，替代空态）+ i18n 2 keys + 2 E2E × 3 viewports（fe-bdd 333 pass）。Stage 112 模型接入 + 文档：models.feature +2 BDD（`/model/new` 注册 mode=embed + `/v1/models` 展示 + `/v1/embeddings` SpendLog call_type=embedding）+ charter（L91/L204 已有 `/v1/embeddings`）+ roadmap/next-steps/ADR-026/TD-012 收尾。验证：aigw-server 135 UT、mock BDD 232 pass（2 新增全绿，仅 pre-existing budget_reset next_tick flake）、fe-bdd 333 pass、fmt + lint green。

**2026-08-09（在途 P1 收尾 + 技术债核实）**: **Responses 稳定 + TD-006/TD-007 全部完成；TD-004 核实已修复。** ① Responses 适配器级 UT：`ResponsesToChatCompletions` + `ResponsesToChatCompletionsStream` 直测 7 个（`caae61f`），并修复 stream tool-call 参数增量块（`id/name` null）被丢弃的潜在 bug；② 流式 SSE 接线：responses.rs 流式路径接 `adapter.stream_adapter()` 把上游 Chat SSE 转 Responses SSE 事件（response.created / output_text.delta / function_call_arguments.delta / response.completed / [DONE]），mock 上游返回真实 Chat SSE 帧，BDD 断言含三事件（`361b99d`）；③ TD-006：`x-call-id` 响应头回写（== SpendLog.call_id）——`aigw_core::request_id::UuidV7RequestId` + main.rs `PropagateRequestIdLayer(x-call-id)` 覆盖全路由 + BDD 场景（`b485f30`，头名改 `x-call-id` `6b6822c`）；④ TD-007：soft_budget 超限 → `general_settings.alert_webhook` 可选 webhook 通知（fire-and-forget）+ config 字段 + 3 alerts UT + 2 config UT（`6e7a58c`）；⑤ TD-004 核实已修复（`b199000` before/after hook）并补标 Resolved（`cb84341`）。验证：workspace 796 UT、mock BDD 233 pass（新增 SSE-events + x-call-id 场景全绿，仅 pre-existing budget_reset flake）。

**待办**（Phase 45 技术债清理 Stage 113-115，规划文档 `docs/plans/2026-08-09-tech-debt-cleanup.md` + `stage-113~115.md`）:
1. **Phase 45 Stage 113**（P1, 8h）：后端可靠性加固 — TD-005 Async Engine 容错 + CancellationToken 优雅关闭；TD-010a health embedding 探测；TD-003 BDD 覆盖率脚本
2. **Phase 45 Stage 114**（P1, 10h）：前端体验 — TD-009a/b 图片压缩 + body 防御；TD-008a/b i18n 懒加载 + TS 类型
3. **Phase 45 Stage 115**（P1, 10h）：多模态精度 — TD-011b HEIC/AVIF 前端转码（方案变更）；TD-011c Anthropic downsizing；TD-012b 多模态 embedding 按模态计费；TD-011a 视频输入（可选）
4. TD-008c/d 后端错误多语言 + RTL、TD-009e 外链缩略图、TD-011a 视频 token 估算（剩余部分）→ 视使用量触发
5. 长期路线 LT-BodyMetrics/LT-BodyCompact/LT-BodyLifecycle 视数据量触发

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

## Phase 41: OpenAI Responses API 接入 ✅（22h，2 Stages，2026-08-05 完成）

**背景**: OpenAI 于 2025 年推出 Responses API（`POST /v1/responses`）。`/v1/responses` 上游生态极窄（仅 OpenAI + litellm），绝大多数 provider 只支持 `/v1/chat/completions`。分两阶段渐进交付：101 先做 Passthrough 让端点可用，102 加 Responses→Chat 协议转换覆盖所有上游。

| Stage | 目标 | 类型 | 预估 | 状态 |
|-------|------|------|------|------|
| Stage 101 | POST /v1/responses Passthrough — 新建 handler + 路由注册 + ClientProtocol::Responses + Usage 双 fallback + 流式透传。TDD: 6 UT + 6 BDD | 后端+测试 | 8h | ✅ 完成（b90f42d） |
| Stage 102 | Responses→Chat 协议桥接 — ResponsesToChatCompletions 适配器（MessageAdapter + StreamAdapter）+ 非流式字段映射 + 流式 SSE 事件映射 + handler 集成。TDD: 5 BDD（适配器级 UT 未单独拆分） | 后端+测试 | 14h | ✅ 完成（6a3ab61） |

**依赖**: Stage 101 → 102（101 落地端点骨架 + ClientProtocol::Responses，102 在此基础上加适配器转换，独立测试验收）。

**关键决策**:
- **先 Passthrough 后 Bridge，分开验收**：两个 Stage 独立可测，101 验证端点→认证→SpendLog 链路正确，102 验证协议转换正确。
- **⚠️ 实现修正**：Stage 101 实际 `select_adapter(ClientProtocol::Responses, ...)` 直接接线 `ResponsesToChatCompletions`（非计划初稿的 `OpenAIPassthrough`）；流式路径保持原始 SSE 透传。
- **显式丢弃字段**：`reasoning`、`previous_response_id`/`conversation`、非 function 工具（400 拒绝）。

**测试缺口（已记录）**: ① 计划声称的 19 适配器 UT 未落地，桥接逻辑由 5 个 BDD 场景覆盖；② `ResponsesToChatCompletionsStream` 未接入 handler 流式路径，流式 SSE 事件转换未被执行覆盖。

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
Phase 30:   ████████████████████ 100% (4/4)  ✅ Body Archive 冷存储（Stage 78-81 生产化后回写）
Phase 31:   ████████████████████ 100% (3/3)  ✅ Stage 82-84 全部完成
Phase 32:   ████████████████████ 100% (1/1)  ✅ request_id → call_id 改名 + 上游对账链路（Stage 85）
Phase 33:   ████████████████████ 100% (1/1)  ✅ aigw↔aigw 多表只读增量同步（Stage 86）
Phase 34:   ████████████████████ 100% (1/1)  ✅ 售后对账链路收尾（Stage 87）
Phase 35:   ████████████████████ 100% (2/2)  ✅ Core Entity Soft-Delete (Stage 88-89)
Phase 36:   ████████████████████ 100% (1/1)  ✅ Upstream Cache Detection & Billing (Stage 90)
Phase 38:   ████████████████████ 100% (3/3)  ✅ UI 多语言 i18n 支持 (Stage 91-93)
Phase 39:   ████████████████████ 100% (4/4)  ✅ Budget Reset 周期任务 + 配置 (Stage 94-97)
Phase 40:   ████████████████████ 100% (3/3)  ✅ BDD Coverage Enhancement (Stage 98-100)
Phase 41:   ████████████████████ 100% (2/2)  ✅ OpenAI Responses API 接入 (Stage 101-102)
Phase 42:   ████████████████████ 100% (3/3)  ✅ Playground 多模态图片 (Stage 103-105)
Phase 43:   ████████████████████ 100% (3/3)  ✅ Image Token Usage Tracking (Stage 106-108)
Phase 44:   ████████████████████ 100% (3/3)  ✅ OpenAI Embeddings API 代理 (Stage 110-112)
Phase 45:   ░░░░░░░░░░░░░░░░░░░░   0% (0/3)  ⏳ 技术债清理 (Stage 113-115)
```

---

## Phase 42: Playground 多模态图片 ✅（2026-08-07，34.5h）

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
| 后端单元 | ≥ 300 tests（aigw-server 135 含 embeddings 6 UT + openapi 8；aigw-core 395 等全 workspace ~791） |
| mock BDD | ≥ 232 scenarios（含 Stage 110 11 + Stage 112 2；16 @skip 未计入） |
| real BDD | ≥ 47 SQLite / ≥ 41 PG / ≥ 41 MySQL（含 Stage 100 11 new） |

---

## 优先级排序

| 优先级 | 目标 | 状态 |
|--------|------|------|
| ✅ | Phase 40 BDD Coverage Enhancement | ✅ 完成（2026-08-03） |
| ✅ | Phase 41 Stage 101 POST /v1/responses Passthrough | ✅ 完成（b90f42d） |
| ✅ | Phase 41 Stage 102 Responses→Chat 协议桥接 | ✅ 完成（6a3ab61） |
| ✅ | Phase 42 Stage 103 多模态适配修复 + 模型模式暴露 | ✅ 完成（cd576dc） |
| ✅ | Phase 42 Stage 104 Playground 图片输入 | ✅ 完成 |
| ✅ | Phase 42 Stage 105 图片渲染 + SpendLog 详情 + 文档 | ✅ 完成 |
| ✅ | Phase 39 补充 Stage 109 预算重置 cron 界面重构 | ✅ 完成（2026-08-08） |
| ✅ | Phase 43 Stage 106-108 Image Token Usage Tracking | ✅ 完成（2026-08-08） |
| ✅ | Phase 44 Stage 110-112 OpenAI Embeddings API | ✅ 完成（2026-08-09，116/116 ALL COMPLETE） |
| ✅ | 在途 P1 收尾：Responses 稳定（适配器 UT + 流式 SSE）+ TD-006 x-call-id + TD-007 webhook | ✅ 完成（2026-08-09） |
| ✅ | TD-004 BDD @real_api 键泄漏 | ✅ 已修复（b199000，2026-07-20） |
| P1 | Phase 45 Stage 113-115 技术债清理（TD-005/009/010a/011/012b/008a-b/003） | ⏳ 待开始（规划完成，Stage 113 先做） |
| P2 | TD-008c/d 后端错误多语言 + RTL、TD-009e 外链缩略图、TD-011a 视频 token 估算（剩余） | 待处理（视使用量） |
| P2 | Phase 41 测试缺口（适配器 UT + 流式接线） | ✅ 关闭（2026-08-09） |
