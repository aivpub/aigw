-- 021_spend_logs_body_archive.sql (PostgreSQL)
--
-- Body archive columns on spend_logs: track archived state + parquet path.
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS body_archived INTEGER NOT NULL DEFAULT 0;
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS parquet_path TEXT;
