-- 029_mcp_gateway.sql (MySQL)
--
-- MCP gateway registry + upstream credentials (Phase 55, Stage 140).
-- See the SQLite variant for the full rationale.
--
-- NOTE on the foreign key: InnoDB parses and SILENTLY IGNORES the column-level
-- `REFERENCES` clause — only a table-level `FOREIGN KEY ... REFERENCES` is
-- enforced. Copying the SQLite body verbatim here would leave `ON DELETE
-- CASCADE` inert (same trap 028 documented).
--
-- NOTE on the timestamps: the Rust side is `String`, and sqlx decodes a
-- `String` from VARCHAR but NOT from DATETIME. The app supplies these itself
-- (RFC 3339), so they are VARCHAR(64) here, exactly as 027/028 do.
--
-- NOTE on `is_default`: MySQL has no partial unique index, so the "at most one
-- default per owner" rule lives in `set_default_mcp_token`'s transaction
-- (zero-then-set), and every read takes `ORDER BY is_default DESC, id DESC
-- LIMIT 1` (stage-140 §3.4). No index below expresses it.
--
-- NOTE on the unique key: `utf8mb4` costs 4 bytes per character against
-- InnoDB's 3072-byte key limit; 64 + 16 + 255 + 255 characters is 2360 bytes,
-- so the composite fits without a prefix declaration.

CREATE TABLE IF NOT EXISTS mcp_servers (
    id                BIGINT AUTO_INCREMENT PRIMARY KEY,
    alias             VARCHAR(64) NOT NULL,
    display_name      VARCHAR(255) NOT NULL,
    url               TEXT NOT NULL,
    transport         VARCHAR(32) NOT NULL DEFAULT 'streamable_http',
    credential_scope  VARCHAR(32) NOT NULL DEFAULT 'shared_or_user',
    enabled           TINYINT NOT NULL DEFAULT 1,
    protocol_version  VARCHAR(32),
    created_at        VARCHAR(64) NOT NULL,
    updated_at        VARCHAR(64) NOT NULL,
    UNIQUE KEY idx_mcp_servers_alias (alias),
    KEY idx_mcp_servers_enabled (enabled)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS mcp_server_tokens (
    id            BIGINT AUTO_INCREMENT PRIMARY KEY,
    server_id     BIGINT NOT NULL,
    owner_type    VARCHAR(16) NOT NULL,
    owner_id      VARCHAR(255) NOT NULL DEFAULT '',
    token_name    VARCHAR(255) NOT NULL,
    token_value   TEXT NOT NULL,
    is_default    TINYINT NOT NULL DEFAULT 0,
    enabled       TINYINT NOT NULL DEFAULT 1,
    expires_at    VARCHAR(64),
    created_by    VARCHAR(255) NOT NULL DEFAULT '',
    created_at    VARCHAR(64) NOT NULL,
    updated_at    VARCHAR(64) NOT NULL,
    UNIQUE KEY uq_mcp_server_tokens_owner (server_id, owner_type, owner_id, token_name),
    KEY idx_mcp_server_tokens_server (server_id),
    CONSTRAINT fk_mcp_server_tokens_server
        FOREIGN KEY (server_id) REFERENCES mcp_servers(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
