-- 034_mcp_call_log_user_index.sql (PostgreSQL)
--
-- Index for the user-scoped call log (`GET /v1/mcp/calls`, Phase 56,
-- Stage 147). See the SQLite variant for the rationale: equality on `user_id`
-- first so `created_at` still gets a range scan and needs no separate sort.
--
-- B-tree indexes are scanned backwards, so `created_at DESC` in the query is
-- served by this index without a DESC keyword.

CREATE INDEX IF NOT EXISTS idx_mcp_call_log_user_created
    ON mcp_call_log(user_id, created_at);
