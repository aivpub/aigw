-- 019_credentials_jsonb.sql (MySQL)
--
-- No-op: MySQL's `TEXT` is compatible with sqlx `serde_json::Value` decode.
-- The PG-side change (crates/aigw-core/migrations/postgres/019_credentials_jsonb.sql)
-- keeps this migration number in sync across drivers.
SELECT 1;
