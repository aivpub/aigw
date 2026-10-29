-- 030_mcp_tool_catalog.sql (PostgreSQL)
--
-- Probe snapshot of one upstream MCP server's `tools/list` (Phase 55, Stage 141).
-- See the SQLite variant for the full rationale; the schema is identical apart
-- from the idiomatic types below.
--
-- `has_conflict` is BOOLEAN (Rust `bool`) and the id is BIGSERIAL (Rust `i64`)
-- for the same reason as 029: sqlx-postgres has no integer→bool decoding, and
-- `INTEGER` would arrive as `i32`.

CREATE TABLE IF NOT EXISTS mcp_tool_catalog (
    id            BIGSERIAL PRIMARY KEY,
    server_id     BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name     TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    input_schema  JSONB NOT NULL DEFAULT '{}'::jsonb,
    annotations   JSONB,
    has_conflict  BOOLEAN NOT NULL DEFAULT FALSE,
    fetched_at    TEXT NOT NULL,
    CONSTRAINT uq_mcp_tool_catalog_server_tool UNIQUE (server_id, tool_name)
);
