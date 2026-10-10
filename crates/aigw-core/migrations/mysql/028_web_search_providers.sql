-- 028_web_search_providers.sql (MySQL)
--
-- Web search provider / instance configuration (Phase 53, Stage 138).
-- See the SQLite variant for the full rationale.
--
-- NOTE on the foreign key: InnoDB parses and SILENTLY IGNORES the column-level
-- `REFERENCES` clause — only a table-level `FOREIGN KEY ... REFERENCES` is
-- enforced. Copying the SQLite body verbatim here would leave `ON DELETE
-- CASCADE` inert (design §3.1b, review F4). Same form as 020_async_jobs.sql.
--
-- NOTE on `created_at` / `updated_at`: the Rust side is `String`, and sqlx
-- decodes a `String` from VARCHAR — but NOT from DATETIME (its `str::compatible`
-- accepts only the text/blob column types). The app always supplies these
-- timestamps itself (RFC 3339), so they are VARCHAR(64) here, exactly as 027's
-- `proxies` table does. `DATETIME DEFAULT CURRENT_TIMESTAMP` would 500 on every
-- read.
--
-- NOTE on the numeric columns: `enabled` is TINYINT (Rust `bool`, per 026);
-- `failover_order` / `timeout_ms` / `weight` are BIGINT (Rust `Option<i64>`).
-- MySQL's sqlx accepts any signed int width for any signed int width, so the
-- wider declarations are for symmetry with the other two dialects rather than
-- necessity.

CREATE TABLE IF NOT EXISTS web_search_providers (
    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
    name            VARCHAR(255) NOT NULL UNIQUE,
    kind            VARCHAR(64) NOT NULL,
    cost_per_query  DOUBLE NOT NULL DEFAULT 0.01,
    enabled         TINYINT NOT NULL DEFAULT 1,
    failover_order  BIGINT,
    timeout_ms      BIGINT,
    created_at      VARCHAR(64) NOT NULL,
    updated_at      VARCHAR(64) NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS web_search_instances (
    id              BIGINT AUTO_INCREMENT PRIMARY KEY,
    provider_id     BIGINT NOT NULL,
    name            VARCHAR(255) NOT NULL,
    base_url        TEXT NOT NULL,
    api_key         TEXT,
    weight          BIGINT,
    enabled         TINYINT NOT NULL DEFAULT 1,
    probe_result    JSON NOT NULL,
    created_at      VARCHAR(64) NOT NULL,
    updated_at      VARCHAR(64) NOT NULL,
    UNIQUE KEY idx_web_search_instances_provider_name (provider_id, name),
    KEY idx_web_search_instances_provider (provider_id),
    CONSTRAINT fk_web_search_instances_provider
        FOREIGN KEY (provider_id) REFERENCES web_search_providers(id) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
