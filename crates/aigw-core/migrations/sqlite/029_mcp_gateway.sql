-- 029_mcp_gateway.sql (SQLite)
--
-- MCP gateway registry + upstream credentials (Phase 55, Stage 140).
--
-- Only these two tables are created here. `mcp_tool_catalog`, `mcp_call_log`
-- and `mcp_tool_allowlist` are appended by 030/031/032, each in the Stage that
-- gives it a consumer — a table that exists before any code writes it is where
-- "nobody knows what this column is for" grows (stage-140 §9).
--
-- `mcp_servers.alias` prefixes every tool name (`{alias}__{tool}`), so it may
-- not contain `__` and may not end in `_`: otherwise `a__b__tool` would have two
-- possible splits. Splitting always takes the FIRST `__`, and registration is
-- where the ambiguity is removed.
--
-- `credential_scope` decides whose token goes outbound:
--   shared          server-level shared token only; anonymous with none
--   shared_or_user  user token -> shared token -> anonymous (default)
--   user_required   user token only; never falls back to the shared one
--
-- `mcp_server_tokens.token_value` holds the AES-GCM `v2:gcm:` ciphertext, never
-- plaintext, and is redacted in every response (design §8.4).
--
-- `is_default`: at most one per (server_id, owner_type, owner_id). SQLite and
-- PostgreSQL express this with a partial unique index; MySQL has no such
-- capability and relies on the transaction in `set_default_mcp_token`
-- (stage-140 §3.4).

CREATE TABLE IF NOT EXISTS mcp_servers (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    alias             TEXT NOT NULL,
    display_name      TEXT NOT NULL,
    url               TEXT NOT NULL,
    transport         TEXT NOT NULL DEFAULT 'streamable_http',
    credential_scope  TEXT NOT NULL DEFAULT 'shared_or_user',
    enabled           INTEGER NOT NULL DEFAULT 1,
    protocol_version  TEXT,
    created_at        TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS mcp_server_tokens (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id     INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    owner_type    TEXT NOT NULL,
    owner_id      TEXT NOT NULL DEFAULT '',
    token_name    TEXT NOT NULL,
    token_value   TEXT NOT NULL,
    is_default    INTEGER NOT NULL DEFAULT 0,
    enabled       INTEGER NOT NULL DEFAULT 1,
    expires_at    TEXT,
    created_by    TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    CONSTRAINT uq_mcp_server_tokens_owner UNIQUE (server_id, owner_type, owner_id, token_name)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_alias
    ON mcp_servers(alias);

CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled
    ON mcp_servers(enabled);

CREATE INDEX IF NOT EXISTS idx_mcp_server_tokens_server
    ON mcp_server_tokens(server_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_server_tokens_default
    ON mcp_server_tokens(server_id, owner_type, owner_id)
    WHERE is_default = 1;
