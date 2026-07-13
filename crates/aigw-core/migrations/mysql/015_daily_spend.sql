-- 015_daily_spend: pre-aggregated daily spend tables (aligned with litellm LiteLLM_Daily*Spend)
-- These tables enable efficient Usage dashboard queries without scanning spend_logs.
--
-- MySQL note: UNIQUE KEY columns are capped at VARCHAR(128) or shorter to stay
-- within the 3072-byte index limit (utf8mb4 × 4 bytes/char × ~8 cols ≈ 2488 < 3072).

CREATE TABLE IF NOT EXISTS daily_user_spend (
    id VARCHAR(255) PRIMARY KEY,
    user_id VARCHAR(255) NOT NULL DEFAULT '',
    date VARCHAR(10) NOT NULL,
    api_key VARCHAR(128) NOT NULL DEFAULT '',
    model VARCHAR(64) NOT NULL DEFAULT '',
    model_group VARCHAR(64) NOT NULL DEFAULT '',
    custom_llm_provider VARCHAR(32) NOT NULL DEFAULT '',
    mcp_namespaced_tool_name VARCHAR(128) NOT NULL DEFAULT '',
    endpoint VARCHAR(64) NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_input_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_input_tokens BIGINT NOT NULL DEFAULT 0,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    api_requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_daily_user (user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_user_spend_date ON daily_user_spend(date);
CREATE INDEX idx_daily_user_spend_user_date ON daily_user_spend(user_id, date);

CREATE TABLE IF NOT EXISTS daily_team_spend (
    id VARCHAR(255) PRIMARY KEY,
    team_id VARCHAR(255) NOT NULL DEFAULT '',
    date VARCHAR(10) NOT NULL,
    api_key VARCHAR(128) NOT NULL DEFAULT '',
    model VARCHAR(64) NOT NULL DEFAULT '',
    model_group VARCHAR(64) NOT NULL DEFAULT '',
    custom_llm_provider VARCHAR(32) NOT NULL DEFAULT '',
    mcp_namespaced_tool_name VARCHAR(128) NOT NULL DEFAULT '',
    endpoint VARCHAR(64) NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_input_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_input_tokens BIGINT NOT NULL DEFAULT 0,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    api_requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_daily_team (team_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_team_spend_date ON daily_team_spend(date);
CREATE INDEX idx_daily_team_spend_team_date ON daily_team_spend(team_id, date);

CREATE TABLE IF NOT EXISTS daily_organization_spend (
    id VARCHAR(255) PRIMARY KEY,
    organization_id VARCHAR(255) NOT NULL DEFAULT '',
    date VARCHAR(10) NOT NULL,
    api_key VARCHAR(128) NOT NULL DEFAULT '',
    model VARCHAR(64) NOT NULL DEFAULT '',
    model_group VARCHAR(64) NOT NULL DEFAULT '',
    custom_llm_provider VARCHAR(32) NOT NULL DEFAULT '',
    mcp_namespaced_tool_name VARCHAR(128) NOT NULL DEFAULT '',
    endpoint VARCHAR(64) NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_input_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_input_tokens BIGINT NOT NULL DEFAULT 0,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    api_requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_daily_org (organization_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_org_spend_date ON daily_organization_spend(date);
CREATE INDEX idx_daily_org_spend_org_date ON daily_organization_spend(organization_id, date);

CREATE TABLE IF NOT EXISTS daily_end_user_spend (
    id VARCHAR(255) PRIMARY KEY,
    end_user_id VARCHAR(255) NOT NULL DEFAULT '',
    date VARCHAR(10) NOT NULL,
    api_key VARCHAR(128) NOT NULL DEFAULT '',
    model VARCHAR(64) NOT NULL DEFAULT '',
    model_group VARCHAR(64) NOT NULL DEFAULT '',
    custom_llm_provider VARCHAR(32) NOT NULL DEFAULT '',
    mcp_namespaced_tool_name VARCHAR(128) NOT NULL DEFAULT '',
    endpoint VARCHAR(64) NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_input_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_input_tokens BIGINT NOT NULL DEFAULT 0,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    api_requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_daily_end_user (end_user_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_end_user_spend_date ON daily_end_user_spend(date);

CREATE TABLE IF NOT EXISTS daily_agent_spend (
    id VARCHAR(255) PRIMARY KEY,
    agent_id VARCHAR(255) NOT NULL DEFAULT '',
    date VARCHAR(10) NOT NULL,
    api_key VARCHAR(128) NOT NULL DEFAULT '',
    model VARCHAR(64) NOT NULL DEFAULT '',
    model_group VARCHAR(64) NOT NULL DEFAULT '',
    custom_llm_provider VARCHAR(32) NOT NULL DEFAULT '',
    mcp_namespaced_tool_name VARCHAR(128) NOT NULL DEFAULT '',
    endpoint VARCHAR(64) NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_input_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_input_tokens BIGINT NOT NULL DEFAULT 0,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    api_requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_daily_agent (agent_id, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_agent_spend_date ON daily_agent_spend(date);

CREATE TABLE IF NOT EXISTS daily_tag_spend (
    id VARCHAR(255) PRIMARY KEY,
    request_id VARCHAR(128) NOT NULL DEFAULT '',
    tag VARCHAR(128) NOT NULL DEFAULT '',
    date VARCHAR(10) NOT NULL,
    api_key VARCHAR(128) NOT NULL DEFAULT '',
    model VARCHAR(64) NOT NULL DEFAULT '',
    model_group VARCHAR(64) NOT NULL DEFAULT '',
    custom_llm_provider VARCHAR(32) NOT NULL DEFAULT '',
    mcp_namespaced_tool_name VARCHAR(128) NOT NULL DEFAULT '',
    endpoint VARCHAR(64) NOT NULL DEFAULT '',
    prompt_tokens BIGINT NOT NULL DEFAULT 0,
    completion_tokens BIGINT NOT NULL DEFAULT 0,
    cache_read_input_tokens BIGINT NOT NULL DEFAULT 0,
    cache_creation_input_tokens BIGINT NOT NULL DEFAULT 0,
    spend DOUBLE NOT NULL DEFAULT 0.0,
    api_requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uq_daily_tag (request_id, tag, date, api_key, model, custom_llm_provider, mcp_namespaced_tool_name, endpoint)
);
CREATE INDEX idx_daily_tag_spend_date ON daily_tag_spend(date);
CREATE INDEX idx_daily_tag_spend_tag ON daily_tag_spend(tag);
