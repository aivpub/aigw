-- 019_credentials_jsonb.sql (PostgreSQL)
--
-- Convert credentials.credential_values / credential_info from TEXT to JSONB.
--
-- Rationale: aigw-core's `Credential` maps these columns as
-- `serde_json::Value`.  sqlx's PG driver picks JSONB as the SQL type for
-- Value, and refuses to decode from TEXT ("mismatched types; Rust type
-- `serde_json::value::Value` (as SQL type `JSONB`) is not compatible with
-- SQL type `TEXT`").  proxy_models.litellm_params / model_info were already
-- JSONB; this brings credentials in line.
--
-- The `USING col::jsonb` clause reinterprets existing TEXT payloads as JSONB
-- in place — legacy rows migrated as JSON scalar strings (`"gAAAAAB..."`)
-- decode back to `Value::String(_)` at read time, which is exactly what
-- runtime `.as_str()` decrypt paths expect.

ALTER TABLE credentials
    ALTER COLUMN credential_values TYPE JSONB USING credential_values::jsonb,
    ALTER COLUMN credential_info   TYPE JSONB USING credential_info::jsonb;

ALTER TABLE credentials
    ALTER COLUMN credential_values SET DEFAULT '{}'::jsonb,
    ALTER COLUMN credential_info   SET DEFAULT '{}'::jsonb;
