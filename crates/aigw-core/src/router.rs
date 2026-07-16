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

/// Router strategy enum.
#[derive(Debug, Clone)]
pub enum RouterStrategy {
    SimpleShuffle,
    // Future: LeastBusy, UsageBasedRouting, LatencyBased
}

impl RouterStrategy {
    pub fn from_str(s: &str) -> Self {
        match s {
            "simple-shuffle" => Self::SimpleShuffle,
            other => {
                tracing::warn!(strategy=%other, "unknown routing strategy, fallback to simple-shuffle");
                Self::SimpleShuffle
            }
        }
    }
}

/// Router — picks deployments and tracks failure/cooldown state.
#[derive(Debug, Clone)]
pub struct Router {
    strategy: RouterStrategy,
    pub allowed_fails: u32,
    pub cooldown_time: f64,
    pub num_retries: u32,
}

impl Default for Router {
    fn default() -> Self {
        Self::from_config(&RouterConfig::default())
    }
}

impl Router {
    pub fn new(strategy: RouterStrategy, allowed_fails: u32, cooldown_time: f64, num_retries: u32) -> Self {
        Self { strategy, allowed_fails, cooldown_time, num_retries }
    }

    pub fn from_config(cfg: &RouterConfig) -> Self {
        Self {
            strategy: RouterStrategy::from_str(&cfg.routing_strategy),
            allowed_fails: cfg.allowed_fails,
            cooldown_time: cfg.cooldown_time,
            num_retries: cfg.num_retries,
        }
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
            .filter(|&i| {
                deployments[i]
                    .cooldown_until
                    .map_or(true, |t| now >= t)
            })
            .collect();

        if active.is_empty() {
            // All are in cooldown — return the one that recovers earliest
            tracing::warn!("All deployments in cooldown, picking earliest recovery");
            return (0..deployments.len()).min_by_key(|&i| {
                deployments[i].cooldown_until.unwrap_or(Instant::now())
            });
        }

        // 2. Shuffle and pick
        match self.strategy {
            RouterStrategy::SimpleShuffle => {
                let idx = rand::thread_rng().gen_range(0..active.len());
                Some(active[idx])
            }
        }
    }

    /// Report a failure on a deployment.
    pub fn report_failure(&self, deployment: &mut Deployment) {
        deployment.fail_count += 1;
        if deployment.fail_count >= self.allowed_fails {
            let cooldown = std::time::Duration::from_secs_f64(self.cooldown_time);
            deployment.cooldown_until = Some(Instant::now() + cooldown);
            tracing::warn!(
                model_name=%deployment.upstream_model,
                fail_count=%deployment.fail_count,
                cooldown_secs=self.cooldown_time,
                "Deployment entering cooldown"
            );
        }
    }

    /// Clear failure state on success.
    pub fn report_success(&self, deployment: &mut Deployment) {
        deployment.fail_count = 0;
        deployment.cooldown_until = None;
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
            raw_params: json!({}),
            model_id: Some(format!("id-{}", name)),
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            fail_count: 0,
            cooldown_until: None,
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
        assert!(seen.len() >= 2, "expected at least 2 different indices, got {}", seen.len());
    }

    // UT-3: cooldown filtering — cooldowned deployment is skipped
    #[test]
    fn test_pick_cooldown_skip() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut deps = vec![
            make_deployment("gpt-4-a"),
            make_deployment("gpt-4-b"),
        ];
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
        router.report_failure(&mut dep);
        assert_eq!(dep.fail_count, 2);
        assert!(dep.cooldown_until.is_none(), "should not enter cooldown below threshold");
    }

    // UT-6: report_failure reaches threshold
    #[test]
    fn test_report_failure_reaches_threshold() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        dep.fail_count = 2;
        router.report_failure(&mut dep);
        assert_eq!(dep.fail_count, 3);
        assert!(dep.cooldown_until.is_some(), "should enter cooldown at threshold");
    }

    // UT-7: report_success clears state
    #[test]
    fn test_report_success_clears() {
        let router = Router::new(RouterStrategy::SimpleShuffle, 3, 5.0, 0);
        let mut dep = make_deployment("gpt-4");
        dep.fail_count = 2;
        dep.cooldown_until = Some(Instant::now() + std::time::Duration::from_secs(300));
        router.report_success(&mut dep);
        assert_eq!(dep.fail_count, 0);
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
        let json = json!({"routing_strategy": "simple-shuffle", "num_retries": 2, "allowed_fails": 5});
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
}
