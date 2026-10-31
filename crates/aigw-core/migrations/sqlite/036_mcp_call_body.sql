-- 036_mcp_call_body.sql (SQLite)
--
-- The request and response an `mcp_call_log` row describes (Phase 58,
-- Stage 152). A tool call that answers `-32602: Invalid arguments for tool
-- metadata: path [docid] Required` (measured on production) is recorded today as
-- `status=failed, error_code=-32602` and 312 bytes — the 312 bytes are exactly
-- what an operator needs and exactly what was thrown away.
--
-- A separate table rather than two columns on `mcp_call_log`: that table is the
-- list page's hot path (30 rows per page), and the list query would start
-- dragging KiB-sized TEXT along with it.
--
-- `call_id` is the same value as `mcp_call_log.call_id`, but there is no foreign
-- key: the retention worker deletes each table by its own `created_at`, and a FK
-- would make the delete order matter for no benefit. The same reasoning as 031
-- and 035.
--
-- `truncated` says the stored body is a prefix, not the whole thing. Recording
-- *that* it was cut keeps "why is this short?" answerable; silently storing a
-- prefix would not.
--
-- No cold storage: bodies here are KB-sized, and a 90-day hot window costs less
-- than a second Parquet schema (ADR-054).

CREATE TABLE IF NOT EXISTS mcp_call_body (
    call_id        TEXT PRIMARY KEY,
    request_body   TEXT,
    response_body  TEXT,
    truncated      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_call_body_created
    ON mcp_call_body(created_at);
