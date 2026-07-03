//! Provider configuration and registry
//!
//! Maps model names to upstream LLM provider configurations,
//! supporting load-balanced instances with routing strategies.

use crate::router::{self, RouterState, Strategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// Upstream LLM provider configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// Base URL for the provider API (e.g. https://api.openai.com/v1)
    pub base_url: String,
    /// API key for the provider
    pub api_key: String,
    /// Routing strategy name
    #[serde(default)]
    pub routing_strategy: String,
    /// Number of consecutive failures before cooldown
    #[serde(default = "default_allowed_fails")]
    pub allowed_fails: u32,
    /// Cooldown duration in seconds
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: f64,
    /// List of provider instance URLs (for load-balanced providers)
    #[serde(default)]
    pub instances: Vec<ProviderInstance>,
}

/// A single provider instance endpoint
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderInstance {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

fn default_allowed_fails() -> u32 {
    3
}

fn default_cooldown_secs() -> f64 {
    30.0
}

/// Provider registry — maps model names to their provider configs
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderRegistry {
    /// Provider configurations keyed by provider name
    pub providers: HashMap<String, ProviderConfig>,
    /// Mapping from model name to provider config key
    #[serde(default)]
    pub model_routing: HashMap<String, String>,
}

impl ProviderRegistry {
    /// Create an empty provider registry
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            model_routing: HashMap::new(),
        }
    }

    /// Create a default registry with common providers (OpenAI, Anthropic).
    /// API keys are read from environment variables.
    pub fn default_with_env() -> Self {
        let mut registry = Self::new();

        // OpenAI
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            registry.providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    base_url: "https://api.openai.com/v1".to_string(),
                    api_key: key,
                    routing_strategy: String::new(),
                    allowed_fails: default_allowed_fails(),
                    cooldown_secs: default_cooldown_secs(),
                    instances: vec![],
                },
            );
        }

        // Anthropic
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            registry.providers.insert(
                "anthropic".to_string(),
                ProviderConfig {
                    base_url: "https://api.anthropic.com/v1".to_string(),
                    api_key: key,
                    routing_strategy: String::new(),
                    allowed_fails: default_allowed_fails(),
                    cooldown_secs: default_cooldown_secs(),
                    instances: vec![],
                },
            );
        }

        registry
    }

    /// Get the provider config for a given model name.
    /// First checks the model_routing mapping, then falls back to
    /// checking if the model name itself is a provider key.
    pub fn get_provider(&self, model: &str) -> Option<&ProviderConfig> {
        // Check model_routing mapping first
        if let Some(provider_key) = self.model_routing.get(model) {
            return self.providers.get(provider_key);
        }
        // Fallback: treat model as provider key
        // e.g., "openai/gpt-4" -> extract "openai"
        if let Some(slash_pos) = model.find('/') {
            let provider_key = &model[..slash_pos];
            return self.providers.get(provider_key);
        }
        // Last resort: try model name as-is
        self.providers.get(model)
    }

    /// Select the best instance URL for a model using the routing strategy.
    ///
    /// If the provider has `instances` configured, uses the router to pick one.
    /// Otherwise, returns the provider's `base_url` directly.
    pub async fn select_url(&self, model: &str, router_state: &RouterState) -> Option<String> {
        let provider = self.get_provider(model)?;

        if provider.instances.is_empty() {
            return Some(provider.base_url.clone());
        }

        let instance_urls: Vec<String> = provider.instances.iter().map(|i| i.url.clone()).collect();

        let Ok(strategy) = Strategy::from_str(&provider.routing_strategy);

        router::select_instance(
            &instance_urls,
            router_state,
            strategy,
            provider.allowed_fails,
            provider.cooldown_secs,
        )
        .await
    }

    /// Get the API key for the provider serving a model
    pub fn get_api_key(&self, model: &str) -> Option<&str> {
        self.get_provider(model).map(|p| p.api_key.as_str())
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn test_provider_registry_get_provider_by_model_routing() {
        let mut registry = ProviderRegistry::new();
        registry.providers.insert(
            "openai".to_string(),
            ProviderConfig {
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                routing_strategy: String::new(),
                allowed_fails: 3,
                cooldown_secs: 30.0,
                instances: vec![],
            },
        );
        registry
            .model_routing
            .insert("gpt-4".to_string(), "openai".to_string());

        let provider = registry.get_provider("gpt-4");
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_provider_registry_get_provider_by_slash_prefix() {
        let mut registry = ProviderRegistry::new();
        registry.providers.insert(
            "openai".to_string(),
            ProviderConfig {
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                routing_strategy: String::new(),
                allowed_fails: 3,
                cooldown_secs: 30.0,
                instances: vec![],
            },
        );

        let provider = registry.get_provider("openai/gpt-4");
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_provider_registry_missing_model() {
        let registry = ProviderRegistry::new();
        assert!(registry.get_provider("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_select_url_with_instances() {
        let mut registry = ProviderRegistry::new();
        registry.providers.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: "https://primary.example.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                routing_strategy: "simple-shuffle".to_string(),
                allowed_fails: 3,
                cooldown_secs: 5.0,
                instances: vec![
                    ProviderInstance {
                        url: "https://instance1.example.com/v1".to_string(),
                        weight: None,
                    },
                    ProviderInstance {
                        url: "https://instance2.example.com/v1".to_string(),
                        weight: None,
                    },
                ],
            },
        );
        registry
            .model_routing
            .insert("test-model".to_string(), "test-provider".to_string());

        let router_state: RouterState = Arc::new(Mutex::new(HashMap::new()));

        let url = registry.select_url("test-model", &router_state).await;
        assert!(url.is_some());
        let selected = url.unwrap();
        assert!(
            selected == "https://instance1.example.com/v1"
                || selected == "https://instance2.example.com/v1"
        );
    }

    #[tokio::test]
    async fn test_select_url_without_instances_uses_base_url() {
        let mut registry = ProviderRegistry::new();
        registry.providers.insert(
            "test-provider".to_string(),
            ProviderConfig {
                base_url: "https://primary.example.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                routing_strategy: String::new(),
                allowed_fails: 3,
                cooldown_secs: 30.0,
                instances: vec![],
            },
        );
        registry
            .model_routing
            .insert("no-instance-model".to_string(), "test-provider".to_string());

        let router_state: RouterState = Arc::new(Mutex::new(HashMap::new()));

        let url = registry
            .select_url("no-instance-model", &router_state)
            .await;
        assert_eq!(url, Some("https://primary.example.com/v1".to_string()));
    }

    #[test]
    fn test_get_api_key() {
        let mut registry = ProviderRegistry::new();
        registry.providers.insert(
            "openai".to_string(),
            ProviderConfig {
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "sk-my-key".to_string(),
                routing_strategy: String::new(),
                allowed_fails: 3,
                cooldown_secs: 30.0,
                instances: vec![],
            },
        );
        registry
            .model_routing
            .insert("gpt-4".to_string(), "openai".to_string());

        assert_eq!(registry.get_api_key("gpt-4"), Some("sk-my-key"));
        assert_eq!(registry.get_api_key("unknown"), None);
    }
}
