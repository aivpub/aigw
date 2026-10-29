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

### TD-003: BDD coverage reporting not automated ✅ Resolved 2026-08-09

- **Date**: 2026-07-04
- **Priority**: P3
- **Description**: No automated BDD endpoint coverage report. Stage 12 acceptance criteria
  includes "BDD 覆盖率报告生成（端点覆盖率 ≥ 90%）" but this requires a coverage mapping
  tool that links .feature scenarios to API routes.
- **Impact**: Cannot quantitatively verify endpoint coverage.
- **Resolution**: ✅ 已实现（Stage 113，2026-08-09）——`scripts/bdd-coverage`（POSIX sh）解析 mock + real `.feature` 的 `发送 (GET|POST|PUT|DELETE|PATCH) /path` 与 `并查询 /path` 步骤，对照内嵌路由表（main.rs 同源）输出覆盖率 + NOT covered 清单；`task bdd-coverage` 门禁。⚠️ **Gate 偏差**：实测 87 路由仅 55 覆盖（63%）——admin-CRUD（org/team/user/budget/credential）、v2/login*、/key/deleted、/spend/model-groups、/system/info 无 mock-BDD step（部分由 lib UT / 前端 E2E 覆盖）。为诚实反映现状，门禁定为 60% **回归基线**（防覆盖率下降），非设计的 ≥90%；预置缺口保留在 NOT covered 清单供后续 Stage 关闭。

### TD-005: Async Engine 无 panic 容错 + 无 shutdown 信号 ✅ Resolved 2026-08-09

- **Date**: 2026-07-25
- **Priority**: P2
- **Source**: `docs/research/2026-07-25-body-archive-production-audit.md`（P2-2 / P2-3）
- **Description**: `crates/aigw-core/src/engine.rs` 的 `tokio::spawn`（L75, L96, L107）内无 `catch_unwind`，tick/exec/cleanup loop 任何 panic 会让该 loop 永久死掉，其他 loop 不受影响但该 task 能力静默下降。`Engine::run`（L62-117）无 shutdown channel，`for h in handles { h.await }` 永远等待，SIGTERM 时正在执行的 step 被 cancel 卡 running，需等 cleanup_loop 下次回收（默认 30s 检查 + 10min 超时）。
- **Impact**: (1) 长期运行后 exec loop 数静默减少，归档吞吐下降不可观测；(2) 滚动部署时 step 卡 running 最长 10min。
- **Resolution**: ✅ 已实现（Stage 113，2026-08-09）——新 `guarded()` helper（`futures::FutureExt::catch_unwind` + `AssertUnwindSafe`）包裹 tick/exec/cleanup 单次迭代，panic 时 `error!` log + sleep 30s + 继续（loop 不死）；`Engine::run_with_cancel(&self, CancellationToken)` 优雅关闭（in-flight step 先完成再退出），`run()` 兼容包装保留；main.rs 在 axum graceful shutdown 后 `engine_token.cancel()`。新增 3 UT。**实现偏差**：用 futures 组合子而非 std `catch_unwind`（std 需要同步 FnOnce）；`tokio-util` 不加 `sync` feature（该 feature 不存在，CancellationToken 在 default features 可用）；exec loop 的取消检查放迭代后（保证 in-flight 完成）。
- **Target Phase**: ✅ 已解决（2026-08-09，Phase 45 Stage 113）。

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

### TD-008: i18n 后续改进项 ✅ TD-008a/b Resolved 2026-08-09

- **Date**: 2026-08-01
- **Priority**: P3

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-008a | 翻译文件懒加载 |3 | 当前所有翻译 bundle 在一个 JS chunk 中。当翻译条目增长到 1000+ 时，按命名空间动态 `import()` 可减首屏体积。 ✅ 已解决（Stage 114，2026-08-09 — en 同步 eager + 检测语言 eager（zh-CN 首访首帧中文）+ 另一语言动态 `import()` 独立 chunk（zh-CN 25kB lazy）；en-US 归一化到 en 防 Unknown dynamic import） |
| TD-008b | TypeScript 类型安全 | P3 | 从 JSON 翻译文件自动生成翻译 key 的 TS 类型（如 `i18next-resources-for-ts`），让 `t('key')` 有 IDE 自动补全和编译期校验。 ✅ 已解决（Stage 114，2026-08-09 — `scripts/fe-i18n-types` 从 en.json 生成 `resources.d.ts` 增广 `i18next.CustomTypeOptions`，`tsc -b` 通过；顺带暴露并修复 5 个缺失 key + 1 个拼写错误） |
| TD-008c | 后端 API 错误消息多语言 | P3 | 当前前端 UI 已双语，但后端 API 返回的英文错误消息仍显示英文。需设计后端 i18n 策略（Accept-Language header 或配置）。 |
| TD-008d | RTL 语言支持 | P3 | 当前仅支持 LTR 语言（中/英）。如需支持阿拉伯语等 RTL 语言，需配合 Tailwind RTL 变体 + CSS logical properties。 |

- **Impact**: 当前双语支持已覆盖 100% 前端 UI 文本，TD-008 a-d 均为增量优化，不阻塞使用。
- **Resolution**: a/b 已在 Stage 114 解决（2026-08-09）；c/d 视需求触发。

**2026-10-29 补记**：`scripts/check-i18n-keys.sh`（Stage 92 引入，校验 en / zh-CN key 集对齐）从落地起就**跑不起来** —— 参数写在 heredoc 结束标记之后（`PYEOF "$EN" "$ZH"`），python 读到该行抛 `SyntaxError`，脚本 exit 1。修复：参数移到 `<<` 那一行（`python3 - "$EN" "$ZH" << 'PYEOF'`），并把该脚本加进 `task fe-lint`（此前全仓无任何调用者，这正是它坏了 50 个 Stage 没人发现的原因）。Stage 142 的 1201 个 key 对齐由它首次真正校验通过。
- **Target Phase**: c/d 无固定排期。

### TD-009: 多模态图片 base64 体积与渲染增强项 ✅ TD-009a/b Resolved 2026-08-09

- **Date**: 2026-08-07
- **Priority**: P2
- **Source**: Phase 42（Playground 多模态图片）规划
- **Description**: Playground 图片以 base64 data URL 直传网关（前端读 FileReader，不走子图/压缩）。超大图（>32 MiB body limit）或大量图片会撑爆请求体；SpendLog 详情渲染的 base64 缩略图无体积上限控制；无点击放大/灯箱。

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-009a | 图片压缩/缩放 | P2 | 前端上传前用 canvas 压缩（如最长边 2048px + JPEG 0.8），降低 base64 体积与 token 成本；需评估与"原图保真"诉求的取舍。 ✅ 已解决（Stage 114，2026-08-09 — `compressImage` 取「原图 vs JPEG 2048px@0.8」较小者，小图保真；E2E 2400x2400 照片压缩后 <2MB） |
| TD-009b | 超大图/多图 body limit 防御 | P2 | 上传前估算 `∑ data URL 长度`，超限（如 >24 MiB）前端提示并拒绝；后端 `request_body_limit_mb` 已默认 32 MiB 但无 413 友好提示。 ✅ 已解决（Stage 114，2026-08-09 — handleSend 预检 `∑ dataUrlBytes > 24MiB` → toast + 拒绝，`window.__AIGW_MAX_IMAGE_BODY__` 测试 override；E2E 验证 toast + 无请求） |
| TD-009c | 图片点击放大/灯箱 | P3 | Playground 缩略图点击放大已在 2026-08-07 `1394a9c` 实现（Dialog lightbox）；SpendLog 详情缩略图放大仍待做。 |
| TD-009d | `/v1/models` 模式标签 UI | P3 | Playground 模型下拉按 `model_info.mode` 显示多模态/纯文本标签，当前不做（阶段已定不强制过滤）。 |
| TD-009e | 外链 image_url 渲染 | P3 | `extractImages` 只渲染 `data:image/` 前缀；SpendLog 里 `https://` image_url 不渲染（admin-only 详情仍收窄任意 URL fetch 面）。后续如需支持外链缩略图，需代理/白名单域名。 |

### TD-010: Embeddings 后续增强项

- **Date**: 2026-08-08
- **Priority**: P3
- **Source**: Phase 44 规划（ADR-026）

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-010a | health.rs embedding-mode 探测 | P2 | `run_and_save_health_check`（health.rs L266）对所有 OpenAICompatible 模型 POST `{model, messages, max_tokens:1}` 到 `/chat/completions`；embedding-only 模型会 400。需按 `model_info.mode="embed"` 分支为 embeddings-friendly 最小探测（POST `{model, input:["health"]}` 到 `/embeddings`）。用户确认非阻塞，Phase 46 候选。 ✅ 已解决（Stage 113，2026-08-09 — 见 TD-012a 同步 Resolved）|
| TD-010b | 多模态 embedding 按模态计费 | P3 | gemini-embedding-2 按模态计费（image $0.45 / audio $6.50 / video $12.00 per 1M，远超 text $0.20）；aigw 单 `input_cost_per_token` 标量无法表达按模态差异计费。等真实多模态 embedding 负载再评估。 |
| TD-010c | Gemini `:embedContent` / Cohere `/v2/embed` 原生格式翻译 | P3 | 薄 OpenAI-compatible Passthrough 只覆盖 `openai/`-前缀；Gemini 原生 `:embedContent`、Cohere `/v2/embed` 是差异化层（Envoy 2026-06 刚合并），等真实 RAG 负载再上。 |
| TD-010d | `/engines/{model}/embeddings` + `/openai/deployments/{model}/embeddings` Azure 别名深度测试 | P3 | 四端点共用同一 handler 已注册；Azure 专属语义（deployment name 映射）留后续。 |

- **Impact**: 大图请求可能 413；详情页缩略图体积大影响加载。不阻塞 Phase 42 核心交付（图片识别已通）。
- **Resolution**: 视生产图片使用量触发；-009a/b 建议随 Phase 42 后的小版本实施，c/d/e 按需。
- **Target Phase**: 无固定排期（视使用量）。

### TD-011: Image Token 估算精度与格式覆盖 ✅ TD-011b/c Resolved 2026-08-09（TD-011a 剩余）

- **Date**: 2026-08-08
- **Priority**: P2
- **Source**: Phase 43（Image Token Usage Tracking）规划（ADR-027）
- **Description**: aigw 客户端 image token 估算（OpenAI tiling / Qwen ViT factor / Anthropic 官方公式）是零依赖 header parser + model-name auto-sniff。部分场景精度/格式未覆盖。

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-011a | 视频 token 估算不支持 | P2 | 视频多模态请求的 token 需 `temporal_patch_size` + mRoPE 公式，超出 image-only 引擎能力。触发：视频多模态请求占比 > 5%。 **剩余**（Stage 115 按设计「可选」跳过 Playground 视频输入；token 估算维持待触发） |
| TD-011b | HEIC/AVIF 格式不支持 | P3 | header parser 仅 PNG/JPEG/WebP/GIF；Apple 生态 HEIC 与 AVIF 无法解析 → 估算落空。触发：Apple 生态用户反馈。 ✅ 已解决（Stage 115，2026-08-09 — 前端转码：compressImage 检测 heic/avif → createImageBitmap（Safari 可解码）→ JPEG；无法解码浏览器 toast 提示；E2E 验证 Chromium reject 路径） |
| TD-011c | Anthropic downsizing 规则未模拟 | P3 | Anthropic 官方公式 `⌈w/28⌉×⌈h/28⌉` 精确，但模型端还有"超出 max_tokens 自动缩放"规则（target=1568 tokens）未在客户端模拟。触发：用户反馈偏差 > 20%。 ✅ 已解决（Stage 115，2026-08-09 — `estimate_anthropic` 迭代缩放保比例到 ≤1568，4 UT） |

- **Impact**: 估算值仅供参考（阿里云官方明示），不阻塞分析/对账用途；多模态占比低时影响可忽略。
- **Resolution**: b/c 已在 Stage 115 解决（2026-08-09）；a 剩余（视频输入 + token 估算留待真实负载）。
- **Target Phase**: TD-011a 无固定排期（视使用量触发）。

### TD-012: health.rs embedding-mode 探测缺失 + 多模态 embedding 计费 ✅ Resolved 2026-08-09

- **Date**: 2026-08-09
- **Priority**: P2
- **Source**: Phase 44（OpenAI Embeddings API 代理）规划（ADR-026）
- **Description**: 两个 embedding 相关缺口登记。

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-012a | health.rs embedding-mode 探测缺失 | P2 | 现有 `run_and_save_health_check` 对所有 OpenAICompatible 模型 POST `{model, messages:[...], max_tokens:1}` 到 `/chat/completions`，embedding-only 模型会 400 误报 unhealthy。需按 `model_info.mode="embed"` 分支探测 `/embeddings`。触发：embedding 模型接入 health 探测。Phase 46 候选。 ✅ 已解决（Stage 113，2026-08-09 — `build_probe_spec` 按 model_info.mode 分支 + BDD 场景）|
| TD-012b | 多模态 embedding 按模态计费不支持 | P3 | gemini-embedding-2 按模态计费（$0.45/$6.50/$12.00），超出 aigw 单 `input_cost_per_token` 标量能力。触发：真实多模态 embedding 流量。 ✅ 已解决（Stage 115，2026-08-09 — `ModalPricing` + `Deployment.modal_pricing` + resolver 提取 + `calc_spend_modal` 纯函数 + 6 UT；**embeddings.rs 接线留待真实 per-modal input 流量**） |

- **Impact**: 非阻塞。薄 passthrough 对 embedding 模型健康探测会误报；多模态 embedding 计费按标量 input_cost 欠精确。
- **Resolution**: 视使用量触发。
- **Target Phase**: 无固定排期（health 探测候选 Phase 46）。

### TD-013: config.yaml 剩余未接线配置块（litellm_settings / 热重载 / 实例级负载路由）

- **Date**: 2026-08-10
- **Priority**: P3
- **Source**: Phase 46 Stage 116（ADR-031）——静态配置模型接入后收尾盘点
- **Description**: Stage 116 接通 `model_list` / `router_settings` / `environment_variables` / `custom_key_generate_length` / `disable_custom_api_keys` / `deployment_mode`，以下 config 能力仍解析但未接线：

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-013a | `litellm_settings`（drop_params / request_timeout / set_verbose）无对应实现 | P3 | 解析为 `serde_json::Value` 但零消费。drop_params 需 chat handler 过滤上游拒绝参数、request_timeout 需 HTTP client 超时、set_verbose 需日志级别联动——均需对应实现。触发：litellm config 迁移用户反馈缺失字段不生效。 |
| TD-013b | config.yaml 热重载 / `/router/settings` 请求时动态应用 | P3 | `PUT /router/settings` 只写 config 表（best-effort 日志"restart recommended"），Router 是 Clone 非 `Arc<Mutex<Router>>`，请求时不会读 config 表合并。触发：生产运维要求不改代码改路由配置即生效。 |
| TD-013c | RouterStrategy 实例级负载路由未实现 | P3 | `FromStr` 已声明 `usage-based-routing-v2` / `latency-based-routing` 变体，`pick_deployment` 沿用共享 shuffle+cooldown 路径。触发：多实例部署需真实负载感知选择。 |

- **Impact**: 非阻塞。静态配置主路径已通（config 驱动模型/环境/路由基础生效）；上述为 litellm 深度兼容与运维能力增强。
- **Resolution**: 视使用量 / 运维诉求触发（TD-013b 需 `Arc<Mutex<Router>>` 改造）。
- **Target Phase**: 无固定排期。

### TD-014: 上游模型停用能力收尾项（Stage 121 后续）

- **Date**: 2026-08-17
- **Priority**: P2
- **Source**: Stage 121 交付后三路 subagent 核实 + 补 BDD（禁用→不转发）后的收尾盘点
- **Description**: Stage 121 修复了"停用完全无效"（独立 `enabled` 列 + SQL/Resolver/Router 三层过滤已生效），但禁用能力仍有三处收尾缺口：

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-014a | 禁用→不转发无 `@real_api` 三端覆盖 | P2 | 本次补的 3 个 BDD 场景（`models.feature`：停用→chat 断言 400 `model_not_found` / 管理列表仍可见 / 重新启用→200）只在 mock 进程内验证（`TestWorld.ensure_state` sqlite::memory: + mock 上游 + deployment_mode="test"）。real BDD（`bdd.rs:137`）过滤器只跑 `@real_api` 标签场景，models.feature 无该标签 → **三端真实 DB 上的禁用→不转发无端到端覆盖**。触发：SQL `AND enabled` 或 resolver filter 回归且仅影响真实 DB 方言。 |
| TD-014b | env 回退兜底仍会转发禁用模型名 | P2 | `resolver.rs:66-105`：禁用模型 → `list_models_by_name` 返回空 → 若 `deployment_mode != "test"`（默认 `"onprem"`）且环境设了 `OPENAI_API_KEY`，`resolve()` 返回 env 上游 Deployment（而非 `model_not_found`），请求到禁用模型名仍被转发到 env 兜底上游（非禁用行的 api_base）。与"未注册模型名"行为一致，是预存行为非 Stage 121 回归；BDD 测试用 `deployment_mode="test"` 隔离了此路径。触发：部署同时配置 `OPENAI_API_KEY`。缓解：不设该 env / 视 env 兜底为 catch-all。 |
| TD-014c | config.yaml `model_list` 无法表达 `enabled:false` | P3 | `config::ModelEntry` 无 `enabled` 字段，`seed_models_from_config`（`config_loader.rs:84`）硬编码 `enabled: true`——静态配置模型无法声明停用，`config.yaml` 写 `enabled: false` 会被静默忽略。触发：运维想用静态配置声明某模型默认停用。 |

- **Impact**: 禁用能力主链路已生效（写路径 `/model/update` 省略保留、新建默认 true、迁移 026 历史行默认启用；读路径 chat/v1_messages/embeddings/responses 全走过滤后的 resolver）；缺口均为覆盖/边界/配置语义，无主路径正确性风险。
- **Resolution**: TD-014a 新增 `@real_api` 标签场景走真实 server+DB+真实上游（复用 `real_api_steps.rs` 模式）；TD-014b 记录为已知限制（文档注释已注明"env fallback is intended catch-all"）；TD-014c 给 `ModelEntry` 加 `enabled` 字段并让 config_loader 使用。
- **Target Phase**: 无固定排期（视真实三端覆盖诉求 / 静态配置停用诉求触发）。

### TD-015: OAuth 反代管线安全/产品收尾项（Stage 128 后续）

- **Date**: 2026-08-20
- **Priority**: P2
- **Source**: Stage 128 Gate 4 多模型代码评审 + 独立安全审计 + 逐条代码验证
- **Description**: Stage 128 反代管线交付后，评审发现的真实/待定项，非主路径正确性风险：

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-015a | 上游错误体原样透传客户端 | P2 | ✅ **Resolved 2026-08-24（Stage 130）** — `chat::upstream_error_message` 收窄，见 Resolved Items。 |
| TD-015b | billing 指纹 12-bit 碰撞 | P3 | `SHA256(SALT+chars[4,7,20]+version)[:3]` = 12 bit。独立验证判定**威胁模型不成立**（fp 只发上游、与账单归属无关、SALT 本就公开、身份 gate 无指纹注册表校验），是 sub2api/Parrot 上游设计固有。记录不修。 |
| TD-015c | SpendLog 明文 body 落库 | P3 | OAuth 分支 `messages: Some(body.clone())`（billing 注入前原始 body）+ `response`（上游响应原文）落 spend_logs。**access_token 只走 Authorization 头永不进 body**，非凭据泄漏；上游响应原文落库是全库既有模式。数据面大，若审计要求可纳入 Stage 130。 |
| TD-015d | OAuth 响应侧协议转换（H4，产品决策） | P2 | chat/responses 只做请求侧 `adapt_to_anthropic`，响应把 Anthropic 原生结构返回给 OpenAI 客户端（缺 `choices`/`usage.prompt_tokens`），OpenAI SDK 客户端无法解析。当前文档记"客户端接收 native response"。若需完整反代应补响应侧 `adapt_response`。**产品决策待定**。 |
| TD-015e | count_tokens 认证语义（M1，产品决策） | P3 | `count_tokens_handler` 用 Bearer-only `ChatAuth`，Anthropic 客户端按惯例用 `x-api-key` 会 401。应支持双认证（x-api-key / Bearer）与 messages_handler 一致。**产品决策待定**。 |
| TD-015f | health 探针不感知 OAuth 部署 | P3 | `health.rs` 探针对 OAuth 模型（api_base 默认 openai + api_key None + AnthropicNative）探错误端点且 401 计 healthy。OAuth 模型恒显示"健康"但探的是错误端点。 |

### TD-016: OAuth 前端 refresh 语义（Stage 129 后续）

| TD 编号 | 标题 | 优先级 | 说明 |
|---------|------|--------|------|
| TD-016a | Refresh 按钮 409 后仅重载列表、不自动降级为 Re-auth 对话框 | P3 | `handleRefresh` 捕获 409 → toast + 重载列表（needs_reauth 徽章 + Re-auth 按钮随即出现）。未做"409 自动打开 Re-auth 对话框"——需用户二次点击。触发：凭证 cookie/refresh 均失效时点 Refresh。缓解：toast 已提示 + 列表徽章可见。 |
| TD-016b | OAuth 凭证编辑对话框仍用通用 Credential 编辑（advanced JSON） | P3 | OAuth 行仍可点铅笔打开通用 `openEdit`（advanced JSON 模式），能手工改 status/last_error 等字段（字段加密落库）。非 Stage 129 需求（§2.1 只要求改 proxy_id + inject_prompt，未单独实现 OAuth 编辑表单）。纳入 Stage 130 可选。 |

### TD-017: Codex Responses 多轮适配遗留项（Stage 131 后续）

- **Date**: 2026-10-05
- **Priority**: P2
- **Source**: Stage 131 调研（`docs/research/2026-10-05-codex-responses-bridge-gap.md`）+ 生产网关实测
- **Description**: Stage 131 修复单轮 Codex 可用性（工具归一化 + role 归一 + content part 映射），以下为已知但**本 Stage 不做**的遗留项：

| Sub-ID | 条目 | 优先级 | 描述 |
|--------|------|--------|------|
| TD-017a | ~~**`input[].type` item 分派缺失**~~ | P2 | ✅ **Resolved 2026-10-05（Stage 132）** — `input_to_messages` 改为 `items_to_messages` 按 `type` 分派（`message` / `reasoning` / `function_call` / `custom_tool_call` / `tool_search_call` / `*_output` / 裸 content part / 未知类型跳过）+ `normalize_tool_pairing` 强制 Chat tool-call 不变量。见 Resolved Items。 |
| TD-017b | `tool_choice` 的 `{type:"namespace"}` 形态 | P3 | Codex 会发 `tool_choice: {type:"function", name, namespace}`（带 namespace 限定）；Stage 131 只处理 `{type:"function"\|"namespace", name}` 的具名选择项随工具进出的清理，未做命名空间字段的剥离与降级。 |
| TD-017c | 服务端工具真实支持（`web_search` 等） | P2 | ✅ **Phase 53 阶段 1 完成（设计 C，Stage 134-138 全绿，2026-10-10）**；设计 A（短路，服务 Claude Code 的独立搜索子请求）与设计 B（agentic loop）**留作独立 Phase**，见 ADR-039。原始记录：🔄 **进行中（Phase 53）—— Stage 134 ✅ 2026-10-06，135 ✅ 2026-10-07（接线），136 ✅ 2026-10-07（按次计费 + 独立 SpendLog 行，含 2026-10-08 真实 SearXNG 端到端 + 3 处缺陷修复 `6c200a7`），137-138 已交付** — Stage 131 对无 chat 对应能力的服务端工具（`web_search` / `web_search_preview` / `code_interpreter` / `computer_use` / `image_generation` / `shell` / `mcp`）**丢弃 + 告警**。2026-10-06 完成三份调研后定案走**设计 C（prompt 注入）**而非 litellm 的 agentic loop：搜索在调上游**之前**执行，结果注入请求，再照常单次调用——**上游零能力依赖**，且绕开两个硬约束（`encrypted_content` 无法伪造；自造服务端工具块会投毒重放历史 → 400）。**三种形态而非两种**：Higress `ai-search` 与 Portkey `exa/online` 各自独立实现了「搜索→改写 prompt→单次调用」。Phase 53 **五 Stage**（2026-10-06 两轮范围修正）：134 后端抽象（**多 provider 架构 + 仅实现 SearXNG，支持多实例**）/ 135 注入接线 / 136 按次计费（**aigw 首个非 token 计价**）/ 137 日志展现 / **138 provider+实例+定价 DB 化与管理 UI**。修正要点：① Tavily/博查不实现（抽象就位，接入是纯增量），多态与 failover 由 test-only `StubProvider` 证明；② 一个 provider 支持多实例（两级选择：实例级加权+冷却照抄 `router.rs:26/366-377/419`，provider 级 failover）；③ **推翻「SearXNG 零成本」**——自建有服务器/带宽/运维成本；`cost_per_query` **缺省 `0.01` USD/次**（= $10/1k，对齐 OpenAI 牌价与 sub2api `174_*.sql`），**缺省即非零故生产默认有金额**，但该值是牌价占位、自建摊销典型 `1e-4` 量级（不改约高估 50 倍）→ 应按「(服务器+带宽+运维) ÷ 月查询量」改写；选 0.01 而非 0.0 是刻意取舍（宁可高估也不要静默为 0）；④ 新增 138 补齐治理一致性（模型定价早可 UI 编辑、代理服务早有 CRUD+探测页，搜索 provider 没有是明确不一致）。调研：`docs/research/2026-10-06-gateway-websearch-architecture.md`（13 网关横向 + 线格式）、`2026-10-06-web-search-providers.md`（厂商选型）、`2026-10-06-aigw-websearch-codebase-map.md`（代码落点）。**注意**：litellm「派生 `web_search_options`」路线在本环境**无实效**（参数被收下但不执行搜索，实测）。**Stage 134 交付（2026-10-06）**：`aigw-core::websearch` 模块（trait/registry/instance/config/client/searxng 六文件）+ `build_websearch_registry` 装载 + `web_search` 顶层 config section + **81 UT**（aigw-core 530→611）；mock BDD 285 场景不变（零行为影响）。刻意未接请求管线（→ Stage 135）。 |
| TD-017g | Anthropic 服务端工具触发 HTTP 500（而非优雅降级） | **P2** | ✅ **Resolved 2026-10-07（Stage 135）** — `ClaudeToolDef` 的 `input_schema` 改为 `Option<Value>`，新增 `tool_type`（`#[serde(rename="type")]`）与 `max_uses`；`adapter.rs` 的 claude→openai 工具映射对 `input_schema.is_none()` 丢弃 + warn（与 Responses 侧 `other =>` 对齐）。带 `web_search_20250305` 的请求从 **500 → 200**，UT `anthropic_request_with_web_search_tool_no_longer_errors` 锁定。见 Resolved Items。 |

> 原始记录（2026-10-06）：`ClaudeToolDef`（`crates/aigw-core/src/models.rs:1065-1071`）的 `input_schema` 为**非 Option 且无 `type` 字段**——Anthropic 的 `web_search_20250305` 等服务端工具不带 `input_schema`，在 `adapter.rs:230-232` 的 `serde_json::from_value` 处直接反序列化失败 → **HTTP 500**。即 `/v1/messages` 路径比 Responses 路径更糟（后者至少是丢弃+告警）。已跨三路由核实落点（`v1_messages.rs:863` / `chat.rs:1537` / `responses.rs:618`）。**与 web search 无关的既存缺陷**，但同批修复最经济 → 已纳入 Stage 135 验收范围。发现于 2026-10-06 代码测绘（`docs/research/2026-10-06-aigw-websearch-codebase-map.md` §A3）。 |

### TD-018: Dashboard「Total Requests」上限 100（既存缺陷）✅ Resolved 2026-10-08（Stage 137）

- **Date**: 2026-10-06
- **Priority**: P3
- **Source**: Stage 137 规划期前端测绘（已人工核实）
- **Description**: Dashboard 的「Total Requests」卡片渲染 `logsData?.count`（`crates/aigw-frontend/src/pages/dashboard/index.tsx:258`），而该查询带 `limit=100`（`:142`）——`count` 是**本页条数**而非总数，故该指标**在请求数超过 100 后恒显示 100**。后端同一端点已返回 `total_count`（Spend Logs 页的 `SpendLogsResponse` 同时声明了 `count` / `total_count` / `page`，`pages/spend-logs/index.tsx:136-140`），但 Dashboard 的局部 interface（`dashboard/index.tsx:61-64`）只声明了 `count`，未取 `total_count`。
- **修复**: Dashboard 的 `SpendLogsResponse` 补 `total_count` 字段，卡片改读 `total_count`。
- **Resolution**: ✅ 已实现（Stage 137，2026-10-08）——`dashboard/index.tsx` 的局部 `SpendLogsResponse` 补 `total_count`，卡片改读 `logsData?.total_count`，并挂「含搜索调用」提示。**实测补充**：`limit` 参数在 `/global/spend/logs` handler（`spend.rs:668`）上被**忽略**（回落 `page_size=30`），故修前该指标的上限实为 **30** 而非 100 —— 详见下方 TD-019。

### TD-019: `SpendLogsQuery` 的 `limit` / `session_id` 是静默失效的死参数 ✅ Resolved 2026-10-10

- **Date**: 2026-10-08
- **Priority**: P3
- **Source**: Stage 137 §4.3 #12 聚合口径审计（实测发现）
- **Description**: `SpendLogsQuery`（`crates/aigw-server/src/routes/spend.rs:38`）声明了 `limit`（`:57`）与 `session_id`（`:45`），但两个消费它的 handler（`global_spend_logs` `:663`、`spend_logs` `:240`）都只读 `page` / `page_size`，从不读这两个字段 → **参数名承诺了行为、实现没给**。
- **Impact**: ① Dashboard「Period Spend」按 `?limit=100` 抓取却实收 30 条（`page_size` 默认值），求和窗口静默缩水 70%；② `session_id` 筛选静默无效 —— 对搜索行来说这本是 `parent_call_id` 之外**第二条**父子线索，且是**可索引的真列**（优于 §3.4 的 JSON 表达式过滤）。
- **Resolution**: ✅ 已实现（2026-10-10，commit `eee9124`）——`limit` 出现时**即 page size**，并加 `MAX_SPEND_LOG_PAGE_SIZE = 1000` 上限（此前无界）；`limit` 缺席时 `page_size` 行为不变。`session_id` 作为真列接通三方言（无需 JSON1/`->>` 分叉），trait / 3 impl / 分发 / **计数查询** / 两个 handler 全部跟上。**计数查询必须同步改**——否则筛选后分页条显示全量条数（Stage 137 踩过同一个坑）。TDD 红绿：`limit_sets_the_page_size` 还原旧代码时红在 `left: 30, right: 40`。

### TD-020: `aigw_migrate::native` 把 MySQL 的 INT 列值读成布尔 ✅ Resolved 2026-10-10

- **Date**: 2026-10-10
- **Priority**: **P0**（数据损坏；此前记为 P2 并判定「不影响实际迁移」—— **该判定是错的**）
- **Source**: Stage 138 real BDD（`bdd-real-mysql`）暴露 0/1 行的表象；随后专项复现发现范围远大于「聚合值」
- **Description**: `crates/aigw-migrate/src/native.rs` 的 `try_mysql_get`（`:986`）按固定顺序试解析列值，**`bool` 排在 `i64` 之前**（`:990` vs `:999`）。MySQL 的 `bool::compatible` 接受 **Tiny / Short / Long / Int24 / LongLong 全部有符号整型**（`sqlx-mysql-0.8.6/src/types/bool.rs:19-31`），解码实现是 `<i8>::decode() != 0`（`:40-42`）。于是：

  | 源值 | `<i8>::decode` | 读出的 `Value` | 迁移写入 | 结果 |
  |------|---------------|---------------|---------|------|
  | `0` | `Ok(0)` | `Bool(false)` | `0` | ✅ 巧合正确 |
  | **`1..=127`** | `Ok(非0)` | **`Bool(true)`** | **`1`** | ❌ **42 → 1、127 → 1** |
  | **负数**（如 `-5`） | `Ok(非0)` | **`Bool(true)`** | **`1`** | ❌ **-5 → 1（丢符号）** |
  | `128` 及更大 | `Err(溢出)` | `Number(n)` | `n` | ✅ 回退到 `i64` |

  **实测**（`spend_logs` 真表，MySQL 源；经 `aigw_migrate::native::read_rows_sql` 读出、`value_to_target_literal` 渲染为 INT 目标 —— SQLite `INTEGER` 与 MySQL `int` 结果相同）：

  ```
  源:      id=500  total_tokens=42   prompt_tokens=30    request_duration_ms=9000
  读+写:   id=500  total_tokens=1    prompt_tokens=1     request_duration_ms=9000
  源:      id=501  total_tokens=200  prompt_tokens=128   request_duration_ms=100
  读+写:   id=501  total_tokens=200  prompt_tokens=128   request_duration_ms=1
  ```

  **42 → 1，9000 → 1。** 任何落在 `1..=127` 或负数的整型列都被改写。

  PG 与 SQLite 源不受影响：PG 把 `i64` 钉在 INT8、`bool` 钉在 BOOL，两者不冲突；SQLite 的 `try_sqlite_get` **没有 `bool` 分支**（`:568`），走 `i64`。

- **Impact**: **MySQL → 任意目标**的迁移会静默改写所有落在 `1..=127` 或负数的整型列 —— `total_tokens` / `prompt_tokens` / `completion_tokens` / `image_tokens`（`1..=127` 是最常见的 token 数区间）、`spend_logs` 的计数列、任何 `INT` 业务列。PG→MySQL 方向不受影响（PG 侧读得准），所以**受害面是「MySQL 作为迁移源」**。

  ⚠️ **受害的是哪条链路**：值经 `try_mysql_get` 读入 → `value_to_target_literal` 按目标列类型渲染。这条路径用于 `aigw-migrate` 的字面量插入模式；native 的 streaming/batch 路径（`insert_rows_batch`）走「源 `Value` 直接绑定为目标类型的 bind 参数」，同样经过 `try_mysql_get`，故一并受影响。**结论：以 MySQL 为源的任何迁移都不可信。**

- **Fix**: `try_*_get` 改为**按列的声明类型分派**（而不是「按固定顺序试」）。
- **Resolution**: ✅ 已实现（2026-10-10，commit `0907f6c`）——`try_mysql_get` 改按 `type_info().name()` 分派：`BOOLEAN`/`BIT` → `bool`（sqlx 的 `bool::compatible` 含 `Bit` 而整型不含，故这两种列**只有 bool 能解**，类型名完整决定路径）；`TINYINT`/`SMALLINT`/`MEDIUMINT`/`INT`/`YEAR` → `i64`；`BIGINT UNSIGNED` → `u64`（超 i64 上界不降级为字符串）；`FLOAT`/`DOUBLE` → `f64`；`JSON` → `Value`；`DATETIME`/`TIMESTAMP`/`DATE`/`TIME` → 字符串；其余走 fallback，**其中不再探 `bool`**。新增 `tests/mysql_int_roundtrip.rs`（`#[ignore]`，`task test-migrate-mysql` 执行）：`0/1/42/127/128/9000/-5/-1` × {INT,BIGINT} 原值往返 + `BOOLEAN`/`BIT(1)` 仍是 bool 而普通 `TINYINT` 是整数。首轮红（`int_col did not decode as an integer: Bool(true)`），改后绿。
- **Target Phase**: ✅ 已解决（2026-10-10）

| TD-017d | Phase 41 遗留适配器 UT 缺口（剩余部分） | P3 | Phase 41 记录的「Stage 102 计划 19 个适配器 UT 未落地」——Stage 131 补 13 个、Stage 132 再补 10 个（item 分派 + tool 配对），剩余（streaming SSE 事件映射等）仍待补。 |
| TD-017e | ~~**Codex 多轮端到端验证缺失**~~ | P2 | ✅ **Resolved 2026-10-05（Stage 132）** — 用假上游驱动 Codex 0.160.0 完成真实一轮 tool 往返（`exec_command` → `echo hello` → 回填 → 收尾），抓包得真实 round-2 body（含 `function_call` / `function_call_output`），固化为 UT fixture + 变换后打真实上游 → 200。 |
| TD-017f | `developer_role_passthrough` 无自动嗅探 | P3 | Stage 131 新增的 `model_info.developer_role_passthrough` 为**显式配置**（缺省=映射）。aigw 无 litellm 式 provider 能力注册表（`ProviderType::infer` 把所有非 `anthropic` 归 `OpenAICompatible`），无法自动区分「真 OpenAI」与「OpenAI 兼容」上游。若将来引入能力注册表（litellm `get_supported_openai_params` 等价物），可改为自动嗅探。 |

### TD-021a: `/v1/messages` 不转发客户端的 `anthropic-beta` 头

- **Date**: 2026-10-28
- **Priority**: **P1**（功能静默降级 + 成本翻倍，无任何报错）
- **Source**: MCP Gateway 设计调研（`docs/plans/2026-10-28-mcp-gateway.md` §2），读码核实，**未实测抓包**
- **Description**: 非 OAuth 的 AnthropicNative 直通路径（`crates/aigw-server/src/routes/v1_messages.rs:901-911`）只为上游构造 `x-api-key` 与 `anthropic-version`，**客户端的 `anthropic-beta` 头被整体丢弃**（handler 内 `headers` 变量解构了 `anthropic-version`（`:280`）与 `user-agent`（`:422`）等，但**无任何一处读取 `anthropic-beta`** —— `grep -rn "anthropic-beta" crates/` 在 server 侧零命中）。
- **Impact**: Claude Code 2.1.305+ 的 tool search（`defer_loading` / `tool_reference`）依赖该 beta 串才启用。beta 丢失 → 上游不认识工具搜索 → CC 收到不识别的 4xx/5xx 后**自动退回全量预加载工具**。用户看到的是「能用但很贵」：每一轮请求都携带完整工具目录，tokend 消耗与首字延迟都按工具数线性上涨，**没有任何错误可查**。MCP 网关上线后工具数只会更多，这笔成本会随之放大。
- **Fix**: 在 handler 内提取 `headers.get("anthropic-beta")`，与 aigw 自身需要的 beta 串**做并集拼接**（不是覆盖），随 `x-api-key` / `anthropic-version` 一同注入上游请求。
- **Target Phase**: **Phase 55 之后单独修**（用户 2026-10-29 决策：MCP 开发完成后再启动，不阻塞 Phase 55 主线）。

### TD-021b: OAuth 反代路径把 `anthropic-beta` 写死，客户端 beta 被覆盖

- **Date**: 2026-10-28
- **Priority**: **P1**（与 TD-021a 同一后果，仅影响 OAuth 反代用户）
- **Source**: 同 TD-021a
- **Description**: `crates/aigw-core/src/oauth_pipeline.rs:64` 定义常量 `ANTHROPIC_BETA = "oauth-2025-04-20, claude-code-20250219"`，`OauthTarget::beta()`（`:286-292`）按 target 返回该常量或 `TOKEN_COUNTING_BETA`。这个头被写**两次**：`apply_cc_headers`（`:259`，CC 伪装头集的一部分）与 `send()` 的 `build` 闭包（`:325`）。两处的值都只由 aigw 决定；`send()` 的入参里**没有客户端请求头**，所以客户端的 beta 串在这一层**根本不可达** —— 是设计上的缺位，不是漏判。
- **Impact**: 走 Claude OAuth 订阅反代的用户，其 CC 的 tool search 同样被关掉（上游收到的是固定的两条 beta，不含工具搜索所需的那些）。与 TD-021a 是**两个独立入口、同一个用户可见后果**，必须一起修，否则修了一个另一个还在。
- **Fix**: `send()` 增加一个「客户端 beta 列表」入参（或 `OauthTarget` 携带），用逗号并集拼接后写一个 `anthropic-beta` 头；`TOKEN_COUNTING_BETA` 与 `ANTHROPIC_BETA` 降级为「aigw 自身必需的两条」，不再整串覆盖。**注意写入点有两处**（`apply_cc_headers:259` 与 `send:325`）—— 走 `apply_cc_headers` 的路径会先设一次，`send` 再覆盖一次，只改一处等于没改。
- **Target Phase**: 同 TD-021a，**Phase 55 之后单独修**。

### TD-022: `task doctor` 用 `|| true` 吞掉 clippy 失败，报「doctor passed」

- **Date**: 2026-10-29
- **Priority**: **P2**（门禁假绿，不产出错误数据，但会让「编译检查通过」这一结论失真）
- **Source**: Phase 55 Stage 143 实际发生 —— `task doctor` EXIT=0 且输出 `doctor passed`，同一时刻 `task lint` EXIT=201（`mcp_rest.rs` 的 `.bare()` 方法不存在）
- **Description**: `Taskfile.yml:8` 是 `cargo clippy --workspace -- -D warnings 2>/dev/null || true`，`2>/dev/null` 还把失败原因一起丢了；只有 `task lint`（`:261`，裸 `cargo clippy --workspace -- -D warnings`）会真退。而 `CLAUDE.md` 的快速命令表把 `task doctor` 描述成「编译检查（cargo check --workspace + clippy）」——文档承诺与实际行为不一致。
- **Impact**: 任何以 `task doctor` 当门禁证据的结论都可能把 clippy 错误当通过。Stage 143 期间主 Agent 差点据此判绿；真正的 clippy 错误只从 `task lint` 才看到。
- **Fix**: 二选一 —— ① 去掉 `|| true` 与 `2>/dev/null`，让 doctor 真的失败（代价：doctor 从「宽口径健康检查」变成硬门禁，与 lint 重复）；② 保留宽口径但在输出里明确写 `clippy skipped (use task lint)`，并同步改 `CLAUDE.md` 的措辞。推荐 ②，因为 doctor 的定位是「快速健康快照」，硬门禁应由 lint 承担。
- **Target Phase**: Phase 55 之后（不影响 MCP Stage 的功能正确性，只是门禁口径问题）。

### TD-023: `task bdd` / `task test` 对 BDD 失败**永不失败** ✅ Resolved 2026-10-29

- **Date**: 2026-10-29
- **Priority**: **P1**（门禁假绿：BDD 断言失败时 `task test` 仍 EXIT=0 并打印 `test passed`）
- **Source**: Phase 55 Stage 143 实测 —— `/tmp/stage141-test-1793217824.log` 里 `tests/features/real/mcp_passthrough_real.feature:24` 有 1 处 `✘ Then 响应状态码为 200`（`Expected status 200, got None`），而同一份日志末行是 `test passed`，`grep -c "^test result: FAILED"` 为 0。
- **Description**: `crates/aigw-server/tests/bdd.rs:140` 用的是 `TestWorld::cucumber()...filter_run(...)`，不是 `filter_run_and_exit`。cucumber-0.21 的 `filter_run` 只返回 writer，失败与否全靠调用方自己看；只有 `filter_run_and_exit`（`cucumber.rs:1250`）会在 `execution_has_failed()`（`writer/mod.rs:150-154`，即 `failed_steps() > 0`）时 `panic!`。所以 `main()` 正常返回 → 测试二进制 exit 0 → `cargo test` 记为 `ok`。
- **Impact**: **任何以 `task bdd` / `task test` 的退出码作为 Gate 3 证据的结论，都可能是假的**，只要失败只发生在 BDD 步骤上（UT 失败仍会正常报红）。Stage 143 的首轮 4 个 BDD 缺陷就是靠主 Agent 逐行读日志 `grep '✘'` 才发现的，退出码一路是绿的。
- **Fix**: ✅ 已修（2026-10-29，主 Agent）。没有换成 `filter_run_and_exit`，而是保留 `filter_run` 拿回的 writer，把清理块留在原处，在**清理之后**断言：
  ```rust
  use cucumber::writer::Stats as _;
  assert!(!writer.execution_has_failed(), "{} BDD step(s) failed, ...", writer.failed_steps(), ...);
  ```
  这样失败照样让 `main()` panic → 测试二进制非 0 → `cargo test` 记 `failed`；而 `server.stop()` / `mgr.drop_db()` 已在 panic 之前执行，不会像 `filter_run_and_exit` 那样每红一次漏掉一个 server 进程与一次性测试库。
- **Verified**: 2026-10-29 `/tmp/stage143-bdd-real-sqlite.log` —— 真实 BDD 失败时末段为
  ```
  thread 'main' panicked at crates/aigw-server/tests/bdd.rs:192:5:
  1 BDD step(s) failed, 0 parsing error(s), 0 hook error(s)
  error: test failed, to rerun pass `-p aigw-server --test bdd`
  task: Failed to run task "bdd-real-sqlite": exit status 101
  ```
  即退出码真的非 0，且清理先跑完。`task` 的 `cmds` 是串行的，`cargo test` 非 0 时后面的 `echo "bdd passed"` 不再执行 —— 所以「日志末尾出现 `bdd passed`」也重新成为一个可信的正向信号。
- **Residual rule**: 退出码现在可信，但「`✘` 计数为 0」仍是必要的交叉校验 —— 若将来有人把断言删掉或换回 `filter_run`，退出码会再次失真而没人能看出来。门禁证据继续给两个数字。
- **Target Phase**: ✅ Phase 55 内已修。

### TD-024: `/mcp` 聚合目录的分页游标是下标偏移，目录变动时静默漂移

- **Date**: 2026-10-29
- **Priority**: **P2**（不丢数据、不报错，只在多页遍历期间出现「跳一条 / 重复一条」）
- **Source**: Phase 55 Stage 143 对 Stage 144 `catalog::page` 的只读复查（`crates/aigw-core/src/mcp/catalog.rs`）
- **Description**: `next_cursor` 是 `end` 这个**网关内下标**（`page()` 里 `Some(end)`），下一页从 `entries[end..]` 开始。而 `entries` 每次请求都重新派生：`list_mcp_servers()` → `filter(enabled)` → `resolve_bearer` → `catalog_tools`（命中缓存，或现场探测，10s 超时）。于是两页之间任何一项变化都会让后续所有下标错位：某 server 被启用/禁用、某 `user_required` 用户的个人 token 被加上或删掉（该 alias 整块出现/消失）、冷缓存探测在第一页失败第二页成功（快照回退的工具数不同）、两页之间新注册了 server。
- **Impact**: 客户端拿到 200、无任何 warning（`warnings` 数组只在凭据/探测失败时触发，不覆盖集合漂移），但会漏掉或重复工具。漂移范围受 `merge` 按 `(alias, tool_name)` 排序约束，是有界的，不是全表乱序。
- **Why not fix now**: 改成 `(alias, tool_name)` 复合游标（`skip_while(|e| (e.alias, e.tool.tool_name) <= key)`）只需一次比较，但对插入稳定；代价是改 `catalog::page` 的签名与出线格式（cursor 从数字串变成带分隔符的键），并连带改 Stage 144 的分页测试与 BDD 断言。当前工具数（个位数 server × 几十工具）远未到需要多页的规模，`DEFAULT_PAGE_SIZE = 100`，实际只有 >100 工具才翻页。
- **Target Phase**: Phase 56 或首个 MCP server 数上两位数的阶段。届时一并决定 cursor 是否要带签名的 opaque token。
- **Related**: 同批复查确认 `merge` 的 `has_conflict` **不上线**（`ToolEntry::to_wire()` 不发该字段、`conflicted()` 无生产消费者），它是「调用方 bug 的防御网」而非线上契约 —— `/mcp` 的 `tools/list` 不带冲突信息，按现状接受，不登记为缺陷。

### TD-025: `task bdd-coverage` 的路由表没有 MCP 的 20 条路由，64% 不代表 Phase 55 之后的真实覆盖率

- **Date**: 2026-10-29
- **Priority**: **P2**（门禁是回归基线而非绝对目标，但「覆盖率没下降」这句话对新增面是虚假的）
- **Source**: Phase 55 收官时主 Agent 实跑 `task bdd-coverage`（64%，87 routes / 56 covered）
- **Description**: `scripts/bdd-coverage:33` 的 `ROUTES` 是一段**手写**的 `"METHOD /path"` 字面量，注释写的是「main.rs 同源」，但没有任何机制保证同步。Phase 55 新增的 20 条路由一条都不在表里：`/admin/mcp/servers` 全家（all / batch-delete / toggle / probe / tools / allowlist / tokens ×3 / reload / calls）、`/mcp`、`/mcp/{alias}`、`/v1/mcp/servers`、`/v1/mcp/servers/{id}/my-token`、`/mcp-rest/{tools/list,tools/call,health}`。于是 87 这个分母是「Phase 54 结束时的路由数」，64% 只描述那张旧表。
- **Impact**: 两个方向都有害 ——（a）真实覆盖率被高估，新增面没有 BDD 覆盖也看不出来；（b）如果有人把 MCP 路由补进表，分母涨到 107，而 MCP 的 BDD 用 helper 驱动（`发送 /mcp-rest/tools/list 请求且 alias 为 ...`、`用 master key 向聚合端点发送 tools/list`），`scan_dir` 的正则只认 `发送 (GET|POST|PUT|DELETE|PATCH) /path` 这种句式，绝大多数不会命中，百分比会掉到 60% 以下并让门禁红。
- **Why not fix now**: 补表 + 补扫描规则 + 让 MCP 的 BDD 改写成可扫描句式，是三件事，且第三件会改动一批已绿的 step 文本。Phase 55 的 BDD 覆盖是「按行为场景」写的，本来就不打算让 grep 数路由 —— 用场景覆盖去追平路由表是拿错了尺子。
- **Target Phase**: Phase 56。届时一并决定：要么把 `ROUTES` 改成从 `main.rs` 自动提取（`rg '\.route\("'` + 方法解析），要么把它降级为「Phase 54 的遗留基线」并停止用它衡量新面。
- **Related**: TD-003 已经记录了 87 → 55 的预置缺口；本条是「表本身过期」，不是同一件事。

### TD-026: `mcp_disable_rest` 默认 false + SSE 无界通道 + 五处 LOW（Phase 55 Gate 4 的剩余项）

- **Date**: 2026-10-29
- **Priority**: **P2**（都不是越权或数据错，但前两条在部署形态变化时会放大）
- **Source**: Phase 55 收官 Gate 4 独立复查（Stage 143/144/145），主 Agent 逐条复核后决定本批不修
- **Description**: 六项被判定「值得记录、不值得在本 Stage 末尾改」的结果：

| # | 级别 | 事实 | 位置 | 为何不现在修 |
|---|---|---|---|---|
| 1 | MED | `mcp_disable_rest` 默认 `false` —— 调试用的 `/mcp-rest/*`（绕过协议协商与 `Mcp-Session-Id`，多一套错误语义）默认在生产可reachable | `config.rs:174`、`main.rs:338-342`、`mcp_rest.rs:189` | 改默认值会同时改掉 143/144 的六条 BDD 与 `real/mcp_passthrough_real.feature` 的前置配置（它们依赖默认开启）；ACL 与凭据仍全程生效，它不是越权面而是「多开一扇门」。**Phase 56 做 OAuth 发现时一并决定**：要么默认 true、测试显式打开，要么在启动日志 WARN |
| 2 | MED | SSE 中继用 `mpsc::unbounded_channel`：客户端建连不读、上游持续推送时无背压 | `mcp_relay.rs:120`、`:160`、`:174` | 换有界 channel 要求 `send` 带超时/阻塞语义，且断开原因要落一个新的 `error_code`，会把 `relay_event_stream` 的 select 循环重写；当前无已知慢客户端场景。与 TD-024 同批处理（都是「多页/长连」类问题） |
| 3 | LOW | `catalog_tools` 在 `replace_mcp_tool_catalog` **失败后照样写内存缓存**，且信任上游 `ttl_ms` 无上限、无 single-flight → 冷缓存并发穿透 + 内存/DB 分叉 | `mcp_gateway.rs:745-754`、`catalog_cache.rs:24-30` | 上游 `ttlMs` 是可选字段（ADR-041 的缓存语义本就以进程内为准），DB 快照写失败已在日志可见；single-flight 需要引入 await 持锁，与 ADR-041「访问器不跨 await 持锁」冲突，要先改 ADR |
| 4 | LOW | `call_status_filter` 对未知状态直接透传 → 查询恒空而不是 400 | `mcp.rs:995-1003` | 是「查询参数拼错」这一类，返回空列表不误导人（列表为空本身就是答案），改成 400 属产品取舍 |
| 5 | LOW | `MyTokenDialog` 的 queryFn 吞掉所有错误返回 `null` → 403/500 在 UI 上均显示「未配置」 | `MyTokenDialog.tsx:68-69` | 与 142 Gate 4 的 M3 同族（前端错误吞噬），但这里最坏后果是用户在只读场景下看到「未配置」；修它要区分 401/403/500 三态 UI，超出 Phase 55 的收尾范围 |
| 6 | LOW | `list_aggregate_tools` 对 `user_required` 无凭据的 server **静默丢弃**，`warnings` 里也没有条目 → 客户端无法区分「没注册」与「缺个人凭据」 | `mcp_gateway.rs:836` | 静默剔除是 Stage 144 §3.3 的**有意设计**（列表里看不到，调用才 403），往 `warnings` 里加会改变聚合响应形状；若要做，应作为 Phase 56 的「凭据缺失可见性」一并设计 |

- **Impact**: 单独看都不构成故障。合并来看，它们共享一个形状：**某条路径的状态没有完整表达出来**（默认值没说清、背压没有、缓存与 DB 不一致、查询错误静默、UI 错误静默、剔除静默）。Phase 56 若做 OAuth 发现（会新增交互面与 401 引导），是统一处理这批的时机。
- **Why not fix now**: 六项里两项会改默认值或改 BDD 前置配置、一项与 ADR-041 直接冲突、三项属产品取舍。Phase 55 的收尾标准是「已确认的正确性缺陷清零」，这不是清零的定义 —— 已确认的 HIGH#1 与 MED#2/#3/#4/#5 已在本日修复并被测试覆盖。
- **Target Phase**: Phase 56（OAuth 发现），或首个真实部署前。
- **Related**: ADR-041（缓存语义）、TD-024（同属「长连/多页」）、TD-025（同批 Gate 4 产物）；`stage-143.md` §8.2 / `stage-144.md` §8 记录了本批的逐条判定。

### TD-027: `mcp_call_log` 没有客户端指纹，真机验收的调用归因只能靠人工比字节数

- **Date**: 2026-10-29
- **Priority**: **P2**（不影响线上行为，只影响「验收能不能被机器判定」）
- **Source**: Stage 143 真机验收（ADR-046）—— 一条 `tools/call` 曾被误判为 Codex 产生，实际是手工 curl
- **Description**: `mcp_call_log` 的列里没有 `clientInfo.name` / `clientInfo.version`，`request_bytes` 只是入站 body 的字节数。MCP 客户端在 `initialize` / 每条请求的 `params._meta` 里都会带 `io.modelcontextprotocol/clientInfo`（CC 2.1.312 带 `{"name":"claude-code","version":"2.1.312"}`，Codex 0.154.0 在 `initialize` 的顶层 `clientInfo` 带 `{"name":"codex-mcp-client","version":"0.154.0"}`），`session.rs` 解析时把它丢掉了。
- **Impact**: 验收一个「两台客户端各跑通一次」的门禁项时，无法从库里区分哪一行是谁打的。本次的判别手段是手工算 body 字节数（`tools/call` 102B = 手写、`tools/list` 85B = Codex 带 `_meta.progressToken`、`initialize` 217B = 带 `clientInfo`）—— 可复现但不能自动化，且 UUIDv7 前缀在这类判断上会误导（同一秒内的手工与 CLI 同前缀）。
- **Why not fix now**: 要动 `031_mcp_call_log.sql` 的三方言（加两列）、`McpCallLogRow`、三个 store impl、`session.rs` 的解析与 `_meta` 提取路径，以及 141 的 `GET /admin/mcp/calls` 返回体。Phase 55 的验收已经用人工方式完成，为「下次验收更省事」在收尾时改表结构不划算。
- **Target Phase**: Phase 56，与 OAuth 发现的 `WWW-Authenticate` 引流一并做（那时会新增交互面，客户端识别本来就是刚需）。
- **Related**: ADR-046（本次归因过程）、`stage-143.md` §8.3、TD-026（同批「状态没被完整表达」类）

### TD-028: BDD 里「步骤匹配不上」被静默算作 skipped，13 个 body_archive 场景从未真正执行

- **Date**: 2026-10-29
- **Priority**: **P1**（不是功能 bug，是**测试证据的可信度**问题：绿灯掩盖了未执行）
- **Source**: Phase 55 收口跑 `task test` 时发现 `47 features / 362 scenarios (344 passed, 18 skipped)`，其中 5 个是 `mcp_aggregate.feature` 的 `tools/list` 场景 —— 该 step 的 expr 写了裸 `/`（cucumber expression 里 `/` 是 alternation 分隔符，必须写成 `\/`），于是**所有断言「聚合目录含哪些工具」的场景整段被跳过**，而 Stage 144 的门禁曾把这些场景当作证据。
- **Description**: 同一类问题在 `body_archive_write.feature` / `body_archive_read.feature` 里还有 13 处，至今仍在跳过：
  - `body_archive_write.feature:15/20/25` 写 `BodyArchiver.tick()`，而 `body_archive_steps.rs:130/138` 的 expr 是 `BodyArchiver.tick`（少了 `\(\)`）；
  - `body_archive_write.feature:31/41` 的 `payload = {"hour": ...}` 与 `expr = "async_job_steps 中有一个 pending step，payload = {string}"` 不匹配（gherkin 未加引号的 `{...}` 段被 expression 当作参数，实际文本不是带引号的字符串）；
  - `body_archive_read.feature` 整文件挂 `@skip` 标签，但 `bdd.rs:144` 的过滤只看 **scenario** 的 tags、不看 feature 的 tags，所以 7 个场景照样进入执行、再因 step 缺失被跳过。
- **Impact**: `task bdd` 的退出码只由 `writer.execution_has_failed()` 决定，而 cucumber-rs 的 `StepSkipped` **不计为失败**（`runner/basic.rs:1345`）。所以「步骤找不到」与「写好了但 @skip」这两种情况在日志里长一个样（`?` + `Step skipped`），且都让整轮测试以 0 退出。按 skipped 数就能看出来，但没人看。
- **Why not fix all now**: `mcp_aggregate` 的两条已在 Phase 55 收口时修好（`tools\/list`）并复跑确认 5 个场景转红为绿；`body_archive_*` 属于 Phase 31/78 的遗留，修 expr 后那些场景**第一次真正执行**，断言是否成立未知，需要单独一轮排查与修实现——不属于 Phase 55 的交付面。
- **Target Phase**: Phase 56 之前单独一轮（与 TD-023 同源：都是「BDD 假绿」）。根治手法是 `bdd.rs` 里加一道 `skipped_steps() == 预期值` 的断言，把「静默跳过」变成硬失败。
- **Related**: TD-023（BDD 失败不失败，已修）、`stage-144.md` §7（曾把跳过场景当证据，现已改为引用真实执行结果）

## Resolved Items
### TD-017g: Anthropic 服务端工具触发 HTTP 500（Resolved 2026-10-07, Stage 135）

- `ClaudeToolDef`（`models.rs`）：`input_schema: serde_json::Value` → `Option<serde_json::Value>`；新增 `tool_type: Option<String>`（`#[serde(rename = "type")]`）与 `max_uses: Option<i64>`。
- `adapter.rs` claude→openai 工具映射：`input_schema.is_none()` 的工具（服务端工具）**丢弃 + `tracing::warn!`**，与 Responses 侧 `normalize_responses_tools` 的 `other =>` 分支语义对齐；全空的工具列表折叠为 `None`，不产出 `tools: []`。
- 效果：带 `web_search_20250305` 的 `/v1/messages` 请求 **500 → 200**（未配置搜索时工具被丢弃；已配置时由 Stage 135 的接线执行搜索并注入）。
- **注意**：`AnthropicPassthrough`（AnthropicNative 直通）路径**不做**该丢弃 —— 上游原生支持服务端工具，网关不介入（这也正是 Stage 135 `upstream_handles_search` 豁免的依据）。


### TD-017a: `input[].type` item 分派缺失（Resolved 2026-10-05, Stage 132）

- `input_to_messages` 拆为 `items_to_messages`（按 `type` 分派）+ `normalize_tool_pairing`（强制 Chat tool-call 不变量）。
- 分派表：`message`/无 type → 普通消息（content parts 归一）；`reasoning` → 暂存，附到下一条 assistant；`function_call`/`custom_tool_call`/`tool_search_call` → assistant 的 `tool_calls`（并行调用合并进同一条 assistant；`namespace` 字段按请求侧同一规则拍平；`custom` 的 free-form `input` 包成 `{"input": ...}`）；`function_call_output`/`custom_tool_call_output`/`tool_search_output` → `role="tool"`（对象/数组输出字符串化）；裸 `input_text`/`text` → user 消息；未知类型（`web_search_call`/`local_shell_call`/`file_search_call` …）跳过。
- **归一化不变量**：未应答的 `tool_call` 剪除（中途重连残留）；孤立 `tool` 回复丢弃；answered `tool_calls` 后紧跟其 tool 回复（按调用顺序）——strict 上游（DeepSeek/OpenAI schema）硬要求。
- 验证：aigw-core **527** UT（Stage 132 新增 10 + 既有）全绿；mock BDD **284 场景（271 pass / 13 skip）**；`task fmt`/`task lint` green。
- **端到端验证**：用假 Responses-SSE 上游驱动 Codex 0.160.0 走完真实一轮 tool 往返，抓得 round-2 body → 变换后打真实上游 **200**（回复「`hello`」，即模型正确读到了 tool 结果）。

### TD-017e: Codex 多轮端到端验证缺失（Resolved 2026-10-05, Stage 132）

- 方法：远端 `/tmp/fake_upstream.py` 按 Responses SSE 协议返回一轮 `function_call`（`exec_command`），Codex 执行后发 round-2（含 `function_call` + `function_call_output`），抓包固化。
- 结论：Codex 端到端一轮往返可跑通；变换后的 round-2 payload 被真实上游接受（200）。fixture 已固化为 `test_responses_to_chat_multiturn_codex_fixture_end_to_end`。

### TD-015a: 上游错误体原样透传客户端（Resolved 2026-08-24, Stage 130）

- 新增 `chat::upstream_error_message(status, body)`——只提取上游 `error.message`（Anthropic object / string 双形状），parse 失败退化为 `Upstream returned HTTP {status}`；chat/v1_messages/responses/embeddings 四处 handler 全部接线。
- real BDD 验证：SQLite/PG/MySQL 三后端 58/58 × 3 全绿；+1 UT（含 prompt-echo 不泄漏断言）。
- TD-015c（SpendLog 明文 body 落库）维持 P3 记录不修（access_token 只走 Authorization 头、永不进 body）。

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
