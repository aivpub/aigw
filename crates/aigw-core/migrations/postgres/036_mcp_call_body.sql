-- 036_mcp_call_body.sql (PostgreSQL)
--
-- The request and response an `mcp_call_log` row describes (Phase 58,
-- Stage 152). See the SQLite variant for the full rationale; the schema is
-- identical apart from the idiomatic types below.
--
-- `created_at` is TEXT for the same reason `mcp_call_log.created_at` is: the
-- Rust side is `String` and the app writes RFC 3339 itself, so the retention
-- worker's range filter stays a string comparison.

CREATE TABLE IF NOT EXISTS mcp_call_body (
    call_id        TEXT PRIMARY KEY,
    request_body   TEXT,
    response_body  TEXT,
    truncated      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_call_body_created
    ON mcp_call_body(created_at);
