-- 004_teams.sql
-- Maps to LiteLLM_TeamTable (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS teams (
    team_id VARCHAR(255) NOT NULL,
    team_alias VARCHAR(255),
    organization_id VARCHAR(255),
    object_permission_id VARCHAR(255),
    admins JSON NOT NULL,
    members JSON NOT NULL,
    members_with_roles JSON NOT NULL,
    metadata JSON NOT NULL,
    max_budget TEXT,
    soft_budget TEXT,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    models JSON NOT NULL,
    max_parallel_requests INTEGER,
    tpm_limit BIGINT,
    rpm_limit BIGINT,
    budget_duration VARCHAR(255),
    budget_reset_at DATETIME(3),
    blocked TINYINT(1) NOT NULL DEFAULT 0,
    created_at DATETIME(3) NOT NULL,
    updated_at DATETIME(3) NOT NULL,
    model_spend JSON NOT NULL,
    model_max_budget JSON NOT NULL,
    router_settings JSON,
    team_member_permissions JSON NOT NULL,
    access_group_ids JSON NOT NULL,
    policies JSON NOT NULL,
    default_team_member_models JSON NOT NULL,
    budget_limits JSON,
    model_id INTEGER,
    allow_team_guardrail_config TINYINT(1) NOT NULL DEFAULT 0,
    PRIMARY KEY (team_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
