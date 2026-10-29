-- 030_mcp_tool_catalog.sql (MySQL)
--
-- Probe snapshot of one upstream MCP server's `tools/list` (Phase 55, Stage 141).
-- See the SQLite variant for the full rationale.
--
-- NOTE on the foreign key: InnoDB parses and SILENTLY IGNORES the column-level
-- `REFERENCES` clause — only a table-level `FOREIGN KEY ... REFERENCES` is
-- enforced. A copy of the SQLite body would leave `ON DELETE CASCADE` inert
-- (same trap 028/029 documented).
--
-- NOTE on `description`: MySQL rejects a DEFAULT on TEXT columns, so it carries
-- none here and the write path always binds a value (empty string when the
-- upstream omitted it). A VARCHAR would have to pick a length, and a tool
-- description that happens to exceed it would fail the whole probe on
-- truncation — the snapshot is not the place to enforce a length budget.
--
-- NOTE on `tool_name`: VARCHAR(255) so the composite unique key stays inside
-- InnoDB's 3072-byte limit (8 + 255 * 4 = 1028 bytes).
--
-- NOTE on the JSON columns: sqlx encodes `serde_json::Value` natively for a
-- JSON column, so the `CAST(CAST(X'hex' AS CHAR) AS JSON)` literal form that
-- `aigw-migrate` needs for raw SQL does not apply here.

CREATE TABLE IF NOT EXISTS mcp_tool_catalog (
    id            BIGINT AUTO_INCREMENT PRIMARY KEY,
    server_id     BIGINT NOT NULL,
    tool_name     VARCHAR(255) NOT NULL,
    description   TEXT NOT NULL,
    input_schema  JSON NOT NULL,
    annotations   JSON,
    has_conflict  TINYINT(1) NOT NULL DEFAULT 0,
    fetched_at    VARCHAR(64) NOT NULL,
    UNIQUE KEY uq_mcp_tool_catalog_server_tool (server_id, tool_name),
    CONSTRAINT fk_mcp_tool_catalog_server
        FOREIGN KEY (server_id) REFERENCES mcp_servers(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
