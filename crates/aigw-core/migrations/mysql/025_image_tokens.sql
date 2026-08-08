-- 025_image_tokens.sql (MySQL)
--
-- Add image_tokens column to spend_logs and the 6 daily_*_spend tables.
-- image_tokens is a subset of prompt_tokens (multimodal image portion) —
-- stored for analysis & reconciliation only, does not affect calc_spend.
-- Source of the value ("upstream" | "estimated") lives in metadata.image_tokens_source.

ALTER TABLE spend_logs ADD COLUMN image_tokens BIGINT;

ALTER TABLE daily_user_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_team_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_organization_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_end_user_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_agent_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
ALTER TABLE daily_tag_spend ADD COLUMN image_tokens BIGINT NOT NULL DEFAULT 0;
