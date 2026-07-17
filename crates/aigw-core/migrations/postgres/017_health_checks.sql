-- 017_health_checks.sql
-- Model health check table aligned with LiteLLM_HealthCheckTable.

CREATE TABLE IF NOT EXISTS health_checks (
    health_check_id TEXT NOT NULL PRIMARY KEY,
    model_name      TEXT NOT NULL,
    model_id        TEXT,
    status          TEXT NOT NULL,
    healthy_count   INTEGER NOT NULL DEFAULT 0,
    unhealthy_count INTEGER NOT NULL DEFAULT 0,
    error_message   TEXT,
    response_time_ms DOUBLE PRECISION,
    details         TEXT NOT NULL DEFAULT '{}',
    checked_by      TEXT,
    checked_at      TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_health_checks_model_name ON health_checks(model_name);
CREATE INDEX IF NOT EXISTS idx_health_checks_checked_at ON health_checks(checked_at);
CREATE INDEX IF NOT EXISTS idx_health_checks_status ON health_checks(status);
