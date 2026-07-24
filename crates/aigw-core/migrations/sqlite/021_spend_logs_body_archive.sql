-- 021_spend_logs_body_archive.sql (SQLite)
-- Add body_archive support columns to spend_logs.
-- body_archived: tracks whether messages/response/proxy_server_request have been archived to cold storage
-- parquet_path: S3/local path to the Parquet file containing archived body data

ALTER TABLE spend_logs ADD COLUMN body_archived INTEGER NOT NULL DEFAULT 0;
ALTER TABLE spend_logs ADD COLUMN parquet_path TEXT;

CREATE INDEX IF NOT EXISTS idx_spend_logs_archive
  ON spend_logs(body_archived, start_time)
  WHERE messages IS NOT NULL;
