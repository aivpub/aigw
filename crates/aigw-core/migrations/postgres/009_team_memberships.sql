-- 009_team_memberships.sql
-- Maps to LiteLLM_TeamMembership (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS team_memberships (
    user_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    total_spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    budget_id TEXT,
    PRIMARY KEY (user_id, team_id)
);
