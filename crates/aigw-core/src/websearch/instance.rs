//! Instance-level selection inside one provider: weighted pick + cooldown.
//!
//! Semantics are lifted verbatim from `router.rs` so operators have exactly one
//! mental model for `weight` and cooldown across the gateway:
//!
//! | here | router.rs |
//! |------|-----------|
//! | [`WebSearchInstanceState::cooldown_until`] | `InstanceState.cooldown_until` (`:26`) |
//! | cooldown filtering | `:79-84` |
//! | all-in-cooldown → earliest recovery | `:366-377` |
//! | "any weight declared → weighted" | `:366-377` `has_weight` |
//! | `weight: 0` excluded, empty pool → uniform | `weighted_pick` (`:419`) |
//! | failures → cooldown, success → reset | `report_failure` / `report_success` (`:450-472`) |
//!
//! This layer handles "one machine of this backend is down" (switch machines,
//! semantically identical). Switching *vendors* is the provider-level concern in
//! [`crate::websearch::WebSearchRegistry`].

use std::time::{Duration, Instant};

use crate::websearch::config::WebSearchInstanceConfig;

/// Mutable health of a single instance.
#[derive(Debug, Clone, Default)]
pub struct WebSearchInstanceState {
    pub consecutive_failures: u32,
    pub cooldown_until: Option<Instant>,
    pub total_requests: u64,
    pub total_failures: u64,
}

/// Cooldown policy shared by every instance of a provider.
#[derive(Debug, Clone, Copy)]
pub struct CooldownPolicy {
    pub allowed_fails: u32,
    pub cooldown_secs: f64,
}

impl Default for CooldownPolicy {
    fn default() -> Self {
        Self {
            allowed_fails: 3,
            cooldown_secs: 60.0,
        }
    }
}

/// Pick an instance index, or `None` when there is nothing to pick from.
///
/// `roll` supplies the randomness as a fraction in `[0, 1)` so the selection is
/// a pure function; [`pick_instance`] is the thread-rng wrapper.
///
/// Returns `None` only when no instance is enabled at all. When every enabled
/// instance is in cooldown it returns the one recovering earliest rather than
/// refusing service — same call as `router.rs:366-377`, and consistent with the
/// Phase-wide stance that unavailable infrastructure degrades rather than
/// blocks.
pub fn pick_instance_with_roll(
    instances: &[WebSearchInstanceConfig],
    states: &[WebSearchInstanceState],
    now: Instant,
    roll: f64,
) -> Option<usize> {
    let enabled: Vec<usize> = (0..instances.len())
        .filter(|&i| instances[i].enabled)
        .collect();
    if enabled.is_empty() {
        return None;
    }

    let active: Vec<usize> = enabled
        .iter()
        .copied()
        .filter(|&i| {
            states
                .get(i)
                .and_then(|s| s.cooldown_until)
                .is_none_or(|t| now >= t)
        })
        .collect();

    if active.is_empty() {
        tracing::warn!("all web search instances in cooldown, picking earliest recovery");
        return enabled
            .into_iter()
            .min_by_key(|&i| states.get(i).and_then(|s| s.cooldown_until).unwrap_or(now));
    }

    let has_weight = active
        .iter()
        .any(|&i| instances[i].weight.is_some_and(|w| w > 0));
    if !has_weight {
        let idx = ((roll * active.len() as f64) as usize).min(active.len() - 1);
        return Some(active[idx]);
    }

    let pool: Vec<(usize, u64)> = active
        .iter()
        .filter_map(|&i| {
            let w = instances[i].weight.unwrap_or(0).max(0) as u64;
            (w > 0).then_some((i, w))
        })
        .collect();
    if pool.is_empty() {
        let idx = ((roll * active.len() as f64) as usize).min(active.len() - 1);
        return Some(active[idx]);
    }

    let total: u64 = pool.iter().map(|(_, w)| w).sum();
    let mut remaining = ((roll * total as f64) as u64).min(total - 1);
    for (i, w) in &pool {
        if remaining < *w {
            return Some(*i);
        }
        remaining -= *w;
    }
    pool.last().map(|(i, _)| *i)
}

/// [`pick_instance_with_roll`] with thread-local randomness.
pub fn pick_instance(
    instances: &[WebSearchInstanceConfig],
    states: &[WebSearchInstanceState],
    now: Instant,
) -> Option<usize> {
    pick_instance_with_roll(instances, states, now, fastrand::f64())
}

/// Record a failure against an instance.
///
/// `status` is the HTTP status when there was one. Business 4xx responses do not
/// count toward cooldown (`router.rs:451` `is_cooldown_status`) — a bad key or
/// exhausted quota is not cured by parking the endpoint.
pub fn report_failure(
    state: &mut WebSearchInstanceState,
    policy: &CooldownPolicy,
    status: Option<u16>,
    now: Instant,
) {
    if let Some(s) = status {
        if !is_cooldown_status(s) {
            return;
        }
    }
    state.total_failures += 1;
    state.consecutive_failures += 1;
    if state.consecutive_failures >= policy.allowed_fails {
        state.cooldown_until = Some(now + Duration::from_secs_f64(policy.cooldown_secs));
        tracing::warn!(
            consecutive_failures = state.consecutive_failures,
            cooldown_secs = policy.cooldown_secs,
            "web search instance entering cooldown"
        );
    }
}

/// Record a success: clears the failure streak and lifts any cooldown.
pub fn report_success(state: &mut WebSearchInstanceState) {
    state.total_requests += 1;
    state.consecutive_failures = 0;
    state.cooldown_until = None;
}

/// Statuses that indicate endpoint unavailability, per `router.rs:451`.
fn is_cooldown_status(status: u16) -> bool {
    matches!(status, 429 | 401 | 408 | 404) || status >= 500
}

/// A provider's instances plus their shared mutable health.
///
/// Lives here rather than in each provider so that adding a vendor needs no
/// instance-management code at all — the new provider holds a pool and loops
/// over [`InstancePool::pick`] / [`InstancePool::report_success`] /
/// [`InstancePool::report_failure`].
#[derive(Debug)]
pub struct InstancePool {
    instances: Vec<WebSearchInstanceConfig>,
    states: std::sync::Mutex<Vec<WebSearchInstanceState>>,
    policy: CooldownPolicy,
}

impl InstancePool {
    pub fn new(instances: Vec<WebSearchInstanceConfig>, policy: CooldownPolicy) -> Self {
        let states = (0..instances.len())
            .map(|_| WebSearchInstanceState::default())
            .collect();
        Self {
            instances,
            states: std::sync::Mutex::new(states),
            policy,
        }
    }

    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    pub fn instance(&self, idx: usize) -> Option<&WebSearchInstanceConfig> {
        self.instances.get(idx)
    }

    /// Select an instance index, honouring enablement, cooldown and weights.
    pub fn pick(&self) -> Option<usize> {
        let states = self.states.lock().expect("instance states poisoned");
        pick_instance(&self.instances, &states, Instant::now())
    }

    /// Select, excluding indices already tried in this request.
    ///
    /// Used by the per-provider retry loop so a single query walks every healthy
    /// instance before giving up and letting provider-level failover take over.
    pub fn pick_excluding(&self, tried: &[usize]) -> Option<usize> {
        let states = self.states.lock().expect("instance states poisoned");
        let candidates: Vec<WebSearchInstanceConfig> = self
            .instances
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let mut c = c.clone();
                if tried.contains(&i) {
                    c.enabled = false;
                }
                c
            })
            .collect();
        pick_instance(&candidates, &states, Instant::now()).filter(|idx| !tried.contains(idx))
    }

    pub fn report_success(&self, idx: usize) {
        let mut states = self.states.lock().expect("instance states poisoned");
        if let Some(s) = states.get_mut(idx) {
            report_success(s);
        }
    }

    pub fn report_failure(&self, idx: usize, status: Option<u16>) {
        let mut states = self.states.lock().expect("instance states poisoned");
        if let Some(s) = states.get_mut(idx) {
            report_failure(s, &self.policy, status, Instant::now());
        }
    }

    /// Whether an instance is currently cooling down (test/observability aid).
    pub fn is_in_cooldown(&self, idx: usize) -> bool {
        let states = self.states.lock().expect("instance states poisoned");
        states
            .get(idx)
            .and_then(|s| s.cooldown_until)
            .is_some_and(|t| Instant::now() < t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(base: &str, weight: Option<i64>, enabled: bool) -> WebSearchInstanceConfig {
        WebSearchInstanceConfig {
            base_url: base.to_string(),
            api_key: None,
            weight,
            enabled,
        }
    }

    fn states(n: usize) -> Vec<WebSearchInstanceState> {
        vec![WebSearchInstanceState::default(); n]
    }

    #[test]
    fn test_pick_instance_skips_disabled() {
        let instances = vec![inst("http://a", None, false), inst("http://b", None, true)];
        let st = states(2);
        let now = Instant::now();
        for roll in [0.0, 0.3, 0.99] {
            assert_eq!(
                pick_instance_with_roll(&instances, &st, now, roll),
                Some(1),
                "disabled instance must never be picked"
            );
        }
    }

    #[test]
    fn test_pick_instance_returns_none_when_all_disabled() {
        let instances = vec![inst("http://a", None, false)];
        assert_eq!(
            pick_instance_with_roll(&instances, &states(1), Instant::now(), 0.0),
            None
        );
    }

    #[test]
    fn test_pick_instance_filters_cooldown() {
        let instances = vec![inst("http://a", None, true), inst("http://b", None, true)];
        let now = Instant::now();
        let mut st = states(2);
        st[0].cooldown_until = Some(now + Duration::from_secs(60));
        for roll in [0.0, 0.5, 0.99] {
            assert_eq!(
                pick_instance_with_roll(&instances, &st, now, roll),
                Some(1),
                "instance in cooldown must be excluded"
            );
        }
    }

    #[test]
    fn test_pick_instance_all_cooldown_returns_earliest_recovery() {
        let instances = vec![inst("http://a", None, true), inst("http://b", None, true)];
        let now = Instant::now();
        let mut st = states(2);
        st[0].cooldown_until = Some(now + Duration::from_secs(10));
        st[1].cooldown_until = Some(now + Duration::from_secs(60));
        assert_eq!(
            pick_instance_with_roll(&instances, &st, now, 0.9),
            Some(0),
            "all in cooldown → earliest recovery, never None"
        );
    }

    #[test]
    fn test_pick_instance_expired_cooldown_becomes_available_again() {
        let instances = vec![inst("http://a", None, true)];
        let now = Instant::now();
        let mut st = states(1);
        st[0].cooldown_until = Some(now - Duration::from_secs(1));
        assert_eq!(
            pick_instance_with_roll(&instances, &st, now, 0.0),
            Some(0),
            "past cooldown_until must be treated as available"
        );
    }

    #[test]
    fn test_pick_instance_uniform_when_no_weight() {
        let instances = vec![inst("http://a", None, true), inst("http://b", None, true)];
        let st = states(2);
        let now = Instant::now();
        assert_eq!(pick_instance_with_roll(&instances, &st, now, 0.0), Some(0));
        assert_eq!(pick_instance_with_roll(&instances, &st, now, 0.75), Some(1));
    }

    #[test]
    fn test_pick_instance_weighted_when_any_weight_declared() {
        // A weight 2 / B weight 1 → A owns rolls in [0, 2/3).
        let instances = vec![
            inst("http://a", Some(2), true),
            inst("http://b", Some(1), true),
        ];
        let st = states(2);
        let now = Instant::now();
        assert_eq!(pick_instance_with_roll(&instances, &st, now, 0.0), Some(0));
        assert_eq!(pick_instance_with_roll(&instances, &st, now, 0.5), Some(0));
        assert_eq!(pick_instance_with_roll(&instances, &st, now, 0.9), Some(1));

        // Sanity check the distribution with real randomness.
        let mut a = 0;
        for _ in 0..600 {
            if pick_instance(&instances, &st, now) == Some(0) {
                a += 1;
            }
        }
        assert!(
            (300..=500).contains(&a),
            "weight 2:1 should land A well above half, got {a}/600"
        );
    }

    #[test]
    fn test_pick_instance_excludes_zero_weight() {
        let instances = vec![
            inst("http://a", Some(0), true),
            inst("http://b", Some(1), true),
        ];
        let st = states(2);
        let now = Instant::now();
        for roll in [0.0, 0.4, 0.99] {
            assert_eq!(
                pick_instance_with_roll(&instances, &st, now, roll),
                Some(1),
                "weight 0 is an exclusion, not a tiny share"
            );
        }
    }

    #[test]
    fn test_pick_instance_all_zero_weight_falls_back_to_uniform() {
        let instances = vec![
            inst("http://a", Some(0), true),
            inst("http://b", Some(0), true),
        ];
        let st = states(2);
        let now = Instant::now();
        assert!(pick_instance_with_roll(&instances, &st, now, 0.0).is_some());
        assert_eq!(pick_instance_with_roll(&instances, &st, now, 0.75), Some(1));
    }

    #[test]
    fn test_report_failure_enters_cooldown_after_allowed_fails() {
        let policy = CooldownPolicy {
            allowed_fails: 3,
            cooldown_secs: 60.0,
        };
        let now = Instant::now();
        let mut st = WebSearchInstanceState::default();

        report_failure(&mut st, &policy, Some(503), now);
        report_failure(&mut st, &policy, Some(503), now);
        assert!(st.cooldown_until.is_none(), "not yet at allowed_fails");

        report_failure(&mut st, &policy, Some(503), now);
        assert!(st.cooldown_until.is_some(), "third failure trips cooldown");

        report_success(&mut st);
        assert_eq!(st.consecutive_failures, 0);
        assert!(st.cooldown_until.is_none(), "success lifts cooldown");
    }

    #[test]
    fn test_report_failure_ignores_business_4xx() {
        let policy = CooldownPolicy::default();
        let now = Instant::now();
        let mut st = WebSearchInstanceState::default();
        for _ in 0..10 {
            report_failure(&mut st, &policy, Some(400), now);
        }
        assert_eq!(st.consecutive_failures, 0, "400 must not count");
        assert!(st.cooldown_until.is_none());

        // 429/401/408/404 do count (router.rs:451).
        report_failure(&mut st, &policy, Some(429), now);
        assert_eq!(st.consecutive_failures, 1);
    }

    #[test]
    fn test_report_failure_without_status_counts() {
        // Transport errors and timeouts carry no status but are exactly the
        // signal cooldown exists for.
        let policy = CooldownPolicy {
            allowed_fails: 2,
            cooldown_secs: 5.0,
        };
        let now = Instant::now();
        let mut st = WebSearchInstanceState::default();
        report_failure(&mut st, &policy, None, now);
        report_failure(&mut st, &policy, None, now);
        assert!(st.cooldown_until.is_some());
    }

    #[test]
    fn test_pool_pick_excluding_walks_every_instance_then_gives_up() {
        let pool = InstancePool::new(
            vec![inst("http://a", None, true), inst("http://b", None, true)],
            CooldownPolicy::default(),
        );
        let first = pool.pick().expect("fresh pool must pick");
        let second = pool
            .pick_excluding(&[first])
            .expect("second instance must be reachable");
        assert_ne!(first, second);
        assert_eq!(
            pool.pick_excluding(&[first, second]),
            None,
            "once every instance is tried the pool is exhausted"
        );
    }

    #[test]
    fn test_pool_report_failure_drives_cooldown() {
        let pool = InstancePool::new(
            vec![inst("http://a", None, true), inst("http://b", None, true)],
            CooldownPolicy {
                allowed_fails: 1,
                cooldown_secs: 60.0,
            },
        );
        pool.report_failure(0, Some(500));
        assert!(pool.is_in_cooldown(0));
        assert_eq!(pool.pick(), Some(1), "cooled instance is skipped");

        pool.report_success(0);
        assert!(!pool.is_in_cooldown(0), "success lifts cooldown");
    }
}
