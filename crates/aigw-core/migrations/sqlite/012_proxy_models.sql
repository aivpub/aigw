-- 012_proxy_models.sql
-- Maps to litellm LiteLLM_ProxyModelTable
-- Stores model deployment configurations (model_name, litellm_params, model_info)

CREATE TABLE IF NOT EXISTS proxy_models (
    model_id TEXT NOT NULL PRIMARY KEY,
    model_name TEXT NOT NULL,
    litellm_params TEXT NOT NULL DEFAULT '{}',  -- JSON: {model, api_base, api_key, rpm, tpm, ...}
    model_info TEXT NOT NULL DEFAULT '{}',      -- JSON: {id, mode, max_tokens, input_cost_per_token, ...}
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_proxy_models_model_name ON proxy_models(model_name);
