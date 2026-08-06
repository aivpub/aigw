# aigw -- Architecture Decision Records (ADR)

## ADR-001: RDD Framework Adoption

- **Date**: 2026-07-03
- **Status**: Accepted
- **Decision**: Use RDD (Research-Driven Development) framework with staged delivery.
- **Rationale**: Structured approach for building a litellm Rust alternative. Each stage
  validates a specific capability before proceeding to the next, reducing risk and
  enabling phased validation against litellm v1.90.0 behavior.
- **Consequences**: 6 Stages across 4 Phases, each with explicit verification gates.
  All stages completed as of 2026-07-03.

## ADR-002: SQLite-First with Multi-DB Support

- **Date**: 2026-07-03
- **Status**: Accepted
- **Decision**: Default to SQLite for local/dev, with PostgreSQL/MySQL available via
  `DATABASE_URL` configuration.
- **Rationale**: Zero-config local development and single-instance self-hosted deployments
  benefit from SQLite's simplicity. Production SaaS deployments with multiple instances
  require PostgreSQL. MySQL support accommodates existing infrastructure.
- **Implementation**: sqlx with compile-time checked queries targeting all three backends.
  Migrations in `crates/aigw-core/migrations/` for SQLite, MySQL, and PostgreSQL.
- **Consequences**: Every schema migration must be tested across all three databases.
  25 unit tests + 2 integration tests (PostgreSQL + MySQL via testcontainers) validate
  cross-DB compatibility.

## ADR-003: litellm Schema Compatibility

- **Date**: 2026-07-03
- **Status**: Accepted
- **Decision**: Maintain 11 table schemas matching litellm structures, using aigw's own
  table naming (e.g., `virtual_keys` instead of `LiteLLM_VerificationToken`).
- **Rationale**: The `aigw-migrate` tool provides the bidirectional mapping layer,
  enabling litellm <-> aigw migration without data loss. Using aigw's own table names
  avoids coupling the codebase to upstream naming conventions.
- **Implementation**: 11 table schemas in `crates/aigw-core/src/models.rs` with full
  column and foreign key alignment. `aigw-migrate` binary handles import/export/verify.
- **Consequences**: Schema changes in upstream litellm require corresponding migration
  updates. The mapping table in `docs/01-charter.md` section 6 documents the bidirectional
  mapping.

## ADR-004: Dual-Mode SaaS Architecture

- **Date**: 2026-07-03
- **Status**: Accepted
- **Decision**: Support both OnPrem (no tenant isolation) and SaaS (org-scoped keys) modes
  from a single binary, controlled by a `DEPLOYMENT_MODE` configuration value.
- **Rationale**: A single binary serving both self-hosted and cloud deployments reduces
  maintenance burden and ensures feature parity across deployment models.
- **Implementation**: `DeploymentMode` enum (`OnPrem` | `SaaS`), `TenantContext` struct
  carrying `organization_id`, `TenantAuth` extractor for request-scoped tenant resolution.
- **Key design**: Master keys bypass all restrictions in both modes. SaaS mode rejects
  keys without `organization_id`. OnPrem mode skips all tenant filtering.
- **Consequences**: Every data access path must be tenant-aware when running in SaaS mode.
  The `TenantDb` wrapper in `crates/aigw-core/src/tenant.rs` provides this abstraction.

## ADR-005: Taskfile.yml as Unified Workflow Entry Point

- **Date**: 2026-07-03
- **Status**: Accepted
- **Decision**: Use Taskfile.yml (the `task` command) for all development workflows
  instead of a Makefile.
- **Rationale**: Better cross-platform support (Linux, macOS, Windows), cleaner YAML
  syntax, and adequate feature coverage for Rust project workflows. No need for the
  complexity of Make for this project's scope.
- **Commands**: `task doctor`, `task test`, `task build`, `task fmt`, `task lint`,
  `task docker-build`, `task docker-up`, `task docker-down`, `task status`.
- **Consequences**: Contributors must have `task` (go-task) installed. This is documented
  in CLAUDE.md and the project README.

## ADR-006: BDD with cucumber-rust and Mock Upstream Server

- **Date**: 2026-07-04
- **Status**: Accepted
- **Decision**: Use cucumber-rust 0.21.1 for BDD testing with an in-memory mock upstream
  server for all end-to-end proxy scenarios. `@real_api` scenarios default to skipped
  unless `AIGW_REAL_API=1` is set.
- **Rationale**: BDD scenarios validate the gateway's contract-level behavior from the
  perspective of an API consumer. A mock upstream server avoids dependency on external
  LLM APIs (cost, flakiness, rate limits) while still validating full request/response
  lifecycle including authentication, validation, proxying, and error propagation.
- **Implementation**:
  - `tests/bdd_support/mock_upstream.rs`: Axum server on ephemeral port supporting
    `/v1/chat/completions` (OpenAI) and `/v1/messages` (Claude) with configurable
    responses and request recording.
  - `tests/bdd_steps/`: Domain-specific step bindings (keys, error, e2e, spend, etc.)
    with `cucumber::when/given/then` attributes using `expr = "..."` syntax.
  - `tests/bdd.rs`: `TestWorld` with lazy `ensure_state()` initialization, serial
    scenario execution via `max_concurrent_scenarios(1)`.
  - `make_request()` helper in `common.rs` adds `Bearer ` prefix automatically.
- **Key design decisions**:
  - Feature files use plain `/` in paths; Rust step bindings use `\/` (cucumber
    expression syntax requirement).
  - `@mock` scenarios always run in CI; `@real_api` scenarios in `features/real/`
    directory auto-skip without `AIGW_REAL_API=1`.
  - Scenarios run sequentially due to shared global MockUpstream state.
- **Consequences**: 63 @mock scenarios passing (end_to_end: 6, error_handling: 7,
  auth: 5, plus model/spend/health/key/protocol features from stages 7-11).
  9 @real_api scenarios created (end_to_end_real: 6, compatibility_real: 3),
  auto-skipped in CI.

## ADR-007: React + TypeScript + shadcn/ui Frontend Stack

- **Date**: 2026-07-08
- **Status**: Accepted
- **Decision**: Use React 19, TypeScript, Vite 8, shadcn/ui (Radix primitives + Tailwind
  CSS v4), TanStack React Query v5, React Router DOM v7, Recharts v3, date-fns v4, and
  sonner for toast notifications. Default path alias `@/` maps to `src/`.
- **Rationale**: React ecosystem has the widest component library support and TanStack
  React Query is the standard for server-state management in React SPA. shadcn/ui
  components (built on Radix primitives) provide accessible, composable UI that matches
  the admin console use case. Tailwind CSS v4's `@theme` directive with HSL CSS custom
  properties enables dark mode support in a single CSS file without tailwind.config.ts.
  Recharts was chosen over alternatives for its React-native charting with minimal config.
- **Alternatives considered**:
  - **HTMX + server-rendered templates**: Rejected — admin console needs rich interactions
    (CRUD modals, charts, real-time search/filter) that require client-side JS.
  - **Next.js**: Rejected — SPA is sufficient for an admin console behind authentication;
    SSR/SSG adds complexity without benefit for this use case.
  - **Svelte**: Rejected — smaller ecosystem for admin UI components and fewer developers
    familiar with it.
- **Implementation**: Frontend in `crates/aigw-frontend/` as a standalone Vite project.
  Components live in `src/components/ui/` (shadcn) and `src/components/layout/` (shell,
  sidebar, header). Pages under `src/pages/` per domain (dashboard, keys, models, health).
  API calls via a lightweight `apiGet/apiPost/apiPut/apiDelete` wrapper around fetch in
  `src/lib/api.ts`.
- **Consequences**: 4 admin pages (dashboard, keys, models, health) delivering working
  production builds. Frontend served via rust-embed from the aigw-server binary at
  `/admin` route, providing a single-binary deployment. All pages consistently use the
  same loading/empty/error state patterns. Future pages follow the same structure.
  TypeScript compilation with `verbatimModuleSyntax` and `erasableSyntaxOnly` ensures
  maximum compatibility with modern tooling.

## ADR-008: rust-embed for Single-Binary Frontend Deployment

- **Date**: 2026-07-08
- **Status**: Accepted
- **Decision**: Use `rust-embed` crate to embed the Vite-built frontend assets
  (`crates/aigw-frontend/dist/`) directly into the `aigw-server` binary, served at
  `/admin` and `/admin/{*rest}` routes with SPA fallback to `index.html`.
- **Rationale**: Single-binary deployment is a core requirement for self-hosted users.
  rust-embed compiles static assets into the binary at build time, eliminating the need
  for a separate web server, reverse proxy configuration, or asset directory management.
  The 840KB frontend adds negligible size to the 15MB release binary. SPA client-side
  routing is preserved through the `index.html` fallback for unmatched paths.
- **Alternatives considered**:
  - **Separate web server (nginx/Caddy)**: Rejected — adds operational complexity for
    self-hosted users. Single binary is simpler.
  - **Tower-http ServeDir**: Rejected — requires the `dist/` directory on disk at
    runtime, complicating deployment.
  - **include_dir**: Rejected — less mature than rust-embed, lacks `mime_guess`
    integration.
- **Implementation**: `crates/aigw-server/src/frontend.rs` defines a `FrontendAssets`
  struct via `#[derive(RustEmbed)]` pointing to `../aigw-frontend/dist/`. Two axum
  routes (`/admin`, `/admin/{*rest}`) serve embedded files with proper MIME types.
  Asset files get `Cache-Control: immutable` for 1 year; HTML gets `no-cache`.
  Unknown paths fall back to `index.html` for React Router client-side navigation.
- **Consequences**: Frontend must be rebuilt (`npm run build`) before compiling the
  server binary. CI pipelines should build the frontend first. Binary size increases
  by ~840KB (the frontend dist size).

## ADR-009: Complete Core Stages (0-30) — Defer Production Advanced Features

- **Date**: 2026-07-08
- **Status**: Accepted
- **Decision**: Mark all 30 core stages as complete. Defer Phase 10 production-advanced
  features (Redis caching, Prometheus/OTEL observability, SSO/OAuth, K8s operator) to
  post-deployment trigger-based activation rather than pre-building them.
- **Background**: Phase 0-11 delivered: BDD-driven backend (72 scenarios, 3 DB backends),
  production litellm migration tooling (aigw-migrate + pre-check + rollback.sh),
  structured JSON logging with request_id tracing, multi-tenant management API (org/team/user
  CRUD, 15 endpoints), single-binary deployment with embedded React frontend,
  JWT+Cookie+scrypt login security, mobile-responsive admin console (6 pages, 69 BDD tests
  across 3 viewports), and health check metrics (/health/metrics).
- **Rationale**: The current system covers the minimum viable production surface for
  self-hosted AI Gateway deployments. Phase 10 items (Redis, Prometheus, K8s) are
  infrastructure optimizations that should be triggered by actual production demand
  signals (QPS thresholds, multi-instance needs, enterprise customer requirements)
  rather than pre-built prematurely. Building infrastructure without real load patterns
  risks over-engineering for scenarios that may never materialize.
- **Impact on Subsequent Stages**:
  - Phase 10 remains as a trigger-based backlog — each item has a clear activation criterion
  - All further work should be demand-driven: wait for production signals before building
  - Current architecture supports incremental addition of Redis/Prometheus/K8s without rework
- **Alternatives Considered**:
  1. **Continue building Phase 10 immediately**: Rejected — no production load data to
     inform sizing/configuration decisions. Risk of building wrong abstractions.
  2. **Only build Prometheus metrics**: Rejected — structured JSON logs already provide
     sufficient observability for single-instance deployments.
  3. **Build K8s operator proactively**: Rejected — single-binary Docker deployment meets
     current needs; K8s is premature without multi-instance demand.

## ADR-010: Phase 12 Completion — Sidebar Restructure + Playground + Spend Logs

- **Date**: 2026-07-09
- **Status**: Accepted
- **Decision**: Complete Phase 12 (Stages 31-33): three-group sidebar navigation matching
  litellm admin UI structure, standalone Spend Logs page with filters, and Playground Chat
  debugging page with SSE streaming support. Mark all 33 stages complete as the production-ready
  baseline.
- **Background**: Phase 12 aligned the frontend admin console with litellm's production UI
  patterns. Stage 31 restructured the sidebar into 3 groups (AI GATEWAY / OBSERVABILITY /
  ACCESS CONTROL) with 8 pages. Stage 32 split Spend Logs from Usage into a dedicated page
  with date/model filtering and 30s auto-refresh. Stage 33 added a Playground page for
  interactive model testing with streaming toggle and Markdown response rendering.
- **Rationale**: The litellm-compatible sidebar structure improves navigation UX for users
  familiar with the upstream project. A standalone Spend Logs page addresses the unbounded
  table growth problem on the Usage dashboard. The Playground provides a zero-cost way for
  admins to test model behavior without leaving the admin console.
- **Implementation**:
  - Sidebar: `src/components/layout/sidebar.tsx` — three `<SidebarGroup>` sections with
    grey uppercase headers, route-aware active state, 8 nav items
  - Routes: `/dash` redirects to `/dash/usage`; `/dash/home` removed; pages at
    `/dash/usage`, `/dash/virtual-keys`, `/dash/models`, `/dash/playground`,
    `/dash/spend-logs`, `/dash/users`, `/dash/teams`, `/dash/organizations`
  - Playground: Right-sidebar controls (model selector, temperature slider 0-2,
    max tokens), left main area (System prompt textarea, User message textarea),
    SSE streaming with abort support, react-markdown response rendering
  - Spend Logs: Desktop table layout (Time, Model, Tokens, Cost, Status columns),
    mobile card list layout, date range picker, model name filter, 30s auto-refresh
- **Test Coverage**: 102 BDD tests passing (34 scenarios × 3 viewports: desktop 1280px,
  tablet 768px, mobile 375px). playwright-bdd with Gherkin .feature files and TypeScript
  step definitions. Mock API routes via Playwright route interception.
- **Consequences**: 33/33 Stages complete. Frontend at 8 admin pages with full mobile
  responsiveness. Phase 10 (Redis/Prometheus/OTEL/K8s) remains deferred to
  trigger-based activation as documented in ADR-009.

## ADR-011: Phase 13 — User Feedback-Driven Improvements + TTFT Gap Fix

- **Date**: 2026-07-10
- **Status**: Accepted
- **Decision**: Initiate Phase 13 (Stages 34-38) to address 4 areas of user feedback:
  (1) Spend Logs page — Live Tail, pagination, request_id search, detail drawer;
  (2) Usage page — decoupled from spend logs, daily aggregation, total requests count;
  (3) Organizations list fix + Users pagination;
  (4) Playground — upgrade to chat-style multi-turn conversation.
  Additionally fix the TTFT gap: `completion_start_time` exists in schema but is never populated,
  and SSE streaming proxy was never implemented (returns stub JSON in `chat.rs`).
- **Background**: Phase 12 delivered baseline 8 pages, but user feedback identified gaps
  in functionality depth and correctness. A parallel deep-dive into TTFT compared litellm's
  approach (streaming handler captures `completion_start_time` on first chunk, SQL computes
  TTFT at query time with `CASE WHEN` guard for non-streaming sentinel values) with aigw's
  current state (column exists but is hardcoded `None` at all 3 insert sites).
- **Key design decisions**:
  - **TTFT follows litellm pattern**: No `ttft_ms` column. Compute at query time.
    SQLite: `(julianday(completion_start_time) - julianday(start_time)) * 86400000`.
    Guard: return NULL when `completion_start_time = end_time` (non-streaming sentinel).
  - **SSE streaming proxy**: Implement real streaming in `chat.rs` using `reqwest::Response`'s
    `bytes_stream()` + axum `Sse`. Capture `completion_start_time = Utc::now()` on first chunk.
    Write SpendLog on stream completion.
  - **Pagination pattern**: `{ data, count, total_count, page, page_size, total_pages }`
    consistent across all paginated endpoints.
  - **Phase 10 remains deferred**: Trigger-based activation unchanged.
- **Implementation**: 5 stages (34-38), 23.5h total, each 4-5.5h. Stage 34 (SSE streaming + TTFT + spend
  logs backend) is the critical path. Stage 34+36 parallelizable. Stage 36 merged the
  original separate backend/frontend stages for Users/Orgs into one end-to-end stage.
  Stage 38 (Playground) independent of other stages.
- **Consequences**: ~25 new BDD scenarios. SSE streaming proxy is the riskiest item — first
  real streaming implementation in the project. Stage 34 gates stages 35 and 38.
  Stage 36-37 (users/orgs) and Stage 39 (playground) are independent tracks.

## ADR-015: Architecture Refactor — ModelResolver + MessageAdapter over Feature Enhancements

- **Date**: 2026-07-14
- **Status**: Accepted
- **Decision**: Replace Phase 17 feature work (Usage multi-view aggregation, Stages 50-51) with a proxy-forwarding architecture refactor: ModelResolver (model → Vec<Deployment>), MessageAdapter trait (OpenAI Chat ↔ Anthropic Messages bidirectional), and Handler slim-down. Defer Usage multi-view aggregation to LT-Usage (P2, activation on user feedback).
- **Background**: Phase 14-16 delivered 10 Stages of feature work (v1/messages fixes, feedback round 2, Playground enhancements), bringing the total to 49 completed Stages. However an architecture audit revealed structural debt:
  - `chat.rs` and `v1_messages.rs` each independently resolve upstream parameters (~230 lines of duplicate logic in `resolve_upstream_params`)
  - `DefaultAdapter` is a monolithic struct with 6 methods, hardcoded for a single conversion direction
  - `provider_registry` and `router_state` exist in `AppState` with `#[allow(dead_code)]` — defined but never wired into any request path
  - No `Deployment` abstraction exists — upstream routing details (api_base, api_key, pricing) are scattered across inline resolution code
  - The current structure makes it impossible to add new upstream protocol types or multi-instance routing without duplicating handler logic
- **Rationale**: Architecture quality gates future velocity. Continuing to add features (Usage multi-view, more endpoints) on top of duplicated handler logic creates compounding tech debt — every new endpoint would copy-paste the resolve-upstream pattern. Fixing the architecture first reduces the cost of all subsequent features. The Usage multi-view feature (pie charts by provider, team/org/key dropdown) is not blocking any current user — it can safely defer to a user-feedback trigger.
- **Design**: Three-stage progressive refactor:
  - **Stage 50**: `ModelResolver` + `Deployment` — new modules (`deployment.rs`, `resolver.rs`), migrate `resolve_upstream_params` logic into `ModelResolver::resolve() → Vec<Deployment>`, replace `chat.rs` call site. No behavior change.
  - **Stage 51**: `MessageAdapter` trait — split current monolithic `DefaultAdapter` into `MessageAdapter` trait + `StreamAdapter` trait, implement `OpenAIPassthrough` and `AnthropicToOpenAI` (migrates DefaultAdapter logic), add `select_adapter()` dispatch based on (client_protocol, provider_type).
  - **Stage 52**: Handler slim-down — refactor `chat.rs` and `v1_messages.rs` to thin orchestration layers: validate → resolve → adapt → upstream call → spend log. Remove dead code and duplicated patterns.
  - See `docs/plans/2026-07-13-arch-refactor-plan.md` for full design details.
- **Consequences**:
  - Stage numbering: 50-52 replaces the old 50-51 (Usage multi-view), which moves to LT-Usage (P2).
  - RouteDispatcher renamed to ModelResolver (resolves, doesn't route). ProviderAdapter/DefaultAdapter renamed to MessageAdapter + implementations (AnthropicToOpenAI, OpenAIPassthrough) — clarifies it's about message format conversion, not provider configuration.
  - `Deployment` = pure value object (api_base, api_key, upstream_model, provider_type, pricing) — one per proxy_models row, ModelResolver returns Vec.
  - Future Router Phase: handler receives Vec<Deployment>, Router selects one (strategy + cooldown + fallback). No change needed in handler interface — just iterate Vec instead of taking [0].
  - Router load balancing and native Anthropic upstream remain trigger-activated (LT-Router, LT-Native).
  - No Stage detail docs created yet — to be written during implementation.

## ADR-016: Anthropic→OpenAI 转换的多 system 消息归一化(能力标志 + 折叠)

- **Date**: 2026-07-16
- **Status**: Accepted
- **Decision**: `AnthropicToOpenAI` 转换路径引入 chat template 兼容性开关,默认按 `upstream_model` 名称自动嗅探;对严格模板模型(当前是 Qwen 家族)把非首位 `role="system"` 折叠进相邻 `user` 消息(用 `<system-reminder>` 标签包裹),对其它模型完全透传。
- **Background**: Claude Code v2.1.153+ 客户端把额外 system 上下文塞入 `messages` 数组(而非只用顶层 `system` 字段)。aigw 的 `DefaultAdapter::claude_to_openai_request` 原样透传 `role`,导致输出 messages 出现多条 system。Qwen 系列的 Jinja chat template 强制 system 只能位于 index 0,否则触发 `raise_exception('System message must be at the beginning.')` 400 错误。GPT / DeepSeek / Claude via Bedrock / Kimi / Moonshot / GLM 等主流上游对多 system 宽容,只有 Qwen 类严格模板会拒。
- **Alternatives**:
  - **A. 丢弃**(litellm 现状):`translate_anthropic_messages_to_openai` 只识别 user/assistant,其它 role 静默丢。信息损失,Claude Code 的 agent 清单/skill 描述被吞。
  - **B. 合并到首位单条 system**:保留信息但破坏时序(中段规则被视作一开始就有的规则)。
  - **C. 换角色为 user**:保留信息但语义降级(系统级规则变用户发言)。
  - **D. 折叠进相邻 user turn**(本决策):new-api 社区 PR #5413 的思路;信息不丢、时序保留、模板兼容。
  - **E. 新增 QwenProvider / 子 adapter**:线协议未变(Qwen 走 OpenAI Chat Completions),模型家族与 provider 类型正交;引入会导致 M×N 配置面爆炸。
- **Rationale**:
  - 默认最大兼容性:非严格模板不做任何变换,零回归风险。
  - 严格模板走折叠:`<system-reminder>` 标签是 Claude Code 客户端自身已使用的语义(LLM 已识别),折叠不改变消息数以外的字段,时序保留。
  - 配置放 `proxy_models.model_info.chat_template_compat` JSON 字段:无 schema 变更;取值 `auto` / `strict` / `loose`;`auto` 按 `lower(upstream_model).contains("qwen")` 嗅探。
  - 与 new-api 社区共识对齐:PR #5413 虽被驳回(维护者主张 Provider 责任),但 aigw 作为客户端和 Provider 之间的网关是最合适的归一化点。
- **Implementation**: 详见 `docs/plans/2026-07-16-system-message-normalization.md`。生效范围仅 `AnthropicToOpenAI::adapt_request`;`OpenAIPassthrough` 直通路径不受影响。8 个 UT 覆盖真实 body 复现、多 system 夹杂、末尾 system、无 user 兜底、Loose 对照、嗅探大小写、显式 override。
- **Consequences**:
  - Claude Code v2.1.153+ + Qwen 上游可用性从 0% 恢复到 100%。
  - 其它上游模型行为无回归(Loose 分支完全透传)。
  - 未来 Gemma / Mistral 若发现同类模板问题,嗅探表加一条 `contains(...)` 即可,无需新架构。
  - 引入 `Deployment.chat_template_compat` 派生字段;`resolver` 装配阶段从 `model_info` 读取。
  - 前端 ModelDialog 新增一个下拉,承担用户显式 override 入口。
  - 附带发现:`claude_message_to_openai` 多 tool_result 丢失(`adapter.rs:622-624`)属独立 bug,不在本 ADR 范围。

## ADR-019: Phase 31 完成 — Body Archive 生产化（Stage 82-84）

- **Date**: 2026-07-27
- **Status**: Accepted
- **Decision**: Phase 30（Stage 78-81）编码落地后未通过生产审计，转入 Phase 31（Stage 82-84，3 Stage / 24h）按审计报告逐条修复 P0/P1。Stage 84 删除 602 行单文件巨石 `jobs.tsx`，启用 `pages/jobs/` 目录化结构（index + job-detail + components/trigger-dialog + lib/api/jobs）；路由化 `/dash/jobs/:jobId` 子路由 + `useSearchParams` 驱动 tab/page/status（Q8 URI 直达/刷新/分享/后退）；`STEP_LABELS` 美化 + fallback（Q4 去下划线）；Manual Trigger 按钮挪到 `TabsList` 同行（Q5）；Archive Disabled 联动 `disabled` + tooltip（Q4）；列表表格化 + 分页 `ListPagination`（Q6，后端 `jobs.rs` list response 加 `total`）；详情页独立路由去冗余 + Steps 分页 pageSize=20 + Payload/Result/Duration 列（Q7）；Logs 按 `step_key` 分组折叠（Q2）；矛盾检测 `displayJobStatus`（summary.running>0 → running，Q1）+ completed+rows_archived=0 → 灰色 "completed (no-op)" badge（Q3）+ 错误 toast 替换 silent fail + a11y（tabIndex/onKeyDown/aria-label）。
- **Background**: 2026-07-25 三路 subagent 并行审计确认 Phase 30 前端 8 个用户反馈问题全部成立。前端 602 行 `jobs.tsx` 单文件巨石组件原型级质量，无路由化、无分页、无矛盾检测、tab 含下划线、Disabled 仍可执行。
- **Key learning (TDD 红绿)**: Stage 84 开始时发现 `jobs.feature` 的 BDD spec 从未生成（`.features-gen/` 缺 `jobs.feature.spec.js`），playwright-bdd `bddgen` 在 cucumber 表达式层崩溃：(1) 含空格的 `/`（`GET /admin/jobs step_type`）被当作 alternation，需 `\/` 转义；(2) `{job_id}` 被当作未注册的 custom parameter type，需改用非花括号占位或注册 ParameterType；(3) 跨 step 文件重复定义同一 Given（`API endpoints are mocked` 在 keys + jobs 两处）；(4) step 定义函数参数个数与 `{string}`/`{int}` 占位不匹配。修齐后 11 个 Stage 84 新场景 × 3 viewports = 81/81 全绿。
- **Consequences**:
  - 前端 Jobs 页面达到生产质量，8 个用户反馈问题（Q1-Q8）逐条解决。
  - 删除 602 行单文件巨石，目录化拆分提升可维护性。
  - 前端 BDD 从 108 增至 252 tests（含 jobs 81 = 27 scenarios × 3 viewports），`task fe-bdd` 全绿。
  - real BDD（AIGW_REAL_API=1，分页/trigger 409/冷数据回源）推迟到生产环境验证（mock BDD 已覆盖前端逻辑）。
  - Phase 31（Stage 82-84）全部完成，Phase 30 待一并标记 ✅。

## ADR-020: Phase 32 完成 — request_id → call_id 改名 + 上游对账链路打通（Stage 85）

- **Date**: 2026-07-28
- **Status**: Accepted
- **Decision**: 将 `spend_logs.request_id`（PK，aigw 网关 UUID v7）改名为 `call_id`，并新增可空 `request_id` 列存储上游 provider 返回的请求 ID（Anthropic `msg_xxx` / OpenAI `chatcmpl-xxx`），加索引 `idx_spend_logs_request_id`。任意 SpendLog 都能用上游 `request_id` 与 provider 对账，无论成功还是 4xx/5xx 失败。`daily_tag_spend.request_id` 同步改名 `call_id`。
- **Background**: aigw 把自身 UUID v7 存为 `spend_logs.request_id`，与行业惯例（含 litellm）冲突——行业里 `request_id` 指上游 provider 返回的 ID。导致语义混淆 + 售后对账断链。设计文档 v6.1 经 Gate-2 多模型评审（lead 独立 + 3 路 subagent）定稿。
- **Key learning (Gate-2 评审最重要的发现)**: v5 设计的"扩展 `update_spend_log` + `COALESCE($new, request_id)` UPDATE"方案**对失败路径无效**——所有失败路径（超时 / 4xx-5xx / resolver 失败）和非流式成功路径都是 INSERT-only，没有 Phase 2 UPDATE 可扩展，`COALESCE` 根本不覆盖这些行 → 失败 `request_id` 仍 NULL → v5 核心预期"失败请求也能对账"静默失败。v6.1 改为：失败路径在 `SpendLog` 构造时直接赋 `request_id: fail_upstream_id` 写入 INSERT。`update_spend_log` 的 `upstream_request_id` 参数 + `COALESCE` 仅用于流式 Phase 2 UPDATE。这是 lead 独立核验漏掉、由 Lens C subagent 发现的关键缺陷——评审的价值所在。
- **三处不改边界**（务必区分，否则破坏功能/契约）:
  1. HTTP 中间件层 `tower_http::request_id::*`（main.rs:57, chat.rs:24, v1_messages.rs:27）—— `let request_id = extensions.get(...)` 局部变量名保留，值透传给 `call_id` 字段。
  2. 对外 LLM API 响应体 `request_id` 字段（v1_messages.rs:48/141/165/179/213 的 `anthropic_error`）—— 字段名保留（Anthropic/OpenAI 协议契约），值 = call_id。
  3. aigw-migrate 的 litellm 源/目标表 SQL（native.rs keyset/SELECT/fixture）—— litellm 表 schema 不改。
- **migrate override 方向**（v6 修正 v5 写反）: `column_override` 由 `native.rs::build_row_values` 以 **key=目标列、value=源列** 消费。故 import 注入 `overrides["call_id"]="request_id"`（litellm 源 request_id → aigw PK call_id）；export 因 `insert_rows` direct-match 优先，需**从源行剥离 request_id** 让 reverse override `["request_id"]="call_id"` 生效（v6.1 §11.1）。
- **Consequences**:
  - 对账链路打通：任意 SpendLog 可用上游 `request_id` 点查（走 `idx_spend_logs_request_id`）与 provider 对账，覆盖成功 + 4xx/5xx 失败（连接/超时无 body 留 NULL，不可避免）。
  - 语义清晰：网关调用 ID = `call_id`（PK），上游返回 ID = `request_id`（可空）。
  - 查询参数 `?request_id=` 保留（§6.2 妥协），同时匹配两列（gateway call_id OR upstream request_id）。
  - body_archive 归档过滤加 `request_id IS NOT NULL`（用户决策）：失败请求（无上游 id）跳过 body 归档，省存储。
  - TD-006（客户端无法从响应头拿 call_id 对账）登记为后续跟进。
  - Gate-2 多模型评审显著降低了设计缺陷流入实现的风险（3 Critical + 3 High + 4 Medium 全部在编码前修正）。

## ADR-021: Phase 33 完成 — aigw↔aigw 多表只读增量同步（Stage 86）

- **Date**: 2026-07-28
- **Status**: Accepted
- **Decision**: 新增 `aigw-migrate sync` 子命令，在两个 aigw DB 实例间（PG↔SQLite 任意组合）只读增量同步。默认全 11 张业务表，`--tables` 选子集；`spend_logs` 按 `--days`/`--resume-after`/`--end-before` 增量，其他表全量幂等追加；重跑不重复（`INSERT OR IGNORE`/`ON CONFLICT DO NOTHING`）。空 overrides direct-match，不做 litellm 的 `call_id←request_id` 列重定向。
- **Background**: 用户诉求——在 aigw 内部不同 DB 实例间同步数据，参数范式参考 `remote-import`/`remote-export`，只读一次性 CLI。现有 `aigw-migrate` 是 litellm↔aigw **异构**迁移（绑死 litellm 表名/camelCase/`call_id←request_id` 重定向 + 密钥轮转），覆盖不了 aigw↔aigw **同构**同步。
- **Key decisions**:
  - **新增 `build_aigw_cursor_sql` 而非改 `build_cursor_sql`**：aigw 锚点列是 `start_time`（litellm 是 camelCase `startTime`）。不改原函数保 litellm 迁移零回归。PG keyset 用 `(start_time, call_id)` 而非 `(startTime, request_id)`。
  - **空 overrides direct-match**：aigw↔aigw 同 schema（同表名/同 snake_case/同 PK `call_id`），不需要 litellm 的列重定向。复用 `SourcePool`/`CursorRange`/`insert_rows_batch`/`migrate_plain_table` 底层抽象，不碰 `remote_import`/`remote_export` 的 litellm-mapping 路径。
  - **加密表直接复制密文**：`credentials`/`proxy_models` 当 plain 表处理，不调 `migrate_credentials`/`migrate_proxy_models`（它们做密钥轮转）。假设两端共享同一 `master_key`（同 aigw 集群内）。跨 key 场景仍用 `remote-import`。
  - **config 默认排除**：`config` 含 `master_key`，默认不同步避免覆盖目标鉴权；显式 `--tables config` 才同步，走 `INSERT OR IGNORE` 只补齐缺失行不覆盖已有 master_key。
  - **`--days` 用 UTC**：`start_time` 存 UTC，避免本地时区跨天错位。与显式 `--resume-after`/`--end-before` 叠加时取更严边界（max resume_after, min end_before），不报错。
  - **只读追加边界**：仅 INSERT，不传播 UPDATE/DELETE；非常驻、非 CDC。符合"只读镜像"诉求。
- **Consequences**:
  - aigw 集群内多实例数据同步有了一条命令路径，参数范式与 `remote-import` 一致降低学习成本。
  - litellm↔aigw 迁移路径完全不受影响（零回归，`build_cursor_sql`/`stream_pg_rows_keyset` 原样）。
  - 跨 master_key 同步加密表仍需 `remote-import`（明确边界，不混淆）。
  - TDD 8 UT 覆盖核心预期（全表/子集/`--days`/幂等/`--skip-body`/非法表名/config 默认排除+显式不覆盖/DEFAULT_TABLES 契约）；PG 跨方言覆盖复用 `bdd-real-*` testcontainers，不阻塞 UT。
## ADR-023: Phase 37 规划 — Budget Reset 周期任务 + 配置（Stage 91-93）

- **Date**: 2026-07-30
- **Status**: Planned（Phase 37 待实施）
- **Decision**: 复用 Stage 82-84 的 AsyncTask+Engine 框架新增 BudgetResetter（step_type=budget_reset），实现 budget_duration 周期性重置 spend。3 Stage / 40h：91 后端（duration 解析 + resetter + Budget CRUD + backfill + config）、92 前端（实体表单内联 budget_duration/soft_budget + Jobs Tab 补全）、93 全栈联调（soft/hard 双轨 + real BDD 三后端）。
- **Background**: budgets 表 + 四实体表 budget 列 Stage 1 就 schema 对齐 litellm 但从未实现周期 reset。budget_duration/budget_reset_at 字段被写入却不消费，spend 累积到上限永久超额。Body Archive 已建成 AsyncTask+Engine + async_jobs 表 + 前端 Jobs 页，KNOWN_STEP_TYPES 已硬编码 budget_reset 但后端无实现、前端 Tab 占位。
- **Key decisions**:
  - **复用 AsyncTask+Engine 而非新建 scheduler**：零新增框架成本，天然多副本安全（claim_next_step SKIP LOCKED），前端 Jobs 页天然展示，与 charter Stage 3 一致。tick_interval=60s。
  - **标准化对齐而非 now()+duration**：24h→UTC 0 点 / 7d→周一 0 点 / 30d/1mo→月初 1 号 0 点 / Nh/Nm/Ns→N 边界。用 chrono+chrono-tz。对齐 litellm get_next_standardized_reset_time，重置时刻可预期。
  - **过期判断下沉 SQL**：WHERE budget_reset_at < now() OR (budget_reset_at IS NULL AND budget_duration IS NOT NULL)。NULL 保护子句防「有 duration 无 reset_at」行永久累积。
  - **实体表单内联而非独立 Budgets 页**：用户选定，对齐 litellm 主流 UX。
  - **reset 只 UPDATE spend=0 + reset_at 重算，不改 spend_logs**：审计流水完整保留。
  - **aigw 无独立 Redis counter**：spend 列是 DB 真值，reset 后 BudgetEnforcer 直接读 DB 新值，无 litellm 的 counter 失效缺口（简化版优势）。
  - **soft_budget 告警留 TD-007**：本 Phase 只 hard reset + max_budget 检查 + soft 记日志，告警通道后续做。
  - **启动期 backfill**：防「有 duration 无 reset_at」行永久累积，对齐 litellm 启动行为。
  - **NaN 防御**：max_budget 解析加 f64::is_finite，对齐 litellm 安全公告 GHSA-2rv4-xv66-fpjg。
- **Consequences**:
  - 配 budget_duration 的 key/team/user/org 到周期点自动清零 spend，支持 Codex/Claude Code 周期配额场景，兑现 charter Stage 3 承诺。
  - Body Archive 的 AsyncTask+Engine 框架得到第二个使用者，验证其通用性。
  - soft_budget 告警需后续接通知通道（TD-007）。
  - TDD：~18 UT + 6 mock BDD + real BDD 三后端 + 前端 8 BDD × 3 viewports。

---

## ADR-023: UI 多语言 i18n 选型与架构

- **Date**: 2026-08-01
- **Status**: Implemented（Phase 38，Stage 91-93）
- **Decision**: 使用 i18next + react-i18next + i18next-browser-languagedetector 实现前端中英双语支持。框架同步初始化（零闪烁），语言检测链 localStorage `aigw-language` → `navigator.language` → hardcoded `'en'` fallback。翻译文件采用 single-JSON per locale 按命名空间组织（common/sidebar/header/login/usage/keys/models/users/orgs/teams/spendLogs/playground/routerSettings/jobs/health/logViewer/pagination/auth），共 ~250 keys。
- **Background**: 当前 aigw 前端所有 UI 文本硬编码为英文，无任何 i18n 框架、翻译文件或语言切换机制。litellm 也无多语言支持，本次是 net-new 能力。
- **Key decisions**:
  - **选 i18next 非 FormatJS**：React 生态事实标准，Tailwind/shadcn 项目常用，社区成熟度高。
  - **单 JSON 文件命名空间**：初期文本量 < 500 keys，打包成本忽略不计，懒加载未必要。
  - **JSON key 用英文 camelCase**：可读性好、IDE 补全、类型安全。
  - **同步初始化不阻塞渲染**：零闪烁；语言检测在 <1ms 内完成。
  - **管理员配置默认语言推迟**：首次访问通过 `navigator.language` 自动检测已覆盖 95%+ 场景。
  - **通用 UI 组件不改**：`components/ui/*` 保持纯净，文案由调用方传入。
  - **zod 校验在 render 时翻译**：语言切换需动态响应。
- **Consequences**:
  - 3 Stage 交付（91 框架 12h + 92 翻译 20h + 93 切换器 10h），共 42h。纯前端 Phase，零后端变更。
  - 翻译文件懒加载、TypeScript 类型安全、后端 API 错误消息多语言、RTL 支持登记为 TD-008 a-d（P3）。
  - 语言切换器位于 Header 右侧，使用 DropdownMenu + Lucide Languages 图标。
  - `<html lang>` 属性随语言切换自动同步。

## ADR-024: Budget Reset 架构与多层级配额约束

- **Date**: 2026-08-01
- **Status**: Approved（Phase 39，Stage 94-97）
- **Decision**: Budget Reset 分为 4 个 Stage 交付（56h）。核心决策：
  (1) entity spend 用 `tokio::spawn` 异步 + 事务批量更新，非同步非 channel 队列；
  (2) 标准化对齐（UTC 0 点/周一/月初），非 now()+duration；
  (3) 多层级独立检查——每层有自己的 spend 列独立与 max_budget 比较，非 Key 累加到上级；
  (4) 配额层级约束在写入时校验（`Key.max_budget ≤ User.max_budget ≤ Team.max_budget ≤ Org.max_budget`），非请求时才报错；
  (5) org 的配额从 budgets 表取，key/team/user 内联。
- **Background**: aigw 的实体 spend 列从未更新（预算检查永远不触发），daily_spend 6 维度只写了 User，周期 reset 未实现，多层级检查缺失，且无层级约束导致可以配出"Key $100 挂 Team $50"的矛盾数据。
- **Key decisions**:
  - **异步事务而非同步**：`tokio::spawn` 消除请求路径延迟（spend_logs 返回后即响应），事务保证 key/user/team/org 一起更新或一起失败，透支窗口 ms 级可忽略。
  - **标准化对齐而非 linear-add**：24h→UTC 0 点、7d→周一 0 点、30d→月初 1 号。重置时刻可预期可记忆，对齐 litellm。
  - **写入时约束而非请求时报错**：在 POST/PUT 端点中校验 `child.max_budget ≤ parent.max_budget`。NULL 上级 = 无限，不约束。比用户请求被拒时才知道"上级超限"更友好。
  - **多级检查集中在 Stage 97**：先让 Stage 94 打通 spend 写入（基础设施），Stage 95 做 reset + 写入约束（杜绝矛盾数据），Stage 97 接上多级 BudgetEnforcer。
  - **NaN 防御**：`f64::is_finite()` 守卫，对齐 litellm 安全公告 GHSA-2rv4-xv66-fpjg（IEEE 754 NaN 所有比较返回 false 导致预算检查静默失效）。
- **Consequences**:
  - Phase 39 从原 3 Stage/40h → 4 Stage/56h（新增 Stage 94 spend 写入基础 + Stage 95 并入层级约束）。
  - BudgetEnforcer 从单层 → 4 层逐级检查（Stage 97）。
  - 架构文档（`docs/research/2026-08-01-budget-reset-architecture.md`）统一记录所有决策。
  - soft_budget 告警（Slack/Email）推迟到 TD-007，budget_limits 多窗口推迟到后续。

### Update 2026-08-04: Stage 97 完成 — Multi-level BudgetEnforcer + soft_budget + 全栈联调

- **Multi-level enforcement**: `BudgetEnforcer::check_budget_multi()` 逐级检查 key → user → team → organization，任一超限即 403（HTTP 429），`entity_type` 字段标识被拒层级。中间实体缺失时静默跳过 + `tracing::warn!`（对齐 litellm）。
- **soft_budget 双轨日志**: `check_soft_budget()` 根据 entity_type、entity_id、spent、soft_budget 发 `tracing::warn!` 但不拒绝请求。生产可接入 tracing subscriber（OpenTelemetry / log aggregator）消费。外部通知通道（Slack/Email/Webhook）留 TD-007。
- **历史用量聚合**: `Database::get_spend_by_team()` / `get_spend_by_org()` 从 `spend_logs` SUM spend，三云方言实现（SQLite/PG/MySQL）。
- **TOCTOU 策略**: spend 在请求完成后异步更新（Stage 94），budget 检查在下一请求读已更新的 spend。并发窗口 ~ms，分布式系统可接受 trade-off。
- **测试覆盖**: 23 UT（budget.rs）+ mock BDD 177 pass（回归无退化）。Real BDD 场景 `multi_level_budget.feature` 已创建（@skip 等待 real BDD runner 环境就绪后解禁）。

## ADR-025: Playground 多模态图片能力（Phase 42，Stage 103-105）

- **Date**: 2026-08-07
- **Status**: Approved（Phase 42，Stage 103-105）
- **Decision**: Playground 支持图片输入与多模态模型（qwen3.5-vl 等）识别，3 Stage 交付共 34.5h。核心决策：
  (1) 前端始终用 OpenAI content-parts（chat 端点）或 Claude content blocks（messages 端点）携带图片，由 `endpointType` 决定，图片在客户端已读为 base64 data URL，网关透传；
  (2) 后端只修最小缺口——`openai_message_to_claude` 的 data URL 解析 bug + `/v1/models` 暴露 `model_info.mode`；
  (3) log-viewer 共享 `extractImages` + `ImageThumbnails` 组件，Playground 与 SpendLog 详情复用，SpendLog drawer 不改结构；
  (4) 不按 `model_info.mode` 强制过滤附件（用户可自由给任意模型发图，由上游裁决）。
- **Background**: 用户诉求——Playground 给多模态模型发图片。代码审计确认后端多模态转换部分就绪（`claude_message_to_openai` L1256/L1295 已正确生成 `data:{media_type};base64,{data}`），但 `openai_message_to_claude` L1113-1115 硬编码 `media_type:"image/jpeg"` 且把完整 data URL 塞入 `ClaudeImageSource.data`（Anthropic 上游要求纯 base64 + 正确 media_type，发送 PNG 会 400）。前端 Playground 仅纯文本（`ChatMessage.content: string`），src/ 无 file input/FileReader 先例。SpendLog 详情 log-viewer 缺 `output_text`（Responses API 引入）与图片 block 渲染。
- **Key decisions**:
  - **前端 base64 直传而非后端子图**：图片在浏览器读为 data URL，随消息 JSON 直接发网关；无独立上传端点、无对象存储，对齐 litellm 多模态（`image_url.url` 即 data URL）。
  - **修 `openai_message_to_claude` 为最小必要**：`parse_data_url` helper 剥离 `data:` 前缀推导 media_type（malformed fallback `image/png`），仅一处调用；`claude_message_to_openai` 已验证正确无需改。
  - **`/v1/models` 暴露 `model_info` 而非新建字段**：`ModelEntry.model_info: Option<Value>`（`skip_serializing_if`），master 路径透传 `ProxyModel.model_info`（含 mode），非 master 路径缺省——litellm 兼容（litellm /v1/models 同样返回 model_info），零回归。
  - **共享 log-viewer 组件**：`extractImages(content)` 递归提取 OpenAI `image_url` / Anthropic `image` block 的 data URL，`ImageThumbnails` 渲染；`extractText`/`parseOutput`/`extractTextContent` 补 `output_text`/`input_text`/`file` 分支——SpendLog 详情 drawer 经现有 hasPrompt/hasResponse 自然透传。
  - **不强制模式过滤**：不按 `model_info.mode` 禁用上传按钮；litellm 亦无此 gate，Playground 保持自由发图、上游裁决。
- **Consequences**:
  - Phase 42 三 Stage：Stage 103 后端修复+BDD（6.5h）、Stage 104 Playground 图片输入（16h）、Stage 105 渲染+文档（12h）。
  - 多模态请求进 SpendLog `messages`/`response` JSONB 原样（含 base64），body_archive 按既有 `messages` 体积分片，超大图 body limit（32 MiB）与压缩留 TD-009。
  - qwen 系列聊天模板已在 Stage 60 嗅探（`chat_template_compat: strict`），图片 content-parts 不经 fold，无模板冲突。
