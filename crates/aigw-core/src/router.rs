//! Routing strategies (litellm-compatible)
//!
//! Available strategies:
//! - simple-shuffle: random selection
//! - usage-based-routing-v2: select the instance with the fewest active requests
//! - latency-based-routing: select the instance with the lowest recent latency
//!
//! Plus cooldown mechanism: instances with N consecutive failures are temporarily excluded.

use std::collections::HashMap;
use std::sync::Arc;
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

impl Strategy {
    pub fn from_str(s: &str) -> Self {
        match s {
            "usage-based-routing-v2" | "usage-based-routing" => Self::UsageBasedRoutingV2,
            "latency-based-routing" => Self::LatencyBasedRouting,
            _ => Self::SimpleShuffle,
        }
    }
}

/// Select an instance from the list using the given strategy
pub async fn select_instance(
    instances: &[String],
    state: &RouterState,
    strategy: Strategy,
    allowed_fails: u32,
    cooldown_secs: f64,
) -> Option<String> {
    let state_map = state.lock().await;
    let now = std::time::Instant::now();

    // Filter out instances in cooldown
    let available: Vec<&String> = instances
        .iter()
        .filter(|name| {
            if let Some(s) = state_map.get(*name) {
                s.cooldown_until.map_or(true, |t| now >= t)
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
                    state_map.get(*name).map(|s| s.active_requests).unwrap_or(0)
                })
                .map(|s| (*s).clone())
        }
        Strategy::LatencyBasedRouting => {
            // Pick instance with lowest last latency
            available
                .iter()
                .min_by(|a, b| {
                    let la = state_map.get(*a).map(|s| s.last_latency_ms).unwrap_or(0.0);
                    let lb = state_map.get(*b).map(|s| s.last_latency_ms).unwrap_or(0.0);
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
