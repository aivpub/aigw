-- 015_daily_spend: pre-aggregated daily spend tables (aligned with litellm LiteLLM_Daily*Spend)
-- These tables enable efficient Usage dashboard queries without scanning spend_logs.

CREATE TABLE IF NOT EXISTS daily_user_spend (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    model_group TEXT NOT NULL DEFAULT '',
    custom_llm_provider TEXT NOT NULL DEFAULT '',
    mcp_namespaced_tool_name TEXT NOT NULL DEFAULT '',
    endpoint TEXT NOT NULL DEFAULT '',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
    spend REAL NOT NULL DEFAULT 0.0,
    api_requests INTEGER NOT NULL DEFAULT 0,
    successful_requests INTEGER NOT NULL DEFAULT 0,
    failed_requests INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX IF NOT EXISTS idx_daily_user_spend_date ON daily_user_spend(date);
CREATE INDEX IF NOT EXISTS idx_daily_user_spend_user_date ON daily_user_spend(user_id, date);

CREATE TABLE IF NOT EXISTS daily_team_spend (
    id TEXT PRIMARY KEY,
    team_id TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    model_group TEXT NOT NULL DEFAULT '',
    custom_llm_provider TEXT NOT NULL DEFAULT '',
    mcp_namespaced_tool_name TEXT NOT NULL DEFAULT '',
    endpoint TEXT NOT NULL DEFAULT '',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
    spend REAL NOT NULL DEFAULT 0.0,
    api_requests INTEGER NOT NULL DEFAULT 0,
    successful_requests INTEGER NOT NULL DEFAULT 0,
    failed_requests INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(team_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX IF NOT EXISTS idx_daily_team_spend_date ON daily_team_spend(date);
CREATE INDEX IF NOT EXISTS idx_daily_team_spend_team_date ON daily_team_spend(team_id, date);

CREATE TABLE IF NOT EXISTS daily_organization_spend (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    model_group TEXT NOT NULL DEFAULT '',
    custom_llm_provider TEXT NOT NULL DEFAULT '',
    mcp_namespaced_tool_name TEXT NOT NULL DEFAULT '',
    endpoint TEXT NOT NULL DEFAULT '',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
    spend REAL NOT NULL DEFAULT 0.0,
    api_requests INTEGER NOT NULL DEFAULT 0,
    successful_requests INTEGER NOT NULL DEFAULT 0,
    failed_requests INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(organization_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX IF NOT EXISTS idx_daily_org_spend_date ON daily_organization_spend(date);
CREATE INDEX IF NOT EXISTS idx_daily_org_spend_org_date ON daily_organization_spend(organization_id, date);

CREATE TABLE IF NOT EXISTS daily_end_user_spend (
    id TEXT PRIMARY KEY,
    end_user_id TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    model_group TEXT NOT NULL DEFAULT '',
    custom_llm_provider TEXT NOT NULL DEFAULT '',
    mcp_namespaced_tool_name TEXT NOT NULL DEFAULT '',
    endpoint TEXT NOT NULL DEFAULT '',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
    spend REAL NOT NULL DEFAULT 0.0,
    api_requests INTEGER NOT NULL DEFAULT 0,
    successful_requests INTEGER NOT NULL DEFAULT 0,
    failed_requests INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(end_user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX IF NOT EXISTS idx_daily_end_user_spend_date ON daily_end_user_spend(date);

CREATE TABLE IF NOT EXISTS daily_agent_spend (
    id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    model_group TEXT NOT NULL DEFAULT '',
    custom_llm_provider TEXT NOT NULL DEFAULT '',
    mcp_namespaced_tool_name TEXT NOT NULL DEFAULT '',
    endpoint TEXT NOT NULL DEFAULT '',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
    spend REAL NOT NULL DEFAULT 0.0,
    api_requests INTEGER NOT NULL DEFAULT 0,
    successful_requests INTEGER NOT NULL DEFAULT 0,
    failed_requests INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(agent_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX IF NOT EXISTS idx_daily_agent_spend_date ON daily_agent_spend(date);

CREATE TABLE IF NOT EXISTS daily_tag_spend (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL DEFAULT '',
    tag TEXT NOT NULL DEFAULT '',
    date TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    model_group TEXT NOT NULL DEFAULT '',
    custom_llm_provider TEXT NOT NULL DEFAULT '',
    mcp_namespaced_tool_name TEXT NOT NULL DEFAULT '',
    endpoint TEXT NOT NULL DEFAULT '',
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_input_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_input_tokens INTEGER NOT NULL DEFAULT 0,
    spend REAL NOT NULL DEFAULT 0.0,
    api_requests INTEGER NOT NULL DEFAULT 0,
    successful_requests INTEGER NOT NULL DEFAULT 0,
    failed_requests INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(request_id, tag, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX IF NOT EXISTS idx_daily_tag_spend_date ON daily_tag_spend(date);
CREATE INDEX IF NOT EXISTS idx_daily_tag_spend_tag ON daily_tag_spend(tag);
