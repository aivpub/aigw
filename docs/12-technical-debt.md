# aigw -- Technical Debt Ledger

## Active Items

### TD-004: BDD @real_api tests leak virtual keys in upstream DB

- **Date**: 2026-07-20
- **Priority**: P2
- **Description**: Real API BDD tests (`@real_api` scenarios) create virtual keys
  via `POST /key/generate` against the upstream litellm PostgreSQL database
  (`real_api_steps.rs:create_key_via_api()`). These keys never get cleaned up —
  there is no `after_scenario` hook or `DELETE /key/delete` call.
  217 stale keys accumulated before first manual cleanup (`hack/backup-bdd-test-keys.sql`).
- **Impact**: upstream `LiteLLM_VerificationToken` table grows unboundedly on each
  test run. After ~30 runs the table had 217 test keys.
- **Resolution**: Add `after_scenario` hook in `bdd.rs` (or a new step module) that
  iterates `TestWorld.created_keys` and calls `DELETE /key/delete` for each key
  created during the scenario. Must guard on `AIGW_REAL_API=1` and handle cleanup
  gracefully (key may have been deleted mid-test).

### TD-003: BDD coverage reporting not automated

- **Date**: 2026-07-04
- **Priority**: P3
- **Description**: No automated BDD endpoint coverage report. Stage 12 acceptance criteria
  includes "BDD 覆盖率报告生成（端点覆盖率 ≥ 90%）" but this requires a coverage mapping
  tool that links .feature scenarios to API routes.
- **Impact**: Cannot quantitatively verify endpoint coverage.
- **Resolution**: Implement a simple script/tool that maps scenarios to endpoints and
  generates a coverage report.

### TD-005: Async Engine 无 panic 容错 + 无 shutdown 信号

- **Date**: 2026-07-25
- **Priority**: P2
- **Source**: `docs/research/2026-07-25-body-archive-production-audit.md`（P2-2 / P2-3）
- **Description**: `crates/aigw-core/src/engine.rs` 的 `tokio::spawn`（L75, L96, L107）内无 `catch_unwind`，tick/exec/cleanup loop 任何 panic 会让该 loop 永久死掉，其他 loop 不受影响但该 task 能力静默下降。`Engine::run`（L62-117）无 shutdown channel，`for h in handles { h.await }` 永远等待，SIGTERM 时正在执行的 step 被 cancel 卡 running，需等 cleanup_loop 下次回收（默认 30s 检查 + 10min 超时）。
- **Impact**: (1) 长期运行后 exec loop 数静默减少，归档吞吐下降不可观测；(2) 滚动部署时 step 卡 running 最长 10min。
- **Resolution**: 每个 loop 体用 `std::panic::AssertUnwindSafe + catch_unwind` 包裹，panic 时 log + sleep 30s + 继续；`Engine::run` 接收 `CancellationToken`，loop 内 `select!` 监听 shutdown，优雅退出前等待 in-flight step。
- **Target Phase**: Phase 32 候选（Phase 31 修复 P0/P1 后处理）。

### TD-006: 客户端无法从响应头获取 call_id 对账

- **Date**: 2026-07-27
- **Priority**: P2
- **Source**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` §10
- **Description**: aigw 未配置 `tower_http::PropagateRequestIdLayer`，调用方无法从响应头拿到 aigw 生成的调用 ID。Stage 85 完成后 DB 有 `call_id`（可前端/日志查），但客户端若想就地用调用 ID 对账需自行从响应 body 取，响应头无回写。
- **Impact**: 客户端对账需多一跳（查 API/前端），无法响应头直取。不阻塞 Stage 85 核心预期（DB 侧对账链路已打通）。
- **Resolution**: 后续加 `PropagateRequestIdLayer` 或自定义响应头 `x-gw-call-id` 回写客户端。需评估是否暴露内部 ID 给客户端的安全影响。
- **Target Phase**: 视客户端对账需求触发，暂不排期。

### TD-007: soft_budget 告警通道未实现

- **Date**: 2026-07-30
- **Priority**: P2
- **Source**: Phase 37/39 规划
- **Status Update 2026-08-04**: Stage 97 实现了 soft_budget 超限 `tracing::warn!` 结构化日志（含 entity_type、entity_id、spent、soft_budget 字段）。生产环境可通过 tracing subscriber（OpenTelemetry exporter、log aggregator）消费这些事件。外部通知通道（Slack/Email/Webhook）仍待接入。
- **Description**: soft_budget 超限检查记 tracing warn 日志但放行请求。litellm 的完整 soft_budget 告警走 Slack/Email/Webhook 外部通知通道，aigw 当前无外部通知集成。
- **Impact**: soft_budget 超限需管理员主动查日志才能发现，无法实时告警。不阻塞核心预期（hard budget check + 周期 reset 已完整）。
- **Resolution**: 后续接入通知通道（webhook / 邮件 / 企微），在 BudgetEnforcer soft 超限分支触发。需评估告警去重（soft_budget_cooldown）与通知模板。
- **Target Phase**: 视运维告警需求触发，暂不排期。

### TD-008: i18n 后续改进项

- **Date**: 2026-08-01
- **Priority**: P3

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-008a | 翻译文件懒加载 | P3 | 当前所有翻译 bundle 在一个 JS chunk 中。当翻译条目增长到 1000+ 时，按命名空间动态 `import()` 可减首屏体积。 |
| TD-008b | TypeScript 类型安全 | P3 | 从 JSON 翻译文件自动生成翻译 key 的 TS 类型（如 `i18next-resources-for-ts`），让 `t('key')` 有 IDE 自动补全和编译期校验。 |
| TD-008c | 后端 API 错误消息多语言 | P3 | 当前前端 UI 已双语，但后端 API 返回的英文错误消息仍显示英文。需设计后端 i18n 策略（Accept-Language header 或配置）。 |
| TD-008d | RTL 语言支持 | P3 | 当前仅支持 LTR 语言（中/英）。如需支持阿拉伯语等 RTL 语言，需配合 Tailwind RTL 变体 + CSS logical properties。 |

- **Impact**: 当前双语支持已覆盖 100% 前端 UI 文本，TD-008 a-d 均为增量优化，不阻塞使用。
- **Resolution**: 按需在各子条目触发条件满足时实施。
- **Target Phase**: 无固定排期。

### TD-009: 多模态图片 base64 体积与渲染增强项

- **Date**: 2026-08-07
- **Priority**: P2
- **Source**: Phase 42（Playground 多模态图片）规划
- **Description**: Playground 图片以 base64 data URL 直传网关（前端读 FileReader，不走子图/压缩）。超大图（>32 MiB body limit）或大量图片会撑爆请求体；SpendLog 详情渲染的 base64 缩略图无体积上限控制；无点击放大/灯箱。

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-009a | 图片压缩/缩放 | P2 | 前端上传前用 canvas 压缩（如最长边 2048px + JPEG 0.8），降低 base64 体积与 token 成本；需评估与"原图保真"诉求的取舍。 |
| TD-009b | 超大图/多图 body limit 防御 | P2 | 上传前估算 `∑ data URL 长度`，超限（如 >24 MiB）前端提示并拒绝；后端 `request_body_limit_mb` 已默认 32 MiB 但无 413 友好提示。 |
| TD-009c | 图片点击放大/灯箱 | P3 | Playground 缩略图点击放大已在 2026-08-07 `1394a9c` 实现（Dialog lightbox）；SpendLog 详情缩略图放大仍待做。 |
| TD-009d | `/v1/models` 模式标签 UI | P3 | Playground 模型下拉按 `model_info.mode` 显示多模态/纯文本标签，当前不做（阶段已定不强制过滤）。 |
| TD-009e | 外链 image_url 渲染 | P3 | `extractImages` 只渲染 `data:image/` 前缀；SpendLog 里 `https://` image_url 不渲染（admin-only 详情仍收窄任意 URL fetch 面）。后续如需支持外链缩略图，需代理/白名单域名。 |

### TD-010: Embeddings 后续增强项

- **Date**: 2026-08-08
- **Priority**: P3
- **Source**: Phase 45 规划（ADR-026）

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-010a | health.rs embedding-mode 探测 | P2 | `run_and_save_health_check`（health.rs L266）对所有 OpenAICompatible 模型 POST `{model, messages, max_tokens:1}` 到 `/chat/completions`；embedding-only 模型会 400。需按 `model_info.mode="embed"` 分支为 embeddings-friendly 最小探测（POST `{model, input:["health"]}` 到 `/embeddings`）。用户确认非阻塞，Phase 46 候选。 |
| TD-010b | 多模态 embedding 按模态计费 | P3 | gemini-embedding-2 按模态计费（image $0.45 / audio $6.50 / video $12.00 per 1M，远超 text $0.20）；aigw 单 `input_cost_per_token` 标量无法表达按模态差异计费。等真实多模态 embedding 负载再评估。 |
| TD-010c | Gemini `:embedContent` / Cohere `/v2/embed` 原生格式翻译 | P3 | 薄 OpenAI-compatible Passthrough 只覆盖 `openai/`-前缀；Gemini 原生 `:embedContent`、Cohere `/v2/embed` 是差异化层（Envoy 2026-06 刚合并），等真实 RAG 负载再上。 |
| TD-010d | `/engines/{model}/embeddings` + `/openai/deployments/{model}/embeddings` Azure 别名深度测试 | P3 | 四端点共用同一 handler 已注册；Azure 专属语义（deployment name 映射）留后续。 |

- **Impact**: 大图请求可能 413；详情页缩略图体积大影响加载。不阻塞 Phase 42 核心交付（图片识别已通）。
- **Resolution**: 视生产图片使用量触发；-009a/b 建议随 Phase 42 后的小版本实施，c/d/e 按需。
- **Target Phase**: 无固定排期（视使用量）。

## Resolved Items

### TD-002: @real_api step bindings implemented (Resolved 2026-07-05)

- Implemented `tests/bdd_steps/real_api_steps.rs` with 19 step bindings covering all 9
  @real_api scenarios across `end_to_end_real.feature` and `compatibility_real.feature`.
- All steps guard on `AIGW_REAL_API=1` env var with `set_skip_pass()` helper to set
  placeholder status/body so shared Then steps don't panic when mode is off.
- Unique step names avoid conflicts with mock step bindings (e.g. `通过 API 创建普通 key`
  vs `一个普通 key`).
- 72 scenarios (72 passed), 257 steps (257 passed) including 9 @real_api vacuously passing.

### TD-001: Dead code cleanup (Resolved 2026-07-03)

- Removed unused `ChatCompletionRequest`, `ChatMessage`, `KeyUpdateQuery` stubs.
- Removed redundant `proxy.rs` auth/handler (replaced by `chat.rs` implementation).
- `TenantAuth` extractor available for future SaaS route enforcement.

## Monitoring Items

| Item | Priority | Trigger |
|------|----------|---------|
| TenantAuth wiring | Low | When SaaS deployments need per-route org enforcement |
| Provider registry in AppState | Low | When chat.rs switches from env-var to registry-based routing |
| Rate limiter in AppState | Low | When TPM/RPM enforcement is enabled on chat endpoints |
