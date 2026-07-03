-- 003_organizations.sql
-- Maps to LiteLLM_OrganizationTable (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS organizations (
    organization_id TEXT NOT NULL,
    organization_alias TEXT NOT NULL,
    budget_id TEXT NOT NULL,
    metadata JSONB NOT NULL,
    models JSONB NOT NULL,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    model_spend JSONB NOT NULL,
    object_permission_id TEXT,
    created_at TIMESTAMPTZ(3) NOT NULL,
    created_by TEXT NOT NULL,
    updated_at TIMESTAMPTZ(3) NOT NULL,
    updated_by TEXT NOT NULL,
    PRIMARY KEY (organization_id)
);
