-- 026_proxy_models_enabled.sql (MySQL)
--
-- Add `enabled` column to proxy_models and deleted_models to support
-- disabling a model without deleting it (Stage 121).
--
-- Existing rows default to enabled=1.

ALTER TABLE proxy_models ADD COLUMN enabled TINYINT(1) NOT NULL DEFAULT 1;
ALTER TABLE deleted_models ADD COLUMN enabled TINYINT(1) NOT NULL DEFAULT 1;
