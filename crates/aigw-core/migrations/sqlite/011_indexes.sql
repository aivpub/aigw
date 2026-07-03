-- 011_indexes.sql
-- Performance indexes for the aigw schema
-- SQLite syntax

CREATE INDEX IF NOT EXISTS idx_spend_logs_start_time ON spend_logs(start_time);
CREATE INDEX IF NOT EXISTS idx_spend_logs_api_key ON spend_logs(api_key);
CREATE INDEX IF NOT EXISTS idx_spend_logs_user_team ON spend_logs("user", team_id);
CREATE INDEX IF NOT EXISTS idx_spend_logs_session_id ON spend_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_virtual_keys_budget_reset ON virtual_keys(budget_reset_at, expires);
