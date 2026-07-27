-- 023_rename_request_id_to_call_id.sql (PostgreSQL)
--
-- Rename spend_logs PK request_id → call_id (aigw gateway call id),
-- and add a new nullable request_id column holding the UPSTREAM provider's
-- request id (e.g. Anthropic msg_xxx / OpenAI chatcmpl-xxx) so every SpendLog
-- can be reconciled against the provider.  See design §3.1 (v6.1).
--
-- Idempotent: double-condition probe (old col exists AND new col absent) so
-- the RENAME is skipped on re-run.  002/015 are NOT modified — they still
-- create request_id; this migration converges all DBs to call_id.

-- Phase 1: spend_logs PK rename (old col present, new col absent)
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'spend_logs' AND column_name = 'request_id'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'spend_logs' AND column_name = 'call_id'
    ) THEN
        ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id;
    END IF;
END $$;

-- Phase 2: new upstream request_id (nullable; filled after upstream responds)
ALTER TABLE spend_logs ADD COLUMN IF NOT EXISTS request_id TEXT;

-- Phase 3: daily_tag_spend.request_id → call_id (same double-condition probe)
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'daily_tag_spend' AND column_name = 'request_id'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'daily_tag_spend' AND column_name = 'call_id'
    ) THEN
        ALTER TABLE daily_tag_spend RENAME COLUMN request_id TO call_id;
    END IF;
END $$;

-- Phase 4: index on upstream request_id (reconciliation point-lookup)
CREATE INDEX IF NOT EXISTS idx_spend_logs_request_id ON spend_logs(request_id);
