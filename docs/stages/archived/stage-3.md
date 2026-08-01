# Stage 3: Chat Completions + Router

**Status**: Complete (2026-07-03)
**Phase**: Phase 2 -- Functional Parity

## Goal

OpenAI-compatible /v1/chat/completions and /v1/models endpoints with routing.

## Deliverables

- POST /v1/chat/completions in `crates/aigw-server/src/routes/chat.rs`
- GET /v1/models in `crates/aigw-server/src/routes/chat.rs`
- Router with 3 strategies in `crates/aigw-core/src/router.rs`
- Provider registry in `crates/aigw-core/src/provider.rs`
- Rate limiter in `crates/aigw-core/src/rate_limiter.rs`
- Budget tracking in `crates/aigw-core/src/budget.rs`

## Verification

- 6 budget tests, 8 rate limiter tests, 6 provider/router tests
- 8 HTTP-level tests for chat endpoints (auth, validation, model permissions)
- Router tests: instance selection, cooldown, success reset
- `task test` passes
