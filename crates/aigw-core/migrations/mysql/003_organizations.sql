-- 003_organizations.sql
-- Maps to LiteLLM_OrganizationTable (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS organizations (
    organization_id VARCHAR(255) NOT NULL,
    organization_alias VARCHAR(255) NOT NULL,
    budget_id VARCHAR(255) NOT NULL,
    metadata JSON NOT NULL,
    models JSON NOT NULL,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    model_spend JSON NOT NULL,
    object_permission_id VARCHAR(255),
    created_at DATETIME(3) NOT NULL,
    created_by VARCHAR(255) NOT NULL,
    updated_at DATETIME(3) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    PRIMARY KEY (organization_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
