-- 002_spend_logs.sql
-- Maps to LiteLLM_SpendLogs table (column-compatible)
-- SQLite syntax

CREATE TABLE IF NOT EXISTS spend_logs (
    request_id TEXT PRIMARY KEY,
    call_type TEXT NOT NULL,
    api_key TEXT NOT NULL,
    spend REAL NOT NULL DEFAULT 0.0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    start_time DATETIME NOT NULL,
    end_time DATETIME NOT NULL,
    request_duration_ms INTEGER,
    completion_start_time DATETIME,
    model TEXT NOT NULL,
    model_id TEXT,
    model_group TEXT,
    custom_llm_provider TEXT,
    api_base TEXT,
    "user" TEXT,
    metadata BLOB,
    cache_hit TEXT,
    cache_key TEXT,
    request_tags BLOB,
    team_id TEXT,
    organization_id TEXT,
    end_user TEXT,
    requester_ip_address TEXT,
    messages BLOB,
    response BLOB,
    session_id TEXT,
    status TEXT,
    mcp_namespaced_tool_name TEXT,
    agent_id TEXT,
    proxy_server_request BLOB
);
