-- 005_users.sql
-- Maps to LiteLLM_UserTable (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS users (
    user_id TEXT NOT NULL,
    user_alias TEXT,
    team_id TEXT,
    sso_user_id TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    password TEXT,
    teams JSONB NOT NULL,
    user_role TEXT,
    max_budget DOUBLE PRECISION,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    user_email TEXT,
    models JSONB NOT NULL,
    metadata JSONB NOT NULL,
    max_parallel_requests INTEGER,
    tpm_limit BIGINT,
    rpm_limit BIGINT,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ(3),
    allowed_cache_controls JSONB NOT NULL,
    policies JSONB NOT NULL,
    model_spend JSONB NOT NULL,
    model_max_budget JSONB NOT NULL,
    created_at TIMESTAMPTZ(3),
    updated_at TIMESTAMPTZ(3),
    PRIMARY KEY (user_id)
);
