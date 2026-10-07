//! Web search wiring shared by the three request entries (Phase 53, Stage 135).
//!
//! Each entry has its own native "search please" shape (see
//! [`aigw_core::websearch::trigger`]); this module runs the detect → search →
//! inject sequence uniformly and hands back a status for the response metadata.
//!
//! Two ordering rules are load-bearing:
//!
//! - **Detect before `adapt_request`.** Search results are folded into the
//!   request the adapter is about to normalize, so the client's own tool
//!   declarations are still intact here.
//! - **A request that asked for search bypasses the exact-match cache.** The
//!   cache key is computed from the pre-injection body and the cached body was
//!   stored without results, so serving it would both waste the search we just
//!   paid for and silently hand back a result-less answer.

use aigw_core::adapter::ClientProtocol;
use aigw_core::deployment::Deployment;
use aigw_core::websearch::{self, SearchServeOutcome, WebSearchRegistry};
use serde_json::{json, Value};

/// Status of the search layer for one request, as reported in the response
/// metadata (`{"aigw": {"web_search": {...}}}`). `None` means the trigger never
/// fired and nothing should be reported.
#[derive(Debug, Clone)]
pub struct SearchStatus {
    pub status: &'static str,
    pub provider: String,
    pub results: usize,
}

impl SearchStatus {
    /// The `aigw.web_search` object, or `None` for the "no trigger" case.
    pub fn to_value(&self) -> Value {
        json!({
            "status": self.status,
            "provider": self.provider,
            "results": self.results,
        })
    }
}

/// Which surfaces forward the search tool to a native upstream that runs it
/// itself (so the gateway must not also run its own search).
///
/// - Anthropic request → `AnthropicNative` deployment: `AnthropicPassthrough`
///   round-trips `web_search_20250305` verbatim (adapter.rs:1626).
/// - Responses request → a deployment declaring `"responses"`: `ResponsesPassthrough`
///   forwards `{"type":"web_search"}` verbatim (adapter.rs:108).
///
/// The OpenAI Chat surface has no such passthrough, so it is never exempt.
pub fn upstream_handles_search(protocol: ClientProtocol, deployment: &Deployment) -> bool {
    match protocol {
        ClientProtocol::Anthropic => {
            deployment.provider_type == aigw_core::deployment::ProviderType::AnthropicNative
        }
        ClientProtocol::Responses => deployment
            .supported_standard_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case("responses")),
        ClientProtocol::OpenAI => false,
    }
}

/// Detect a search trigger on `protocol`, and if present run it and fold the
/// results into `body`.
///
/// When the resolved upstream handles search natively (see
/// [`upstream_handles_search`]) this is a no-op: those upstreams run the search
/// themselves, so running a second search here would double-charge and merge
/// two unrelated result sets. Same reasoning as the OAuth branch boundary
/// (§3.6a), applied to the non-OAuth native paths.
///
/// Degrades rather than failing: a search backend outage leaves the request
/// unmodified and reports `degraded`. An absent registry reports
/// `not_configured` and performs no work — the same code path as a configured
/// one, so "config absent means zero behaviour change" needs no second branch.
pub async fn maybe_serve(
    registry: Option<&WebSearchRegistry>,
    protocol: ClientProtocol,
    native_upstream: bool,
    body: &mut Value,
) -> Option<SearchStatus> {
    if native_upstream {
        return None;
    }

    let trigger = match protocol {
        ClientProtocol::OpenAI => websearch::trigger::detect_chat(body),
        ClientProtocol::Responses => body
            .get("tools")
            .and_then(|v| v.as_array())
            .and_then(|tools| websearch::trigger::detect_responses(tools)),
        ClientProtocol::Anthropic => body
            .get("tools")
            .and_then(|v| v.as_array())
            .and_then(|tools| websearch::trigger::detect_anthropic(tools)),
    }?;

    let Some(registry) = registry else {
        tracing::info!(
            surface = trigger.surface.as_str(),
            "web search requested but web_search is not configured; dropping"
        );
        return Some(SearchStatus {
            status: "not_configured",
            provider: String::new(),
            results: 0,
        });
    };

    let now = chrono::Utc::now();
    let outcome = websearch::serve_trigger(registry, &trigger, body, &now).await;
    let results = match &outcome {
        SearchServeOutcome::Injected { results, .. } => *results,
        _ => 0,
    };
    Some(SearchStatus {
        status: outcome.status(),
        provider: outcome.provider().to_string(),
        results,
    })
}

/// Fold the search status into a JSON response body's top-level `aigw` key.
///
/// Kept out of the streaming path deliberately: the SSE loop in
/// `responses.rs` has been the site of three production incidents and is not
/// touched by this Stage.
pub fn attach_status(body: &mut Value, status: &SearchStatus) {
    if let Some(obj) = body.as_object_mut() {
        let entry = obj.entry("aigw").or_insert_with(|| json!({}));
        if let Some(aigw) = entry.as_object_mut() {
            aigw.insert("web_search".to_string(), status.to_value());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_deployment() -> Deployment {
        aigw_core::deployment::Deployment {
            api_base: "http://x".into(),
            api_key: None,
            upstream_model: "m".into(),
            provider_type: aigw_core::deployment::ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            developer_role_passthrough: None,
            supported_standard_types: vec![],
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        }
    }

    #[test]
    fn anthropic_native_upstream_handles_search() {
        let mut dep = openai_deployment();
        dep.provider_type = aigw_core::deployment::ProviderType::AnthropicNative;
        assert!(upstream_handles_search(ClientProtocol::Anthropic, &dep));
        assert!(!upstream_handles_search(ClientProtocol::OpenAI, &dep));
    }

    #[test]
    fn responses_declared_upstream_handles_search() {
        let mut dep = openai_deployment();
        dep.supported_standard_types = vec!["responses".to_string()];
        assert!(upstream_handles_search(ClientProtocol::Responses, &dep));
        assert!(!upstream_handles_search(ClientProtocol::Anthropic, &dep));
    }

    #[tokio::test]
    async fn registry_absent_marks_not_configured() {
        let mut body =
            json!({"messages": [{"role": "user", "content": "q"}], "web_search_options": {}});
        let before = body.clone();
        let status = maybe_serve(None, ClientProtocol::OpenAI, false, &mut body)
            .await
            .expect("trigger fires");
        assert_eq!(status.status, "not_configured");
        assert_eq!(
            body.get("web_search_options"),
            None,
            "the OpenAI-native param is still consumed"
        );
        // Messages themselves are untouched.
        assert_eq!(body["messages"], before["messages"]);
    }

    #[tokio::test]
    async fn no_trigger_reports_nothing() {
        let mut body = json!({"messages": [{"role": "user", "content": "q"}]});
        assert!(maybe_serve(None, ClientProtocol::OpenAI, false, &mut body)
            .await
            .is_none());
        let mut body = json!({"input": "q"});
        assert!(
            maybe_serve(None, ClientProtocol::Responses, false, &mut body)
                .await
                .is_none()
        );
    }

    #[test]
    fn attach_status_creates_aigw_key() {
        let mut body = json!({"id": "x"});
        attach_status(
            &mut body,
            &SearchStatus {
                status: "ok",
                provider: "searxng".into(),
                results: 3,
            },
        );
        assert_eq!(body["aigw"]["web_search"]["status"], "ok");
        assert_eq!(body["aigw"]["web_search"]["results"], 3);
    }
}
