# Stage 1: Schema Alignment + Migration Tool

**Status**: Complete (2026-07-03)
**Phase**: Phase 1 -- Data Compatibility

## Goal

100% schema alignment with litellm (11 tables) across SQLite, MySQL, and PostgreSQL.

## Deliverables

- 11 table schemas in `crates/aigw-core/src/models.rs`
- SQLite/MySQL/PostgreSQL migrations in `crates/aigw-core/migrations/`
- `aigw-migrate` binary: import/export/verify commands
- Database layer in `crates/aigw-core/src/db.rs`

## Verification

- 25 unit tests in db.rs (CRUD operations for all 11 tables)
- 2 integration tests (PostgreSQL + MySQL via testcontainers)
- `task test` passes
