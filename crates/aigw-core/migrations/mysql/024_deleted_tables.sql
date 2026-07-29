-- 024_deleted_tables.sql (MySQL)
--
-- Create four standalone archive/trash tables mirroring source table
-- columns plus an auto-increment id PK and a deleted_at timestamp.
-- Pattern: delete_key() tombstone-then-delete extended to teams,
-- users, organizations, and proxy_models.

-- ━━━━ deleted_organizations ━━━━
CREATE TABLE IF NOT EXISTS deleted_organizations (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    organization_id VARCHAR(255) NOT NULL,
    organization_alias VARCHAR(255) NOT NULL,
    budget_id VARCHAR(255) NOT NULL,
    metadata JSON,
    models JSON,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    model_spend JSON,
    object_permission_id VARCHAR(255),
    created_at DATETIME(3) NOT NULL,
    created_by VARCHAR(255) NOT NULL,
    updated_at DATETIME(3) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    deleted_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_deleted_orgs_org_id (organization_id),
    INDEX idx_deleted_orgs_deleted_at (deleted_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ━━━━ deleted_teams ━━━━
CREATE TABLE IF NOT EXISTS deleted_teams (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    team_id VARCHAR(255) NOT NULL,
    team_alias VARCHAR(255),
    organization_id VARCHAR(255),
    object_permission_id VARCHAR(255),
    admins JSON,
    members JSON,
    members_with_roles JSON,
    metadata JSON,
    max_budget VARCHAR(255),
    soft_budget VARCHAR(255),
    spend DOUBLE NOT NULL DEFAULT 0.0,
    models JSON,
    max_parallel_requests VARCHAR(255),
    tpm_limit VARCHAR(255),
    rpm_limit VARCHAR(255),
    budget_duration VARCHAR(255),
    budget_reset_at DATETIME(3),
    blocked TINYINT(1) NOT NULL DEFAULT 0,
    created_at DATETIME(3) NOT NULL,
    updated_at DATETIME(3) NOT NULL,
    model_spend JSON,
    model_max_budget JSON,
    router_settings JSON,
    team_member_permissions JSON,
    access_group_ids JSON,
    policies JSON,
    default_team_member_models JSON,
    budget_limits JSON,
    model_id INT,
    allow_team_guardrail_config TINYINT(1) NOT NULL DEFAULT 0,
    deleted_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_deleted_teams_team_id (team_id),
    INDEX idx_deleted_teams_deleted_at (deleted_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ━━━━ deleted_users ━━━━
CREATE TABLE IF NOT EXISTS deleted_users (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL,
    user_alias VARCHAR(255),
    team_id VARCHAR(255),
    sso_user_id VARCHAR(255),
    organization_id VARCHAR(255),
    object_permission_id VARCHAR(255),
    password VARCHAR(255),
    teams JSON,
    user_role VARCHAR(255),
    max_budget VARCHAR(255),
    spend DOUBLE NOT NULL DEFAULT 0.0,
    user_email VARCHAR(255),
    models JSON,
    metadata JSON,
    max_parallel_requests VARCHAR(255),
    tpm_limit VARCHAR(255),
    rpm_limit VARCHAR(255),
    budget_duration VARCHAR(255),
    budget_reset_at DATETIME(3),
    allowed_cache_controls JSON,
    policies JSON,
    model_spend JSON,
    model_max_budget JSON,
    created_at DATETIME(3),
    updated_at DATETIME(3),
    deleted_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_deleted_users_user_id (user_id),
    INDEX idx_deleted_users_deleted_at (deleted_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

-- ━━━━ deleted_models ━━━━
CREATE TABLE IF NOT EXISTS deleted_models (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    model_id VARCHAR(255) NOT NULL,
    model_name VARCHAR(255) NOT NULL,
    litellm_params JSON,
    model_info JSON,
    created_at VARCHAR(255) NOT NULL DEFAULT '',
    created_by VARCHAR(255),
    updated_at VARCHAR(255) NOT NULL DEFAULT '',
    updated_by VARCHAR(255),
    deleted_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    INDEX idx_deleted_models_model_id (model_id),
    INDEX idx_deleted_models_deleted_at (deleted_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
