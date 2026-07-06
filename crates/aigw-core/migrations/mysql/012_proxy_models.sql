-- 012_proxy_models.sql (MySQL)
-- Maps to litellm LiteLLM_ProxyModelTable

CREATE TABLE IF NOT EXISTS proxy_models (
    model_id VARCHAR(255) NOT NULL PRIMARY KEY,
    model_name VARCHAR(255) NOT NULL,
    litellm_params TEXT NOT NULL,
    model_info TEXT NOT NULL,
    created_at VARCHAR(64) NOT NULL,
    created_by VARCHAR(255),
    updated_at VARCHAR(64) NOT NULL,
    updated_by VARCHAR(255)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE UNIQUE INDEX idx_proxy_models_model_name ON proxy_models(model_name);
