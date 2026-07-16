-- 016_proxy_models_non_unique.sql
-- Drop UNIQUE INDEX on model_name to allow multiple deployments with the same model_name.
-- This enables load-balancing across multiple upstream instances (Phase 23 Router).

DROP INDEX idx_proxy_models_model_name ON proxy_models;
CREATE INDEX idx_proxy_models_model_name ON proxy_models(model_name);
