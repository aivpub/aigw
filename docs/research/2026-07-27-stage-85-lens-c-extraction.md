# Stage 85 Reviewer Lens C — Upstream ID Extraction + Protocol Boundary

Review the DESIGN (not code) for aigw Stage 85. Read first:
- `docs/research/2026-07-27-stage-85-design-review-brief.md` (brief)
- `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` (design v5)
- `docs/stages/stage-85.md` (stage)

## Your lens: upstream ID extraction logic (success + 4xx/5xx failure) + protocol boundary correctness

This is the CORE business value of Stage 85: every SpendLog must carry the upstream provider's `request_id`, for both success and 4xx/5xx failure paths.

### 1. §4.3 success path — non-streaming
- Read `crates/aigw-server/src/routes/chat.rs` around the non-streaming SpendLog construction (design cites `:1184`) and `v1_messages.rs`. Confirm `response_json.get("id")` is the right extraction for OpenAI (chatcmpl-xxx). For Anthropic non-streaming, where is the id? Confirm.

### 2. §4.3 success path — streaming (OpenAI + Anthropic)
- Read `chat.rs:1228` `chunk_jsons: Vec<Value>` collection loop. Design says: extract first chunk's `id` IN the same loop (don't open a new one). Confirm the loop structure — is there a single `while let Some(chunk_result) = stream.next().await` that pushes to `chunk_jsons`? Can `upstream_id` extraction be slotted in cleanly?
- Read `v1_messages.rs` streaming. Design says: extract `message_start` event's `message.id`. Confirm the SSE event parsing structure — where is `message_start` handled? Is the chunk JSON shape `{"type":"message_start","message":{"id":"msg_xxx"}}`?

### 3. §4.3 failure path (v5 — the core expectation's blind-spot coverage)
This is the v5 increment. Design says:
- OpenAI 4xx/5xx: parse `error_body` (string at `chat.rs:1093`) as JSON, take `id`, fallback to upstream response header `x-request-id` (reuse `:1067` logic).
- Anthropic 4xx/5xx: parse error body, take `request_id` field (protocol field name, value = upstream id), fallback to response headers `request-id` / `x-request-id`.

Verify:
- Read `chat.rs:1065-1110` (the x-request-id mismatch + 4xx/5xx error_body block). Confirm `error_body` is a String and `upstream_resp.headers()` is accessible at the failure point.
- Read `v1_messages.rs` failure path. Where is the error body? Where are response headers? Confirm both are reachable at the failure SpendLog construction.
- Does the failure path SpendLog construction site pass through an `upstream_id` to `update_spend_log`? The design says failure path also calls `update_spend_log` with `upstream_id=fail_upstream_id`. Confirm there IS a Phase 2 UPDATE call in the failure path (or that the failure INSERT carries it). If failures only INSERT once and never UPDATE, the upstream_id must be in the INSERT — flag if the design's "UPDATE with COALESCE" doesn't apply to failure path.

### 4. §4.3 Phase 2 UPDATE signature change
- Design option A: extend `update_spend_log` to take `upstream_request_id: Option<&str>`, SQL `UPDATE ... SET request_id = COALESCE($new, request_id) WHERE call_id = $N`.
- Read `crates/aigw-core/src/db.rs` `update_spend_log` signature (design cites `:1191/1343/1631` — three impls). Confirm all three impls (Sqlite/Mysql/Postgres) + the `Database` dispatch layer need the new param. Count call sites — design says failure ×3 + streaming Phase 2 ×2. Verify against actual call sites with `grep -n "update_spend_log" crates/aigw-server/src/`.

### 5. §6.3 protocol boundary
- Design: outbound LLM API response body keeps field NAME `request_id`, value = call_id. Sites: `v1_messages.rs:48/141/165/179/213` + chat.rs response bodies + `v1_messages.rs:1424/1428` test assertions.
- Read those exact lines. Confirm each is a JSON response body field (not a DB field, not an HTTP header). Flag any that is actually a DB field mislabeled as "do not change".

### 6. §4.2 search dual-column
- Design: `WHERE call_id LIKE $1 OR request_id LIKE $1`. Read `db.rs` `query_spend_logs_filtered` (3 impls). Confirm the search currently binds `request_id` once; dual-column means 2 binds + OR. Check for SQL injection / placeholder count bugs in the dynamic SQL builder (design cites `:1512/1541`).

## Output
Report ONLY design-level defects that would cause implementation failure or contract breaks. For each: `file:line` anchor, concrete failure scenario, severity (Critical/High/Medium/Low), fix. If sound, say so with evidence. No stylistic nits.
