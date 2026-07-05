-- 014_credentials.sql
-- Maps to litellm LiteLLM_CredentialsTable
-- Stores encrypted credential values (api keys, connection strings) for model deployments

CREATE TABLE IF NOT EXISTS credentials (
    credential_id TEXT NOT NULL PRIMARY KEY,
    credential_name TEXT NOT NULL,
    credential_values TEXT NOT NULL DEFAULT '{}',
    credential_info TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_credentials_credential_name ON credentials(credential_name);
