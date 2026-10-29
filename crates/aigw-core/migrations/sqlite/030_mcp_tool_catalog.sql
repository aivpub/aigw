-- 030_mcp_tool_catalog.sql (SQLite)
--
-- Probe snapshot of one upstream MCP server's `tools/list` (Phase 55, Stage 141).
--
-- This table is a SNAPSHOT, not an incremental cache: a probe deletes the
-- server's rows and re-inserts, inside one transaction. `(server_id, tool_name)`
-- being unique is that semantics written down — the same server cannot offer the
-- same tool twice, and the constraint catches a write path that forgot the
-- delete.
--
-- `tool_name` is the upstream name **without** the alias prefix; the prefix is a
-- Stage 144 aggregation concern, and storing `github__search` here would bake
-- one consumer's naming into the source of truth.
--
-- `input_schema` / `annotations` hold `serde_json::Value` exactly as
-- `proxies.probe_result` does. `annotations` is nullable because the upstream
-- may omit it, and storing `{}` would claim it sent an empty one.
--
-- No separate index on `server_id`: the unique constraint's left prefix already
-- covers it.

CREATE TABLE IF NOT EXISTS mcp_tool_catalog (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id     INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name     TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    input_schema  TEXT NOT NULL DEFAULT '{}',
    annotations   TEXT,
    has_conflict  INTEGER NOT NULL DEFAULT 0,
    fetched_at    TEXT NOT NULL,
    CONSTRAINT uq_mcp_tool_catalog_server_tool UNIQUE (server_id, tool_name)
);
