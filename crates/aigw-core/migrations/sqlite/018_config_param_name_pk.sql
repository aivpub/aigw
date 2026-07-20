-- 018_config_param_name_pk.sql
-- Drop the surrogate `id` column and promote `param_name` to PRIMARY KEY.
--
-- Upstream litellm's `LiteLLM_Config` has no `id` column — `param_name` alone
-- is the natural key.  The previous schema forced migrate to invent a fake id
-- per row (or send empty strings), which collided under the PK constraint and
-- silently dropped rows via INSERT OR IGNORE.
--
-- SQLite can't drop a PK column in place; we rebuild the table.

CREATE TABLE IF NOT EXISTS config_new (
    param_name TEXT NOT NULL PRIMARY KEY,
    param_value TEXT NOT NULL DEFAULT ''
);

INSERT OR IGNORE INTO config_new (param_name, param_value)
SELECT param_name, param_value FROM config;

DROP TABLE config;
ALTER TABLE config_new RENAME TO config;

-- The old unique index on (param_name) becomes redundant now that the column
-- IS the primary key.  Recreate for parity with the old schema (harmless).
CREATE UNIQUE INDEX IF NOT EXISTS idx_config_param_name ON config(param_name);
