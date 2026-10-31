-- 034_mcp_call_log_user_index.sql (SQLite)
--
-- Index for the user-scoped call log (`GET /v1/mcp/calls`, Phase 56,
-- Stage 147). That query always constrains `user_id` by equality and orders /
-- ranges on `created_at`; neither 031 index leads with `user_id`, so both would
-- fall back to a full scan once the table grows.
--
-- Column order matters: the leading column must be the equality predicate, so
-- `created_at` still gets a range scan and the sort it already produces. No
-- DESC variant is needed — SQLite scans an index backwards just as happily.
--
-- `user_id` IS NULL (master key, or a key with no bound user) is not indexed
-- usefully, but those rows are admin-only by design and never queried per user.

CREATE INDEX IF NOT EXISTS idx_mcp_call_log_user_created
    ON mcp_call_log(user_id, created_at);
