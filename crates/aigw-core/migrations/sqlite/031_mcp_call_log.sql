-- 031_mcp_call_log.sql (SQLite)
--
-- One row per request to an MCP endpoint (Phase 55, Stage 143).
--
-- Written whether the call succeeded or failed: a failed call is the main input
-- to troubleshooting, so logging only successes would be no audit at all
-- (stage-143 §3.6).
--
-- `call_id` is the gateway's UUID v7 request id (the same value
-- `spend_logs.call_id` and the `x-call-id` response header carry), falling back
-- to a fresh v7 when a handler runs outside the request-id layer.
--
-- `method` holds the **MCP** method name (`tools/list`, `tools/call`,
-- `initialize`, `server/discover`) — not the HTTP method.
--
-- `key_id` holds `KeyIdentity.token_hash`; master key requests carry the literal
-- `*master*`, so it is always populated even though the column is nullable.
-- `user_id` is the only one that can be empty (master key, or a key with no
-- bound user).
--
-- `token_owner` answers "whose identity went upstream": `server` / `user` /
-- `anonymous`. The token itself is never stored here — not the plaintext and not
-- the ciphertext.
--
-- No foreign key to `mcp_servers`: the log outlives the registration it
-- describes, which is exactly what an audit trail is for.
--
-- `latency_ms` / `request_bytes` / `response_bytes` are BIGINT because the Rust
-- side is `i64`; the two indexes serve Stage 144's `GET /admin/mcp/calls`, which
-- filters by alias and orders by time.

CREATE TABLE IF NOT EXISTS mcp_call_log (
    call_id         TEXT PRIMARY KEY,
    server_alias    TEXT NOT NULL,
    tool_name       TEXT,
    method          TEXT NOT NULL,
    key_id          TEXT,
    user_id         TEXT,
    token_owner     TEXT NOT NULL DEFAULT 'anonymous',
    status          TEXT NOT NULL,
    error_code      TEXT,
    latency_ms      BIGINT NOT NULL DEFAULT 0,
    request_bytes   BIGINT NOT NULL DEFAULT 0,
    response_bytes  BIGINT NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_call_log_created_at
    ON mcp_call_log(created_at);

CREATE INDEX IF NOT EXISTS idx_mcp_call_log_alias_created
    ON mcp_call_log(server_alias, created_at);
