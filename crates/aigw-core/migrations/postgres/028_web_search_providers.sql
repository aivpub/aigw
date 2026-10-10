-- 028_web_search_providers.sql (PostgreSQL)
--
-- Web search provider / instance configuration (Phase 53, Stage 138).
-- See the SQLite variant for the full rationale; the schema is identical apart
-- from the idiomatic types below.
--
-- Two type choices are load-bearing for sqlx, not stylistic:
--   * `enabled` is BOOLEAN (Rust `bool`). sqlx-postgres maps `bool` to BOOL and
--     `i64` to INT8 with no cross-width rule, so an INTEGER column fails to
--     decode into either. Same shape as 026_proxy_models_enabled.
--   * `failover_order` / `timeout_ms` / `weight` are BIGINT because the Rust
--     sides are `Option<i64>` — an INT4 column only decodes while it is NULL
--     (sqlx skips the type check for NULL), so the bug would surface the first
--     time one of them was set. BIGINT matches `provider_id` and 025's
--     `image_tokens BIGINT`.

CREATE TABLE IF NOT EXISTS web_search_providers (
    id              BIGSERIAL PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,
    kind            TEXT NOT NULL,
    cost_per_query  DOUBLE PRECISION NOT NULL DEFAULT 0.01,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    failover_order  BIGINT,
    timeout_ms      BIGINT,
    created_at      TEXT NOT NULL DEFAULT now(),
    updated_at      TEXT NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS web_search_instances (
    id              BIGSERIAL PRIMARY KEY,
    provider_id     BIGINT NOT NULL REFERENCES web_search_providers(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    base_url        TEXT NOT NULL,
    api_key         TEXT,
    weight          BIGINT,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    probe_result    JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TEXT NOT NULL DEFAULT now(),
    updated_at      TEXT NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_web_search_instances_provider_name
    ON web_search_instances(provider_id, name);

CREATE INDEX IF NOT EXISTS idx_web_search_instances_provider
    ON web_search_instances(provider_id);
