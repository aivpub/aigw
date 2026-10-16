//! Search-row projection for the spend-log detail view (Stage 139).
//!
//! A `call_type="search"` row stores the provider's results verbatim in
//! `spend_logs.response` (`{result_count, results:[{title,url,snippet,score,
//! published_date}]}`). The generic response renderers do not know that shape —
//! `OutputCard::parseOutput` recognises `choices[]` / `output[]` / `data[]` and
//! falls through to a blank result — so the results were stored and never seen.
//!
//! This module turns one `SpendLog` into the shape the UI actually renders.
//! It is a pure function over the row: projection limits (result cap, snippet
//! truncation, URL scheme allow-list) live here once rather than in each client.

use aigw_core::models::SpendLog;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Cap on how many results are projected. A single search returns at most
/// `max_results` (default 5), so this only guards against a misconfigured or
/// future provider returning a large set.
pub const SEARCH_RESULTS_INLINE_MAX: usize = 20;

/// Cap on snippet length. SearXNG does not bound `content`; the raw row keeps
/// the full text, the projection truncates for display.
pub const SEARCH_SNIPPET_MAX_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub title: String,
    /// `None` when the URL is absent or uses a scheme we will not turn into a
    /// link (`javascript:`, `data:`, …). The title still renders.
    pub url: Option<String>,
    /// Host extracted from `url`, for a compact second line.
    pub domain: Option<String>,
    pub snippet: Option<String>,
    pub score: Option<f64>,
    pub published_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchView {
    /// Logical backend name (`metadata.search_provider`), falling back to
    /// `custom_llm_provider` — the two agree today, but the metadata value is
    /// the one the search layer wrote.
    pub provider: Option<String>,
    /// The instance that actually answered (`api_base`).
    pub instance: Option<String>,
    pub query: Option<String>,
    pub query_count: Option<u64>,
    /// What the provider reported.
    pub result_count: Option<u64>,
    /// How many results the log actually kept (`min(results.len(), cap)`).
    /// Equal to `result_count` in the normal case; the UI shows both only when
    /// they disagree.
    pub kept_count: usize,
    pub cost_per_query: Option<f64>,
    pub parent_call_id: Option<String>,
    pub results: Vec<SearchResultItem>,
}

/// Project a search row into the shape the UI renders, or `None` for any other
/// call type.
///
/// Every field is optional-on-input and optional-on-output: a row whose metadata
/// never made it to the DB still renders its results, and vice versa.
pub fn project_search_view(log: &SpendLog) -> Option<SearchView> {
    if log.call_type != "search" {
        return None;
    }

    let meta = log.metadata.as_ref().and_then(Value::as_object);
    let str_of = |key: &str| -> Option<String> {
        meta.and_then(|m| m.get(key))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let response = log.response.as_ref().and_then(Value::as_object);

    let results: Vec<SearchResultItem> = response
        .and_then(|r| r.get("results"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .take(SEARCH_RESULTS_INLINE_MAX)
                .filter_map(project_result)
                .collect()
        })
        .unwrap_or_default();

    let result_count = response
        .and_then(|r| r.get("result_count"))
        .and_then(Value::as_u64);

    Some(SearchView {
        provider: str_of("search_provider").or_else(|| log.custom_llm_provider.clone()),
        instance: log.api_base.clone().filter(|s| !s.is_empty()),
        query: log
            .messages
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|m| m.get("query"))
            .and_then(Value::as_str)
            .map(str::to_string),
        query_count: meta
            .and_then(|m| m.get("search_query_count"))
            .and_then(Value::as_u64),
        result_count,
        kept_count: results.len(),
        cost_per_query: meta
            .and_then(|m| m.get("cost_per_query"))
            .and_then(Value::as_f64),
        parent_call_id: str_of("parent_call_id"),
        results,
    })
}

/// Just the provider-reported result count, for the **list** DTO.
///
/// The list only needs this one integer; projecting the whole view there would
/// ship every result's title/snippet to the browser for rows the user has not
/// opened. Returns `None` for non-search rows.
pub fn result_count_of(log: &SpendLog) -> Option<u64> {
    if log.call_type != "search" {
        return None;
    }
    log.response
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|r| r.get("result_count"))
        .and_then(Value::as_u64)
}

/// One result entry. Requires a title; everything else degrades.
///
/// A result with a non-http(s) URL keeps its title/snippet but loses the link —
/// dropping the whole entry would hide a result just because its URL is odd.
fn project_result(v: &Value) -> Option<SearchResultItem> {
    let obj = v.as_object()?;
    let title = obj
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();

    let raw_url = obj
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let url = raw_url.filter(|u| is_http_url(u)).map(str::to_string);
    let domain = url.as_deref().and_then(host_of);

    let snippet = obj
        .get("snippet")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| truncate_chars(s, SEARCH_SNIPPET_MAX_CHARS));

    Some(SearchResultItem {
        title,
        url,
        domain,
        snippet,
        score: obj.get("score").and_then(Value::as_f64),
        published_date: obj
            .get("published_date")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    })
}

/// Only `http` / `https` may become a clickable link.
fn is_http_url(u: &str) -> bool {
    let lower = u.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// `https://a.b/c?d` → `a.b`. Returns `None` when there is no host.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    let host = host.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Truncate on a character boundary (never splits a multi-byte char).
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn search_log(metadata: Value, messages: Value, response: Value) -> SpendLog {
        let now = chrono::Utc::now();
        SpendLog {
            call_id: "01a133ec-search".to_string(),
            request_id: None,
            call_type: "search".to_string(),
            api_key: "master_key".to_string(),
            spend: 0.01,
            total_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            start_time: now,
            end_time: now,
            request_duration_ms: Some(2430),
            completion_start_time: None,
            model: "SearXNG/search".to_string(),
            model_id: None,
            model_group: None,
            custom_llm_provider: Some("SearXNG".to_string()),
            api_base: Some("http://30.184.60.216:9099".to_string()),
            user: None,
            metadata: Some(metadata),
            cache_hit: None,
            cache_key: None,
            request_tags: None,
            team_id: None,
            organization_id: None,
            end_user: None,
            requester_ip_address: None,
            messages: Some(messages),
            response: Some(response),
            session_id: None,
            status: Some("success".to_string()),
            mcp_namespaced_tool_name: None,
            agent_id: None,
            proxy_server_request: None,
            body_archived: false,
            parquet_path: None,
            image_tokens: None,
        }
    }

    fn full_metadata() -> Value {
        json!({
            "cost_per_query": 0.01,
            "parent_call_id": "01a133ec-parent",
            "search_provider": "SearXNG",
            "search_query_count": 1,
        })
    }

    fn one_result_response() -> Value {
        json!({
            "result_count": 1,
            "results": [{
                "title": "Discord.js deleteCommand",
                "url": "https://stackoverflow.com/q/42708210",
                "snippet": "some snippet",
                "score": 2.2,
                "published_date": null,
            }],
        })
    }

    #[test]
    fn project_extracts_all_fields_from_a_search_row() {
        let log = search_log(
            full_metadata(),
            json!({"query": "say PONG"}),
            one_result_response(),
        );
        let v = project_search_view(&log).expect("search row must project");

        assert_eq!(v.provider.as_deref(), Some("SearXNG"));
        assert_eq!(v.instance.as_deref(), Some("http://30.184.60.216:9099"));
        assert_eq!(v.query.as_deref(), Some("say PONG"));
        assert_eq!(v.query_count, Some(1));
        assert_eq!(v.result_count, Some(1));
        assert_eq!(v.kept_count, 1);
        assert_eq!(v.cost_per_query, Some(0.01));
        assert_eq!(v.parent_call_id.as_deref(), Some("01a133ec-parent"));
        assert_eq!(v.results.len(), 1);
        assert_eq!(v.results[0].domain.as_deref(), Some("stackoverflow.com"));
        assert_eq!(v.results[0].score, Some(2.2));
    }

    #[test]
    fn project_returns_none_for_non_search_rows() {
        let mut log = search_log(full_metadata(), json!({}), one_result_response());
        log.call_type = "completion".to_string();
        assert!(project_search_view(&log).is_none());
    }

    /// A row whose metadata never made it to the DB still renders its results.
    #[test]
    fn project_tolerates_empty_metadata() {
        let log = search_log(json!({}), json!({}), one_result_response());
        let v = project_search_view(&log).expect("must still project");
        assert_eq!(
            v.provider.as_deref(),
            Some("SearXNG"),
            "falls back to provider column"
        );
        assert!(v.query_count.is_none());
        assert!(v.cost_per_query.is_none());
        assert!(v.parent_call_id.is_none());
        assert_eq!(v.results.len(), 1, "results survive missing metadata");
    }

    #[test]
    fn project_caps_results_and_snippet() {
        let long = "x".repeat(600);
        let results: Vec<Value> = (0..50)
            .map(|i| {
                json!({
                    "title": format!("r{i}"),
                    "url": format!("https://example.com/{i}"),
                    "snippet": long,
                })
            })
            .collect();
        let log = search_log(
            full_metadata(),
            json!({"query": "q"}),
            json!({"result_count": 50, "results": results}),
        );
        let v = project_search_view(&log).unwrap();

        assert_eq!(v.results.len(), SEARCH_RESULTS_INLINE_MAX);
        let snippet = v.results[0].snippet.as_deref().unwrap();
        assert_eq!(
            snippet.chars().count(),
            SEARCH_SNIPPET_MAX_CHARS + 1,
            "cap + ellipsis"
        );
        assert!(snippet.ends_with('…'));
    }

    /// Only http(s) becomes a link; the result is kept either way.
    #[test]
    fn project_drops_non_http_urls() {
        let log = search_log(
            full_metadata(),
            json!({"query": "q"}),
            json!({"result_count": 2, "results": [
                {"title": "bad", "url": "javascript:alert(1)", "snippet": "s"},
                {"title": "ok", "url": "https://ok.example/x", "snippet": "s"},
            ]}),
        );
        let v = project_search_view(&log).unwrap();

        assert_eq!(
            v.results.len(),
            2,
            "the entry is kept, only its link is dropped"
        );
        assert!(
            v.results[0].url.is_none(),
            "javascript: must not become a link"
        );
        assert!(v.results[0].domain.is_none());
        assert_eq!(v.results[0].title, "bad", "the title still renders");
        assert_eq!(v.results[1].url.as_deref(), Some("https://ok.example/x"));
    }

    /// The provider's count and what the log kept can differ; both are reported.
    #[test]
    fn project_reports_both_counts_when_they_disagree() {
        let log = search_log(
            full_metadata(),
            json!({"query": "q"}),
            json!({"result_count": 5, "results": [
                {"title": "a", "url": "https://a.example/", "snippet": "s"},
                {"title": "b", "url": "https://b.example/", "snippet": "s"},
            ]}),
        );
        let v = project_search_view(&log).unwrap();
        assert_eq!(v.result_count, Some(5));
        assert_eq!(v.kept_count, 2);
    }

    /// A result missing its title is skipped; the others still render.
    /// (`snippet` is empty for ~2/35 of real SearXNG results — see Stage 134.)
    #[test]
    fn project_skips_results_without_a_title() {
        let log = search_log(
            full_metadata(),
            json!({"query": "q"}),
            json!({"result_count": 3, "results": [
                {"url": "https://no-title.example/", "snippet": "s"},
                {"title": "kept", "url": "https://kept.example/", "snippet": ""},
                {"title": "also kept", "url": "https://k2.example/"},
            ]}),
        );
        let v = project_search_view(&log).unwrap();
        assert_eq!(v.kept_count, 2);
        assert_eq!(v.results[0].title, "kept");
        assert!(
            v.results[0].snippet.is_none(),
            "empty snippet is not a snippet"
        );
        assert!(v.results[1].snippet.is_none());
    }

    /// The list DTO needs only the integer, and only for search rows.
    #[test]
    fn result_count_of_is_search_only() {
        let log = search_log(full_metadata(), json!({}), one_result_response());
        assert_eq!(result_count_of(&log), Some(1));

        let mut other = log.clone();
        other.call_type = "completion".to_string();
        assert_eq!(
            result_count_of(&other),
            None,
            "non-search rows carry no count"
        );

        let mut no_response = log;
        no_response.response = None;
        assert_eq!(result_count_of(&no_response), None);
    }

    /// A malformed `response` must not panic and must not hide the rest.
    #[test]
    fn project_survives_malformed_response() {
        for bad in [json!("a string"), json!([1, 2, 3]), json!(42)] {
            let log = search_log(full_metadata(), json!({"query": "q"}), bad);
            let v = project_search_view(&log).expect("row is still a search row");
            assert!(v.results.is_empty());
            assert!(v.result_count.is_none());
            assert_eq!(
                v.provider.as_deref(),
                Some("SearXNG"),
                "other fields survive"
            );
        }
    }
}
