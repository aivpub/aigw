//! SearXNG provider — the only vendor implemented in this Stage.
//!
//! Chosen because a self-hosted instance is already available (zero signup, zero
//! purchase cost). Everything below was verified against a real instance on
//! 2026-10-06 and the captured response is the UT fixture
//! (`docs/fixtures/searxng-search-response-2026-10-06.json`).
//!
//! Five behaviours that documentation alone would not reveal:
//!
//! 1. `results[]` is interleaved by **per-engine position**, not global score —
//!    handled by the shared re-sort in [`crate::websearch::types::normalize`].
//! 2. `&results=` / `&limit=` are **ignored**, so the count cap is gateway-side
//!    only; we do not send those params at all.
//! 3. A nonsense query still returns ~35 unrelated hits — there is no "no
//!    results" signal to detect.
//! 4. A bad/missing `format` yields **HTTP 200 with an HTML body**, not a 4xx —
//!    [`parse_response`] must say so legibly instead of leaking a bare serde error.
//! 5. Latency is 2.0–3.1s because it waits for every engine (bing + yandex +
//!    sogou on the measured instance), hence the per-provider timeout override.
//!
//! Structure is three pure functions plus one thin IO shell — the template every
//! future provider follows.

use crate::websearch::instance::InstancePool;
use crate::websearch::provider::{SearchError, SearchProvider};
use crate::websearch::types::{SearchRequest, SearchResponse, SearchResult};

pub const KIND: &str = "searxng";

/// One logical SearXNG backend over N physical instances.
pub struct SearxngProvider {
    name: String,
    cost_per_query: f64,
    pool: InstancePool,
    client: reqwest_middleware::ClientWithMiddleware,
    timeout_ms: u64,
}

impl std::fmt::Debug for SearxngProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearxngProvider")
            .field("name", &self.name)
            .field("cost_per_query", &self.cost_per_query)
            .field("instances", &self.pool.len())
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

impl SearxngProvider {
    pub fn new(
        name: impl Into<String>,
        cost_per_query: f64,
        pool: InstancePool,
        client: reqwest_middleware::ClientWithMiddleware,
        timeout_ms: u64,
    ) -> Self {
        Self {
            name: name.into(),
            cost_per_query,
            pool,
            client,
            timeout_ms,
        }
    }

    /// Request URL for one query against one instance.
    ///
    /// Pure. Deliberately omits `results` / `limit` (ignored by SearXNG) so the
    /// wire stays free of params that do nothing.
    pub fn build_url(base_url: &str, query: &str) -> String {
        format!(
            "{}/search?q={}&format=json",
            base_url.trim_end_matches('/'),
            urlencode(query)
        )
    }
}

/// Minimal percent-encoding for a query-string value.
///
/// Hand-rolled rather than adding a dependency for one query-string value —
/// `reqwest` exposes no standalone encoder, and this has its own test.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Parse a SearXNG `format=json` body. Pure — the main UT surface.
pub fn parse_response(
    provider: &str,
    query: &str,
    bytes: &[u8],
) -> Result<SearchResponse, SearchError> {
    let parsed: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| SearchError::Parse {
            provider: provider.to_string(),
            detail: format!(
                "response is not JSON ({e}); SearXNG answers HTTP 200 with HTML when \
             `format=json` is absent or unsupported — check that base_url points at \
             a SearXNG instance and that `search.formats` in its settings.yml includes `json`"
            ),
        })?;

    let items = parsed
        .get("results")
        .and_then(|v| v.as_array())
        .ok_or_else(|| SearchError::Parse {
            provider: provider.to_string(),
            detail: "missing `results` array in SearXNG response".to_string(),
        })?;

    let results = items
        .iter()
        .map(|item| SearchResult {
            title: str_field(item, "title"),
            url: str_field(item, "url"),
            // Snippet lives in `content`, NOT `snippet`. Empty for ~2 of 35 real
            // results, which must be tolerated rather than unwrapped.
            snippet: str_field(item, "content"),
            // Present but null for ~80% of real results.
            published_date: item
                .get("publishedDate")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string),
            // Always present on the measured instance, but it is a per-engine
            // `1/position` value, not a cross-engine relevance score.
            score: item.get("score").and_then(|v| v.as_f64()).map(|f| f as f32),
        })
        .collect();

    if let Some(unresponsive) = parsed
        .get("unresponsive_engines")
        .and_then(|v| v.as_array())
    {
        if !unresponsive.is_empty() {
            // Engines rot as upstreams change their anti-scraping; surfacing this
            // makes gradual degradation observable before results go empty.
            tracing::debug!(
                provider = provider,
                unresponsive = %serde_json::Value::Array(unresponsive.clone()),
                "SearXNG reported unresponsive engines"
            );
        }
    }

    Ok(SearchResponse {
        results,
        query: parsed
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or(query)
            .to_string(),
        provider: provider.to_string(),
        reported_credits: None,
    })
}

/// Map a non-2xx response to a [`SearchError`]. Pure.
pub fn map_error(provider: &str, status: u16, bytes: &[u8]) -> SearchError {
    let body = String::from_utf8_lossy(bytes)
        .chars()
        .take(500)
        .collect::<String>();
    let detail = match status {
        403 => format!(
            "403 from SearXNG — the instance most likely has JSON output disabled; \
             add `json` to `search.formats` in its settings.yml. body: {body}"
        ),
        429 => format!(
            "429 from SearXNG — the instance limiter is rate-limiting this IP. body: {body}"
        ),
        400 => {
            format!("400 from SearXNG — malformed query (an empty `q` returns 400). body: {body}")
        }
        _ => body,
    };
    SearchError::Http {
        provider: provider.to_string(),
        status,
        body: detail,
    }
}

fn str_field(item: &serde_json::Value, key: &str) -> String {
    item.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

#[async_trait::async_trait]
impl SearchProvider for SearxngProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn cost_per_query(&self) -> f64 {
        self.cost_per_query
    }

    /// Thin IO shell: walk healthy instances until one answers, then hand the
    /// bytes to the pure parser.
    async fn search(&self, req: &SearchRequest) -> Result<SearchResponse, SearchError> {
        if req.query.trim().is_empty() {
            // SearXNG answers 400 for an empty `q`; refuse before the round-trip.
            return Err(SearchError::EmptyQuery);
        }

        let mut tried: Vec<usize> = Vec::new();
        let mut last_err: Option<SearchError> = None;

        while let Some(idx) = self.pool.pick_excluding(&tried) {
            tried.push(idx);
            let Some(instance) = self.pool.instance(idx) else {
                break;
            };
            let url = Self::build_url(&instance.base_url, &req.query);

            let mut request = self.client.get(&url);
            if let Some(key) = instance.api_key.as_deref().filter(|k| !k.is_empty()) {
                // SearXNG itself needs no auth, but a reverse proxy in front of
                // it may; honour the configured key when present.
                request = request.bearer_auth(key);
            }

            match request.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let bytes = resp.bytes().await.unwrap_or_default();
                    if !(200..300).contains(&status) {
                        let err = map_error(&self.name, status, &bytes);
                        self.pool.report_failure(idx, Some(status));
                        if !err.is_retriable() {
                            return Err(err);
                        }
                        last_err = Some(err);
                        continue;
                    }
                    match parse_response(&self.name, &req.query, &bytes) {
                        Ok(resp) => {
                            self.pool.report_success(idx);
                            return Ok(resp);
                        }
                        Err(err) => {
                            // A 200 carrying HTML means this instance is
                            // misconfigured; another one may be fine.
                            self.pool.report_failure(idx, None);
                            last_err = Some(err);
                        }
                    }
                }
                Err(e) => {
                    let err = if is_timeout(&e) {
                        SearchError::Timeout {
                            provider: self.name.clone(),
                            ms: self.timeout_ms,
                        }
                    } else {
                        SearchError::Transport {
                            provider: self.name.clone(),
                            detail: e.to_string(),
                        }
                    };
                    self.pool.report_failure(idx, None);
                    last_err = Some(err);
                }
            }
        }

        Err(last_err.unwrap_or(SearchError::NotConfigured))
    }
}

fn is_timeout(e: &reqwest_middleware::Error) -> bool {
    match e {
        reqwest_middleware::Error::Reqwest(r) => r.is_timeout(),
        reqwest_middleware::Error::Middleware(_) => {
            // reqwest-retry wraps the final transport failure; the message is
            // the only signal available through the opaque middleware error.
            e.to_string().to_lowercase().contains("timed out")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real capture from `http://30.184.60.216:9099` on 2026-10-06 — 35 results
    /// across bing/yandex/sogou, with the interleaved score sequence, 2 empty
    /// `content` values and 28 null `publishedDate` values that the UTs below
    /// depend on. Hand-written data cannot reproduce that shape.
    const SEARXNG_FIXTURE: &str =
        include_str!("../../../../docs/fixtures/searxng-search-response-2026-10-06.json");

    #[test]
    fn test_searxng_parse_response_snippet_from_content() {
        let resp = parse_response(
            "searxng",
            "Anthropic latest model",
            SEARXNG_FIXTURE.as_bytes(),
        )
        .expect("fixture must parse");
        assert_eq!(resp.results.len(), 35);
        assert_eq!(resp.provider, "searxng");
        assert_eq!(resp.query, "Anthropic latest model");
        assert!(resp.reported_credits.is_none());

        let first = &resp.results[0];
        assert_eq!(first.url, "https://www.anthropic.com/");
        assert!(
            first.snippet.contains("Opus 5.5"),
            "snippet must come from `content`, got {:?}",
            first.snippet
        );
    }

    #[test]
    fn test_searxng_parse_response_missing_optional_fields() {
        let body =
            br#"{"query":"q","results":[{"title":"t","url":"https://e.com/","content":"c"}]}"#;
        let resp = parse_response("searxng", "q", body).expect("minimal item must parse");
        assert_eq!(resp.results.len(), 1);
        assert!(resp.results[0].score.is_none());
        assert!(resp.results[0].published_date.is_none());
    }

    #[test]
    fn test_searxng_parse_response_tolerates_empty_content() {
        let resp = parse_response("searxng", "q", SEARXNG_FIXTURE.as_bytes()).unwrap();
        let empty = resp.results.iter().filter(|r| r.snippet.is_empty()).count();
        assert_eq!(
            empty, 2,
            "the two real empty-content results must be kept, not dropped"
        );
    }

    #[test]
    fn test_searxng_parse_response_null_published_date() {
        let resp = parse_response("searxng", "q", SEARXNG_FIXTURE.as_bytes()).unwrap();
        let nulls = resp
            .results
            .iter()
            .filter(|r| r.published_date.is_none())
            .count();
        assert_eq!(nulls, 28, "80% of real results have publishedDate: null");
        assert!(resp.results.iter().any(|r| r.published_date.is_some()));
    }

    #[test]
    fn test_searxng_parse_response_scores_are_engine_interleaved() {
        // Locks the shape the normalization re-sort exists for: index 9 carries
        // 1.0 (third engine's top hit) even though index 8 carries 0.25.
        let resp = parse_response("searxng", "q", SEARXNG_FIXTURE.as_bytes()).unwrap();
        let scores: Vec<f32> = resp.results.iter().map(|r| r.score.unwrap()).collect();
        assert_eq!(scores.len(), 35, "score is present on every real result");
        assert_eq!(scores[8], 0.25);
        assert_eq!(scores[9], 1.0, "results[] is NOT globally score-sorted");
    }

    #[test]
    fn test_searxng_map_error_403_hints_json_format() {
        let err = map_error("searxng", 403, b"Forbidden");
        let msg = err.to_string();
        assert!(msg.contains("settings.yml"), "{msg}");
        assert!(msg.contains("json"), "{msg}");
        assert_eq!(err.status(), Some(403));
        assert!(!err.is_retriable(), "403 is a 4xx — no failover");
    }

    #[test]
    fn test_searxng_map_error_429_and_400_are_explained() {
        assert!(map_error("searxng", 429, b"")
            .to_string()
            .contains("limiter"));
        assert!(map_error("searxng", 400, b"").to_string().contains("empty"));
        assert!(
            map_error("searxng", 502, b"bad gateway").is_retriable(),
            "5xx must trigger failover"
        );
    }

    #[test]
    fn test_searxng_parse_response_rejects_html_body() {
        let html = b"<!DOCTYPE html><html><body>searxng</body></html>";
        let err = parse_response("searxng", "q", html).expect_err("HTML must not parse");
        let msg = err.to_string();
        assert!(matches!(err, SearchError::Parse { .. }));
        assert!(msg.contains("base_url"), "needs a triage hint: {msg}");
        assert!(
            msg.contains("format=json") || msg.contains("`json`"),
            "{msg}"
        );
    }

    #[test]
    fn test_searxng_parse_response_rejects_json_without_results() {
        let err = parse_response("searxng", "q", br#"{"query":"q"}"#)
            .expect_err("missing results array must fail");
        assert!(err.to_string().contains("results"), "{err}");
    }

    #[test]
    fn test_searxng_build_request_format_json() {
        let url = SearxngProvider::build_url("http://searxng.internal:8080/", "量子计算 latest");
        assert!(url.starts_with("http://searxng.internal:8080/search?q="));
        assert!(url.contains("format=json"));
        assert!(
            !url.contains("results=") && !url.contains("limit="),
            "SearXNG ignores those params; sending them is noise: {url}"
        );
        assert!(
            url.contains("%E9%87%8F"),
            "query must be percent-encoded: {url}"
        );
        assert!(url.contains('+'), "spaces encode as +: {url}");
    }
}
