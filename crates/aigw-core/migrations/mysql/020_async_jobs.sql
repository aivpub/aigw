-- 020_async_jobs.sql (MySQL)
-- General-purpose async job framework tables for aigw.

CREATE TABLE IF NOT EXISTS async_jobs (
    id VARCHAR(255) PRIMARY KEY,
    step_type VARCHAR(255) NOT NULL,
    trigger_type VARCHAR(50) NOT NULL,
    triggered_by VARCHAR(255),
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_steps INTEGER NOT NULL DEFAULT 0,
    completed_steps INTEGER NOT NULL DEFAULT 0,
    failed_steps INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    max_retries INTEGER NOT NULL DEFAULT 3,
    started_at DATETIME(3),
    completed_at DATETIME(3),
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS async_job_steps (
    id VARCHAR(255) PRIMARY KEY,
    job_id VARCHAR(255) NOT NULL,
    step_key VARCHAR(255) NOT NULL,
    step_type VARCHAR(255) NOT NULL,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    payload JSON DEFAULT NULL,
    result JSON DEFAULT NULL,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at DATETIME(3),
    completed_at DATETIME(3),
    UNIQUE INDEX uq_job_step (job_id, step_key),
    FOREIGN KEY (job_id) REFERENCES async_jobs(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE TABLE IF NOT EXISTS async_job_logs (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    job_id VARCHAR(255) NOT NULL,
    step_key VARCHAR(255),
    level VARCHAR(20) NOT NULL DEFAULT 'info',
    message TEXT NOT NULL,
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    FOREIGN KEY (job_id) REFERENCES async_jobs(id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

CREATE INDEX idx_async_jobs_status ON async_jobs(status);
CREATE INDEX idx_async_jobs_type ON async_jobs(step_type, status);
CREATE INDEX idx_async_job_steps_claim ON async_job_steps(step_type, status, step_key);
CREATE INDEX idx_async_job_logs_job ON async_job_logs(job_id, created_at);
