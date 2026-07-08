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

## ADR-008: Complete Core Stages (0-30) — Defer Production Advanced Features

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
