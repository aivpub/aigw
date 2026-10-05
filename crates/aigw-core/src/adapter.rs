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
///
/// `next` is called **once per upstream byte chunk**, not in a drain loop: the
/// chunk is fully consumed into events on that single call. Callers must not
/// loop until `None` — a chunk that produced events always yields `Some` on the
/// same input (the converter is not a queue), so a drain loop never terminates
/// and the response stream hangs open.
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
        let mut out: Vec<u8> = Vec::new();
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
                // Emit `message_start` once, before any content events. Do NOT
                // early-return here: the first chunk may carry `role` AND the
                // first content delta in the same frame (e.g.
                // `delta:{"role":"assistant","content":"Mult"}`). Early-returning
                // dropped that first text delta.
                if !self.started {
                    self.started = true;
                    if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
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
                    }) {
                        out.extend_from_slice(&ev);
                    }
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
                                if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
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
                                }) {
                                    out.extend_from_slice(&ev);
                                }
                            }
                            // Emit the first content_block_delta in the SAME
                            // frame as content_block_start. The old early-return
                            // dropped the first text delta (e.g. "Mult").
                            if let Some(ev) = self.emit_event(&ClaudeStreamEvent {
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
                            }) {
                                out.extend_from_slice(&ev);
                            }
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
                                    out.extend_from_slice(&ev);
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
                                out.extend_from_slice(&ev);
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
                        out.extend_from_slice(&ev);
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

/// Chat Completions tool names are capped by most upstreams; sub2api uses the
/// same 64-char ceiling before falling back to a hashed suffix.
const CHAT_TOOL_NAME_MAX_LEN: usize = 64;

/// The `custom` tool has no Chat Completions equivalent — its raw input is
/// passed through as a single string field (matches sub2api's `customToolInputSchema`).
const CUSTOM_TOOL_INPUT_SCHEMA: &str = r#"{"type":"object","properties":{"input":{"type":"string","description":"The raw input for this tool, passed through verbatim."}},"required":["input"]}"#;

/// `tool_search` is invoked by name from the client side, so it cannot be
/// renamed; it degrades to a same-named function proxy (sub2api-compatible).
const TOOL_SEARCH_PROXY_NAME: &str = "tool_search";
const TOOL_SEARCH_PROXY_SCHEMA: &str = r#"{"type":"object","properties":{"query":{"type":"string","description":"Search query for tools or connectors to load."},"limit":{"type":"integer","description":"Maximum number of tool groups to return."}},"required":["query"]}"#;

/// Server-side tool types with no Chat Completions equivalent are listed in the
/// `normalize_responses_tools` doc comment; they are dropped with a warning
/// (whether the upstream accepts them as-is is provider-specific, and measured
/// against the target MaaS upstream all of them 400).
///
/// Flatten a Responses `content` value (string or array of parts) to plain text.
/// Used when merging `developer` messages into the leading system message.
fn flatten_responses_content_to_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
            .collect::<Vec<&str>>()
            .join(""),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Map Responses content parts to their Chat Completions equivalents.
/// `input_text` / `output_text` → `text`; anything else is left untouched.
fn normalize_content_parts(content: &Value) -> Value {
    let Value::Array(parts) = content else {
        return content.clone();
    };
    Value::Array(
        parts
            .iter()
            .map(|part| {
                let Some(obj) = part.as_object() else {
                    return part.clone();
                };
                let part_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if part_type == "input_text" || part_type == "output_text" {
                    let mut out = obj.clone();
                    out.insert("type".to_string(), json!("text"));
                    Value::Object(out)
                } else {
                    part.clone()
                }
            })
            .collect(),
    )
}

/// Namespace-qualified function name: `{namespace}__{child}`, hashed when over
/// the chat tool-name ceiling. Byte-compatible with sub2api / litellm.
fn flatten_namespace_tool_name(namespace: &str, name: &str) -> String {
    let full = format!("{}__{}", namespace, name);
    if full.len() <= CHAT_TOOL_NAME_MAX_LEN {
        return full;
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(full.as_bytes());
    let digest = hasher.finalize();
    let suffix = format!("__{}", hex::encode(&digest[..4]));
    let prefix_len = CHAT_TOOL_NAME_MAX_LEN - suffix.len();
    let prefix: String = full.chars().take(prefix_len).collect();
    format!("{}{}", prefix, suffix)
}

/// Build a nested Chat Completions function tool from a flat Responses function tool.
fn nested_function_tool(name: &str, description: &str, parameters: Value, strict: Value) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
            "strict": strict,
        }
    })
}

impl ResponsesToChatCompletions {
    /// Convert Responses API `input` field to Chat Completions `messages`.
    ///
    /// `input[]` is a tagged union, not a plain message list: alongside
    /// `message` items it carries `function_call` / `function_call_output` /
    /// `reasoning` items that a multi-turn tool history needs. Dispatching on
    /// `type` is what makes round 2+ work — treating every item as a message
    /// would drop the assistant's tool call and misfile its output.
    fn input_to_messages(input: &Value) -> Result<Vec<Value>, AdapterError> {
        match input {
            Value::String(text) => Ok(vec![json!({"role": "user", "content": text})]),
            Value::Array(items) => {
                if items.is_empty() {
                    return Err(AdapterError::Unsupported(
                        "input array must not be empty".to_string(),
                    ));
                }
                let messages = Self::items_to_messages(items);
                Ok(Self::normalize_tool_pairing(messages))
            }
            _ => Err(AdapterError::Unsupported(
                "input must be a string or array".to_string(),
            )),
        }
    }

    /// Walk `input[]` items, dispatching each by its `type` tag.
    fn items_to_messages(items: &[Value]) -> Vec<Value> {
        let mut messages: Vec<Value> = Vec::with_capacity(items.len());
        let mut pending_reasoning = String::new();

        for item in items {
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match item_type {
                "" | "message" => {
                    let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                    let content = item
                        .get("content")
                        .map(normalize_content_parts)
                        .unwrap_or_else(|| {
                            // Some items carry the text directly (`text` field).
                            item.get("text").cloned().unwrap_or(Value::Null)
                        });
                    if role != "assistant" {
                        pending_reasoning.clear();
                    }
                    messages.push(json!({"role": role, "content": content}));
                }
                "reasoning" => {
                    if let Some(text) = extract_responses_reasoning_text(item) {
                        pending_reasoning = text;
                    }
                }
                "function_call" | "custom_tool_call" | "tool_search_call" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                    if call_id.is_empty() {
                        tracing::debug!(item_type = %item_type, "dropping tool call without call_id");
                        pending_reasoning.clear();
                        continue;
                    }
                    let name = Self::tool_call_name(item);
                    let arguments = Self::tool_call_arguments(item);
                    append_assistant_tool_call(
                        &mut messages,
                        json!({
                            "id": call_id,
                            "type": "function",
                            "function": {"name": name, "arguments": arguments}
                        }),
                        std::mem::take(&mut pending_reasoning),
                    );
                }
                "function_call_output" | "custom_tool_call_output" | "tool_search_output" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                    let output = item
                        .get("output")
                        .map(responses_output_to_text)
                        .unwrap_or_default();
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": call_id,
                        "content": output
                    }));
                    pending_reasoning.clear();
                }
                // Bare content parts (no wrapping message item).
                "input_text" | "text" => {
                    messages.push(json!({
                        "role": "user",
                        "content": item.get("text").cloned().unwrap_or(Value::Null)
                    }));
                    pending_reasoning.clear();
                }
                other => {
                    // Server-side call items (`web_search_call`, `local_shell_call`,
                    // `file_search_call`, ...) have no Chat equivalent. Converting
                    // them would inject a stray message between an assistant's
                    // tool_calls and its replies, which strict upstreams reject.
                    tracing::debug!(item_type = %other, "skipping Responses item with no Chat equivalent");
                    pending_reasoning.clear();
                }
            }
        }

        messages
    }

    /// Chat tool name for a call item, namespace-qualified when the item
    /// carries a `namespace` (mirrors request-side flattening).
    fn tool_call_name(item: &Value) -> String {
        let cy_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if cy_type == "tool_search_call" {
            return TOOL_SEARCH_PROXY_NAME.to_string();
        }
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
        match item.get("namespace").and_then(|v| v.as_str()) {
            Some(ns) if !ns.is_empty() => flatten_namespace_tool_name(ns, name),
            _ => name.to_string(),
        }
    }

    /// Chat arguments string for a call item.
    fn tool_call_arguments(item: &Value) -> String {
        match item.get("arguments") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
            Some(Value::Object(_)) | Some(Value::Array(_)) => {
                item.get("arguments").unwrap().to_string()
            }
            _ => {
                // `custom` calls carry free-form `input`, which the degraded
                // tool takes as a single `{"input": ...}` string.
                if let Some(input) = item.get("input") {
                    json!({"input": responses_output_to_text(input)}).to_string()
                } else {
                    "{}".to_string()
                }
            }
        }
    }

    /// Enforce the Chat Completions tool-call invariant: an assistant message
    /// carrying `tool_calls` must be followed immediately by one `tool` message
    /// per `tool_call_id`. Codex histories violate this (a call left dangling by
    /// a mid-execution reconnect, a reply whose announcing call was trimmed), and
    /// strict upstreams reject the request outright.
    ///
    /// Unanswered `tool_calls` are dropped; an assistant left with neither
    /// content nor calls is dropped; orphan `tool` replies are dropped.
    fn normalize_tool_pairing(messages: Vec<Value>) -> Vec<Value> {
        use std::collections::{HashMap, HashSet};

        let mut replies: HashMap<String, Value> = HashMap::new();
        for msg in &messages {
            if msg.get("role").and_then(|v| v.as_str()) == Some("tool") {
                if let Some(id) = msg.get("tool_call_id").and_then(|v| v.as_str()) {
                    if !id.is_empty() {
                        // Last wins on duplicates (matches sub2api).
                        replies.insert(id.to_string(), msg.clone());
                    }
                }
            }
        }

        let mut emitted: HashSet<String> = HashSet::new();
        let mut out: Vec<Value> = Vec::with_capacity(messages.len());

        for msg in messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "tool" {
                let id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                // A reply already emitted next to its assistant, or with no
                // announcing call at all, would be an orphan upstream.
                if id.is_empty() || !emitted.insert(id.to_string()) {
                    continue;
                }
                continue;
            }

            let kept_calls: Vec<Value> = msg
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .map(|calls| {
                    calls
                        .iter()
                        .filter(|c| {
                            c.get("id")
                                .and_then(|v| v.as_str())
                                .map(|id| replies.contains_key(id))
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            if kept_calls.is_empty() {
                let has_content = match msg.get("content") {
                    Some(Value::String(s)) => !s.is_empty(),
                    Some(Value::Array(a)) => !a.is_empty(),
                    _ => false,
                };
                let had_calls = msg.get("tool_calls").is_some();
                if had_calls && !has_content {
                    continue; // assistant stripped of all calls and empty
                }
                out.push(msg);
                continue;
            }

            let mut assistant = msg.clone();
            if let Some(obj) = assistant.as_object_mut() {
                obj.insert("tool_calls".to_string(), Value::Array(kept_calls.clone()));
            }
            out.push(assistant);
            for call in &kept_calls {
                if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                    if let Some(reply) = replies.get(id) {
                        out.push(reply.clone());
                    }
                }
            }
        }

        out
    }

    /// Normalize `developer` messages into the leading system message.
    ///
    /// Codex always emits `[system?, developer, user...]`. OpenAI treats a
    /// `developer` message as equivalent to `instructions` (application-level
    /// rules), so it belongs in the system slot — merging there preserves its
    /// system-level priority instead of downgrading it into a user turn, and
    /// avoids producing a second `system` message (which strict Jinja chat
    /// templates such as Qwen's reject outright).
    ///
    /// When `passthrough` is set the roles are left untouched (the upstream is
    /// known to accept `developer` natively).
    fn merge_developer_into_system(messages: Vec<Value>, passthrough: bool) -> Vec<Value> {
        if passthrough {
            return messages;
        }
        let mut merged_texts: Vec<String> = Vec::new();
        let mut rest: Vec<Value> = Vec::with_capacity(messages.len());

        for msg in messages {
            if msg.get("role").and_then(|v| v.as_str()) == Some("developer") {
                let text =
                    flatten_responses_content_to_text(msg.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    merged_texts.push(text);
                }
                continue;
            }
            rest.push(msg);
        }

        if merged_texts.is_empty() {
            return rest;
        }

        let appendix = merged_texts.join("\n\n");
        if let Some(first) = rest.first_mut() {
            if first.get("role").and_then(|v| v.as_str()) == Some("system") {
                let existing =
                    flatten_responses_content_to_text(first.get("content").unwrap_or(&Value::Null));
                let combined = if existing.is_empty() {
                    appendix
                } else {
                    format!("{}\n\n{}", existing, appendix)
                };
                if let Some(obj) = first.as_object_mut() {
                    obj.insert("content".to_string(), json!(combined));
                }
                return rest;
            }
        }

        // No leading system message — the developer content becomes one.
        let mut out = Vec::with_capacity(rest.len() + 1);
        out.push(json!({"role": "system", "content": appendix}));
        out.extend(rest);
        out
    }

    /// Collapse every `system` message into a single leading one.
    ///
    /// `input[]` may carry inline `system` messages of its own. Strict Jinja
    /// templates (Qwen family) reject a `system` beyond index 0, so they are
    /// merged into the leading system slot rather than dropped — the content is
    /// system-level rules and must keep that priority.
    fn consolidate_system_messages(messages: Vec<Value>) -> Vec<Value> {
        let mut system_texts: Vec<String> = Vec::new();
        let mut rest: Vec<Value> = Vec::with_capacity(messages.len());

        for msg in messages {
            if msg.get("role").and_then(|v| v.as_str()) == Some("system") {
                let text =
                    flatten_responses_content_to_text(msg.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    system_texts.push(text);
                }
                continue;
            }
            rest.push(msg);
        }

        if system_texts.is_empty() {
            return rest;
        }

        let mut out = Vec::with_capacity(rest.len() + 1);
        out.push(json!({"role": "system", "content": system_texts.join("\n\n")}));
        out.extend(rest);
        out
    }

    /// Normalize the Responses `tools` array into Chat Completions tools.
    ///
    /// Returns the converted tools plus the set of declared (surviving) tool
    /// names, which callers use to drop dangling `tool_choice` references.
    ///
    /// Sub2api/litellm-compatible behaviour:
    /// - `function`  → nested `{type, function:{...}}`
    /// - `namespace` → children flattened to `{ns}__{child}` functions
    /// - `custom`    → degraded to a single-string-input function
    /// - `tool_search` → same-named function proxy (cannot be renamed)
    /// - server-side tools (`web_search`, `code_interpreter`, `mcp`, ...) → dropped
    fn normalize_responses_tools(
        tools: &[Value],
    ) -> Result<(Vec<Value>, std::collections::HashSet<String>), AdapterError> {
        use std::collections::{HashMap, HashSet};

        let top_level: HashSet<&str> = tools
            .iter()
            .filter(|t| {
                matches!(
                    t.get("type").and_then(|v| v.as_str()),
                    Some("function" | "custom")
                )
            })
            .filter_map(|t| t.get("name").and_then(|v| v.as_str()))
            .collect();

        let mut out: Vec<Value> = Vec::with_capacity(tools.len());
        let mut declared: HashSet<String> = HashSet::new();
        let mut flat_owner: HashMap<String, String> = HashMap::new();
        let mut tool_search_declared = false;

        for tool in tools {
            let tool_type = tool.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
            match tool_type {
                "function" => {
                    let converted = nested_function_tool(
                        name,
                        tool.get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        tool.get("parameters").cloned().unwrap_or(json!({})),
                        tool.get("strict").cloned().unwrap_or(json!(false)),
                    );
                    declared.insert(name.to_string());
                    out.push(converted);
                }
                "namespace" => {
                    for child in namespace_children(tool) {
                        let child_name = child.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        if child_name.is_empty() {
                            continue;
                        }
                        let flat = flatten_namespace_tool_name(name, child_name);
                        // A flattened name colliding with a top-level tool (or with
                        // another namespace's child) is unrepresentable upstream —
                        // silently overwriting would route the model's call to the
                        // wrong tool, so reject explicitly.
                        if top_level.contains(flat.as_str()) {
                            return Err(AdapterError::Unsupported(format!(
                                "namespace tool '{}/{}' flattens to '{}' which conflicts with a top-level tool of the same name; this upstream cannot disambiguate them, rename one of the tools",
                                name, child_name, flat
                            )));
                        }
                        if let Some(prev) = flat_owner.get(&flat) {
                            if prev != &format!("{}/{}", name, child_name) {
                                return Err(AdapterError::Unsupported(format!(
                                    "namespace tools '{}' and '{}/{}' both flatten to '{}'; this upstream cannot disambiguate them, rename one of the tools",
                                    prev, name, child_name, flat
                                )));
                            }
                        }
                        flat_owner.insert(flat.clone(), format!("{}/{}", name, child_name));
                        declared.insert(flat.clone());
                        out.push(nested_function_tool(
                            &flat,
                            child
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or(""),
                            child.get("parameters").cloned().unwrap_or(json!({})),
                            child.get("strict").cloned().unwrap_or(json!(false)),
                        ));
                    }
                }
                "custom" => {
                    let converted = nested_function_tool(
                        name,
                        tool.get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        serde_json::from_str(CUSTOM_TOOL_INPUT_SCHEMA).unwrap_or(json!({})),
                        json!(false),
                    );
                    declared.insert(name.to_string());
                    out.push(converted);
                }
                "tool_search" => {
                    if top_level.contains(TOOL_SEARCH_PROXY_NAME) {
                        return Err(AdapterError::Unsupported(format!(
                            "built-in tool_search conflicts with a declared tool named '{}'; this upstream cannot disambiguate them, rename the tool",
                            TOOL_SEARCH_PROXY_NAME
                        )));
                    }
                    if tool_search_declared {
                        continue;
                    }
                    tool_search_declared = true;
                    declared.insert(TOOL_SEARCH_PROXY_NAME.to_string());
                    out.push(nested_function_tool(
                        TOOL_SEARCH_PROXY_NAME,
                        "Search and load tools, plugins, connectors, and MCP namespaces for the current task.",
                        serde_json::from_str(TOOL_SEARCH_PROXY_SCHEMA).unwrap_or(json!({})),
                        json!(false),
                    ));
                }
                other => {
                    tracing::warn!(
                        tool_type = %other,
                        tool_name = %name,
                        "dropping Responses API tool: no Chat Completions equivalent"
                    );
                }
            }
        }

        Ok((out, declared))
    }
}

/// Concatenate the `text` of every part in a reasoning item's `summary`
/// (falling back to `content`), matching sub2api's extractor.
fn extract_responses_reasoning_text(item: &Value) -> Option<String> {
    let collect = |v: Option<&Value>| -> Vec<String> {
        v.and_then(|x| x.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()).map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut texts = collect(item.get("summary"));
    if texts.is_empty() {
        texts = collect(item.get("content"));
    }
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

/// Render a tool output value as the string a Chat `tool` message carries.
/// Objects/arrays (e.g. a `tool_search` result list) are stringified whole.
fn responses_output_to_text(output: &Value) -> String {
    match output {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Append or extend an assistant `tool_calls` message. Parallel calls from one
/// assistant turn merge into a single message (Chat schema requires one
/// assistant message holding them all).
fn append_assistant_tool_call(messages: &mut Vec<Value>, call: Value, reasoning: String) {
    if let Some(last) = messages.last_mut() {
        if last.get("role").and_then(|v| v.as_str()) == Some("assistant") {
            if let Some(obj) = last.as_object_mut() {
                let entry = obj
                    .entry("tool_calls".to_string())
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(arr) = entry.as_array_mut() {
                    arr.push(call);
                }
                if !reasoning.is_empty()
                    && obj
                        .get("reasoning_content")
                        .and_then(|v| v.as_str())
                        .map(|s| s.is_empty())
                        .unwrap_or(true)
                {
                    obj.insert("reasoning_content".to_string(), json!(reasoning));
                }
                return;
            }
        }
    }
    let mut msg = json!({"role": "assistant", "content": Value::Null, "tool_calls": [call]});
    if !reasoning.is_empty() {
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("reasoning_content".to_string(), json!(reasoning));
        }
    }
    messages.push(msg);
}

/// Children of a `namespace` tool — `tools` preferred, `children` as fallback.
fn namespace_children(tool: &Value) -> Vec<&Value> {
    for key in ["tools", "children"] {
        if let Some(arr) = tool.get(key).and_then(|v| v.as_array()) {
            if !arr.is_empty() {
                return arr
                    .iter()
                    .filter(|c| c.get("type").and_then(|v| v.as_str()) == Some("function"))
                    .collect();
            }
        }
    }
    Vec::new()
}

/// Drop a `tool_choice` that points at a tool which did not survive conversion —
/// Chat Completions upstreams reject choices naming undeclared tools.
fn normalize_tool_choice(
    tool_choice: &Value,
    declared: &std::collections::HashSet<String>,
) -> Option<Value> {
    match tool_choice {
        Value::String(_) => Some(tool_choice.clone()),
        Value::Object(obj) => {
            let is_named = matches!(
                obj.get("type").and_then(|v| v.as_str()),
                Some("function" | "custom" | "namespace" | "tool_search")
            );
            if !is_named {
                return Some(tool_choice.clone());
            }
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    obj.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("");
            if name.is_empty() || declared.contains(name) {
                return Some(tool_choice.clone());
            }
            tracing::debug!(tool_name = %name, "dropping tool_choice referencing a tool that was not declared");
            None
        }
        _ => Some(tool_choice.clone()),
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

        // 1. Normalize tools — flatten namespaces, degrade custom/tool_search,
        //    drop server-side tools. Never reject the whole request.
        let mut declared_tools = std::collections::HashSet::new();
        if let Some(tools) = obj.get("tools").and_then(|v| v.as_array()).cloned() {
            let (normalized, declared) = Self::normalize_responses_tools(&tools)?;
            declared_tools = declared;
            if normalized.is_empty() {
                obj.remove("tools");
            } else {
                obj.insert("tools".to_string(), Value::Array(normalized));
            }
        }

        // 1b. Drop a tool_choice that survives but references a dropped tool.
        if let Some(choice) = obj.get("tool_choice").cloned() {
            match normalize_tool_choice(&choice, &declared_tools) {
                Some(kept) => {
                    obj.insert("tool_choice".to_string(), kept);
                }
                None => {
                    obj.remove("tool_choice");
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

        // 3b. Collapse inline `system` messages into the single leading one, then
        //     fold `developer` messages into it as well (see merge_developer_into_system).
        let messages = Self::consolidate_system_messages(messages);
        let passthrough = deployment.developer_role_passthrough.unwrap_or(false);
        let messages = Self::merge_developer_into_system(messages, passthrough);

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
            developer_role_passthrough: None,
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

    /// Regression: the first text delta must not be dropped. The old
    /// early-return in `next()` emitted only `content_block_start` and
    /// swallowed the first `content_block_delta` (e.g. "Mult").
    #[test]
    fn test_stream_adapter_first_text_delta_not_dropped() {
        let mut stream = AnthropicToOpenAIStream::new();
        // Frame 1: role only (no content).
        stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"g\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"}}]}",
        );
        // Frame 2: first content delta — the "Mult" case.
        let result = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"g\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Multica\"}}]}",
        )
        .expect("second frame must emit events");
        let text = String::from_utf8(result).unwrap();
        assert!(
            text.contains("content_block_start"),
            "expected content_block_start, got: {text}"
        );
        assert!(
            text.contains("content_block_delta"),
            "expected content_block_delta in same frame, got: {text}"
        );
        assert!(
            text.contains("Multica"),
            "first text delta 'Multica' must be forwarded, got: {text}"
        );
    }

    /// Regression: if the first frame carries role AND content together
    /// (`delta:{"role":"assistant","content":"Mult"}`), the content must
    /// still be forwarded after `message_start`.
    #[test]
    fn test_stream_adapter_role_and_content_same_frame() {
        let mut stream = AnthropicToOpenAIStream::new();
        let result = stream.next(
            b"data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"g\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Mult\"}}]}",
        )
        .expect("first frame must emit events");
        let text = String::from_utf8(result).unwrap();
        assert!(text.contains("message_start"), "got: {text}");
        assert!(text.contains("content_block_delta"), "got: {text}");
        assert!(
            text.contains("\"text\":\"Mult\""),
            "first delta 'Mult' dropped, got: {text}"
        );
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
            developer_role_passthrough: None,
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
            developer_role_passthrough: None,
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
            developer_role_passthrough: None,
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

    // ── Stage 131 Responses→Chat bridge — tools/role/part normalization ──

    fn bridge_dep() -> Deployment {
        Deployment {
            upstream_model: "upstream-model".into(),
            ..test_deployment()
        }
    }

    fn bridge_adapt(body: serde_json::Value) -> serde_json::Value {
        ResponsesToChatCompletions
            .adapt_request(body, &bridge_dep())
            .expect("adapt_request")
    }

    fn bridge_adapt_with(body: serde_json::Value, passthrough: Option<bool>) -> serde_json::Value {
        let dep = Deployment {
            developer_role_passthrough: passthrough,
            ..bridge_dep()
        };
        ResponsesToChatCompletions
            .adapt_request(body, &dep)
            .expect("adapt_request")
    }

    #[test]
    fn test_responses_to_chat_tools_flatten_function() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [{
                "type": "function",
                "name": "exec_command",
                "description": "run",
                "parameters": {"type": "object", "properties": {}},
                "strict": false
            }]
        }));
        let tool = &adapted["tools"][0];
        assert_eq!(tool["type"].as_str(), Some("function"));
        assert_eq!(tool["function"]["name"].as_str(), Some("exec_command"));
        assert_eq!(tool["function"]["description"].as_str(), Some("run"));
        assert!(tool.get("name").is_none(), "flat name must not leak");
    }

    #[test]
    fn test_responses_to_chat_tools_namespace_flattened() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [{
                "type": "namespace",
                "name": "multi_agent_v1",
                "tools": [
                    {"type": "function", "name": "spawn_agent", "parameters": {"type": "object"}},
                    {"type": "function", "name": "close_agent", "parameters": {"type": "object"}}
                ]
            }]
        }));
        let names: Vec<&str> = adapted["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec!["multi_agent_v1__spawn_agent", "multi_agent_v1__close_agent"]
        );
    }

    #[test]
    fn test_responses_to_chat_tools_namespace_name_collision() {
        let err = ResponsesToChatCompletions
            .adapt_request(
                json!({
                    "model": "m",
                    "input": "hi",
                    "tools": [
                        {"type": "function", "name": "ns__child", "parameters": {"type": "object"}},
                        {"type": "namespace", "name": "ns", "tools": [
                            {"type": "function", "name": "child", "parameters": {"type": "object"}}
                        ]}
                    ]
                }),
                &bridge_dep(),
            )
            .expect_err("flatten collision must be rejected");
        assert!(matches!(err, AdapterError::Unsupported(_)), "got {err:?}");
    }

    #[test]
    fn test_responses_to_chat_tools_server_side_dropped() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [
                {"type": "web_search"},
                {"type": "web_search_preview"},
                {"type": "code_interpreter"},
                {"type": "computer_use_preview"},
                {"type": "mcp", "server_label": "x"},
                {"type": "function", "name": "keep_me", "parameters": {"type": "object"}}
            ]
        }));
        let tools = adapted["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "only the function tool survives");
        assert_eq!(tools[0]["function"]["name"].as_str(), Some("keep_me"));
    }

    #[test]
    fn test_responses_to_chat_tools_all_dropped_removes_key() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "web_search"}]
        }));
        assert!(
            adapted.get("tools").is_none(),
            "empty tools must be removed"
        );
    }

    #[test]
    fn test_responses_to_chat_tool_choice_dropped_with_tool() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "web_search"}],
            "tool_choice": {"type": "function", "name": "web_search"}
        }));
        assert!(
            adapted.get("tool_choice").is_none(),
            "tool_choice pointing at a dropped tool must be dropped too"
        );
    }

    #[test]
    fn test_responses_to_chat_tool_choice_kept_for_surviving() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "function", "name": "get_weather", "parameters": {"type": "object"}}],
            "tool_choice": {"type": "function", "name": "get_weather"}
        }));
        assert_eq!(adapted["tool_choice"]["name"].as_str(), Some("get_weather"));

        let auto = bridge_adapt(json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "function", "name": "get_weather", "parameters": {"type": "object"}}],
            "tool_choice": "auto"
        }));
        assert_eq!(auto["tool_choice"].as_str(), Some("auto"));
    }

    #[test]
    fn test_responses_to_chat_developer_role_becomes_system() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "no network"}]},
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        assert!(
            messages
                .iter()
                .all(|m| m["role"].as_str() != Some("developer")),
            "developer role must not reach upstream: {messages:?}"
        );
    }

    #[test]
    fn test_responses_to_chat_developer_passthrough_when_configured() {
        let adapted = bridge_adapt_with(
            json!({
                "model": "m",
                "input": [
                    {"role": "developer", "content": [{"type": "input_text", "text": "no network"}]},
                    {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
                ]
            }),
            Some(true),
        );
        let messages = adapted["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"].as_str(), Some("developer"));
        assert_eq!(
            messages[0]["content"][0]["text"].as_str(),
            Some("no network")
        );
    }

    #[test]
    fn test_responses_to_chat_developer_merged_into_leading_system() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "instructions": "base rules",
            "input": [
                {"role": "developer", "content": [{"type": "input_text", "text": "permissions"}]},
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "got {messages:?}");
        assert_eq!(messages[0]["role"].as_str(), Some("system"));
        assert_eq!(
            messages[0]["content"].as_str(),
            Some("base rules\n\npermissions")
        );
        assert_eq!(messages[1]["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_responses_to_chat_no_second_system() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "instructions": "base rules",
            "input": [
                {"role": "developer", "content": "permissions"},
                {"role": "system", "content": "inline system"},
                {"role": "user", "content": "hi"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let systems: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m["role"].as_str() == Some("system"))
            .map(|(i, _)| i)
            .collect();
        assert!(
            systems.len() <= 1,
            "at most one leading system message allowed, got {:?} in {messages:?}",
            systems
        );
    }

    #[test]
    fn test_responses_to_chat_input_text_part_mapped() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]
        }));
        assert_eq!(
            adapted["messages"][0]["content"][0]["type"].as_str(),
            Some("text")
        );
        assert_eq!(
            adapted["messages"][0]["content"][0]["text"].as_str(),
            Some("hi")
        );
    }

    /// End-to-end regression: a real Codex CLI request body must not 400.
    /// Captured from Codex 0.157.1 (`wire_api = "responses"`) — the exact shape
    /// that previously failed with "tool type 'namespace' is not supported".
    #[test]
    fn test_responses_to_chat_codex_fixture_end_to_end() {
        let body = json!({
            "model": "tokenhub/deepseek-v4-flash",
            "instructions": "You are a coding agent running in the Codex CLI.",
            "input": [
                {"type": "message", "role": "developer",
                 "content": [{"type": "input_text", "text": "<permissions instructions>"}]},
                {"type": "message", "role": "user",
                 "content": [{"type": "input_text", "text": "hi"}]}
            ],
            "tools": [
                {"type": "function", "name": "exec_command", "description": "run", "strict": false,
                 "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}},
                {"type": "function", "name": "write_stdin", "description": "stdin", "strict": false,
                 "parameters": {"type": "object", "properties": {}}},
                {"type": "namespace", "name": "multi_agent_v1", "description": "Sub-agents.",
                 "tools": [
                     {"type": "function", "name": "spawn_agent", "strict": false,
                      "parameters": {"type": "object", "properties": {}}},
                     {"type": "function", "name": "close_agent", "strict": false,
                      "parameters": {"type": "object", "properties": {}}}
                 ]},
                {"type": "web_search", "external_web_access": false}
            ],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "stream": true
        });

        let adapted = bridge_adapt(body);

        // Must not 400 — every tool is either converted or dropped.
        let tool_names: Vec<&str> = adapted["tools"]
            .as_array()
            .expect("tools present")
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert_eq!(
            tool_names,
            vec![
                "exec_command",
                "write_stdin",
                "multi_agent_v1__spawn_agent",
                "multi_agent_v1__close_agent"
            ]
        );
        // No flat `type`/`name` may leak; every tool is nested.
        for tool in adapted["tools"].as_array().unwrap() {
            assert_eq!(tool["type"].as_str(), Some("function"));
            assert!(tool.get("function").is_some());
        }
        // developer folded into the single leading system message.
        let messages = adapted["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"].as_str(), Some("system"));
        assert_eq!(
            messages.len(),
            2,
            "instructions + developer merge into one system: {messages:?}"
        );
        assert_eq!(messages[1]["content"][0]["type"].as_str(), Some("text"));
        // tool_choice "auto" survives; stream_options injected.
        assert_eq!(adapted["tool_choice"].as_str(), Some("auto"));
        assert_eq!(
            adapted["stream_options"]["include_usage"].as_bool(),
            Some(true)
        );
    }

    // ── Stage 132 item dispatch — multi-turn tool history (TD-017a) ──

    #[test]
    fn test_responses_to_chat_item_function_call_becomes_assistant_tool_calls() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":[{"type":"input_text","text":"run echo"}]},
                {"type":"function_call","id":"fc_1","call_id":"call_abc","name":"exec_command",
                 "arguments":"{\"cmd\":\"echo hello\"}"},
                {"type":"function_call_output","id":"fco_1","call_id":"call_abc",
                 "output":"hello\n"},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"thanks"}]}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            roles,
            vec!["user", "assistant", "tool", "user"],
            "got {messages:?}"
        );
        // assistant holds the call verbatim
        assert_eq!(
            messages[1]["tool_calls"][0]["id"].as_str(),
            Some("call_abc")
        );
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["name"].as_str(),
            Some("exec_command")
        );
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["arguments"].as_str(),
            Some("{\"cmd\":\"echo hello\"}")
        );
        // tool reply carries the matching id
        assert_eq!(messages[2]["tool_call_id"].as_str(), Some("call_abc"));
        assert_eq!(messages[2]["content"].as_str(), Some("hello\n"));
    }

    #[test]
    fn test_responses_to_chat_item_parallel_calls_merge_into_one_assistant() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"function_call","call_id":"c1","name":"f1","arguments":"{}"},
                {"type":"function_call","call_id":"c2","name":"f2","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":"r1"},
                {"type":"function_call_output","call_id":"c2","output":"r2"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            roles,
            vec!["user", "assistant", "tool", "tool"],
            "got {messages:?}"
        );
        let calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2, "parallel calls merge into one assistant");
        // replies follow in call order
        assert_eq!(messages[2]["tool_call_id"].as_str(), Some("c1"));
        assert_eq!(messages[3]["tool_call_id"].as_str(), Some("c2"));
    }

    #[test]
    fn test_responses_to_chat_item_unanswered_call_dropped() {
        // A call whose reply never arrived (mid-execution reconnect) would be
        // rejected by strict upstreams, so it is trimmed.
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"function_call","call_id":"c1","name":"f1","arguments":"{}"},
                {"type":"function_call","call_id":"c2","name":"f2","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":"r1"},
                {"type":"message","role":"user","content":"next"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            roles,
            vec!["user", "assistant", "tool", "user"],
            "got {messages:?}"
        );
        let calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1, "unanswered call trimmed");
        assert_eq!(calls[0]["id"].as_str(), Some("c1"));
    }

    #[test]
    fn test_responses_to_chat_item_orphan_tool_output_dropped() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"function_call_output","call_id":"nowhere","output":"stray"},
                {"type":"message","role":"user","content":"next"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            roles,
            vec!["user", "user"],
            "orphan reply dropped: {messages:?}"
        );
    }

    #[test]
    fn test_responses_to_chat_item_reasoning_attaches_to_assistant() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"reasoning","summary":[{"type":"summary_text","text":"thinking..."}]},
                {"type":"function_call","call_id":"c1","name":"f1","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":"r1"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"].as_str(), Some("assistant"));
        assert_eq!(
            messages[1]["reasoning_content"].as_str(),
            Some("thinking...")
        );
    }

    #[test]
    fn test_responses_to_chat_item_unknown_type_skipped() {
        // `web_search_call` etc. have no Chat equivalent; injecting a stray
        // message between an assistant's tool_calls and its reply breaks upstreams.
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"function_call","call_id":"c1","name":"f1","arguments":"{}"},
                {"type":"web_search_call","id":"ws_1","status":"completed"},
                {"type":"function_call_output","call_id":"c1","output":"r1"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            roles,
            vec!["user", "assistant", "tool"],
            "no stray message: {messages:?}"
        );
    }

    #[test]
    fn test_responses_to_chat_item_namespace_qualified_call_flattened() {
        // A historical call to a namespace child must use the same flattened
        // name the request-side tool declaration produced, or the model sees a
        // call to a tool it was never offered.
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"function_call","call_id":"c1","name":"spawn_agent","namespace":"multi_agent_v1","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":"r1"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["name"].as_str(),
            Some("multi_agent_v1__spawn_agent")
        );
    }

    #[test]
    fn test_responses_to_chat_item_custom_tool_call_wraps_input() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"custom_tool_call","call_id":"c1","name":"exec","input":"freeform text"},
                {"type":"custom_tool_call_output","call_id":"c1","output":"ok"}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["name"].as_str(),
            Some("exec")
        );
        let args: serde_json::Value = serde_json::from_str(
            messages[1]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .expect("arguments must be JSON");
        assert_eq!(args["input"].as_str(), Some("freeform text"));
    }

    #[test]
    fn test_responses_to_chat_item_object_output_stringified() {
        let adapted = bridge_adapt(json!({
            "model": "m",
            "input": [
                {"type":"message","role":"user","content":"go"},
                {"type":"function_call","call_id":"c1","name":"f1","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":{"hits":[1,2]}}
            ]
        }));
        let messages = adapted["messages"].as_array().unwrap();
        let content = messages[2]["content"].as_str().expect("content string");
        assert!(content.contains("\"hits\""), "got {content}");
    }

    /// Multi-turn end-to-end regression: a real Codex round-2 body (assistant
    /// tool call + its output, captured from Codex 0.160.0) must bridge into a
    /// valid Chat tool-call sequence.
    #[test]
    fn test_responses_to_chat_multiturn_codex_fixture_end_to_end() {
        let body = json!({
            "model": "tokenhub/deepseek-v4-flash",
            "instructions": "You are a coding agent running in the Codex CLI.",
            "input": [
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"<permissions>"}]},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"run echo hello"}]},
                {"type":"function_call","id":"fc_1","call_id":"call_abc123",
                 "name":"exec_command","arguments":"{\"cmd\":\"echo hello\"}"},
                {"type":"function_call_output","id":"fco_1","call_id":"call_abc123",
                 "output":"Chunk ID: 9a26e1\nProcess exited with code 0\nOutput:\nhello\n"}
            ],
            "tools": [
                {"type":"function","name":"exec_command","strict":false,
                 "parameters":{"type":"object","properties":{"cmd":{"type":"string"}}}},
                {"type":"web_search","external_web_access":false}
            ],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "stream": true
        });

        let adapted = bridge_adapt(body);
        let messages = adapted["messages"].as_array().unwrap();
        let roles: Vec<&str> = messages
            .iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            roles,
            vec!["system", "user", "assistant", "tool"],
            "got {messages:?}"
        );
        assert_eq!(
            messages[2]["tool_calls"][0]["id"].as_str(),
            Some("call_abc123")
        );
        assert_eq!(messages[3]["tool_call_id"].as_str(), Some("call_abc123"));
        // developer folded away, function nested, web_search dropped.
        assert!(messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("<permissions>"));
        let tools = adapted["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"].as_str(), Some("exec_command"));
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
            developer_role_passthrough: None,
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
            developer_role_passthrough: None,
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
            developer_role_passthrough: None,
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
            developer_role_passthrough: None,
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
            chat_template_compat: None,
            developer_role_passthrough: None, // auto-sniff detects qwen → Strict
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
            developer_role_passthrough: None,
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
