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

use crate::deployment::{Deployment, ProviderType};
use crate::models::{
    AssistantMessage, ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse,
    ChatContent, ChatMessage, Choice, ChunkChoice, ClaudeContent, ClaudeContentBlock, ClaudeDelta,
    ClaudeImageSource, ClaudeMessage, ClaudeMessageRequest, ClaudeMessageResponse,
    ClaudeStreamEvent, ClaudeSystemMessage, ClaudeUsage, ContentPart, Delta, ImageUrl, ToolCall,
    ToolCallFunction, Usage,
};
use serde_json::{json, Value};

/// The client-facing protocol of the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientProtocol {
    /// /v1/chat/completions
    OpenAI,
    /// /v1/messages
    Anthropic,
    /// /v1/responses (OpenAI Responses API)
    Responses,
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

pub fn select_adapter(
    client: ClientProtocol,
    provider: &ProviderType,
) -> Option<&'static dyn MessageAdapter> {
    match (client, provider) {
        (ClientProtocol::OpenAI, ProviderType::OpenAICompatible) => Some(&OpenAIPassthrough),
        (ClientProtocol::Anthropic, ProviderType::OpenAICompatible) => Some(&AnthropicToOpenAI),
        (ClientProtocol::Anthropic, ProviderType::AnthropicNative) => Some(&AnthropicPassthrough),
        (ClientProtocol::OpenAI, ProviderType::AnthropicNative) => Some(&OpenAIToAnthropic),
        (ClientProtocol::Responses, ProviderType::OpenAICompatible) => {
            Some(&ResponsesToChatCompletions)
        }
        (ClientProtocol::Responses, ProviderType::AnthropicNative) => None,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Legacy ProviderAdapter trait
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub trait ProviderAdapter {
    fn openai_to_claude_request(
        req: &ChatCompletionRequest,
        max_tokens: i32,
    ) -> ClaudeMessageRequest;
    fn claude_to_openai_response(
        resp: &ClaudeMessageResponse,
        model: &str,
    ) -> ChatCompletionResponse;
    fn claude_to_openai_request(req: &ClaudeMessageRequest) -> ChatCompletionRequest;
    fn openai_to_claude_response(resp: &ChatCompletionResponse) -> ClaudeMessageResponse;
    fn claude_stream_to_openai_chunk(
        event: &ClaudeStreamEvent,
        model: &str,
        request_id: &str,
    ) -> Option<ChatCompletionChunk>;
    fn openai_chunk_to_claude_stream(chunk: &ChatCompletionChunk) -> Option<ClaudeStreamEvent>;
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OpenAIPassthrough
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct OpenAIPassthrough;

impl MessageAdapter for OpenAIPassthrough {
    fn client_protocol(&self) -> ClientProtocol {
        ClientProtocol::OpenAI
    }

    fn adapt_request(
        &self,
        mut body: Value,
        deployment: &Deployment,
    ) -> Result<Value, AdapterError> {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("model".to_string(), json!(deployment.upstream_model));
            // Inject stream_options so upstream returns token usage in the final SSE chunk
            if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
                obj.insert("stream_options".to_string(), json!({"include_usage": true}));
            }
        }
        Ok(body)
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> {
        Ok(body)
    }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(PassthroughStream))
    }
}

struct PassthroughStream;
impl StreamAdapter for PassthroughStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        Some(chunk.to_vec())
    }
    fn finish(&mut self) -> Option<Vec<u8>> {
        None
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AnthropicToOpenAI
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

pub struct AnthropicToOpenAI;

impl MessageAdapter for AnthropicToOpenAI {
    fn client_protocol(&self) -> ClientProtocol {
        ClientProtocol::Anthropic
    }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let req: ClaudeMessageRequest = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid Claude request: {}", e)))?;
        let oai_req = DefaultAdapter::claude_to_openai_request(&req);

        // Stage 60: System message normalization for strict chat templates
        let compat = resolve_chat_template_compat(deployment);
        let oai_req = match compat {
            ChatTemplateCompat::Strict => {
                let messages = fold_extra_systems_into_adjacent_user(oai_req.messages);
                ChatCompletionRequest {
                    messages,
                    ..oai_req
                }
            }
            ChatTemplateCompat::Loose => oai_req,
            ChatTemplateCompat::Auto => {
                // Already resolved to Strict or Loose by resolve_chat_template_compat;
                // this arm is unreachable but kept for exhaustiveness.
                oai_req
            }
        };

        let mut json =
            serde_json::to_value(&oai_req).map_err(|e| AdapterError::Parse(e.to_string()))?;
        if let Some(obj) = json.as_object_mut() {
            obj.insert("model".to_string(), json!(deployment.upstream_model));
            // Inject stream_options so upstream returns token usage in the final SSE chunk
            if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
                obj.insert("stream_options".to_string(), json!({"include_usage": true}));
            }
        }
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
        // reasoning_content -> thinking block (must come before text/tool for Claude protocol)
        if let Some(ref rc) = choice.message.reasoning_content {
            if !rc.is_empty() {
                content.push(ClaudeContentBlock {
                    content_type: "thinking".to_string(),
                    text: None,
                    source: None,
                    id: None,
                    name: None,
                    input: None,
                    tool_use_id: None,
                    content: None,
                    thinking: Some(rc.clone()),
                    signature: None,
                    citations: None,
                });
            }
        }
        if !choice.message.content.is_empty() {
            content.push(ClaudeContentBlock {
                content_type: "text".to_string(),
                text: Some(choice.message.content.clone()),
                source: None,
                id: None,
                name: None,
                input: None,
                tool_use_id: None,
                content: None,
                thinking: None,
                signature: None,
                citations: None,
            });
        }
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let input: Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                content.push(ClaudeContentBlock {
                    content_type: "tool_use".to_string(),
                    text: None,
                    source: None,
                    id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                    input: Some(input),
                    tool_use_id: None,
                    content: None,
                    thinking: None,
                    signature: None,
                    citations: None,
                });
            }
        }
    }
    let stop_reason = match resp
        .choices
        .first()
        .and_then(|c| c.finish_reason.as_deref())
    {
        Some("tool_calls") => Some("tool_use".to_string()),
        Some("stop") => Some("end_turn".to_string()),
        Some("length") => Some("max_tokens".to_string()),
        Some(s) => Some(s.to_string()),
        None => None,
    };
    ClaudeMessageResponse {
        id: resp.id.clone(),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: resp.model.clone(),
        stop_reason,
        stop_sequence: None,
        usage: ClaudeUsage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
            cache_read_input_tokens: None,
            cache_creation_input_tokens: None,
        },
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
            let reminders: Vec<String> = std::mem::take(&mut pending_reminders);
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
                reasoning_content: None,
            });
        }
    }

    // Post-condition: only index 0 can be system
    debug_assert!(
        out.iter()
            .enumerate()
            .all(|(i, m)| i == 0 || m.role != "system"),
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
        ChatContent::Text(t) => ChatContent::Text(format!("{}\n\n{}", reminder_text, t)),
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
        reasoning_content: None,
    }
}

/// Append text to a user ChatMessage's content (for tail system reminders).
fn append_text_to_chat_message(msg: &ChatMessage, text: &str) -> ChatMessage {
    let new_content = match &msg.content {
        ChatContent::Text(t) => ChatContent::Text(format!("{}\n\n{}", t, text)),
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
        reasoning_content: None,
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// AnthropicToOpenAIStream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

enum BlockType {
    Text,
    #[allow(dead_code)]
    ToolUse {
        id: String,
        name: String,
    },
}

pub struct AnthropicToOpenAIStream {
    model: String,
    message_id: String,
    current_block_index: i32,
    current_block: Option<BlockType>,
    started: bool,
}

impl Default for AnthropicToOpenAIStream {
    fn default() -> Self {
        Self::new()
    }
}

impl AnthropicToOpenAIStream {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            message_id: format!("msg_{}", uuid::Uuid::new_v4()),
            current_block_index: 0,
            current_block: None,
            started: false,
        }
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
                event_type: "content_block_stop".to_string(),
                index: Some(self.current_block_index - 1),
                delta: None,
                content_block: None,
                message: None,
                usage: None,
            }) {
                buf.extend_from_slice(&cbs);
            }
            self.current_block = None;
        }
        if let Some(ms) = self.emit_event(&ClaudeStreamEvent {
            event_type: "message_stop".to_string(),
            index: None,
            delta: None,
            content_block: None,
            message: None,
            usage: None,
        }) {
            buf.extend_from_slice(&ms);
        }
        self.current_block_index = -1; // mark as finished
        if buf.is_empty() {
            None
        } else {
            Some(buf)
        }
    }
}

impl StreamAdapter for AnthropicToOpenAIStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        let text = String::from_utf8_lossy(chunk);
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let data = line
                .strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
                .unwrap_or(line);
            if data == "[DONE]" {
                return None;
            }
            let chunk: ChatCompletionChunk = serde_json::from_str(data).ok()?;

            if !self.started && !chunk.model.is_empty() {
                self.model = chunk.model.clone();
            }

            for choice in &chunk.choices {
                if !self.started {
                    self.started = true;
                    return self.emit_event(&ClaudeStreamEvent {
                        event_type: "message_start".to_string(),
                        index: None,
                        delta: None,
                        content_block: None,
                        message: Some(ClaudeMessageResponse {
                            id: self.message_id.clone(),
                            response_type: "message".to_string(),
                            role: "assistant".to_string(),
                            content: vec![],
                            model: self.model.clone(),
                            stop_reason: None,
                            stop_sequence: None,
                            usage: ClaudeUsage {
                                input_tokens: 0,
                                output_tokens: 0,
                                cache_read_input_tokens: None,
                                cache_creation_input_tokens: None,
                            },
                        }),
                        usage: None,
                    });
                }

                let has_tool_calls = choice
                    .delta
                    .tool_calls
                    .as_ref()
                    .map(|tc| {
                        tc.iter()
                            .any(|t| t.id.as_ref().map(|id| !id.is_empty()).unwrap_or(false))
                    })
                    .unwrap_or(false);

                if !has_tool_calls {
                    if let Some(ref text) = choice.delta.content {
                        if !text.is_empty() {
                            let needs_new_block =
                                !matches!(&self.current_block, Some(BlockType::Text));
                            if needs_new_block {
                                self.current_block = Some(BlockType::Text);
                                let idx = self.current_block_index;
                                self.current_block_index += 1;
                                return self.emit_event(&ClaudeStreamEvent {
                                    event_type: "content_block_start".to_string(),
                                    index: Some(idx),
                                    delta: None,
                                    content_block: Some(ClaudeContentBlock {
                                        content_type: "text".to_string(),
                                        text: None,
                                        source: None,
                                        id: None,
                                        name: None,
                                        input: None,
                                        tool_use_id: None,
                                        content: None,
                                        thinking: None,
                                        signature: None,
                                        citations: None,
                                    }),
                                    message: None,
                                    usage: None,
                                });
                            }
                            return self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_delta".to_string(),
                                index: Some(self.current_block_index - 1),
                                delta: Some(ClaudeDelta {
                                    delta_type: "text_delta".to_string(),
                                    text: Some(text.clone()),
                                    partial_json: None,
                                }),
                                content_block: None,
                                message: None,
                                usage: None,
                            });
                        }
                    }
                }

                // Process tool_calls BEFORE text content — DeepSeek thinking models
                // emit reasoning_content (text) and tool_calls in the same chunk;
                // tool_calls must take priority to create the correct block type.
                //
                // Stage 120: tool_calls 分支必须在同一 chunk 内 emit `content_block_start`
                // 与 `input_json_delta` 两个事件.早期 early-return 会丢掉与 id 同帧的
                // arguments 首帧(tokenhub GLM-5.2 首帧 `id + "{\""` 触发 bug),导致下游
                // Claude Code 累积后的 partial JSON 缺开头 `{"` → `Invalid tool parameters`.
                // 修复:累积到 local buffer,循环结束统一返回.
                let mut tool_out: Vec<u8> = Vec::new();
                if let Some(ref tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        if let Some(ref id) = tc.id {
                            if !id.is_empty() {
                                let tc_name = tc.function.name.clone().unwrap_or_default();
                                self.current_block = Some(BlockType::ToolUse {
                                    id: id.clone(),
                                    name: tc_name.clone(),
                                });
                                let idx = self.current_block_index;
                                self.current_block_index += 1;
                                if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
                                    event_type: "content_block_start".to_string(),
                                    index: Some(idx),
                                    delta: None,
                                    content_block: Some(ClaudeContentBlock {
                                        content_type: "tool_use".to_string(),
                                        text: None,
                                        source: None,
                                        id: Some(id.clone()),
                                        name: Some(tc_name),
                                        input: Some(json!({})),
                                        tool_use_id: None,
                                        content: None,
                                        thinking: None,
                                        signature: None,
                                        citations: None,
                                    }),
                                    message: None,
                                    usage: None,
                                }) {
                                    tool_out.extend_from_slice(&ev);
                                }
                            }
                        }
                        if !tc.function.arguments.is_empty() {
                            if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_delta".to_string(),
                                index: Some(self.current_block_index - 1),
                                delta: Some(ClaudeDelta {
                                    delta_type: "input_json_delta".to_string(),
                                    text: None,
                                    partial_json: Some(tc.function.arguments.clone()),
                                }),
                                content_block: None,
                                message: None,
                                usage: None,
                            }) {
                                tool_out.extend_from_slice(&ev);
                            }
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
                    if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
                        event_type: "message_delta".to_string(),
                        index: None,
                        delta: Some(ClaudeDelta {
                            delta_type: "stop_reason".to_string(),
                            text: sr,
                            partial_json: None,
                        }),
                        content_block: None,
                        message: None,
                        usage: None,
                    }) {
                        tool_out.extend_from_slice(&ev);
                    }
                }

                if !tool_out.is_empty() {
                    return Some(tool_out);
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
            tools: None,
            tool_choice: None,
            thinking: None,
        }
    }

    fn claude_to_openai_response(
        resp: &ClaudeMessageResponse,
        model: &str,
    ) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: resp.id.clone(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: model.to_string(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".to_string(),
                    content: claude_content_to_text(&resp.content),
                    tool_calls: None,
                    reasoning_content: None,
                    refusal: None,
                },
                finish_reason: claude_stop_to_openai(&resp.stop_reason),
            }],
            usage: Usage {
                prompt_tokens: resp.usage.input_tokens,
                completion_tokens: resp.usage.output_tokens,
                total_tokens: resp.usage.input_tokens + resp.usage.output_tokens,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            },
            system_fingerprint: None,
        }
    }

    fn claude_to_openai_request(req: &ClaudeMessageRequest) -> ChatCompletionRequest {
        let mut messages: Vec<ChatMessage> = Vec::new();
        if let Some(ref sys) = req.system {
            match sys {
                ClaudeSystemMessage::Text(t) => messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: ChatContent::Text(t.clone()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                }),
                ClaudeSystemMessage::Blocks(blocks) => {
                    let text = claude_blocks_to_text(blocks);
                    if !text.is_empty() {
                        messages.push(ChatMessage {
                            role: "system".to_string(),
                            content: ChatContent::Text(text),
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                            reasoning_content: None,
                        });
                    }
                }
            }
        }
        for msg in &req.messages {
            messages.extend(claude_message_to_openai(msg));
        }

        // Map Claude tools → OpenAI tools
        let tools = req.tools.as_ref().map(|claude_tools| {
            claude_tools
                .iter()
                .map(|ct| crate::models::ToolDef {
                    tool_type: "function".to_string(),
                    function: crate::models::ToolDefFunction {
                        name: ct.name.clone(),
                        description: ct.description.clone(),
                        parameters: Some(ct.input_schema.clone()),
                    },
                })
                .collect()
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
                    if tc.is_string() {
                        tc.clone()
                    } else {
                        json!("auto")
                    }
                }
            }
        });

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
            tools,
            tool_choice,
            response_format: None,
            reasoning_effort: None,
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
                id: None,
                name: None,
                input: None,
                tool_use_id: None,
                content: None,
                thinking: None,
                signature: None,
                citations: None,
            }],
            model: resp.model.clone(),
            stop_reason: openai_stop_to_claude(
                &resp.choices.first().and_then(|c| c.finish_reason.clone()),
            ),
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: resp.usage.prompt_tokens,
                output_tokens: resp.usage.completion_tokens,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
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
                Some(ChatCompletionChunk {
                    id: request_id.to_string(),
                    object: "chat.completion.chunk".to_string(),
                    created: now,
                    model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: event.index.unwrap_or(0),
                        delta: Delta {
                            role: None,
                            content: delta.text.clone(),
                            tool_calls: None,
                            reasoning_content: None,
                            refusal: None,
                        },
                        finish_reason: None,
                    }],
                    usage: None,
                    system_fingerprint: None,
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
                        tool_calls: None,
                        reasoning_content: None,
                        refusal: None,
                    },
                    finish_reason: None,
                }],
                usage: None,
                system_fingerprint: None,
            }),
            "message_delta" => {
                let stop_reason = event.delta.as_ref().and_then(|d| {
                    if d.delta_type == "stop_reason" {
                        d.text.clone()
                    } else {
                        None
                    }
                });
                claude_stop_to_openai(&stop_reason).map(|fr| ChatCompletionChunk {
                    id: request_id.to_string(),
                    object: "chat.completion.chunk".to_string(),
                    created: now,
                    model: model.to_string(),
                    choices: vec![ChunkChoice {
                        index: 0,
                        delta: Delta {
                            role: None,
                            content: None,
                            tool_calls: None,
                            reasoning_content: None,
                            refusal: None,
                        },
                        finish_reason: Some(fr),
                    }],
                    usage: None,
                    system_fingerprint: None,
                })
            }
            _ => None,
        }
    }

    fn openai_chunk_to_claude_stream(chunk: &ChatCompletionChunk) -> Option<ClaudeStreamEvent> {
        for choice in &chunk.choices {
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
                            cache_read_input_tokens: None,
                            cache_creation_input_tokens: None,
                        },
                    }),
                    usage: None,
                });
            }
            if let Some(ref text) = choice.delta.content {
                return Some(ClaudeStreamEvent {
                    event_type: "content_block_delta".to_string(),
                    index: Some(choice.index),
                    delta: Some(ClaudeDelta {
                        delta_type: "text_delta".to_string(),
                        text: Some(text.clone()),
                        partial_json: None,
                    }),
                    content_block: None,
                    message: None,
                    usage: None,
                });
            }
            if let Some(ref finish) = choice.finish_reason {
                let stop_reason = openai_stop_to_claude(&Some(finish.clone()));
                return Some(ClaudeStreamEvent {
                    event_type: "message_delta".to_string(),
                    index: Some(choice.index),
                    delta: Some(ClaudeDelta {
                        delta_type: "stop_reason".to_string(),
                        text: stop_reason,
                        partial_json: None,
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
// Helpers
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

/// Parse a `data:<media_type>;base64,<payload>` URL into `(media_type, data)`.
///
/// Anthropic's Messages API requires `ClaudeImageSource.data` to be the **raw
/// base64 payload** (no `data:` prefix) and `media_type` to match the actual
/// image format. On malformed input (no comma / non-data prefix) the payload is
/// passed through verbatim with an `image/png` fallback so the image is never
/// silently dropped — the upstream decides whether to reject.
fn parse_data_url(url: &str) -> (String, String) {
    let Some(rest) = url.strip_prefix("data:") else {
        return ("image/png".to_string(), url.to_string());
    };
    let Some(comma) = rest.find(',') else {
        return ("image/png".to_string(), url.to_string());
    };
    let mime = &rest[..comma];
    let payload = &rest[comma + 1..];
    // MIME segment may carry parameters (e.g. "image/jpeg;base64", "image/png;charset=utf-8").
    let media_type = mime.split(';').next().unwrap_or("image/png").to_string();
    if media_type.is_empty() {
        ("image/png".to_string(), payload.to_string())
    } else {
        (media_type, payload.to_string())
    }
}

fn openai_message_to_claude(msg: &ChatMessage) -> ClaudeMessage {
    let mut blocks: Vec<ClaudeContentBlock> = Vec::new();

    // 1. Content blocks
    match &msg.content {
        ChatContent::Text(t) => {
            if !t.is_empty() {
                blocks.push(ClaudeContentBlock {
                    content_type: "text".to_string(),
                    text: Some(t.clone()),
                    source: None,
                    id: None,
                    name: None,
                    input: None,
                    tool_use_id: None,
                    content: None,
                    thinking: None,
                    signature: None,
                    citations: None,
                });
            }
        }
        ChatContent::Parts(parts) => {
            blocks.extend(parts.iter().map(|p| {
                if let Some(ref image_url) = p.image_url {
                    let (media_type, data) = parse_data_url(&image_url.url);
                    ClaudeContentBlock {
                        content_type: "image".to_string(),
                        text: None,
                        source: Some(ClaudeImageSource {
                            source_type: "base64".to_string(),
                            media_type,
                            data,
                        }),
                        id: None,
                        name: None,
                        input: None,
                        tool_use_id: None,
                        content: None,
                        thinking: None,
                        signature: None,
                        citations: None,
                    }
                } else {
                    ClaudeContentBlock {
                        content_type: "text".to_string(),
                        text: p.text.clone(),
                        source: None,
                        id: None,
                        name: None,
                        input: None,
                        tool_use_id: None,
                        content: None,
                        thinking: None,
                        signature: None,
                        citations: None,
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
                content_type: "tool_use".to_string(),
                text: None,
                source: None,
                id: Some(tc.id.clone()),
                name: Some(tc.function.name.clone()),
                input: Some(input),
                tool_use_id: None,
                content: None,
                thinking: None,
                signature: None,
                citations: None,
            });
        }
    }

    if blocks.is_empty() {
        ClaudeMessage {
            role: msg.role.clone(),
            content: ClaudeContent::Text(String::new()),
        }
    } else {
        ClaudeMessage {
            role: msg.role.clone(),
            content: ClaudeContent::Blocks(blocks),
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

fn claude_message_to_openai(msg: &ClaudeMessage) -> Vec<ChatMessage> {
    match &msg.content {
        ClaudeContent::Text(t) => vec![ChatMessage {
            role: msg.role.clone(),
            content: ChatContent::Text(t.clone()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }],
        ClaudeContent::Blocks(blocks) => {
            // Extract reasoning_content from thinking blocks (Anthropic -> OpenAI)
            let reasoning: Option<String> = blocks
                .iter()
                .filter(|b| b.content_type == "thinking")
                .filter_map(|b| b.thinking.clone())
                .reduce(|a, b| a + &b);

            let tool_results: Vec<(String, String)> = blocks
                .iter()
                .filter(|b| b.content_type == "tool_result")
                .filter_map(|b| {
                    let tui = b.tool_use_id.clone()?;
                    let c = b
                        .content
                        .as_ref()
                        .and_then(|v| v.as_str().map(String::from))
                        .or_else(|| b.text.clone())
                        .unwrap_or_default();
                    Some((tui, c))
                })
                .collect();

            if !tool_results.is_empty() && msg.role == "user" {
                let mut out = Vec::new();

                // Each tool_result → one tool message FIRST (OpenAI protocol:
                // tool messages must immediately follow the assistant message
                // that issued the tool_calls; user text must come after).
                for (tool_use_id, content) in &tool_results {
                    out.push(ChatMessage {
                        role: "tool".to_string(),
                        content: ChatContent::Text(content.clone()),
                        name: None,
                        tool_calls: None,
                        tool_call_id: Some(tool_use_id.clone()),
                        reasoning_content: None,
                    });
                }

                // Non-tool_result content parts (text/image) → user message AFTER tool messages
                let non_tool_parts: Vec<ContentPart> = blocks
                    .iter()
                    .filter(|b| b.content_type != "tool_result")
                    .filter(|b| b.content_type == "text" || b.content_type == "image")
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
                if !non_tool_parts.is_empty() {
                    out.push(ChatMessage {
                        role: "user".to_string(),
                        content: ChatContent::Parts(non_tool_parts),
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: None,
                    });
                }

                out
            } else {
                // Only emit ContentParts for text/image blocks.
                // tool_use and tool_result are handled separately above
                // (tool_calls / tool_call_id); including them would produce
                // ContentPart { type:"text", text:None } which upstream
                // rejects as "missing field `text`".
                let parts: Vec<ContentPart> = blocks
                    .iter()
                    .filter(|b| b.content_type == "text" || b.content_type == "image")
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
                let tool_calls: Vec<ToolCall> = blocks
                    .iter()
                    .filter(|b| b.content_type == "tool_use")
                    .filter_map(|b| {
                        let id = b.id.clone()?;
                        let name = b.name.clone()?;
                        let input = b.input.clone().unwrap_or(json!({}));
                        Some(ToolCall {
                            id,
                            call_type: "function".to_string(),
                            function: ToolCallFunction {
                                name,
                                arguments: input.to_string(),
                            },
                        })
                    })
                    .collect();
                let tc = if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                };
                vec![ChatMessage {
                    role: msg.role.clone(),
                    content: ChatContent::Parts(parts),
                    name: None,
                    tool_calls: tc,
                    tool_call_id: None,
                    reasoning_content: reasoning.clone(),
                }]
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
                content_type: "text".to_string(),
                text: Some(t),
                source: None,
                id: None,
                name: None,
                input: None,
                tool_use_id: None,
                content: None,
                thinking: None,
                signature: None,
                citations: None,
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
    fn client_protocol(&self) -> ClientProtocol {
        ClientProtocol::Anthropic
    }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let mut req: ClaudeMessageRequest = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid Claude request: {}", e)))?;

        // Mirror AnthropicToOpenAI normalization on the Anthropic body level.
        // Strip role="system" messages and <system-reminder> blocks from messages,
        // merging them into the top-level `system` field.
        let compat = resolve_chat_template_compat(deployment);
        if compat == ChatTemplateCompat::Strict {
            let (cleaned, reminders) =
                extract_and_merge_system_reminders(std::mem::take(&mut req.messages));
            req.messages = cleaned;

            if !reminders.is_empty() {
                let combined = reminders.join("\n\n");
                req.system = Some(match req.system.take() {
                    Some(ClaudeSystemMessage::Text(existing)) => {
                        ClaudeSystemMessage::Text(format!("{}\n\n{}", existing, combined))
                    }
                    Some(ClaudeSystemMessage::Blocks(mut blocks)) => {
                        blocks.extend(reminders.into_iter().map(|t| ClaudeContentBlock {
                            content_type: "text".to_string(),
                            text: Some(t),
                            source: None,
                            id: None,
                            name: None,
                            input: None,
                            tool_use_id: None,
                            content: None,
                            thinking: None,
                            signature: None,
                            citations: None,
                        }));
                        ClaudeSystemMessage::Blocks(blocks)
                    }
                    None => ClaudeSystemMessage::Text(combined),
                });
            }
        }

        req.model = deployment.upstream_model.clone();
        let is_stream = req.stream.unwrap_or(false);
        let mut json =
            serde_json::to_value(&req).map_err(|e| AdapterError::Parse(e.to_string()))?;
        // Anthropic requires `include_usage` in body for streaming responses
        // to include usage.{input_tokens, output_tokens} in message_delta events
        if is_stream {
            if let Some(obj) = json.as_object_mut() {
                obj.insert("stream_options".to_string(), json!({"include_usage": true}));
            }
        }
        Ok(json)
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> {
        Ok(body)
    }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(AnthropicPassthroughStream))
    }
}

/// Stream adapter: transparent passthrough of Anthropic SSE events.
struct AnthropicPassthroughStream;

impl StreamAdapter for AnthropicPassthroughStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        Some(chunk.to_vec())
    }
    fn finish(&mut self) -> Option<Vec<u8>> {
        None
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// OpenAIToAnthropic (Stage 61)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Client (OpenAI) → OpenAIToAnthropic → Upstream (Anthropic Native)
// Bidirectional: OpenAI Chat Completions ↔ Anthropic Messages.

pub struct OpenAIToAnthropic;

impl MessageAdapter for OpenAIToAnthropic {
    fn client_protocol(&self) -> ClientProtocol {
        ClientProtocol::OpenAI
    }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let oai_req: ChatCompletionRequest = serde_json::from_value(body)
            .map_err(|e| AdapterError::Parse(format!("Invalid OpenAI request: {}", e)))?;
        let max_tokens = oai_req.max_tokens.unwrap_or(4096);
        let claude_req = DefaultAdapter::openai_to_claude_request(&oai_req, max_tokens);
        let mut json =
            serde_json::to_value(&claude_req).map_err(|e| AdapterError::Parse(e.to_string()))?;
        if let Some(obj) = json.as_object_mut() {
            obj.insert("model".to_string(), json!(deployment.upstream_model));
        }
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
    ToolUse {
        id: String,
        name: String,
    },
}

pub struct OpenAIToAnthropicStream {
    model: String,
    message_id: String,
    current_block_index: i32,
    current_block: Option<O2ABlockType>,
    started: bool,
    finished: bool,
}

impl Default for OpenAIToAnthropicStream {
    fn default() -> Self {
        Self::new()
    }
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
                delta: None,
                content_block: None,
                message: None,
                usage: None,
            }) {
                buf.extend_from_slice(&cbs);
            }
            self.current_block = None;
        }
        if let Some(ms) = self.emit_event(&ClaudeStreamEvent {
            event_type: "message_stop".to_string(),
            index: None,
            delta: None,
            content_block: None,
            message: None,
            usage: None,
        }) {
            buf.extend_from_slice(&ms);
        }
        if buf.is_empty() {
            None
        } else {
            Some(buf)
        }
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
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let data = line
                .strip_prefix("data: ")
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
                        event_type: "message_start".to_string(),
                        index: None,
                        delta: None,
                        content_block: None,
                        message: Some(ClaudeMessageResponse {
                            id: self.message_id.clone(),
                            response_type: "message".to_string(),
                            role: "assistant".to_string(),
                            content: vec![],
                            model: self.model.clone(),
                            stop_reason: None,
                            stop_sequence: None,
                            usage: ClaudeUsage {
                                input_tokens: 0,
                                output_tokens: 0,
                                cache_read_input_tokens: None,
                                cache_creation_input_tokens: None,
                            },
                        }),
                        usage: None,
                    });
                }

                // Tool calls processed BEFORE text — same reasoning as AnthropicToOpenAIStream
                // Tool calls processed BEFORE text — same reasoning as AnthropicToOpenAIStream
                // (DeepSeek thinking models emit both in the same chunk)
                //
                // Stage 120: 对称修复.同一 chunk 内 emit `content_block_start`
                // 与 `input_json_delta` 两个事件,避免首帧 arguments 丢帧.
                let mut tool_out: Vec<u8> = Vec::new();
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
                                if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
                                    event_type: "content_block_start".to_string(),
                                    index: Some(idx),
                                    delta: None,
                                    content_block: Some(ClaudeContentBlock {
                                        content_type: "tool_use".to_string(),
                                        text: None,
                                        source: None,
                                        id: Some(id.clone()),
                                        name: Some(tc_name),
                                        input: Some(json!({})),
                                        tool_use_id: None,
                                        content: None,
                                        thinking: None,
                                        signature: None,
                                        citations: None,
                                    }),
                                    message: None,
                                    usage: None,
                                }) {
                                    tool_out.extend_from_slice(&ev);
                                }
                            }
                        }
                        if !tc.function.arguments.is_empty() {
                            if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_delta".to_string(),
                                index: Some((self.current_block_index - 1).max(0)),
                                delta: Some(ClaudeDelta {
                                    delta_type: "input_json_delta".to_string(),
                                    text: None,
                                    partial_json: Some(tc.function.arguments.clone()),
                                }),
                                content_block: None,
                                message: None,
                                usage: None,
                            }) {
                                tool_out.extend_from_slice(&ev);
                            }
                        }
                    }
                }
                if !tool_out.is_empty() {
                    return Some(tool_out);
                }

                // Text content
                if let Some(ref text) = choice.delta.content {
                    if !text.is_empty() {
                        let needs_new_block =
                            !matches!(&self.current_block, Some(O2ABlockType::Text));
                        if needs_new_block {
                            self.current_block = Some(O2ABlockType::Text);
                            let idx = self.current_block_index;
                            self.current_block_index += 1;
                            return self.emit_event(&ClaudeStreamEvent {
                                event_type: "content_block_start".to_string(),
                                index: Some(idx),
                                delta: None,
                                content_block: Some(ClaudeContentBlock {
                                    content_type: "text".to_string(),
                                    text: None,
                                    source: None,
                                    id: None,
                                    name: None,
                                    input: None,
                                    tool_use_id: None,
                                    content: None,
                                    thinking: None,
                                    signature: None,
                                    citations: None,
                                }),
                                message: None,
                                usage: None,
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
                            content_block: None,
                            message: None,
                            usage: None,
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
                        event_type: "message_delta".to_string(),
                        index: None,
                        delta: Some(ClaudeDelta {
                            delta_type: "stop_reason".to_string(),
                            text: sr,
                            partial_json: None,
                        }),
                        content_block: None,
                        message: None,
                        usage: None,
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
// ResponsesToChatCompletions (Stage 102)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Client (Responses API) → ResponsesToChatCompletions → Upstream (Chat Completions)
// Transparent bridge: converts Responses API format to/from Chat Completions.

pub struct ResponsesToChatCompletions;

impl ResponsesToChatCompletions {
    /// Convert Responses API `input` field to Chat Completions `messages`.
    fn input_to_messages(input: &Value) -> Result<Vec<Value>, AdapterError> {
        match input {
            Value::String(text) => Ok(vec![json!({"role": "user", "content": text})]),
            Value::Array(items) => {
                if items.is_empty() {
                    return Err(AdapterError::Unsupported(
                        "input array must not be empty".to_string(),
                    ));
                }
                // Map {role, content} directly
                let messages: Vec<Value> = items
                    .iter()
                    .map(|item| {
                        let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                        json!({"role": role, "content": item.get("content").cloned().unwrap_or(Value::Null)})
                    })
                    .collect();
                Ok(messages)
            }
            _ => Err(AdapterError::Unsupported(
                "input must be a string or array".to_string(),
            )),
        }
    }
}

impl MessageAdapter for ResponsesToChatCompletions {
    fn client_protocol(&self) -> ClientProtocol {
        ClientProtocol::Responses
    }

    fn adapt_request(&self, body: Value, deployment: &Deployment) -> Result<Value, AdapterError> {
        let mut obj = match body {
            Value::Object(o) => o,
            _ => {
                return Err(AdapterError::Parse(
                    "request body must be a JSON object".to_string(),
                ));
            }
        };

        // 1. Validate tools — only function type is supported
        if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()) {
            for tool in tools {
                let tool_type = tool.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match tool_type {
                    "function" => {} // allowed
                    other => {
                        return Err(AdapterError::Unsupported(format!(
                            "tool type '{}' is not supported in Responses→Chat bridge. Only 'function' tools are supported.",
                            other
                        )));
                    }
                }
            }
        }

        // 2. Extract input → messages
        let input = obj
            .remove("input")
            .ok_or_else(|| AdapterError::Unsupported("missing 'input' field".to_string()))?;
        let mut messages = Self::input_to_messages(&input)?;

        // 3. Extract instructions → prepend system message
        if let Some(instructions) = obj
            .remove("instructions")
            .and_then(|v| v.as_str().map(|s| s.to_string()))
        {
            if !instructions.is_empty() {
                messages.insert(0, json!({"role": "system", "content": instructions}));
            }
        }

        // 4. Field rename: max_output_tokens → max_tokens
        if let Some(mot) = obj.remove("max_output_tokens") {
            obj.insert("max_tokens".to_string(), mot);
        }

        // 5. Drop unsupported fields (log warnings)
        for field in &[
            "reasoning",
            "previous_response_id",
            "conversation",
            "include",
            "truncation",
            "text",
        ] {
            if obj.remove(*field).is_some() {
                tracing::debug!("dropped unsupported Responses API field: {}", field);
            }
        }

        // 6. Inject messages + model
        obj.insert("messages".to_string(), Value::Array(messages));
        obj.insert("model".to_string(), json!(deployment.upstream_model));

        // 7. Inject stream_options for streaming
        if obj.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
            obj.insert("stream_options".to_string(), json!({"include_usage": true}));
        }

        Ok(Value::Object(obj))
    }

    fn adapt_response(&self, body: Value) -> Result<Value, AdapterError> {
        let obj = match body {
            Value::Object(o) => o,
            _ => {
                return Err(AdapterError::Parse(
                    "response body must be a JSON object".to_string(),
                ));
            }
        };

        let id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| format!("resp_{}", s))
            .unwrap_or_else(|| "resp_unknown".to_string());
        let model = obj
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Extract choices[0].message
        let message = obj
            .get("choices")
            .and_then(|v| v.as_array())
            .and_then(|choices| choices.first());

        // Build output array
        let mut output: Vec<Value> = Vec::new();

        if let Some(msg) = message {
            let role = msg
                .get("message")
                .and_then(|m| m.get("role"))
                .and_then(|v| v.as_str())
                .unwrap_or("assistant")
                .to_string();

            let content_text = msg
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let tool_calls = msg
                .get("message")
                .and_then(|m| m.get("tool_calls"))
                .and_then(|v| v.as_array());

            // Build content array for the output message
            let mut content_parts: Vec<Value> = Vec::new();

            if let Some(ref text) = content_text {
                if !text.is_empty() {
                    content_parts.push(json!({"type": "output_text", "text": text}));
                }
            }

            if let Some(tcs) = tool_calls {
                for tc in tcs {
                    output.push(json!({
                        "type": "function_call",
                        "call_id": tc.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": tc.get("function").and_then(|f| f.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                        "arguments": tc.get("function").and_then(|f| f.get("arguments")).and_then(|v| v.as_str()).unwrap_or(""),
                    }));
                }
            }

            if !content_parts.is_empty() {
                output.push(json!({
                    "type": "message",
                    "role": role,
                    "content": content_parts,
                }));
            } else if tool_calls.is_none() || tool_calls.unwrap().is_empty() {
                // No content and no tool_calls → empty assistant output
                output.push(json!({
                    "type": "message",
                    "role": role,
                    "content": [{"type": "output_text", "text": ""}],
                }));
            }
        }

        // Extract usage and rename fields
        let usage = obj.get("usage");
        let usage_out = usage.map(|u| {
            json!({
                "input_tokens": u.get("prompt_tokens").cloned().unwrap_or(json!(0)),
                "output_tokens": u.get("completion_tokens").cloned().unwrap_or(json!(0)),
                "total_tokens": u.get("total_tokens").cloned().unwrap_or(json!(0)),
            })
        });

        let finish_reason = message
            .and_then(|m| m.get("finish_reason"))
            .and_then(|v| v.as_str());
        let status = match finish_reason {
            Some("stop") | Some("tool_calls") => "completed",
            Some("length") => "completed", // max_tokens → completed (no error)
            Some(_) => "completed",
            None => {
                if output.is_empty() {
                    "failed"
                } else {
                    "completed"
                }
            }
        };

        let mut result = serde_json::Map::new();
        result.insert("id".to_string(), json!(id));
        result.insert("object".to_string(), json!("response"));
        result.insert("status".to_string(), json!(status));
        result.insert("model".to_string(), json!(model));
        result.insert("output".to_string(), Value::Array(output));
        if let Some(u) = usage_out {
            result.insert("usage".to_string(), u);
        }

        Ok(Value::Object(result))
    }

    fn stream_adapter(&self) -> Option<Box<dyn StreamAdapter>> {
        Some(Box::new(ResponsesToChatCompletionsStream::new()))
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// ResponsesToChatCompletionsStream
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
//
// Converts Chat Completions SSE delta chunks → Responses API SSE events.
//
// State machine:
//   First chunk (role)         → response.created
//   delta.content              → response.output_text.delta
//   delta.tool_calls           → response.function_call_arguments.delta
//   finish_reason + usage      → response.completed
//   [DONE]                     → data: [DONE]

struct ResponsesToChatCompletionsStream {
    response_id: String,
    model: String,
    created_sent: bool,
    done: bool,
    content_index: usize,
    pending_usage: Option<Value>,
    tool_call_buf: Vec<ToolCallState>,
}

struct ToolCallState {
    call_id: String,
    name: String,
    arguments: String,
    done: bool,
}

impl ResponsesToChatCompletionsStream {
    fn new() -> Self {
        Self {
            response_id: format!("resp_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            model: String::new(),
            created_sent: false,
            done: false,
            content_index: 0,
            pending_usage: None,
            tool_call_buf: Vec::new(),
        }
    }

    fn emit_sse(&self, event: &str, data: &str) -> Vec<u8> {
        format!("event: {}\ndata: {}\n\n", event, data).into_bytes()
    }
}

impl StreamAdapter for ResponsesToChatCompletionsStream {
    fn next(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        if self.done {
            return None;
        }

        let text = String::from_utf8_lossy(chunk);
        let mut out = Vec::new();

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let data = line
                .strip_prefix("data: ")
                .or_else(|| line.strip_prefix("data:"))
                .unwrap_or(line);
            if data == "[DONE]" {
                self.done = true;
                out.extend_from_slice(b"data: [DONE]\n\n");
                return if out.is_empty() { None } else { Some(out) };
            }

            let chunk_val: Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Capture model from the first chunk
            if self.model.is_empty() {
                if let Some(m) = chunk_val.get("model").and_then(|v| v.as_str()) {
                    self.model = m.to_string();
                }
            }

            // Emit response.created on first non-empty chunk
            if !self.created_sent {
                self.created_sent = true;
                let created_event = json!({
                    "response": {
                        "id": self.response_id,
                        "object": "response",
                        "status": "in_progress",
                        "model": self.model,
                        "output": []
                    }
                });
                out.extend_from_slice(&self.emit_sse(
                    "response.created",
                    &serde_json::to_string(&created_event).unwrap(),
                ));
            }

            let choices = chunk_val.get("choices").and_then(|v| v.as_array());
            let usage = chunk_val.get("usage");

            if let Some(choices) = choices {
                for choice in choices {
                    let delta = choice.get("delta");

                    // Text content
                    if let Some(content) = delta
                        .and_then(|d| d.get("content"))
                        .and_then(|v| v.as_str())
                    {
                        if !content.is_empty() {
                            let idx = self.content_index;
                            self.content_index += 1;
                            let text_delta = json!({
                                "delta": content,
                                "content_index": idx,
                                "output_index": 0
                            });
                            out.extend_from_slice(&self.emit_sse(
                                "response.output_text.delta",
                                &serde_json::to_string(&text_delta).unwrap(),
                            ));
                        }
                    }

                    // Tool calls
                    if let Some(tool_calls) = delta
                        .and_then(|d| d.get("tool_calls"))
                        .and_then(|v| v.as_array())
                    {
                        for tc in tool_calls {
                            let idx =
                                tc.get("index").and_then(|v| v.as_i64()).unwrap_or(0) as usize;
                            let tc_id =
                                tc.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
                            let tc_name = tc
                                .get("function")
                                .and_then(|f| f.get("name"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let tc_args = tc
                                .get("function")
                                .and_then(|f| f.get("arguments"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            // Ensure the tool-call slot exists (id/name may be
                            // null on argument-delta chunks — OpenAI streams
                            // id+name on the first chunk, then args-only).
                            while self.tool_call_buf.len() <= idx {
                                self.tool_call_buf.push(ToolCallState {
                                    call_id: String::new(),
                                    name: String::new(),
                                    arguments: String::new(),
                                    done: false,
                                });
                            }
                            let buf = &mut self.tool_call_buf[idx];

                            // New tool call (id + name present) → register
                            if let (Some(id), Some(name)) = (tc_id, tc_name) {
                                if !buf.done && buf.call_id.is_empty() {
                                    buf.call_id = id;
                                    buf.name = name;
                                }
                            }
                            // Argument delta (may arrive with null id/name —
                            // use the stored call_id from the first chunk)
                            if !tc_args.is_empty() {
                                buf.arguments.push_str(tc_args);
                                let arg_delta = json!({
                                    "delta": tc_args,
                                    "call_id": buf.call_id,
                                    "output_index": idx
                                });
                                out.extend_from_slice(&self.emit_sse(
                                    "response.function_call_arguments.delta",
                                    &serde_json::to_string(&arg_delta).unwrap(),
                                ));
                            }
                        }
                    }

                    // Capture finish_reason + usage → emit in finish()
                    if choice.get("finish_reason").is_some() || usage.is_some() {
                        self.pending_usage = usage.cloned();
                    }
                }
            }
        }

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn finish(&mut self) -> Option<Vec<u8>> {
        let mut out = Vec::new();

        // Send function_call_arguments.done for each tool call
        for (idx, tc) in self.tool_call_buf.iter().enumerate() {
            if !tc.done && !tc.call_id.is_empty() {
                let done_event = json!({
                    "call_id": tc.call_id,
                    "name": tc.name,
                    "arguments": tc.arguments,
                    "output_index": idx
                });
                out.extend_from_slice(&self.emit_sse(
                    "response.function_call_arguments.done",
                    &serde_json::to_string(&done_event).unwrap(),
                ));
            }
        }

        // Emit response.completed with usage
        let usage = self.pending_usage.take().unwrap_or(json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0
        }));
        let completed = json!({
            "response": {
                "id": self.response_id,
                "object": "response",
                "status": "completed",
                "model": self.model,
                "usage": {
                    "input_tokens": usage.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                    "output_tokens": usage.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                    "total_tokens": usage.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
                }
            }
        });
        out.extend_from_slice(&self.emit_sse(
            "response.completed",
            &serde_json::to_string(&completed).unwrap(),
        ));

        // [DONE]
        out.extend_from_slice(b"data: [DONE]\n\n");

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deployment::ProviderType;
    use crate::models::{ChatContent, ChatMessage, TokenDetails};

    fn make_openai_req(text: &str) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Text(text.to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            }],
            stream: false,
            temperature: Some(0.7),
            max_tokens: Some(1024),
            top_p: None,
            frequency_penalty: None,
            presence_penalty: None,
            stop: None,
            user: None,
            tools: None,
            tool_choice: None,
            response_format: None,
            reasoning_effort: None,
        }
    }

    fn test_deployment() -> Deployment {
        Deployment {
            api_base: "https://api.openai.com/v1".into(),
            api_key: None,
            upstream_model: "gpt-4".into(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({"custom_llm_provider": "openai"}),
            model_id: Some("test-model-id".into()),
            model_group: Some("gpt-4".into()),
            custom_llm_provider: Some("openai".into()),
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        }
    }

    // ── MessageAdapter tests ──

    #[test]
    fn test_openai_passthrough_swaps_model() {
        let body = json!({"model": "gpt-4", "messages": [{"role": "user", "content": "Hello"}]});
        let adapted = OpenAIPassthrough
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        assert!(
            select_adapter(ClientProtocol::Anthropic, &ProviderType::AnthropicNative).is_some()
        );
        assert!(select_adapter(ClientProtocol::OpenAI, &ProviderType::AnthropicNative).is_some());
    }

    // ── Tool conversion tests ──

    #[test]
    fn test_anthropic_to_openai_tool_use_to_tool_calls() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 1024,
            "messages": [{"role": "assistant", "content": [{"type": "tool_use", "id": "toolu_01", "name": "get_weather", "input": {"city": "NYC"}}]}]
        });
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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

    // ── Stage 102 Responses→Chat bridge — adapter-level UT (Phase 41 test gap ①) ──

    #[test]
    fn test_responses_to_chat_adapt_request_string_input() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "input": "Hello from Responses",
            "instructions": "Be helpful",
            "max_output_tokens": 128,
            "reasoning": {"effort": "low"},
            "stream": true
        });
        let dep = Deployment {
            api_base: "https://api.openai.com/v1".to_string(),
            api_key: Some("sk-test".to_string()),
            upstream_model: "gpt-4o-upstream".to_string(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: serde_json::json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        };
        let adapted = ResponsesToChatCompletions
            .adapt_request(body, &dep)
            .expect("adapt_request");
        // instructions → prepended system message (index 0)
        assert_eq!(
            adapted["messages"][0]["role"].as_str(),
            Some("system"),
            "instructions should become system message"
        );
        assert_eq!(
            adapted["messages"][0]["content"].as_str(),
            Some("Be helpful")
        );
        // input string → single user message (index 1)
        assert_eq!(
            adapted["messages"][1]["role"].as_str(),
            Some("user"),
            "string input should become a user message"
        );
        assert_eq!(
            adapted["messages"][1]["content"].as_str(),
            Some("Hello from Responses")
        );
        // max_output_tokens → max_tokens
        assert_eq!(adapted["max_tokens"].as_i64(), Some(128));
        assert!(adapted.get("max_output_tokens").is_none());
        // unsupported reasoning dropped
        assert!(adapted.get("reasoning").is_none());
        // model rewritten to upstream
        assert_eq!(adapted["model"].as_str(), Some("gpt-4o-upstream"));
        // stream_options injected for streaming
        assert!(adapted["stream_options"]["include_usage"].as_bool() == Some(true));
    }

    #[test]
    fn test_responses_to_chat_adapt_request_array_input() {
        let body = serde_json::json!({
            "model": "gpt-4o",
            "input": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "hello"}
            ]
        });
        let dep = Deployment {
            api_base: "https://api.openai.com/v1".to_string(),
            api_key: None,
            upstream_model: "gpt-4o".to_string(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: serde_json::json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        };
        let adapted = ResponsesToChatCompletions
            .adapt_request(body, &dep)
            .expect("adapt_request");
        let msgs = adapted["messages"].as_array().expect("messages array");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"].as_str(), Some("user"));
        assert_eq!(msgs[1]["role"].as_str(), Some("assistant"));
    }

    #[test]
    fn test_responses_to_chat_adapt_request_rejects_empty_array() {
        let body = serde_json::json!({"model": "gpt-4o", "input": []});
        let dep = Deployment {
            api_base: "https://api.openai.com/v1".to_string(),
            api_key: None,
            upstream_model: "gpt-4o".to_string(),
            provider_type: ProviderType::OpenAICompatible,
            input_cost_per_token: None,
            output_cost_per_token: None,
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: serde_json::json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
        };
        let err = ResponsesToChatCompletions
            .adapt_request(body, &dep)
            .expect_err("empty input should be rejected");
        assert!(matches!(err, AdapterError::Unsupported(_)));
    }

    #[test]
    fn test_responses_to_chat_adapt_response_non_stream() {
        let upstream = serde_json::json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from the model",
                    "tool_calls": [{
                        "id": "call_001",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });
        let adapted = ResponsesToChatCompletions
            .adapt_response(upstream)
            .expect("adapt_response");
        assert_eq!(adapted["object"].as_str(), Some("response"));
        assert_eq!(adapted["status"].as_str(), Some("completed"));
        // text output as output_text block
        let output = adapted["output"].as_array().expect("output array");
        let msg = output
            .iter()
            .find(|o| o["type"] == "message")
            .expect("message");
        assert_eq!(msg["content"][0]["type"].as_str(), Some("output_text"));
        assert_eq!(
            msg["content"][0]["text"].as_str(),
            Some("Hello from the model")
        );
        // tool call → function_call output
        let fc = output
            .iter()
            .find(|o| o["type"] == "function_call")
            .expect("function_call");
        assert_eq!(fc["call_id"].as_str(), Some("call_001"));
        assert_eq!(fc["name"].as_str(), Some("get_weather"));
        assert_eq!(fc["arguments"].as_str(), Some("{\"city\":\"NYC\"}"));
        // usage renamed prompt_tokens → input_tokens
        assert_eq!(adapted["usage"]["input_tokens"].as_i64(), Some(10));
        assert_eq!(adapted["usage"]["output_tokens"].as_i64(), Some(5));
        assert_eq!(adapted["usage"]["total_tokens"].as_i64(), Some(15));
    }

    #[test]
    fn test_responses_to_chat_stream_next_text_delta() {
        let mut stream = ResponsesToChatCompletionsStream::new();
        // First chunk: role → response.created
        let first = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}",
        );
        let out = String::from_utf8(first.expect("created event")).unwrap();
        assert!(
            out.contains("event: response.created"),
            "first chunk should emit response.created: {}",
            out
        );
        // Text delta chunk → output_text.delta
        let delta = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}",
        );
        let out = String::from_utf8(delta.expect("text delta event")).unwrap();
        assert!(
            out.contains("event: response.output_text.delta"),
            "text delta should emit output_text.delta: {}",
            out
        );
        assert!(out.contains("\"delta\":\"Hello\""));
        assert!(out.contains("\"content_index\":0"));
    }

    #[test]
    fn test_responses_to_chat_stream_finish_completed() {
        let mut stream = ResponsesToChatCompletionsStream::new();
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}",
        );
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}",
        );
        // finish_reason + usage chunk
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2,\"total_tokens\":5}}",
        );
        let finished = stream.finish().expect("finish event");
        let out = String::from_utf8(finished).unwrap();
        assert!(
            out.contains("event: response.completed"),
            "finish should emit response.completed: {}",
            out
        );
        // usage mapped prompt_tokens → input_tokens
        assert!(out.contains("\"input_tokens\":3"), "got: {}", out);
        assert!(out.contains("\"output_tokens\":2"), "got: {}", out);
        assert!(out.contains("data: [DONE]"), "got: {}", out);
    }

    #[test]
    fn test_responses_to_chat_stream_tool_call_delta() {
        let mut stream = ResponsesToChatCompletionsStream::new();
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}",
        );
        // Tool call start chunk
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_001\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}",
        );
        // Tool call arguments delta
        let arg_chunk = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":null,\"type\":null,\"function\":{\"name\":null,\"arguments\":\"{\\\"city\\\":\\\"NYC\\\"}\"}}]},\"finish_reason\":null}]}",
        );
        let out = String::from_utf8(arg_chunk.expect("tool call delta event")).unwrap();
        assert!(
            out.contains("event: response.function_call_arguments.delta"),
            "tool arg delta should emit function_call_arguments.delta: {}",
            out
        );
        assert!(out.contains("\"call_id\":\"call_001\""));
        // finish() → function_call_arguments.done + response.completed
        let finished = stream.finish().expect("finish event");
        let out = String::from_utf8(finished).unwrap();
        assert!(
            out.contains("event: response.function_call_arguments.done"),
            "tool finish should emit done: {}",
            out
        );
        assert!(out.contains("\"name\":\"get_weather\""));
        assert!(out.contains("\"arguments\":\"{\\\"city\\\":\\\"NYC\\\"}\""));
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
        req.messages.insert(
            0,
            ChatMessage {
                role: "system".to_string(),
                content: ChatContent::Text("Helpful".to_string()),
                name: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
        );
        let c = DefaultAdapter::openai_to_claude_request(&req, 512);
        assert!(c.system.is_some());
    }

    #[test]
    fn test_claude_to_openai_response() {
        let cr = ClaudeMessageResponse {
            id: "1".into(),
            response_type: "message".into(),
            role: "assistant".into(),
            content: vec![ClaudeContentBlock {
                content_type: "text".into(),
                text: Some("Hi".into()),
                source: None,
                id: None,
                name: None,
                input: None,
                tool_use_id: None,
                content: None,
                thinking: None,
                signature: None,
                citations: None,
            }],
            model: "claude-sonnet".into(),
            stop_reason: Some("end_turn".into()),
            stop_sequence: None,
            usage: ClaudeUsage {
                input_tokens: 1,
                output_tokens: 1,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            },
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // Two tool_results → 2 tool messages
        assert_eq!(
            msgs.len(),
            2,
            "expected 2 tool messages, got {}",
            msgs.len()
        );
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        // Verify all three tool_call_ids are present and distinct
        let ids: Vec<&str> = msgs
            .iter()
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        // tool message MUST come before user text (OpenAI protocol:
        // tool messages must immediately follow the assistant tool_calls).
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[0]["tool_call_id"].as_str(), Some("toolu_01"));
        assert_eq!(msgs[1]["role"].as_str(), Some("user"));
        assert_eq!(
            msgs[1]["content"].as_array().unwrap()[0]["text"].as_str(),
            Some("here is the result")
        );
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"].as_str(), Some("user"));
        assert_eq!(
            msgs[0]["content"].as_array().unwrap()[0]["text"].as_str(),
            Some("hello world")
        );
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
        let adapted = AnthropicToOpenAI
            .adapt_request(body, &test_deployment())
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        let assistant = msgs.iter().find(|m| m["role"] == "assistant").unwrap();
        let content = assistant["content"].as_array().unwrap();
        // Must not contain a {"type":"text"} without text field
        for part in content {
            if part["type"] == "text" {
                assert!(
                    part.get("text").and_then(|v| v.as_str()).is_some(),
                    "text ContentPart must have a non-null text field: {}",
                    part
                );
            }
        }
        assert!(
            assistant["tool_calls"].as_array().unwrap().len() > 0,
            "tool_calls must be present"
        );
    }

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Stage 60: System Message Normalization tests
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    fn make_system_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content: ChatContent::Text(text.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    fn make_user_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Text(text.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    fn make_user_parts(parts: Vec<ContentPart>) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: ChatContent::Parts(parts),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    fn make_assistant_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Text(text.to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
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
        assert_eq!(
            folded.len(),
            2,
            "expected 2 messages after fold: system + user"
        );
        assert_eq!(folded[0].role, "system");
        assert_eq!(folded[1].role, "user");
        // The user should now contain the system-reminder
        match &folded[1].content {
            ChatContent::Parts(parts) => {
                assert!(
                    parts.iter().any(|p| p
                        .text
                        .as_deref()
                        .unwrap_or("")
                        .contains("<system-reminder>")),
                    "user Parts should contain <system-reminder>"
                );
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
        assert_eq!(
            folded.len(),
            4,
            "expected 4 messages after fold: u1, a1, u2+reminder, u3+reminders"
        );
        assert_eq!(folded[0].role, "user");
        assert_eq!(folded[1].role, "assistant");
        // u2 now contains s1
        match &folded[2].content {
            ChatContent::Text(t) => {
                assert!(
                    t.contains("<system-reminder>"),
                    "u2 should contain s1 reminder, got: {}",
                    t
                );
                assert!(t.contains("system 1"), "u2 should contain s1 content");
                assert!(
                    t.contains("second question"),
                    "original user text preserved"
                );
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
                assert!(
                    t.contains("<system-reminder>"),
                    "u2 should contain reminder"
                );
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
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({"custom_llm_provider": "openai"}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: Some("loose".to_string()),
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
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
        let systems: Vec<&str> = msgs
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        // Loose → passthrough, systems should exist at multiple positions
        let system_count = systems.iter().filter(|r| **r == "system").count();
        assert!(
            system_count > 1,
            "Loose mode should preserve extra systems, got {} system messages",
            system_count
        );
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
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
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
            cache_read_input_token_cost: None,
            cache_creation_input_token_cost: None,
            raw_params: json!({}),
            model_id: None,
            model_group: None,
            custom_llm_provider: None,
            chat_template_compat: Some("loose".to_string()),
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
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
            cache_read_input_token_cost: Some(0.0000003),
            cache_creation_input_token_cost: Some(0.00000375),
            raw_params: json!({"custom_llm_provider": "anthropic"}),
            model_id: Some("anthro-001".into()),
            model_group: Some("claude-sonnet-4".into()),
            custom_llm_provider: Some("anthropic".into()),
            chat_template_compat: None,
            modal_pricing: None,
            weight: None,
            rpm: None,
            tpm: None,
            priority: None,
            fail_count: 0,
            cooldown_until: None,
            last_latency_ms: 0.0,
            oauth: None,
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
        let adapted = AnthropicPassthrough
            .adapt_request(body, &anthropic_deployment())
            .unwrap();
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
        let adapted = AnthropicPassthrough
            .adapt_request(body, &anthropic_deployment())
            .unwrap();
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
        let adapted = OpenAIToAnthropic
            .adapt_request(body, &anthropic_deployment())
            .unwrap();
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
        assert_eq!(
            choices[0]["message"]["content"].as_str(),
            Some("Rust is great!")
        );
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
        let adapted = OpenAIToAnthropic
            .adapt_request(body, &anthropic_deployment())
            .unwrap();
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
        assert!(
            s1.contains("event: message_start"),
            "expected message_start, got: {}",
            s1
        );

        // Second chunk: text content
        let result2 = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}"
        );
        assert!(result2.is_some());
        let s2_buf = result2.unwrap();
        let s2 = String::from_utf8_lossy(&s2_buf);
        assert!(
            s2.contains("content_block_start"),
            "expected content_block_start, got: {}",
            s2
        );
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
        assert!(
            s.contains("content_block_start"),
            "expected content_block_start, got: {}",
            s
        );
        assert!(
            s.contains("tool_use"),
            "expected tool_use block, got: {}",
            s
        );
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
        assert!(
            s.contains("content_block_stop") || s.contains("message_stop"),
            "expected stop events, got: {}",
            s
        );
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
    // Stage 120: GLM5 首帧 tool_use id + arguments 同帧丢帧回归
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    /// Stage 120 — AnthropicToOpenAIStream: 首帧同时含 tool_call id 和 arguments="{\""
    /// (tokenhub GLM-5.2 逐 token 增量模式的实际表现).
    /// 修复前: emit content_block_start 后 early-return, 丢弃 arguments="{\"".
    /// 修复后: 同帧必须返回 content_block_start + input_json_delta 两个事件.
    #[test]
    fn test_stage120_glm5_first_chunk_id_and_args() {
        let mut stream = AnthropicToOpenAIStream::new();
        // 先送 assistant role 头, 触发 message_start
        let _ = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
        );
        // 首帧: id + arguments="{\"" 同帧(GLM-5.2 tokenhub 行为)
        let result = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_glm5\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"\"}}]}}]}"
        );
        assert!(
            result.is_some(),
            "first tool_call chunk must produce SSE output"
        );
        let buf = result.unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(
            s.contains("event: content_block_start"),
            "expected content_block_start, got: {}",
            s
        );
        assert!(
            s.contains("\"type\":\"tool_use\""),
            "expected tool_use block, got: {}",
            s
        );
        assert!(s.contains("call_glm5"), "expected tool_call id, got: {}", s);
        assert!(
            s.contains("event: content_block_delta"),
            "expected content_block_delta with input_json_delta in same chunk, got: {}",
            s
        );
        assert!(
            s.contains("\"type\":\"input_json_delta\""),
            "expected input_json_delta, got: {}",
            s
        );
        assert!(
            s.contains("\"partial_json\":\"{\\\"\""),
            "expected partial_json '{{\"' NOT dropped, got: {}",
            s
        );
    }

    /// Stage 120 — OpenAIToAnthropicStream: 对称场景.
    /// 修复前同一 early-return bug; 修复后同帧必须 emit content_block_start + input_json_delta.
    #[test]
    fn test_stage120_glm5_reverse_first_chunk_id_and_args() {
        let mut stream = OpenAIToAnthropicStream::new();
        // Start
        let _ = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
        );
        // First tool_call chunk with id + non-empty arguments
        let result = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_r\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"{\\\"\"}}]}}]}"
        );
        assert!(
            result.is_some(),
            "reverse first chunk must produce SSE output"
        );
        let buf = result.unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(
            s.contains("event: content_block_start"),
            "expected content_block_start, got: {}",
            s
        );
        assert!(s.contains("call_r"), "expected id call_r, got: {}", s);
        assert!(
            s.contains("event: content_block_delta"),
            "expected content_block_delta same chunk, got: {}",
            s
        );
        assert!(
            s.contains("\"partial_json\":\"{\\\"\""),
            "expected partial_json '{{\"' NOT dropped, got: {}",
            s
        );
    }

    /// Stage 120 — 后续多个纯 arguments 增量帧顺序透传, 无遗漏.
    /// 覆盖 GLM-5.2 后续逐 token 增量场景, 确认修复不影响后续帧语义.
    #[test]
    fn test_stage120_multiple_arg_frags_accumulate() {
        let mut stream = AnthropicToOpenAIStream::new();
        let _ = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}"
        );
        // 首帧带 id, 空 arguments (MAAS 行为), 只 emit content_block_start
        let _ = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"\"}}]}}]}"
        );
        // 后续三个纯 arguments 增量帧 — 每帧独立 emit
        let r1 = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"\"}}]}}]}"
        );
        let r2 = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"cmd\"}}]}}]}"
        );
        let r3 = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"glm-5\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\":\\\"ls\\\"}\"}}]}}]}"
        );
        for (i, r) in [&r1, &r2, &r3].iter().enumerate() {
            assert!(r.is_some(), "arg frag {} must emit event", i);
            let s = String::from_utf8_lossy(r.as_ref().unwrap());
            assert!(
                s.contains("\"type\":\"input_json_delta\""),
                "arg frag {} expected input_json_delta, got: {}",
                i,
                s
            );
        }
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
        assert_eq!(
            strip_system_reminder("<system-reminder></system-reminder>"),
            None
        ); // empty
    }

    // UT-HF1: extract_and_merge_system_reminders — from user with text + reminder
    #[test]
    fn test_hf_extract_system_reminders_basic() {
        let messages = vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeContent::Blocks(vec![
                ClaudeContentBlock {
                    content_type: "text".to_string(),
                    text: Some("<system-reminder>\nagent list\n</system-reminder>".to_string()),
                    source: None,
                    id: None,
                    name: None,
                    input: None,
                    tool_use_id: None,
                    content: None,
                    thinking: None,
                    signature: None,
                    citations: None,
                },
                ClaudeContentBlock {
                    content_type: "text".to_string(),
                    text: Some("actual query".to_string()),
                    source: None,
                    id: None,
                    name: None,
                    input: None,
                    tool_use_id: None,
                    content: None,
                    thinking: None,
                    signature: None,
                    citations: None,
                },
            ]),
        }];
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
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text(
                    "<system-reminder>\ncontext\n</system-reminder>".to_string(),
                ),
            },
            ClaudeMessage {
                role: "assistant".to_string(),
                content: ClaudeContent::Text("ok".to_string()),
            },
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
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text("check hostname".to_string()),
            },
            ClaudeMessage {
                role: "assistant".to_string(),
                content: ClaudeContent::Text("ok".to_string()),
            },
            ClaudeMessage {
                role: "system".to_string(),
                content: ClaudeContent::Text("Extra system context".to_string()),
            },
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text("do next task".to_string()),
            },
        ];
        let (cleaned, extra) = extract_and_merge_system_reminders(messages);
        assert_eq!(
            extra.len(),
            1,
            "role=system should be extracted, got {:?}",
            extra
        );
        assert_eq!(extra[0], "Extra system context");
        // No role="system" in output
        for msg in &cleaned {
            assert!(
                matches!(msg.role.as_str(), "user" | "assistant"),
                "role should not be 'system': {}",
                msg.role
            );
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
                    ClaudeContentBlock {
                        content_type: "text".to_string(),
                        text: Some("sys block A".to_string()),
                        source: None,
                        id: None,
                        name: None,
                        input: None,
                        tool_use_id: None,
                        content: None,
                        thinking: None,
                        signature: None,
                        citations: None,
                    },
                    ClaudeContentBlock {
                        content_type: "text".to_string(),
                        text: Some("sys block B".to_string()),
                        source: None,
                        id: None,
                        name: None,
                        input: None,
                        tool_use_id: None,
                        content: None,
                        thinking: None,
                        signature: None,
                        citations: None,
                    },
                ]),
            },
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeContent::Text("query".to_string()),
            },
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
        let adapted = AnthropicPassthrough
            .adapt_request(body, &make_strict_deployment())
            .unwrap();

        // System must contain both original + extracted reminders
        let sys = adapted["system"].as_str().unwrap();
        assert!(
            sys.contains("You are Claude Code"),
            "missing original system: {}",
            sys
        );
        assert!(
            sys.contains("Available agent types"),
            "missing extracted reminder: {}",
            sys
        );

        // The last user message must NOT contain system-reminder tags
        let msgs = adapted["messages"].as_array().unwrap();
        let last_content = &msgs.last().unwrap()["content"];
        if let Some(t) = last_content.as_str() {
            assert!(
                !t.contains("system-reminder"),
                "user text still has reminder: {}",
                t
            );
        } else if let Some(arr) = last_content.as_array() {
            for b in arr {
                if let Some(t) = b["text"].as_str() {
                    assert!(
                        !t.contains("system-reminder"),
                        "user block still has reminder: {}",
                        t
                    );
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
        let deployment = Deployment {
            chat_template_compat: Some("loose".to_string()),
            ..make_strict_deployment()
        };
        let adapted = AnthropicPassthrough
            .adapt_request(body, &deployment)
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(
            adapted["system"].is_null(),
            "no system field should be added"
        );
    }

    // UT-HF5: non-qwen upstream — passthrough
    #[test]
    fn test_hf_passthrough_non_qwen_passthrough() {
        let body = json!({
            "model": "claude-sonnet", "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "<system-reminder>\nctx\n</system-reminder>"}]}]
        });
        let deployment = Deployment {
            upstream_model: "gpt-4".into(),
            ..make_strict_deployment()
        };
        let adapted = AnthropicPassthrough
            .adapt_request(body, &deployment)
            .unwrap();
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
        let adapted = AnthropicPassthrough
            .adapt_request(body, &make_strict_deployment())
            .unwrap();
        let msgs = adapted["messages"].as_array().unwrap();

        // Must NOT contain role="tool" (Anthropic protocol rejects it)
        for msg in msgs {
            let role = msg["role"].as_str().unwrap();
            assert!(
                matches!(role, "user" | "assistant"),
                "illegal role in Anthropic body: {}",
                role
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

    // ── reasoning_content round-trip tests ──

    #[test]
    fn test_reasoning_content_roundtrip_assistant_message() {
        let oai_resp = ChatCompletionResponse {
            id: "chatcmpl-001".to_string(),
            object: "chat.completion".to_string(),
            created: 1234567890,
            model: "deepseek-v4-flash".to_string(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".to_string(),
                    content: "Let me think...".to_string(),
                    tool_calls: None,
                    reasoning_content: Some("analyzing step by step".to_string()),
                    refusal: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Usage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            },
            system_fingerprint: None,
        };

        let claude_resp = oai_response_to_claude_messages(&oai_resp);

        // Build a ClaudeMessageRequest from the response for round-trip testing
        // ClaudeMessageResponse(role="assistant", content=[...]) -> ClaudeMessageRequest(messages=[ClaudeMessage{..}])
        let claude_req = ClaudeMessageRequest {
            model: claude_resp.model.clone(),
            max_tokens: 1024,
            messages: vec![ClaudeMessage {
                role: claude_resp.role,
                content: ClaudeContent::Blocks(claude_resp.content),
            }],
            stream: None,
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            metadata: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };
        let oai_req = DefaultAdapter::claude_to_openai_request(&claude_req);

        let assistant_msg = oai_req
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .unwrap();
        assert_eq!(
            assistant_msg.reasoning_content.as_deref(),
            Some("analyzing step by step"),
            "reasoning_content should survive round-trip"
        );
    }

    #[test]
    fn test_delta_reasoning_content_deserialization() {
        let chunk_json = json!({
            "id": "chatcmpl-001",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "deepseek-v4-flash",
            "choices": [{
                "index": 0,
                "delta": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "Let me analyze..."
                },
                "finish_reason": null
            }]
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(chunk_json).unwrap();
        let delta = &chunk.choices[0].delta;
        assert_eq!(
            delta.reasoning_content.as_deref(),
            Some("Let me analyze...")
        );
    }

    #[test]
    fn test_chat_message_reasoning_content_serialization() {
        let msg = ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Text("Hello".to_string()),
            name: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: Some("Deep thinking...".to_string()),
        };
        let json_val = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            json_val["reasoning_content"].as_str(),
            Some("Deep thinking...")
        );
    }

    #[test]
    fn test_usage_details_serialization() {
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            prompt_tokens_details: Some(TokenDetails {
                cached_tokens: Some(80),
                reasoning_tokens: None,
                audio_tokens: None,
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
            }),
            completion_tokens_details: Some(TokenDetails {
                cached_tokens: None,
                reasoning_tokens: Some(20),
                audio_tokens: None,
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
            }),
        };
        let json_val = serde_json::to_value(&usage).unwrap();
        let pt = json_val["prompt_tokens_details"].as_object().unwrap();
        assert_eq!(pt["cached_tokens"].as_i64(), Some(80));
        let ct = json_val["completion_tokens_details"].as_object().unwrap();
        assert_eq!(ct["reasoning_tokens"].as_i64(), Some(20));
    }

    #[test]
    fn test_chunk_usage_deserialization() {
        let chunk_json = json!({
            "id": "chatcmpl-001",
            "object": "chat.completion.chunk",
            "created": 1234567890,
            "model": "deepseek-v4-flash",
            "choices": [],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        });
        let chunk: ChatCompletionChunk = serde_json::from_value(chunk_json).unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn test_claude_content_block_thinking_deserialization() {
        let block_json = json!({
            "type": "thinking",
            "thinking": "Let me analyze this problem...",
            "signature": "abc123"
        });
        let block: ClaudeContentBlock = serde_json::from_value(block_json).unwrap();
        assert_eq!(block.content_type, "thinking");
        assert_eq!(
            block.thinking.as_deref(),
            Some("Let me analyze this problem...")
        );
        assert_eq!(block.signature.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_claude_usage_cache_tokens() {
        let usage = ClaudeUsage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_input_tokens: Some(800),
            cache_creation_input_tokens: Some(200),
        };
        let json_val = serde_json::to_value(&usage).unwrap();
        assert_eq!(json_val["cache_read_input_tokens"].as_i64(), Some(800));
        assert_eq!(json_val["cache_creation_input_tokens"].as_i64(), Some(200));
    }

    #[test]
    fn test_thinking_param_in_claude_request() {
        let req_json = json!({
            "model": "claude-sonnet",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}],
            "thinking": {"type": "enabled", "budget_tokens": 4000}
        });
        let req: ClaudeMessageRequest = serde_json::from_value(req_json).unwrap();
        let thinking = req.thinking.unwrap();
        assert_eq!(thinking["type"].as_str(), Some("enabled"));
        assert_eq!(thinking["budget_tokens"].as_i64(), Some(4000));
    }

    // ── Stage 103: multimodal image data-URL parsing ──

    #[test]
    fn test_parse_data_url_png() {
        let (media_type, data) = parse_data_url("data:image/png;base64,iVBORw0KGgo=");
        assert_eq!(media_type, "image/png");
        assert_eq!(data, "iVBORw0KGgo=");
    }

    #[test]
    fn test_parse_data_url_jpeg_with_params() {
        // MIME segment may carry parameters after `;base64` — take the first segment
        let (media_type, data) = parse_data_url("data:image/jpeg;charset=utf-8;base64,/9j/4AAQ==");
        assert_eq!(media_type, "image/jpeg");
        assert_eq!(data, "/9j/4AAQ==");
    }

    #[test]
    fn test_parse_data_url_malformed_falls_back() {
        // No comma / non-data prefix → media_type fallback image/png, data kept verbatim
        let (media_type, data) = parse_data_url("not-a-data-url");
        assert_eq!(media_type, "image/png");
        assert_eq!(data, "not-a-data-url");
        let (media_type2, data2) = parse_data_url("data:image/png");
        assert_eq!(media_type2, "image/png");
        assert_eq!(data2, "data:image/png");
    }

    #[test]
    fn test_openai_to_claude_image_strips_data_prefix() {
        // OpenAI image_url `data:image/webp;base64,UklGR...` → Claude image block
        // with pure base64 data + correct media_type (NOT the full data URL, NOT
        // a hardcoded image/jpeg).
        let msg = make_user_parts(vec![ContentPart {
            content_type: "image_url".to_string(),
            text: None,
            image_url: Some(ImageUrl {
                url: "data:image/webp;base64,UklGRlNvbWVEYXRh".to_string(),
            }),
        }]);
        let claude = openai_message_to_claude(&msg);
        let blocks = match &claude.content {
            ClaudeContent::Blocks(b) => b,
            _ => panic!("expected content blocks"),
        };
        let block = &blocks[0];
        assert_eq!(block.content_type, "image");
        let source = block.source.as_ref().expect("image source");
        assert_eq!(source.source_type, "base64");
        assert_eq!(source.media_type, "image/webp");
        assert_eq!(source.data, "UklGRlNvbWVEYXRh");
    }

    #[test]
    fn test_claude_to_openai_image_reconstructs_data_url() {
        // Claude image block {source: {type:base64, media_type, data}} →
        // OpenAI image_url `data:{media_type};base64,{data}`.
        let msg = ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeContent::Blocks(vec![ClaudeContentBlock {
                content_type: "image".to_string(),
                text: None,
                source: Some(ClaudeImageSource {
                    source_type: "base64".to_string(),
                    media_type: "image/png".to_string(),
                    data: "iVBORw0KGgo=".to_string(),
                }),
                id: None,
                name: None,
                input: None,
                tool_use_id: None,
                content: None,
                thinking: None,
                signature: None,
                citations: None,
            }]),
        };
        let openai_msgs = claude_message_to_openai(&msg);
        assert_eq!(openai_msgs.len(), 1);
        let content = &openai_msgs[0].content;
        let parts = match content {
            ChatContent::Parts(parts) => parts,
            _ => panic!("expected content parts"),
        };
        assert_eq!(parts.len(), 1);
        let part = &parts[0];
        assert_eq!(part.content_type, "image_url");
        assert_eq!(
            part.image_url.as_ref().unwrap().url,
            "data:image/png;base64,iVBORw0KGgo="
        );
    }

    #[test]
    fn test_image_roundtrip_openai_claude_openai() {
        // OpenAI image_url → Claude image block → OpenAI image_url — image preserved.
        let orig = ContentPart {
            content_type: "image_url".to_string(),
            text: None,
            image_url: Some(ImageUrl {
                url: "data:image/jpeg;base64,/9j/4AAQ==".to_string(),
            }),
        };
        let claude = openai_message_to_claude(&make_user_parts(vec![orig.clone()]));
        let blocks = match &claude.content {
            ClaudeContent::Blocks(b) => b,
            _ => panic!("expected content blocks"),
        };
        let back = claude_message_to_openai(&ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeContent::Blocks(blocks.clone()),
        });
        let parts = match &back[0].content {
            ChatContent::Parts(p) => p,
            _ => panic!("expected content parts"),
        };
        assert_eq!(parts.len(), 1);
        assert_eq!(
            parts[0].image_url.as_ref().unwrap().url,
            "data:image/jpeg;base64,/9j/4AAQ=="
        );
    }
}
