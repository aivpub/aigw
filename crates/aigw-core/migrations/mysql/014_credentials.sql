-- 014_credentials.sql (MySQL)
-- Maps to litellm LiteLLM_CredentialsTable

CREATE TABLE IF NOT EXISTS credentials (
    credential_id VARCHAR(255) NOT NULL PRIMARY KEY,
    credential_name VARCHAR(255) NOT NULL,
    credential_values TEXT NOT NULL,
    credential_info TEXT NOT NULL,
    created_at VARCHAR(64) NOT NULL,
    created_by VARCHAR(255),
    updated_at VARCHAR(64) NOT NULL,
    updated_by VARCHAR(255)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE UNIQUE INDEX idx_credentials_credential_name ON credentials(credential_name);
