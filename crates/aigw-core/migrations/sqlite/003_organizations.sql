-- 003_organizations.sql
-- Maps to LiteLLM_OrganizationTable (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS organizations (
    organization_id TEXT NOT NULL,
    organization_alias TEXT NOT NULL,
    budget_id TEXT NOT NULL,
    metadata BLOB NOT NULL,
    models BLOB NOT NULL,
    spend REAL NOT NULL DEFAULT 0.0,
    model_spend BLOB NOT NULL,
    object_permission_id TEXT,
    created_at DATETIME NOT NULL,
    created_by TEXT NOT NULL,
    updated_at DATETIME NOT NULL,
    updated_by TEXT NOT NULL,
    PRIMARY KEY (organization_id)
);
