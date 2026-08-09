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
    /// USD per cache-read input token (Anthropic cache_read_input_tokens / OpenAI prompt_tokens_details.cached_tokens)
    /// Falls back to input_cost_per_token when absent (litellm behaviour)
    pub cache_read_input_token_cost: Option<f64>,
    /// USD per cache-creation input token (Anthropic cache_creation_input_tokens / OpenAI prompt_tokens_details.cache_write_tokens)
    /// Typically 25% more than input_cost; falls back to input_cost_per_token when absent
    pub cache_creation_input_token_cost: Option<f64>,
    /// Decrypted litellm_params JSON (all original fields preserved)
    pub raw_params: Value,
    /// proxy_models UUID (model_id column)
    pub model_id: Option<String>,
    /// proxy_models.model_name — deployment name for model_group (litellm-compatible)
    pub model_group: Option<String>,
    /// litellm_params.custom_llm_provider value — e.g. "openai", "anthropic"
    pub custom_llm_provider: Option<String>,
    /// chat template compatibility mode from model_info
    /// "auto" (default/absent) / "strict" (fold extra system messages) / "loose" (passthrough)
    pub chat_template_compat: Option<String>,
    /// Per-modality input pricing (TD-012b), extracted from model_info
    /// `modal_pricing: {image, audio, video}` (USD per 1M tokens, e.g. Gemini
    /// embeddings image $0.45 / audio $6.50 / video $12.00). None when the
    /// deployment is single-modal (falls back to input_cost_per_token).
    pub modal_pricing: Option<crate::models::ModalPricing>,
    /// Runtime cooldown tracking — not persisted, managed by Router.
    /// Number of consecutive failures for this deployment.
    pub fail_count: u32,
    /// If Some(instant), this deployment is in cooldown until that time.
    pub cooldown_until: Option<std::time::Instant>,
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

    /// Whether this provider is Anthropic-native style.
    /// Anthropic returns cache tokens in top-level usage fields (cache_read_input_tokens,
    /// cache_creation_input_tokens) and its input_tokens does NOT include cached tokens.
    /// Callers must normalize before calc_spend.
    pub fn is_anthropic_style(&self) -> bool {
        matches!(self, ProviderType::AnthropicNative)
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
            cache_read_input_token_cost: Some(0.0000075),
            cache_creation_input_token_cost: Some(0.0000375),
            raw_params: serde_json::json!({"model": "gpt-4", "custom_llm_provider": "openai"}),
            model_id: Some("m1".to_string()),
            model_group: Some("gpt-4".to_string()),
            custom_llm_provider: Some("openai".to_string()),
            chat_template_compat: None,
            modal_pricing: None,
            fail_count: 0,
            cooldown_until: None,
        };

        assert_eq!(d.api_base, "https://api.openai.com/v1");
        assert_eq!(d.api_key.as_deref(), Some("sk-test"));
        assert_eq!(d.upstream_model, "gpt-4");
        assert_eq!(d.provider_type, ProviderType::OpenAICompatible);
        assert_eq!(d.input_cost_per_token, Some(0.00003));
        assert_eq!(d.output_cost_per_token, Some(0.00006));
        assert_eq!(d.cache_read_input_token_cost, Some(0.0000075));
        assert_eq!(d.cache_creation_input_token_cost, Some(0.0000375));
        assert_eq!(d.raw_params["custom_llm_provider"].as_str(), Some("openai"));
    }
}
