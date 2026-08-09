//! Step bindings for anthropic_native.feature
//!
//! Tests the Anthropic Native upstream adapters: AnthropicPassthrough and OpenAIToAnthropic.

use crate::TestWorld;
use aigw_core::adapter::{
    select_adapter, AnthropicPassthrough, AnthropicToOpenAI, ClientProtocol, MessageAdapter,
};
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
            modal_pricing: None,
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Stage 103: OpenAIToAnthropic reverse conversion — image data URL
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "adapt_request 传入含 image_url 的 OpenAI Chat 请求")]
async fn when_adapt_openai_image_request(_world: &mut TestWorld, step: &Step) {
    let body_str = step.docstring.as_ref().expect("docstring").to_string();
    let body: serde_json::Value = serde_json::from_str(&body_str).unwrap();
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapter = state.adapter.expect("no adapter");
        let deploy = state.deployment.as_ref().expect("no deployment");
        let result = adapter.adapt_request(body, deploy);
        drop(state);
        s.borrow_mut().adapted = result.ok();
    });
}

/// The OpenAIToAnthropic adapter must convert the OpenAI image_url data URL into
/// a Claude image block with stripped base64 payload + derived media_type
/// (NOT the hardcoded image/jpeg + full data URL — Stage 103 fix).
#[then(expr = "Claude 请求的 image block 已剥离 data 前缀且 media_type 推导正确")]
async fn then_claude_image_block_stripped(_world: &mut TestWorld) {
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapted = state.adapted.as_ref().expect("adapt_request not called");
        let messages = adapted["messages"].as_array().expect("no messages array");
        let user_msg = messages
            .iter()
            .find(|m| m["role"] == "user")
            .expect("no user message");
        let content = user_msg["content"]
            .as_array()
            .expect("content should be an array");
        let image_block = content
            .iter()
            .find(|b| b["type"] == "image")
            .expect("no image block");
        let source = &image_block["source"];
        assert_eq!(source["type"].as_str(), Some("base64"));
        assert_eq!(source["media_type"].as_str(), Some("image/webp"));
        assert_eq!(
            source["data"].as_str(),
            Some("UklGRlNvbWVEYXRh"),
            "data must be the raw base64 payload without the data: prefix"
        );
    });
}

/// Reverse roundtrip: OpenAI image_url → Claude image block → back to OpenAI
/// content array — the data URL must survive.
#[when(expr = "adapt_response 返回含 image_url 的 OpenAI Chat 响应后 roundtrip")]
async fn when_roundtrip_image_via_adapter(_world: &mut TestWorld) {
    ADAPTER_STATE.with(|s| {
        let mut state = s.borrow_mut();
        let adapted = state.adapted.take().expect("no adapted body");
        // OpenAI Chat request → adapted Claude body already stored. Feed it back
        // through AnthropicToOpenAI (client protocol Anthropic → OpenAI upstream)
        // to complete the roundtrip.
        let adapter = AnthropicToOpenAI;
        let result = adapter.adapt_request(
            adapted,
            &Deployment {
                api_base: "https://api.openai.com/v1".into(),
                api_key: None,
                upstream_model: "gpt-4o".into(),
                provider_type: ProviderType::OpenAICompatible,
                input_cost_per_token: None,
                output_cost_per_token: None,
                cache_read_input_token_cost: None,
                cache_creation_input_token_cost: None,
                raw_params: json!({"custom_llm_provider": "openai"}),
                model_id: None,
                model_group: None,
                custom_llm_provider: Some("openai".into()),
                chat_template_compat: None,
                modal_pricing: None,
                fail_count: 0,
                cooldown_until: None,
            },
        );
        state.adapted = result.ok();
    });
}

#[then(regex = r#"^roundtrip 后 OpenAI 请求的 image_url 为 "(.+)"$"#)]
async fn then_roundtrip_image_url(_world: &mut TestWorld, expected: String) {
    ADAPTER_STATE.with(|s| {
        let state = s.borrow();
        let adapted = state.adapted.as_ref().expect("roundtrip not run");
        let messages = adapted["messages"].as_array().expect("no messages array");
        let user_msg = messages
            .iter()
            .find(|m| m["role"] == "user")
            .expect("no user message");
        let content = user_msg["content"]
            .as_array()
            .expect("content should be an array");
        let image = content
            .iter()
            .find(|p| p["type"] == "image_url")
            .expect("no image_url part");
        assert_eq!(
            image["image_url"]["url"].as_str(),
            Some(expected.as_str()),
            "roundtrip image_url should be preserved"
        );
    });
}
