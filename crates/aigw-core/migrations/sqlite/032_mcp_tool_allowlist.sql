-- 032_mcp_tool_allowlist.sql (SQLite)
--
-- Tool-level ACL for the aggregated `/mcp` endpoint (Phase 55, Stage 144).
--
-- A row means "someone decided something about this tool". **No row means
-- unrestricted** (stage-144 §3.3): ACLs are added progressively, so the default
-- has to be "usable", otherwise registering a new server would demand a
-- complete allowlist before any tool worked.
--
-- `scope_type` is `key` / `team` / `org`. `scope_id` holds `KeyIdentity.token_hash`
-- for `key` (that struct has no `key_id`, and `mcp_call_log.key_id` already
-- stores the same hash), the team id for `team`, and the organization id for
-- `org`. Precedence is key > team > org, resolved in `mcp::acl`.
--
-- No surrogate id: the row *is* the quadruple (one decision per tool per
-- scope), and the admin endpoint replaces the whole set for a server in one
-- transaction, so nothing ever needs to address a single row. The composite key
-- also serves every read — the left prefix covers `server_id`.
--
-- `tool_name` stores the upstream name without the alias prefix, matching
-- `mcp_tool_catalog` (the prefix is an aggregation concern, not a stored fact).
-- No existence check against the catalog: a rule may be written before the tool
-- is probed, and a failed probe must not block editing.

CREATE TABLE IF NOT EXISTS mcp_tool_allowlist (
    server_id   INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name   TEXT NOT NULL,
    scope_type  TEXT NOT NULL,
    scope_id    TEXT NOT NULL,
    allowed     INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (server_id, tool_name, scope_type, scope_id)
);
