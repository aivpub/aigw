-- 006_projects.sql
-- Maps to LiteLLM_ProjectTable (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT NOT NULL,
    project_alias TEXT,
    description TEXT,
    team_id TEXT,
    budget_id TEXT,
    metadata BLOB NOT NULL,
    models BLOB NOT NULL,
    spend REAL NOT NULL DEFAULT 0.0,
    model_spend BLOB NOT NULL,
    model_rpm_limit BLOB NOT NULL,
    model_tpm_limit BLOB NOT NULL,
    blocked INTEGER NOT NULL DEFAULT 0,
    object_permission_id TEXT,
    created_at DATETIME NOT NULL,
    created_by TEXT NOT NULL,
    updated_at DATETIME NOT NULL,
    updated_by TEXT NOT NULL,
    PRIMARY KEY (project_id)
);
