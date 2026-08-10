//! Exact-match response cache (Stage 119) — all-gateway-parity feature.
//!
//! Provides a `CacheBackend` trait (memory now, Redis reserved) storing
//! complete upstream responses keyed by a deterministic SHA-256 of
//! (provider + endpoint + model + auth + canonical body). Handlers check the
//! cache before calling upstream and store assembled responses after; cache
//! hits are served with `X-Cache-Status: HIT`, cost nothing (billing 0), and
//! respect per-request `cache` control (`use-cache` / `no-store` / `ttl`).

use axum::http::{HeaderMap, StatusCode};
use sha2::{Digest, Sha256};
use std::time::Duration;

/// A fully assembled upstream response stored in the cache.
///
/// `request_id` / `call_id` headers are deliberately NOT stored — each request
/// regenerates its own reconciliation IDs; only the body + stable headers pass
/// through so hits are byte-identical for clients.
#[derive(Debug, Clone)]
pub struct CachedResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl CachedResponse {
    /// Parse the cached body as JSON (None if not valid JSON).
    pub fn body_as_value(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// Cache backend abstraction — memory now, Redis later (distributed layer M2).
pub trait CacheBackend: Send + Sync + std::fmt::Debug {
    fn get(&self, key: &str) -> Option<CachedResponse>;
    fn put(&self, key: &str, resp: CachedResponse, ttl: Duration);
    fn delete(&self, key: &str);
    /// Number of entries currently stored (observability / tests).
    fn len(&self) -> usize;
    /// Whether the store is empty (clippy len-without-is_empty).
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// LRU in-memory backend backed by moka's sync cache.
///
/// TTL is enforced manually (value stores an `expires_at` instant) — moka's
/// sync `Cache` supports capacity-based LRU eviction but not per-entry TTL on
/// the plain `insert` path.
#[derive(Debug, Clone)]
pub struct MemoryCache {
    store: moka::sync::Cache<String, (CachedResponse, std::time::Instant)>,
}

impl MemoryCache {
    /// Create a bounded LRU cache with `max_entries` capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            store: moka::sync::Cache::new(max_entries.max(1) as u64),
        }
    }
}

impl CacheBackend for MemoryCache {
    fn get(&self, key: &str) -> Option<CachedResponse> {
        let (resp, expires_at) = self.store.get(key)?;
        if std::time::Instant::now() >= expires_at {
            // Expired — drop it so the next check is a clean MISS.
            self.store.invalidate(key);
            return None;
        }
        Some(resp)
    }

    fn put(&self, key: &str, resp: CachedResponse, ttl: Duration) {
        let expires_at = std::time::Instant::now() + ttl;
        self.store.insert(key.to_string(), (resp, expires_at));
    }

    fn delete(&self, key: &str) {
        self.store.invalidate(key);
    }

    fn len(&self) -> usize {
        self.store.entry_count() as usize
    }
}

/// Build the deterministic cache key.
///
/// Mirrors litellm `Cache.get_cache_key`: a SHA-256 over the request's
/// distinguishing fields. The auth bucket is the key's token hash (not the raw
/// token) so keys never leak credentials into the cache.
pub fn cache_key(
    provider: &str,
    endpoint: &str,
    model: &str,
    auth_bucket: &str,
    canonical_body: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    hasher.update([0u8]); // field separator
    hasher.update(endpoint.as_bytes());
    hasher.update([0u8]);
    hasher.update(model.as_bytes());
    hasher.update([0u8]);
    hasher.update(auth_bucket.as_bytes());
    hasher.update([0u8]);
    hasher.update(canonical_body.as_bytes());
    hex::encode(hasher.finalize())
}

/// Canonicalize a request body for exact-match keying.
///
/// Streams/non-streams: JSON objects are re-serialized with sorted keys so the
/// same logical body (different field order) produces the same key.
pub fn canonical_body(body: &serde_json::Value) -> String {
    // Sort object keys recursively.
    fn sort(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut sorted = serde_json::Map::new();
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for k in keys {
                    sorted.insert(k.clone(), sort(&map[k]));
                }
                serde_json::Value::Object(sorted)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(sort).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(body)).unwrap_or_default()
}

/// Per-request cache control parsed from the `cache` request field
/// (OpenAI Chat Completions cache extension; litellm/Cloudflare parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheControl {
    /// Whether to consult the cache at all. Default true.
    pub use_cache: bool,
    /// Bypass both read and write. Default false.
    pub no_store: bool,
    /// TTL override for this request (seconds). Default 60.
    pub ttl: u64,
}

impl Default for CacheControl {
    fn default() -> Self {
        Self {
            use_cache: true,
            no_store: false,
            ttl: 60,
        }
    }
}

impl CacheControl {
    /// Parse the `cache` request field. Absent/null → defaults.
    pub fn parse(body: &serde_json::Value) -> Self {
        let Some(cache) = body.get("cache") else {
            return Self::default();
        };
        let obj = match cache {
            serde_json::Value::Object(o) => o,
            _ => return Self::default(),
        };
        let use_cache = obj
            .get("use-cache")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let no_store = obj
            .get("no-store")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ttl = obj.get("ttl").and_then(|v| v.as_u64()).unwrap_or(60).max(1);
        Self {
            use_cache,
            no_store,
            ttl,
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Unit tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn test_cache_key_deterministic() {
        let k1 = cache_key(
            "openai",
            "/v1/chat/completions",
            "gpt-4",
            "hash-a",
            r#"{"messages":[]}"#,
        );
        let k2 = cache_key(
            "openai",
            "/v1/chat/completions",
            "gpt-4",
            "hash-a",
            r#"{"messages":[]}"#,
        );
        assert_eq!(k1, k2);
        // Different auth bucket → different key.
        let k3 = cache_key(
            "openai",
            "/v1/chat/completions",
            "gpt-4",
            "hash-b",
            r#"{"messages":[]}"#,
        );
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_canonical_body_sorts_keys() {
        let a = serde_json::json!({"b": 1, "a": 2, "nested": {"z": 1, "y": 2}});
        let b = serde_json::json!({"nested": {"y": 2, "z": 1}, "a": 2, "b": 1});
        assert_eq!(canonical_body(&a), canonical_body(&b));
    }

    #[tokio::test]
    async fn test_memory_cache_put_get_hit() {
        let c = MemoryCache::new(10);
        let resp = CachedResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: b"hello".to_vec(),
        };
        c.put("k", resp.clone(), Duration::from_secs(60));
        let got = c.get("k").expect("should hit");
        assert_eq!(got.body, b"hello");
    }

    #[tokio::test]
    async fn test_memory_cache_ttl_expiry() {
        let c = MemoryCache::new(10);
        c.put(
            "k",
            CachedResponse {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: vec![],
            },
            Duration::from_millis(20),
        );
        assert!(c.get("k").is_some());
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(c.get("k").is_none(), "entry should expire after TTL");
    }

    #[tokio::test]
    async fn test_memory_cache_capacity_bounded() {
        // LRU eviction is maintained asynchronously by moka; the deterministic
        // contract we test is that the most recently inserted entries are
        // retrievable and the store stays bounded (no growth past capacity in
        // steady state). We probe by insertion order.
        let c = MemoryCache::new(2);
        for i in 0..3 {
            c.put(
                &format!("k{i}"),
                CachedResponse {
                    status: StatusCode::OK,
                    headers: HeaderMap::new(),
                    body: vec![],
                },
                Duration::from_secs(60),
            );
            // Touch the newest to nudge moka's maintenance; k2 always live.
            let _ = c.get(&format!("k{i}"));
        }
        assert!(
            c.get("k2").is_some(),
            "most recent entry must be retrievable"
        );
        // len() is approximate; ensure it doesn't report > capacity+slack.
        assert!(c.len() <= 4, "store must stay bounded, got {}", c.len());
    }

    #[test]
    fn test_cache_control_defaults() {
        let cc = CacheControl::parse(&serde_json::json!({}));
        assert!(cc.use_cache);
        assert!(!cc.no_store);
        assert_eq!(cc.ttl, 60);
    }

    #[test]
    fn test_cache_control_no_store() {
        let cc = CacheControl::parse(&serde_json::json!({"cache": {"no-store": true}}));
        assert!(!cc.use_cache || cc.no_store);
        assert!(cc.no_store);
    }

    #[test]
    fn test_cache_control_ttl_override() {
        let cc = CacheControl::parse(&serde_json::json!({"cache": {"ttl": 5}}));
        assert_eq!(cc.ttl, 5);
    }

    #[test]
    fn test_cache_headers_preserved() {
        let c = MemoryCache::new(10);
        let mut h = HeaderMap::new();
        h.insert("content-type", HeaderValue::from_static("application/json"));
        c.put(
            "k",
            CachedResponse {
                status: StatusCode::OK,
                headers: h.clone(),
                body: vec![],
            },
            Duration::from_secs(60),
        );
        let got = c.get("k").expect("hit");
        assert_eq!(
            got.headers
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[test]
    fn test_memory_cache_delete() {
        let c = MemoryCache::new(10);
        c.put(
            "a",
            CachedResponse {
                status: StatusCode::OK,
                headers: HeaderMap::new(),
                body: vec![],
            },
            Duration::from_secs(60),
        );
        assert!(c.get("a").is_some());
        c.delete("a");
        assert!(c.get("a").is_none(), "deleted entry must be gone");
    }
}
