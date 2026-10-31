-- 035_mcp_auth_rejection.sql (PostgreSQL)
--
-- One row per MCP request rejected before the handler ran (Phase 58,
-- Stage 150). See the SQLite variant for the full rationale; the schema is
-- identical apart from the idiomatic types below.
--
-- `created_at` is TEXT for the same reason `mcp_call_log.created_at` is: the
-- Rust side is `String` and the app always writes RFC 3339 itself, so range
-- filters stay string comparisons that sort correctly for one spelling.

CREATE TABLE IF NOT EXISTS mcp_auth_rejection (
    id           TEXT PRIMARY KEY,
    created_at   TEXT NOT NULL,
    path         TEXT NOT NULL,
    http_method  TEXT NOT NULL,
    reason       TEXT NOT NULL,
    token_hash   TEXT,
    client_ip    TEXT,
    user_agent   TEXT
);

CREATE INDEX IF NOT EXISTS idx_mcp_auth_rejection_created
    ON mcp_auth_rejection(created_at);

CREATE INDEX IF NOT EXISTS idx_mcp_auth_rejection_reason_created
    ON mcp_auth_rejection(reason, created_at);
