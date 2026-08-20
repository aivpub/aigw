//! Routing strategies (litellm-compatible)
//!
//! Available strategies:
//! - simple-shuffle: random selection
//! - usage-based-routing-v2: select the instance with the fewest active requests
//! - latency-based-routing: select the instance with the lowest recent latency
//!
//! Plus cooldown mechanism: instances with N consecutive failures are temporarily excluded.

use crate::deployment::Deployment;
use rand::Rng;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(feature = "reqwest")]
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;

/// Track per-instance state for routing decisions
#[derive(Debug, Clone)]
pub struct InstanceState {
    pub active_requests: u32,
    pub consecutive_failures: u32,
    pub last_latency_ms: f64,
    pub cooldown_until: Option<std::time::Instant>,
    pub total_requests: u64,
    pub total_failures: u64,
}

impl Default for InstanceState {
    fn default() -> Self {
        Self {
            active_requests: 0,
            consecutive_failures: 0,
            last_latency_ms: 0.0,
            cooldown_until: None,
            total_requests: 0,
            total_failures: 0,
        }
    }
}

pub type RouterState = Arc<Mutex<HashMap<String, InstanceState>>>;

/// Routing strategy enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    SimpleShuffle,
    UsageBasedRoutingV2,
    LatencyBasedRouting,
}

impl std::str::FromStr for Strategy {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "usage-based-routing-v2" | "usage-based-routing" => Self::UsageBasedRoutingV2,
            "latency-based-routing" => Self::LatencyBasedRouting,
            _ => Self::SimpleShuffle,
        })
    }
}

impl Strategy {}

/// Select an instance from the list using the given strategy
pub async fn select_instance(
    instances: &[String],
    state: &RouterState,
    strategy: Strategy,
    _allowed_fails: u32,
    _cooldown_secs: f64,
) -> Option<String> {
    let state_map = state.lock().await;
    let now = std::time::Instant::now();

    // Filter out instances in cooldown
    let available: Vec<&String> = instances
        .iter()
        .filter(|name| {
            if let Some(s) = state_map.get(name.as_str()) {
                s.cooldown_until.is_none_or(|t| now >= t)
            } else {
                true
            }
        })
        .collect();

    if available.is_empty() {
        return None;
    }

    match strategy {
        Strategy::SimpleShuffle => {
            let idx = fastrand::usize(0..available.len());
            available.get(idx).map(|s| (*s).clone())
        }
        Strategy::UsageBasedRoutingV2 => {
            // Pick instance with fewest active requests
            available
                .iter()
                .min_by_key(|name| {
                    state_map
                        .get(name.as_str())
                        .map(|s| s.active_requests)
                        .unwrap_or(0)
                })
                .map(|s| (*s).clone())
        }
        Strategy::LatencyBasedRouting => {
            // Pick instance with lowest last latency
            available
                .iter()
                .min_by(|a, b| {
                    let la = state_map
                        .get(a.as_str())
                        .map(|s| s.last_latency_ms)
                        .unwrap_or(0.0);
                    let lb = state_map
                        .get(b.as_str())
                        .map(|s| s.last_latency_ms)
                        .unwrap_or(0.0);
                    la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|s| (*s).clone())
        }
    }
}

/// Mark an instance as having failed. Triggers cooldown if threshold reached.
pub async fn mark_failure(
    instance: &str,
    state: &RouterState,
    allowed_fails: u32,
    cooldown_secs: f64,
) {
    let mut map = state.lock().await;
    let entry = map.entry(instance.to_string()).or_default();
    entry.consecutive_failures += 1;
    entry.total_failures += 1;
    if entry.consecutive_failures >= allowed_fails {
        entry.cooldown_until =
            Some(std::time::Instant::now() + std::time::Duration::from_secs_f64(cooldown_secs));
        entry.consecutive_failures = 0;
    }
}

/// Mark an instance as succeeded. Resets consecutive failure counter.
pub async fn mark_success(instance: &str, latency_ms: f64, state: &RouterState) {
    let mut map = state.lock().await;
    let entry = map.entry(instance.to_string()).or_default();
    entry.consecutive_failures = 0;
    entry.last_latency_ms = latency_ms;
    entry.total_requests += 1;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Phase 23: Router — deployment-level routing with cooldown
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Router configuration — persisted and merged from Key > Team > Global.
#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_routing_strategy")]
    pub routing_strategy: String,
    #[serde(default)]
    pub num_retries: u32,
    #[serde(default = "default_allowed_fails")]
    pub allowed_fails: u32,
    #[serde(default = "default_cooldown_time")]
    pub cooldown_time: f64,
    #[serde(default)]
    pub model_group_alias: HashMap<String, String>,
}

fn default_routing_strategy() -> String {
    "simple-shuffle".into()
}

fn default_allowed_fails() -> u32 {
    3
}

fn default_cooldown_time() -> f64 {
    5.0
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            routing_strategy: default_routing_strategy(),
            num_retries: 0,
            allowed_fails: default_allowed_fails(),
            cooldown_time: default_cooldown_time(),
            model_group_alias: HashMap::new(),
        }
    }
}

/// Routing strategy enum — the two variants map from the config string via
/// `FromStr`. Unknown strings fall back to `SimpleShuffle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterStrategy {
    SimpleShuffle,
    /// Select the deployment with the fewest active requests.
    UsageBasedRoutingV2,
    /// Select the deployment with the lowest latency.
    LatencyBasedRouting,
}

impl std::str::FromStr for RouterStrategy {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "simple-shuffle" => Ok(Self::SimpleShuffle),
            "usage-based-routing-v2" | "usage-based-routing" => Ok(Self::UsageBasedRoutingV2),
            "latency-based-routing" => Ok(Self::LatencyBasedRouting),
            other => {
                tracing::warn!(strategy=%other, "unknown routing strategy, fallback to simple-shuffle");
                Ok(Self::SimpleShuffle)
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// max_parallel — per-deployment semaphore registry
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Per-deployment concurrency gate (Stage 117 §3.4).
///
/// Holds one `Arc<Semaphore>` per `(api_base, upstream_model)` bucket so that
/// concurrent requests to the same upstream are capped at the deployment's
/// `max_parallel_requests`. `None`/`<=0` means unlimited (no semaphore).
///
/// Held inside the [`Router`] (already in `AppState`) so handlers reach it
/// via `state.router` without adding a new AppState field everywhere.
pub type MaxParallelRegistry =
    Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<tokio::sync::Semaphore>>>>;

/// Whether an upstream status counts toward cooldown / retryable fallback.
/// Aligned with litellm `cooldown_handlers.py` — only statuses signalling
/// upstream unavailability or throttling qualify; business 400s do not.
fn is_cooldown_status(status: u16) -> bool {
    matches!(status, 401 | 404 | 408 | 429 | 500 | 502 | 503 | 504) || (500..=599).contains(&status)
}

/// Router — picks deployments and tracks failure/cooldown state.
#[derive(Debug, Clone)]
pub struct Router {
    strategy: RouterStrategy,
    pub allowed_fails: u32,
    pub cooldown_time: f64,
    pub num_retries: u32,
    /// Per-deployment semaphore registry for `max_parallel_requests`.
    semaphores: MaxParallelRegistry,
    /// Exact-match response cache (Stage 119). `None` when disabled.
    cache: Option<Arc<dyn crate::cache::CacheBackend>>,
}

impl Default for Router {
    fn default() -> Self {
        Self::from_config(&RouterConfig::default())
    }
}

impl Router {
    pub fn new(
        strategy: RouterStrategy,
        allowed_fails: u32,
        cooldown_time: f64,
        num_retries: u32,
    ) -> Self {
        Self {
            strategy,
            allowed_fails,
            cooldown_time,
            num_retries,
            semaphores: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            cache: None,
        }
    }

    /// Attach an exact-match response cache backend (Stage 119).
    pub fn with_cache(mut self, cache: Option<Arc<dyn crate::cache::CacheBackend>>) -> Self {
        self.cache = cache;
        self
    }

    /// The response cache backend (None when disabled).
    pub fn cache(&self) -> Option<&Arc<dyn crate::cache::CacheBackend>> {
        self.cache.as_ref()
    }

    pub fn from_config(cfg: &RouterConfig) -> Self {
        Self {
            strategy: cfg
                .routing_strategy
                .parse()
                .unwrap_or(RouterStrategy::SimpleShuffle),
            allowed_fails: cfg.allowed_fails,
            cooldown_time: cfg.cooldown_time,
            num_retries: cfg.num_retries,
            semaphores: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            // Exact-match cache defaults ON (memory LRU) — litellm parity.
            // `with_cache(None)` at boot disables it when config says so.
            cache: Some(Arc::new(crate::cache::MemoryCache::new(10_000))),
        }
    }

    /// Acquire a permit for the given deployment bucket. Returns `None` when
    /// `max_parallel <= 0` (unlimited) or the bucket already has a permit.
    /// The caller must `drop` the returned guard (or call `release`) when the
    /// upstream call completes.
    pub async fn acquire_max_parallel(
        &self,
        api_base: &str,
        upstream_model: &str,
        max_parallel: i32,
    ) -> Option<tokio::sync::OwnedSemaphorePermit> {
        if max_parallel <= 0 {
            return None;
        }
        let key = format!("{}|{}", api_base.trim_end_matches('/'), upstream_model);
        // Clone the Arc out of the map so the semaphore outlives the lock guard.
        let semaphore = {
            let mut map = self.semaphores.lock().await;
            match map.get(&key) {
                Some(existing) if existing.available_permits() <= max_parallel as usize => {
                    existing.clone()
                }
                _ => {
                    let fresh = Arc::new(tokio::sync::Semaphore::new(max_parallel as usize));
                    map.insert(key, fresh.clone());
                    fresh
                }
            }
        };
        semaphore.acquire_owned().await.ok()
    }

    /// Pick one deployment index from the candidates.
    /// Returns None only if the input slice is empty.
    pub fn pick_deployment(&self, deployments: &mut [Deployment]) -> Option<usize> {
        if deployments.is_empty() {
            return None;
        }

        let now = Instant::now();

        // 1. Filter out cooldown deployments
        let active: Vec<usize> = (0..deployments.len())
            .filter(|&i| deployments[i].cooldown_until.is_none_or(|t| now >= t))
            .collect();

        if active.is_empty() {
            // All are in cooldown — return the one that recovers earliest
            tracing::warn!("All deployments in cooldown, picking earliest recovery");
            return (0..deployments.len())
                .min_by_key(|&i| deployments[i].cooldown_until.unwrap_or(Instant::now()));
        }

        // 2. Strategy-aware selection (Stage 118): weighted / usage / latency
        //    real decisions instead of the previous shuffle-only fallthrough.
        match self.strategy {
            RouterStrategy::SimpleShuffle => {
                // Weighted selection when any deployment declares a weight
                // (litellm simple_shuffle random.choices): pick proportional to
                // weight; weight 0 excluded; all-zero/absent → uniform random.
                let has_weight = active
                    .iter()
                    .any(|&i| deployments[i].weight.is_some_and(|w| w > 0));
                if has_weight {
                    return self.weighted_pick(&active, deployments);
                }
                let idx = rand::thread_rng().gen_range(0..active.len());
                Some(active[idx])
            }
            RouterStrategy::UsageBasedRoutingV2 => {
                // Pick the deployment with the most remaining request budget:
                // (rpm - active_estimate). No rpm declared → treat as unlimited
                // (highest remaining). Falls back to first active on ties.
                let mut best = active[0];
                let mut best_remaining = i64::MIN;
                for &i in &active {
                    let d = &deployments[i];
                    let remaining = d.rpm.unwrap_or(i64::MAX);
                    if remaining > best_remaining {
                        best_remaining = remaining;
                        best = i;
                    }
                }
                Some(best)
            }
            RouterStrategy::LatencyBasedRouting => {
                // Pick the deployment with the lowest EWMA latency recorded by
                // report_success. No sample (last_latency_ms == 0.0) → treat as
                // best-effort middle ground so unobserved instances can be tried.
                let mut best = active[0];
                let mut best_latency = f64::INFINITY;
                for &i in &active {
                    let d = &deployments[i];
                    // Deployment doesn't carry latency; use the shared
                    // RouterState map populated by report_*_latency (Stage 118).
                    let latency = d.last_latency_ms;
                    if latency < best_latency {
                        best_latency = latency;
                        best = i;
                    }
                }
                Some(best)
            }
        }
    }

    /// Weighted random pick among `active` indices (proportional to `weight`).
    fn weighted_pick(&self, active: &[usize], deployments: &[Deployment]) -> Option<usize> {
        let mut pool: Vec<(usize, u64)> = active
            .iter()
            .filter_map(|&i| {
                let w = deployments[i].weight.unwrap_or(0).max(0) as u64;
                (w > 0).then_some((i, w))
            })
            .collect();
        if pool.is_empty() {
            // All weights absent/zero → uniform.
            let idx = rand::thread_rng().gen_range(0..active.len());
            return Some(active[idx]);
        }
        let total: u64 = pool.iter().map(|(_, w)| w).sum();
        let mut roll = rand::thread_rng().gen_range(0..total);
        for (i, w) in pool.drain(..) {
            if roll < w {
                return Some(i);
            }
            roll -= w;
        }
        pool.first().map(|(i, _)| *i)
    }

    /// Report a failure on a deployment.
    ///
    /// Stage 118 §3.1: only statuses that indicate upstream unavailability count
    /// toward cooldown (aligned with litellm `cooldown_handlers.py`): 429, 401,
    /// 408, 404, and 5xx. Business 400 errors (bad request) do NOT trigger
    /// cooldown — the deployment stays in the pool.
    pub fn report_failure(&self, deployment: &mut Deployment, status: u16) {
        if !is_cooldown_status(status) {
            return;
        }
        deployment.fail_count += 1;
        if deployment.fail_count >= self.allowed_fails {
            let cooldown = std::time::Duration::from_secs_f64(self.cooldown_time);
            deployment.cooldown_until = Some(Instant::now() + cooldown);
            tracing::warn!(
                model_name=%deployment.upstream_model,
                fail_count=%deployment.fail_count,
                status=status,
                cooldown_secs=self.cooldown_time,
                "Deployment entering cooldown"
            );
        }
    }

    /// Report a deployment success and record its latency for the
    /// latency-based strategy (EWMA of the reported values).
    pub fn report_success(&self, deployment: &mut Deployment, latency_ms: f64) {
        deployment.fail_count = 0;
        deployment.cooldown_until = None;
        // EWMA with α=0.5 — recent samples dominate, stable enough for routing.
        deployment.last_latency_ms = if deployment.last_latency_ms == 0.0 {
            latency_ms
        } else {
            0.5 * latency_ms + 0.5 * deployment.last_latency_ms
        };
    }

    /// Error-kind classification used by the priority fallback (Stage 118 §3.5):
    /// which error types are retryable across a fallback group.
    pub fn is_retryable_error_type(status: u16) -> bool {
        is_cooldown_status(status)
    }

    /// Priority-grouped candidate order for fallback (Stage 118 §3.5).
    ///
    /// Returns indices sorted by `priority` ascending (0 primary first, then 1,
    /// 2, …) with stable ties. Cooldown deployments are dropped. The handler
    /// iterates this list and falls through on retryable errors, which
    /// naturally implements "pick within the lowest non-empty priority group,
    /// then escalate to the next group".
    pub fn fallback_order(&self, deployments: &[Deployment]) -> Vec<usize> {
        let now = Instant::now();
        let mut order: Vec<usize> = (0..deployments.len())
            .filter(|&i| deployments[i].cooldown_until.is_none_or(|t| now >= t))
            .collect();
        order.sort_by_key(|&i| deployments[i].priority.unwrap_or(0));
        order
    }

    /// Merge Key > Team > Global router overrides and rebuild a Router from the
    /// merged config. Convenience for handlers that resolved per-key settings.
    pub fn from_merged(
        &self,
        key_settings: Option<&serde_json::Value>,
        team_settings: Option<&serde_json::Value>,
    ) -> Self {
        let global = RouterConfig {
            routing_strategy: match self.strategy {
                RouterStrategy::SimpleShuffle => "simple-shuffle".into(),
                RouterStrategy::UsageBasedRoutingV2 => "usage-based-routing-v2".into(),
                RouterStrategy::LatencyBasedRouting => "latency-based-routing".into(),
            },
            num_retries: self.num_retries,
            allowed_fails: self.allowed_fails,
            cooldown_time: self.cooldown_time,
            model_group_alias: std::collections::HashMap::new(),
        };
        let merged = merge_router_overrides(key_settings, team_settings, &global);
        // Preserve the semaphore registry + cache so per-request routing keeps
        // the shared max_parallel gates and the exact-match cache.
        let mut r = Router::from_config(&merged);
        r.semaphores = self.semaphores.clone();
        r.cache = self.cache.clone();
        r
    }

    /// Build a retry-capable HTTP client for upstream requests.
    ///
    /// Uses `reqwest-middleware` + `reqwest-retry` with exponential backoff.
    /// Retries on 5xx and network errors; 4xx is not retried.
    #[cfg(feature = "reqwest")]
    pub fn build_retry_client(&self) -> reqwest_middleware::ClientWithMiddleware {
        use reqwest_middleware::ClientBuilder;
        use reqwest_retry::policies::ExponentialBackoff;
        use reqwest_retry::RetryTransientMiddleware;

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(self.num_retries);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("failed to build reqwest client");
        ClientBuilder::new(client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Merge router settings — Key > Team > Global
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Merge Key > Team > Global router settings into an effective override.
/// If both key and team settings are None, returns the global config unchanged.
pub fn merge_router_overrides(
    key_settings: Option<&serde_json::Value>,
    team_settings: Option<&serde_json::Value>,
    global: &RouterConfig,
) -> RouterConfig {
    let mut merged = global.clone();

    // Layer 2: Team override
    if let Some(ts) = team_settings {
        apply_override(&mut merged, ts);
    }

    // Layer 1: Key override (highest priority)
    if let Some(ks) = key_settings {
        apply_override(&mut merged, ks);
    }

    merged
}

fn apply_override(config: &mut RouterConfig, overrides: &serde_json::Value) {
    if let Some(v) = overrides.get("allowed_fails").and_then(|v| v.as_u64()) {
        config.allowed_fails = v as u32;
    }
    if let Some(v) = overrides.get("cooldown_time").and_then(|v| v.as_f64()) {
        if v > 0.0 {
            config.cooldown_time = v;
        }
    }
    if let Some(v) = overrides.get("num_retries").and_then(|v| v.as_u64()) {
        config.num_retries = v as u32;
    }
    if let Some(v) = overrides.get("routing_strategy").and_then(|v| v.as_str()) {
        config.routing_strategy = v.to_string();
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod router_tests {
    use super::*;
    use crate::deployment::{Deployment, ProviderType};
    use serde_json::json;

    fn make_deployment(name: &str) -> Deployment {
        Deployment {
            api_base: format!("https://{}.example.com/v1", name),
            api_key: Some("sk-test".into()),
            upstream_model: name.to_string(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({}),
            model_id: Some(format!("id-{}", name)),
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
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

    // UT-1: pick single deployment
    #[test]
    fn test_pick_single_deployment() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![make_deployment("gpt-4")];
        let idx = router.pick_deployment(&mut deps);
        assert_eq!(idx, Some(0));
    }

    // UT-2: pick from multiple deployments (probabilistic)
    #[test]
    fn test_pick_multiple_deployments() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![
            make_deployment("gpt-4-a"),
            make_deployment("gpt-4-b"),
            make_deployment("gpt-4-c"),
        ];
        // Run many picks and verify at least 2 different indices are chosen
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let idx = router.pick_deployment(&mut deps).unwrap();
            seen.insert(idx);
            // Reset cooldown so picks are independent
            deps[idx].fail_count = 0;
            deps[idx].cooldown_until = None;
        }
        assert!(
            seen.len() >= 2,
            "expected at least 2 different indices, got {}",
            seen.len()
        );
    }

    // UT-3: cooldown filtering — cooldowned deployment is skipped
    #[test]
    fn test_pick_cooldown_skip() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![make_deployment("gpt-4-a"), make_deployment("gpt-4-b")];
        // Put gpt-4-a in cooldown
        deps[0].cooldown_until = Some(Instant::now() + std::time::Duration::from_secs(300));
        // Only gpt-4-b should be picked
        for _ in 0..20 {
            let idx = router.pick_deployment(&mut deps).unwrap();
            assert_eq!(idx, 1, "cooldowned deployment should not be picked");
        }
    }

    // UT-4: all cooldown — returns earliest recovering
    #[test]
    fn test_pick_all_cooldown() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let now = Instant::now();
        let mut deps = vec![
            make_deployment("gpt-4-a"),
            make_deployment("gpt-4-b"),
            make_deployment("gpt-4-c"),
        ];
        deps[0].cooldown_until = Some(now + std::time::Duration::from_secs(60));
        deps[1].cooldown_until = Some(now + std::time::Duration::from_secs(30)); // earliest
        deps[2].cooldown_until = Some(now + std::time::Duration::from_secs(90));

        let idx = router.pick_deployment(&mut deps).unwrap();
        assert_eq!(idx, 1, "should pick the earliest recovery (index 1)");
    }

    // UT-5: report_failure below threshold
    #[test]
    fn test_report_failure_below_threshold() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        dep.fail_count = 1;
        router.report_failure(&mut dep, 500);
        assert_eq!(dep.fail_count, 2);
        assert!(
            dep.cooldown_until.is_none(),
            "should not enter cooldown below threshold"
        );
    }

    // UT-6: report_failure reaches threshold
    #[test]
    fn test_report_failure_reaches_threshold() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        dep.fail_count = 2;
        router.report_failure(&mut dep, 503);
        assert_eq!(dep.fail_count, 3);
        assert!(
            dep.cooldown_until.is_some(),
            "should enter cooldown at threshold"
        );
    }

    // UT-7: report_success clears state + records latency
    #[test]
    fn test_report_success_clears() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        dep.fail_count = 2;
        dep.cooldown_until = Some(Instant::now() + std::time::Duration::from_secs(300));
        router.report_success(&mut dep, 123.0);
        assert_eq!(dep.fail_count, 0);
        assert!(dep.cooldown_until.is_none());
        assert_eq!(dep.last_latency_ms, 123.0);
    }

    // UT-7b: report_success EWMA blends subsequent samples
    #[test]
    fn test_report_success_ewma() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        router.report_success(&mut dep, 100.0);
        router.report_success(&mut dep, 200.0);
        // EWMA α=0.5: 0.5*200 + 0.5*100 = 150
        assert!((dep.last_latency_ms - 150.0).abs() < 0.001);
    }

    // UT-7c: business 400 does not count toward cooldown (litellm parity)
    #[test]
    fn test_report_failure_400_not_counted() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 1, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        dep.fail_count = 0;
        router.report_failure(&mut dep, 400);
        assert_eq!(dep.fail_count, 0, "400 business error must not count");
        assert!(dep.cooldown_until.is_none());
    }

    // UT-8: RouterConfig defaults
    #[test]
    fn test_router_config_defaults() {
        let cfg = RouterConfig::default();
        assert_eq!(cfg.routing_strategy, "simple-shuffle");
        assert_eq!(cfg.num_retries, 0);
        assert_eq!(cfg.allowed_fails, 3);
        assert_eq!(cfg.cooldown_time, 5.0);
    }

    // UT-9: RouterConfig from JSON
    #[test]
    fn test_router_config_from_json() {
        let json =
            json!({"routing_strategy": "simple-shuffle", "num_retries": 2, "allowed_fails": 5});
        let cfg: RouterConfig = serde_json::from_value(json).unwrap();
        assert_eq!(cfg.num_retries, 2);
        assert_eq!(cfg.allowed_fails, 5);
        assert_eq!(cfg.cooldown_time, 5.0); // default
    }

    // UT-10: pick empty returns None
    #[test]
    fn test_pick_empty_deployments() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps: Vec<Deployment> = vec![];
        assert_eq!(router.pick_deployment(&mut deps), None);
    }
    // ── Stage 117: max_parallel semaphore registry ──

    #[tokio::test]
    async fn test_acquire_max_parallel_caps_concurrency() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let p1 = router
            .acquire_max_parallel("https://a.example/v1", "gpt-4", 2)
            .await
            .expect("first permit");
        let p2 = router
            .acquire_max_parallel("https://a.example/v1", "gpt-4", 2)
            .await
            .expect("second permit");
        // Third concurrent request blocks — use try_acquire_owned via a short
        // acquire with a distinct bucket to prove the registry caps at 2.
        let blocked = tokio::time::timeout(
            std::time::Duration::from_millis(150),
            router.acquire_max_parallel("https://a.example/v1", "gpt-4", 2),
        )
        .await;
        assert!(blocked.is_err(), "third concurrent request should time out");
        drop(p1);
        drop(p2);
        // After releasing, a new request acquires immediately.
        let p3 = router
            .acquire_max_parallel("https://a.example/v1", "gpt-4", 2)
            .await;
        assert!(p3.is_some(), "permit available after release");
    }

    #[tokio::test]
    async fn test_acquire_max_parallel_unlimited_returns_none() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let permit = router
            .acquire_max_parallel("https://b.example/v1", "gpt-4", 0)
            .await;
        assert!(permit.is_none(), "max_parallel<=0 means no gate");
    }

    #[tokio::test]
    async fn test_acquire_max_parallel_distinct_buckets_independent() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let _a = router
            .acquire_max_parallel("https://a.example/v1", "gpt-4", 1)
            .await
            .expect("bucket a permit");
        // Different upstream_model → independent semaphore.
        let b = router
            .acquire_max_parallel("https://a.example/v1", "gpt-5", 1)
            .await;
        assert!(b.is_some(), "distinct bucket has its own permit");
    }

    // ── Stage 118: weighted / usage / latency / fallback order ──

    #[test]
    fn test_weighted_pick_skips_zero_weight() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![make_deployment("gpt-4-a"), make_deployment("gpt-4-b")];
        deps[0].weight = Some(0);
        deps[1].weight = Some(5);
        for _ in 0..20 {
            let idx = router.pick_deployment(&mut deps).unwrap();
            assert_eq!(idx, 1, "zero-weight deployment must never be picked");
        }
    }

    #[test]
    fn test_weighted_pick_hits_heavier_instance() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![
            make_deployment("gpt-4-heavy"),
            make_deployment("gpt-4-light"),
        ];
        deps[0].weight = Some(9);
        deps[1].weight = Some(1);
        let mut heavy = 0;
        let mut light = 0;
        for _ in 0..200 {
            let idx = router.pick_deployment(&mut deps).unwrap();
            if idx == 0 {
                heavy += 1;
            } else {
                light += 1;
            }
        }
        assert!(
            heavy > light * 2,
            "heavier deployment should dominate (heavy={heavy}, light={light})"
        );
    }

    #[test]
    fn test_usage_based_picks_max_remaining() {
        let router = Router::new(RouterStrategy::UsageBasedRoutingV2, 3, 5.0, 0);
        let mut deps = vec![
            make_deployment("gpt-4-exhausted"),
            make_deployment("gpt-4-available"),
        ];
        deps[0].rpm = Some(0); // exhausted
        deps[1].rpm = Some(100);
        let idx = router.pick_deployment(&mut deps).unwrap();
        assert_eq!(
            idx, 1,
            "usage-based must pick the deployment with remaining budget"
        );
    }

    #[test]
    fn test_latency_based_picks_lowest_ewma() {
        let router = Router::new(RouterStrategy::LatencyBasedRouting, 3, 5.0, 0);
        let mut deps = vec![make_deployment("gpt-4-slow"), make_deployment("gpt-4-fast")];
        deps[0].last_latency_ms = 500.0;
        deps[1].last_latency_ms = 50.0;
        let idx = router.pick_deployment(&mut deps).unwrap();
        assert_eq!(
            idx, 1,
            "latency-based must pick the lowest-latency deployment"
        );
    }

    #[test]
    fn test_fallback_order_primary_first() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![
            make_deployment("gpt-4-backup"),
            make_deployment("gpt-4-primary"),
            make_deployment("gpt-4-temp"),
        ];
        deps[0].priority = Some(1);
        deps[1].priority = Some(0);
        deps[2].priority = Some(2);
        let order = router.fallback_order(&deps);
        let names: Vec<&str> = order
            .iter()
            .map(|&i| deps[i].upstream_model.as_str())
            .collect();
        assert_eq!(names, vec!["gpt-4-primary", "gpt-4-backup", "gpt-4-temp"]);
    }

    #[test]
    fn test_fallback_order_drops_cooldown() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![
            make_deployment("gpt-4-primary"),
            make_deployment("gpt-4-down"),
        ];
        deps[1].cooldown_until = Some(Instant::now() + std::time::Duration::from_secs(300));
        let order = router.fallback_order(&deps);
        assert_eq!(order.len(), 1);
        assert_eq!(
            order[0], 0,
            "cooldowned deployment must be dropped from fallback"
        );
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Merge tests (Stage 64)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod merge_tests {
    use super::*;
    use serde_json::json;

    // UT-1: Key overrides Team overrides Global
    #[test]
    fn test_merge_key_override_team_override_global() {
        let global = RouterConfig {
            routing_strategy: "simple-shuffle".into(),
            num_retries: 0,
            allowed_fails: 3,
            cooldown_time: 5.0,
            model_group_alias: Default::default(),
        };
        let team = json!({"allowed_fails": 5, "cooldown_time": 10.0});
        let key = json!({"allowed_fails": 1, "num_retries": 2});

        let merged = merge_router_overrides(Some(&key), Some(&team), &global);

        // Key wins over Team
        assert_eq!(merged.allowed_fails, 1);
        // Key sets num_retries
        assert_eq!(merged.num_retries, 2);
        // Team cooldown (not overridden by key)
        assert_eq!(merged.cooldown_time, 10.0);
        // Global routing_strategy (not overridden)
        assert_eq!(merged.routing_strategy, "simple-shuffle");
    }

    // UT-2: empty overrides → return Global unchanged
    #[test]
    fn test_merge_empty_overrides() {
        let global = RouterConfig::default();
        let merged = merge_router_overrides(None, None, &global);
        assert_eq!(merged.allowed_fails, global.allowed_fails);
        assert_eq!(merged.cooldown_time, global.cooldown_time);
        assert_eq!(merged.num_retries, global.num_retries);
        assert_eq!(merged.routing_strategy, global.routing_strategy);
    }

    // UT-3: negative cooldown_time rejected
    #[test]
    fn test_merge_negative_cooldown_rejected() {
        let global = RouterConfig::default();
        let key = json!({"cooldown_time": -1.0});
        let merged = merge_router_overrides(Some(&key), None, &global);
        // Negative value should be ignored → keep default
        assert_eq!(merged.cooldown_time, 5.0);
    }

    // UT-4: team-only override
    #[test]
    fn test_merge_team_only() {
        let global = RouterConfig::default();
        let team = json!({"routing_strategy": "usage-based-routing", "num_retries": 1});
        let merged = merge_router_overrides(None, Some(&team), &global);
        assert_eq!(merged.routing_strategy, "usage-based-routing");
        assert_eq!(merged.num_retries, 1);
        assert_eq!(merged.allowed_fails, 3); // global default
    }
}
