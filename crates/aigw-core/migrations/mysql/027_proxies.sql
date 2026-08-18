-- 027_proxies.sql (MySQL)
--
-- Proxy service management table (Phase 50, Stage 122).
-- Maps to sub2api `proxies` table but stores the whole `proxy_url` string
-- encrypted (AES-GCM v2:gcm:, master_key) instead of splitting fields —
-- reqwest consumes `scheme://user:pass@host:port` natively.

CREATE TABLE IF NOT EXISTS proxies (
    id           BIGINT AUTO_INCREMENT PRIMARY KEY,
    name         VARCHAR(255) NOT NULL,
    proxy_url    TEXT NOT NULL,                -- 整串加密落库 (v2:gcm: prefix)
    status       VARCHAR(50) NOT NULL DEFAULT 'active',  -- active / inactive / expired
    expires_at   VARCHAR(64),
    probe_result JSON NOT NULL,                -- sqlx maps serde_json::Value → MySQL JSON
    created_at   VARCHAR(64) NOT NULL,
    updated_at   VARCHAR(64) NOT NULL,
    INDEX idx_proxies_status (status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
