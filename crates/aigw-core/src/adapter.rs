//! Provider adapter conversion layer
//!
//! Provides bidirectional conversion between OpenAI Chat Completions format
//! and Anthropic/Claude Messages API format. The `ProviderAdapter` trait
//! enables aigw to act as a protocol translator:
//!
//! ```text
//! Client (OpenAI) → aigw adapter → Upstream (OpenAI)    [passthrough]
//! Client (OpenAI) → aigw adapter → Upstream (Claude)    [OpenAI→Claude]
//! Client (Claude)  → aigw adapter → Upstream (Claude)    [passthrough]
//! Client (Claude)  → aigw adapter → Upstream (OpenAI)   [Claude→OpenAI]
//! ```

use crate::models::{
    AssistantMessage, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
    ChatContent, ChatMessage, Choice, ChunkChoice, ClaudeContent, ClaudeContentBlock,
    ClaudeDelta, ClaudeImageSource, ClaudeMessage, ClaudeMessageRequest, ClaudeMessageResponse,
    ClaudeStreamEvent, ClaudeSystemMessage, ClaudeUsage, ContentPart, Delta, ImageUrl, Usage,
};

/// Provider adapter trait — converts between OpenAI and Anthropic message formats.
///
/// Each direction supports both request and response (including streaming).
pub trait ProviderAdapter {
    /// Convert an OpenAI ChatCompletion request to a Claude Messages request
    fn openai_to_claude_request(req: &ChatCompletionRequest, max_tokens: i32)
        -> ClaudeMessageRequest;

    /// Convert a Claude Messages response to an OpenAI ChatCompletion response
    fn claude_to_openai_response(resp: &ClaudeMessageResponse, model: &str)
        -> ChatCompletionResponse;

    /// Convert a Claude Messages request to an OpenAI ChatCompletion request
    fn claude_to_openai_request(req: &ClaudeMessageRequest) -> ChatCompletionRequest;

    /// Convert an OpenAI ChatCompletion response to a Claude Messages response
    fn openai_to_claude_response(resp: &ChatCompletionResponse) -> ClaudeMessageResponse;

    /// Convert a Claude stream event (SSE) to an OpenAI ChatCompletion chunk
    fn claude_stream_to_openai_chunk(
        event: &ClaudeStreamEvent,
        model: &str,
        request_id: &str,
    ) -> Option<ChatCompletionChunk>;

    /// Convert an OpenAI ChatCompletion chunk to a Claude stream event
    fn openai_chunk_to_claude_stream(
        chunk: &ChatCompletionChunk,
    ) -> Option<ClaudeStreamEvent>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// DefaultAdapter — full 4-way bidirectional implementation
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Default implementation of ProviderAdapter covering all 4 conversions.
pub struct DefaultAdapter;

impl ProviderAdapter for DefaultAdapter {
    fn openai_to_claude_request(
        req: &ChatCompletionRequest,
        max_tokens: i32,
    ) -> ClaudeMessageRequest {
        let system = extract_openai_system(&req.messages);
        let messages: Vec<ClaudeMessage> = req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(openai_message_to_claude)
            .collect();

        ClaudeMessageRequest {
            model: req.model.clone(),
            messages,
            max_tokens,
            stream: if req.stream { Some(true) } else { None },
            system: system.map(ClaudeSystemMessage::Text),
            temperature: req.temperature,
            top_p: req.top_p,
            top_k: None,
            stop_sequences: req.stop.clone(),
            metadata: None,
        }
    }

    fn claude_to_openai_response(
        resp: &ClaudeMessageResponse,
        model: &str,
    ) -> ChatCompletionResponse {
        let content_text = claude_content_to_text(&resp.content);
        ChatCompletionResponse {
            id: resp.id.clone(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: model.to_string(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".to_string(),
                    content: content_text,
                },
                finish_reason: claude_stop_to_openai(&resp.stop_reason),
            }],
            usage: Usage {
                prompt_tokens: resp.usage.input_tokens,
                completion_tokens: resp.usage.output_tokens,
                total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
            },
        }
    }

    fn claude_to_openai_request(req: &ClaudeMessageRequest) -> ChatCompletionRequest {
        let mut messages: Vec<ChatMessage> = Vec::new();
        // Convert system message
        if let Some(ref sys) = req.system {
            match sys {
                ClaudeSystemMessage::Text(t) => {
                    messages.push(ChatMessage {
                        role: "system".to_string(),
                        content: ChatContent::Text(t.clone()),
                        name: None,
                    });
                }
                ClaudeSystemMessage::Blocks(blocks) => {
                    let text = claude_blocks_to_text(blocks);
                    if !text.is_empty() {
                        messages.push(ChatMessage {
                            role: "system".to_string(),
                            content: ChatContent::Text(text),
                            name: None,
                        });
                    }
                }
            }
        }

        // Convert messages
        for msg in &req.messages {
            messages.push(claude_message_to_openai(msg));
        }

        ChatCompletionRequest {
            model: req.model.clone(),
            messages,
            stream: req.stream.unwrap_or(false),
            temperature: req.temperature,
            max_tokens: Some(req.max_tokens),
            top_p: req.top_p,
            frequency_penalty: None,
            presence_penalty: None,
            stop: req.stop_sequences.clone(),
            user: None,
        }
    }

    fn openai_to_claude_response(resp: &ChatCompletionResponse) -> ClaudeMessageResponse {
        let content_text = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        ClaudeMessageResponse {
            id: resp.id.clone(),
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![ClaudeContentBlock {
                content_type: "text".to_string(),
                text: Some(content_text),
                source: None,
            }],
            model: resp.model.clone(),
            stop_reason: openai_stop_to_claude(
                &resp.choices.first().and_then(|c| c.finish_reason.clone()),
            ),
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: resp.usage.prompt_tokens,
                output_tokens: resp.usage.completion_tokens,
            },
        }
    }

    fn claude_stream_to_openai_chunk(
        event: &ClaudeStreamEvent,
        model: &str,
        request_id: &str,
    ) -> Option<ChatCompletionChunk> {
        let now = chrono::Utc::now().timestamp();
        match event.event_type.as_str() {
            "content_block_delta" => {
                let delta = event.delta.as_ref()?;
                if delta.delta_type != "text_delta" {
                    return None;
                }
                let content = delta.text.clone()?;
                Some(ChatCompletionChunk {
                    id: request_id.to_string(),
                    object: "chat.completion.chunk".to_string(),
                    created: now,
                    model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: event.index.unwrap_or(0),
                        delta: Delta {
                            role: None,
                            content: Some(content),
                        },
                        finish_reason: None,
                    }],
                })
            }
            "message_start" => Some(ChatCompletionChunk {
                id: request_id.to_string(),
                object: "chat.completion.chunk".to_string(),
                created: now,
                model: model.to_string(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta {
                        role: Some("assistant".to_string()),
                        content: None,
                    },
                    finish_reason: None,
                }],
            }),
            "message_delta" => {
                let stop_reason = event
                    .delta
                    .as_ref()
                    .and_then(|d| {
                        if d.delta_type == "stop_reason" {
                            d.text.clone()
                        } else {
                            None
                        }
                    });
                let finish_reason = claude_stop_to_openai(&stop_reason);
                if finish_reason.is_some() {
                    Some(ChatCompletionChunk {
                        id: request_id.to_string(),
                        object: "chat.completion.chunk".to_string(),
                        created: now,
                        model: model.to_string(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: Delta {
                                role: None,
                                content: None,
                            },
                            finish_reason,
                        }],
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn openai_chunk_to_claude_stream(
        chunk: &ChatCompletionChunk,
    ) -> Option<ClaudeStreamEvent> {
        for choice in &chunk.choices {
            // Role delta → message_start event
            if choice.delta.role.is_some() {
                return Some(ClaudeStreamEvent {
                    event_type: "message_start".to_string(),
                    index: Some(choice.index),
                    delta: None,
                    content_block: None,
                    message: Some(ClaudeMessageResponse {
                        id: chunk.id.clone(),
                        response_type: "message".to_string(),
                        role: "assistant".to_string(),
                        content: vec![],
                        model: chunk.model.clone(),
                        stop_reason: None,
                        stop_sequence: None,
                        usage: ClaudeUsage {
                            input_tokens: 0,
                            output_tokens: 0,
                        },
                    }),
                    usage: None,
                });
            }
            // Content delta → content_block_delta event
            if let Some(ref text) = choice.delta.content {
                return Some(ClaudeStreamEvent {
                    event_type: "content_block_delta".to_string(),
                    index: Some(choice.index),
                    delta: Some(ClaudeDelta {
                        delta_type: "text_delta".to_string(),
                        text: Some(text.clone()),
                    }),
                    content_block: None,
                    message: None,
                    usage: None,
                });
            }
            // Finish reason → message_delta event
            if let Some(ref finish) = choice.finish_reason {
                let stop_reason = openai_stop_to_claude(&Some(finish.clone()));
                return Some(ClaudeStreamEvent {
                    event_type: "message_delta".to_string(),
                    index: Some(choice.index),
                    delta: Some(ClaudeDelta {
                        delta_type: "stop_reason".to_string(),
                        text: stop_reason,
                    }),
                    content_block: None,
                    message: None,
                    usage: None,
                });
            }
        }
        None
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helper functions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn extract_openai_system(messages: &[ChatMessage]) -> Option<String> {
    let result: String = messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| chat_content_to_string(&m.content))
        .collect::<Vec<_>>()
        .join("\n");
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn chat_content_to_string(content: &ChatContent) -> String {
    match content {
        ChatContent::Text(t) => t.clone(),
        ChatContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| p.text.clone())
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn openai_message_to_claude(msg: &ChatMessage) -> ClaudeMessage {
    match &msg.content {
        ChatContent::Text(t) => ClaudeMessage {
            role: msg.role.clone(),
            content: ClaudeContent::Text(t.clone()),
        },
        ChatContent::Parts(parts) => {
            let blocks: Vec<ClaudeContentBlock> = parts
                .iter()
                .map(|p| {
                    if let Some(ref image_url) = p.image_url {
                        ClaudeContentBlock {
                            content_type: "image".to_string(),
                            text: None,
                            source: Some(ClaudeImageSource {
                                source_type: "base64".to_string(),
                                media_type: "image/jpeg".to_string(),
                                data: image_url.url.clone(),
                            }),
                        }
                    } else {
                        ClaudeContentBlock {
                            content_type: "text".to_string(),
                            text: p.text.clone(),
                            source: None,
                        }
                    }
                })
                .collect();
            ClaudeMessage {
                role: msg.role.clone(),
                content: ClaudeContent::Blocks(blocks),
            }
        }
    }
}

fn claude_content_to_text(blocks: &[ClaudeContentBlock]) -> String {
    blocks
        .iter()
        .filter(|b| b.content_type == "text")
        .filter_map(|b| b.text.clone())
        .collect::<Vec<_>>()
        .join("")
}

fn claude_blocks_to_text(blocks: &[ClaudeContentBlock]) -> String {
    blocks
        .iter()
        .filter(|b| b.content_type == "text")
        .filter_map(|b| b.text.clone())
        .collect::<Vec<_>>()
        .join("")
}

fn claude_message_to_openai(msg: &ClaudeMessage) -> ChatMessage {
    match &msg.content {
        ClaudeContent::Text(t) => ChatMessage {
            role: msg.role.clone(),
            content: ChatContent::Text(t.clone()),
            name: None,
        },
        ClaudeContent::Blocks(blocks) => {
            let parts: Vec<ContentPart> = blocks
                .iter()
                .map(|b| {
                    if b.content_type == "image" {
                        ContentPart {
                            content_type: "image_url".to_string(),
                            text: None,
                            image_url: b.source.as_ref().map(|s| ImageUrl {
                                url: format!("data:{};base64,{}", s.media_type, s.data),
                            }),
                        }
                    } else {
                        ContentPart {
                            content_type: "text".to_string(),
                            text: b.text.clone(),
                            image_url: None,
                        }
                    }
                })
                .collect();
            ChatMessage {
                role: msg.role.clone(),
                content: ChatContent::Parts(parts),
                name: None,
            }
        }
    }
}

fn claude_stop_to_openai(stop_reason: &Option<String>) -> Option<String> {
    match stop_reason.as_deref() {
        Some("end_turn") => Some("stop".to_string()),
        Some("max_tokens") => Some("length".to_string()),
        Some("stop_sequence") => Some("stop".to_string()),
        Some(s) => Some(s.to_string()),
        None => None,
    }
}

fn openai_stop_to_claude(finish_reason: &Option<String>) -> Option<String> {
    match finish_reason.as_deref() {
        Some("stop") => Some("end_turn".to_string()),
        Some("length") => Some("max_tokens".to_string()),
        Some(s) => Some(s.to_string()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, ChatContent};

    fn make_openai_req(text: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text(text.to_string()),
                name: None,
            }],
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(1024),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            user: None,
        }
    }

    #[test]
    fn test_openai_to_claude_request_basic() {
        let req = make_openai_req("Hello, how are you?");
        let claude_req = DefaultAdapter::openai_to_claude_request(&req, 1024);

        assert_eq!(claude_req.model, "gpt-4");
        assert_eq!(claude_req.max_tokens, 1024);
        assert_eq!(claude_req.temperature, Some(0.7));
        assert_eq!(claude_req.messages.len(), 1);
        assert_eq!(
            claude_req.messages[0].role, "user"
        );
        match &claude_req.messages[0].content {
            ClaudeContent::Text(t) => assert_eq!(t, "Hello, how are you?"),
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn test_openai_to_claude_request_with_system() {
        let mut req = make_openai_req("Hi");
        req.messages.insert(0, ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text("You are helpful.".to_string()),
            name: None,
        });

        let claude_req = DefaultAdapter::openai_to_claude_request(&req, 512);
        assert_eq!(claude_req.messages.len(), 1); // only user message
        assert_eq!(claude_req.messages[0].role, "user");
        assert!(claude_req.system.is_some());
        match claude_req.system.unwrap() {
            ClaudeSystemMessage::Text(t) => assert_eq!(t, "You are helpful."),
            _ => panic!("expected text system message"),
        }
    }

    #[test]
    fn test_claude_to_openai_response() {
        let claude_resp = ClaudeMessageResponse {
            id: "msg_123".to_string(),
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![ClaudeContentBlock {
                content_type: "text".to_string(),
                text: Some("Hello! How can I help?".to_string()),
                source: None,
            }],
            model: "claude-sonnet-4-20250514".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: 10,
                output_tokens: 20,
            },
        };

        let oai_resp = DefaultAdapter::claude_to_openai_response(&claude_resp, "claude-sonnet");
        assert_eq!(oai_resp.object, "chat.completion");
        assert_eq!(oai_resp.model, "claude-sonnet");
        assert_eq!(oai_resp.choices.len(), 1);
        assert_eq!(oai_resp.choices[0].message.content, "Hello! How can I help?");
        assert_eq!(oai_resp.choices[0].message.role, "assistant");
        assert_eq!(oai_resp.choices[0].finish_reason, Some("stop".to_string()));
        assert_eq!(oai_resp.usage.prompt_tokens, 10);
        assert_eq!(oai_resp.usage.completion_tokens, 20);
        assert_eq!(oai_resp.usage.total_tokens, 30);
    }

    #[test]
    fn test_claude_to_openai_request() {
        let claude_req = ClaudeMessageRequest {
            model: "claude-sonnet".to_string(),
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text("What is Rust?".to_string()),
            }],
            max_tokens: 1024,
            stream: None,
            system: Some(ClaudeSystemMessage::Text("Be concise.".to_string())),
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
        };

        let oai_req = DefaultAdapter::claude_to_openai_request(&claude_req);
        assert_eq!(oai_req.model, "claude-sonnet");
        assert!(!oai_req.stream);
        assert_eq!(oai_req.max_tokens, Some(1024));
        assert_eq!(oai_req.messages.len(), 2); // system + user
        assert_eq!(oai_req.messages[0].role, "system");
        assert_eq!(oai_req.messages[1].role, "user");
    }

    #[test]
    fn test_openai_to_claude_response() {
        let oai_resp = ChatCompletionResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion".to_string(),
            created: 1234567890,
            model: "gpt-4".to_string(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".to_string(),
                    content: "Sure!".to_string(),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Usage {
                prompt_tokens: 5,
                completion_tokens: 3,
                total_tokens: 8,
            },
        };

        let claude_resp = DefaultAdapter::openai_to_claude_response(&oai_resp);
        assert_eq!(claude_resp.response_type, "message");
        assert_eq!(claude_resp.role, "assistant");
        assert_eq!(claude_resp.content.len(), 1);
        assert_eq!(claude_resp.content[0].text, Some("Sure!".to_string()));
        assert_eq!(claude_resp.stop_reason, Some("end_turn".to_string()));
        assert_eq!(claude_resp.usage.input_tokens, 5);
        assert_eq!(claude_resp.usage.output_tokens, 3);
    }

    #[test]
    fn test_roundtrip_openai_claude_openai() {
        let original = make_openai_req("Hello world!");
        let claude_req = DefaultAdapter::openai_to_claude_request(&original, 512);
        let roundtrip = DefaultAdapter::claude_to_openai_request(&claude_req);

        assert_eq!(roundtrip.model, original.model);
        assert_eq!(roundtrip.max_tokens, Some(512));
        assert_eq!(roundtrip.temperature, original.temperature);
        // Messages roundtrip: user message should be preserved
        assert_eq!(roundtrip.messages.len(), 1);
        assert_eq!(roundtrip.messages[0].role, "user");
    }
}
