-- 034_mcp_call_log_user_index.sql (MySQL)
--
-- Index for the user-scoped call log (`GET /v1/mcp/calls`, Phase 56,
-- Stage 147). See the SQLite variant for the rationale: equality on `user_id`
-- first so `created_at` still gets a range scan and needs no separate sort.
--
-- `user_id` is VARCHAR(255) utf8mb4 = 1020 bytes on its own, but InnoDB's index
-- limit applies per key part (3072 bytes with DYNAMIC row format), so no prefix
-- length is required.
--
-- No `IF NOT EXISTS`: MySQL does not support it on `CREATE INDEX` (the SQLite
-- and PostgreSQL variants do). The omission is the dialect's, not an oversight.

CREATE INDEX idx_mcp_call_log_user_created
    ON mcp_call_log(user_id, created_at);
