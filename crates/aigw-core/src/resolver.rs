//! ModelResolver — model name to upstream Deployment resolution.
//!
//! Replaces chat.rs's resolve_upstream_params() with a standalone component
//! that returns Vec<Deployment> — one result per matching proxy_models row.

use crate::db::Database;
use crate::deployment::{Deployment, ProviderType};
use crate::crypto::{decrypt_json_fields, decrypt_litellm_value};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

/// Resolves a model name to zero or more upstream Deployments.
///
/// One model_name may match multiple proxy_models rows (different api_base/key).
/// The current handler takes [0]; a future Router phase will iterate & select.
#[derive(Debug, Clone)]
pub struct ModelResolver {
    db: Database,
    aigw_master_key: Option<String>,
    deployment_mode: String,
}

impl ModelResolver {
    pub fn new<D: Into<String>>(db: Database, aigw_master_key: Option<D>, deployment_mode: D) -> Self {
        let aigw_master_key = aigw_master_key.map(|k| k.into());
        let deployment_mode = deployment_mode.into();
        Self { db, aigw_master_key, deployment_mode }
    }

    /// Resolve all upstream Deployments for the given model_name.
    pub async fn resolve(
        &self,
        model_name: &str,
    ) -> Result<Vec<Deployment>, (StatusCode, Json<Value>)> {
        let models = self.db.list_models_by_name(model_name).await.map_err(|e| {
            tracing::warn!("Failed to look up model '{}': {}", model_name, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("DB error: {}", e), "type": "db_error"}})),
            )
        })?;

        if !models.is_empty() {
            let mut deployments = Vec::new();
            for m in models {
                let d = self.resolve_one(m, model_name).await?;
                deployments.push(d);
            }
            return Ok(deployments);
        }

        // Fallback to env vars (except in test mode)
        if self.deployment_mode != "test" {
            let api_key_env = std::env::var("OPENAI_API_KEY").ok()
                .or_else(|| std::env::var("OPENAPI_KEY").ok());
            let api_base_env = std::env::var("OPENAI_BASE_URL")
                .or_else(|_| std::env::var("OPENAPI_BASE_URL"))
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

            if api_key_env.is_some() {
                tracing::info!(
                    %model_name,
                    api_base = %api_base_env,
                    "Model not in proxy_models, falling back to env vars"
                );
                return Ok(vec![Deployment {
                    api_base: api_base_env,
                    api_key: api_key_env,
                    upstream_model: model_name.to_string(),
                    provider_type: ProviderType::OpenAICompatible,
                    input_cost_per_token: None,
                    output_cost_per_token: None,
                    raw_params: json!({}),
                    model_id: None,
                    model_group: None,
                    custom_llm_provider: None,
                    chat_template_compat: None,
                    fail_count: 0,
                    cooldown_until: None,
                }]);
            }
        }

        Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": format!("Model '{}' not found. Add it to proxy_models or check model_name spelling.", model_name),
                    "type": "invalid_request_error",
                    "code": "model_not_found"
                }
            })),
        ))
    }

    /// Resolve a single ProxyModel row into a Deployment.
    async fn resolve_one(
        &self,
        m: crate::models::ProxyModel,
        model_name: &str,
    ) -> Result<Deployment, (StatusCode, Json<Value>)> {
        let litellm_params_str = m.litellm_params.as_str()
            .map(String::from)
            .unwrap_or_else(|| m.litellm_params.to_string());

        let params_json: Value = if litellm_params_str.starts_with('{') {
            m.litellm_params.clone()
        } else {
            let key = self.aigw_master_key.as_deref().ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": {
                            "message": "Model has encrypted params but AIGW_MASTER_KEY is not configured",
                            "type": "config_error"
                        }
                    })),
                )
            })?;

            let decrypted = decrypt_litellm_value(&litellm_params_str, key).map_err(|e| {
                tracing::error!("Failed to decrypt litellm_params for model '{}': {}", model_name, e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": {
                            "message": format!("Failed to decrypt model params: {}", e),
                            "type": "crypto_error"
                        }
                    })),
                )
            })?;

            serde_json::from_str(&decrypted).unwrap_or_else(|_| json!({}))
        };

        // Decrypt individually encrypted fields inside the JSON object
        let params_json = if let Some(key) = self.aigw_master_key.as_deref() {
            decrypt_json_fields(&params_json, key)
        } else {
            params_json.clone()
        };

        // Extract pricing
        let (input_cost, output_cost) = extract_pricing(&m.model_info, &params_json);

        // Extract model_group/model_id/custom_llm_provider from proxy_models for SpendLog
        let model_id = Some(m.model_id.clone());
        let model_group = params_json
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let custom_llm_provider = params_json
            .get("custom_llm_provider")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Resolve credential reference if present
        if let Some(cred_name) = params_json
            .get("litellm_credential_name")
            .and_then(|v| v.as_str())
        {
            let cred = self.db.get_credential_by_name(cred_name).await.map_err(|e| {
                tracing::error!("Failed to look up credential '{}': {}", cred_name, e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": {
                            "message": format!("Credential '{}' not found", cred_name),
                            "type": "not_found"
                        }
                    })),
                )
            })?;

            let cred = cred.ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": {
                            "message": format!("Credential '{}' not found", cred_name),
                            "type": "not_found"
                        }
                    })),
                )
            })?;

            let cred_values_str = cred.credential_values.to_string();
            let cred_values: Value = if cred_values_str.starts_with('{') {
                cred.credential_values.clone()
            } else {
                let key = self.aigw_master_key.as_deref().ok_or_else(|| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": "Credential is encrypted but AIGW_MASTER_KEY is not configured",
                                "type": "config_error"
                            }
                        })),
                    )
                })?;
                let decrypted = decrypt_litellm_value(&cred_values_str, key).map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({
                            "error": {
                                "message": format!("Failed to decrypt credential: {}", e),
                                "type": "crypto_error"
                            }
                        })),
                    )
                })?;
                serde_json::from_str(&decrypted).unwrap_or_else(|_| json!({}))
            };

            let cred_values = if let Some(key) = self.aigw_master_key.as_deref() {
                decrypt_json_fields(&cred_values, key)
            } else {
                cred_values
            };

            // Merge: credential values take precedence where not already set
            let mut merged = cred_values;
            if let Some(obj) = merged.as_object_mut() {
                for (k, v) in params_json.as_object().into_iter().flat_map(|o| o.iter()) {
                    if !obj.contains_key(k) {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }

            let api_base = merged
                .get("api_base")
                .and_then(|v| v.as_str())
                .unwrap_or("https://api.openai.com/v1")
                .to_string();
            let api_key = merged.get("api_key").and_then(|v| v.as_str()).map(|s| s.to_string());
            let upstream_model = merged
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or(model_name)
                .to_string();

            let provider_type = ProviderType::infer(
                merged.get("custom_llm_provider").and_then(|v| v.as_str()),
                &api_base,
            );

            // Extract chat_template_compat from model_info
            let chat_template_compat = m.model_info
                .get("chat_template_compat")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            Ok(Deployment {
                api_base,
                api_key,
                upstream_model,
                provider_type,
                input_cost_per_token: input_cost,
                output_cost_per_token: output_cost,
                raw_params: params_json,
                model_id,
                model_group: model_group.clone(),
                custom_llm_provider: custom_llm_provider.clone(),
                chat_template_compat,
                fail_count: 0,
                cooldown_until: None,
            })
        } else {
            tracing::warn!(%model_name, "resolve: NO credential reference, using litellm_params directly");
            let api_base = params_json
                .get("api_base")
                .and_then(|v| v.as_str())
                .unwrap_or("https://api.openai.com/v1")
                .to_string();
            let api_key = params_json
                .get("api_key")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let upstream_model = params_json
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or(model_name)
                .to_string();

            let provider_type = ProviderType::infer(
                params_json.get("custom_llm_provider").and_then(|v| v.as_str()),
                &api_base,
            );

            // Extract chat_template_compat from model_info
            let chat_template_compat = m.model_info
                .get("chat_template_compat")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // When proxy_models has api_key=None (encrypted/empty), fall back to env vars.
            let (api_base, api_key) = if api_key.is_none() {
                let env_key = std::env::var("OPENAI_API_KEY").ok()
                    .or_else(|| std::env::var("OPENAPI_KEY").ok());
                let env_base = std::env::var("OPENAI_BASE_URL")
                    .or_else(|_| std::env::var("OPENAPI_BASE_URL"))
                    .unwrap_or_else(|_| api_base);
                (env_base, env_key)
            } else {
                (api_base, api_key)
            };

            tracing::warn!(%model_name, %api_base, ?api_key, %upstream_model, "resolve: DIRECT PARAMS RESOLVED");
            Ok(Deployment {
                api_base,
                api_key,
                upstream_model,
                provider_type,
                input_cost_per_token: input_cost,
                output_cost_per_token: output_cost,
                raw_params: params_json,
                model_id,
                model_group,
                custom_llm_provider,
                chat_template_compat,
                fail_count: 0,
                cooldown_until: None,
            })
        }
    }
}

/// Extract pricing — primary from model_info, fallback to litellm_params.
fn extract_pricing(model_info: &Value, params_json: &Value) -> (Option<f64>, Option<f64>) {
    let input = model_info
        .get("input_cost_per_token")
        .and_then(|v| v.as_f64())
        .or_else(|| params_json.get("input_cost_per_token").and_then(|v| v.as_f64()));
    let output = model_info
        .get("output_cost_per_token")
        .and_then(|v| v.as_f64())
        .or_else(|| params_json.get("output_cost_per_token").and_then(|v| v.as_f64()));
    (input, output)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encrypt_litellm_value;
    use crate::db::Database;
    use crate::models::ProxyModel;

    async fn make_resolver(db: Database, master_key: Option<&str>) -> ModelResolver {
        ModelResolver::new(
            db,
            master_key.map(String::from),
            "onprem".to_string(),
        )
    }

    /// Helper: insert a model with plaintext params and optional model_info pricing.
    async fn insert_plaintext_model(
        db: &Database,
        model_name: &str,
        litellm_params: Value,
        model_info: Value,
    ) {
        let model = ProxyModel {
            model_id: uuid::Uuid::new_v4().to_string(),
            model_name: model_name.to_string(),
            litellm_params,
            model_info,
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
            updated_by: None,
        };
        db.insert_model(&model).await.expect("insert model");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // TDD Tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[tokio::test]
    async fn test_resolve_model_found_plaintext() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        insert_plaintext_model(
            &db,
            "gpt-4",
            json!({"model": "gpt-4", "api_base": "https://api.openai.com/v1", "api_key": "sk-test", "custom_llm_provider": "openai"}),
            json!({"input_cost_per_token": 0.00003, "output_cost_per_token": 0.00006}),
        )
        .await;

        let resolver = make_resolver(db, None).await;
        let deployments = resolver.resolve("gpt-4").await.unwrap();

        assert_eq!(deployments.len(), 1);
        let d = &deployments[0];
        assert_eq!(d.api_base, "https://api.openai.com/v1");
        assert_eq!(d.api_key.as_deref(), Some("sk-test"));
        assert_eq!(d.upstream_model, "gpt-4");
        assert_eq!(d.provider_type, ProviderType::OpenAICompatible);
        assert_eq!(d.input_cost_per_token, Some(0.00003));
        assert_eq!(d.output_cost_per_token, Some(0.00006));
        assert_eq!(d.raw_params["custom_llm_provider"].as_str(), Some("openai"));
    }

    #[tokio::test]
    async fn test_resolve_model_with_encrypted_params() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let master_key = "sk-test-master-key-32chars!!!";

        let params = json!({"model": "gpt-4", "api_base": "https://api.openai.com/v1", "api_key": "sk-encrypted-test", "custom_llm_provider": "openai"});
        let params_str = params.to_string();
        let encrypted = encrypt_litellm_value(&params_str, master_key).unwrap();
        let encrypted_params: Value = serde_json::from_str(&format!("\"{}\"", encrypted)).unwrap();

        insert_plaintext_model(
            &db,
            "gpt-4",
            encrypted_params,
            json!({}),
        )
        .await;

        let resolver = make_resolver(db, Some(master_key)).await;
        let deployments = resolver.resolve("gpt-4").await.unwrap();

        assert_eq!(deployments.len(), 1);
        let d = &deployments[0];
        assert_eq!(d.api_key.as_deref(), Some("sk-encrypted-test"));
        assert_eq!(d.provider_type, ProviderType::OpenAICompatible);
    }

    #[tokio::test]
    async fn test_resolve_model_not_found() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let resolver = make_resolver(db, None).await;

        // test mode (deployment_mode is "onprem" but no env vars set)
        let result = resolver.resolve("nonexistent-model").await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_resolve_pricing_from_model_info_priority() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        // model_info has pricing, litellm_params also has different pricing
        insert_plaintext_model(
            &db,
            "gpt-4",
            json!({"model": "gpt-4", "api_base": "https://api.openai.com/v1", "input_cost_per_token": 0.1, "output_cost_per_token": 0.2, "custom_llm_provider": "openai"}),
            json!({"input_cost_per_token": 0.00003, "output_cost_per_token": 0.00006}),
        )
        .await;

        let resolver = make_resolver(db, None).await;
        let deployments = resolver.resolve("gpt-4").await.unwrap();

        // model_info takes priority
        assert_eq!(deployments[0].input_cost_per_token, Some(0.00003));
        assert_eq!(deployments[0].output_cost_per_token, Some(0.00006));
    }

    #[tokio::test]
    async fn test_resolve_pricing_fallback_to_params() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        // model_info has no pricing → fallback to litellm_params
        insert_plaintext_model(
            &db,
            "gpt-4",
            json!({"model": "gpt-4", "api_base": "https://api.openai.com/v1", "input_cost_per_token": 0.001, "output_cost_per_token": 0.002, "custom_llm_provider": "openai"}),
            json!({}),
        )
        .await;

        let resolver = make_resolver(db, None).await;
        let deployments = resolver.resolve("gpt-4").await.unwrap();

        // fallback to litellm_params
        assert_eq!(deployments[0].input_cost_per_token, Some(0.001));
        assert_eq!(deployments[0].output_cost_per_token, Some(0.002));
    }

    #[tokio::test]
    async fn test_resolve_provider_type_from_custom_llm_provider() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        insert_plaintext_model(
            &db,
            "claude",
            json!({"model": "claude-3", "api_base": "https://api.anthropic.com/v1", "api_key": "sk-ant-test", "custom_llm_provider": "anthropic"}),
            json!({}),
        )
        .await;

        let resolver = make_resolver(db, None).await;
        let deployments = resolver.resolve("claude").await.unwrap();

        assert_eq!(deployments[0].provider_type, ProviderType::AnthropicNative);
    }

    #[tokio::test]
    async fn test_resolve_list_by_name_single_row() {
        let db = Database::init("sqlite::memory:").await.unwrap();

        insert_plaintext_model(
            &db,
            "gpt-4",
            json!({"model": "gpt-4", "api_base": "https://api.openai.com/v1", "custom_llm_provider": "openai"}),
            json!({}),
        )
        .await;

        let resolver = make_resolver(db, None).await;
        let deployments = resolver.resolve("gpt-4").await.unwrap();

        // model_name has UNIQUE constraint — returns 1 row, but the API returns Vec
        assert_eq!(deployments.len(), 1);
    }

    #[tokio::test]
    async fn test_resolve_decrypt_failure_returns_error() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let master_key = "sk-test-master-key-32chars!!!";

        // Encrypt with one key, try to decrypt with different key
        let params = json!({"model": "gpt-4"});
        let wrong_encrypted = encrypt_litellm_value(&params.to_string(), "some-other-key-32chrs!!!").unwrap();

        let encrypted_val: Value = serde_json::from_str(&format!("\"{}\"", wrong_encrypted)).unwrap();
        insert_plaintext_model(&db, "gpt-4", encrypted_val, json!({})).await;

        let resolver = make_resolver(db, Some(master_key)).await;
        let result = resolver.resolve("gpt-4").await;
        assert!(result.is_err());
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"].as_str(), Some("crypto_error"));
    }

    #[tokio::test]
    async fn test_resolve_encrypted_params_no_master_key() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        let params = json!({"model": "gpt-4"});
        let encrypted = encrypt_litellm_value(&params.to_string(), "some-key-32chars-long!!!!!").unwrap();
        let encrypted_val: Value = serde_json::from_str(&format!("\"{}\"", encrypted)).unwrap();
        insert_plaintext_model(&db, "gpt-4", encrypted_val, json!({})).await;

        let resolver = make_resolver(db, None).await; // NO master key
        let result = resolver.resolve("gpt-4").await;
        assert!(result.is_err());
        let (status, body) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["type"].as_str(), Some("config_error"));
    }

    #[tokio::test]
    async fn test_resolve_raw_params_preserved() {
        let db = Database::init("sqlite::memory:").await.unwrap();
        insert_plaintext_model(
            &db,
            "gpt-4",
            json!({"model": "gpt-4", "api_base": "https://api.openai.com/v1", "rpm": 100, "tpm": 5000, "custom_llm_provider": "openai"}),
            json!({}),
        )
        .await;

        let resolver = make_resolver(db, None).await;
        let deployments = resolver.resolve("gpt-4").await.unwrap();

        // raw_params preserves all original fields, not just the resolved ones
        assert_eq!(deployments[0].raw_params["rpm"].as_i64(), Some(100));
        assert_eq!(deployments[0].raw_params["tpm"].as_i64(), Some(5000));
    }
}
