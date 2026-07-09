//! Verification test: connect to upstream PG litellm database and verify
//! that nested field decryption works correctly for all specified models.
//!
//! Reads upstream connection info from .env:
//!   AIGW_UPSTREAM_DB_URL     — litellm PG database
//!   AIGW_UPSTREAM_MASTER_KEY — litellm master key for decryption (fallback;
//!                              the test prefers the key stored in the DB's
//!                              LiteLLM_Config.general_settings.master_key)

use aigw_core::crypto::{decrypt_json_fields, decrypt_litellm_value};
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};

const MODELS_TO_CHECK: &[&str] = &[
    "maas/deepseek-v4-flash",
    "qcloud/deepseek-v4-flash",
    "qcloud/glm-5.1",
    "tke/deepseek-v4-flash",
    "tke/kimi-k25",
];

/// Heuristic: does `s` still look like a litellm-encrypted value?
///
/// Encrypted litellm values are base64 blobs: ≥ 24 nonce + 16 poly1305 tag
/// = 40 raw bytes = ~56 base64 chars.  We use a 60-char threshold plus
/// character-set checks to minimise false positives on long API keys.
fn looks_encrypted(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    if s.starts_with("v2:gcm:") {
        return true;
    }
    // Exclude known plaintext patterns
    if s.starts_with("sk-")
        || s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("Bearer ")
        || s.starts_with('{')
    {
        return false;
    }
    s.len() >= 60
        && s.bytes().all(|b| {
            b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=' || b == b'-' || b == b'_'
        })
        && !s.contains(' ')
}

fn collect_all_string_leaves(value: &Value, prefix: &str, results: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                collect_all_string_leaves(v, &format!("{}.{}", prefix, k), results);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                collect_all_string_leaves(v, &format!("{}[{}]", prefix, i), results);
            }
        }
        Value::String(s) => {
            results.push((prefix.to_string(), s.clone()));
        }
        _ => {}
    }
}

/// Resolve the authoritative master key from the upstream DB.
///
/// 1. `LiteLLM_Config.param_name = 'general_settings'` → JSON `master_key` field
/// 2. `LiteLLM_Config.param_name = 'litellm_master_key'` (legacy)
/// 3. `AIGW_UPSTREAM_MASTER_KEY` env var
async fn resolve_master_key(pool: &PgPool) -> String {
    if let Ok(Some((gs_raw,))) = sqlx::query_as::<_, (String,)>(
        "SELECT param_value::text FROM \"LiteLLM_Config\" WHERE param_name = 'general_settings'",
    )
    .fetch_optional(pool)
    .await
    {
        if let Ok(gs) = serde_json::from_str::<Value>(&gs_raw) {
            if let Some(key) = gs.get("master_key").and_then(|v| v.as_str()) {
                if !key.is_empty() {
                    return key.to_string();
                }
            }
        }
    }
    if let Ok(Some((lmk,))) = sqlx::query_as::<_, (String,)>(
        "SELECT param_value::text FROM \"LiteLLM_Config\" WHERE param_name = 'litellm_master_key'",
    )
    .fetch_optional(pool)
    .await
    {
        if !lmk.is_empty() {
            return lmk;
        }
    }
    std::env::var("AIGW_UPSTREAM_MASTER_KEY").unwrap_or_default()
}

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

#[tokio::test]
async fn verify_nested_decrypt_for_all_models() {
    let env_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.env");
    if env_path.exists() {
        dotenvy::from_path(&env_path).ok();
    }

    let db_url = std::env::var("AIGW_UPSTREAM_DB_URL")
        .expect("AIGW_UPSTREAM_DB_URL must be set in .env");

    println!("\n=== 连接上游 PG 数据库 ===");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&db_url)
        .await
        .expect("无法连接上游 PG 数据库");

    let master_key = resolve_master_key(&pool).await;
    println!("  master_key resolved from upstream DB");

    let mut any_failure = false;

    for &model_name in MODELS_TO_CHECK {
        println!("\n--- 模型: {} ---", model_name);

        // 1. Query litellm_params
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT litellm_params::text FROM \"LiteLLM_ProxyModelTable\" WHERE model_name = $1",
        )
        .bind(model_name)
        .fetch_optional(&pool)
        .await
        .expect("查询失败");

        let params_raw = match row {
            Some((raw,)) => raw,
            None => {
                println!("  [SKIP] 模型不存在于上游数据库");
                continue;
            }
        };
        let params_json = parse_params(&params_raw, &master_key);

        // 2. Collect encrypted fields before nested decryption
        let mut before_fields = Vec::new();
        collect_all_string_leaves(&params_json, "litellm_params", &mut before_fields);
        let before_encrypted: Vec<_> = before_fields
            .iter()
            .filter(|(_, v)| looks_encrypted(v))
            .map(|(p, _)| p.clone())
            .collect();

        // 3. Apply nested decryption
        let decrypted = decrypt_json_fields(&params_json, &master_key);

        // 4. Collect string leaves after nested decryption
        let mut after_fields = Vec::new();
        collect_all_string_leaves(&decrypted, "litellm_params", &mut after_fields);

        // 5. Report
        let still_encrypted: Vec<_> = after_fields
            .iter()
            .filter(|(_, v)| looks_encrypted(v))
            .collect();
        if still_encrypted.is_empty() {
            println!(
                "  [OK] litellm_params 全部解密成功 ({}/{} encrypted → 0 残留)",
                before_encrypted.len(),
                after_fields.len(),
            );
        } else {
            for (path, value) in &still_encrypted {
                println!("  [FAIL] {} = {} (仍然加密!)", path, &value[..value.len().min(60)]);
                any_failure = true;
            }
        }

        // Show key fields
        let keys_to_show = [
            "litellm_params.model",
            "litellm_params.api_base",
            "litellm_params.api_key",
            "litellm_params.litellm_credential_name",
        ];
        for key in &keys_to_show {
            if let Some(val) = after_fields.iter().find(|(p, _)| p == key) {
                let masked = if val.1.len() > 30 {
                    format!("{}...{}", &val.1[..15], &val.1[val.1.len() - 4..])
                } else {
                    val.1.clone()
                };
                println!("    {} = {}", key, masked);
            }
        }

        // 6. If credential reference exists, verify credential_values too
        let cred_name = decrypted
            .get("litellm_credential_name")
            .and_then(|v| v.as_str());

        if let Some(cred_name) = cred_name {
            println!("  credential '{}' → 验证 credential_values...", cred_name);

            let cred_row = sqlx::query_as::<_, (String,)>(
                "SELECT credential_values::text FROM \"LiteLLM_CredentialsTable\" WHERE credential_name = $1",
            )
            .bind(cred_name)
            .fetch_optional(&pool)
            .await
            .expect("credential 查询失败");

            if let Some((cred_raw,)) = cred_row {
                let cred_json = parse_params(&cred_raw, &master_key);
                let cred_decrypted = decrypt_json_fields(&cred_json, &master_key);

                let mut cred_after = Vec::new();
                collect_all_string_leaves(&cred_decrypted, "credential_values", &mut cred_after);

                let cred_still: Vec<_> = cred_after
                    .iter()
                    .filter(|(_, v)| looks_encrypted(v))
                    .collect();
                if cred_still.is_empty() {
                    println!("  [OK] credential_values 全部解密成功 ({} 字段)", cred_after.len());
                } else {
                    for (path, value) in &cred_still {
                        println!("  [FAIL] {} = {} (仍然加密!)", path, &value[..value.len().min(60)]);
                        any_failure = true;
                    }
                }
            } else {
                println!("  [WARN] credential '{}' 不存在于上游数据库", cred_name);
            }
        }
    }

    if any_failure {
        panic!("有些字段未能成功解密, 详情见上面 [FAIL] 输出");
    }
    println!("\n=== 全部模型验证通过 ===");
}
