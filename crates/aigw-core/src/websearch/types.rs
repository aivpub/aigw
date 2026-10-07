//! Web search request/result types and the provider-agnostic normalization.
//!
//! Shapes follow sub2api's `websearch/types.go` (already production-proven):
//! a flat `SearchResult` with url/title/snippet, plus the two optional fields
//! only some vendors supply (`score`, `published_date`).
//!
//! The normalization pipeline in [`normalize`] is **shared by every provider**
//! and is where the server-side guardrails (max_results clamp, snippet length,
//! domain lists) are actually enforced — callers cannot bypass it because
//! `WebSearchRegistry::search` always runs it on the provider's output.

use serde::{Deserialize, Serialize};

/// Default result count when the caller does not ask for a specific number.
///
/// 5 matches OpenRouter's default, Tavily's wrapper default and sub2api's
/// `defaultMaxResults` — three independent implementations agree.
pub const DEFAULT_MAX_RESULTS: usize = 5;

/// Default per-result snippet cap in characters.
pub const DEFAULT_SNIPPET_MAX_CHARS: usize = 2000;

/// A single search query as seen by a provider.
#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    pub query: String,
    /// `None` → [`DEFAULT_MAX_RESULTS`]. Always clamped to the configured
    /// server-side ceiling before reaching a provider (see [`Guardrails`]).
    pub max_results: Option<usize>,
}

impl SearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            max_results: None,
        }
    }

    /// Effective result count: caller value or [`DEFAULT_MAX_RESULTS`].
    pub fn effective_max_results(&self) -> usize {
        self.max_results.unwrap_or(DEFAULT_MAX_RESULTS)
    }
}

/// One normalized search hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Kept as the vendor's raw string — never parsed here.
    ///
    /// Vendor date formats are three-way split (Tavily RFC2822, Bocha/SearXNG
    /// ISO8601, Serper relative strings like "2 days ago"). Parsing in this
    /// layer buys nothing and adds a panic surface; the display side can parse
    /// with its own fallback.
    pub published_date: Option<String>,
    /// Only Tavily / Valyu / Exa supply a real score; SearXNG supplies a
    /// per-engine `1/position` value. `None` when the vendor has none —
    /// never fabricated.
    pub score: Option<f32>,
}

/// A provider's answer to one query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub query: String,
    /// Provider `name()` that produced this response (billing attribution in
    /// Stage 136, and the failover audit trail today).
    pub provider: String,
    /// Vendor self-reported consumption (Tavily `usage.credits`). `None` for
    /// SearXNG. Carried only — no billing happens in this Stage.
    pub reported_credits: Option<f64>,
    /// `base_url` of the physical instance that actually served this query.
    /// `None` when the provider does not track instances (test stubs).
    /// Stage 136 records it as the search SpendLog row's `api_base`, so
    /// multi-instance deployments can attribute latency and failures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// Server-side guardrails. Clients and models can lower `max_results` but can
/// never raise it, and have no way to reach the other knobs at all.
///
/// Mirrors Vercel's server-tool `config` semantics (developer defaults
/// override model-generated values as a prompt-injection defence).
#[derive(Debug, Clone)]
pub struct Guardrails {
    /// Hard ceiling on result count.
    pub max_results: usize,
    /// Per-result snippet character cap.
    pub snippet_max_chars: usize,
    /// When non-empty, only these domains (and their subdomains) survive.
    pub allowed_domains: Vec<String>,
    /// When non-empty, these domains (and their subdomains) are dropped.
    pub blocked_domains: Vec<String>,
}

impl Default for Guardrails {
    fn default() -> Self {
        Self {
            max_results: DEFAULT_MAX_RESULTS,
            snippet_max_chars: DEFAULT_SNIPPET_MAX_CHARS,
            allowed_domains: Vec::new(),
            blocked_domains: Vec::new(),
        }
    }
}

impl Guardrails {
    /// Clamp a caller-supplied count to the configured ceiling.
    ///
    /// Lowering is honoured; raising is silently capped.
    pub fn clamp_max_results(&self, requested: Option<usize>) -> usize {
        requested
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .min(self.max_results)
            .max(1)
    }
}

/// Normalize a provider's raw results in place.
///
/// Order is load-bearing and must not be reordered:
///
/// 1. drop results without url or title (no url → the model invents links)
/// 2. apply domain allow/block lists
/// 3. dedup by url, preserving first occurrence
/// 4. **re-sort by score, descending, stably**
/// 5. truncate to `max_results`
/// 6. truncate each snippet to `snippet_max_chars`
///
/// Step 4 before step 5 is the whole point: SearXNG returns results
/// interleaved by *per-engine position*, not by global score, so the third
/// engine's best hit (score 1.0) sits at index 9. Truncating without
/// re-sorting throws it away and feeds the model "the head of each engine"
/// instead of the most relevant hits. Tavily/Bocha are already ordered, so the
/// re-sort is a no-op for them — one pipeline serves all providers.
///
/// Empty snippets are **kept** (rule 3 in the stage plan): SearXNG really does
/// return results with `content: ""`, and title+url alone still carry signal.
pub fn normalize(response: &mut SearchResponse, guardrails: &Guardrails, max_results: usize) {
    let effective = max_results.min(guardrails.max_results).max(1);

    response.results.retain(|r| {
        if r.url.trim().is_empty() || r.title.trim().is_empty() {
            return false;
        }
        domain_allowed(&r.url, guardrails)
    });

    let mut seen: Vec<String> = Vec::with_capacity(response.results.len());
    response.results.retain(|r| {
        let key = r.url.clone();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });

    // Stable sort: equal scores keep their original (engine) order, so the
    // re-ordering introduces no randomness. `None` sorts last.
    response.results.sort_by(|a, b| {
        let sa = a.score.unwrap_or(0.0);
        let sb = b.score.unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    response.results.truncate(effective);

    for r in &mut response.results {
        if r.snippet.chars().count() > guardrails.snippet_max_chars {
            r.snippet = r
                .snippet
                .chars()
                .take(guardrails.snippet_max_chars)
                .collect();
        }
    }
}

/// Whether a result url passes the configured domain lists.
///
/// `allowed_domains` and `blocked_domains` are mutually exclusive by config
/// validation, so at most one branch is ever active.
fn domain_allowed(url: &str, guardrails: &Guardrails) -> bool {
    let Some(host) = url_host(url) else {
        // Unparseable host: keep only when no allow-list is in force.
        return guardrails.allowed_domains.is_empty();
    };
    if !guardrails.allowed_domains.is_empty() {
        return guardrails
            .allowed_domains
            .iter()
            .any(|d| host_matches(&host, d));
    }
    !guardrails
        .blocked_domains
        .iter()
        .any(|d| host_matches(&host, d))
}

/// Host of a url, lowercased, without userinfo or port.
///
/// Hand-rolled rather than pulling in the `url` crate: `reqwest` re-exports no
/// parser and the two cases we need (host extraction for domain matching) are a
/// handful of lines with their own tests. Not worth a direct dependency.
fn url_host(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url.trim_start_matches("//"));
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host = authority.split(':').next()?;
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Exact host match or a subdomain of `domain`.
///
/// `reddit.com` matches `reddit.com` and `www.reddit.com`, but not
/// `notreddit.com`.
fn host_matches(host: &str, domain: &str) -> bool {
    let domain = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return false;
    }
    host == domain || host.ends_with(&format!(".{}", domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(url: &str, score: Option<f32>) -> SearchResult {
        SearchResult {
            title: format!("title for {}", url),
            url: url.to_string(),
            snippet: "snippet".to_string(),
            published_date: None,
            score,
        }
    }

    fn response(results: Vec<SearchResult>) -> SearchResponse {
        SearchResponse {
            results,
            query: "q".to_string(),
            provider: "stub".to_string(),
            reported_credits: None,
            endpoint: None,
        }
    }

    #[test]
    fn test_normalize_default_max_results_is_five() {
        let results = (0..10)
            .map(|i| result(&format!("https://e{}.com/a", i), None))
            .collect();
        let mut resp = response(results);
        let g = Guardrails::default();
        normalize(&mut resp, &g, DEFAULT_MAX_RESULTS);
        assert_eq!(resp.results.len(), 5);
    }

    #[test]
    fn test_normalize_dedup_by_url_preserves_order() {
        let mut resp = response(vec![
            result("https://a.com/1", Some(1.0)),
            result("https://b.com/1", Some(1.0)),
            result("https://a.com/1", Some(1.0)),
        ]);
        normalize(&mut resp, &Guardrails::default(), 10);
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].url, "https://a.com/1");
        assert_eq!(resp.results[1].url, "https://b.com/1");
    }

    #[test]
    fn test_normalize_drops_results_without_url() {
        let mut resp = response(vec![result("https://a.com/1", None), result("", None)]);
        normalize(&mut resp, &Guardrails::default(), 10);
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].url, "https://a.com/1");
    }

    #[test]
    fn test_normalize_keeps_results_with_empty_snippet() {
        let mut empty = result("https://a.com/1", Some(1.0));
        empty.snippet = String::new();
        let mut resp = response(vec![empty, result("https://b.com/1", Some(0.5))]);
        normalize(&mut resp, &Guardrails::default(), 10);
        assert_eq!(resp.results.len(), 2, "empty snippet must be tolerated");
        assert_eq!(resp.results[0].snippet, "");
    }

    #[test]
    fn test_normalize_truncates_snippet_to_limit() {
        let mut long = result("https://a.com/1", None);
        long.snippet = "x".repeat(5000);
        let mut resp = response(vec![long]);
        normalize(&mut resp, &Guardrails::default(), 10);
        assert_eq!(resp.results[0].snippet.chars().count(), 2000);
    }

    #[test]
    fn test_normalize_dedup_before_truncate() {
        // 7 entries, 3 duplicate pairs collapsing to 4 distinct urls.
        let mut resp = response(vec![
            result("https://a.com/1", None),
            result("https://a.com/1", None),
            result("https://b.com/1", None),
            result("https://b.com/1", None),
            result("https://c.com/1", None),
            result("https://c.com/1", None),
            result("https://d.com/1", None),
        ]);
        normalize(&mut resp, &Guardrails::default(), 5);
        assert_eq!(
            resp.results.len(),
            4,
            "dedup must run before truncate, otherwise duplicates eat the quota"
        );
    }

    #[test]
    fn test_normalize_sorts_by_score_desc_before_truncate() {
        // Real SearXNG shape: per-engine interleave, so the third engine's
        // best hit (score 1.0) sits at index 9.
        let scores = [
            1.0f32, 1.0, 0.5, 0.5, 0.333, 0.333, 0.333, 0.25, 0.25, 1.0, 0.5, 0.25,
        ];
        let results = scores
            .iter()
            .enumerate()
            .map(|(i, s)| result(&format!("https://e{}.com/a", i), Some(*s)))
            .collect();
        let mut resp = response(results);
        normalize(&mut resp, &Guardrails::default(), 5);

        let got: Vec<f32> = resp.results.iter().map(|r| r.score.unwrap()).collect();
        assert_eq!(got, vec![1.0, 1.0, 1.0, 0.5, 0.5]);
        assert!(
            resp.results.iter().any(|r| r.url == "https://e9.com/a"),
            "index-9 result (third engine's top hit) must survive truncation"
        );
    }

    #[test]
    fn test_normalize_sort_is_stable_for_equal_scores() {
        let mut resp = response(vec![
            result("https://bing.com/a", Some(1.0)),
            result("https://yandex.com/a", Some(1.0)),
            result("https://sogou.com/a", Some(1.0)),
        ]);
        normalize(&mut resp, &Guardrails::default(), 10);
        let urls: Vec<&str> = resp.results.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://bing.com/a",
                "https://yandex.com/a",
                "https://sogou.com/a"
            ]
        );
    }

    #[test]
    fn test_normalize_sort_treats_none_score_as_lowest() {
        let mut resp = response(vec![
            result("https://none1.com/a", None),
            result("https://has.com/a", Some(0.5)),
            result("https://none2.com/a", None),
        ]);
        normalize(&mut resp, &Guardrails::default(), 10);
        assert_eq!(resp.results[0].url, "https://has.com/a");
        assert!(resp.results[1].score.is_none());
        assert!(resp.results[2].score.is_none());
    }

    #[test]
    fn test_guardrail_clamps_caller_max_results_to_config() {
        let g = Guardrails {
            max_results: 5,
            ..Default::default()
        };
        assert_eq!(g.clamp_max_results(Some(50)), 5);
    }

    #[test]
    fn test_guardrail_allows_caller_to_lower_max_results() {
        let g = Guardrails {
            max_results: 5,
            ..Default::default()
        };
        assert_eq!(g.clamp_max_results(Some(3)), 3);
    }

    #[test]
    fn test_guardrail_blocked_domain_filtered_post_hoc() {
        let g = Guardrails {
            blocked_domains: vec!["reddit.com".to_string()],
            ..Default::default()
        };
        let mut resp = response(vec![
            result("https://www.reddit.com/r/rust", None),
            result("https://reddit.com/top", None),
            result("https://notreddit.com/x", None),
            result("https://example.com/x", None),
        ]);
        normalize(&mut resp, &g, 10);
        let urls: Vec<&str> = resp.results.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://notreddit.com/x", "https://example.com/x"],
            "subdomains blocked, lookalike domain kept"
        );
    }

    #[test]
    fn test_guardrail_allowed_domain_whitelist_only() {
        let g = Guardrails {
            allowed_domains: vec!["example.com".to_string()],
            ..Default::default()
        };
        let mut resp = response(vec![
            result("https://example.com/a", None),
            result("https://docs.example.com/b", None),
            result("https://evil.com/c", None),
        ]);
        normalize(&mut resp, &g, 10);
        let urls: Vec<&str> = resp.results.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["https://example.com/a", "https://docs.example.com/b"]
        );
    }

    #[test]
    fn test_url_host_strips_port_and_userinfo() {
        assert_eq!(
            url_host("https://a:b@Example.COM:8443/x"),
            Some("example.com".into())
        );
        assert_eq!(
            url_host("http://10.0.0.1:9099/search"),
            Some("10.0.0.1".into())
        );
        assert_eq!(url_host("not a url"), Some("not a url".into()));
        assert_eq!(url_host(""), None);
    }
}
