-- 012_proxy_models.sql (PostgreSQL)
-- Maps to litellm LiteLLM_ProxyModelTable

CREATE TABLE IF NOT EXISTS proxy_models (
    model_id TEXT NOT NULL PRIMARY KEY,
    model_name TEXT NOT NULL,
    litellm_params TEXT NOT NULL DEFAULT '{}',
    model_info TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    created_by TEXT,
    updated_at TEXT NOT NULL,
    updated_by TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_proxy_models_model_name ON proxy_models(model_name);
