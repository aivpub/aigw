-- 008_organization_memberships.sql
-- Maps to LiteLLM_OrganizationMembership (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS organization_memberships (
    user_id TEXT NOT NULL,
    organization_id TEXT NOT NULL,
    user_role TEXT,
    spend DOUBLE PRECISION,
    budget_id TEXT,
    created_at TIMESTAMPTZ(3),
    updated_at TIMESTAMPTZ(3),
    PRIMARY KEY (user_id, organization_id)
);
