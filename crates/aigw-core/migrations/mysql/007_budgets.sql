-- 007_budgets.sql
-- Maps to LiteLLM_BudgetTable (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS budgets (
    budget_id VARCHAR(255) NOT NULL,
    max_budget TEXT,
    soft_budget TEXT,
    max_parallel_requests INTEGER,
    tpm_limit BIGINT,
    rpm_limit BIGINT,
    model_max_budget JSON NOT NULL,
    budget_duration VARCHAR(255),
    budget_reset_at DATETIME(3),
    allowed_models JSON NOT NULL,
    created_at DATETIME(3) NOT NULL,
    created_by VARCHAR(255) NOT NULL,
    updated_at DATETIME(3) NOT NULL,
    updated_by VARCHAR(255) NOT NULL,
    PRIMARY KEY (budget_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
