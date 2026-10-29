-- 029_mcp_gateway.sql (PostgreSQL)
--
-- MCP gateway registry + upstream credentials (Phase 55, Stage 140).
-- See the SQLite variant for the full rationale; the schema is identical apart
-- from the idiomatic types below.
--
-- Three type choices are load-bearing for sqlx, not stylistic:
--   * `enabled` / `is_default` are BOOLEAN (Rust `bool`). sqlx-postgres maps
--     `bool` to BOOL only, so an INTEGER column fails to decode.
--   * the auto-increment ids are BIGSERIAL (Rust `i64`).
--   * `created_at` / `updated_at` / `expires_at` are TEXT because the Rust side
--     is `String` and the app always supplies RFC 3339 itself.

CREATE TABLE IF NOT EXISTS mcp_servers (
    id                BIGSERIAL PRIMARY KEY,
    alias             TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    url               TEXT NOT NULL,
    transport         TEXT NOT NULL DEFAULT 'streamable_http',
    credential_scope  TEXT NOT NULL DEFAULT 'shared_or_user',
    enabled           BOOLEAN NOT NULL DEFAULT TRUE,
    protocol_version  TEXT,
    created_at        TEXT NOT NULL DEFAULT now(),
    updated_at        TEXT NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS mcp_server_tokens (
    id            BIGSERIAL PRIMARY KEY,
    server_id     BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    owner_type    TEXT NOT NULL,
    owner_id      TEXT NOT NULL DEFAULT '',
    token_name    TEXT NOT NULL,
    token_value   TEXT NOT NULL,
    is_default    BOOLEAN NOT NULL DEFAULT FALSE,
    enabled       BOOLEAN NOT NULL DEFAULT TRUE,
    expires_at    TEXT,
    created_by    TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT now(),
    updated_at    TEXT NOT NULL DEFAULT now(),
    CONSTRAINT uq_mcp_server_tokens_owner UNIQUE (server_id, owner_type, owner_id, token_name)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_alias
    ON mcp_servers(alias);

CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled
    ON mcp_servers(enabled);

CREATE INDEX IF NOT EXISTS idx_mcp_server_tokens_server
    ON mcp_server_tokens(server_id);

-- `WHERE is_default` rather than `WHERE is_default = 1`: PostgreSQL has no
-- boolean = integer operator, and the column is BOOLEAN here (INTEGER would
-- fail to decode into the Rust `bool`).
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_server_tokens_default
    ON mcp_server_tokens(server_id, owner_type, owner_id)
    WHERE is_default;
