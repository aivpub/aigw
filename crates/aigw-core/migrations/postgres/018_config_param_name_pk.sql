-- 018_config_param_name_pk.sql (PostgreSQL)
-- Drop the surrogate `id` column and promote `param_name` to PRIMARY KEY.
-- See sqlite/018_config_param_name_pk.sql for the rationale.

ALTER TABLE config DROP CONSTRAINT IF EXISTS config_pkey;
ALTER TABLE config DROP COLUMN IF EXISTS id;
ALTER TABLE config ADD PRIMARY KEY (param_name);

-- Retain the unique index for parity (redundant with the PK but harmless).
CREATE UNIQUE INDEX IF NOT EXISTS idx_config_param_name ON config(param_name);
