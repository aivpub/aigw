-- 028_web_search_providers.sql (PostgreSQL)
--
-- Web search provider / instance configuration (Phase 53, Stage 138).
-- See the SQLite variant for the full rationale; the schema is identical apart
-- from the idiomatic types below.

CREATE TABLE IF NOT EXISTS web_search_providers (
    id              BIGSERIAL PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    kind            TEXT NOT NULL,
    cost_per_query  DOUBLE PRECISION NOT NULL DEFAULT 0.01,
    enabled         INTEGER NOT NULL DEFAULT 1,
    failover_order  INTEGER,
    timeout_ms      INTEGER,
    created_at      TEXT NOT NULL DEFAULT now(),
    updated_at      TEXT NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS web_search_instances (
    id              BIGSERIAL PRIMARY KEY,
    provider_id     BIGINT NOT NULL REFERENCES web_search_providers(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    base_url        TEXT NOT NULL,
    api_key         TEXT,
    weight          INTEGER,
    enabled         INTEGER NOT NULL DEFAULT 1,
    probe_result    JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TEXT NOT NULL DEFAULT now(),
    updated_at      TEXT NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_web_search_instances_provider_name
    ON web_search_instances(provider_id, name);

CREATE INDEX IF NOT EXISTS idx_web_search_instances_provider
    ON web_search_instances(provider_id);
