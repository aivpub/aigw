-- 031_mcp_call_log.sql (MySQL)
--
-- One row per request to an MCP endpoint (Phase 55, Stage 143).
-- See the SQLite variant for the full rationale.
--
-- NOTE on `error_code` / `tool_name`: TEXT cannot carry a DEFAULT in MySQL, and
-- neither of these needs one — both are NULLable and the write path always binds
-- a value (NULL when absent). `server_alias` / `method` / `status` DO carry
-- defaults, so they are VARCHAR.
--
-- NOTE on `created_at`: the Rust side is `String`, and sqlx decodes a `String`
-- from VARCHAR but not from DATETIME. The app supplies RFC 3339 itself, so this
-- is VARCHAR(64) — same reasoning as 029/030.
--
-- NOTE on `latency_ms` / `request_bytes` / `response_bytes`: BIGINT because the
-- Rust side is `i64`. `key_id` is VARCHAR(128) because it holds a SHA-256 hex
-- digest (`token_hash`) or the literal `*master*`.
--
-- No foreign key to `mcp_servers` — see the SQLite variant.

CREATE TABLE IF NOT EXISTS mcp_call_log (
    call_id         VARCHAR(64) NOT NULL PRIMARY KEY,
    server_alias    VARCHAR(64) NOT NULL,
    tool_name       VARCHAR(255) NULL,
    method          VARCHAR(64) NOT NULL,
    key_id          VARCHAR(128) NULL,
    user_id         VARCHAR(255) NULL,
    token_owner     VARCHAR(16) NOT NULL DEFAULT 'anonymous',
    status          VARCHAR(16) NOT NULL,
    error_code      VARCHAR(64) NULL,
    latency_ms      BIGINT NOT NULL DEFAULT 0,
    request_bytes   BIGINT NOT NULL DEFAULT 0,
    response_bytes  BIGINT NOT NULL DEFAULT 0,
    created_at      VARCHAR(64) NOT NULL,
    KEY idx_mcp_call_log_created_at (created_at),
    KEY idx_mcp_call_log_alias_created (server_alias, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
