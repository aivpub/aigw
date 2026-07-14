//! Message adapter conversion layer
//!
//! Provides bidirectional conversion between OpenAI Chat Completions format
//! and Anthropic/Claude Messages API format. The `MessageAdapter` trait
//! enables aigw to act as a protocol translator:
//!
//! ```text
//! Client (OpenAI) → OpenAIPassthrough → Upstream (OpenAI)    [passthrough]
//! Client (Claude)  → AnthropicToOpenAI → Upstream (OpenAI)   [Claude→OpenAI]
//! ```

use crate::models::{
    AssistantMessage, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
    ChatContent, ChatMessage, Choice, ChunkChoice, ClaudeContent, ClaudeContentBlock,
    ClaudeDelta, ClaudeImageSource, ClaudeMessage, ClaudeMessageRequest, ClaudeMessageResponse,
    ClaudeStreamEvent, ClaudeSystemMessage, ClaudeUsage, ContentPart, Delta, ImageUrl,
    ToolCall, ToolCallFunction, Usage,
};
use crate::deployment::{Deployment, ProviderType};
use serde_json::{json, Value};

/// The client-facing protocol of the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientProtocol {
    /// /v1/chat/completions
    OpenAI,
    /// /v1/messages
    Anthropic,
}

/// Message format converter — bidirectional between client protocol and upstream format.
pub trait MessageAdapter: Send + Sync {
    /// Which client protocol this adapter handles.
    fn client_protocol(&self) -> ClientProtocol;

    /// Convert request body from client format to upstream format.
    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError>;

    /// Convert non-streaming response from upstream format to client format.
    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError>;

    /// Return a streaming chunk converter, if supported.
    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>>;
}

/// Streaming chunk-by-chunk converter.
pub trait StreamAdapter: Send {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>>;
    fn finish(&mut self) -> Option<Vec<u8>>;
}

#[derive(Debug)]
pub enum AdapterError {
    Unsupported(String),
    Parse(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "Unsupported: {}", msg),
            Self::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

pub fn select_adapter(client: ClientProtocol, provider: &ProviderType) -> Option<&'static dyn MessageAdapter> {
    match (client, provider) {
        (ClientProtocol::OpenAI, ProviderType::OpenAICompatible) => Some(&OpenAIPassthrough),
        (ClientProtocol::Anthropic, ProviderType::OpenAICompatible) => Some(&AnthropicToOpenAI),
        _ => None,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Legacy ProviderAdapter trait
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub trait ProviderAdapter {
    fn openai_to_claude_request(req: &ChatCompletionRequest, max_tokens: i32) -> ClaudeMessageRequest;
    fn claude_to_openai_response(resp: &ClaudeMessageResponse, model: &str) -> ChatCompletionResponse;
    fn claude_to_openai_request(req: &ClaudeMessageRequest) -> ChatCompletionRequest;
    fn openai_to_claude_response(resp: &ChatCompletionResponse) -> ClaudeMessageResponse;
    fn claude_stream_to_openai_chunk(event: &ClaudeStreamEvent, model: &str, request_id: &str) -> Option<ChatCompletionChunk>;
    fn openai_chunk_to_claude_stream(chunk: &ChatCompletionChunk) -> Option<ClaudeStreamEvent>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OpenAIPassthrough
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct OpenAIPassthrough;

impl MessageAdapter for OpenAIPassthrough {
    fn client_protocol(&self) -> ClientProtocol { ClientProtocol::OpenAI }

    fn adapt_request(&self, mut body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        body.as_object_mut().map(|obj| {
            obj.insert("model".to_string(), json!(deployment.upstream_model));
            // Inject stream_options so upstream returns token usage in the final SSE chunk
            if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
                obj.insert("stream_options".to_string(), json!({"include_usage": true}));
            }
        });
        Ok(body)
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> { Ok(body) }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> { Some(Box::new(PassthroughStream)) }
}

struct PassthroughStream;
impl StreamAdapter for PassthroughStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> { Some(chunk.to_vec()) }
    fn finish(&mut self) -> Option<Vec<u8>> { None }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AnthropicToOpenAI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct AnthropicToOpenAI;

impl MessageAdapter for AnthropicToOpenAI {
    fn client_protocol(&self) -> ClientProtocol { ClientProtocol::Anthropic }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let req: ClaudeMessageRequest = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid Claude request: {}", e)))?;
        let oai_req = DefaultAdapter::claude_to_openai_request(&req);
        let mut json = serde_json::to_value(&oai_req).map_err(|e| AdapterError::Parse(e.to_string()))?;
        json.as_object_mut().map(|obj| {
            obj.insert("model".to_string(), json!(deployment.upstream_model));
            // Inject stream_options so upstream returns token usage in the final SSE chunk
            if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
                obj.insert("stream_options".to_string(), json!({"include_usage": true}));
            }
        });
        Ok(json)
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> {
        let oai_resp: ChatCompletionResponse = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid OpenAI response: {}", e)))?;
        let claude_resp = oai_response_to_claude_messages(&oai_resp);
        serde_json::to_value(&claude_resp).map_err(|e| AdapterError::Parse(e.to_string()))
    }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(AnthropicToOpenAIStream::new()))
    }
}

fn oai_response_to_claude_messages(resp: &ChatCompletionResponse) -> ClaudeMessageResponse {
    let mut content: Vec<ClaudeContentBlock> = Vec::new();
    if let Some(choice) = resp.choices.first() {
        if !choice.message.content.is_empty() {
            content.push(ClaudeContentBlock {
                content_type: "text".to_string(), text: Some(choice.message.content.clone()), source: None,
                id: None, name: None, input: None, tool_use_id: None, content: None,
            });
        }
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                content.push(ClaudeContentBlock {
                    content_type: "tool_use".to_string(), text: None, source: None,
                    id: Some(tc.id.clone()), name: Some(tc.function.name.clone()), input: Some(input),
                    tool_use_id: None, content: None,
                });
            }
        }
    }
    let stop_reason = match resp.choices.first().and_then(|c| c.finish_reason.as_deref()) {
        Some("tool_calls") => Some("tool_use".to_string()),
        Some("stop") => Some("end_turn".to_string()),
        Some("length") => Some("max_tokens".to_string()),
        Some(s) => Some(s.to_string()),
        None => None,
    };
    ClaudeMessageResponse {
        id: resp.id.clone(), response_type: "message".to_string(), role: "assistant".to_string(),
        content, model: resp.model.clone(), stop_reason, stop_sequence: None,
        usage: ClaudeUsage { input_tokens: resp.usage.prompt_tokens, output_tokens: resp.usage.completion_tokens },
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AnthropicToOpenAIStream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

enum BlockType {
    Text,
    ToolUse { id: String, name: String },
}

struct AnthropicToOpenAIStream {
    model: String,
    message_id: String,
    current_block_index: i32,
    current_block: Option<BlockType>,
    started: bool,
}

impl AnthropicToOpenAIStream {
    fn new() -> Self {
        Self { model: String::new(), message_id: format!("msg_{}", uuid::Uuid::new_v4()),
               current_block_index: 0, current_block: None, started: false }
    }

    fn emit_event(&self, event: &ClaudeStreamEvent) -> Option<Vec<u8>> {
        let json = serde_json::to_string(event).ok()?;
        Some(format!("event: {}\ndata: {}\n\n", event.event_type, json).into_bytes())
    }
}

impl StreamAdapter for AnthropicToOpenAIStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        let text = String::from_utf8_lossy(chunk);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') { continue; }
            let data = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")).unwrap_or(line);
            if data == "[DONE]" { return None; }
            let chunk: ChatCompletionChunk = serde_json::from_str(data).ok()?;

            if !self.started && !chunk.model.is_empty() { self.model = chunk.model.clone(); }

            for choice in &chunk.choices {
                if !self.started {
                    self.started = true;
                    return self.emit_event(&ClaudeStreamEvent {
                        event_type: "message_start".to_string(), index: None, delta: None, content_block: None,
                        message: Some(ClaudeMessageResponse {
                            id: self.message_id.clone(), response_type: "message".to_string(), role: "assistant".to_string(),
                            content: vec![], model: self.model.clone(), stop_reason: None, stop_sequence: None,
                            usage: ClaudeUsage { input_tokens: 0, output_tokens: 0 },
                        }), usage: None,
                    });
                }

                if let Some(ref text) = choice.delta.content {
                    if !text.is_empty() {
                        let needs_new_block = !matches!(&self.current_block, Some(BlockType::Text));
                        if needs_new_block {
                            self.current_block = Some(BlockType::Text);
                            let idx = self.current_block_index; self.current_block_index += 1;
                            return self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_start".to_string(), index: Some(idx), delta: None,
                                content_block: Some(ClaudeContentBlock {
                                    content_type: "text".to_string(), text: None, source: None,
                                    id: None, name: None, input: None, tool_use_id: None, content: None,
                                }), message: None, usage: None,
                            });
                        }
                        return self.emit_event(&ClaudeStreamEvent {
                            event_type: "content_block_delta".to_string(), index: Some(self.current_block_index - 1),
                            delta: Some(ClaudeDelta { delta_type: "text_delta".to_string(), text: Some(text.clone()) }),
                            content_block: None, message: None, usage: None,
                        });
                    }
                }

                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        if let Some(ref id) = tc.id {
                            if !id.is_empty() {
                                let tc_name = tc.function.name.clone().unwrap_or_default();
                                self.current_block = Some(BlockType::ToolUse { id: id.clone(), name: tc_name.clone() });
                                let idx = self.current_block_index; self.current_block_index += 1;
                                return self.emit_event(&ClaudeStreamEvent {
                                    event_type: "content_block_start".to_string(), index: Some(idx), delta: None,
                                    content_block: Some(ClaudeContentBlock {
                                        content_type: "tool_use".to_string(), text: None, source: None,
                                        id: Some(id.clone()), name: Some(tc_name), input: Some(json!({})),
                                        tool_use_id: None, content: None,
                                    }), message: None, usage: None,
                                });
                            }
                        }
                        if !tc.function.arguments.is_empty() {
                            return self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_delta".to_string(), index: Some(self.current_block_index - 1),
                                delta: Some(ClaudeDelta { delta_type: "input_json_delta".to_string(), text: Some(tc.function.arguments.clone()) }),
                                content_block: None, message: None, usage: None,
                            });
                        }
                    }
                }

                if let Some(ref finish) = choice.finish_reason {
                    let sr = match finish.as_str() {
                        "tool_calls" => Some("tool_use".to_string()),
                        "stop" => Some("end_turn".to_string()),
                        "length" => Some("max_tokens".to_string()),
                        s => Some(s.to_string()),
                    };
                    return self.emit_event(&ClaudeStreamEvent {
                        event_type: "message_delta".to_string(), index: None,
                        delta: Some(ClaudeDelta { delta_type: "stop_reason".to_string(), text: sr }),
                        content_block: None, message: None, usage: None,
                    });
                }
            }
        }
        None
    }

    fn finish(&mut self) -> Option<Vec<u8>> {
        self.emit_event(&ClaudeStreamEvent {
            event_type: "message_stop".to_string(), index: None, delta: None,
            content_block: None, message: None, usage: None,
        })
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// DefaultAdapter (legacy)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct DefaultAdapter;

impl ProviderAdapter for DefaultAdapter {
    fn openai_to_claude_request(req: &ChatCompletionRequest, max_tokens: i32) -> ClaudeMessageRequest {
        let system = extract_openai_system(&req.messages);
        let messages: Vec<ClaudeMessage> = req.messages.iter()
            .filter(|m| m.role != "system").map(openai_message_to_claude).collect();
        ClaudeMessageRequest {
            model: req.model.clone(), messages, max_tokens,
            stream: if req.stream { Some(true) } else { None },
            system: system.map(ClaudeSystemMessage::Text), temperature: req.temperature,
            top_p: req.top_p, top_k: None, stop_sequences: req.stop.clone(), metadata: None,
            tools: None, tool_choice: None,
        }
    }

    fn claude_to_openai_response(resp: &ClaudeMessageResponse, model: &str) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: resp.id.clone(), object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(), model: model.to_string(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage { role: "assistant".to_string(), content: claude_content_to_text(&resp.content), tool_calls: None },
                finish_reason: claude_stop_to_openai(&resp.stop_reason),
            }],
            usage: Usage { prompt_tokens: resp.usage.input_tokens, completion_tokens: resp.usage.output_tokens,
                           total_tokens: resp.usage.input_tokens + resp.usage.output_tokens },
        }
    }

    fn claude_to_openai_request(req: &ClaudeMessageRequest) -> ChatCompletionRequest {
        let mut messages: Vec<ChatMessage> = Vec::new();
        if let Some(ref sys) = req.system {
            match sys {
                ClaudeSystemMessage::Text(t) => messages.push(ChatMessage {
                    role: "system".to_string(), content: ChatContent::Text(t.clone()),
                    name: None, tool_calls: None, tool_call_id: None }),
                ClaudeSystemMessage::Blocks(blocks) => {
                    let text = claude_blocks_to_text(blocks);
                    if !text.is_empty() {
                        messages.push(ChatMessage {
                            role: "system".to_string(), content: ChatContent::Text(text),
                            name: None, tool_calls: None, tool_call_id: None });
                    }
                }
            }
        }
        for msg in &req.messages { messages.push(claude_message_to_openai(msg)); }

        // Map Claude tools → OpenAI tools
        let tools = req.tools.as_ref().map(|claude_tools| {
            claude_tools.iter().map(|ct| {
                crate::models::ToolDef {
                    tool_type: "function".to_string(),
                    function: crate::models::ToolDefFunction {
                        name: ct.name.clone(),
                        description: ct.description.clone(),
                        parameters: Some(ct.input_schema.clone()),
                    },
                }
            }).collect()
        });

        ChatCompletionRequest {
            model: req.model.clone(), messages, stream: req.stream.unwrap_or(false),
            temperature: req.temperature, max_tokens: Some(req.max_tokens), top_p: req.top_p,
            frequency_penalty: None, presence_penalty: None, stop: req.stop_sequences.clone(), user: None,
            tools,
            tool_choice: req.tool_choice.clone(),
        }
    }

    fn openai_to_claude_response(resp: &ChatCompletionResponse) -> ClaudeMessageResponse {
        let content_text = resp.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
        ClaudeMessageResponse {
            id: resp.id.clone(), response_type: "message".to_string(), role: "assistant".to_string(),
            content: vec![ClaudeContentBlock {
                content_type: "text".to_string(), text: Some(content_text), source: None,
                id: None, name: None, input: None, tool_use_id: None, content: None,
            }],
            model: resp.model.clone(),
            stop_reason: openai_stop_to_claude(&resp.choices.first().and_then(|c| c.finish_reason.clone())),
            stop_sequence: None,
            usage: ClaudeUsage { input_tokens: resp.usage.prompt_tokens, output_tokens: resp.usage.completion_tokens },
        }
    }

    fn claude_stream_to_openai_chunk(event: &ClaudeStreamEvent, model: &str, request_id: &str) -> Option<ChatCompletionChunk> {
        let now = chrono::Utc::now().timestamp();
        match event.event_type.as_str() {
            "content_block_delta" => {
                let delta = event.delta.as_ref()?;
                if delta.delta_type != "text_delta" { return None; }
                Some(ChatCompletionChunk {
                    id: request_id.to_string(), object: "chat.completion.chunk".to_string(),
                    created: now, model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: event.index.unwrap_or(0),
                        delta: Delta { role: None, content: delta.text.clone(), tool_calls: None },
                        finish_reason: None,
                    }],
                })
            }
            "message_start" => Some(ChatCompletionChunk {
                id: request_id.to_string(), object: "chat.completion.chunk".to_string(),
                created: now, model: model.to_string(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta { role: Some("assistant".to_string()), content: None, tool_calls: None },
                    finish_reason: None,
                }],
            }),
            "message_delta" => {
                let stop_reason = event.delta.as_ref().and_then(|d| if d.delta_type == "stop_reason" { d.text.clone() } else { None });
                claude_stop_to_openai(&stop_reason).map(|fr| ChatCompletionChunk {
                    id: request_id.to_string(), object: "chat.completion.chunk".to_string(),
                    created: now, model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta { role: None, content: None, tool_calls: None },
                        finish_reason: Some(fr),
                    }],
                })
            }
            _ => None,
        }
    }

    fn openai_chunk_to_claude_stream(chunk: &ChatCompletionChunk) -> Option<ClaudeStreamEvent> {
        for choice in &chunk.choices {
            if choice.delta.role.is_some() {
                return Some(ClaudeStreamEvent {
                    event_type: "message_start".to_string(), index: Some(choice.index), delta: None, content_block: None,
                    message: Some(ClaudeMessageResponse {
                        id: chunk.id.clone(), response_type: "message".to_string(), role: "assistant".to_string(),
                        content: vec![], model: chunk.model.clone(), stop_reason: None, stop_sequence: None,
                        usage: ClaudeUsage { input_tokens: 0, output_tokens: 0 },
                    }), usage: None,
                });
            }
            if let Some(ref text) = choice.delta.content {
                return Some(ClaudeStreamEvent {
                    event_type: "content_block_delta".to_string(), index: Some(choice.index),
                    delta: Some(ClaudeDelta { delta_type: "text_delta".to_string(), text: Some(text.clone()) }),
                    content_block: None, message: None, usage: None,
                });
            }
            if let Some(ref finish) = choice.finish_reason {
                let stop_reason = openai_stop_to_claude(&Some(finish.clone()));
                return Some(ClaudeStreamEvent {
                    event_type: "message_delta".to_string(), index: Some(choice.index),
                    delta: Some(ClaudeDelta { delta_type: "stop_reason".to_string(), text: stop_reason }),
                    content_block: None, message: None, usage: None,
                });
            }
        }
        None
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Helpers
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn extract_openai_system(messages: &[ChatMessage]) -> Option<String> {
    let result: String = messages.iter().filter(|m| m.role == "system")
        .map(|m| chat_content_to_string(&m.content)).collect::<Vec<_>>().join("\n");
    if result.is_empty() { None } else { Some(result) }
}

fn chat_content_to_string(content: &ChatContent) -> String {
    match content {
        ChatContent::Text(t) => t.clone(),
        ChatContent::Parts(parts) => parts.iter().filter_map(|p| p.text.clone()).collect::<Vec<_>>().join(""),
    }
}

fn openai_message_to_claude(msg: &ChatMessage) -> ClaudeMessage {
    match &msg.content {
        ChatContent::Text(t) => ClaudeMessage { role: msg.role.clone(), content: ClaudeContent::Text(t.clone()) },
        ChatContent::Parts(parts) => {
            let blocks: Vec<ClaudeContentBlock> = parts.iter().map(|p| {
                if let Some(ref image_url) = p.image_url {
                    ClaudeContentBlock {
                        content_type: "image".to_string(), text: None,
                        source: Some(ClaudeImageSource { source_type: "base64".to_string(), media_type: "image/jpeg".to_string(), data: image_url.url.clone() }),
                        id: None, name: None, input: None, tool_use_id: None, content: None,
                    }
                } else {
                    ClaudeContentBlock {
                        content_type: "text".to_string(), text: p.text.clone(), source: None,
                        id: None, name: None, input: None, tool_use_id: None, content: None,
                    }
                }
            }).collect();
            ClaudeMessage { role: msg.role.clone(), content: ClaudeContent::Blocks(blocks) }
        }
    }
}

fn claude_content_to_text(blocks: &[ClaudeContentBlock]) -> String {
    blocks.iter().filter(|b| b.content_type == "text").filter_map(|b| b.text.clone()).collect::<Vec<_>>().join("")
}

fn claude_blocks_to_text(blocks: &[ClaudeContentBlock]) -> String {
    blocks.iter().filter(|b| b.content_type == "text").filter_map(|b| b.text.clone()).collect::<Vec<_>>().join("")
}

fn claude_message_to_openai(msg: &ClaudeMessage) -> ChatMessage {
    match &msg.content {
        ClaudeContent::Text(t) => ChatMessage {
            role: msg.role.clone(), content: ChatContent::Text(t.clone()),
            name: None, tool_calls: None, tool_call_id: None,
        },
        ClaudeContent::Blocks(blocks) => {
            let parts: Vec<ContentPart> = blocks.iter().map(|b| {
                if b.content_type == "image" {
                    ContentPart {
                        content_type: "image_url".to_string(), text: None,
                        image_url: b.source.as_ref().map(|s| ImageUrl { url: format!("data:{};base64,{}", s.media_type, s.data) }),
                    }
                } else {
                    ContentPart { content_type: "text".to_string(), text: b.text.clone(), image_url: None }
                }
            }).collect();
            let tool_calls: Vec<ToolCall> = blocks.iter()
                .filter(|b| b.content_type == "tool_use")
                .filter_map(|b| {
                    let id = b.id.clone()?;
                    let name = b.name.clone()?;
                    let input = b.input.clone().unwrap_or(json!({}));
                    Some(ToolCall { id, call_type: "function".to_string(), function: ToolCallFunction { name, arguments: input.to_string() } })
                }).collect();
            let tc = if tool_calls.is_empty() { None } else { Some(tool_calls) };
            let tool_results: Vec<(String, String)> = blocks.iter()
                .filter(|b| b.content_type == "tool_result")
                .filter_map(|b| {
                    let tui = b.tool_use_id.clone()?;
                    let c = b.content.as_ref().and_then(|v| v.as_str().map(String::from)).or_else(|| b.text.clone()).unwrap_or_default();
                    Some((tui, c))
                }).collect();
            if !tool_results.is_empty() && msg.role == "user" {
                let (tool_use_id, content) = &tool_results[0];
                ChatMessage { role: "tool".to_string(), content: ChatContent::Text(content.clone()), name: None, tool_calls: None, tool_call_id: Some(tool_use_id.clone()) }
            } else {
                ChatMessage { role: msg.role.clone(), content: ChatContent::Parts(parts), name: None, tool_calls: tc, tool_call_id: None }
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

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatMessage, ChatContent};

    fn make_openai_req(text: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(), content: ChatContent::Text(text.to_string()),
                name: None, tool_calls: None, tool_call_id: None,
            }],
            stream: false, temperature: Some(0.7), max_tokens: Some(1024),
            top_p: None, frequency_penalty: None, presence_penalty: None, stop: None, user: None,
            tools: None, tool_choice: None,
        }
    }

    fn test_deployment() -> Deployment {
        Deployment {
            api_base: "https://api.openai.com/v1".into(), api_key: None,
            upstream_model: "gpt-4".into(), provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None, output_cost_per_token: None, raw_params: json!({"custom_llm_provider": "openai"}),
        }
    }

    // ── MessageAdapter tests ──

    #[test]
    fn test_openai_passthrough_swaps_model() {
        let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]});
        let adapted = OpenAIPassthrough.adapt_request(body, &test_deployment()).unwrap();
        assert_eq!(adapted["model"].as_str(), Some("gpt-4"));
    }

    #[test]
    fn test_openai_passthrough_response_unchanged() {
        let resp = json!({"choices": [{"message": {"role": "assistant", "content": "Hi!"}}]});
        let adapted = OpenAIPassthrough.adapt_response(resp.clone()).unwrap();
        assert_eq!(adapted, resp);
    }

    #[test]
    fn test_select_adapter_openai_passthrough() {
        let a = select_adapter(ClientProtocol::OpenAI, &ProviderType::OpenAICompatible).unwrap();
        assert_eq!(a.client_protocol(), ClientProtocol::OpenAI);
    }

    #[test]
    fn test_select_adapter_anthropic_to_openai() {
        let a = select_adapter(ClientProtocol::Anthropic, &ProviderType::OpenAICompatible).unwrap();
        assert_eq!(a.client_protocol(), ClientProtocol::Anthropic);
    }

    #[test]
    fn test_select_adapter_unsupported() {
        assert!(select_adapter(ClientProtocol::OpenAI, &ProviderType::AnthropicNative).is_none());
        assert!(select_adapter(ClientProtocol::Anthropic, &ProviderType::AnthropicNative).is_none());
    }

    // ── Tool conversion tests ──

    #[test]
    fn test_anthropic_to_openai_tool_use_to_tool_calls() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_01", "name": "get_weather", "input": {"city": "NYC"}}]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        let assistant = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
        let tc = assistant["tool_calls"].as_array().unwrap();
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0]["id"].as_str(), Some("toolu_01"));
        assert_eq!(tc[0]["function"]["name"].as_str(), Some("get_weather"));
    }

    #[test]
    fn test_anthropic_to_openai_tool_result_to_tool_role() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_01", "content": "72F, sunny"}]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[0]["tool_call_id"].as_str(), Some("toolu_01"));
    }

    #[test]
    fn test_anthropic_to_openai_response_with_tool_calls() {
        let resp = json!({
            "id": "chatcmpl-001", "object": "chat.completion", "created": 1, "model": "gpt-4",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "",
                "tool_calls": [{"id": "call_001", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\": \"NYC\"}"}}]},
                "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
        });
        let adapted = AnthropicToOpenAI.adapt_response(resp).unwrap();
        let content = adapted["content"].as_array().unwrap();
        let tool_use = content.iter().find(|b| b["type"] == "tool_use").unwrap();
        assert_eq!(tool_use["id"].as_str(), Some("call_001"));
        assert_eq!(tool_use["name"].as_str(), Some("get_weather"));
        assert_eq!(tool_use["input"]["city"].as_str(), Some("NYC"));
        assert_eq!(adapted["stop_reason"].as_str(), Some("tool_use"));
    }

    #[test]
    fn test_stream_adapter_text_delta() {
        let mut stream = AnthropicToOpenAIStream::new();
        let result = stream.next(b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"g\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}");
        assert!(result.is_some());
    }

    #[test]
    fn test_stream_adapter_finish_reason() {
        let mut stream = AnthropicToOpenAIStream::new();
        stream.next(b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"g\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}");
        let result = stream.next(b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"g\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}");
        assert!(result.is_some());
    }

    #[test]
    fn test_passthrough_stream() {
        let mut stream = PassthroughStream;
        assert_eq!(stream.next(b"test").unwrap(), b"test");
    }

    // ── Legacy tests ──

    #[test]
    fn test_openai_to_claude_request_basic() {
        let req = make_openai_req("Hello");
        let c = DefaultAdapter::openai_to_claude_request(&req, 1024);
        assert_eq!(c.model, "gpt-4");
    }

    #[test]
    fn test_openai_to_claude_request_with_system() {
        let mut req = make_openai_req("Hi");
        req.messages.insert(0, ChatMessage { role: "system".to_string(), content: ChatContent::Text("Helpful".to_string()), name: None, tool_calls: None, tool_call_id: None });
        let c = DefaultAdapter::openai_to_claude_request(&req, 512);
        assert!(c.system.is_some());
    }

    #[test]
    fn test_claude_to_openai_response() {
        let cr = ClaudeMessageResponse {
            id: "1".into(), response_type: "message".into(), role: "assistant".into(),
            content: vec![ClaudeContentBlock { content_type: "text".into(), text: Some("Hi".into()), source: None, id: None, name: None, input: None, tool_use_id: None, content: None }],
            model: "claude-sonnet".into(), stop_reason: Some("end_turn".into()), stop_sequence: None,
            usage: ClaudeUsage { input_tokens: 1, output_tokens: 1 },
        };
        let oai = DefaultAdapter::claude_to_openai_response(&cr, "claude");
        assert_eq!(oai.choices[0].message.content, "Hi");
    }

    #[test]
    fn test_roundtrip_openai_claude_openai() {
        let orig = make_openai_req("Hello world!");
        let claude = DefaultAdapter::openai_to_claude_request(&orig, 512);
        let rt = DefaultAdapter::claude_to_openai_request(&claude);
        assert_eq!(rt.model, orig.model);
    }
}
