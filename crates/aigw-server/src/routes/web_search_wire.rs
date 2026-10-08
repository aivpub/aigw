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

use super::chat::calc_search_spend;

/// Status of the search layer for one request, as reported in the response
/// metadata (`{"aigw": {"web_search": {...}}}`). `None` means the trigger never
/// fired and nothing should be reported.
#[derive(Debug, Clone)]
pub struct SearchStatus {
    /// `true` when a search request actually reached a provider (status `ok` or
    /// `degraded`). `not_configured` and `no_target` never searched, so they must
    /// not opt a request out of the exact-match cache — they consume the trigger
    /// field but perform no work.
    pub searched: bool,
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
    ctx: &SearchSpendContext<'_>,
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
            searched: false,
            status: "not_configured",
            provider: String::new(),
            results: 0,
        });
    };

    let now = chrono::Utc::now();
    let started = std::time::Instant::now();
    let outcome = websearch::serve_trigger(registry, &trigger, body, &now).await;
    let results = outcome.results_detail().len();

    record_search_spend(ctx, registry, &outcome, started.elapsed()).await;

    Some(SearchStatus {
        searched: outcome.performed_search(),
        status: outcome.status(),
        provider: outcome.provider().to_string(),
        results,
    })
}

/// Everything the search SpendLog row inherits from the LLM call that triggered
/// it. Held by the caller (the route), because only it has the resolved auth
/// identity and session context.
pub struct SearchSpendContext<'a> {
    pub db: &'a aigw_core::db::Database,
    /// The LLM row's `call_id` — recorded as `metadata.parent_call_id`, which is
    /// how a search row is joined back to the request that caused it.
    pub parent_call_id: &'a str,
    pub token_hash: &'a str,
    pub user_id: Option<&'a str>,
    pub team_id: Option<&'a str>,
    pub organization_id: Option<&'a str>,
    pub end_user: Option<&'a str>,
    pub requester_ip: Option<&'a str>,
    pub session_id: Option<&'a str>,
}

/// Write one `call_type="search"` SpendLog row and fold its cost into the four
/// budget counters.
///
/// A search is billed per call, not per token — aigw's first non-token pricing
/// unit — so the row carries the provider, the instance that served it and the
/// unit price snapshot, and zeroes for every token column.
///
/// **A zero-spend row is still written and still increments.** Short-circuiting
/// on `spend > 0.0` would erase the call record for deployments that set
/// `cost_per_query: 0.0`, and would mean switching to a non-zero price later
/// required a code change.
async fn record_search_spend(
    ctx: &SearchSpendContext<'_>,
    registry: &WebSearchRegistry,
    outcome: &SearchServeOutcome,
    elapsed: std::time::Duration,
) {
    if !outcome.performed_search() {
        // NoTarget never reached a provider — billing it would charge for work
        // that did not happen.
        return;
    }

    let provider = outcome.provider().to_string();
    let unit = registry.cost_per_query(Some(&provider));
    let spend = calc_search_spend(1, unit);
    let now = chrono::Utc::now();
    let start_time = now - chrono::Duration::from_std(elapsed).unwrap_or_default();

    let metadata = json!({
        "search_query_count": 1,
        "parent_call_id": ctx.parent_call_id,
        "search_provider": provider.clone(),
        "cost_per_query": unit,
    });

    let log = aigw_core::models::SpendLog {
        call_id: uuid::Uuid::now_v7().to_string(),
        call_type: "search".to_string(),
        api_key: ctx.token_hash.to_string(),
        spend,
        total_tokens: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        start_time,
        end_time: now,
        request_duration_ms: Some(elapsed.as_millis() as i32),
        completion_start_time: None,
        model: format!("{}/search", provider),
        model_id: None,
        // Deliberately not repurposed: this column feeds the "Spend by Model
        // Group" chart, and `mcp_namespaced_tool_name` is part of the
        // daily_spend_queue aggregation key.
        model_group: None,
        custom_llm_provider: Some(provider.clone()),
        api_base: outcome.endpoint().map(str::to_string),
        user: ctx.user_id.map(str::to_string),
        metadata: Some(metadata),
        cache_hit: None,
        cache_key: None,
        request_tags: None,
        team_id: ctx.team_id.map(str::to_string),
        organization_id: ctx.organization_id.map(str::to_string),
        end_user: ctx.end_user.map(str::to_string),
        requester_ip_address: ctx.requester_ip.map(str::to_string),
        messages: outcome.query().map(|q| json!({ "query": q })),
        response: Some(json!({
            "result_count": outcome.results_detail().len(),
            "results": outcome.results_detail(),
        })),
        session_id: ctx.session_id.map(str::to_string),
        // `empty` still means the provider answered — only a transport-level
        // `degraded` is a failure.
        status: Some(
            if matches!(outcome.status(), "ok" | "empty") {
                "success"
            } else {
                "failure"
            }
            .to_string(),
        ),
        mcp_namespaced_tool_name: None,
        agent_id: None,
        proxy_server_request: None,
        body_archived: false,
        parquet_path: None,
        request_id: None,
        image_tokens: None,
    };

    if let Err(e) = ctx.db.insert_spend_log(&log).await {
        tracing::warn!(error = %e, "web search spend log insert failed");
    }

    // Independent of the model call's own increments — the two costs must stay
    // separate or one silently swallows the other (litellm's logged bug).
    let mut incremented = true;
    if let Err(e) = ctx.db.increment_key_spend(ctx.token_hash, spend).await {
        tracing::warn!(error = %e, "search spend key increment failed");
        incremented = false;
    }
    if let Some(uid) = ctx.user_id {
        let _ = ctx.db.increment_user_spend(uid, spend).await;
    }
    if let Some(tid) = ctx.team_id {
        let _ = ctx.db.increment_team_spend(tid, spend).await;
    }
    if let Some(oid) = ctx.organization_id {
        let _ = ctx.db.increment_org_spend(oid, spend).await;
    }
    if incremented {
        tracing::info!(
            parent_call_id = ctx.parent_call_id,
            provider = %provider,
            spend,
            "web search spend recorded"
        );
    }
}

/// Echo the search count back on the response's `usage.server_tool_use`.
///
/// Anthropic defines this shape natively; aigw emits it on all three surfaces so
/// a client can tell how many searches its request caused. Only a real search
/// counts — `no_target` / `not_configured` leave `usage` untouched, so a request
/// that never searched has a byte-identical usage block.
pub fn echo_web_search_requests(body: &mut Value, status: &SearchStatus) {
    if !status.searched {
        return;
    }
    let Some(usage) = body.get_mut("usage").and_then(|v| v.as_object_mut()) else {
        return;
    };
    usage.insert(
        "server_tool_use".to_string(),
        json!({ "web_search_requests": 1 }),
    );
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

    fn test_ctx(db: &aigw_core::db::Database) -> SearchSpendContext<'_> {
        SearchSpendContext {
            db,
            parent_call_id: "parent-1",
            token_hash: "tok",
            user_id: None,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip: None,
            session_id: None,
        }
    }

    #[tokio::test]
    async fn registry_absent_marks_not_configured() {
        let db = aigw_core::db::Database::init("sqlite::memory:")
            .await
            .expect("db");
        let mut body =
            json!({"messages": [{"role": "user", "content": "q"}], "web_search_options": {}});
        let before = body.clone();
        let status = maybe_serve(
            None,
            ClientProtocol::OpenAI,
            false,
            &mut body,
            &test_ctx(&db),
        )
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
        let db = aigw_core::db::Database::init("sqlite::memory:")
            .await
            .expect("db");
        let mut body = json!({"messages": [{"role": "user", "content": "q"}]});
        assert!(maybe_serve(
            None,
            ClientProtocol::OpenAI,
            false,
            &mut body,
            &test_ctx(&db)
        )
        .await
        .is_none());
        let mut body = json!({"input": "q"});
        assert!(maybe_serve(
            None,
            ClientProtocol::Responses,
            false,
            &mut body,
            &test_ctx(&db)
        )
        .await
        .is_none());
    }

    #[test]
    fn attach_status_creates_aigw_key() {
        let mut body = json!({"id": "x"});
        attach_status(
            &mut body,
            &SearchStatus {
                searched: true,
                status: "ok",
                provider: "searxng".into(),
                results: 3,
            },
        );
        assert_eq!(body["aigw"]["web_search"]["status"], "ok");
        assert_eq!(body["aigw"]["web_search"]["results"], 3);
    }

    #[test]
    fn echo_adds_server_tool_use_when_searched() {
        let mut body = json!({"usage": {"prompt_tokens": 1}});
        let st = SearchStatus {
            searched: true,
            status: "ok",
            provider: "searxng".into(),
            results: 3,
        };
        echo_web_search_requests(&mut body, &st);
        assert_eq!(body["usage"]["server_tool_use"]["web_search_requests"], 1);
    }

    #[test]
    fn echo_leaves_usage_untouched_when_not_searched() {
        let mut body = json!({"usage": {"prompt_tokens": 1}});
        let before = body.clone();
        let st = SearchStatus {
            searched: false,
            status: "no_target",
            provider: "searxng".into(),
            results: 0,
        };
        echo_web_search_requests(&mut body, &st);
        assert_eq!(body, before);
    }

    #[test]
    fn echo_noops_without_usage() {
        let mut body = json!({"id": "x"});
        let before = body.clone();
        let st = SearchStatus {
            searched: true,
            status: "ok",
            provider: "searxng".into(),
            results: 1,
        };
        echo_web_search_requests(&mut body, &st);
        assert_eq!(body, before);
    }
}
