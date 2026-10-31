-- 036_mcp_call_body.sql (MySQL)
--
-- The request and response an `mcp_call_log` row describes (Phase 58,
-- Stage 152). See the SQLite variant for the full rationale.
--
-- `MEDIUMTEXT`, not `TEXT`: MySQL's `TEXT` holds 65,535 **bytes**, and the
-- default body limit is 64 KiB — a body at the limit overflows `TEXT` once the
-- character set is more than one byte wide, turning a truncation into a write
-- error. `MEDIUMTEXT` (16 MiB) leaves room without inviting unbounded rows,
-- because the Rust side truncates before binding.
--
-- `truncated` is `TINYINT(1)` rather than `BOOLEAN`: MySQL accepts the keyword
-- but stores it as this, and 031/035's neighbours are written the same way.
--
-- No `CREATE INDEX IF NOT EXISTS` in MySQL, so the key is declared inline.

CREATE TABLE IF NOT EXISTS mcp_call_body (
    call_id        VARCHAR(64) NOT NULL PRIMARY KEY,
    request_body   MEDIUMTEXT NULL,
    response_body  MEDIUMTEXT NULL,
    truncated      TINYINT(1) NOT NULL DEFAULT 0,
    created_at     VARCHAR(64) NOT NULL,
    KEY idx_mcp_call_body_created (created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
