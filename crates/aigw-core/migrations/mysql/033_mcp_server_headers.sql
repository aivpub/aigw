-- 033_mcp_server_headers.sql (MySQL)
--
-- Custom outbound auth header name and extra request headers (Phase 56,
-- Stage 146). See the SQLite variant for the full rationale.
--
-- NOTE on the types: TEXT cannot carry a DEFAULT in MySQL, and none of the
-- three needs one — all are NULLable and the write path always binds a value
-- (NULL when unset). TEXT rather than VARCHAR because the two value columns
-- hold ciphertext, whose length is ~1.4x the plaintext; the limits are enforced
-- in Rust so the dialects agree.
--
-- MySQL has no `ADD COLUMN IF NOT EXISTS`; sqlx applies each migration exactly
-- once, so the plain form is correct here (same as 026).

ALTER TABLE mcp_servers ADD COLUMN auth_header_name TEXT;
ALTER TABLE mcp_servers ADD COLUMN auth_header_value TEXT;
ALTER TABLE mcp_servers ADD COLUMN extra_headers TEXT;
