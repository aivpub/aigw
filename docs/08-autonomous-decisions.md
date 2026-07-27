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
