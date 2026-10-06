//! Web search configuration — two levels: provider → instances.
//!
//! A **provider** is a logical backend: it carries the `kind` discriminant
//! (which client code to use) plus pricing and timeout. An **instance** is a
//! physical endpoint: its own `base_url` / `api_key` / `weight` / `enabled`.
//! SearXNG's typical deployment is several instances behind one logical
//! backend, so a single-`base_url` model would be wrong.
//!
//! These are plain deserializable structs and the loader
//! ([`crate::config_loader::build_websearch_registry`]) takes an already-parsed
//! value rather than reading a file. That keeps Stage 138's DB-backed config
//! source a pure addition: build the same structs from DB rows and every piece
//! of validation, key decryption and instance selection is reused unchanged.

use serde::{Deserialize, Serialize};

use crate::websearch::types::{DEFAULT_MAX_RESULTS, DEFAULT_SNIPPET_MAX_CHARS};

/// Provider kinds implemented today.
///
/// Adding a vendor = one new `websearch/<name>.rs` plus one arm here; the error
/// message for an unknown kind enumerates this list automatically, which is
/// what makes "adding a provider is purely additive" a checkable claim rather
/// than a promise.
pub const SUPPORTED_KINDS: &[&str] = &["searxng"];

/// Top-level `web_search:` block. Absent → the whole layer is off.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    /// Master switch. `false` disables the layer entirely (same semantics as
    /// `CacheConfig.enabled`).
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Provider used when the caller names none. Must appear in `providers`.
    pub default_provider: String,

    /// Provider-level failover chain. With one provider configured this is a
    /// no-op in production, but it is still parsed and validated so that adding
    /// a second vendor is a YAML change rather than a code change.
    #[serde(default)]
    pub failover_order: Vec<String>,

    /// Server-side ceiling on result count. Clients may lower, never raise.
    #[serde(default = "default_max_results")]
    pub max_results: usize,

    /// Global per-search timeout. Overridable per provider.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// Transport-level retries (5xx / network only, never 4xx).
    #[serde(default = "default_retries")]
    pub retries: u32,

    /// Per-result snippet character cap.
    #[serde(default = "default_snippet_max_chars")]
    pub snippet_max_chars: usize,

    /// Mutually exclusive with `blocked_domains`.
    #[serde(default)]
    pub allowed_domains: Vec<String>,

    /// Mutually exclusive with `allowed_domains`.
    #[serde(default)]
    pub blocked_domains: Vec<String>,

    /// Empty → direct connection; non-empty → egress through this proxy.
    #[serde(default)]
    pub proxy_url: String,

    /// Instance-level: consecutive failures before cooldown (`router.rs:455`).
    #[serde(default = "default_allowed_fails")]
    pub allowed_fails: u32,

    /// Instance-level: cooldown duration in seconds (`router.rs:456`).
    #[serde(default = "default_cooldown_secs")]
    pub cooldown_secs: f64,

    /// Logical backends. N-ary, `kind` is the discriminant.
    #[serde(default)]
    pub providers: Vec<WebSearchProviderConfig>,
}

/// One logical backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchProviderConfig {
    /// Referenced by `default_provider` / `failover_order`.
    pub name: String,

    /// Which client implementation to use. See [`SUPPORTED_KINDS`].
    pub kind: String,

    /// USD per query.
    ///
    /// Defaults to `0.01` (= $10/1k), matching OpenAI's list price and
    /// sub2api's `174_group_web_search_price_per_call.sql`. **That default is a
    /// list-price placeholder, not SearXNG's self-hosted cost** — amortized
    /// self-hosting is typically around `1e-4`, so leaving it unchanged
    /// overstates cost by roughly 50x. Deployers should rewrite it as
    /// `(server + bandwidth + ops) / monthly queries`. A non-zero default is
    /// deliberate: over-stating is visible and one line to fix, whereas a
    /// silent `0.0` makes "what did search cost" permanently unanswerable.
    #[serde(default = "default_cost_per_query")]
    pub cost_per_query: f64,

    /// Overrides the global `timeout_ms`. SearXNG measures 2.0–3.1s because it
    /// waits for every configured engine, so the global 5000 is tight for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,

    /// Physical endpoints. Must be non-empty with at least one enabled.
    #[serde(default)]
    pub instances: Vec<WebSearchInstanceConfig>,
}

/// One physical endpoint of a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchInstanceConfig {
    pub base_url: String,

    /// Plaintext or a `v2:gcm:` ciphertext — decrypted at load time by
    /// `crypto::decrypt_litellm_value`, which passes non-ciphertext through
    /// unchanged. SearXNG needs no auth, so this is empty for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Traffic share. If **any** instance declares a weight, all selection goes
    /// weighted (`router.rs:366-377`); `weight: 0` excludes an instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<i64>,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}
fn default_max_results() -> usize {
    DEFAULT_MAX_RESULTS
}
fn default_timeout_ms() -> u64 {
    5000
}
fn default_retries() -> u32 {
    1
}
fn default_snippet_max_chars() -> usize {
    DEFAULT_SNIPPET_MAX_CHARS
}
fn default_allowed_fails() -> u32 {
    3
}
fn default_cooldown_secs() -> f64 {
    60.0
}
fn default_cost_per_query() -> f64 {
    0.01
}

impl WebSearchConfig {
    /// Validate the whole block. Any violation fails startup rather than
    /// surfacing at request time (litellm's "validate at load, refuse to boot
    /// on bad values" convention).
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        if self.providers.is_empty() {
            return Err("web_search.providers must not be empty when enabled".to_string());
        }

        for p in &self.providers {
            if !SUPPORTED_KINDS.contains(&p.kind.as_str()) {
                return Err(format!(
                    "unsupported provider kind \"{}\" for provider \"{}\"; supported kinds: {}",
                    p.kind,
                    p.name,
                    SUPPORTED_KINDS.join(", ")
                ));
            }
            if p.instances.is_empty() {
                return Err(format!(
                    "web_search provider \"{}\" has no instances",
                    p.name
                ));
            }
            if !p.instances.iter().any(|i| i.enabled) {
                return Err(format!(
                    "web_search provider \"{}\" has no enabled instance",
                    p.name
                ));
            }
            for i in &p.instances {
                if i.base_url.trim().is_empty() {
                    return Err(format!(
                        "web_search provider \"{}\" has an instance with empty base_url",
                        p.name
                    ));
                }
            }
        }

        if !self
            .providers
            .iter()
            .any(|p| p.name == self.default_provider)
        {
            return Err(format!(
                "web_search.default_provider \"{}\" is not among providers: {}",
                self.default_provider,
                self.provider_names().join(", ")
            ));
        }

        for name in &self.failover_order {
            if !self.providers.iter().any(|p| &p.name == name) {
                return Err(format!(
                    "web_search.failover_order entry \"{}\" is not among providers: {}",
                    name,
                    self.provider_names().join(", ")
                ));
            }
        }

        if !self.allowed_domains.is_empty() && !self.blocked_domains.is_empty() {
            return Err(
                "web_search.allowed_domains and blocked_domains are mutually exclusive".to_string(),
            );
        }

        Ok(())
    }

    fn provider_names(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.name.clone()).collect()
    }

    /// Effective attempt order: `default_provider` first, then
    /// `failover_order` minus the default, skipping unknown names (validation
    /// has already rejected those when enabled).
    pub fn attempt_order(&self) -> Vec<String> {
        let mut order = vec![self.default_provider.clone()];
        for name in &self.failover_order {
            if !order.contains(name) {
                order.push(name.clone());
            }
        }
        order
    }
}

impl WebSearchProviderConfig {
    /// This provider's timeout, falling back to the global value.
    pub fn effective_timeout_ms(&self, global_ms: u64) -> u64 {
        self.timeout_ms.unwrap_or(global_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_YAML: &str = r#"
enabled: true
default_provider: searxng
failover_order: [searxng]
max_results: 5
timeout_ms: 5000
retries: 1
snippet_max_chars: 2000
allowed_domains: []
blocked_domains: []
proxy_url: ""
allowed_fails: 3
cooldown_secs: 60
providers:
  - name: searxng
    kind: searxng
    cost_per_query: 0.0002
    timeout_ms: 8000
    instances:
      - base_url: http://searxng-a.internal:8080
        weight: 2
        enabled: true
      - base_url: http://searxng-b.internal:8080
        weight: 1
        enabled: true
"#;

    const MINIMAL_YAML: &str = r#"
enabled: true
default_provider: searxng
providers:
  - name: searxng
    kind: searxng
    instances:
      - base_url: http://localhost:9099
"#;

    fn parse(yaml: &str) -> WebSearchConfig {
        serde_yaml::from_str(yaml).expect("parse web_search yaml")
    }

    #[test]
    fn test_websearch_config_parse_full_yaml() {
        let cfg = parse(FULL_YAML);
        assert!(cfg.enabled);
        assert_eq!(cfg.default_provider, "searxng");
        assert_eq!(cfg.failover_order, vec!["searxng"]);
        assert_eq!(cfg.max_results, 5);
        assert_eq!(cfg.timeout_ms, 5000);
        assert_eq!(cfg.retries, 1);
        assert_eq!(cfg.snippet_max_chars, 2000);
        assert_eq!(cfg.allowed_fails, 3);
        assert_eq!(cfg.cooldown_secs, 60.0);
        assert_eq!(cfg.providers.len(), 1);
        assert_eq!(cfg.providers[0].instances.len(), 2);
        assert_eq!(cfg.providers[0].instances[0].weight, Some(2));
        assert_eq!(cfg.providers[0].cost_per_query, 0.0002);
        cfg.validate().expect("full yaml must validate");
    }

    #[test]
    fn test_websearch_config_defaults_applied() {
        let cfg = parse(MINIMAL_YAML);
        assert_eq!(cfg.max_results, 5);
        assert_eq!(cfg.timeout_ms, 5000);
        assert_eq!(cfg.snippet_max_chars, 2000);
        assert_eq!(cfg.allowed_fails, 3);
        assert_eq!(cfg.cooldown_secs, 60.0);
        assert_eq!(cfg.retries, 1);
        assert_eq!(
            cfg.providers[0].cost_per_query, 0.01,
            "default is the $10/1k list-price placeholder, deliberately non-zero"
        );
        assert!(cfg.providers[0].instances[0].enabled);
        assert!(cfg.providers[0].instances[0].weight.is_none());
        cfg.validate().expect("minimal yaml must validate");
    }

    #[test]
    fn test_websearch_config_validate_unknown_default_provider() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.default_provider = "nope".to_string();
        let err = cfg.validate().expect_err("unknown default must fail");
        assert!(err.contains("nope"), "{err}");
        assert!(err.contains("searxng"), "{err}");
    }

    #[test]
    fn test_websearch_config_validate_unknown_failover_entry() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.failover_order = vec!["ghost".to_string()];
        let err = cfg
            .validate()
            .expect_err("unknown failover entry must fail");
        assert!(err.contains("ghost"), "{err}");
    }

    #[test]
    fn test_websearch_config_validate_domain_lists_mutually_exclusive() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.allowed_domains = vec!["a.com".into()];
        cfg.blocked_domains = vec!["b.com".into()];
        let err = cfg.validate().expect_err("both lists must fail");
        assert!(err.contains("mutually exclusive"), "{err}");
    }

    #[test]
    fn test_websearch_config_unknown_kind_error_lists_supported_kinds() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.providers[0].kind = "tavily".to_string();
        let err = cfg.validate().expect_err("unimplemented kind must fail");
        assert!(
            err.contains("tavily"),
            "must name the offending kind: {err}"
        );
        assert!(
            err.contains("searxng"),
            "must enumerate supported kinds so new arms self-document: {err}"
        );
    }

    #[test]
    fn test_websearch_config_validate_rejects_empty_instances() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.providers[0].instances.clear();
        let err = cfg.validate().expect_err("empty instances must fail");
        assert!(err.contains("no instances"), "{err}");

        let mut cfg = parse(MINIMAL_YAML);
        cfg.providers[0].instances[0].enabled = false;
        let err = cfg.validate().expect_err("all-disabled must fail");
        assert!(err.contains("no enabled instance"), "{err}");
    }

    #[test]
    fn test_websearch_config_validate_rejects_empty_base_url() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.providers[0].instances[0].base_url = "  ".to_string();
        let err = cfg.validate().expect_err("empty base_url must fail");
        assert!(err.contains("base_url"), "{err}");
    }

    #[test]
    fn test_websearch_config_provider_timeout_overrides_global() {
        let cfg = parse(FULL_YAML);
        assert_eq!(cfg.providers[0].effective_timeout_ms(cfg.timeout_ms), 8000);

        let cfg = parse(MINIMAL_YAML);
        assert_eq!(
            cfg.providers[0].effective_timeout_ms(cfg.timeout_ms),
            5000,
            "absent per-provider timeout falls back to global"
        );
    }

    #[test]
    fn test_websearch_config_disabled_skips_validation() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.enabled = false;
        cfg.providers.clear();
        cfg.validate()
            .expect("a disabled layer need not be well-formed");
    }

    #[test]
    fn test_config_example_yaml_block_parses_into_this_struct() {
        // The commented `web_search:` block in config.example.yaml is the only
        // documentation operators copy from. If it drifts from this struct it
        // silently becomes wrong, so uncomment and deserialize it here.
        let example = include_str!("../../../../config.example.yaml");
        let start = example
            .find("\n# web_search:")
            .expect("config.example.yaml must document a web_search block")
            + 1;

        let mut yaml = String::new();
        for line in example[start..].lines() {
            let Some(body) = line
                .strip_prefix("# ")
                .or_else(|| (line == "#").then_some(""))
            else {
                break;
            };
            if body.trim_start().starts_with('#') {
                continue; // nested explanatory comment
            }
            if body.starts_with("The instance must have") {
                break; // trailing prose, no longer YAML
            }
            yaml.push_str(body);
            yaml.push('\n');
        }

        #[derive(serde::Deserialize)]
        struct Wrapper {
            web_search: WebSearchConfig,
        }
        let parsed: Wrapper = serde_yaml::from_str(&yaml).unwrap_or_else(|e| {
            panic!("documented web_search block no longer parses: {e}\n{yaml}")
        });
        parsed
            .web_search
            .validate()
            .expect("the documented example must also pass validation");
        assert_eq!(parsed.web_search.providers.len(), 1);
        assert_eq!(parsed.web_search.providers[0].instances.len(), 2);
        assert_eq!(
            parsed.web_search.providers[0].timeout_ms,
            Some(8000),
            "the example must keep the SearXNG-specific timeout override"
        );
    }

    #[test]
    fn test_attempt_order_puts_default_first_without_duplicates() {
        let mut cfg = parse(MINIMAL_YAML);
        cfg.failover_order = vec!["searxng".into()];
        assert_eq!(cfg.attempt_order(), vec!["searxng"]);
    }
}
