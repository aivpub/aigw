-- 028_web_search_providers.sql (SQLite)
--
-- Web search provider / instance configuration (Phase 53, Stage 138).
--
-- Two levels, matching the runtime `WebSearchConfig` model (Stage 134):
--   web_search_providers — logical backend + pricing (registry key)
--   web_search_instances — physical endpoints, N per provider
--
-- `api_key` is stored encrypted (AES-GCM `v2:gcm:` with master_key) and redacted
-- in responses, same as `proxies.proxy_url` (027). `probe_result` is a single
-- JSON snapshot, same shape as `proxies.probe_result`.
--
-- Global policy (proxy_url / domain allow+block lists / snippet cap / retries /
-- cooldown) deliberately has NO column here: it is infrastructure policy, not
-- per-entity data, and stays in config.yaml (design §3.1a).
--
-- "Which provider is the default" is NOT a column either — it is the row with
-- the smallest non-NULL `failover_order` (ties broken by id). One fact, one
-- source; this makes "two rows both default" and "no row default"
-- structurally impossible (design §3.1, review F3).

CREATE TABLE IF NOT EXISTS web_search_providers (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL UNIQUE,
    kind            TEXT NOT NULL,
    cost_per_query  REAL NOT NULL DEFAULT 0.01,
    enabled         INTEGER NOT NULL DEFAULT 1,
    failover_order  INTEGER,
    timeout_ms      INTEGER,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS web_search_instances (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id     INTEGER NOT NULL REFERENCES web_search_providers(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    base_url        TEXT NOT NULL,
    api_key         TEXT,
    weight          INTEGER,
    enabled         INTEGER NOT NULL DEFAULT 1,
    probe_result    TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_web_search_instances_provider_name
    ON web_search_instances(provider_id, name);

CREATE INDEX IF NOT EXISTS idx_web_search_instances_provider
    ON web_search_instances(provider_id);
