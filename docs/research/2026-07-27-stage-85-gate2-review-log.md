# Stage 85 Design Review Log (Gate 2)

**Date**: 2026-07-27
**Design**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` v5 → v6 → v6.1
**Stage**: `docs/stages/stage-85.md`
**Method**: Multi-model review — lead independent verification + 3 parallel subagents
- Lens A: migration safety / DB schema
- Lens B: migrate-override / frontend / tracing / tests
- Lens C: upstream-ID-extraction / protocol-boundary

## Findings applied to v6 (first patch)

| # | Finding | Severity | Source | Status |
|---|---------|----------|--------|--------|
| 1 | Migration `022` already taken by Stage 82 (`022_next_retry_at.sql`) → renumber to `023` | Critical | Lead + Lens A | ✅ v6 |
| 2 | Migrate IMPORT override direction inverted (`request_id→call_id` should be `call_id→request_id`) | Critical | Lead + Lens B | ✅ v6 |
| 6 | Test list misses `body_archive_read.feature` + `stage82_state_machine.rs` + `stage83_read_path.rs` | High | Lead + Lens B | ✅ v6 |
| (sqlite) | SQLite idempotency claim (PRAGMA probing) aspirational; sqlx version table is the real mechanism | Low | Lead + Lens A | ✅ v6 |

## Findings applied to v6.1 (Gate-2 second patch — core expectation + data integrity)

| # | Finding | Severity | Source | Status |
|---|---------|----------|--------|--------|
| 3 | Migrate EXPORT override preempted by direct-match → NULL writes to litellm PK; strip `request_id` from source rows | High | Lens B (lead missed) | ✅ v6.1 §11.1 |
| 4 | Failure-path upstream_id goes through INSERT not UPDATE — `COALESCE` UPDATE doesn't cover failure rows → core expectation silently fails | **Critical** | Lens C (lead missed) | ✅ v6.1 §11.2 |
| 5 | Anthropic streaming extraction must be BEFORE `if choices` branch + borrow (push moves `raw`); Anthropic-native `message_start` has no `choices` | High | Lens C | ✅ v6.1 §11.3 |
| 7 | Anthropic failure response headers consumed before extraction; pre-extract `request-id` alongside `x-request-id` | Medium | Lens C | ✅ v6.1 §11.4 |
| 8 | Dual-column search bind mechanics per-impl (Sqlite filtered + 3 counts = SQL-level dual-bind; MySQL/Postgres filtered = in-memory dual-check; Postgres count placeholder-counter hazard) | Medium | Lens C | ✅ v6.1 §11.5 |
| 9 | Migrate NULL semantics — litellm `request_id` IS the upstream id; historical rows get both `call_id`+`request_id` = litellm `request_id` (semantically correct, not a defect) | Medium | Lens B | ✅ v6.1 §11.6 |

## Factual line-number corrections (Low — recorded for implementer)

| # | Finding | Source | Status |
|---|---------|--------|--------|
| 10 | `remote_import.rs` aigw `spend_logs` fixture at `:1243-1260` (not `:1224`, which is litellm fixture) | Lens B | ✅ v6.1 §11.7 |
| 11 | chat.rs non-streaming success SpendLog at `:1536-1582` (not `:1184`, which is streaming Phase 1) | Lens C | ✅ v6.1 §11.7 |
| 12 | chat.rs response bodies have NO `request_id` field (only `v1_messages.rs::anthropic_error` injects it) | Lens C | ✅ v6.1 §11.7 |
| 13 | `update_spend_log` has 5 locations (trait + 3 impls + dispatch), not "3" | Lens C | ✅ v6.1 §11.7 |
| (mysql) | `RENAME COLUMN` is a new pattern in this repo (018 uses DROP+rebuild); target 8.4 supports; README should state min MySQL 8.0/MariaDB 10.5.2 | Lens A | ✅ v6.1 §11.7 |
| (parquet) | parquet reader uses projection-mask by name + positional `batch.column(N)`; reader compat must try `call_id` then fallback `request_id` | Lens A | ✅ v6.1 §11.7 |

## Verified sound (no defects)

- §10 tracing option A: no log-collector config in repo depends on `request_id` field name → safe to rename span field to `call_id` (Lens B)
- §5 frontend: 3 independent interfaces + 5 state vars; "~16 + 3" accurate (Lens B)
- §4.5 litellm-side fixtures correctly identified don't-rename (Lens B)
- OpenAI non-streaming `id` extraction at `chat.rs:1536`/`v1_messages.rs:960` correct (Lens C)
- OpenAI streaming single-loop + `chunk_jsons.push` at `chat.rs:1262` confirmed (Lens C)
- Anthropic streaming `message_start.message.id` shape confirmed (Lens C)
- OpenAI failure `error_body` String + headers accessible (Lens C)
- §6.3 protocol boundary lines all response-body/test-assertion (Lens C)
- `models.rs:118` SpendLog.request_id + `:185` Tag variant (Lead)
- MySQL 8.4 target supports `RENAME COLUMN` (Lead + Lens A)
- SQLite idempotency via sqlx version table (Lead + Lens A)
- `daily_spend_queue.rs:196` UNIQUE col string anchor valid (Lead)
- `openapi.rs:255` anchor valid (Lead)
- `spend.rs` URL + JSON anchors valid (Lead)

## AI pre-filter note
~50% of AI findings are typically false positives. Each finding above was independently verified against the actual codebase before folding into v6/v6.1. Lens A's "MySQL RENAME COLUMN version risk" was downgraded to a docs note (target 8.4 supports it). Lens C's "§6.3 chat.rs response body request_id" was confirmed but had zero implementation impact (don't-change boundary).

## Gate 2 decision
**Design amended v5 → v6 → v6.1**. All Critical/High findings applied. Medium findings applied. Low findings recorded for implementer. Design is now implementation-ready.

## Artifacts
- Brief: `docs/research/2026-07-27-stage-85-design-review-brief.md`
- Lead findings: `docs/research/2026-07-27-stage-85-design-review-lead.md`
- Consolidated: `docs/research/2026-07-27-stage-85-design-review-consolidated.md`
- Lens assignments: `docs/research/2026-07-27-stage-85-lens-{a-migration,b-migrate-frontend,c-extraction}.md`
- v6 patch script: `hack/stage85-design-v6-patch.py`
