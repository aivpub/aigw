-- 032_mcp_tool_allowlist.sql (MySQL)
--
-- Tool-level ACL for the aggregated `/mcp` endpoint (Phase 55, Stage 144).
-- See the SQLite variant for the full rationale.
--
-- NOTE on the key columns: an InnoDB primary key must be indexable without a
-- prefix length, so `tool_name` / `scope_type` / `scope_id` are VARCHAR, not
-- TEXT. The four columns together are 8 + 255*4 + 16*4 + 255*4 = 2112 bytes,
-- inside InnoDB's 3072-byte limit (same budget as 030's composite unique key).
--
-- NOTE on the foreign key: InnoDB silently ignores a column-level `REFERENCES`,
-- so `ON DELETE CASCADE` only takes effect when declared table-level (trap
-- documented in 028/029/030).
--
-- NOTE on `allowed`: TINYINT(1) is what MySQL means by a boolean; sqlx decodes
-- it into a Rust `bool`, and the handler hands the UI a JSON boolean — never 0/1.

CREATE TABLE IF NOT EXISTS mcp_tool_allowlist (
    server_id   BIGINT NOT NULL,
    tool_name   VARCHAR(255) NOT NULL,
    scope_type  VARCHAR(16) NOT NULL,
    scope_id    VARCHAR(255) NOT NULL,
    allowed     TINYINT(1) NOT NULL DEFAULT 1,
    PRIMARY KEY (server_id, tool_name, scope_type, scope_id),
    CONSTRAINT fk_mcp_tool_allowlist_server
        FOREIGN KEY (server_id) REFERENCES mcp_servers(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
