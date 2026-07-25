-- 022_next_retry_at.sql (PostgreSQL)
-- Add next_retry_at column to async_job_steps for exponential backoff retry scheduling.

ALTER TABLE async_job_steps ADD COLUMN next_retry_at TIMESTAMPTZ;
