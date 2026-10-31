-- 035_mcp_auth_rejection.sql (SQLite)
--
-- One row per MCP request rejected before the handler ran (Phase 58,
-- Stage 150). A 401/403 comes from the `SpendAuth` extractor, so `post_mcp`
-- never executes and `mcp_call_log` stays empty — measured on production:
-- 14 deliberately unauthorised calls moved `total_count` by 0.
--
-- Deliberately a separate table rather than rows in `mcp_call_log`: that log's
-- `key_id` / `user_id` answer "who called", and a rejected request has no
-- identity at all. Stuffing NULLs there would break the equality filters and
-- every existing user-scoped query.
--
-- `token_hash` is the SHA-256 of the presented token (the same format
-- `virtual_keys.token` uses), never the token itself. It is what lets an
-- operator answer "which key was sent to the wrong endpoint". NULL when the
-- request carried no credential at all.
--
-- `client_ip` comes from X-Forwarded-For's rightmost hop (the gateway-wide
-- rule), so it is spoofable and must never be used as a ban decision on its
-- own.
--
-- `reason` holds `missing_header` / `invalid_format` / `invalid_key`; no CHECK
-- constraint, matching how `token_owner` and `status` are handled elsewhere.
--
-- No foreign key to `mcp_servers`: the same reasoning as `mcp_call_log` — the
-- path is stored as text and the registration may be gone by the time anyone
-- looks.

CREATE TABLE IF NOT EXISTS mcp_auth_rejection (
    id           TEXT PRIMARY KEY,
    created_at   TEXT NOT NULL,
    path         TEXT NOT NULL,
    http_method  TEXT NOT NULL,
    reason       TEXT NOT NULL,
    token_hash   TEXT,
    client_ip    TEXT,
    user_agent   TEXT
);

CREATE INDEX IF NOT EXISTS idx_mcp_auth_rejection_created
    ON mcp_auth_rejection(created_at);

CREATE INDEX IF NOT EXISTS idx_mcp_auth_rejection_reason_created
    ON mcp_auth_rejection(reason, created_at);
