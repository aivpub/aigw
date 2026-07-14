//! Deployment — upstream routing target
//!
//! A Deployment is the resolved product of one proxy_models row:
//! - api_base + api_key (decrypted)
//! - upstream_model (the name to send to the actual upstream)
//! - provider_type (decides which MessageAdapter to use)
//! - pricing info (from model_info / litellm_params)
//! - raw_params (decrypted full litellm_params, for adapter use)

use serde_json::Value;

/// One upstream deployment — product of resolving one proxy_models row.
///
/// ModelResolver::resolve() returns `Vec<Deployment>` because
/// the same model_name may have multiple rows (different api_base/key).
#[derive(Debug, Clone)]
pub struct Deployment {
    /// Upstream API base URL (e.g. https://api.openai.com/v1)
    pub api_base: String,
    /// Upstream API key (decrypted plaintext)
    pub api_key: Option<String>,
    /// Model name to send to upstream (may differ from aigw proxy name)
    pub upstream_model: String,
    /// The type of upstream — decides MessageAdapter selection
    pub provider_type: ProviderType,
    /// USD per input token
    pub input_cost_per_token: Option<f64>,
    /// USD per output token
    pub output_cost_per_token: Option<f64>,
    /// Decrypted litellm_params JSON (all original fields preserved)
    pub raw_params: Value,
}

/// Upstream provider type.
///
/// Inferred from litellm_params.custom_llm_provider (primary),
/// with api_base as fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderType {
    /// OpenAI-compatible API (/v1/chat/completions)
    /// Covers: OpenAI, DeepSeek, Ollama, vLLM, Groq, etc.
    OpenAICompatible,
    /// Anthropic Messages API (/v1/messages) — native protocol
    AnthropicNative,
}

impl ProviderType {
    /// Infer ProviderType from litellm_params.custom_llm_provider field.
    ///
    /// Primary: custom_llm_provider value:
    ///   "anthropic" → AnthropicNative
    ///   "openai" / "deepseek" / "ollama" / "hosted_vllm" / ... → OpenAICompatible
    ///
    /// Fallback: api_base contains "anthropic" → AnthropicNative, else OpenAICompatible.
    pub fn infer(custom_llm_provider: Option<&str>, api_base: &str) -> Self {
        match custom_llm_provider {
            Some("anthropic") => ProviderType::AnthropicNative,
            Some(_) => ProviderType::OpenAICompatible,
            None => {
                if api_base.contains("anthropic") {
                    ProviderType::AnthropicNative
                } else {
                    ProviderType::OpenAICompatible
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type_infer_from_custom_llm_provider() {
        assert_eq!(
            ProviderType::infer(Some("openai"), "https://api.openai.com/v1"),
            ProviderType::OpenAICompatible
        );
        assert_eq!(
            ProviderType::infer(Some("deepseek"), "https://api.deepseek.com/v1"),
            ProviderType::OpenAICompatible
        );
        assert_eq!(
            ProviderType::infer(Some("ollama"), "http://localhost:11434"),
            ProviderType::OpenAICompatible
        );
        assert_eq!(
            ProviderType::infer(Some("hosted_vllm"), "https://vllm.example.com"),
            ProviderType::OpenAICompatible
        );
        assert_eq!(
            ProviderType::infer(Some("anthropic"), "https://api.anthropic.com"),
            ProviderType::AnthropicNative
        );
    }

    #[test]
    fn test_provider_type_infer_fallback_api_base() {
        // No custom_llm_provider → fallback to api_base
        assert_eq!(
            ProviderType::infer(None, "https://api.openai.com/v1"),
            ProviderType::OpenAICompatible
        );
        assert_eq!(
            ProviderType::infer(None, "https://api.anthropic.com/v1/messages"),
            ProviderType::AnthropicNative
        );
        assert_eq!(
            ProviderType::infer(None, "https://api.deepseek.com/v1"),
            ProviderType::OpenAICompatible
        );
    }

    #[test]
    fn test_deployment_construction() {
        let d = Deployment {
            api_base: "https://api.openai.com/v1".to_string(),
            api_key: Some("sk-test".to_string()),
            upstream_model: "gpt-4".to_string(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: Some(0.00003),
            output_cost_per_token: Some(0.00006),
            raw_params: serde_json::json!({"model": "gpt-4", "custom_llm_provider": "openai"}),
        };

        assert_eq!(d.api_base, "https://api.openai.com/v1");
        assert_eq!(d.api_key.as_deref(), Some("sk-test"));
        assert_eq!(d.upstream_model, "gpt-4");
        assert_eq!(d.provider_type, ProviderType::OpenAICompatible);
        assert_eq!(d.input_cost_per_token, Some(0.00003));
        assert_eq!(d.output_cost_per_token, Some(0.00006));
        assert_eq!(
            d.raw_params["custom_llm_provider"].as_str(),
            Some("openai")
        );
    }
}
