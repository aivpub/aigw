-- 014_credentials.sql (PostgreSQL)
-- Maps to litellm LiteLLM_CredentialsTable

CREATE TABLE IF NOT EXISTS credentials (
    credential_id TEXT NOT NULL PRIMARY KEY,
    credential_name TEXT NOT NULL UNIQUE,
    credential_values TEXT NOT NULL DEFAULT '{}',
    credential_info TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    created_by TEXT,
    updated_at TEXT NOT NULL,
    updated_by TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_credentials_credential_name ON credentials(credential_name);
