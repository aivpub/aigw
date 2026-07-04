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
