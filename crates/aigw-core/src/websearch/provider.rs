//! The `SearchProvider` trait and its error type.
//!
//! Deliberately a new trait rather than a reuse of `MessageAdapter`
//! (`adapter.rs:34-46`): none of that trait's four methods apply to a search
//! vendor — search takes no OpenAI chat body and returns no token stream. The
//! legacy `ProviderRegistry` in `provider.rs:49` is likewise unusable (it is a
//! pre-DB stub that still reads `OPENAI_API_KEY` from the environment and sits
//! on no live request path).

use crate::websearch::types::{SearchRequest, SearchResponse};

/// One search backend behind one physical endpoint selection.
///
/// **Must stay object-safe** — [`crate::websearch::WebSearchRegistry`] holds
/// heterogeneous providers as `Arc<dyn SearchProvider>`. That means: no generic
/// methods, no `Self: Sized` bounds, no associated constants.
///
/// Implementations are expected to split into three pure functions
/// (`build_request` / `parse_response` / `map_error`) plus a thin IO shell, so
/// the interesting logic is unit-testable without a network.
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    /// Stable identifier, e.g. `"searxng"`. Flows into logs and (Stage 136)
    /// billing attribution.
    fn name(&self) -> &str;

    /// Price of one query in USD. Carried only in this Stage — Stage 136 is the
    /// first consumer.
    fn cost_per_query(&self) -> f64;

    async fn search(&self, req: &SearchRequest) -> Result<SearchResponse, SearchError>;
}

/// Why a search attempt failed.
///
/// The variants are split along the one axis that matters for failover:
/// `Transport` / `Timeout` / `Http{5xx}` mean "this endpoint is unhealthy, try
/// another", whereas `Http{4xx}` means "retrying elsewhere cannot help and
/// would only inflate the bill". See [`SearchError::is_retriable`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum SearchError {
    // Field is `detail`, not `source`: thiserror treats a `source` field as the
    // error-chain source and then requires it to implement `Error`.
    #[error("{provider}: transport: {detail}")]
    Transport { provider: String, detail: String },

    #[error("{provider}: HTTP {status}: {body}")]
    Http {
        provider: String,
        status: u16,
        body: String,
    },

    #[error("{provider}: parse: {detail}")]
    Parse { provider: String, detail: String },

    #[error("{provider}: timeout after {ms}ms")]
    Timeout { provider: String, ms: u64 },

    #[error("web search is not configured")]
    NotConfigured,

    #[error("web search query is empty")]
    EmptyQuery,
}

impl SearchError {
    /// Whether this failure should move on to another instance / provider.
    ///
    /// Mirrors `Router::is_cooldown_status` (`router.rs:451`): a business 4xx
    /// keeps the endpoint in the pool and fails the request outright, because
    /// quota exhaustion or a bad key is not fixed by switching machines.
    pub fn is_retriable(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::Timeout { .. } => true,
            Self::Http { status, .. } => *status >= 500,
            // A parse failure means the endpoint answered with something that
            // is not a search response (e.g. SearXNG serving HTML because
            // `format=json` is off) — another instance may well be configured
            // correctly.
            Self::Parse { .. } => true,
            Self::NotConfigured | Self::EmptyQuery => false,
        }
    }

    /// Provider name carried by this error, when it has one.
    pub fn provider(&self) -> Option<&str> {
        match self {
            Self::Transport { provider, .. }
            | Self::Http { provider, .. }
            | Self::Parse { provider, .. }
            | Self::Timeout { provider, .. } => Some(provider),
            Self::NotConfigured | Self::EmptyQuery => None,
        }
    }

    /// HTTP status carried by this error, when it has one. Feeds the
    /// instance-level cooldown accounting.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_error_5xx_is_retriable_4xx_is_not() {
        let e5 = SearchError::Http {
            provider: "p".into(),
            status: 503,
            body: String::new(),
        };
        let e4 = SearchError::Http {
            provider: "p".into(),
            status: 401,
            body: String::new(),
        };
        assert!(e5.is_retriable());
        assert!(!e4.is_retriable(), "4xx must not trigger failover");
        assert_eq!(e4.status(), Some(401));
    }

    #[test]
    fn test_search_error_transport_and_timeout_are_retriable() {
        assert!(SearchError::Transport {
            provider: "p".into(),
            detail: "dns".into(),
        }
        .is_retriable());
        assert!(SearchError::Timeout {
            provider: "p".into(),
            ms: 5000,
        }
        .is_retriable());
        assert!(!SearchError::NotConfigured.is_retriable());
        assert!(!SearchError::EmptyQuery.is_retriable());
    }
}
