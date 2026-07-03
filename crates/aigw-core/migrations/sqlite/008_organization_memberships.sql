-- 008_organization_memberships.sql
-- Maps to LiteLLM_OrganizationMembership (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS organization_memberships (
    user_id TEXT NOT NULL,
    organization_id TEXT NOT NULL,
    user_role TEXT,
    spend REAL,
    budget_id TEXT,
    created_at DATETIME,
    updated_at DATETIME,
    PRIMARY KEY (user_id, organization_id)
);
