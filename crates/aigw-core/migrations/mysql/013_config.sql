-- 013_config.sql (MySQL)
-- Maps to litellm LiteLLM_Config

CREATE TABLE IF NOT EXISTS config (
    id VARCHAR(255) NOT NULL PRIMARY KEY,
    param_name VARCHAR(255) NOT NULL,
    param_value TEXT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE UNIQUE INDEX idx_config_param_name ON config(param_name);
