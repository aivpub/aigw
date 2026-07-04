# aigw -- Technical Debt Ledger

## Active Items

### TD-002: @real_api step bindings not yet implemented

- **Date**: 2026-07-04
- **Priority**: P2
- **Description**: The `@real_api` feature files (`end_to_end_real.feature`, `compatibility_real.feature`)
  have Gherkin scenarios defined but no step bindings. These scenarios correctly skip in CI
  (9 skipped when `AIGW_REAL_API` is unset). Step bindings need `AIGW_REAL_API=1` guard
  to skip when env var is not set, and actual HTTP calls to the running aigw server with
  real LLM API keys.
- **Impact**: Real API integration cannot be validated via BDD. Manual testing required.
- **Resolution**: Implement step bindings in `tests/bdd_steps/real_api_steps.rs` with
  env-var guards and real HTTP client calls.

### TD-003: BDD coverage reporting not automated

- **Date**: 2026-07-04
- **Priority**: P3
- **Description**: No automated BDD endpoint coverage report. Stage 12 acceptance criteria
  includes "BDD 覆盖率报告生成（端点覆盖率 ≥ 90%）" but this requires a coverage mapping
  tool that links .feature scenarios to API routes.
- **Impact**: Cannot quantitatively verify endpoint coverage.
- **Resolution**: Implement a simple script/tool that maps scenarios to endpoints and
  generates a coverage report.

## Resolved Items

### TD-001: Dead code cleanup (Resolved 2026-07-03)

- Removed unused `ChatCompletionRequest`, `ChatMessage`, `KeyUpdateQuery` stubs.
- Removed redundant `proxy.rs` auth/handler (replaced by `chat.rs` implementation).
- `TenantAuth` extractor available for future SaaS route enforcement.

## Monitoring Items

| Item | Priority | Trigger |
|------|----------|---------|
| TenantAuth wiring | Low | When SaaS deployments need per-route org enforcement |
| Provider registry in AppState | Low | When chat.rs switches from env-var to registry-based routing |
| Rate limiter in AppState | Low | When TPM/RPM enforcement is enabled on chat endpoints |
