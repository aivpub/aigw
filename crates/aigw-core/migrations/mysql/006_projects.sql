-- 006_projects.sql
-- Maps to LiteLLM_ProjectTable (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS projects (
    project_id VARCHAR(255) NOT NULL,
    project_alias VARCHAR(255),
    description TEXT,
    team_id VARCHAR(255),
    budget_id VARCHAR(255),
    metadata JSON NOT NULL,
    models JSON NOT NULL,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    model_spend JSON NOT NULL,
    model_rpm_limit JSON NOT NULL,
    model_tpm_limit JSON NOT NULL,
    blocked TINYINT(1) NOT NULL DEFAULT 0,
    object_permission_id VARCHAR(255),
    created_at DATETIME(3) NOT NULL,
    created_by VARCHAR(255) NOT NULL,
    updated_at DATETIME(3) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    PRIMARY KEY (project_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
