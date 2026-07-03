-- 004_teams.sql
-- Maps to LiteLLM_TeamTable (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS teams (
    team_id TEXT NOT NULL,
    team_alias TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    admins BLOB NOT NULL,
    members BLOB NOT NULL,
    members_with_roles BLOB NOT NULL,
    metadata BLOB NOT NULL,
    max_budget REAL,
    soft_budget REAL,
    spend REAL NOT NULL DEFAULT 0.0,
    models BLOB NOT NULL,
    max_parallel_requests INTEGER,
    tpm_limit INTEGER,
    rpm_limit INTEGER,
    budget_duration TEXT,
    budget_reset_at DATETIME,
    blocked INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    model_spend BLOB NOT NULL,
    model_max_budget BLOB NOT NULL,
    router_settings BLOB,
    team_member_permissions BLOB NOT NULL,
    access_group_ids BLOB NOT NULL,
    policies BLOB NOT NULL,
    default_team_member_models BLOB NOT NULL,
    budget_limits BLOB,
    model_id INTEGER,
    allow_team_guardrail_config INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (team_id)
);
