# Stage 85 Design Review — Reviewer Brief

You are reviewing the **design** (not the code) for aigw Stage 85:
`request_id → call_id` rename + upstream request_id extraction for provider reconciliation.

## Docs to read
- Design: `docs/plans/2026-07-25-request-id-to-gw-call-id-rename.md` (v5, 769 lines, 5 review rounds)
- Stage: `docs/stages/stage-85.md`

## Core expectation (the ONE business goal)
Any SpendLog row can be reconciled against the upstream provider using the
provider's `request_id` — for BOTH success and 4xx/5xx failure paths.

## Renaming map
- `spend_logs.request_id` (PK, aigw's own UUID v7) → renamed `call_id`
- NEW `spend_logs.request_id TEXT` (nullable) = upstream provider's id (msg_xxx / chatcmpl-xxx)
- `daily_tag_spend.request_id` → `call_id`
- `models.rs::Tag { tag, request_id }` → `Tag { tag, call_id }`

## Hard "do NOT change" boundaries (design §2.2 / §4.5 / §6.3)
1. HTTP layer: `tower_http::request_id::*` (main.rs:57, chat.rs:24) — variable names `request_id` stay
2. Outbound protocol response body field `request_id` (Anthropic/OpenAI contract, v1_messages.rs:48/141/165/179/213, chat.rs) — field NAME stays, value = call_id
3. litellm source/target SQL in aigw-migrate `native.rs` — stays `request_id`

## Your lens
{{LENS}}

## How to verify
Read the cited source files to confirm the design's code anchors are still
accurate (Stage 84 landed 2026-07-27, design written 2026-07-25). Report ONLY
design-level defects that would cause the implementation to fail or break a
contract. For each finding: file:line anchor, concrete failure scenario,
severity (Critical/High/Medium/Low), and a fix. If the design is sound for
your lens, say so explicitly with the evidence you checked. Do NOT propose
stylistic changes.
