-- 021_spend_logs_body_archive.sql (PostgreSQL)
-- Add body_archive support columns to spend_logs.

ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS body_archived BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS parquet_path TEXT;

CREATE INDEX IF NOT EXISTS idx_spend_logs_archive
  ON spend_logs(body_archived, start_time)
  WHERE messages IS NOT NULL;
