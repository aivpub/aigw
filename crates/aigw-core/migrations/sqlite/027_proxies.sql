-- 027_proxies.sql (SQLite)
--
-- Proxy service management table (Phase 50, Stage 122).
-- Maps to sub2api `proxies` table but stores the whole `proxy_url` string
-- encrypted (AES-GCM v2:gcm:, master_key) instead of splitting protocol/host/
-- port/username/password — reqwest consumes `scheme://user:pass@host:port`
-- natively. `probe_result` is a single JSON snapshot column (Stage 123 fills
-- it); the top-level `status` column is only for filtering.

CREATE TABLE IF NOT EXISTS proxies (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL,
    proxy_url    TEXT NOT NULL,                -- 整串加密落库 (v2:gcm: prefix)
    status       TEXT NOT NULL DEFAULT 'active',  -- active / inactive / expired
    expires_at   TEXT,                         -- NULL = 永不过期; expired 由它派生
    probe_result TEXT NOT NULL DEFAULT '{}',   -- 检测快照 JSON (Stage 123)
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_proxies_status ON proxies(status);
