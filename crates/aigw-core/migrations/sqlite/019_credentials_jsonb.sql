-- 019_credentials_jsonb.sql (SQLite)
--
-- No-op: SQLite is dynamically typed and `TEXT` already round-trips
-- `serde_json::Value` for us (sqlx parses the text as JSON).  The
-- PG-side change (crates/aigw-core/migrations/postgres/019_credentials_jsonb.sql)
-- keeps this migration number in sync across drivers.
SELECT 1;
