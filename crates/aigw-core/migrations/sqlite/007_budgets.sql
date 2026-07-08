-- 007_budgets.sql
-- Maps to LiteLLM_BudgetTable (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS budgets (
    budget_id TEXT NOT NULL,
    max_budget TEXT,
    soft_budget TEXT,
    max_parallel_requests INTEGER,
    tpm_limit INTEGER,
    rpm_limit INTEGER,
    model_max_budget BLOB NOT NULL,
    budget_duration TEXT,
    budget_reset_at DATETIME,
    allowed_models BLOB NOT NULL,
    created_at DATETIME NOT NULL,
    created_by TEXT NOT NULL,
    updated_at DATETIME NOT NULL,
    updated_by TEXT NOT NULL,
    PRIMARY KEY (budget_id)
);
