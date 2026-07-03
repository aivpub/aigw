-- 008_organization_memberships.sql
-- Maps to LiteLLM_OrganizationMembership (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS organization_memberships (
    user_id VARCHAR(255) NOT NULL,
    organization_id VARCHAR(255) NOT NULL,
    user_role VARCHAR(255),
    spend DOUBLE,
    budget_id VARCHAR(255),
    created_at DATETIME(3),
    updated_at DATETIME(3),
    PRIMARY KEY (user_id, organization_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
