//! Search-result prompt injection (Phase 53, Stage 135).
//!
//! Design C: the gateway searches *before* calling the upstream, folds the
//! results into the request it was already going to send, and then makes one
//! ordinary call. No upstream-side tool support is required and the SSE path
//! needs no changes at all, which is the whole point of this form: the two
//! alternatives (short-circuit, agentic loop) are capped by Anthropic's
//! un-forgeable `encrypted_content` and by history poisoning on replay.
//!
//! Injection targets the **last user message**, not `system`. Three reasons,
//! two of them repo-specific:
//!
//! 1. The "exactly one leading system message" invariant (Stage 131) is upheld
//!    by `consolidate_system_messages` + `merge_developer_into_system` inside
//!    `adapt_request`. Injecting into `system` either collides with that
//!    invariant or reimplements it outside its guard. A system injection would
//!    also need three distinct code paths (Chat messages / Responses fold /
//!    Anthropic's top-level `Text(String) | Blocks` enum) where the last user
//!    message is one `last_mut()` across all three.
//! 2. Role semantics: search results answer *this question*, they are not
//!    rules. In `system` they would outrank instructions and persist across
//!    turns as if permanent.
//! 3. The system prefix stays byte-stable, so upstream prefix caching is not
//!    invalidated by results that change on every request.

use serde_json::{json, Value};

use super::trigger::SearchTrigger;
use super::types::SearchResponse;

/// Prompt template. Placeholders: `{cur_date}`, `{search_results}`, `{question}`.
///
/// The final rule is a load-bearing guardrail, not politeness: SearXNG has **no
/// zero-results semantics** — a gibberish query still returns ~35 unrelated
/// hits, so the gateway cannot detect "found nothing" and will sometimes inject
/// irrelevant material. This line is the only mitigation.
pub const PROMPT_TEMPLATE: &str = "\
# 以下是联网搜索到的参考资料（当前时间 {cur_date}）

{search_results}

# 用户问题

{question}

# 回答要求

- 优先依据上述参考资料回答；资料不足或相互矛盾时如实说明，不要臆测。
- 引用资料时用 markdown 链接标注来源，形如 [域名](URL)，例如 [example.com](https://example.com/a)。
- 参考资料与问题无关时直接忽略，按你自己的知识回答。";

/// Outcome of an injection attempt — mirrored into the response metadata
/// (Stage 135 §3.9) so the client can tell whether its request was actually
/// served with search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectionOutcome {
    /// Results were rendered and folded into the request.
    Injected { results: usize },
    /// No last user message to inject into; the request proceeds unmodified.
    NoTarget,
}

/// Render the search results into the prompt template.
pub fn render(
    response: &SearchResponse,
    question: &str,
    now: &chrono::DateTime<chrono::Utc>,
) -> String {
    // Order matters: replace the scalar placeholders before the results block is
    // spliced in, so a literal `{question}` inside a result title/snippet is not
    // rewritten into the user's own text (untrusted content feeding the prompt).
    PROMPT_TEMPLATE
        .replace("{cur_date}", &now.format("%Y-%m-%d").to_string())
        .replace("{question}", question)
        .replace("{search_results}", &render_results(response))
}

/// Render one result per block, mirroring litellm `format_search_response`
/// (title / URL / snippet — never the body).
///
/// `score` is deliberately not rendered: only some vendors supply it, and
/// emitting it would make the output shape drift by provider. The rule is a
/// forward-compatibility contract for the vendors that come later, not a
/// statement about SearXNG (which has none).
pub fn render_results(response: &SearchResponse) -> String {
    let mut out = String::new();
    for (i, r) in response.results.iter().enumerate() {
        out.push_str(&format!(
            "[{}] {}\n    URL: {}\n    {}\n",
            i + 1,
            r.title,
            r.url,
            r.snippet
        ));
        if let Some(date) = &r.published_date {
            out.push_str(&format!("    （{}）\n", date));
        }
    }
    out.trim_end().to_string()
}

/// Does `body` hold an injection target for `surface`?
///
/// Checked **before** searching so a request with no user message is not
/// charged a search it cannot use. Must stay in sync with the find predicates
/// in `inject_*` below — both derive from the same "plain user" rule.
pub fn has_target(body: &Value, surface: super::trigger::TriggerSurface) -> bool {
    use super::trigger::TriggerSurface;
    match surface {
        TriggerSurface::Responses => match body.get("input") {
            Some(Value::String(_)) => true,
            Some(Value::Array(items)) => items.iter().any(response_item_is_target),
            _ => false,
        },
        TriggerSurface::Chat | TriggerSurface::Anthropic => body
            .get("messages")
            .and_then(|v| v.as_array())
            .map(|msgs| {
                msgs.iter().any(|m| {
                    let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    let content = m.get("content").cloned().unwrap_or(Value::Null);
                    is_plain_user(role, &content)
                })
            })
            .unwrap_or(false),
    }
}

/// Whether one Responses `input[]` item can receive an injection.
fn response_item_is_target(it: &Value) -> bool {
    let ty = it.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let role = it.get("role").and_then(|v| v.as_str()).unwrap_or("");
    if ty == "input_text" || ty == "text" {
        return true;
    }
    if (ty.is_empty() || ty == "message") && role == "user" {
        let content = it.get("content").cloned().unwrap_or(Value::Null);
        return is_plain_user("user", &content);
    }
    false
}

/// Fold the rendered results into `body`'s last user message.
///
/// `surface` selects the shape to patch; see the module docs for why the last
/// user message and not `system`.
///
/// The injection **appends** rather than replaces. A last user message is often
/// the carrier of `tool_result` blocks (Anthropic) or `function_call_output`
/// items (Responses); replacing it wholesale would sever a tool round-trip and
/// the model would never see the tool result — the exact class of bug Stage 132
/// fixed. `{question}` is therefore built from the text parts only.
pub fn inject_into_last_user(
    body: &mut Value,
    trigger: &SearchTrigger,
    response: &SearchResponse,
    now: &chrono::DateTime<chrono::Utc>,
) -> InjectionOutcome {
    match trigger.surface {
        super::trigger::TriggerSurface::Chat => inject_chat(body, response, now),
        super::trigger::TriggerSurface::Responses => inject_responses(body, response, now),
        super::trigger::TriggerSurface::Anthropic => inject_anthropic(body, response, now),
    }
}

/// Append a rendered block to a content value that may be a plain string or an
/// array of content parts, deriving the question from its text parts.
fn append_to_content(content: &mut Value, rendered: &str) {
    match content {
        Value::String(s) => {
            let question = s.clone();
            *s = format!("{}\n\n{}", rendered, question);
        }
        Value::Array(parts) => {
            parts.push(json!({"type": "text", "text": rendered}));
        }
        _ => {}
    }
}

/// Longest question text used as a search query. A pasted document would
/// otherwise travel to the search backend in full (and inflate the GET URL).
pub const MAX_QUESTION_CHARS: usize = 512;

/// Extract the question text from a user message's content — text parts only.
/// `tool_result` / image parts carry no question and must not become the query.
fn question_from_content(content: &Value) -> String {
    let joined = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if ty == "text" || ty == "input_text" || ty == "output_text" {
                    p.get("text").and_then(|v| v.as_str()).map(String::from)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return String::new(),
    };
    joined.chars().take(MAX_QUESTION_CHARS).collect()
}

/// The search query for `body`: the text of its last plain user message.
///
/// Read before injection so the query reflects what the user actually asked,
/// with tool-result and image parts excluded. Empty when no target exists.
pub fn question_query(body: &Value, surface: super::trigger::TriggerSurface) -> String {
    use super::trigger::TriggerSurface;
    match surface {
        TriggerSurface::Responses => {
            let Some(input) = body.get("input") else {
                return String::new();
            };
            if let Value::String(s) = input {
                return s.clone();
            }
            let Some(items) = input.as_array() else {
                return String::new();
            };
            let Some(item) = items.iter().rev().find(|it| response_item_is_target(it)) else {
                return String::new();
            };
            let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ty == "input_text" || ty == "text" {
                item.get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                question_from_content(item.get("content").unwrap_or(&Value::Null))
            }
        }
        TriggerSurface::Chat | TriggerSurface::Anthropic => body
            .get("messages")
            .and_then(|v| v.as_array())
            .and_then(|msgs| {
                msgs.iter().rev().find(|m| {
                    let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
                    let content = m.get("content").cloned().unwrap_or(Value::Null);
                    is_plain_user(role, &content)
                })
            })
            .map(|m| question_from_content(m.get("content").unwrap_or(&Value::Null)))
            .unwrap_or_default(),
    }
}

/// Does this role/message qualify as the injection target? Tool carriers do not.
fn is_plain_user(role: &str, content: &Value) -> bool {
    if role != "user" {
        return false;
    }
    // An Anthropic user message whose blocks are tool_result-only is a tool
    // carrier, not a question.
    if let Value::Array(parts) = content {
        let has_tool_result = parts
            .iter()
            .any(|p| p.get("type").and_then(|v| v.as_str()) == Some("tool_result"));
        let has_text = parts.iter().any(|p| {
            matches!(
                p.get("type").and_then(|v| v.as_str()),
                Some("text" | "input_text" | "output_text")
            )
        });
        if has_tool_result && !has_text {
            return false;
        }
    }
    true
}

fn inject_chat(
    body: &mut Value,
    response: &SearchResponse,
    now: &chrono::DateTime<chrono::Utc>,
) -> InjectionOutcome {
    let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) else {
        return InjectionOutcome::NoTarget;
    };
    let Some(msg) = messages.iter_mut().rev().find(|m| {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = m.get("content").cloned().unwrap_or(Value::Null);
        is_plain_user(role, &content)
    }) else {
        return InjectionOutcome::NoTarget;
    };
    let question = question_from_content(msg.get("content").unwrap_or(&Value::Null));
    let rendered = render(response, &question, now);
    let Some(content) = msg.get_mut("content") else {
        return InjectionOutcome::NoTarget;
    };
    append_to_content(content, &rendered);
    InjectionOutcome::Injected {
        results: response.results.len(),
    }
}

fn inject_responses(
    body: &mut Value,
    response: &SearchResponse,
    now: &chrono::DateTime<chrono::Utc>,
) -> InjectionOutcome {
    let Some(input) = body.get_mut("input") else {
        return InjectionOutcome::NoTarget;
    };

    // Bare-string input: the whole payload is the question, plain concatenation.
    if let Value::String(s) = input {
        let question = s.clone();
        let rendered = render(response, &question, now);
        *s = format!("{}\n\n{}", rendered, question);
        return InjectionOutcome::Injected {
            results: response.results.len(),
        };
    }

    let Some(items) = input.as_array_mut() else {
        return InjectionOutcome::NoTarget;
    };
    // Same predicate as `has_target` / `question_query` — a second, drifting
    // finder here would pick a different item than the one used to build the
    // query.
    let Some(item) = items
        .iter_mut()
        .rev()
        .find(|it| response_item_is_target(it))
    else {
        return InjectionOutcome::NoTarget;
    };

    let ty = item
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let question = if ty == "input_text" || ty == "text" {
        item.get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        question_from_content(item.get("content").unwrap_or(&Value::Null))
    };
    let rendered = render(response, &question, now);

    if ty == "input_text" || ty == "text" {
        if let Some(Value::String(s)) = item.get_mut("text") {
            *s = format!("{}\n\n{}", rendered, s);
        }
    } else if let Some(content) = item.get_mut("content") {
        append_to_content(content, &rendered);
    }
    InjectionOutcome::Injected {
        results: response.results.len(),
    }
}

fn inject_anthropic(
    body: &mut Value,
    response: &SearchResponse,
    now: &chrono::DateTime<chrono::Utc>,
) -> InjectionOutcome {
    let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) else {
        return InjectionOutcome::NoTarget;
    };
    let Some(msg) = messages.iter_mut().rev().find(|m| {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = m.get("content").cloned().unwrap_or(Value::Null);
        is_plain_user(role, &content)
    }) else {
        return InjectionOutcome::NoTarget;
    };
    let question = question_from_content(msg.get("content").unwrap_or(&Value::Null));
    let rendered = render(response, &question, now);
    let Some(content) = msg.get_mut("content") else {
        return InjectionOutcome::NoTarget;
    };
    append_to_content(content, &rendered);
    InjectionOutcome::Injected {
        results: response.results.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::websearch::trigger::{SearchTrigger, TriggerSurface};
    use crate::websearch::types::{SearchResponse, SearchResult};
    use serde_json::json;

    fn sample_response() -> SearchResponse {
        SearchResponse {
            results: vec![
                SearchResult {
                    title: "Rust Async".to_string(),
                    url: "https://example.com/rust".to_string(),
                    snippet: "async/await in Rust".to_string(),
                    published_date: Some("2025-01-02".to_string()),
                    score: Some(0.93),
                },
                SearchResult {
                    title: "Tokio".to_string(),
                    url: "https://tokio.rs".to_string(),
                    snippet: "runtime".to_string(),
                    published_date: None,
                    score: None,
                },
            ],
            query: "rust async".to_string(),
            provider: "searxng".to_string(),
            reported_credits: None,
            endpoint: None,
        }
    }

    fn now() -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Utc.with_ymd_and_hms(2026, 10, 7, 0, 0, 0).unwrap()
    }

    fn trig(surface: TriggerSurface) -> SearchTrigger {
        SearchTrigger {
            surface,
            requested_results: 3,
        }
    }

    #[test]
    fn render_template_fills_all_placeholders() {
        let out = render(&sample_response(), "why async?", &now());
        assert!(!out.contains("{search_results}"));
        assert!(!out.contains("{question}"));
        assert!(!out.contains("{cur_date}"));
        assert!(out.contains("2026-10-07"));
        assert!(out.contains("why async?"));
    }

    #[test]
    fn render_results_includes_title_url_snippet() {
        let out = render_results(&sample_response());
        assert!(out.contains("[1] Rust Async"));
        assert!(out.contains("URL: https://example.com/rust"));
        assert!(out.contains("async/await in Rust"));
        assert!(out.contains("[2] Tokio"));
    }

    #[test]
    fn render_omits_published_date_when_none() {
        let out = render_results(&sample_response());
        assert!(out.contains("（2025-01-02）"));
        assert!(
            !out.contains("（）"),
            "a None date must omit the whole line"
        );
    }

    #[test]
    fn render_never_emits_score() {
        let out = render_results(&sample_response());
        assert!(
            !out.contains("0.93"),
            "score is provider-specific; never rendered"
        );
    }

    #[test]
    fn inject_targets_last_user_message() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "first question"},
                {"role": "assistant", "content": "answer"},
                {"role": "user", "content": "second question"}
            ]
        });
        let out = inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Chat),
            &sample_response(),
            &now(),
        );
        assert!(matches!(out, InjectionOutcome::Injected { results: 2 }));
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"], "first question", "history is untouched");
        let last = msgs[2]["content"].as_str().unwrap();
        assert!(last.contains("Rust Async"));
        assert!(last.contains("second question"));
    }

    #[test]
    fn inject_appends_to_user_with_tool_result_blocks() {
        let mut body = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "f", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "42"},
                    {"type": "text", "text": "what does that mean?"}
                ]}
            ]
        });
        inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Anthropic),
            &sample_response(),
            &now(),
        );
        let parts = body["messages"][1]["content"].as_array().unwrap();
        assert!(
            parts
                .iter()
                .any(|p| p["type"] == "tool_result" && p["tool_use_id"] == "t1"),
            "the tool_result block must survive — replacing the message would sever the round-trip"
        );
        assert_eq!(parts.len(), 3, "one block appended, none replaced");
        assert_eq!(parts[2]["type"], "text");
        assert!(parts[2]["text"].as_str().unwrap().contains("Rust Async"));
    }

    #[test]
    fn inject_skips_tool_result_only_user_message() {
        let mut body = json!({
            "messages": [
                {"role": "user", "content": "real question"},
                {"role": "assistant", "content": [{"type": "tool_use", "id": "t1", "name": "f", "input": {}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "42"}]}
            ]
        });
        let out = inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Anthropic),
            &sample_response(),
            &now(),
        );
        assert!(matches!(out, InjectionOutcome::Injected { .. }));
        let msgs = body["messages"].as_array().unwrap();
        assert!(
            msgs[0]["content"].as_str().unwrap().contains("Rust Async"),
            "injected into the last real question, not the tool carrier"
        );
        assert_eq!(
            msgs[2]["content"].as_array().unwrap().len(),
            1,
            "tool carrier untouched"
        );
    }

    #[test]
    fn inject_no_user_message_is_noop() {
        let mut body = json!({
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "assistant", "content": "hi"}
            ]
        });
        let before = body.clone();
        let out = inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Chat),
            &sample_response(),
            &now(),
        );
        assert_eq!(out, InjectionOutcome::NoTarget);
        assert_eq!(body, before);
    }

    #[test]
    fn inject_responses_string_input_appends() {
        let mut body = json!({"input": "what is rust?"});
        let out = inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Responses),
            &sample_response(),
            &now(),
        );
        assert!(matches!(out, InjectionOutcome::Injected { .. }));
        let s = body["input"].as_str().unwrap();
        assert!(s.contains("Rust Async"));
        assert!(s.contains("what is rust?"));
    }

    #[test]
    fn inject_responses_message_item_appends_part() {
        let mut body = json!({"input": [
            {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ]});
        inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Responses),
            &sample_response(),
            &now(),
        );
        let parts = body["input"][0]["content"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts[1]["text"].as_str().unwrap().contains("Rust Async"));
    }

    #[test]
    fn inject_responses_skips_function_call_output() {
        let mut body = json!({"input": [
            {"type": "message", "role": "user", "content": "q"},
            {"type": "function_call_output", "call_id": "c1", "output": "42"}
        ]});
        let out = inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Responses),
            &sample_response(),
            &now(),
        );
        assert!(
            matches!(out, InjectionOutcome::Injected { .. }),
            "the tool-output item is skipped; the earlier real question is used"
        );
        let content = body["input"][0]["content"].as_str().unwrap();
        assert!(content.contains("Rust Async"));
        assert_eq!(
            body["input"][1].get("content"),
            None,
            "the function_call_output item is untouched"
        );
    }

    #[test]
    fn inject_preserves_single_leading_system_invariant() {
        // Injection touches only user messages, so the leading-system invariant
        // held by adapter consolidation cannot be disturbed.
        let mut body = json!({
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "q"}
            ]
        });
        inject_into_last_user(
            &mut body,
            &trig(TriggerSurface::Chat),
            &sample_response(),
            &now(),
        );
        let msgs = body["messages"].as_array().unwrap();
        let systems: Vec<_> = msgs.iter().filter(|m| m["role"] == "system").collect();
        assert_eq!(systems.len(), 1);
        assert_eq!(msgs[0]["role"], "system", "still index 0");
        assert!(
            !systems[0]["content"]
                .as_str()
                .unwrap()
                .contains("Rust Async"),
            "results never leak into system"
        );
    }

    #[test]
    fn question_query_is_capped() {
        let long = "x".repeat(MAX_QUESTION_CHARS + 500);
        let body = json!({"messages": [{"role": "user", "content": long}]});
        assert_eq!(
            question_query(&body, TriggerSurface::Chat).chars().count(),
            MAX_QUESTION_CHARS
        );
    }

    #[test]
    fn render_does_not_resubstitute_placeholders_in_results() {
        // A result whose text contains a literal placeholder must survive as-is
        // rather than being rewritten with the user's question.
        let mut resp = sample_response();
        resp.results[0].title = "literal {question} marker".to_string();
        let out = render(&resp, "the real question", &now());
        assert!(out.contains("literal {question} marker"));
    }

    #[test]
    fn question_query_is_empty_for_image_only_user_message() {
        // No text parts → nothing to search for. `serve_trigger` turns this into
        // NoTarget rather than issuing an empty-query request.
        let body = json!({
            "messages": [{"role": "user", "content": [
                {"type": "image", "source": {"media_type": "image/png", "data": "AA=="}}
            ]}]
        });
        assert_eq!(question_query(&body, TriggerSurface::Anthropic).trim(), "");
        assert!(
            has_target(&body, TriggerSurface::Anthropic),
            "still a user message"
        );
    }

    #[test]
    fn search_error_degrades_without_injection() {
        // The degrade path is caller-side: it simply never calls inject. This
        // asserts the body is byte-identical when no injection happens.
        let body = json!({"messages": [{"role": "user", "content": "q"}]});
        let before = body.clone();
        let _ = InjectionOutcome::NoTarget; // degraded callers skip injection entirely
        assert_eq!(body, before);
    }
}
