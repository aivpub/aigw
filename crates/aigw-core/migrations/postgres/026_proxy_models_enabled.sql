-- 026_proxy_models_enabled.sql (PostgreSQL)
--
-- Add `enabled` column to proxy_models and deleted_models to support
-- disabling a model without deleting it (Stage 121).
--
-- Existing rows default to enabled=TRUE.

ALTER TABLE proxy_models ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE deleted_models ADD COLUMN enabled BOOLEAN NOT NULL DEFAULT TRUE;
