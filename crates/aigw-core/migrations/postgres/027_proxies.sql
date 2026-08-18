-- 027_proxies.sql (PostgreSQL)
--
-- Proxy service management table (Phase 50, Stage 122).
-- Maps to sub2api `proxies` table but stores the whole `proxy_url` string
-- encrypted (AES-GCM v2:gcm:, master_key) instead of splitting fields —
-- reqwest consumes `scheme://user:pass@host:port` natively.
--
-- `probe_result` is JSONB: sqlx maps `serde_json::Value` to JSONB on PG, and
-- refuses to decode TEXT columns as JSONB (same constraint that forced
-- credentials.credential_values → JSONB in migration 019).

CREATE TABLE IF NOT EXISTS proxies (
    id           BIGSERIAL PRIMARY KEY,
    name         TEXT NOT NULL,
    proxy_url    TEXT NOT NULL,                -- 整串加密落库 (v2:gcm: prefix)
    status       TEXT NOT NULL DEFAULT 'active',  -- active / inactive / expired
    expires_at   TEXT,
    probe_result JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TEXT NOT NULL DEFAULT now(),
    updated_at   TEXT NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_proxies_status ON proxies(status);
