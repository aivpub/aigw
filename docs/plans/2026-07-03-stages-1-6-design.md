# Stages 1-6 Implementation Design

**Project**: aigw (AI Gateway)
**Date**: 2026-07-03
**Branch**: feature/stages-1-6

---

## Architecture Overview

Multi-database support (SQLite/MySQL/PostgreSQL) via sqlx enum dispatch + testcontainers for integration testing.

### Database Support

| Database | Feature Flag | Pool Type | Usage |
|----------|-------------|-----------|-------|
| SQLite | `sqlx/sqlite` | `SqlitePool` | Dev, single-instance on-prem |
| MySQL | `sqlx/mysql` | `MySqlPool` | Shared hosting |
| PostgreSQL | `sqlx/postgres` | `PgPool` | Production SaaS |

### Runtime Dispatch

```rust
enum Database {
    Sqlite(SqlitePool),
    Mysql(MySqlPool),
    Postgres(PgPool),
}
```

Pool selected by `DATABASE_URL` prefix at startup.

### Migration Strategy

```
crates/aigw-core/migrations/
├── sqlite/
├── mysql/
└── postgres/
```

Per-database SQL, loaded by sqlx::migrate::Migrator at runtime.

### Integration Testing

testcontainers-rs spins up real Postgres/MySQL containers for integration tests.

---

## Stage 1: Schema 100% Alignment + Migration Tool

### Deliverables

1. **Migrations Agent**: 11 tables x 3 databases = 33 SQL files + indexes + migration runner
2. **aigw-migrate Agent**: New crate with import/export/verify subcommands + column mapping
3. **DB Core Agent**: Database enum, pool init, main.rs wiring, unit + integration tests

### 11 Tables

virtual_keys, spend_logs, organizations, teams, users, projects, budgets,
organization_memberships, team_memberships, deprecated_keys, deleted_keys

### Indexes

- spend_logs: (start_time), (api_key), (user_id, team_id)
- virtual_keys: (budget_reset_at, expires), (token)
- spend_logs: (session_id)

---

## Stage 2: Key API + SpendLog Endpoints

### Deliverables

1. **Key API Agent**: /key/generate, /key/info, /key/update, /key/delete, /key/list, /key/regenerate
2. **Spend API Agent**: /spend/logs, /spend/keys, /spend/users, /spend/tags, /global/spend/*
3. **Auth Agent**: Complete middleware with DB-backed key lookup, SHA256 hashing

---

## Stage 3: Chat Completions + Claude Code/Codex Compat

### Deliverables

1. **Chat Completions Agent**: /v1/chat/completions (non-stream + SSE stream), /v1/models
2. **Rate Limit Agent**: RPM/TPM tracking, budget reset, cooldown, max_parallel_requests
3. **Integration Agent**: Router integration, error propagation matching litellm

---

## Stage 4: OpenAPI 3.1 + Frontend Planning

### Deliverables

1. **OpenAPI Agent**: utoipa annotations, /docs endpoint with Swagger UI
2. **Frontend Planning Agent**: Tech selection doc, feature planning
3. **Schema Agent**: Request/response validation

---

## Stage 5: Docker + Deployment

### Deliverables

1. **Docker Agent**: Multi-stage Dockerfile, Docker Compose
2. **Health Agent**: /health, /health/readiness, /health/liveness endpoints
3. **Deploy Agent**: Deployment docs, graceful shutdown, config hot-reload

---

## Stage 6: SaaS Architecture

### Deliverables

1. **Gateway Agent**: NGINX/Kong auth request integration design
2. **Multi-Instance Agent**: Multi-instance config, data isolation strategy
3. **Billing Agent**: Usage-based billing data export interface

---

## Test Strategy

| Layer | Tool | When |
|-------|------|------|
| Unit | `cargo test` (SQLite in-memory) | Every build |
| Integration | testcontainers (Postgres + MySQL) | `cargo test --features integration` |
| E2E | aigw-migrate round-trip | Stage 1 only |

## Gate Checks Per Stage

- `cargo build --workspace` — zero errors
- `cargo test --workspace` — all tests pass
- `cargo clippy --workspace` — no warnings (allow clippy::style)
- `task test` — unified entry works
- `task doctor` — health check passes

## Taskfile Requirements

```yaml
tasks:
  doctor: cargo check --workspace && cargo clippy --workspace -- -D warnings
  test: cargo test --workspace && cargo test --workspace --features integration
  build: cargo build --workspace --release
  fmt: cargo fmt --all -- --check
```
