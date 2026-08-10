# aigw -- 下一步行动

**上次更新**: 2026-08-10
**当前阶段**: **Phase 47 ✅ 完成（Stage 117-119，A 类接线 + exact-match 缓存，40h）**；全部交付（117-119 ✅）

---

## 当前状态：Phase 47 全部完成（Stage 117/118/119 ✅）+ BDD 基线锁定

**2026-08-10（Phase 47 交付 ✅）**: 差距调研（`docs/research/2026-08-09-aigw-gap-vs-industry-leaders.md`）确认的 **A 类「代码在但运行时未接线」** 全部接线 + **B 类「缓存=0」** 补齐，三 Stage 交付：

| Stage | Commit | 交付 |
|-------|--------|------|
| **117** | `d1000b0` | 4 handler 入口挂 `check_request_limits`（多级预算 key→user→team→org + RPM/TPM + soft_budget webhook + max_parallel Semaphore）；`LimitError::IntoResponse` 带 `x-ratelimit-limit/remaining` + `Retry-After`；`real/multi_level_budget` 去 @skip 4 场景 |
| **118** | `abad4db` | Router 智能路由真实决策：weighted（weight 加权随机）+ usage（max remaining rpm）+ latency（min EWMA）+ cooldown 分类计数（429/401/408/404/5xx，400 业务错不计）+ priority fallback 顺序 + key>team>global `merge_router_overrides` 接入请求路径 |
| **119** | `ad981b2` | exact-match 响应缓存：`aigw_core::cache`（moka LRU + 手动 TTL + SHA-256 key + canonical body）+ `X-Cache-Status: HIT/MISS` + cache-hit 计费 0 元（`cached=1`）+ no-store 绕过 + `CacheControl`（use-cache/ttl） |

**基线（Phase 47 收尾）**: aigw-core 432 + aigw-server 145+152 UT（**合计 861**）、mock BDD **246 场景（233 pass / 13 @skip body_archive / 0 fail）**、real BDD sqlite/pg/mysql **47/47 × 3**、fmt + clippy `-D warnings` green。ADR-032 Accepted（Stage 117-119 落地）。roadmap v55.0。

**顺带修复**: ① BDD chat 步骤缺 request-id layer（所有 mock chat 请求 call_id="unknown" 撞 spend_logs.call_id UNIQUE，缓存写第二条时暴露）→ e2e + cache 步骤补 `SetRequestIdLayer`（UUID-v7）；② alerts.rs 测试 flake（`dispatch_soft_budget_alert` 的 `tokio::spawn` 在同步 `#[test]` 无 reactor panic）→ 改 `#[tokio::test]`。

**收尾（✅ 全部完成）**: ① 前端 RouterSettings 下拉解锁 usage/latency（`9fe6329`，weight/rpm/tpm 输入留后续模型页）；② config `cache` 块解析 + boot 注入（`9fe6329`）；③ max_parallel 从 key/budget 表字段层级接线（`cada57b`，key→team→org-budget→deployment）。

**后续候选**（中期 3-6 月，视人力）:
4. **M1** guardrails 最小集（regex 提示注入 + PII 脱敏 + Moderation hook，P0，4-5 pw）
5. **M2** Redis 分布式层（跨实例限流/预算 counter/共享缓存，P0，4-5 pw）
6. **M3** MCP 最小透传 + WebSocket（P1，3-4 pw）
7. **M4** 审计日志 + RBAC 矩阵（P1，5-7 pw）
8. **M5** `gen_ai.*` 可观测语义 + 指标扩展（P2，2-3 pw）

**按使用量触发**: litellm_settings 接线、config 热重载、TD-008c/d、TD-009e、TD-011a 视频 token 估算、长期路线 LT-*

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
Phase 45:   ████████████████████ 100% (3/3)  ✅ 技术债清理 (Stage 113-115 全部完成)
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
| 后端单元 | ≥ 300 tests（aigw-server 145+152 含 embeddings 6 UT + openapi 8 + health probe 1；aigw-core 432 等全 workspace ~861） |
| mock BDD | ≥ 246 scenarios（233 pass / 13 @skip body_archive，Phase 47 收尾基线；含 rate_limit/soft_budget/router/cache 新场景） |
| 前端 BDD | ≥ 342 passed（Stage 114 全量回归；含 3 压缩场景 × 3 viewports + i18n-switcher 9） |
| real BDD | ≥ 47 SQLite / ≥ 47 PG / ≥ 47 MySQL（Phase 47 三后端全绿） |

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
| ✅ | Phase 45 Stage 113 后端可靠性加固（TD-005 + TD-010a + TD-003） | ✅ 完成（2026-08-09） |
| ✅ | Phase 45 Stage 114 前端体验（TD-009a/b 图片压缩 + TD-008a/b i18n 懒加载） | ✅ 完成（2026-08-09） |
| ✅ | Phase 45 Stage 115 多模态精度（TD-011b/c + TD-012b + TD-011a 可选） | ✅ 完成（2026-08-09，TD-011a 视频 SKIPPED） |
| ✅ | Phase 46 Stage 116 静态配置模型接入（config.yaml model_list/router/env + key gates） | ✅ 完成（2026-08-10，ADR-031 Accepted） |
| ✅ | BDD 漏洞审计（4 静默跳过洞 + 流式 x-call-id 头） | ✅ 完成（2026-08-10，484ea70，mock BDD 254 基线） |
| ✅ | Phase 47 Stage 117 A 类接线核心（限流 + 多级预算 + soft_budget 告警 + max_parallel） | ✅ 完成（2026-08-10，d1000b0） |
| ✅ | Phase 47 Stage 118 Router 智能路由接线（cooldown/weighted/usage/latency/fallback） | ✅ 完成（2026-08-10，abad4db） |
| ✅ | Phase 47 Stage 119 exact-match 响应缓存（moka LRU + X-Cache-Status + 计费 0 元） | ✅ 完成（2026-08-10，ad981b2） |
| ✅ | Phase 47 收尾：前端 RouterSettings 下拉解锁 + config cache 块 + max_parallel key/budget 表字段 | ✅ 完成（2026-08-10，9fe6329 / cada57b） |
| P2 | TD-008c/d 后端错误多语言 + RTL、TD-009e 外链缩略图、TD-011a 视频 token 估算（剩余） | 待处理（视使用量） |
| P2 | Phase 41 测试缺口（适配器 UT + 流式接线） | ✅ 关闭（2026-08-09） |
