-- 006_projects.sql
-- Maps to LiteLLM_ProjectTable (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT NOT NULL,
    project_alias TEXT,
    description TEXT,
    team_id TEXT,
    budget_id TEXT,
    metadata JSONB NOT NULL,
    models JSONB NOT NULL,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    model_spend JSONB NOT NULL,
    model_rpm_limit JSONB NOT NULL,
    model_tpm_limit JSONB NOT NULL,
    blocked BOOLEAN NOT NULL DEFAULT FALSE,
    object_permission_id TEXT,
    created_at TIMESTAMPTZ(3) NOT NULL,
    created_by TEXT NOT NULL,
    updated_at TIMESTAMPTZ(3) NOT NULL,
    updated_by TEXT NOT NULL,
    PRIMARY KEY (project_id)
);
