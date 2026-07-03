-- 009_team_memberships.sql
-- Maps to LiteLLM_TeamMembership (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS team_memberships (
    user_id VARCHAR(255) NOT NULL,
    team_id VARCHAR(255) NOT NULL,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    total_spend DOUBLE NOT NULL DEFAULT 0.0,
    budget_id VARCHAR(255),
    PRIMARY KEY (user_id, team_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
