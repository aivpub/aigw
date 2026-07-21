//! Export encrypted test fixtures from an upstream litellm database.
//!
//! Usage:
//!   cargo run --bin export_fixtures --features postgres
//!
//! Reads .env for AIGW_UPSTREAM_DB_URL and AIGW_UPSTREAM_MASTER_KEY,
//! connects to the upstream PG DB, fetches litellm_params and
//! credential_values for a hardcoded model list, decrypts them with the
//! upstream master key, then re-encrypts the plaintext with a fixed
//! test key.  The resulting encrypted blobs are written as a JSON fixture
//! file that verify_nested_decrypt tests can consume **without** any DB
//! connection or real master key.

use aigw_core::crypto::{decrypt_litellm_value, encrypt_litellm_value};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;

/// The fixed test key used to re-encrypt plaintext for fixtures.
/// This is intentionally public — it is *not* a production secret.
const TEST_MASTER_KEY: &str = "sk-test-fixture-key-0000000000000000";

/// One model's encrypted fixture data.
#[derive(Debug, Serialize, Deserialize)]
struct ModelFixture {
    model_name: String,
    encrypted_litellm_params: String,
    encrypted_credential_values: Option<String>,
    /// A few known plaintext field values (after decryption) for quick sanity checks.
    expected_api_base_suffix: Option<String>,
    expected_model: Option<String>,
}

#[derive(Debug, Serialize)]
struct FixtureFile {
    description: String,
    test_key: String,
    models: Vec<ModelFixture>,
}

const MODELS: &[&str] = &[
    "maas/deepseek-v4-flash",
    "qcloud/deepseek-v4-flash",
    "qcloud/glm-5.1",
    "tke/deepseek-v4-flash",
    "tke/kimi-k25",
];

/// Parse litellm_params / credential_values from source DB.
/// If it's a JSON object, parse it directly. Otherwise decrypt the blob first.
fn parse_params(raw: &str, master_key: &str) -> Value {
    if raw.is_empty() || raw == "{}" {
        return Value::Object(Default::default());
    }
    if raw.starts_with('{') {
        serde_json::from_str(raw).unwrap_or_default()
    } else {
        decrypt_litellm_value(raw, master_key)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.env");
    if env_path.exists() {
        dotenvy::from_path(&env_path).ok();
    }

    let db_url = std::env::var("AIGW_UPSTREAM_DB_URL")?;
    let master_key = std::env::var("AIGW_UPSTREAM_MASTER_KEY").unwrap_or_default();

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await?;

    // Resolve master key from upstream DB first (same logic as verify test)
    let real_master_key = {
        if let Ok(Some((gs_raw,))) = sqlx::query_as::<_, (String,)>(
            "SELECT param_value::text FROM \"LiteLLM_Config\" WHERE param_name = 'general_settings'",
        )
        .fetch_optional(&pool)
        .await
        {
            if let Ok(gs) = serde_json::from_str::<Value>(&gs_raw) {
                if let Some(key) = gs.get("master_key").and_then(|v| v.as_str()) {
                    if !key.is_empty() {
                        key.to_string()
                    } else {
                        master_key
                    }
                } else {
                    master_key
                }
            } else {
                master_key
            }
        } else {
            master_key
        }
    };
    eprintln!("resolved master key (len={})", real_master_key.len());

    let mut fixtures = Vec::new();

    for &model_name in MODELS {
        eprintln!("--- processing {} ---", model_name);

        let row = sqlx::query_as::<_, (String,)>(
            "SELECT litellm_params::text FROM \"LiteLLM_ProxyModelTable\" WHERE model_name = $1",
        )
        .bind(model_name)
        .fetch_optional(&pool)
        .await?;

        let params_raw = match row {
            Some((raw,)) => raw,
            None => {
                eprintln!("  SKIP: model not found in upstream");
                continue;
            }
        };

        // Decrypt with real key → plaintext JSON
        let params_json = parse_params(&params_raw, &real_master_key);
        let params_str = serde_json::to_string(&params_json)?;

        // Re-encrypt with test key → safe to commit as fixture
        let encrypted_params = encrypt_litellm_value(&params_str, TEST_MASTER_KEY)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Round-trip check: decrypt with test key → must match original
        let roundtrip =
            decrypt_litellm_value(&encrypted_params, TEST_MASTER_KEY)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        assert_eq!(roundtrip, params_str, "roundtrip mismatch for {}", model_name);

        // Extract expected plaintext fields for sanity checks
        let expected_api_base = params_json
            .get("api_base")
            .and_then(|v| v.as_str())
            .map(|s| {
                // Keep only host:port suffix to avoid leaking full URL
                s.trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_string()
            });
        let expected_model = params_json
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Fetch credential_values if litellm_credential_name is present
        let encrypted_cred = if let Some(cred_name) = params_json
            .get("litellm_credential_name")
            .and_then(|v| v.as_str())
        {
            eprintln!("  fetching credential '{}'...", cred_name);
            let cred_row = sqlx::query_as::<_, (String,)>(
                "SELECT credential_values::text FROM \"LiteLLM_CredentialsTable\" WHERE credential_name = $1",
            )
            .bind(cred_name)
            .fetch_optional(&pool)
            .await?;

            if let Some((cred_raw,)) = cred_row {
                let cred_json = parse_params(&cred_raw, &real_master_key);
                let cred_str = serde_json::to_string(&cred_json)?;
                let enc = encrypt_litellm_value(&cred_str, TEST_MASTER_KEY)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                let rt = decrypt_litellm_value(&enc, TEST_MASTER_KEY)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                assert_eq!(rt, cred_str, "cred roundtrip mismatch for {}", model_name);
                Some(enc)
            } else {
                eprintln!("  WARN: credential '{}' not found", cred_name);
                None
            }
        } else {
            None
        };

        fixtures.push(ModelFixture {
            model_name: model_name.to_string(),
            encrypted_litellm_params: encrypted_params,
            encrypted_credential_values: encrypted_cred,
            expected_api_base_suffix: expected_api_base,
            expected_model,
        });

        eprintln!("  OK");
    }

    let fixture_file = FixtureFile {
        description: "Encrypted litellm_params fixtures for verify_nested_decrypt tests. \
                      Re-encrypted with TEST_MASTER_KEY so no real secrets are exposed."
            .to_string(),
        test_key: TEST_MASTER_KEY.to_string(),
        models: fixtures,
    };

    let json = serde_json::to_string_pretty(&fixture_file)?;
    let out_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/encrypted_params_fixtures.json");
    std::fs::create_dir_all(out_path.parent().unwrap())?;
    std::fs::write(&out_path, &json)?;

    println!("Fixtures written to {}", out_path.display());
    Ok(())
}
