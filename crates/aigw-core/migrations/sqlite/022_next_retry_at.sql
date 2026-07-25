-- 022_next_retry_at.sql (SQLite)
-- Add next_retry_at column to async_job_steps for exponential backoff retry scheduling.
-- claim_next_step filters out steps where next_retry_at is in the future.

ALTER TABLE async_job_steps ADD COLUMN next_retry_at TEXT;
