-- 032_mcp_tool_allowlist.sql (PostgreSQL)
--
-- Tool-level ACL for the aggregated `/mcp` endpoint (Phase 55, Stage 144).
-- See the SQLite variant for the full rationale.
--
-- `allowed` is BOOLEAN (Rust `bool`) — sqlx-postgres has no integer→bool
-- decoding, same reason as `mcp_tool_catalog.has_conflict`.

CREATE TABLE IF NOT EXISTS mcp_tool_allowlist (
    server_id   BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name   TEXT NOT NULL,
    scope_type  TEXT NOT NULL,
    scope_id    TEXT NOT NULL,
    allowed     BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (server_id, tool_name, scope_type, scope_id)
);
