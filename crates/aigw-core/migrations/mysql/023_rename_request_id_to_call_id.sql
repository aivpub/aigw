-- 023_rename_request_id_to_call_id.sql (MySQL)
--
-- Rename spend_logs PK request_id → call_id (aigw gateway call id),
-- and add a new nullable request_id column holding the UPSTREAM provider's
-- request id (e.g. Anthropic msg_xxx / OpenAI chatcmpl-xxx) so every SpendLog
-- can be reconciled against the provider.  See design §3.1 (v6.1).
--
-- Idempotent: INFORMATION_SCHEMA + PREPARE probe on every phase (native MySQL
-- has no DO blocks and no ADD COLUMN IF NOT EXISTS).  002/015 NOT modified.

-- Phase 1: spend_logs PK rename (old col present AND new col absent)
SET @col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND column_name = 'request_id');
SET @new_col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND column_name = 'call_id');
SET @sql = IF(@col_exists > 0 AND @new_col_exists = 0,
    'ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;

-- Phase 2: new upstream request_id (nullable; absent → ADD)
SET @col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND column_name = 'request_id');
SET @sql = IF(@col_exists = 0,
    'ALTER TABLE spend_logs ADD COLUMN request_id TEXT',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;

-- Phase 3: daily_tag_spend.request_id → call_id (same double-condition probe)
SET @col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'daily_tag_spend' AND column_name = 'request_id');
SET @new_col_exists = (SELECT COUNT(*) FROM information_schema.columns
    WHERE table_schema = DATABASE() AND table_name = 'daily_tag_spend' AND column_name = 'call_id');
SET @sql = IF(@col_exists > 0 AND @new_col_exists = 0,
    'ALTER TABLE daily_tag_spend RENAME COLUMN request_id TO call_id',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;

-- Phase 4: index on upstream request_id (reconciliation point-lookup).
-- MySQL requires a prefix length for indexing a TEXT column (error 1170
-- "BLOB/TEXT column used in key specification without a key length" otherwise).
-- 128 chars covers all known upstream id formats (Anthropic msg_xxx / OpenAI
-- chatcmpl-xxx are well under 64 chars) while keeping the index compact.
SET @idx_exists = (SELECT COUNT(*) FROM information_schema.statistics
    WHERE table_schema = DATABASE() AND table_name = 'spend_logs' AND index_name = 'idx_spend_logs_request_id');
SET @sql = IF(@idx_exists = 0,
    'CREATE INDEX idx_spend_logs_request_id ON spend_logs(request_id(128))',
    'SELECT 1');
PREPARE stmt FROM @sql; EXECUTE stmt; DEALLOCATE PREPARE stmt;
