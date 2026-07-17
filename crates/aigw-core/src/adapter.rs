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
        (ClientProtocol::Anthropic, ProviderType::AnthropicNative) => Some(&AnthropicPassthrough),
        (ClientProtocol::OpenAI, ProviderType::AnthropicNative) => Some(&OpenAIToAnthropic),
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

        // Stage 60: System message normalization for strict chat templates
        let compat = resolve_chat_template_compat(deployment);
        let oai_req = match compat {
            ChatTemplateCompat::Strict => {
                let messages = fold_extra_systems_into_adjacent_user(oai_req.messages);
                ChatCompletionRequest { messages, ..oai_req }
            }
            ChatTemplateCompat::Loose => oai_req,
            ChatTemplateCompat::Auto => {
                // Already resolved to Strict or Loose by resolve_chat_template_compat;
                // this arm is unreachable but kept for exhaustiveness.
                oai_req
            }
        };

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
// System Message Normalization (Stage 60)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Chat template compatibility mode for Anthropic→OpenAI conversion.
///
/// Some upstream models (e.g. Qwen series) enforce that `role="system"` messages
/// can only appear at index 0. Claude Code clients may inject extra system
/// messages into the `messages` array, which causes 400 errors on strict templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatTemplateCompat {
    /// Auto-detect: sniff by upstream model name (default behavior)
    Auto,
    /// Fold extra system messages into adjacent user turns with `<system-reminder>` wrapper
    Strict,
    /// Pass through all messages unchanged
    Loose,
}

/// Resolve the effective [`ChatTemplateCompat`] mode from a deployment.
///
/// Decision chain:
///   1. Explicit `chat_template_compat` field: "strict" → Strict, "loose" → Loose
///   2. Unknown value → warn + fall through to auto sniff
///   3. Auto sniff: upstream_model name contains "qwen" (case-insensitive) → Strict, else Loose
pub fn resolve_chat_template_compat(deployment: &Deployment) -> ChatTemplateCompat {
    match deployment.chat_template_compat.as_deref() {
        Some("strict") => return ChatTemplateCompat::Strict,
        Some("loose") => return ChatTemplateCompat::Loose,
        Some(other) => {
            tracing::warn!(%other, "unknown chat_template_compat value, falling back to auto sniff");
        }
        _ => {}
    }
    // Auto sniff: check if upstream model name contains "qwen" (case-insensitive)
    if deployment.upstream_model.to_lowercase().contains("qwen") {
        ChatTemplateCompat::Strict
    } else {
        ChatTemplateCompat::Loose
    }
}

/// Fold extra system messages (index > 0) into adjacent user turns.
///
/// The folding wraps each extra system's content in `<system-reminder>...</system-reminder>`
/// tags and prepends it to the next user message's content. Pending reminders that
/// have no following user message are appended to the last user message, or a new
/// user message is created as a fallback.
///
/// Post-condition: `role="system"` only appears at index 0 (if at all).
pub fn fold_extra_systems_into_adjacent_user(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut out = Vec::with_capacity(messages.len());
    let mut pending_reminders: Vec<String> = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        if i == 0 && msg.role == "system" {
            out.push(msg.clone());
            continue;
        }
        if msg.role == "system" {
            let text = flatten_chat_content_to_text(&msg.content);
            let wrapped = format!("<system-reminder>\n{}\n</system-reminder>", text);
            pending_reminders.push(wrapped);
            continue;
        }
        if msg.role == "user" && !pending_reminders.is_empty() {
            let reminders: Vec<String> = pending_reminders.drain(..).collect();
            out.push(prepend_text_to_chat_message(msg, &reminders));
        } else {
            out.push(msg.clone());
        }
    }

    // Flush remaining reminders
    if !pending_reminders.is_empty() {
        let text = pending_reminders.join("\n\n");
        if let Some(last_user_idx) = out.iter().rposition(|m| m.role == "user") {
            let last_user = &out[last_user_idx];
            out[last_user_idx] = append_text_to_chat_message(last_user, &text);
        } else {
            out.push(ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text(text),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    // Post-condition: only index 0 can be system
    debug_assert!(
        out.iter().enumerate().all(|(i, m)| i == 0 || m.role != "system"),
        "fold_extra_systems_into_adjacent_user: system message found beyond index 0"
    );

    out
}

/// Extract a plain text representation from a [`ChatContent`].
fn flatten_chat_content_to_text(content: &ChatContent) -> String {
    match content {
        ChatContent::Text(t) => t.clone(),
        ChatContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<&str>>()
            .join(""),
    }
}

/// Prepend reminder texts to a user ChatMessage's content.
fn prepend_text_to_chat_message(msg: &ChatMessage, reminders: &[String]) -> ChatMessage {
    let reminder_text = reminders.join("\n\n");
    let new_content = match &msg.content {
        ChatContent::Text(t) => {
            ChatContent::Text(format!("{}\n\n{}", reminder_text, t))
        }
        ChatContent::Parts(parts) => {
            let mut new_parts = vec![ContentPart {
                content_type: "text".to_string(),
                text: Some(reminder_text),
                image_url: None,
            }];
            new_parts.extend(parts.clone());
            ChatContent::Parts(new_parts)
        }
    };
    ChatMessage {
        role: msg.role.clone(),
        content: new_content,
        name: msg.name.clone(),
        tool_calls: msg.tool_calls.clone(),
        tool_call_id: msg.tool_call_id.clone(),
    }
}

/// Append text to a user ChatMessage's content (for tail system reminders).
fn append_text_to_chat_message(msg: &ChatMessage, text: &str) -> ChatMessage {
    let new_content = match &msg.content {
        ChatContent::Text(t) => {
            ChatContent::Text(format!("{}\n\n{}", t, text))
        }
        ChatContent::Parts(parts) => {
            let mut new_parts = parts.clone();
            new_parts.push(ContentPart {
                content_type: "text".to_string(),
                text: Some(text.to_string()),
                image_url: None,
            });
            ChatContent::Parts(new_parts)
        }
    };
    ChatMessage {
        role: msg.role.clone(),
        content: new_content,
        name: msg.name.clone(),
        tool_calls: msg.tool_calls.clone(),
        tool_call_id: msg.tool_call_id.clone(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AnthropicToOpenAIStream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

enum BlockType {
    Text,
    #[allow(dead_code)]
    ToolUse { id: String, name: String },
}

pub struct AnthropicToOpenAIStream {
    model: String,
    message_id: String,
    current_block_index: i32,
    current_block: Option<BlockType>,
    started: bool,
}

impl AnthropicToOpenAIStream {
    pub fn new() -> Self {
        Self { model: String::new(), message_id: format!("msg_{}", uuid::Uuid::new_v4()),
               current_block_index: 0, current_block: None, started: false }
    }

    fn emit_event(&self, event: &ClaudeStreamEvent) -> Option<Vec<u8>> {
        let json = serde_json::to_string(event).ok()?;
        Some(format!("event: {}\ndata: {}\n\n", event.event_type, json).into_bytes())
    }

    /// Build a single SSE frame with content_block_stop followed by message_stop.
    /// Returns `None` if already finished (idempotent).
    fn build_finish_events(&mut self) -> Option<Vec<u8>> {
        if self.current_block.is_none() && self.current_block_index == -1 {
            return None; // already finished, idempotent
        }
        let mut buf = Vec::new();
        if self.current_block.is_some() {
            if let Some(cbs) = self.emit_event(&ClaudeStreamEvent {
                event_type: "content_block_stop".to_string(), index: Some(self.current_block_index - 1),
                delta: None, content_block: None, message: None, usage: None,
            }) {
                buf.extend_from_slice(&cbs);
            }
            self.current_block = None;
        }
        if let Some(ms) = self.emit_event(&ClaudeStreamEvent {
            event_type: "message_stop".to_string(), index: None, delta: None,
            content_block: None, message: None, usage: None,
        }) {
            buf.extend_from_slice(&ms);
        }
        self.current_block_index = -1; // mark as finished
        if buf.is_empty() { None } else { Some(buf) }
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

                let has_tool_calls = choice.delta.tool_calls.as_ref()
                    .map(|tc| tc.iter().any(|t| t.id.as_ref().map(|id| !id.is_empty()).unwrap_or(false)))
                    .unwrap_or(false);

                if !has_tool_calls {
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
                                delta: Some(ClaudeDelta { delta_type: "text_delta".to_string(), text: Some(text.clone()), partial_json: None }),
                                content_block: None, message: None, usage: None,
                            });
                        }
                    }
                }

                // Process tool_calls BEFORE text content — DeepSeek thinking models
                // emit reasoning_content (text) and tool_calls in the same chunk;
                // tool_calls must take priority to create the correct block type.
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
                                delta: Some(ClaudeDelta { delta_type: "input_json_delta".to_string(), text: None, partial_json: Some(tc.function.arguments.clone()) }),
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
                        delta: Some(ClaudeDelta { delta_type: "stop_reason".to_string(), text: sr, partial_json: None }),
                        content_block: None, message: None, usage: None,
                    });
                }
            }
        }
        None
    }

    fn finish(&mut self) -> Option<Vec<u8>> {
        self.build_finish_events()
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
        for msg in &req.messages { messages.extend(claude_message_to_openai(msg)); }

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

        // Convert Claude tool_choice → OpenAI tool_choice:
        //   {"type":"auto"}  → "auto"
        //   {"type":"any"}   → "required"
        //   {"type":"tool","name":"x"} → {"type":"function","function":{"name":"x"}}
        //   strings passthrough, null/absent passthrough
        let tool_choice = req.tool_choice.as_ref().map(|tc| {
            match tc.get("type").and_then(|v| v.as_str()) {
                Some("auto") => json!("auto"),
                Some("any") => json!("required"),
                Some("tool") => {
                    let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    json!({"type": "function", "function": {"name": name}})
                }
                _ => {
                    // Already a string? Passthrough (e.g. "auto", "none")
                    if tc.is_string() { tc.clone() } else { json!("auto") }
                }
            }
        });

        ChatCompletionRequest {
            model: req.model.clone(), messages, stream: req.stream.unwrap_or(false),
            temperature: req.temperature, max_tokens: Some(req.max_tokens), top_p: req.top_p,
            frequency_penalty: None, presence_penalty: None, stop: req.stop_sequences.clone(), user: None,
            tools,
            tool_choice,
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
                    delta: Some(ClaudeDelta { delta_type: "text_delta".to_string(), text: Some(text.clone()), partial_json: None }),
                    content_block: None, message: None, usage: None,
                });
            }
            if let Some(ref finish) = choice.finish_reason {
                let stop_reason = openai_stop_to_claude(&Some(finish.clone()));
                return Some(ClaudeStreamEvent {
                    event_type: "message_delta".to_string(), index: Some(choice.index),
                    delta: Some(ClaudeDelta { delta_type: "stop_reason".to_string(), text: stop_reason, partial_json: None }),
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
    let mut blocks: Vec<ClaudeContentBlock> = Vec::new();

    // 1. Content blocks
    match &msg.content {
        ChatContent::Text(t) => {
            if !t.is_empty() {
                blocks.push(ClaudeContentBlock {
                    content_type: "text".to_string(), text: Some(t.clone()), source: None,
                    id: None, name: None, input: None, tool_use_id: None, content: None,
                });
            }
        }
        ChatContent::Parts(parts) => {
            blocks.extend(parts.iter().map(|p| {
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
            }));
        }
    }

    // 2. Tool call blocks (OpenAI tool_calls → Claude tool_use)
    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
            blocks.push(ClaudeContentBlock {
                content_type: "tool_use".to_string(), text: None, source: None,
                id: Some(tc.id.clone()), name: Some(tc.function.name.clone()),
                input: Some(input), tool_use_id: None, content: None,
            });
        }
    }

    if blocks.is_empty() {
        ClaudeMessage { role: msg.role.clone(), content: ClaudeContent::Text(String::new()) }
    } else {
        ClaudeMessage { role: msg.role.clone(), content: ClaudeContent::Blocks(blocks) }
    }
}

fn claude_content_to_text(blocks: &[ClaudeContentBlock]) -> String {
    blocks.iter().filter(|b| b.content_type == "text").filter_map(|b| b.text.clone()).collect::<Vec<_>>().join("")
}

fn claude_blocks_to_text(blocks: &[ClaudeContentBlock]) -> String {
    blocks.iter().filter(|b| b.content_type == "text").filter_map(|b| b.text.clone()).collect::<Vec<_>>().join("")
}

fn claude_message_to_openai(msg: &ClaudeMessage) -> Vec<ChatMessage> {
    match &msg.content {
        ClaudeContent::Text(t) => vec![ChatMessage {
            role: msg.role.clone(), content: ChatContent::Text(t.clone()),
            name: None, tool_calls: None, tool_call_id: None,
        }],
        ClaudeContent::Blocks(blocks) => {
            let tool_results: Vec<(String, String)> = blocks.iter()
                .filter(|b| b.content_type == "tool_result")
                .filter_map(|b| {
                    let tui = b.tool_use_id.clone()?;
                    let c = b.content.as_ref()
                        .and_then(|v| v.as_str().map(String::from))
                        .or_else(|| b.text.clone())
                        .unwrap_or_default();
                    Some((tui, c))
                }).collect();

            if !tool_results.is_empty() && msg.role == "user" {
                let mut out = Vec::new();

                // Non-tool_result content parts (text/image) → user message
                let non_tool_parts: Vec<ContentPart> = blocks.iter()
                    .filter(|b| b.content_type != "tool_result")
                    .filter(|b| b.content_type == "text" || b.content_type == "image")
                    .map(|b| {
                        if b.content_type == "image" {
                            ContentPart {
                                content_type: "image_url".to_string(), text: None,
                                image_url: b.source.as_ref().map(|s| ImageUrl { url: format!("data:{};base64,{}", s.media_type, s.data) }),
                            }
                        } else {
                            ContentPart { content_type: "text".to_string(), text: b.text.clone(), image_url: None }
                        }
                    }).collect();
                if !non_tool_parts.is_empty() {
                    out.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Parts(non_tool_parts),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }

                // Each tool_result → one tool message
                for (tool_use_id, content) in &tool_results {
                    out.push(ChatMessage {
                        role: "tool".to_string(),
                        content: ChatContent::Text(content.clone()),
                        name: None,
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id.clone()),
                    });
                }
                out
            } else {
                // Only emit ContentParts for text/image blocks.
                // tool_use and tool_result are handled separately above
                // (tool_calls / tool_call_id); including them would produce
                // ContentPart { type:"text", text:None } which upstream
                // rejects as "missing field `text`".
                let parts: Vec<ContentPart> = blocks.iter()
                    .filter(|b| b.content_type == "text" || b.content_type == "image")
                    .map(|b| {
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
                vec![ChatMessage { role: msg.role.clone(), content: ChatContent::Parts(parts), name: None, tool_calls: tc, tool_call_id: None }]
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
// AnthropicPassthrough (Stage 61)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Client (Anthropic) → AnthropicPassthrough → Upstream (Anthropic Native)
// Body passthrough with system message folding for strict templates.

/// Normalize Anthropic messages for strict chat templates.
///
/// Some Anthropic clients (including Claude Code) inject extra system-level
/// context into the `messages` array on subsequent turns. Two forms observed:
///   1. `role="system"` messages with plain text content
///   2. `role="user"` messages with `<system-reminder>...</system-reminder>` blocks
///
/// Downstream Anthropic→OpenAI converters may extract both as separate
/// `role="system"` messages, which strict ChatML/Jinja templates (qwen) reject
/// when they appear at non-zero indices.
///
/// This function:
///   - Removes all `role="system"` messages from the array, extracting their
///     text into `extra_systems`
///   - Strips `<system-reminder>` blocks from user messages, extracting their
///     inner text into `extra_systems`
///   - Returns cleaned messages (only user/assistant) and the collected texts
///     to be merged into the top-level `system` field.
pub fn extract_and_merge_system_reminders(
    messages: Vec<ClaudeMessage>,
) -> (Vec<ClaudeMessage>, Vec<String>) {
    let mut out: Vec<ClaudeMessage> = Vec::with_capacity(messages.len());
    let mut extra_systems: Vec<String> = Vec::new();

    for msg in messages {
        // ── role="system" messages: extract text, merge to top-level ──
        if msg.role == "system" {
            let text = match &msg.content {
                ClaudeContent::Text(t) => t.clone(),
                ClaudeContent::Blocks(blocks) => claude_blocks_to_text(blocks),
            };
            if !text.is_empty() {
                extra_systems.push(text);
            }
            continue;
        }

        // ── role="user" messages: strip <system-reminder> blocks ──
        if msg.role != "user" {
            out.push(msg);
            continue;
        }

        let mut cleaned_blocks: Vec<ClaudeContentBlock> = Vec::new();
        let mut had_reminders = false;

        let blocks = match msg.content {
            ClaudeContent::Text(t) => vec![ClaudeContentBlock {
                content_type: "text".to_string(), text: Some(t),
                source: None, id: None, name: None, input: None,
                tool_use_id: None, content: None,
            }],
            ClaudeContent::Blocks(b) => b,
        };

        for block in blocks {
            if block.content_type == "text" {
                if let Some(ref t) = block.text {
                    if let Some(inner) = strip_system_reminder(t) {
                        extra_systems.push(inner);
                        had_reminders = true;
                        continue;
                    }
                }
            }
            cleaned_blocks.push(block);
        }

        if had_reminders && cleaned_blocks.is_empty() {
            continue; // drop empty user message
        }

        if cleaned_blocks.len() == 1 && cleaned_blocks[0].content_type == "text" {
            out.push(ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text(cleaned_blocks[0].text.clone().unwrap_or_default()),
            });
        } else {
            out.push(ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Blocks(cleaned_blocks),
            });
        }
    }

    (out, extra_systems)
}

/// Extract inner text from a `<system-reminder>...</system-reminder>` block.
/// Returns `None` if the text is not wrapped in system-reminder tags.
fn strip_system_reminder(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.starts_with("<system-reminder>") && trimmed.ends_with("</system-reminder>") {
        let inner = &trimmed["<system-reminder>".len()..trimmed.len() - "</system-reminder>".len()];
        let inner = inner.trim();
        if inner.is_empty() {
            None
        } else {
            Some(inner.to_string())
        }
    } else {
        None
    }
}

pub struct AnthropicPassthrough;

impl MessageAdapter for AnthropicPassthrough {
    fn client_protocol(&self) -> ClientProtocol { ClientProtocol::Anthropic }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let mut req: ClaudeMessageRequest = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid Claude request: {}", e)))?;

        // Mirror AnthropicToOpenAI normalization on the Anthropic body level.
        // Strip role="system" messages and <system-reminder> blocks from messages,
        // merging them into the top-level `system` field.
        let compat = resolve_chat_template_compat(deployment);
        if compat == ChatTemplateCompat::Strict {
            let (cleaned, reminders) = extract_and_merge_system_reminders(
                std::mem::take(&mut req.messages),
            );
            req.messages = cleaned;

            if !reminders.is_empty() {
                let combined = reminders.join("\n\n");
                req.system = Some(match req.system.take() {
                    Some(ClaudeSystemMessage::Text(existing)) => {
                        ClaudeSystemMessage::Text(format!("{}\n\n{}", existing, combined))
                    }
                    Some(ClaudeSystemMessage::Blocks(mut blocks)) => {
                        blocks.extend(reminders.into_iter().map(|t| ClaudeContentBlock {
                            content_type: "text".to_string(), text: Some(t),
                            source: None, id: None, name: None, input: None,
                            tool_use_id: None, content: None,
                        }));
                        ClaudeSystemMessage::Blocks(blocks)
                    }
                    None => ClaudeSystemMessage::Text(combined),
                });
            }
        }

        req.model = deployment.upstream_model.clone();
        let is_stream = req.stream.unwrap_or(false);
        let mut json = serde_json::to_value(&req)
            .map_err(|e| AdapterError::Parse(e.to_string()))?;
        // Anthropic requires `include_usage` in body for streaming responses
        // to include usage.{input_tokens, output_tokens} in message_delta events
        if is_stream {
            json.as_object_mut().map(|obj| {
                obj.insert("stream_options".to_string(), json!({"include_usage": true}));
            });
        }
        Ok(json)
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> { Ok(body) }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(AnthropicPassthroughStream))
    }
}

/// Stream adapter: transparent passthrough of Anthropic SSE events.
struct AnthropicPassthroughStream;

impl StreamAdapter for AnthropicPassthroughStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> { Some(chunk.to_vec()) }
    fn finish(&mut self) -> Option<Vec<u8>> { None }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OpenAIToAnthropic (Stage 61)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Client (OpenAI) → OpenAIToAnthropic → Upstream (Anthropic Native)
// Bidirectional: OpenAI Chat Completions ↔ Anthropic Messages.

pub struct OpenAIToAnthropic;

impl MessageAdapter for OpenAIToAnthropic {
    fn client_protocol(&self) -> ClientProtocol { ClientProtocol::OpenAI }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let oai_req: ChatCompletionRequest = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid OpenAI request: {}", e)))?;
        let max_tokens = oai_req.max_tokens.unwrap_or(4096);
        let claude_req = DefaultAdapter::openai_to_claude_request(&oai_req, max_tokens);
        let mut json = serde_json::to_value(&claude_req)
            .map_err(|e| AdapterError::Parse(e.to_string()))?;
        json.as_object_mut().map(|obj| {
            obj.insert("model".to_string(), json!(deployment.upstream_model));
        });
        Ok(json)
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> {
        let claude_resp: ClaudeMessageResponse = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid Claude response: {}", e)))?;
        let model = claude_resp.model.clone();
        let oai_resp = DefaultAdapter::claude_to_openai_response(&claude_resp, &model);
        serde_json::to_value(&oai_resp).map_err(|e| AdapterError::Parse(e.to_string()))
    }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(OpenAIToAnthropicStream::new()))
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OpenAIToAnthropicStream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Reverse of AnthropicToOpenAIStream: OpenAI SSE chunk → Anthropic SSE event.
//
// State machine:
//   OpenAI chunk                    → Anthropic event
//   ─────────────────────────────   ──────────────────────────────
//   role="assistant" (first chunk)  → message_start
//   delta.content (text)            → content_block_start(text) + content_block_delta(text_delta)
//   delta.tool_calls[].id (new)     → content_block_start(tool_use)
//   delta.tool_calls[].function.args→ content_block_delta(input_json_delta)
//   finish_reason                   → content_block_stop + message_delta(stop_reason)
//   usage (final chunk)             → message_delta(usage)

enum O2ABlockType {
    Text,
    #[allow(dead_code)]
    ToolUse { id: String, name: String },
}

pub struct OpenAIToAnthropicStream {
    model: String,
    message_id: String,
    current_block_index: i32,
    current_block: Option<O2ABlockType>,
    started: bool,
    finished: bool,
}

impl OpenAIToAnthropicStream {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            message_id: format!("msg_{}", uuid::Uuid::new_v4()),
            current_block_index: 0,
            current_block: None,
            started: false,
            finished: false,
        }
    }

    fn emit_event(&self, event: &ClaudeStreamEvent) -> Option<Vec<u8>> {
        let json = serde_json::to_string(event).ok()?;
        Some(format!("event: {}\ndata: {}\n\n", event.event_type, json).into_bytes())
    }

    /// Emit content_block_stop for current block, followed by message_stop.
    fn build_finish_events(&mut self) -> Option<Vec<u8>> {
        if self.finished {
            return None;
        }
        self.finished = true;
        let mut buf = Vec::new();
        if self.current_block.is_some() {
            if let Some(cbs) = self.emit_event(&ClaudeStreamEvent {
                event_type: "content_block_stop".to_string(),
                index: Some(self.current_block_index.saturating_sub(1).max(0)),
                delta: None, content_block: None, message: None, usage: None,
            }) {
                buf.extend_from_slice(&cbs);
            }
            self.current_block = None;
        }
        if let Some(ms) = self.emit_event(&ClaudeStreamEvent {
            event_type: "message_stop".to_string(),
            index: None, delta: None, content_block: None, message: None, usage: None,
        }) {
            buf.extend_from_slice(&ms);
        }
        if buf.is_empty() { None } else { Some(buf) }
    }
}

impl StreamAdapter for OpenAIToAnthropicStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        if self.finished {
            return None;
        }
        let text = String::from_utf8_lossy(chunk);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') { continue; }
            let data = line.strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
                .unwrap_or(line);
            if data == "[DONE]" {
                return self.build_finish_events();
            }
            let chunk: ChatCompletionChunk = serde_json::from_str(data).ok()?;

            if !self.started && !chunk.model.is_empty() {
                self.model = chunk.model.clone();
            }

            for choice in &chunk.choices {
                if !self.started {
                    self.started = true;
                    return self.emit_event(&ClaudeStreamEvent {
                        event_type: "message_start".to_string(), index: None, delta: None,
                        content_block: None,
                        message: Some(ClaudeMessageResponse {
                            id: self.message_id.clone(),
                            response_type: "message".to_string(),
                            role: "assistant".to_string(),
                            content: vec![],
                            model: self.model.clone(),
                            stop_reason: None,
                            stop_sequence: None,
                            usage: ClaudeUsage { input_tokens: 0, output_tokens: 0 },
                        }),
                        usage: None,
                    });
                }

                // Tool calls processed BEFORE text — same reasoning as AnthropicToOpenAIStream
                // (DeepSeek thinking models emit both in the same chunk)
                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        if let Some(ref id) = tc.id {
                            if !id.is_empty() {
                                let tc_name = tc.function.name.clone().unwrap_or_default();
                                self.current_block = Some(O2ABlockType::ToolUse {
                                    id: id.clone(),
                                    name: tc_name.clone(),
                                });
                                let idx = self.current_block_index;
                                self.current_block_index += 1;
                                return self.emit_event(&ClaudeStreamEvent {
                                    event_type: "content_block_start".to_string(),
                                    index: Some(idx), delta: None,
                                    content_block: Some(ClaudeContentBlock {
                                        content_type: "tool_use".to_string(), text: None,
                                        source: None, id: Some(id.clone()),
                                        name: Some(tc_name), input: Some(json!({})),
                                        tool_use_id: None, content: None,
                                    }),
                                    message: None, usage: None,
                                });
                            }
                        }
                        if !tc.function.arguments.is_empty() {
                            return self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_delta".to_string(),
                                index: Some((self.current_block_index - 1).max(0)),
                                delta: Some(ClaudeDelta {
                                    delta_type: "input_json_delta".to_string(),
                                    text: None,
                                    partial_json: Some(tc.function.arguments.clone()),
                                }),
                                content_block: None, message: None, usage: None,
                            });
                        }
                    }
                }

                // Text content
                if let Some(ref text) = choice.delta.content {
                    if !text.is_empty() {
                        let needs_new_block = !matches!(&self.current_block, Some(O2ABlockType::Text));
                        if needs_new_block {
                            self.current_block = Some(O2ABlockType::Text);
                            let idx = self.current_block_index;
                            self.current_block_index += 1;
                            return self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_start".to_string(),
                                index: Some(idx), delta: None,
                                content_block: Some(ClaudeContentBlock {
                                    content_type: "text".to_string(), text: None, source: None,
                                    id: None, name: None, input: None,
                                    tool_use_id: None, content: None,
                                }),
                                message: None, usage: None,
                            });
                        }
                        return self.emit_event(&ClaudeStreamEvent {
                            event_type: "content_block_delta".to_string(),
                            index: Some((self.current_block_index - 1).max(0)),
                            delta: Some(ClaudeDelta {
                                delta_type: "text_delta".to_string(),
                                text: Some(text.clone()),
                                partial_json: None,
                            }),
                            content_block: None, message: None, usage: None,
                        });
                    }
                }

                // Finish reason
                if let Some(ref finish) = choice.finish_reason {
                    let sr = match finish.as_str() {
                        "tool_calls" => Some("tool_use".to_string()),
                        "stop" => Some("end_turn".to_string()),
                        "length" => Some("max_tokens".to_string()),
                        s => Some(s.to_string()),
                    };
                    return self.emit_event(&ClaudeStreamEvent {
                        event_type: "message_delta".to_string(), index: None,
                        delta: Some(ClaudeDelta {
                            delta_type: "stop_reason".to_string(),
                            text: sr,
                            partial_json: None,
                        }),
                        content_block: None, message: None, usage: None,
                    });
                }

            }
        }
        None
    }

    fn finish(&mut self) -> Option<Vec<u8>> {
        self.build_finish_events()
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
            input_cost_per_token: None, output_cost_per_token: None,
            raw_params: json!({"custom_llm_provider": "openai"}),
            model_id: Some("test-model-id".into()),
            model_group: Some("gpt-4".into()),
            custom_llm_provider: Some("openai".into()),
            chat_template_compat: None,
            fail_count: 0,
            cooldown_until: None,
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
        // With Stage 61, AnthropicNative is now supported.
        // Only truly unsupported combos return None.
        // For now there are no unsupported combos — the matrix is complete.
        assert!(select_adapter(ClientProtocol::Anthropic, &ProviderType::AnthropicNative).is_some());
        assert!(select_adapter(ClientProtocol::OpenAI, &ProviderType::AnthropicNative).is_some());
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

    // ── tool_choice conversion tests ──

    #[test]
    fn test_tool_choice_auto_conversion() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "get_weather", "input_schema": {"type": "object", "properties": {}}}],
            "tool_choice": {"type": "auto"}
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        // Claude {"type":"auto"} → OpenAI "auto"
        assert_eq!(adapted["tool_choice"].as_str(), Some("auto"));
    }

    #[test]
    fn test_tool_choice_any_conversion() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "get_weather", "input_schema": {"type": "object", "properties": {}}}],
            "tool_choice": {"type": "any"}
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        // Claude {"type":"any"} → OpenAI "required"
        assert_eq!(adapted["tool_choice"].as_str(), Some("required"));
    }

    #[test]
    fn test_tool_choice_specific_tool_conversion() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "get_weather", "input_schema": {"type": "object", "properties": {}}}],
            "tool_choice": {"type": "tool", "name": "get_weather"}
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        // Claude {"type":"tool","name":"x"} → OpenAI {"type":"function","function":{"name":"x"}}
        let tc = &adapted["tool_choice"];
        assert_eq!(tc["type"].as_str(), Some("function"));
        assert_eq!(tc["function"]["name"].as_str(), Some("get_weather"));
    }

    #[test]
    fn test_tool_choice_string_passthrough() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "get_weather", "input_schema": {"type": "object", "properties": {}}}],
            "tool_choice": "none"
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        assert_eq!(adapted["tool_choice"].as_str(), Some("none"));
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 59: Multi tool_result tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    #[test]
    fn test_stage59_single_tool_result_regression() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_01", "content": "output1"}
            ]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // Single tool_result → 1 tool message
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[0]["tool_call_id"].as_str(), Some("toolu_01"));
    }

    #[test]
    fn test_stage59_double_tool_result() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_01", "content": "Bash output"},
                {"type": "tool_result", "tool_use_id": "toolu_02", "content": "Read output"}
            ]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // Two tool_results → 2 tool messages
        assert_eq!(msgs.len(), 2, "expected 2 tool messages, got {}", msgs.len());
        assert_eq!(msgs[0]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[0]["tool_call_id"].as_str(), Some("toolu_01"));
        assert_eq!(msgs[1]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[1]["tool_call_id"].as_str(), Some("toolu_02"));
    }

    #[test]
    fn test_stage59_triple_tool_result() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "tc1", "content": "r1"},
                {"type": "tool_result", "tool_use_id": "tc2", "content": "r2"},
                {"type": "tool_result", "tool_use_id": "tc3", "content": "r3"}
            ]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        // Verify all three tool_call_ids are present and distinct
        let ids: Vec<&str> = msgs.iter()
            .filter_map(|m| m["tool_call_id"].as_str())
            .collect();
        assert_eq!(ids, vec!["tc1", "tc2", "tc3"]);
        // All should be tool role
        for msg in msgs {
            assert_eq!(msg["role"].as_str(), Some("tool"));
        }
    }

    #[test]
    fn test_stage59_tool_result_plus_text() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "here is the result"},
                {"type": "tool_result", "tool_use_id": "toolu_01", "content": "output"}
            ]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // 1 user message (text) + 1 tool message (tool_result)
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"].as_str(), Some("user"));
        assert_eq!(msgs[0]["content"].as_array().unwrap()[0]["text"].as_str(), Some("here is the result"));
        assert_eq!(msgs[1]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[1]["tool_call_id"].as_str(), Some("toolu_01"));
    }

    #[test]
    fn test_stage59_empty_tool_results_boundary() {
        // User message with no tool_result blocks → single user message, name preserved
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "hello world"}
            ]}]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"].as_str(), Some("user"));
        assert_eq!(msgs[0]["content"].as_array().unwrap()[0]["text"].as_str(), Some("hello world"));
    }

    /// Regression: assistant message with tool_use block must NOT produce
    /// ContentPart { type:"text", text:None } — upstream rejects "missing field `text`".
    #[test]
    fn test_assistant_tool_use_excludes_empty_text() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "check hostname"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": ""},
                    {"type": "tool_use", "id": "toolu_01", "name": "hostname", "input": {}}
                ]}
            ],
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &test_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        let assistant = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
        let content = assistant["content"].as_array().unwrap();
        // Must not contain a {"type":"text"} without text field
        for part in content {
            if part["type"] == "text" {
                assert!(part.get("text").and_then(|v| v.as_str()).is_some(),
                    "text ContentPart must have a non-null text field: {}", part);
            }
        }
        assert!(assistant["tool_calls"].as_array().unwrap().len() > 0,
            "tool_calls must be present");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 60: System Message Normalization tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_system_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(text.to_string()),
            name: None, tool_calls: None, tool_call_id: None,
        }
    }

    fn make_user_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(text.to_string()),
            name: None, tool_calls: None, tool_call_id: None,
        }
    }

    fn make_user_parts(parts: Vec<ContentPart>) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Parts(parts),
            name: None, tool_calls: None, tool_call_id: None,
        }
    }

    fn make_assistant_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Text(text.to_string()),
            name: None, tool_calls: None, tool_call_id: None,
        }
    }

    #[test]
    fn test_stage60_real_body_with_top_system_and_inline_system() {
        // UT-1: Top-level system + user_with_parts + inline system(agent-list) → folded
        let messages = vec![
            make_system_msg("You are Claude Code, Anthropic's official CLI..."),
            make_user_parts(vec![ContentPart {
                content_type: "text".to_string(),
                text: Some("check hostname".to_string()),
                image_url: None,
            }]),
            make_system_msg("Available agent types for the Agent tool:..."),
        ];
        let folded = fold_extra_systems_into_adjacent_user(messages);
        assert_eq!(folded.len(), 2, "expected 2 messages after fold: system + user");
        assert_eq!(folded[0].role, "system");
        assert_eq!(folded[1].role, "user");
        // The user should now contain the system-reminder
        match &folded[1].content {
            ChatContent::Parts(parts) => {
                assert!(parts.iter().any(|p| p.text.as_deref().unwrap_or("").contains("<system-reminder>")),
                    "user Parts should contain <system-reminder>");
            }
            _ => panic!("expected Parts content"),
        }
        // No system beyond index 0
        for (i, m) in folded.iter().enumerate().skip(1) {
            assert_ne!(m.role, "system", "system found at index {}", i);
        }
    }

    #[test]
    fn test_stage60_multiple_systems_between() {
        // UT-2: [u1, s1, a1, u2, s2, s3, u3] → folded
        let messages = vec![
            make_user_msg("first question"),
            make_system_msg("system 1 — agent list"),
            make_assistant_msg("I'll help"),
            make_user_msg("second question"),
            make_system_msg("system 2 — skill desc"),
            make_system_msg("system 3 — more context"),
            make_user_msg("third question"),
        ];
        let folded = fold_extra_systems_into_adjacent_user(messages);
        // Expected: [u1, a1, u2(with s1), u3(with s2+s3)] = 4 messages
        // s1, s2, s3 are NOT in the output — only reminders prepended to users
        assert_eq!(folded.len(), 4, "expected 4 messages after fold: u1, a1, u2+reminder, u3+reminders");
        assert_eq!(folded[0].role, "user");
        assert_eq!(folded[1].role, "assistant");
        // u2 now contains s1
        match &folded[2].content {
            ChatContent::Text(t) => {
                assert!(t.contains("<system-reminder>"), "u2 should contain s1 reminder, got: {}", t);
                assert!(t.contains("system 1"), "u2 should contain s1 content");
                assert!(t.contains("second question"), "original user text preserved");
            }
            _ => panic!("expected Text"),
        }
        // u3 now contains s2 + s3 (at index 3, since folded has 4 msgs)
        match &folded[3].content {
            ChatContent::Text(t) => {
                assert!(t.contains("system 2"), "u3 should contain s2");
                assert!(t.contains("system 3"), "u3 should contain s3");
                assert!(t.contains("third question"), "original user text preserved");
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_stage60_tail_system() {
        // UT-3: [u1, u2, s1] → s1 appended to u2
        let messages = vec![
            make_user_msg("question 1"),
            make_user_msg("question 2"),
            make_system_msg("tail system"),
        ];
        let folded = fold_extra_systems_into_adjacent_user(messages);
        assert_eq!(folded.len(), 2, "tail system folded into u2");
        match &folded[1].content {
            ChatContent::Text(t) => {
                assert!(t.contains("<system-reminder>"), "u2 should contain reminder");
                assert!(t.contains("tail system"), "u2 should contain tail system");
                assert!(t.contains("question 2"), "original text preserved");
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_stage60_adjacent_systems() {
        // UT-4: [s1, u1, s2, s3, u2] → s1 stays at 0, s2+s3 folded into u2
        let messages = vec![
            make_system_msg("first system at index 0"),
            make_user_msg("user 1"),
            make_system_msg("system 2"),
            make_system_msg("system 3"),
            make_user_msg("user 2"),
        ];
        let folded = fold_extra_systems_into_adjacent_user(messages);
        assert_eq!(folded.len(), 3, "expected 3 messages");
        assert_eq!(folded[0].role, "system"); // s1 retained
        assert_eq!(folded[1].role, "user");
        // u2 should contain both s2 and s3
        match &folded[2].content {
            ChatContent::Text(t) => {
                assert!(t.contains("system 2"), "u2 should contain s2");
                assert!(t.contains("system 3"), "u2 should contain s3");
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_stage60_no_user_fallback() {
        // UT-5: [s1, assistant, s2] → s1 at 0, s2 creates new user
        let messages = vec![
            make_system_msg("system 1"),
            make_assistant_msg("I'll help"),
            make_system_msg("system 2 — no user follows"),
        ];
        let folded = fold_extra_systems_into_adjacent_user(messages);
        // Expected: [s1, assistant, user(with s2)]
        assert_eq!(folded.len(), 3);
        assert_eq!(folded[0].role, "system");
        assert_eq!(folded[1].role, "assistant");
        assert_eq!(folded[2].role, "user");
        match &folded[2].content {
            ChatContent::Text(t) => {
                assert!(t.contains("system 2"), "fallback user should contain s2");
            }
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn test_stage60_loose_no_fold() {
        // UT-6: Loose mode — verify adapter preserves extra systems when chat_template_compat = "loose"
        // Use qwen model (which auto-sniffs as Strict) but set explicit "loose"
        let deployment = Deployment {
            api_base: "http://localhost:1234/v1".into(),
            api_key: None,
            upstream_model: "qwen/qwen3.5-9b".into(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            raw_params: json!({"custom_llm_provider": "openai"}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: Some("loose".to_string()),
            fail_count: 0,
            cooldown_until: None,
        };
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "first question"},
                {"role": "system", "content": "system 1 — agent list"},
                {"role": "assistant", "content": "I'll help"},
                {"role": "user", "content": "second question"},
                {"role": "system", "content": "system 2 — skill desc"},
                {"role": "system", "content": "system 3 — more context"},
                {"role": "user", "content": "third question"}
            ]
        });
        let adapted = AnthropicToOpenAI.adapt_request(body, &deployment).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // In Loose mode, all messages are preserved (system messages at various positions)
        // top-level system → index 0, then messages array: user, system, assistant, user, system, system, user = 7
        let systems: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap_or("")).collect();
        // Loose → passthrough, systems should exist at multiple positions
        let system_count = systems.iter().filter(|r| **r == "system").count();
        assert!(system_count > 1, "Loose mode should preserve extra systems, got {} system messages", system_count);
    }

    #[test]
    fn test_stage60_sniff_case_insensitive() {
        // UT-7: Test resolve_chat_template_compat sniff logic
        let mk_deployment = |name: &str| Deployment {
            api_base: "https://api.openai.com/v1".into(),
            api_key: None,
            upstream_model: name.into(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            fail_count: 0,
            cooldown_until: None,
        };

        assert_eq!(
            resolve_chat_template_compat(&mk_deployment("qwen/qwen3.5-9b")),
            ChatTemplateCompat::Strict
        );
        assert_eq!(
            resolve_chat_template_compat(&mk_deployment("Qwen2.5-VL-72B")),
            ChatTemplateCompat::Strict
        );
        assert_eq!(
            resolve_chat_template_compat(&mk_deployment("gpt-4")),
            ChatTemplateCompat::Loose
        );
    }

    #[test]
    fn test_stage60_explicit_override() {
        // UT-8: Explicit chat_template_compat="loose" overrides qwen sniff
        let deployment = Deployment {
            api_base: "http://localhost:1234/v1".into(),
            api_key: None,
            upstream_model: "qwen-max".into(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: Some("loose".to_string()),
            fail_count: 0,
            cooldown_until: None,
        };
        assert_eq!(
            resolve_chat_template_compat(&deployment),
            ChatTemplateCompat::Loose,
            "explicit 'loose' should override qwen sniff"
        );
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 61: AnthropicPassthrough tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn anthropic_deployment() -> Deployment {
        Deployment {
            api_base: "https://api.anthropic.com/v1".into(),
            api_key: Some("sk-ant-test".into()),
            upstream_model: "claude-sonnet-4-20250514".into(),
            provider_type: ProviderType::AnthropicNative,
            input_cost_per_token: Some(0.000003),
            output_cost_per_token: Some(0.000015),
            raw_params: json!({"custom_llm_provider": "anthropic"}),
            model_id: Some("anthro-001".into()),
            model_group: Some("claude-sonnet-4".into()),
            custom_llm_provider: Some("anthropic".into()),
            chat_template_compat: None,
            fail_count: 0,
            cooldown_until: None,
        }
    }

    // UT-1: AnthropicPassthrough adapt_request — body content unchanged (except model swap)
    #[test]
    fn test_s61_anthropic_passthrough_request_body_passthrough() {
        let body = json!({
            "model": "claude-sonnet",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let adapted = AnthropicPassthrough.adapt_request(body, &anthropic_deployment()).unwrap();
        // Model swapped
        assert_eq!(adapted["model"].as_str(), Some("claude-sonnet-4-20250514"));
        // Messages preserved
        assert_eq!(adapted["messages"][0]["role"].as_str(), Some("user"));
        assert_eq!(adapted["messages"][0]["content"].as_str(), Some("Hello"));
        // Max tokens preserved
        assert_eq!(adapted["max_tokens"].as_i64(), Some(1024));
    }

    // UT-2: AnthropicPassthrough adapt_request — model injected correctly
    #[test]
    fn test_s61_anthropic_passthrough_model_injection() {
        let body = json!({"model": "wrong-model", "max_tokens": 512, "messages": []});
        let adapted = AnthropicPassthrough.adapt_request(body, &anthropic_deployment()).unwrap();
        assert_eq!(adapted["model"].as_str(), Some("claude-sonnet-4-20250514"));
    }

    // UT-3: AnthropicPassthrough adapt_response — error JSON passthrough
    #[test]
    fn test_s61_anthropic_passthrough_response_error_passthrough() {
        let resp = json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "Bad request"}
        });
        let adapted = AnthropicPassthrough.adapt_response(resp.clone()).unwrap();
        assert_eq!(adapted, resp);
    }

    // UT-4: AnthropicPassthrough stream — multiple SSE events passthrough
    #[test]
    fn test_s61_anthropic_passthrough_stream_multiple_events() {
        let mut stream = AnthropicPassthroughStream;
        let event = b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n";
        let result = stream.next(event).unwrap();
        assert_eq!(result, event);
        let event2 = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n";
        let result2 = stream.next(event2).unwrap();
        assert_eq!(result2, event2);
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 61: OpenAIToAnthropic tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    // UT-5: OpenAIToAnthropic adapt_request — system+user+assistant → ClaudeMessageRequest
    #[test]
    fn test_s61_openai_to_anthropic_request_basic() {
        let body = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "What is Rust?"},
                {"role": "assistant", "content": "Rust is a systems programming language."}
            ]
        });
        let adapted = OpenAIToAnthropic.adapt_request(body, &anthropic_deployment()).unwrap();
        // Model swapped to upstream
        assert_eq!(adapted["model"].as_str(), Some("claude-sonnet-4-20250514"));
        // System extracted to top-level field
        assert_eq!(adapted["system"].as_str(), Some("You are helpful."));
        // Messages preserved (excluding system)
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2); // user + assistant only
        assert_eq!(msgs[0]["role"].as_str(), Some("user"));
        assert_eq!(msgs[1]["role"].as_str(), Some("assistant"));
    }

    // UT-6: OpenAIToAnthropic adapt_response — ClaudeMessageResponse → ChatCompletionResponse
    #[test]
    fn test_s61_openai_to_anthropic_response_conversion() {
        let resp = json!({
            "id": "msg_001",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Rust is great!"}],
            "model": "claude-sonnet-4-20250514",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 15, "output_tokens": 8}
        });
        let adapted = OpenAIToAnthropic.adapt_response(resp).unwrap();
        assert_eq!(adapted["object"].as_str(), Some("chat.completion"));
        assert_eq!(adapted["model"].as_str(), Some("claude-sonnet-4-20250514"));
        let choices = adapted["choices"].as_array().unwrap();
        assert_eq!(choices[0]["message"]["content"].as_str(), Some("Rust is great!"));
        assert_eq!(choices[0]["finish_reason"].as_str(), Some("stop"));
        let usage = &adapted["usage"];
        assert_eq!(usage["prompt_tokens"].as_i64(), Some(15));
        assert_eq!(usage["completion_tokens"].as_i64(), Some(8));
    }

    // UT-7: OpenAIToAnthropic adapt_request — tool_calls → tool_use
    #[test]
    fn test_s61_openai_to_anthropic_tool_calls() {
        let body = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "What's the weather?"},
                {"role": "assistant", "content": "", "tool_calls": [
                    {"id": "call_001", "type": "function",
                     "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}}
                ]}
            ]
        });
        let adapted = OpenAIToAnthropic.adapt_request(body, &anthropic_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // User message + assistant with tool_use
        let assistant = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
        let content = assistant["content"].as_array().unwrap();
        let tool_use = content.iter().find(|b| b["type"] == "tool_use").unwrap();
        assert_eq!(tool_use["id"].as_str(), Some("call_001"));
        assert_eq!(tool_use["name"].as_str(), Some("get_weather"));
    }

    // UT-8: OpenAIToAnthropicStream — text_delta → content_block_delta.text_delta
    #[test]
    fn test_s61_stream_text_delta() {
        let mut stream = OpenAIToAnthropicStream::new();
        // First chunk: role + model
        let result1 = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
        );
        assert!(result1.is_some());
        let s1_buf = result1.unwrap();
        let s1 = String::from_utf8_lossy(&s1_buf);
        assert!(s1.contains("event: message_start"), "expected message_start, got: {}", s1);

        // Second chunk: text content
        let result2 = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}"
        );
        assert!(result2.is_some());
        let s2_buf = result2.unwrap();
        let s2 = String::from_utf8_lossy(&s2_buf);
        assert!(s2.contains("content_block_start"), "expected content_block_start, got: {}", s2);
        assert!(s2.contains("\"text\""), "expected text block");
    }

    // UT-9: OpenAIToAnthropicStream — tool_calls → content_block_start + input_json_delta
    #[test]
    fn test_s61_stream_tool_calls() {
        let mut stream = OpenAIToAnthropicStream::new();
        // Start the stream first
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
        );
        // Tool call chunk
        let result = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_001\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"{\\\"city\\\":\\\"NYC\\\"}\"}}]}}]}"
        );
        assert!(result.is_some());
        let s_buf = result.unwrap();
        let s = String::from_utf8_lossy(&s_buf);
        assert!(s.contains("content_block_start"), "expected content_block_start, got: {}", s);
        assert!(s.contains("tool_use"), "expected tool_use block, got: {}", s);
        assert!(s.contains("call_001"), "expected call_001 id, got: {}", s);
    }

    // UT-10: OpenAIToAnthropicStream — [DONE] boundary
    #[test]
    fn test_s61_stream_done_boundary() {
        let mut stream = OpenAIToAnthropicStream::new();
        // Start the stream
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
        );
        // Content blocks
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Test\"}}]}"
        );
        // [DONE]
        let result = stream.next(b"data: [DONE]");
        assert!(result.is_some());
        let s_buf = result.unwrap();
        let s = String::from_utf8_lossy(&s_buf);
        assert!(s.contains("content_block_stop") || s.contains("message_stop"),
            "expected stop events, got: {}", s);
    }

    // UT-10b: OpenAIToAnthropicStream — finish() idempotent
    #[test]
    fn test_s61_stream_finish_idempotent() {
        let mut stream = OpenAIToAnthropicStream::new();
        let r1 = stream.finish();
        assert!(r1.is_some(), "first finish should return events");
        let r2 = stream.finish();
        assert!(r2.is_none(), "second finish should be idempotent (None)");
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Hotfix: AnthropicPassthrough + Strict system-reminder extraction
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Real-world Claude Code body: system-reminder injected into a user message
    /// alongside the real query, after tool_result context.
    fn make_claude_code_qwen_body() -> Value {
        json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 4096,
            "system": "You are Claude Code, Anthropic's official CLI for Claude.",
            "messages": [
                {"role": "user", "content": "check hostname"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_01", "name": "hostname", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_01", "content": "myhost"}
                ]},
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>\nAvailable agent types for the Agent tool: foo, bar\n</system-reminder>"},
                    {"type": "text", "text": "do something"}
                ]}
            ]
        })
    }

    fn make_strict_deployment() -> Deployment {
        Deployment {
            upstream_model: "qwen/qwen3.5-9b".into(),
            chat_template_compat: None, // auto-sniff detects qwen → Strict
            ..anthropic_deployment()
        }
    }

    // UT-HF0: strip_system_reminder
    #[test]
    fn test_hf_strip_system_reminder() {
        assert_eq!(
            strip_system_reminder("<system-reminder>\nI am a system reminder\n</system-reminder>"),
            Some("I am a system reminder".to_string())
        );
        assert_eq!(
            strip_system_reminder("<system-reminder>Single line</system-reminder>"),
            Some("Single line".to_string())
        );
        assert_eq!(strip_system_reminder("plain text"), None);
        assert_eq!(strip_system_reminder("<system-reminder></system-reminder>"), None); // empty
    }

    // UT-HF1: extract_and_merge_system_reminders — from user with text + reminder
    #[test]
    fn test_hf_extract_system_reminders_basic() {
        let messages = vec![
            ClaudeMessage { role: "user".to_string(), content: ClaudeContent::Blocks(vec![
                ClaudeContentBlock { content_type: "text".to_string(), text: Some("<system-reminder>\nagent list\n</system-reminder>".to_string()), source: None, id: None, name: None, input: None, tool_use_id: None, content: None },
                ClaudeContentBlock { content_type: "text".to_string(), text: Some("actual query".to_string()), source: None, id: None, name: None, input: None, tool_use_id: None, content: None },
            ])},
        ];
        let (cleaned, extra) = extract_and_merge_system_reminders(messages);
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0], "agent list");
        assert_eq!(cleaned.len(), 1);
        match &cleaned[0].content {
            ClaudeContent::Text(t) => assert_eq!(t, "actual query"),
            _ => panic!("expected Text after single block remaining"),
        }
    }

    // UT-HF2: extract_and_merge_system_reminders — pure reminder user is dropped
    #[test]
    fn test_hf_extract_pure_reminder_dropped() {
        let messages = vec![
            ClaudeMessage { role: "user".to_string(), content: ClaudeContent::Text("<system-reminder>\ncontext\n</system-reminder>".to_string()) },
            ClaudeMessage { role: "assistant".to_string(), content: ClaudeContent::Text("ok".to_string()) },
        ];
        let (cleaned, extra) = extract_and_merge_system_reminders(messages);
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0], "context");
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].role, "assistant");
    }

    // UT-HF2b: extract role="system" messages (the actual DB-recorded failure mode)
    #[test]
    fn test_hf_extract_role_system_message() {
        let messages = vec![
            ClaudeMessage { role: "user".to_string(), content: ClaudeContent::Text("check hostname".to_string()) },
            ClaudeMessage { role: "assistant".to_string(), content: ClaudeContent::Text("ok".to_string()) },
            ClaudeMessage { role: "system".to_string(), content: ClaudeContent::Text("Extra system context".to_string()) },
            ClaudeMessage { role: "user".to_string(), content: ClaudeContent::Text("do next task".to_string()) },
        ];
        let (cleaned, extra) = extract_and_merge_system_reminders(messages);
        assert_eq!(extra.len(), 1, "role=system should be extracted, got {:?}", extra);
        assert_eq!(extra[0], "Extra system context");
        // No role="system" in output
        for msg in &cleaned {
            assert!(matches!(msg.role.as_str(), "user" | "assistant"),
                "role should not be 'system': {}", msg.role);
        }
        assert_eq!(cleaned.len(), 3); // user + assistant + user
    }

    // UT-HF2c: role="system" with Blocks content
    #[test]
    fn test_hf_extract_role_system_blocks() {
        let messages = vec![
            ClaudeMessage {
                role: "system".to_string(),
                content: ClaudeContent::Blocks(vec![
                    ClaudeContentBlock { content_type: "text".to_string(), text: Some("sys block A".to_string()), source: None, id: None, name: None, input: None, tool_use_id: None, content: None },
                    ClaudeContentBlock { content_type: "text".to_string(), text: Some("sys block B".to_string()), source: None, id: None, name: None, input: None, tool_use_id: None, content: None },
                ]),
            },
            ClaudeMessage { role: "user".to_string(), content: ClaudeContent::Text("query".to_string()) },
        ];
        let (cleaned, extra) = extract_and_merge_system_reminders(messages);
        assert_eq!(extra.len(), 1);
        assert!(extra[0].contains("sys block A"));
        assert!(extra[0].contains("sys block B"));
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].role, "user");
    }

    // UT-HF3: AnthropicPassthrough Strict — system-reminder merged into top-level system
    #[test]
    fn test_hf_passthrough_strict_fold() {
        let body = make_claude_code_qwen_body();
        let adapted = AnthropicPassthrough.adapt_request(body, &make_strict_deployment()).unwrap();

        // System must contain both original + extracted reminders
        let sys = adapted["system"].as_str().unwrap();
        assert!(sys.contains("You are Claude Code"), "missing original system: {}", sys);
        assert!(sys.contains("Available agent types"), "missing extracted reminder: {}", sys);

        // The last user message must NOT contain system-reminder tags
        let msgs = adapted["messages"].as_array().unwrap();
        let last_content = &msgs.last().unwrap()["content"];
        if let Some(t) = last_content.as_str() {
            assert!(!t.contains("system-reminder"), "user text still has reminder: {}", t);
        } else if let Some(arr) = last_content.as_array() {
            for b in arr {
                if let Some(t) = b["text"].as_str() {
                    assert!(!t.contains("system-reminder"), "user block still has reminder: {}", t);
                }
            }
        }
    }

    // UT-HF4: Loose mode — no extraction
    #[test]
    fn test_hf_passthrough_loose_no_extraction() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "<system-reminder>\nctx\n</system-reminder>"}]}]
        });
        let deployment = Deployment { chat_template_compat: Some("loose".to_string()), ..make_strict_deployment() };
        let adapted = AnthropicPassthrough.adapt_request(body, &deployment).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(adapted["system"].is_null(), "no system field should be added");
    }

    // UT-HF5: non-qwen upstream — passthrough
    #[test]
    fn test_hf_passthrough_non_qwen_passthrough() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "<system-reminder>\nctx\n</system-reminder>"}]}]
        });
        let deployment = Deployment { upstream_model: "gpt-4".into(), ..make_strict_deployment() };
        let adapted = AnthropicPassthrough.adapt_request(body, &deployment).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
    }

    // UT-HF6: tool_result messages preserved in Strict mode (no role="tool" leak)
    #[test]
    fn test_hf_passthrough_strict_preserves_tool_result() {
        // Real-world body after tool execution: tool_result + system-reminder + query
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "max_tokens": 32000,
            "system": "You are Claude Code.",
            "messages": [
                {"role": "user", "content": "check hostname"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_01", "name": "Bash", "input": {"command": "uname -r"}},
                    {"type": "tool_use", "id": "toolu_02", "name": "Bash", "input": {"command": "lsmod"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_01", "content": "7.0.11\naarch64"},
                    {"type": "tool_result", "tool_use_id": "toolu_02", "content": "Exit code 127\nlsmod: not found"}
                ]},
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>\nAvailable agent types...\n</system-reminder>"},
                    {"type": "text", "text": "do next task"}
                ]}
            ]
        });
        let adapted = AnthropicPassthrough.adapt_request(body, &make_strict_deployment()).unwrap();
        let msgs = adapted["messages"].as_array().unwrap();

        // Must NOT contain role="tool" (Anthropic protocol rejects it)
        for msg in msgs {
            let role = msg["role"].as_str().unwrap();
            assert!(
                matches!(role, "user" | "assistant"),
                "illegal role in Anthropic body: {}", role
            );
        }

        // tool_result user message preserved (index 2: after assistant with tool_use)
        assert_eq!(msgs[2]["role"].as_str(), Some("user"));
        let tr_content = msgs[2]["content"].as_array().unwrap();
        assert_eq!(tr_content[0]["type"].as_str(), Some("tool_result"));

        // system-reminder stripped from last user (index 3)
        let last_user = msgs.get(3).unwrap_or_else(|| msgs.last().unwrap());
        let last_content = &last_user["content"];
        if let Some(t) = last_content.as_str() {
            assert!(!t.contains("system-reminder"));
        }
    }
}
