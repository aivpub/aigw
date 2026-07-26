-- 020_async_jobs.sql (MySQL)
--
-- Async job framework: async_jobs / async_job_steps / async_job_logs.
CREATE TABLE IF NOT EXISTS async_jobs (
    id VARCHAR(255) NOT NULL PRIMARY KEY,
    step_type VARCHAR(255) NOT NULL,
    trigger_type VARCHAR(64) NOT NULL,        -- 'cron' | 'manual'
    triggered_by VARCHAR(255),
    status VARCHAR(64) NOT NULL DEFAULT 'pending',
    total_steps INTEGER NOT NULL DEFAULT 0,
    completed_steps INTEGER NOT NULL DEFAULT 0,
    failed_steps INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    max_retries INTEGER NOT NULL DEFAULT 3,
    started_at VARCHAR(64),
    completed_at VARCHAR(64),
    created_at VARCHAR(64) NOT NULL,
    updated_at VARCHAR(64) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX idx_async_jobs_status ON async_jobs(status);
CREATE INDEX idx_async_jobs_type ON async_jobs(step_type, status);

CREATE TABLE IF NOT EXISTS async_job_steps (
    id VARCHAR(255) NOT NULL PRIMARY KEY,
    job_id VARCHAR(255) NOT NULL,
    step_key VARCHAR(255) NOT NULL,
    step_type VARCHAR(255) NOT NULL,
    status VARCHAR(64) NOT NULL DEFAULT 'pending',
    payload JSON,
    result JSON,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at VARCHAR(64),
    completed_at VARCHAR(64),
    next_retry_at VARCHAR(64),
    UNIQUE KEY uq_async_job_steps (job_id, step_key),
    CONSTRAINT fk_async_job_steps_job FOREIGN KEY (job_id) REFERENCES async_jobs(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX idx_async_job_steps_claim ON async_job_steps(step_type, status, step_key);

CREATE TABLE IF NOT EXISTS async_job_logs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    job_id VARCHAR(255) NOT NULL,
    step_key VARCHAR(255),
    level VARCHAR(32) NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at VARCHAR(64) NOT NULL,
    CONSTRAINT fk_async_job_logs_job FOREIGN KEY (job_id) REFERENCES async_jobs(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
CREATE INDEX idx_async_job_logs_job ON async_job_logs(job_id, created_at);
