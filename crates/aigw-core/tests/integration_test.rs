//! Integration tests using testcontainers
//!
//! These tests require Docker and are gated behind the `integration` feature flag.
//! Run with: `cargo test --features integration`

#[cfg(feature = "integration")]
use aigw_core::db::Database;

#[cfg(feature = "integration")]
const EXPECTED_TABLES: &[&str] = &[
    "virtual_keys",
    "spend_logs",
    "organizations",
    "teams",
    "users",
    "projects",
    "budgets",
    "organization_memberships",
    "team_memberships",
    "deprecated_keys",
    "deleted_keys",
];

#[cfg(feature = "integration")]
mod postgres_tests {
    use super::*;
    use aigw_core::models::Credential;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    #[tokio::test]
    async fn test_postgres_migration() {
        // Spin up PostgreSQL container
        let container = Postgres::default()
            .start()
            .await
            .expect("failed to start postgres container");

        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("failed to get host port");
        let url = format!("postgresql://postgres:postgres@localhost:{}/postgres", port);

        // Initialize database and run migrations
        let db = Database::init(&url).await.expect("postgres init");

        match db {
            Database::Postgres(pool) => {
                // Verify all 11 tables exist
                for table_name in EXPECTED_TABLES {
                    let row: (String,) = sqlx::query_as(
                        "SELECT table_name FROM information_schema.tables WHERE table_name = $1 AND table_schema = 'public'",
                    )
                    .bind(table_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|_| {
                        panic!("table '{}' should exist after postgres migration", table_name)
                    });
                    assert_eq!(row.0, *table_name);
                }
            }
            _ => panic!("expected PostgreSQL"),
        }
    }

    /// Regression: `credentials.credential_values` / `credential_info` used to be
    /// TEXT in PG, which made `list_credentials` explode with:
    ///
    ///   mismatched types; Rust type `serde_json::value::Value`
    ///   (as SQL type `JSONB`) is not compatible with SQL type `TEXT`
    ///
    /// Migration 019 flips them to JSONB.  This test:
    ///   1. Confirms migrations succeed against a real PG.
    ///   2. Inserts a credential whose `credential_values` is a *JSON scalar
    ///      string* (mimicking what remote-import writes for opaque encrypted
    ///      blobs like `"gAAAAAB..."`).
    ///   3. Reads it back via the store trait — this is the path that used
    ///      to fail — and asserts we get `Value::String(_)` verbatim.
    #[tokio::test]
    async fn test_postgres_credentials_jsonb_roundtrip() {
        use aigw_core::db::CredentialsStore;

        let container = Postgres::default()
            .start()
            .await
            .expect("failed to start postgres container");
        let port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("failed to get host port");
        let url = format!("postgresql://postgres:postgres@localhost:{}/postgres", port);
        let db = Database::init(&url).await.expect("postgres init");
        let pool = match db {
            Database::Postgres(p) => p,
            _ => panic!("expected PostgreSQL"),
        };

        // Verify column type is jsonb (not text) after migration 019.
        let (values_ty, info_ty): (String, String) = sqlx::query_as(
            "SELECT
               (SELECT data_type FROM information_schema.columns
                WHERE table_name='credentials' AND column_name='credential_values'),
               (SELECT data_type FROM information_schema.columns
                WHERE table_name='credentials' AND column_name='credential_info')",
        )
        .fetch_one(&pool)
        .await
        .expect("column type lookup");
        assert_eq!(values_ty, "jsonb", "credential_values should be JSONB after 019");
        assert_eq!(info_ty, "jsonb", "credential_info should be JSONB after 019");

        // Insert an opaque encrypted-blob credential.  This mirrors the
        // remote-import output when litellm's LiteLLM_CredentialsTable stores
        // a single encrypted string rather than a JSON object.
        let cred = Credential {
            credential_id: "id-1".to_string(),
            credential_name: "acme".to_string(),
            credential_values: serde_json::Value::String("gAAAAABopaqueBase64Blob".to_string()),
            credential_info: serde_json::Value::Object(Default::default()),
            created_at: "2026-07-20T00:00:00Z".to_string(),
            created_by: None,
            updated_at: "2026-07-20T00:00:00Z".to_string(),
            updated_by: None,
        };
        pool.insert_credential(&cred).await.expect("insert");

        // Read back via the same code path the /credential/list handler uses.
        let listed = pool.list_credentials().await.expect("list");
        assert_eq!(listed.len(), 1);
        let got = &listed[0];
        assert_eq!(got.credential_name, "acme");
        assert_eq!(
            got.credential_values,
            serde_json::Value::String("gAAAAABopaqueBase64Blob".to_string()),
            "opaque encrypted blob must round-trip as Value::String, not Value::Object({{}})",
        );
    }

        /// Regression test for /global/spend/keys/rankings on PostgreSQL.
        ///
        /// `aggregate_spend_by_keys` selects `vk.key_alias` (from the LEFT JOIN on
        /// virtual_keys) while grouping by `sl.api_key`. PostgreSQL enforces that every
        /// non-aggregated SELECT column appears in GROUP BY, so the original SQL raised
        /// `column "vk.key_alias" must appear in the GROUP BY clause`. SQLite/MySQL are
        /// lenient and the bug only surfaced on the PG deployment.
        #[tokio::test]
        async fn test_postgres_aggregate_spend_by_keys() {
            use aigw_core::crypto::hash_token;
            use aigw_core::db::Database;
            use aigw_core::models::{SpendLog, VirtualKey};
            use chrono::Utc;
            use uuid::Uuid;

            let container = Postgres::default()
                .start()
                .await
                .expect("failed to start postgres container");
            let port = container
                .get_host_port_ipv4(5432)
                .await
                .expect("failed to get host port");
            let url = format!("postgresql://postgres:postgres@localhost:{}/postgres", port);
            let db = Database::init(&url).await.expect("postgres init");

            // virtual_keys.token is the PK; key_alias is functionally dependent on it
            // via the LEFT JOIN, so grouping by both keeps cardinality unchanged.
            let key_a = hash_token("key-a");
            let key_b = hash_token("key-b");
            let make_key = |token: &str, alias: &str| VirtualKey {
                token: token.to_string(),
                key_name: Some(alias.to_string()),
                key_alias: Some(alias.to_string()),
                soft_budget_cooldown: "false".to_string(),
                spend: 0.0,
                expires: None,
                models: serde_json::json!([]),
                aliases: serde_json::json!({}),
                config: serde_json::json!({}),
                router_settings: None,
                user_id: Some("user-1".to_string()),
                team_id: None,
                agent_id: None,
                project_id: None,
                permissions: serde_json::json!({}),
                max_parallel_requests: None,
                metadata: serde_json::json!({}),
                blocked: None,
                tpm_limit: None,
                rpm_limit: None,
                max_budget: None,
                budget_duration: None,
                budget_reset_at: None,
                allowed_cache_controls: serde_json::json!([]),
                allowed_routes: serde_json::json!([]),
                policies: serde_json::json!([]),
                access_group_ids: serde_json::json!([]),
                model_spend: serde_json::json!({}),
                model_max_budget: serde_json::json!({}),
                budget_id: None,
                organization_id: None,
                object_permission_id: None,
                created_at: Some(Utc::now()),
                created_by: None,
                updated_at: Some(Utc::now()),
                updated_by: None,
                last_active: None,
                rotation_count: None,
                auto_rotate: None,
                rotation_interval: None,
                last_rotation_at: None,
                key_rotation_at: None,
                budget_limits: None,
            };
            db.insert_key(&make_key(&key_a, "alias-a")).await.expect("insert key a");
            db.insert_key(&make_key(&key_b, "alias-b")).await.expect("insert key b");

            let make_log = |api_key: &str, spend: f64, tokens: i32| SpendLog {
                request_id: Uuid::new_v4().to_string(),
                call_type: "completion".to_string(),
                api_key: api_key.to_string(),
                spend,
                total_tokens: tokens,
                prompt_tokens: tokens / 2,
                completion_tokens: tokens - tokens / 2,
                start_time: Utc::now(),
                end_time: Utc::now(),
                request_duration_ms: Some(500),
                completion_start_time: None,
                model: "gpt-4".to_string(),
                model_id: None,
                model_group: None,
                custom_llm_provider: Some("openai".to_string()),
                api_base: None,
                user: Some("u1".to_string()),
                metadata: None,
                cache_hit: None,
                cache_key: None,
                request_tags: None,
                team_id: None,
                organization_id: None,
                end_user: None,
                requester_ip_address: None,
                messages: None,
                response: None,
                session_id: None,
                status: Some("success".to_string()),
                mcp_namespaced_tool_name: None,
                agent_id: None,
                proxy_server_request: None,
            };
            db.insert_spend_log(&make_log(&key_a, 10.0, 100)).await.expect("insert log a1");
            db.insert_spend_log(&make_log(&key_a, 3.0, 30)).await.expect("insert log a2");
            db.insert_spend_log(&make_log(&key_b, 5.0, 50)).await.expect("insert log b");

            // This call previously raised the GROUP BY error on PostgreSQL.
            let rankings = db
                .aggregate_spend_by_keys("2020-01-01", "2030-12-31", 10)
                .await
                .expect("aggregate_spend_by_keys on postgres");

            assert!(rankings.len() >= 2, "should rank at least 2 keys");
            // Descending by total_spend: key_a (13.0) before key_b (5.0).
            assert_eq!(rankings[0].api_key, key_a);
            assert_eq!(rankings[0].total_spend, 13.0);
            assert_eq!(rankings[0].key_alias.as_deref(), Some("alias-a"));
            assert_eq!(rankings[1].api_key, key_b);
            assert_eq!(rankings[1].total_spend, 5.0);
        }
}

#[cfg(feature = "integration")]
mod mysql_tests {
    use super::*;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::mysql::Mysql;

    #[tokio::test]
    async fn test_mysql_migration() {
        // Spin up MySQL container
        let container = Mysql::default()
            .start()
            .await
            .expect("failed to start mysql container");

        let port = container
            .get_host_port_ipv4(3306)
            .await
            .expect("failed to get host port");
        let url = format!("mysql://root:test@localhost:{}/test", port);

        // Initialize database and run migrations
        let db = Database::init(&url).await.expect("mysql init");

        match db {
            Database::Mysql(pool) => {
                // Verify all 11 tables exist
                for table_name in EXPECTED_TABLES {
                    let row: (String,) = sqlx::query_as(
                        "SELECT TABLE_NAME FROM information_schema.tables WHERE TABLE_NAME = ? AND TABLE_SCHEMA = DATABASE()",
                    )
                    .bind(table_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap_or_else(|_| {
                        panic!("table '{}' should exist after mysql migration", table_name)
                    });
                    assert_eq!(row.0, *table_name);
                }
            }
            _ => panic!("expected MySQL"),
        }
    }
}
