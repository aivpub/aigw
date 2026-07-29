-- 024_deleted_tables.sql (PostgreSQL)
--
-- Create four standalone archive/trash tables mirroring source table
-- columns plus an auto-increment id PK and a deleted_at timestamp.
-- Pattern: delete_key() tombstone-then-delete extended to teams,
-- users, organizations, and proxy_models.

-- ━━━━ deleted_organizations ━━━━
CREATE TABLE IF NOT EXISTS deleted_organizations (
    id BIGSERIAL PRIMARY KEY,
    organization_id TEXT NOT NULL,
    organization_alias TEXT NOT NULL,
    budget_id TEXT NOT NULL,
    metadata JSONB,
    models JSONB,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    model_spend JSONB,
    object_permission_id TEXT,
    created_at TIMESTAMPTZ(3) NOT NULL,
    created_by TEXT NOT NULL,
    updated_at TIMESTAMPTZ(3) NOT NULL,
    updated_by TEXT NOT NULL,
    deleted_at TIMESTAMPTZ(3) NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_deleted_orgs_org_id ON deleted_organizations(organization_id);
CREATE INDEX IF NOT EXISTS idx_deleted_orgs_deleted_at ON deleted_organizations(deleted_at);

-- ━━━━ deleted_teams ━━━━
CREATE TABLE IF NOT EXISTS deleted_teams (
    id BIGSERIAL PRIMARY KEY,
    team_id TEXT NOT NULL,
    team_alias TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    admins JSONB,
    members JSONB,
    members_with_roles JSONB,
    metadata JSONB,
    max_budget TEXT,
    soft_budget TEXT,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    models JSONB,
    max_parallel_requests TEXT,
    tpm_limit TEXT,
    rpm_limit TEXT,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ(3),
    blocked BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ(3) NOT NULL,
    updated_at TIMESTAMPTZ(3) NOT NULL,
    model_spend JSONB,
    model_max_budget JSONB,
    router_settings JSONB,
    team_member_permissions JSONB,
    access_group_ids JSONB,
    policies JSONB,
    default_team_member_models JSONB,
    budget_limits JSONB,
    model_id INTEGER,
    allow_team_guardrail_config BOOLEAN NOT NULL DEFAULT false,
    deleted_at TIMESTAMPTZ(3) NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_deleted_teams_team_id ON deleted_teams(team_id);
CREATE INDEX IF NOT EXISTS idx_deleted_teams_deleted_at ON deleted_teams(deleted_at);

-- ━━━━ deleted_users ━━━━
CREATE TABLE IF NOT EXISTS deleted_users (
    id BIGSERIAL PRIMARY KEY,
    user_id TEXT NOT NULL,
    user_alias TEXT,
    team_id TEXT,
    sso_user_id TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    password TEXT,
    teams JSONB,
    user_role TEXT,
    max_budget TEXT,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    user_email TEXT,
    models JSONB,
    metadata JSONB,
    max_parallel_requests TEXT,
    tpm_limit TEXT,
    rpm_limit TEXT,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ(3),
    allowed_cache_controls JSONB,
    policies JSONB,
    model_spend JSONB,
    model_max_budget JSONB,
    created_at TIMESTAMPTZ(3),
    updated_at TIMESTAMPTZ(3),
    deleted_at TIMESTAMPTZ(3) NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_deleted_users_user_id ON deleted_users(user_id);
CREATE INDEX IF NOT EXISTS idx_deleted_users_deleted_at ON deleted_users(deleted_at);

-- ━━━━ deleted_models ━━━━
CREATE TABLE IF NOT EXISTS deleted_models (
    id BIGSERIAL PRIMARY KEY,
    model_id TEXT NOT NULL,
    model_name TEXT NOT NULL,
    litellm_params JSONB,
    model_info JSONB,
    created_at TEXT NOT NULL DEFAULT '',
    created_by TEXT,
    updated_at TEXT NOT NULL DEFAULT '',
    updated_by TEXT,
    deleted_at TIMESTAMPTZ(3) NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_deleted_models_model_id ON deleted_models(model_id);
CREATE INDEX IF NOT EXISTS idx_deleted_models_deleted_at ON deleted_models(deleted_at);
