-- 021_spend_logs_body_archive.sql (PostgreSQL)
--
-- Body archive columns on spend_logs: track archived state + parquet path.
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS body_archived BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS parquet_path TEXT;
