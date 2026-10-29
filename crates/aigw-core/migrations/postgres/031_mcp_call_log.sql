-- 031_mcp_call_log.sql (PostgreSQL)
--
-- One row per request to an MCP endpoint (Phase 55, Stage 143).
-- See the SQLite variant for the full rationale; the schema is identical apart
-- from the idiomatic types below.
--
-- `latency_ms` / `request_bytes` / `response_bytes` are BIGINT (Rust `i64`).
-- `INTEGER` would only decode into `Option<i64>` while the column is NULL —
-- sqlx-postgres refuses an `i32` → `i64` widening.
--
-- `created_at` is TEXT: the Rust side is `String` and the app always writes
-- RFC 3339 itself, exactly as 029/030 do.

CREATE TABLE IF NOT EXISTS mcp_call_log (
    call_id         TEXT PRIMARY KEY,
    server_alias    TEXT NOT NULL,
    tool_name       TEXT,
    method          TEXT NOT NULL,
    key_id          TEXT,
    user_id         TEXT,
    token_owner     TEXT NOT NULL DEFAULT 'anonymous',
    status          TEXT NOT NULL,
    error_code      TEXT,
    latency_ms      BIGINT NOT NULL DEFAULT 0,
    request_bytes   BIGINT NOT NULL DEFAULT 0,
    response_bytes  BIGINT NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_call_log_created_at
    ON mcp_call_log(created_at);

CREATE INDEX IF NOT EXISTS idx_mcp_call_log_alias_created
    ON mcp_call_log(server_alias, created_at);
