-- 020_async_jobs.sql (PostgreSQL)
-- General-purpose async job framework tables for aigw.

CREATE TABLE IF NOT EXISTS async_jobs (
    id TEXT PRIMARY KEY,
    step_type TEXT NOT NULL,
    trigger_type TEXT NOT NULL,
    triggered_by TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    total_steps INTEGER NOT NULL DEFAULT 0,
    completed_steps INTEGER NOT NULL DEFAULT 0,
    failed_steps INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    max_retries INTEGER NOT NULL DEFAULT 3,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS async_job_steps (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES async_jobs(id),
    step_key TEXT NOT NULL,
    step_type TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    payload JSONB DEFAULT '{}',
    result JSONB DEFAULT '{}',
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    UNIQUE(job_id, step_key)
);

CREATE TABLE IF NOT EXISTS async_job_logs (
    id SERIAL PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES async_jobs(id),
    step_key TEXT,
    level TEXT NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_async_jobs_status ON async_jobs(status);
CREATE INDEX IF NOT EXISTS idx_async_jobs_type ON async_jobs(step_type, status);
CREATE INDEX IF NOT EXISTS idx_async_job_steps_claim ON async_job_steps(step_type, status, step_key);
CREATE INDEX IF NOT EXISTS idx_async_job_logs_job ON async_job_logs(job_id, created_at);
