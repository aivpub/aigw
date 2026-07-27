# Stage 85 Design Review — Lead's Independent Findings (pre-merge)

Conducted by lead in parallel with 3 subagent reviewers (Lens A migration, Lens B migrate+frontend+tracing+tests, Lens C extraction+protocol).

## CRITICAL: Migration 022 number collision (design v5 invalid)

**Finding**: Design §3.1 / §4.6 / stage-85.md §1 specify migration `022_rename_request_id_to_call_id.sql`.
But **022 is already taken**: `crates/aigw-core/migrations/{sqlite,postgres,mysql}/022_next_retry_at.sql` (added by Stage 82 on 2026-07-27, AFTER the design was written 2026-07-25).

The aigw migration runner (`db.rs:153/163/172`) uses `sqlx::migrate!("./migrations/<driver>").run(pool)`. sqlx-migrate enforces a **strictly unique version per file** and the migration files are discovered by their numeric prefix. Two files both numbered `022_*` is a hard `MigrationsDirectory` error — startup fails on every DB driver.

**Severity**: Critical (blocks every deployment — existing DBs, fresh DBs, all three drivers).

**Fix**: Renumber to **`023_rename_request_id_to_call_id.sql`** (3 files: sqlite/postgres/mysql). Update every `022` reference in `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` and `docs/stages/stage-85.md` to `023`. The SQL content (double-condition RENAME + ADD COLUMN + INDEX) is unaffected; only the filename/number changes.

## Migrate override key semantics — design doc has the direction backwards

**Finding**: Design §4.5 (line 489, "key = 源列名 request_id … value = 目标列名 call_id") says the override is `overrides.insert("request_id" → "call_id")`.

But the actual code (`native.rs:1291-1297` `build_row_values` + `:1228-1233` in `insert_rows`):
```rust
let v = row_map.get(col_name.as_str())           // col_name = TARGET column
    .or_else(|| column_override.get(col_name.as_str())   // override keyed by TARGET column
        .and_then(|mapped| row_map.get(mapped.as_str()))) // mapped = SOURCE column
```
So `column_override` is keyed by **TARGET** column name → value is **SOURCE** column name.

The existing `build_snake_overrides` (remote_import.rs:38) builds `{camel_to_snake(src)=TARGET-style: src=actual source}` — i.e. key=target-shaped, value=source. Confirmed consistent with the consumer.

To redirect litellm source `request_id` → aigw target `call_id`, the override entry must be:
```rust
overrides.insert("call_id".to_string(), "request_id".to_string());
//                   ^target          ^source
```
NOT `"request_id" → "call_id"` as the design states. The design's direction would make target `request_id` (the new upstream column) read from source `call_id` (which doesn't exist on litellm) — no effect, AND the real target `call_id` PK stays NULL → INSERT fails (the exact failure the design is trying to prevent).

**Severity**: Critical (silently defeats the entire migrate fix; existing-DB import breaks with NULL PK).

**Fix**: Insert `overrides.insert("call_id".to_string(), "request_id".to_string())` in `remote_import.rs:566` after `build_snake_overrides`. In `remote_export.rs:384`, the reverse is needed: source (aigw) `call_id` → target (litellm) `request_id`, so insert `overrides.insert("request_id".to_string(), "call_id".to_string())` (target=litellm `request_id`, source=aigw `call_id`). Verify direction empirically by running the existing migrate test after the change.

**Caveat**: Need to confirm with the migrate test that this override doesn't get overwritten by `build_snake_overrides`'s existing entry for `request_id`. The `HashMap::insert` semantics: last write wins. `build_snake_overrides` for aigw target would map `request_id`(target-style) → `request_id`(source, aigw side after rename aigw has `call_id`+`request_id` cols). Actually for **import**, source is litellm (has `request_id`), so `build_snake_overrides(&select_columns)` produces `{"request_id": "request_id"}` — target `request_id` ← source `request_id`. That's the WRONG mapping (fills upstream col, leaves PK NULL). Our inserted override `{"call_id": "request_id"}` adds the right mapping for the PK. The two coexist (different keys), so both target cols get filled from the single source col. Good — no overwrite conflict.

For **export**, source is aigw (has `call_id` + `request_id`), target is litellm (`request_id` only). `build_snake_overrides(src_col_names)` over aigw source cols: `{"call_id": "call_id", "request_id": "request_id", ...}`. Target litellm cols include `request_id`. `build_row_values` for target `request_id`: `row_map.get("request_id")` → aigw's upstream `request_id` col (NULL for historical rows!). That's WRONG — litellm `request_id` should get aigw's `call_id` (the PK equivalent). Insert `overrides.insert("request_id", "call_id")` to redirect. But `build_snake_overrides` already inserted `{"request_id": "request_id"}` — our `.insert("request_id", "call_id")` would OVERWRITE it. That's fine and intended: we want target litellm `request_id` ← source aigw `call_id` (the PK), not aigw's upstream `request_id` (which is NULL for historical rows and would break litellm's PK).

## SQLite migration idempotency — design claim aspirational, version table saves it

**Finding**: Design §3.1 SQLite says "迁移器在 Rust 侧 PRAGMA table_info 探测后决定是否执行". The actual runner (`db.rs:153` `sqlx::migrate!().run()`) does NOT do `PRAGMA table_info` probing — it relies on sqlx's `_sqlx_migrations` version table to apply each file exactly once.

**Severity**: Low (not a defect — the version table makes SQL-level idempotency belt-and-suspenders, not a hard requirement). But the design's stated rationale is inaccurate; the SQLite `ALTER TABLE … RENAME COLUMN` + `ADD COLUMN` SQLs would fail on re-run IF they ever re-ran. Since they won't (version table), this is fine. Document the accurate rationale.

**Fix**: Update design §3.1 SQLite note to: "SQLite migrations are applied once by sqlx's version table; the SQL itself is NOT re-entrant (no `IF NOT EXISTS` on RENAME/ADD), but the version table guarantees single application. The PG/MySQL double-condition is defense-in-depth for direct re-application."

## MySQL RENAME COLUMN — version OK, syntax OK

**Finding**: Design §3.1 MySQL uses `ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id`. MySQL target version is 8.4 (docker-compose.db.yml:30, docker-compose.test.yml:30). `RENAME COLUMN` is supported since MySQL 8.0. Safe.

No existing migration uses `RENAME COLUMN` (grep found only `RENAME TO` for table rename in 018). But 8.4 supports it natively. The PREPARE/INFORMATION_SCHEMA pattern is consistent with the design's approach; no existing migration uses PREPARE but the syntax is valid MySQL 8.4.

**Severity**: None (sound).

## daily_spend_queue.rs:196 — anchor valid, fix is a string literal

**Finding**: `crates/aigw-core/src/daily_spend_queue.rs:196` has `"daily_tag_spend" => ("tag", "request_id, tag, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint")`. This is the UNIQUE-constraint column string for the upsert, and it MUST change to `call_id` to match the 023 migration's Phase 3 RENAME of `daily_tag_spend.request_id → call_id`. Anchor confirmed accurate.

**Severity**: None (sound, but it's a string literal — easy to miss in grep; the design flags it correctly).

## openapi.rs:255 — anchor valid

**Finding**: `crates/aigw-server/src/openapi.rs:255` has `"request_id": { "type": "string" }`. Splitting into `call_id` (required) + `request_id` (nullable) is straightforward. Anchor confirmed.

## spend.rs URL + JSON — anchors valid

**Finding**: `crates/aigw-server/src/routes/spend.rs` confirmed: `:44` (struct field), `:248/260` (search param), `:295/426/643` (JSON response), `:338/342/348` (URL `Path(request_id)`), `:1240/1391` (route registration `/{request_id}`). The `:1240` is the real route; `:1391` is a test route. Both need `/{call_id}`. Anchor list accurate; design's "~16 处" reasonable.

## Test file list — design complete (with the 022→023 caveat)

**Finding**: `grep -rl "request_id"` over `crates/aigw-server/tests/` + `crates/aigw-core/tests/` returns 13 files. Design §4.6 lists 10 BDD + 5 non-BDD = 15. Reconciling:
- BDD (10): spend.feature, spend_aggregation.feature, body_archive_write.feature, body_archive_steps.rs, spend_end_user_steps.rs, real_db_seed.rs, messages_steps.rs, spend_steps.rs, common.rs, common_steps.rs — ✓ all present
- body_archive_read.feature (6 hits) — **design lists it under "测试文件清单" implicitly?** Design §4.6 table does NOT list `body_archive_read.feature`. Added by Stage 83 on 2026-07-27 (after design 2026-07-25). **Missed by design**.
- Non-BDD (5): integration_test.rs, body_archive/query.rs, body_archive/writer.rs, db.rs, spend.rs — ✓
- stage82_state_machine.rs (1 hit), stage83_read_path.rs (3 hits) — added by Stage 82/83 on 2026-07-27. **Missed by design** (post-design additions).

**Severity**: Medium (3 test files missed → compile failures when running their `request_id`-bearing assertions after rename). Easy fix: add to the rename checklist.

**Fix**: Add `body_archive_read.feature`, `stage82_state_machine.rs`, `stage83_read_path.rs` to the test rename list in stage-85.md §4.6.

## Frontend — 3 interfaces confirmed, state vars confirmed

**Finding**: 
- `spend-logs/index.tsx:35` interface SpendLog (inline, not shared) ✓
- `spend-logs/index.tsx:47` interface SpendLogDetail (inline) ✓
- `dashboard/index.tsx:43` interface SpendLog (inline, duplicate) ✓
- State vars: `requestIdFilter` (:481), `requestIdInput` (:482), `detailRequestId` (:491), `handleRequestIdInput` (fn), URL `&request_id=` (:538), queryKey arrays (:532, :558), endpoint URL `/global/spend/logs/${detailRequestId}` (:559)
- Plus `spend-logs.steps.ts` + `api-mocks.ts` (frontend BDD) — design mentions ✓

Design's "~16 + 3 changes" estimate is reasonable; the URL param `?request_id=` stays (§6.2 compromise), so state var names can stay `requestId*` — only the API field `request_id` → `call_id` must change. Design §5.5 note is accurate.

**Severity**: None (sound).

## Next: merge subagent findings

The 3 subagents (Lens A/B/C) are running in background. Their findings will be merged into this file once they return. The lead's independent findings above are sufficient to start implementation with the design amended (022→023, override direction fix, test list additions).
