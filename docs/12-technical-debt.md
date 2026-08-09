# aigw -- Technical Debt Ledger

## Active Items

### TD-004: BDD @real_api tests leak virtual keys in upstream DB ✅ Resolved 2026-07-20

- **Date**: 2026-07-20
- **Priority**: P2
- **Description**: Real API BDD tests (`@real_api` scenarios) create virtual keys
  via `POST /key/generate` against the upstream litellm PostgreSQL database
  (`real_api_steps.rs:create_key_via_api()`). These keys never get cleaned up —
  there is no `after_scenario` hook or `DELETE /key/delete` call.
  217 stale keys accumulated before first manual cleanup (`hack/backup-bdd-test-keys.sql`).
- **Impact**: upstream `LiteLLM_VerificationToken` table grows unboundedly on each
  test run. After ~30 runs the table had 217 test keys.
- **Resolution**: ✅ 已实现（commit `b199000`，2026-07-20）——`bdd.rs` 挂 `.before()`/`.after()` hooks（`real_api_steps.rs` `before_scenario_hook` / `after_scenario_hook`），每个 @real_api 场景结束后遍历 `TestWorld.created_keys` 调用 `DELETE /key/delete`（best-effort，guard `AIGW_REAL_API=1`，master key 跳过）；before hook 同时预清理上次崩溃残留的已知 alias keys。代码现状确认：`real_api_steps.rs` L72-98 after hook 在途。

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

### TD-006: 客户端无法从响应头获取 call_id 对账 ✅ Resolved 2026-08-09

- **Date**: 2026-07-27
- **Priority**: P2
- **Source**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` §10
- **Description**: aigw 未配置 `tower_http::PropagateRequestIdLayer`，调用方无法从响应头拿到 aigw 生成的调用 ID。Stage 85 完成后 DB 有 `call_id`（可前端/日志查），但客户端若想就地用调用 ID 对账需自行从响应 body 取，响应头无回写。
- **Impact**: 客户端对账需多一跳（查 API/前端），无法响应头直取。不阻塞 Stage 85 核心预期（DB 侧对账链路已打通）。
- **Resolution**: ✅ 已实现（commit `b485f30` + `6b6822c`）——main.rs 挂 `PropagateRequestIdLayer(x-call-id)`，所有路由（chat/messages/responses/embeddings）响应头回写网关调用 ID（== `spend_logs.call_id`）；responses.rs 非流式成功响应额外直接写 `x-call-id`；`aigw_core::request_id::UuidV7RequestId` 供 server + 测试共享。BDD 场景 `响应头包含 x-call-id 且匹配 SpendLog call_id` 覆盖。安全评估：暴露的是网关内部 UUID v7（非 secret，仅用于关联查询），已在文档注明。
- **Target Phase**: ✅ 已解决（2026-08-09）。

### TD-007: soft_budget 告警通道未实现 ✅ Resolved 2026-08-09

- **Date**: 2026-07-30
- **Priority**: P2
- **Source**: Phase 37/39 规划
- **Status Update 2026-08-04**: Stage 97 实现了 soft_budget 超限 `tracing::warn!` 结构化日志（含 entity_type、entity_id、spent、soft_budget 字段）。生产环境可通过 tracing subscriber（OpenTelemetry exporter、log aggregator）消费这些事件。外部通知通道（Slack/Email/Webhook）仍待接入。
- **Description**: soft_budget 超限检查记 tracing warn 日志但放行请求。litellm 的完整 soft_budget 告警走 Slack/Email/Webhook 外部通知通道，aigw 当前无外部通知集成。
- **Impact**: soft_budget 超限需管理员主动查日志才能发现，无法实时告警。不阻塞核心预期（hard budget check + 周期 reset 已完整）。
- **Resolution**: ✅ 已实现（commit `6e7a58c`）——新 `aigw_core::alerts` dispatcher：`general_settings.alert_webhook` 可选配置，`check_soft_budget` 超限时 POST JSON 到 webhook（fire-and-forget，3s 超时，失败仅 warn，不阻塞请求路径）；未配置 → 保持 `tracing::warn`。config.rs 加字段 + UT，Cargo.toml default features 补 reqwest。后续增强（邮件/企微模板、告警去重）视运维需求。
- **Target Phase**: ✅ 已解决（2026-08-09）。

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
- **Source**: Phase 44 规划（ADR-026）

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-010a | health.rs embedding-mode 探测 | P2 | `run_and_save_health_check`（health.rs L266）对所有 OpenAICompatible 模型 POST `{model, messages, max_tokens:1}` 到 `/chat/completions`；embedding-only 模型会 400。需按 `model_info.mode="embed"` 分支为 embeddings-friendly 最小探测（POST `{model, input:["health"]}` 到 `/embeddings`）。用户确认非阻塞，Phase 46 候选。 |
| TD-010b | 多模态 embedding 按模态计费 | P3 | gemini-embedding-2 按模态计费（image $0.45 / audio $6.50 / video $12.00 per 1M，远超 text $0.20）；aigw 单 `input_cost_per_token` 标量无法表达按模态差异计费。等真实多模态 embedding 负载再评估。 |
| TD-010c | Gemini `:embedContent` / Cohere `/v2/embed` 原生格式翻译 | P3 | 薄 OpenAI-compatible Passthrough 只覆盖 `openai/`-前缀；Gemini 原生 `:embedContent`、Cohere `/v2/embed` 是差异化层（Envoy 2026-06 刚合并），等真实 RAG 负载再上。 |
| TD-010d | `/engines/{model}/embeddings` + `/openai/deployments/{model}/embeddings` Azure 别名深度测试 | P3 | 四端点共用同一 handler 已注册；Azure 专属语义（deployment name 映射）留后续。 |

- **Impact**: 大图请求可能 413；详情页缩略图体积大影响加载。不阻塞 Phase 42 核心交付（图片识别已通）。
- **Resolution**: 视生产图片使用量触发；-009a/b 建议随 Phase 42 后的小版本实施，c/d/e 按需。
- **Target Phase**: 无固定排期（视使用量）。

### TD-011: Image Token 估算精度与格式覆盖

- **Date**: 2026-08-08
- **Priority**: P2
- **Source**: Phase 43（Image Token Usage Tracking）规划（ADR-027）
- **Description**: aigw 客户端 image token 估算（OpenAI tiling / Qwen ViT factor / Anthropic 官方公式）是零依赖 header parser + model-name auto-sniff。部分场景精度/格式未覆盖。

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-011a | 视频 token 估算不支持 | P2 | 视频多模态请求的 token 需 `temporal_patch_size` + mRoPE 公式，超出 image-only 引擎能力。触发：视频多模态请求占比 > 5%。 |
| TD-011b | HEIC/AVIF 格式不支持 | P3 | header parser 仅 PNG/JPEG/WebP/GIF；Apple 生态 HEIC 与 AVIF 无法解析 → 估算落空。触发：Apple 生态用户反馈。 |
| TD-011c | Anthropic downsizing 规则未模拟 | P3 | Anthropic 官方公式 `⌈w/28⌉×⌈h/28⌉` 精确，但模型端还有"超出 max_tokens 自动缩放"规则（target=1568 tokens）未在客户端模拟。触发：用户反馈偏差 > 20%。 |

- **Impact**: 估算值仅供参考（阿里云官方明示），不阻塞分析/对账用途；多模态占比低时影响可忽略。
- **Resolution**: 视使用量按子条目触发实施。
- **Target Phase**: 无固定排期。

### TD-012: health.rs embedding-mode 探测缺失 + 多模态 embedding 计费

- **Date**: 2026-08-09
- **Priority**: P2
- **Source**: Phase 44（OpenAI Embeddings API 代理）规划（ADR-026）
- **Description**: 两个 embedding 相关缺口登记。

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-012a | health.rs embedding-mode 探测缺失 | P2 | 现有 `run_and_save_health_check` 对所有 OpenAICompatible 模型 POST `{model, messages:[...], max_tokens:1}` 到 `/chat/completions`，embedding-only 模型会 400 误报 unhealthy。需按 `model_info.mode="embed"` 分支探测 `/embeddings`。触发：embedding 模型接入 health 探测。Phase 46 候选。 |
| TD-012b | 多模态 embedding 按模态计费不支持 | P3 | gemini-embedding-2 按模态计费（$0.45/$6.50/$12.00），超出 aigw 单 `input_cost_per_token` 标量能力。触发：真实多模态 embedding 流量。 |

- **Impact**: 非阻塞。薄 passthrough 对 embedding 模型健康探测会误报；多模态 embedding 计费按标量 input_cost 欠精确。
- **Resolution**: 视使用量触发。
- **Target Phase**: 无固定排期（health 探测候选 Phase 46）。

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
