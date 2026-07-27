-- 021_spend_logs_body_archive.sql (MySQL)
--
-- Body archive columns on spend_logs: track archived state + parquet path.
ALTER TABLE spend_logs ADD COLUMN body_archived BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE spend_logs ADD COLUMN parquet_path TEXT;
