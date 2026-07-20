-- 018_config_param_name_pk.sql (MySQL)
-- Drop the surrogate `id` column and promote `param_name` to PRIMARY KEY.
-- See sqlite/018_config_param_name_pk.sql for the rationale.

ALTER TABLE config DROP INDEX idx_config_param_name;
ALTER TABLE config DROP PRIMARY KEY;
ALTER TABLE config DROP COLUMN id;
ALTER TABLE config ADD PRIMARY KEY (param_name);
-- Retain the named unique index for parity (redundant with the PK).
CREATE UNIQUE INDEX idx_config_param_name ON config(param_name);
