-- 024_deleted_tables.sql (SQLite)
--
-- Create four standalone archive/trash tables mirroring source table
-- columns plus an auto-increment id PK and a deleted_at timestamp.
-- Pattern: delete_key() tombstone-then-delete extended to teams,
-- users, organizations, and proxy_models.
--
-- Each archive table mirrors ALL columns from the source table so
-- every archived row is a complete, self-contained historical record.
-- The id column is an auto-increment PK because the source-table PK
-- (team_id / user_id / organization_id / model_id) may be reused
-- after deletion (delete → recreate → delete again → PK collision).

-- ━━━━ deleted_organizations ━━━━
CREATE TABLE IF NOT EXISTS deleted_organizations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    organization_id TEXT NOT NULL,
    organization_alias TEXT NOT NULL,
    budget_id TEXT NOT NULL,
    metadata BLOB NOT NULL,
    models BLOB NOT NULL,
    spend REAL NOT NULL DEFAULT 0.0,
    model_spend BLOB NOT NULL,
    object_permission_id TEXT,
    created_at DATETIME NOT NULL,
    created_by TEXT NOT NULL,
    updated_at DATETIME NOT NULL,
    updated_by TEXT NOT NULL,
    deleted_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_deleted_orgs_org_id ON deleted_organizations(organization_id);
CREATE INDEX IF NOT EXISTS idx_deleted_orgs_deleted_at ON deleted_organizations(deleted_at);

-- ━━━━ deleted_teams ━━━━
CREATE TABLE IF NOT EXISTS deleted_teams (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    team_id TEXT NOT NULL,
    team_alias TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    admins BLOB NOT NULL,
    members BLOB NOT NULL,
    members_with_roles BLOB NOT NULL,
    metadata BLOB NOT NULL,
    max_budget TEXT,
    soft_budget TEXT,
    spend REAL NOT NULL DEFAULT 0.0,
    models BLOB NOT NULL,
    max_parallel_requests TEXT,
    tpm_limit TEXT,
    rpm_limit TEXT,
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
    deleted_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_deleted_teams_team_id ON deleted_teams(team_id);
CREATE INDEX IF NOT EXISTS idx_deleted_teams_deleted_at ON deleted_teams(deleted_at);

-- ━━━━ deleted_users ━━━━
CREATE TABLE IF NOT EXISTS deleted_users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL,
    user_alias TEXT,
    team_id TEXT,
    sso_user_id TEXT,
    organization_id TEXT,
    object_permission_id TEXT,
    password TEXT,
    teams BLOB NOT NULL,
    user_role TEXT,
    max_budget TEXT,
    spend REAL NOT NULL DEFAULT 0.0,
    user_email TEXT,
    models BLOB NOT NULL,
    metadata BLOB NOT NULL,
    max_parallel_requests TEXT,
    tpm_limit TEXT,
    rpm_limit TEXT,
    budget_duration TEXT,
    budget_reset_at DATETIME,
    allowed_cache_controls BLOB NOT NULL,
    policies BLOB NOT NULL,
    model_spend BLOB NOT NULL,
    model_max_budget BLOB NOT NULL,
    created_at DATETIME,
    updated_at DATETIME,
    deleted_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_deleted_users_user_id ON deleted_users(user_id);
CREATE INDEX IF NOT EXISTS idx_deleted_users_deleted_at ON deleted_users(deleted_at);

-- ━━━━ deleted_models ━━━━
CREATE TABLE IF NOT EXISTS deleted_models (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id TEXT NOT NULL,
    model_name TEXT NOT NULL,
    litellm_params TEXT NOT NULL DEFAULT '{}',
    model_info TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT,
    deleted_at DATETIME NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_deleted_models_model_id ON deleted_models(model_id);
CREATE INDEX IF NOT EXISTS idx_deleted_models_deleted_at ON deleted_models(deleted_at);
