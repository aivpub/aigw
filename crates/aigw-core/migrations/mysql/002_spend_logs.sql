-- 002_spend_logs.sql
-- Maps to LiteLLM_SpendLogs table (column-compatible)
-- MySQL syntax

CREATE TABLE IF NOT EXISTS spend_logs (
    request_id VARCHAR(255) NOT NULL,
    call_type VARCHAR(255) NOT NULL,
    api_key VARCHAR(255) NOT NULL,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    start_time DATETIME(3) NOT NULL,
    end_time DATETIME(3) NOT NULL,
    request_duration_ms INTEGER,
    completion_start_time DATETIME(3),
    model VARCHAR(255) NOT NULL,
    model_id VARCHAR(255),
    model_group VARCHAR(255),
    custom_llm_provider VARCHAR(255),
    api_base VARCHAR(255),
    user VARCHAR(255),
    metadata JSON,
    cache_hit VARCHAR(255),
    cache_key VARCHAR(255),
    request_tags JSON,
    team_id VARCHAR(255),
    organization_id VARCHAR(255),
    end_user VARCHAR(255),
    requester_ip_address VARCHAR(255),
    messages JSON,
    response JSON,
    session_id VARCHAR(255),
    status VARCHAR(255),
    mcp_namespaced_tool_name VARCHAR(255),
    agent_id VARCHAR(255),
    proxy_server_request JSON,
    PRIMARY KEY (request_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
