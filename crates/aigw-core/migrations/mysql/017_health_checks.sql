-- 017_health_checks.sql
-- Model health check table aligned with LiteLLM_HealthCheckTable.

CREATE TABLE IF NOT EXISTS health_checks (
    health_check_id VARCHAR(255) NOT NULL PRIMARY KEY,
    model_name      VARCHAR(255) NOT NULL,
    model_id        VARCHAR(255),
    status          VARCHAR(50) NOT NULL,
    healthy_count   INTEGER NOT NULL DEFAULT 0,
    unhealthy_count INTEGER NOT NULL DEFAULT 0,
    error_message   TEXT,
    response_time_ms DOUBLE,
    details         TEXT NOT NULL DEFAULT '{}',
    checked_by      VARCHAR(255),
    checked_at      VARCHAR(255) NOT NULL,
    created_at      VARCHAR(255) NOT NULL,
    updated_at      VARCHAR(255) NOT NULL
);

CREATE INDEX idx_health_checks_model_name ON health_checks(model_name);
CREATE INDEX idx_health_checks_checked_at ON health_checks(checked_at);
CREATE INDEX idx_health_checks_status ON health_checks(status);
