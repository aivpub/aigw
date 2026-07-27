-- 023_rename_request_id_to_call_id.sql (SQLite)
--
-- Rename spend_logs PK request_id → call_id (aigw gateway call id),
-- and add a new nullable request_id column holding the UPSTREAM provider's
-- request id (e.g. Anthropic msg_xxx / OpenAI chatcmpl-xxx) so every SpendLog
-- can be reconciled against the provider.  See design §3.1 (v6.1).
--
-- SQLite has no conditional RENAME / no DO blocks.  sqlx's _sqlx_migrations
-- version table applies each migration file exactly once, so the SQL below
-- is NOT re-entrant but never re-runs.  PG/MySQL double-condition probes are
-- defense-in-depth for direct re-application.  002/015 NOT modified.

-- Phase 1: spend_logs PK rename
ALTER TABLE spend_logs RENAME COLUMN request_id TO call_id;

-- Phase 2: new upstream request_id (nullable; filled after upstream responds)
ALTER TABLE spend_logs ADD COLUMN request_id TEXT;

-- Phase 3: daily_tag_spend.request_id → call_id
ALTER TABLE daily_tag_spend RENAME COLUMN request_id TO call_id;

-- Phase 4: index on upstream request_id (reconciliation point-lookup)
CREATE INDEX IF NOT EXISTS idx_spend_logs_request_id ON spend_logs(request_id);
