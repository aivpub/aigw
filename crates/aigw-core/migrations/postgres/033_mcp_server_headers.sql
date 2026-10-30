-- 033_mcp_server_headers.sql (PostgreSQL)
--
-- Custom outbound auth header name and extra request headers (Phase 56,
-- Stage 146). See the SQLite variant for the full rationale.
--
-- TEXT (not VARCHAR) for every one of the three: the values are ciphertext
-- (`v2:gcm:` + base64, roughly 1.4x the plaintext), so a width chosen from the
-- plaintext would overflow after sealing. Length limits live in Rust
-- (`mcp::headers`) so all three dialects reject the same inputs.

ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS auth_header_name TEXT;
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS auth_header_value TEXT;
ALTER TABLE mcp_servers ADD COLUMN IF NOT EXISTS extra_headers TEXT;
