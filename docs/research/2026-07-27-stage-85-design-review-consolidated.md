# Stage 85 Design Review — Consolidated Findings (Gate 2)

**Date**: 2026-07-27
**Reviewers**: Lead (independent) + 3 subagents (Lens A migration, Lens B migrate+frontend+tracing+tests, Lens C extraction+protocol)
**Design**: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` v5 → amended to v6

## AI pre-filter note
~50% of AI review findings are typically false positives. Each finding below was independently verified against the actual codebase by the lead before folding into v6. Findings marked ✅ applied to v6; ❌ rejected with reason.

---

## CRITICAL (blocks implementation / silent data corruption)

### 1. Migration number 022 already taken → must be 023
- **Source**: Lead (independent), confirmed by codebase
- **Evidence**: `crates/aigw-core/migrations/{sqlite,postgres,mysql}/022_next_retry_at.sql` exists (Stage 82, 2026-07-27). Runner `db.rs:153` uses `sqlx::migrate!` which enforces unique version per file → two `022_*` files = hard startup error on every DB.
- **Fix**: Renumber to `023_rename_request_id_to_call_id.sql` (3 files).
- **Status**: ✅ Applied to v6 (design + stage-85.md).

### 2. Migrate IMPORT override direction INVERTED
- **Source**: Lead + Lens B (B-C1, agrees)
- **Evidence**: `native.rs:1281-1304 build_row_values` + `:1228-1233 insert_rows` consume `column_override` as `key=TARGET col, value=SOURCE col`. Design said `overrides.insert("request_id" → "call_id")` (key=source) — wrong; would leave target `call_id` PK NULL → INSERT fails.
- **Fix**: `overrides.insert("call_id".to_string(), "request_id".to_string())` at `remote_import.rs:566` after `build_snake_overrides`.
- **Status**: ✅ Applied to v6.

### 3. Migrate EXPORT override preempted by direct match (lead MISSED, Lens B B-C2 found)
- **Source**: Lens B
- **Evidence**: `insert_rows` (`native.rs:1222-1236`) tries `row_map.get(target_col)` (direct) BEFORE `column_override.get(target_col)` (fallback). For export (aigw→litellm): aigw source has `call_id`+`request_id`; litellm target has `request_id`. Direct match `row_map["request_id"]` finds aigw's upstream `request_id` (NULL for historical rows migrated from litellm per §3.5) → writes NULL to litellm's `request_id` PK. The reverse override `["request_id"]="call_id"` is never reached.
- **Impact**: Silent data loss — litellm PK becomes NULL on export of historical rows.
- **Fix**: Strip `request_id` from each aigw source row before `insert_rows` (so direct match fails and override kicks in), OR change `insert_rows` to prefer override. Option (a) is less invasive. Apply at `remote_export.rs:384` area.
- **Status**: ❌ NOT yet applied — must add to v6 §4.5 export path before implementing.

### 4. Failure-path upstream_id NEVER written (lead MISSED, Lens C C1 found) — CORE EXPECTATION BREAKER
- **Source**: Lens C
- **Evidence**: Design §4.3 v5 says "失败路径 ×3 ... 均需补 upstream_id 参数" treating them as `update_spend_log` call sites. But all 3 failure paths only call `insert_spend_log(&sl)`:
  - `chat.rs:1047` (timeout), `chat.rs:1148` (stream 4xx/5xx), `chat.rs:1518` (non-stream 4xx/5xx)
  - `v1_messages.rs:421,609,705` — all `insert_spend_log`
  The `COALESCE($new, request_id)` UPDATE protection does NOT cover failure rows → failure `request_id` stays NULL → **v5 core expectation "失败请求也能对账" silently fails**.
- **Fix**: Failure paths put `upstream_id` into `SpendLog.request_id` at the INSERT site: `SpendLog { call_id: fail_request_id, request_id: fail_upstream_id, ... }`. No `update_spend_log` added to failure paths. `COALESCE` only for streaming Phase 2 UPDATE.
- **Status**: ❌ NOT yet applied — must rewrite v6 §4.3 failure-path section + §7 step 7.

### 5. Anthropic streaming `upstream_id` extraction placement (Lens C C2)
- **Source**: Lens C
- **Evidence**: `v1_messages.rs:814` only pushes to `chunk_jsons` when `raw.get("choices")` is non-empty. Anthropic-native SSE events (`message_start`) have NO `choices` → push never fires → extraction "in the same loop body as push" never runs. Also `raw` is MOVED into `push(raw)` → extraction after the `if` is use-after-move.
- **Fix**: Place extraction immediately after `serde_json::from_str::<Value>(data)` succeeds, BEFORE the `if choices` branch, using `raw.get("message")` (borrow). Same caution for chat.rs (though OpenAI chunks always have choices, lower hazard).
- **Status**: ❌ NOT yet applied — must fix v6 §4.3 streaming pseudocode.

---

## HIGH

### 6. Test list misses 3 post-design files (Lead + Lens B H1)
- **Files**: `body_archive_read.feature` (6), `stage82_state_machine.rs` (1, `:569`), `stage83_read_path.rs` (3, `:31/196/205`)
- **Impact**: compile failure after rename.
- **Status**: ✅ Applied to v6 (design §4.6).

### 7. Anthropic failure-path response headers consumed before extraction (Lens C C4)
- **Evidence**: `v1_messages.rs:713,949` `upstream_resp.text().await` consumes `upstream_resp` → `upstream_resp.headers()` inaccessible at failure SpendLog construction. Pre-extracted `upstream_req_id` (`:624-628`) only has `x-request-id`, not Anthropic's `request-id` header.
- **Fix**: Pre-extract `request-id` header alongside `x-request-id` at `:624-628` before `.text().await`, OR drop `request-id` from fallback and rely on `x-request-id` + error-body `request_id`. Option (a) recommended (Anthropic's official header is `request-id`).
- **Status**: ❌ NOT yet applied — add to v6 §4.3 failure-path.

---

## MEDIUM

### 8. Dual-column search bind mechanics unspecified (Lens C C5)
- **Evidence**: `db.rs:1482-1551` (Sqlite filtered+count), `:1826-1851` (Mysql count), `:2107-2132` (Postgres count) build dynamic SQL. Changing `AND request_id = ?` → `AND (call_id = ? OR request_id = ?)` needs binding the value TWICE; Postgres count uses a counter `i` that must increment twice for one logical term. MySQL/Postgres FILTERED impls (`:1814,2095`) filter IN-MEMORY (`log.request_id != rid`), not SQL — fix is closure `log.call_id != rid && log.request_id != rid`.
- **Fix**: v6 §4.2 specify per-impl: Sqlite filtered + 3 counts = SQL-level dual-bind (mind placeholder count); MySQL/Postgres filtered = in-memory dual-check.
- **Status**: ❌ NOT yet applied.

### 9. "Migrate upstream request_id stays NULL" unachievable + rationale questionable (Lens B M1)
- With corrected import override, target `call_id` gets litellm `request_id` AND target `request_id` (upstream) ALSO gets litellm `request_id` via direct match (litellm `request_id` → aigw `request_id` direct name match). Can't suppress direct match without stripping. Also litellm's `request_id` IS the upstream id (§1.2), so writing it to aigw upstream `request_id` is semantically correct, not wrong — §3.5 rationale ("历史从未存上游 id") is itself flawed.
- **Fix**: Accept that historical rows get both `call_id` and `request_id` = litellm's `request_id` (semantically fine); or strip to force NULL. Clarify intent in v6.
- **Status**: ❌ NOT yet applied — needs design decision.

---

## LOW (factual, non-blocking — recorded for implementer)

### 10. Fixture line `remote_import.rs:1224` mislabeled (Lens B L1)
- `:1219-1225` is litellm `LiteLLM_SpendLogs` fixture (DON'T rename); actual aigw `spend_logs` fixture at `:1243-1260` (rename).

### 11. chat.rs non-streaming SpendLog line wrong (Lens C C6)
- Design cites `chat.rs:1184` for non-streaming; `:1184` is streaming Phase 1 placeholder. Actual non-streaming success SpendLog at `chat.rs:1536-1582`.

### 12. chat.rs response bodies have NO `request_id` field (Lens C C7)
- Only `v1_messages.rs` (via `anthropic_error` helper) puts `request_id` in response bodies. chat.rs error/success bodies are `{"error":{...}}`. §6.3 claim inaccurate but no implementation impact (it's a don't-change boundary).

### 13. `update_spend_log` has 4 impls + 1 dispatch, not "three" (Lens C C8)
- trait `:1192`, Sqlite `:1341`, Mysql `:1629`, Postgres `:1904`, Database dispatch `:2166` = 5 locations.

---

## Sound (verified, no defects)

- §10 tracing option A: no log-collector config in repo depends on `request_id` field name → safe to rename span field to `call_id`. (Lens B)
- §5 frontend: 3 independent interfaces + 5 state vars confirmed; "~16 + 3" accurate. (Lens B)
- §4.5 litellm-side fixtures correctly identified don't-rename. (Lens B)
- OpenAI non-streaming `id` extraction (`chat.rs:1536`, `v1_messages.rs:960`) correct. (Lens C)
- OpenAI streaming chunk shape + single loop confirmed. (Lens C)
- Anthropic streaming `{"type":"message_start","message":{"id":"msg_xxx"}}` shape confirmed. (Lens C)
- OpenAI failure `error_body` String + headers accessible. (Lens C)
- §6.3 protocol boundary lines all confirmed response-body/test-assertion, not DB/header. (Lens C)
- `models.rs:118` SpendLog.request_id + `:185` Tag variant confirmed. (Lead)
- MySQL 8.4 target supports `RENAME COLUMN` (Lead).
- SQLite idempotency via sqlx version table (Lead; design note corrected in v6).

---

## Gate 2 decision

**Design amended to v6** for: 022→023 (#1), import override direction (#2), test list (#6). Findings #3, #4, #5, #7, #8, #9 require a further v6.1 patch before implementation can safely start — they affect the **core expectation** (#4) and **data integrity** (#3). Applying these now.

Findings #10-#13 are factual line-number/claim corrections recorded for the implementer; folded into the implementation checklist.
