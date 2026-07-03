-- 007_budgets.sql
-- Maps to LiteLLM_BudgetTable (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS budgets (
    budget_id TEXT NOT NULL,
    max_budget DOUBLE PRECISION,
    soft_budget DOUBLE PRECISION,
    max_parallel_requests INTEGER,
    tpm_limit BIGINT,
    rpm_limit BIGINT,
    model_max_budget JSONB NOT NULL,
    budget_duration TEXT,
    budget_reset_at TIMESTAMPTZ(3),
    allowed_models JSONB NOT NULL,
    created_at TIMESTAMPTZ(3) NOT NULL,
    created_by TEXT NOT NULL,
    updated_at TIMESTAMPTZ(3) NOT NULL,
    updated_by TEXT NOT NULL,
    PRIMARY KEY (budget_id)
);
