# Stage 85 Reviewer Lens B — Migrate Override + Frontend + Tracing + Tests

Review the DESIGN (not code) for aigw Stage 85. Read first:
- `docs/research/2026-07-27-stage-85-design-review-brief.md` (brief)
- `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` (design v5)
- `docs/stages/stage-85.md` (stage)

## Your lens: migrate override correctness + frontend + tracing/observability + test coverage

### 1. §4.5 migrate override (most critical — silent data corruption)
Design claims: `remote_import.rs:546` `build_snake_overrides` produces an overrides map; we inject `overrides.insert("request_id" → "call_id")` so litellm's source `request_id` writes to aigw's `call_id` PK (not the new upstream `request_id` column).

- Read `crates/aigw-migrate/src/remote_import.rs` around `migrate_spend_logs` + `build_snake_overrides` + the `insert_rows_batch` call. Confirm:
  - (a) source (litellm `LiteLLM_SpendLogs`) has ONE `request_id` column,
  - (b) target (aigw `spend_logs`) after 022 has BOTH `call_id` (PK NOT NULL) + `request_id` (nullable upstream),
  - (c) without the override, column-name-based mapping would write source `request_id` → target `request_id` (upstream col), leaving target `call_id` (PK) NULL → INSERT fails.
- Read `crates/aigw-migrate/src/native.rs` `build_row_values` to confirm value lookup is by source column name through the override map (so the override actually redirects the value).
- Confirm the exact injection point (design says `:546` after `let overrides = build_snake_overrides(...)`).

### 2. §4.5 reverse override (export)
- Read `crates/aigw-migrate/src/remote_export.rs`. Find the export path for spend_logs → litellm. Confirm a reverse override is needed (aigw `call_id` → litellm `request_id`) and locate the injection point (design cites `:367-394`).

### 3. §4.5 test fixtures
- Verify aigw-side fixtures that must rename: `remote_import.rs:1224`, `remote_export.rs:547` (CREATE TABLE spend_logs).
- Verify litellm-side fixtures that must NOT rename: `remote_import.rs:865/1023/1041/1200/1211`, `remote_export.rs:573/574`, `native.rs` keyset/SELECT.
- Report any fixture the design mislabeled.

### 4. §5 frontend
- Read `crates/aigw-frontend/src/pages/spend-logs/index.tsx` and `dashboard/index.tsx`. Confirm:
  - 3 independent `SpendLog` interfaces exist (not a shared type),
  - 5 state variables around request_id,
  - CSV headers, list columns, detail drawer, search placeholder, queryKey, detail endpoint URL.
- Assess: does the design's "~16 + 3 changes" match reality? Any frontend field the design missed?

### 5. §10 tracing
- Read `crates/aigw-server/src/main.rs:114-130` `RequestIdMakeSpan`. Confirm span field `request_id = %request_id`. Design option A renames the span FIELD to `call_id` (variable name stays). Is there a downstream log-collector rule or OpenSearch dashboard that filters by `request_id`? Check for any `logfmt`/`json` parser config in repo that would break — if none, option A is safe.

### 6. Test coverage completeness
- Design §4.6 lists 10 BDD + 5 non-BDD unit test files. Cross-check against `crates/aigw-server/tests/` (BDD features + steps) and `crates/aigw-core/tests/` + inline `#[cfg(test)]`. Any `request_id`-bearing test file the design missed? Use `grep -rl "request_id" crates/aigw-server/tests/ crates/aigw-core/tests/ crates/aigw-core/src/ crates/aigw-server/src/ crates/aigw-migrate/src/ crates/aigw-frontend/` and reconcile against the design's list.

## Output
Report ONLY design-level defects that would cause implementation failure or contract breaks. For each: `file:line` anchor, concrete failure scenario, severity (Critical/High/Medium/Low), fix. If sound, say so with evidence. No stylistic nits.
