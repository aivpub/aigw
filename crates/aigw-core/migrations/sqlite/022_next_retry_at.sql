-- 022_next_retry_at.sql (SQLite)
--
-- No-op: the `next_retry_at` column on async_job_steps was already added
-- in migration 020 (async_job_steps.next_retry_at).  This migration number
-- is kept in sync across drivers for the body-archive / async-job phase.
SELECT 1;
