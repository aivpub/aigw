-- 002_spend_logs.sql
-- Maps to LiteLLM_SpendLogs table (column-compatible)
-- PostgreSQL syntax

CREATE TABLE IF NOT EXISTS spend_logs (
    request_id TEXT NOT NULL,
    call_type TEXT NOT NULL,
    api_key TEXT NOT NULL,
    spend DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    start_time TIMESTAMPTZ(3) NOT NULL,
    end_time TIMESTAMPTZ(3) NOT NULL,
    request_duration_ms INTEGER,
    completion_start_time TIMESTAMPTZ(3),
    model TEXT NOT NULL,
    model_id TEXT,
    model_group TEXT,
    custom_llm_provider TEXT,
    api_base TEXT,
    "user" TEXT,
    metadata JSONB,
    cache_hit TEXT,
    cache_key TEXT,
    request_tags JSONB,
    team_id TEXT,
    organization_id TEXT,
    end_user TEXT,
    requester_ip_address TEXT,
    messages JSONB,
    response JSONB,
    session_id TEXT,
    status TEXT,
    mcp_namespaced_tool_name TEXT,
    agent_id TEXT,
    proxy_server_request JSONB,
    PRIMARY KEY (request_id)
);
