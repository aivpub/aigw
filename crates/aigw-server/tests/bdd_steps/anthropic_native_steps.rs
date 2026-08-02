//! Step bindings for anthropic_native.feature
//!
//! Tests the Anthropic Native upstream adapters: AnthropicPassthrough and OpenAIToAnthropic.

use crate::TestWorld;
use aigw_core::adapter::{select_adapter, AnthropicPassthrough, ClientProtocol, MessageAdapter};
use aigw_core::deployment::{Deployment, ProviderType};
use cucumber::gherkin::Step;
use cucumber::{given, then, when};
use serde_json::json;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// State helpers in TestWorld
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct ScenarioAdapter {
    adapter: Option<&'static dyn MessageAdapter>,
    deployment: Option<Deployment>,
    adapted: Option<serde_json::Value>,
}

impl ScenarioAdapter {
    fn new() -> Self {
        Self {
            adapter: None,
            deployment: None,
            adapted: None,
        }
    }
}

// Store scenario-level state in TestWorld. Since TestWorld doesn't have
// fields for this, we use a global thread-local.
use std::cell::RefCell;
thread_local! {
    static ADAPTER_STATE: RefCell<ScenarioAdapter> = RefCell::new(ScenarioAdapter::new());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "使用 ClientProtocol {word} 和 ProviderType {word} 选择适配器")]
async fn when_select_adapter(_world: &mut TestWorld, client: String, provider: String) {
    let client_protocol = match client.as_str() {
        "Anthropic" => ClientProtocol::Anthropic,
        "OpenAI" => ClientProtocol::OpenAI,
        _ => panic!("unknown client protocol: {}", client),
    };
    let provider_type = match provider.as_str() {
        "AnthropicNative" => ProviderType::AnthropicNative,
        "OpenAICompatible" => ProviderType::OpenAICompatible,
        _ => panic!("unknown provider type: {}", provider),
    };
    ADAPTER_STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.adapter = select_adapter(client_protocol, &provider_type);
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "适配器已选择")]
async fn then_adapter_selected(_world: &mut TestWorld) {
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        assert!(state.adapter.is_some(), "adapter should be selected");
    });
}

#[then(expr = "适配器的 client_protocol 为 {word}")]
async fn then_client_protocol(_world: &mut TestWorld, expected: String) {
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapter = state.adapter.expect("no adapter selected");
        let actual = format!("{:?}", adapter.client_protocol());
        assert_eq!(
            actual, expected,
            "expected client_protocol {:?}, got {:?}",
            expected, actual
        );
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "一个 AnthropicPassthrough 适配器")]
async fn given_passthrough_adapter(_world: &mut TestWorld) {
    ADAPTER_STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.adapter = Some(&AnthropicPassthrough);
    });
}

#[given(expr = "一个 Anthropic Native Deployment {string}")]
async fn given_anthropic_deployment(_world: &mut TestWorld, model: String) {
    ADAPTER_STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.deployment = Some(Deployment {
            api_base: "https://api.anthropic.com/v1".into(),
            api_key: Some("sk-ant-test-key".into()),
            upstream_model: model,
            provider_type: ProviderType::AnthropicNative,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({"custom_llm_provider": "anthropic"}),
            model_id: None,
            model_group: None,
            custom_llm_provider: Some("anthropic".into()),
            chat_template_compat: None,
            fail_count: 0,
            cooldown_until: None,
        });
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "adapt_request 传入 Anthropic Messages 请求")]
async fn when_adapt_anthropic_request(_world: &mut TestWorld, step: &Step) {
    let body_str = step.docstring.as_ref().expect("docstring").to_string();
    let body: serde_json::Value = serde_json::from_str(&body_str).unwrap();
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapter = state.adapter.expect("no adapter");
        let deploy = state.deployment.as_ref().expect("no deployment");
        let result = adapter.adapt_request(body, deploy);
        drop(state); // release borrow
        s.borrow_mut().adapted = result.ok();
    });
}

#[when(expr = "adapt_response 传入 Anthropic Messages 响应")]
async fn when_adapt_anthropic_response(_world: &mut TestWorld, step: &Step) {
    let body_str = step.docstring.as_ref().expect("docstring").to_string();
    let body: serde_json::Value = serde_json::from_str(&body_str).unwrap();
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapter = state.adapter.expect("no adapter");
        let result = adapter.adapt_response(body);
        drop(state);
        s.borrow_mut().adapted = result.ok();
    });
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(expr = "请求 body 不变且 model 已替换")]
async fn then_body_passthrough_model_swapped(_world: &mut TestWorld) {
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapted = state.adapted.as_ref().expect("adapt_request not called");
        assert_eq!(adapted["model"].as_str(), Some("claude-sonnet-4-20250514"));
        assert_eq!(adapted["max_tokens"].as_i64(), Some(100));
        assert_eq!(adapted["messages"][0]["role"].as_str(), Some("user"));
        assert_eq!(adapted["messages"][0]["content"].as_str(), Some("hello"));
    });
}

#[then(expr = "响应不变")]
async fn then_response_unchanged(_world: &mut TestWorld) {
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapted = state.adapted.as_ref().expect("adapt_response not called");
        assert_eq!(adapted["id"].as_str(), Some("msg_001"));
        assert_eq!(adapted["type"].as_str(), Some("message"));
        assert_eq!(adapted["stop_reason"].as_str(), Some("end_turn"));
    });
}
