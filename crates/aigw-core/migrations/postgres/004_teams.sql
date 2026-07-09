-- 004_teams.sql
-- Maps to LiteLLM_TeamTable (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS teams (
    team_id TEXT NOT NULL,
    team_alias TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    admins JSONB NOT NULL,
    members JSONB NOT NULL,
    members_with_roles JSONB NOT NULL,
    metadata JSONB NOT NULL,
    max_budget TEXT,
    soft_budget TEXT,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    models JSONB NOT NULL,
    max_parallel_requests TEXT,
    tpm_limit TEXT,
    rpm_limit TEXT,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ(3),
    blocked BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ(3) NOT NULL,
    updated_at TIMESTAMPTZ(3) NOT NULL,
    model_spend JSONB NOT NULL,
    model_max_budget JSONB NOT NULL,
    router_settings JSONB,
    team_member_permissions JSONB NOT NULL,
    access_group_ids JSONB NOT NULL,
    policies JSONB NOT NULL,
    default_team_member_models JSONB NOT NULL,
    budget_limits JSONB,
    model_id INTEGER,
    allow_team_guardrail_config BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (team_id)
);
