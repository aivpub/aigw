//! Step bindings for adapter.feature

use cucumber::{given, then, when};
use cucumber::gherkin::Step;
use aigw_core::adapter::{DefaultAdapter, ProviderAdapter};
use aigw_core::models::{
    ChatCompletionRequest, ChatCompletionResponse, ClaudeMessageRequest, ClaudeMessageResponse,
};
use crate::TestWorld;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Given
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[given(expr = "一个 OpenAI ChatCompletion 请求")]
async fn given_openai_request(world: &mut TestWorld, step: &Step) {
    let doc = step.docstring.as_ref().expect("docstring not found");
    let req: ChatCompletionRequest = serde_json::from_str(doc).expect("parse OpenAI request");
    world.last_body = Some(serde_json::to_value(&req).expect("serialize request"));
}

#[given(expr = "一个包含系统消息的 OpenAI 请求")]
async fn given_openai_with_system(world: &mut TestWorld, step: &Step) {
    let doc = step.docstring.as_ref().expect("docstring not found");
    let req: ChatCompletionRequest = serde_json::from_str(doc).expect("parse OpenAI request");
    world.last_body = Some(serde_json::to_value(&req).expect("serialize request"));
}

#[given(expr = "一个 Claude Messages 响应")]
async fn given_claude_response(world: &mut TestWorld) {
    let resp = ClaudeMessageResponse {
        id: "msg_001".to_string(),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![aigw_core::models::ClaudeContentBlock {
            content_type: "text".to_string(),
            text: Some("Hello!".to_string()),
            source: None,
            id: None,
            name: None,
            input: None,
            tool_use_id: None,
            content: None,
        }],
        model: "claude-sonnet".to_string(),
        stop_reason: Some("end_turn".to_string()),
        stop_sequence: None,
        usage: aigw_core::models::ClaudeUsage {
            input_tokens: 10,
            output_tokens: 5,
        },
    };
    world.last_body = Some(serde_json::to_value(&resp).expect("serialize"));
}

#[given(expr = "一个 Claude Messages 请求")]
async fn given_claude_request(world: &mut TestWorld, step: &Step) {
    let doc = step.docstring.as_ref().expect("docstring not found");
    let req: ClaudeMessageRequest = serde_json::from_str(doc).expect("parse Claude request");
    world.last_body = Some(serde_json::to_value(&req).expect("serialize request"));
}

#[given(expr = "一个 OpenAI ChatCompletion 响应")]
async fn given_openai_response(world: &mut TestWorld) {
    let resp = ChatCompletionResponse {
        id: "chatcmpl-001".to_string(),
        object: "chat.completion".to_string(),
        created: 1234567890,
        model: "gpt-4".to_string(),
        choices: vec![aigw_core::models::Choice {
            index: 0,
            message: aigw_core::models::AssistantMessage {
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
                tool_calls: None,
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: aigw_core::models::Usage {
            prompt_tokens: 8,
            completion_tokens: 2,
            total_tokens: 10,
        },
    };
    world.last_body = Some(serde_json::to_value(&resp).expect("serialize"));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// When
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[when(expr = "通过适配器转换为 Claude 请求")]
async fn when_convert_to_claude_req(world: &mut TestWorld) {
    let body = world.last_body.take().expect("no stored request");
    let req: ChatCompletionRequest =
        serde_json::from_value(body).expect("parse OpenAI request");
    let claude_req = DefaultAdapter::openai_to_claude_request(&req, 1024);
    world.last_body =
        Some(serde_json::to_value(&claude_req).expect("serialize Claude request"));
}

#[when(expr = "通过适配器转换为 OpenAI 响应")]
async fn when_convert_to_openai_resp(world: &mut TestWorld) {
    let body = world.last_body.take().expect("no stored response");
    let resp: ClaudeMessageResponse = serde_json::from_value(body).expect("parse Claude response");
    let oai_resp = DefaultAdapter::claude_to_openai_response(&resp, "claude-sonnet");
    world.last_body =
        Some(serde_json::to_value(&oai_resp).expect("serialize OpenAI response"));
}

#[when(expr = "通过适配器转换为 OpenAI 请求")]
async fn when_convert_to_openai_req(world: &mut TestWorld) {
    let body = world.last_body.take().expect("no stored request");
    let req: ClaudeMessageRequest = serde_json::from_value(body).expect("parse Claude request");
    let oai_req = DefaultAdapter::claude_to_openai_request(&req);
    world.last_body =
        Some(serde_json::to_value(&oai_req).expect("serialize OpenAI request"));
}

#[when(expr = "通过适配器转换为 Claude 响应")]
async fn when_convert_to_claude_resp(world: &mut TestWorld) {
    let body = world.last_body.take().expect("no stored response");
    let resp: ChatCompletionResponse = serde_json::from_value(body).expect("parse OpenAI response");
    let claude_resp = DefaultAdapter::openai_to_claude_response(&resp);
    world.last_body =
        Some(serde_json::to_value(&claude_resp).expect("serialize Claude response"));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Then
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[then(regex = "^Claude 请求的 model 为 \"(.+)\"$")]
async fn then_claude_model_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let model = body.get("model").and_then(|v| v.as_str()).expect("no model field");
    assert_eq!(model, expected);
}

#[then(expr = "Claude 请求包含 messages 数组")]
async fn then_claude_has_messages(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("no messages array");
    assert!(!msgs.is_empty(), "messages array is empty");
}

#[then(expr = "Claude 请求的 max_tokens 为 {int}")]
async fn then_claude_max_tokens(world: &mut TestWorld, expected: i32) {
    let body = world.last_body.as_ref().expect("no body");
    let mt = body
        .get("max_tokens")
        .and_then(|v| v.as_i64())
        .expect("no max_tokens");
    assert_eq!(mt as i32, expected);
}

#[then(regex = "^Claude 请求的 system 字段为 \"(.+)\"$")]
async fn then_claude_system_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let system = body
        .get("system")
        .and_then(|v| v.as_str())
        .expect("no system field");
    assert_eq!(system, expected);
}

#[then(expr = "Claude 请求的 messages 只包含 user 消息")]
async fn then_claude_only_user_messages(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let msgs = body
        .get("messages")
        .and_then(|v| v.as_array())
        .expect("no messages array");
    assert_eq!(msgs.len(), 1);
    assert_eq!(
        msgs[0].get("role").and_then(|v| v.as_str()),
        Some("user")
    );
}

#[then(regex = "^OpenAI 响应的 object 为 \"(.+)\"$")]
async fn then_openai_object_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let obj = body.get("object").and_then(|v| v.as_str()).expect("no object");
    assert_eq!(obj, expected);
}

#[then(expr = "OpenAI 响应包含 choices 数组")]
async fn then_openai_has_choices(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    let choices = body
        .get("choices")
        .and_then(|v| v.as_array())
        .expect("no choices array");
    assert!(!choices.is_empty());
}

#[then(expr = "OpenAI 响应包含 usage 信息")]
async fn then_openai_has_usage(world: &mut TestWorld) {
    let body = world.last_body.as_ref().expect("no body");
    assert!(body.get("usage").is_some(), "no usage in response");
}

#[then(regex = "^OpenAI 请求的 model 为 \"(.+)\"$")]
async fn then_openai_model_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let model = body.get("model").and_then(|v| v.as_str()).expect("no model");
    assert_eq!(model, expected);
}

#[then(expr = "OpenAI 请求的 max_tokens 为 {int}")]
async fn then_openai_max_tokens(world: &mut TestWorld, expected: i32) {
    let body = world.last_body.as_ref().expect("no body");
    let mt = body
        .get("max_tokens")
        .and_then(|v| v.as_i64())
        .expect("no max_tokens");
    assert_eq!(mt as i32, expected);
}

#[then(regex = "^Claude 响应的 type 为 \"(.+)\"$")]
async fn then_claude_type_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let typ = body
        .get("response_type")  // type is renamed via #[serde(rename = "type")]
        .or_else(|| body.get("type"))
        .and_then(|v| v.as_str())
        .expect("no type field");
    assert_eq!(typ, expected);
}

#[then(regex = "^Claude 响应的 role 为 \"(.+)\"$")]
async fn then_claude_role_is(world: &mut TestWorld, expected: String) {
    let body = world.last_body.as_ref().expect("no body");
    let role = body.get("role").and_then(|v| v.as_str()).expect("no role");
    assert_eq!(role, expected);
}
