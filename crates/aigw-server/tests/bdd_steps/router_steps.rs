//! Step bindings for router.feature — Stage 118 智能路由行为验收

use cucumber::{given, then};
use serde_json::json;

use crate::bdd_steps::e2e_steps::mock_upstream;
use crate::bdd_support::mock_upstream::RecordedRequest;
use crate::TestWorld;

// ━━━━ Given ━━━━

/// Update a proxy_models row's litellm_params to add `weight` for the
/// weighted-routing scenario. Targets the row by model_name + upstream_model
/// (the two-rows-same-model case creates distinct model_id per row).
#[given(regex = r#"^更新 model "(.+)" 的上游 "(.+)" weight=(\d+)$"#)]
async fn given_update_model_weight(
    world: &mut TestWorld,
    model_name: String,
    upstream_model: String,
    weight: i64,
) {
    let state = world.ensure_state().await;
    let models = state
        .db
        .list_models_by_name(&model_name)
        .await
        .expect("list models by name");
    let target = models
        .into_iter()
        .find(|m| {
            m.litellm_params.get("model").and_then(|v| v.as_str()) == Some(upstream_model.as_str())
        })
        .expect("model row with upstream_model not found");
    let mut params = target.litellm_params.clone();
    if let Some(obj) = params.as_object_mut() {
        obj.insert("weight".to_string(), json!(weight));
    }
    let mut updated = target.clone();
    updated.litellm_params = params;
    state
        .db
        .update_model(&updated)
        .await
        .expect("update model weight");
}

/// Update a key's router_settings (key-level override) to set allowed_fails
/// and cooldown_time — exercised by the cooldown scenario so one 429 triggers
/// a single-deployment exclusion.
#[given(regex = r#"^更新 key "(.+)" 的 router 设置 allowed_fails=(\d+) cooldown_time=(\d+)$"#)]
async fn given_update_key_router_settings(
    world: &mut TestWorld,
    alias: String,
    allowed_fails: i64,
    cooldown_time: i64,
) {
    let state = world.ensure_state().await;
    let raw = world
        .created_keys
        .get(&alias)
        .cloned()
        .unwrap_or_else(|| format!("sk-{}", alias));
    let token_hash = aigw_core::crypto::hash_token(&raw);
    let key = state
        .db
        .get_key_by_token(&token_hash)
        .await
        .expect("lookup")
        .expect("key exists");
    let mut updated = key.clone();
    updated.router_settings = Some(json!({
        "allowed_fails": allowed_fails,
        "cooldown_time": cooldown_time,
    }));
    state
        .db
        .update_key(&token_hash, &updated)
        .await
        .expect("update key router settings");
}

// ━━━━ Then ━━━━

async fn recorded_upstream_models(world: &TestWorld) -> Vec<String> {
    let mu = mock_upstream();
    let guard = mu.lock().await;
    guard
        .as_ref()
        .expect("mock upstream not started")
        .recorded_requests()
        .iter()
        .filter_map(|r: &RecordedRequest| {
            r.body
                .get("model")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

#[then(regex = r#"^mock 上游收到的请求中 upstream_model "(.+)" 的数量大于 "(.+)"$"#)]
async fn then_weighted_hits_dominate(world: &mut TestWorld, heavy: String, light: String) {
    let models = recorded_upstream_models(world).await;
    let heavy_count = models.iter().filter(|m| *m == &heavy).count();
    let light_count = models.iter().filter(|m| *m == &light).count();
    assert!(
        heavy_count > light_count,
        "weighted routing should prefer the heavy deployment (heavy={heavy_count}, light={light_count})"
    );
}

#[then(expr = "mock 上游收到的请求中包含两个不同 upstream_model")]
async fn then_two_distinct_upstream_models(world: &mut TestWorld) {
    let models = recorded_upstream_models(world).await;
    assert!(
        models.len() >= 2,
        "expected at least 2 upstream requests, got {}",
        models.len()
    );
    let distinct: std::collections::HashSet<&String> = models.iter().collect();
    assert!(
        distinct.len() >= 2,
        "cooldown should route the second request to a different deployment, got {:?}",
        distinct
    );
}
