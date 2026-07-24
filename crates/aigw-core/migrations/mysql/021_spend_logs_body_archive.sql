-- 021_spend_logs_body_archive.sql (MySQL)
-- Add body_archive support columns to spend_logs.

ALTER TABLE spend_logs ADD COLUMN body_archived TINYINT(1) NOT NULL DEFAULT 0;
ALTER TABLE spend_logs ADD COLUMN parquet_path TEXT;

-- MySQL doesn't support partial WHERE indexes, create a regular index instead
CREATE INDEX idx_spend_logs_archive ON spend_logs(body_archived, start_time);
