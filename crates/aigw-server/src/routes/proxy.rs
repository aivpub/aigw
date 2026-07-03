//! Reverse proxy handler — router logic tests for instance selection, cooldown, and failure tracking.
//!
//! The proxy handler and auth extractor have been removed in favor of the chat.rs handler.
//! This file retains router-level unit tests for the core routing logic.

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_router_selects_instance() {
        // Verify the router can select from instances
        use aigw_core::router::{select_instance, Strategy};

        let instances: Vec<String> = vec![
            "https://instance1.test.com/v1".to_string(),
            "https://instance2.test.com/v1".to_string(),
        ];
        let state: aigw_core::router::RouterState =
            std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        for _ in 0..20 {
            let selected =
                select_instance(&instances, &state, Strategy::SimpleShuffle, 3, 30.0).await;
            assert!(selected.is_some());
            let url = selected.unwrap();
            assert!(
                url == "https://instance1.test.com/v1" || url == "https://instance2.test.com/v1"
            );
        }
    }

    #[tokio::test]
    async fn test_mark_failure_triggers_cooldown() {
        use aigw_core::router::mark_failure;

        let instance = "https://test-cooldown.example.com".to_string();
        let state: aigw_core::router::RouterState =
            std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        // Mark 3 failures (allowed_fails = 3)
        for _ in 0..3 {
            mark_failure(&instance, &state, 3, 5.0).await;
        }

        // Now the instance should be in cooldown
        let instances = vec![instance.clone()];
        let selected = aigw_core::router::select_instance(
            &instances,
            &state,
            aigw_core::router::Strategy::SimpleShuffle,
            3,
            5.0,
        )
        .await;
        assert!(selected.is_none(), "Instance should be in cooldown");
    }

    #[tokio::test]
    async fn test_mark_success_resets_failures() {
        use aigw_core::router::{mark_failure, mark_success};

        let instance = "https://test-reset.example.com".to_string();
        let state: aigw_core::router::RouterState =
            std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        // Mark 2 failures (below threshold of 3)
        mark_failure(&instance, &state, 3, 5.0).await;
        mark_failure(&instance, &state, 3, 5.0).await;

        // Mark success — resets failure counter
        mark_success(&instance, 200.0, &state).await;

        // One more failure should NOT trigger cooldown (counter was reset)
        mark_failure(&instance, &state, 3, 5.0).await;

        // Instance should still be available
        let instances = vec![instance.clone()];
        let selected = aigw_core::router::select_instance(
            &instances,
            &state,
            aigw_core::router::Strategy::SimpleShuffle,
            3,
            5.0,
        )
        .await;
        assert!(
            selected.is_some(),
            "Instance should still be available after reset"
        );
    }
}
