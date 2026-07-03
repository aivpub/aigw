-- 005_users.sql
-- Maps to LiteLLM_UserTable (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS users (
    user_id TEXT NOT NULL,
    user_alias TEXT,
    team_id TEXT,
    sso_user_id TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    password TEXT,
    teams BLOB NOT NULL,
    user_role TEXT,
    max_budget REAL,
    spend REAL NOT NULL DEFAULT 0.0,
    user_email TEXT,
    models BLOB NOT NULL,
    metadata BLOB NOT NULL,
    max_parallel_requests INTEGER,
    tpm_limit INTEGER,
    rpm_limit INTEGER,
    budget_duration TEXT,
    budget_reset_at DATETIME,
    allowed_cache_controls BLOB NOT NULL,
    policies BLOB NOT NULL,
    model_spend BLOB NOT NULL,
    model_max_budget BLOB NOT NULL,
    created_at DATETIME,
    updated_at DATETIME,
    PRIMARY KEY (user_id)
);
