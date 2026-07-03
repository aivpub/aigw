-- 009_team_memberships.sql
-- Maps to LiteLLM_TeamMembership (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS team_memberships (
    user_id TEXT NOT NULL,
    team_id TEXT NOT NULL,
    spend REAL NOT NULL DEFAULT 0.0,
    total_spend REAL NOT NULL DEFAULT 0.0,
    budget_id TEXT,
    PRIMARY KEY (user_id, team_id)
);
