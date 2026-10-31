-- 035_mcp_auth_rejection.sql (MySQL)
--
-- One row per MCP request rejected before the handler ran (Phase 58,
-- Stage 150). See the SQLite variant for the full rationale.
--
-- NOTE on the types: the Rust side is `String` for every column and sqlx
-- decodes a `String` from VARCHAR but not from DATETIME, so `created_at` is
-- VARCHAR(64) — same reasoning as 029/030/031. `path` and `user_agent` are
-- unbounded in principle, hence TEXT; `user_agent` is truncated to 512 chars
-- in Rust before it is bound.
--
-- NOTE on the indexes: MySQL has no `CREATE INDEX IF NOT EXISTS`, so they are
-- declared inline as `KEY`s (same as 031).
--
-- No foreign key to `mcp_servers` — see the SQLite variant.

CREATE TABLE IF NOT EXISTS mcp_auth_rejection (
    id           VARCHAR(64) NOT NULL PRIMARY KEY,
    created_at   VARCHAR(64) NOT NULL,
    path         TEXT NOT NULL,
    http_method  VARCHAR(16) NOT NULL,
    reason       VARCHAR(32) NOT NULL,
    token_hash   VARCHAR(128) NULL,
    client_ip    VARCHAR(64) NULL,
    user_agent   TEXT NULL,
    KEY idx_mcp_auth_rejection_created (created_at),
    KEY idx_mcp_auth_rejection_reason_created (reason, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
