-- 005_users.sql
-- Maps to LiteLLM_UserTable (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS users (
    user_id VARCHAR(255) NOT NULL,
    user_alias VARCHAR(255),
    team_id VARCHAR(255),
    sso_user_id VARCHAR(255),
    organization_id VARCHAR(255),
    object_permission_id VARCHAR(255),
    password VARCHAR(255),
    teams JSON NOT NULL,
    user_role VARCHAR(255),
    max_budget TEXT,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    user_email VARCHAR(255),
    models JSON NOT NULL,
    metadata JSON NOT NULL,
    max_parallel_requests TEXT,
    tpm_limit TEXT,
    rpm_limit TEXT,
    budget_duration VARCHAR(255),
    budget_reset_at DATETIME(3),
    allowed_cache_controls JSON NOT NULL,
    policies JSON NOT NULL,
    model_spend JSON NOT NULL,
    model_max_budget JSON NOT NULL,
    created_at DATETIME(3),
    updated_at DATETIME(3),
    PRIMARY KEY (user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
