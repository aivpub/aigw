-- 013_config.sql
-- Maps to litellm LiteLLM_Config
-- Stores key-value configuration parameters (e.g., master_key, environment settings)

CREATE TABLE IF NOT EXISTS config (
    id TEXT NOT NULL PRIMARY KEY,
    param_name TEXT NOT NULL,
    param_value TEXT NOT NULL DEFAULT ''
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_config_param_name ON config(param_name);
