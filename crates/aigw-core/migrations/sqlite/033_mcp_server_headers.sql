-- 033_mcp_server_headers.sql (SQLite)
--
-- Custom outbound auth header name and extra request headers (Phase 56,
-- Stage 146). The gateway used to inject exactly one header
-- (`Authorization: Bearer <token>`), which made every upstream that wants a
-- differently-named credential unreachable: TAPD wants
-- `X-Tapd-Access-Token`, 工蜂 wants `PRIVATE-TOKEN`.
--
-- `auth_header_name` NULL or empty means "keep the current behaviour", i.e.
-- `Authorization: Bearer <resolved token>`. A non-empty name means the resolved
-- token goes into THAT header verbatim (no `Bearer ` prefix) — those upstreams
-- expect a bare token.
--
-- `auth_header_value` holds an ENCRYPTED value (`v2:gcm:`, see `seal_token`).
-- It is a second credential, deliberately NOT a row in `mcp_server_tokens`:
-- that table is keyed by owner (server/user), while the auth header is a single
-- static per-server setting.
--
-- `extra_headers` holds an ENCRYPTED JSON array `[{"name":..,"value":..}]`.
-- Encrypted as one blob because "which of these is a secret" cannot be decided
-- at registration time; per-entry flags would cost more than they buy.
--
-- All three are nullable and append-only, so existing rows keep behaving
-- exactly as before with no backfill.

ALTER TABLE mcp_servers ADD COLUMN auth_header_name TEXT;
ALTER TABLE mcp_servers ADD COLUMN auth_header_value TEXT;
ALTER TABLE mcp_servers ADD COLUMN extra_headers TEXT;
