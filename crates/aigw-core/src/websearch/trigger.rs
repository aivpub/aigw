//! Trigger detection for the three client surfaces (Phase 53, Stage 135).
//!
//! Each surface announces "please search" with its own native shape:
//!
//! | surface | shape |
//! |---------|-------|
//! | OpenAI Chat | top-level `web_search_options` (an empty object counts) |
//! | OpenAI Responses | a `tools[]` entry of `type` `web_search` / `web_search_preview` |
//! | Anthropic | a `tools[]` entry of `type` `web_search_20250305` |
//!
//! Detection is a pure function over the request JSON — no provider knowledge,
//! no IO. Which backend ends up serving the query is [`super::WebSearchRegistry`]'s
//! business, not this layer's.

use serde_json::Value;

/// Which client surface asked for a search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSurface {
    Chat,
    Responses,
    Anthropic,
}

impl TriggerSurface {
    /// Log / metadata label.
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerSurface::Chat => "chat",
            TriggerSurface::Responses => "responses",
            TriggerSurface::Anthropic => "anthropic",
        }
    }
}

/// A detected search request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTrigger {
    /// Which surface hit, for logging and Stage 136 attribution.
    pub surface: TriggerSurface,
    /// How many results the client asked for. An upper bound only — the
    /// registry clamps it against the server-side ceiling before any provider
    /// sees it ([`super::Guardrails::clamp_max_results`]).
    pub requested_results: usize,
}

/// `search_context_size` low/medium/high → 1/3/5 results.
///
/// Mirrors Higress `ai-search`, which repurposes OpenAI's native parameter as
/// its own switch, so the parameter means the same thing to the client as it
/// would against OpenAI. Unknown strings degrade to medium with a warning
/// rather than erroring — a bad hint must not break the request.
pub fn map_context_size(value: Option<&str>) -> usize {
    match value {
        Some("low") => 1,
        Some("medium") | None => 3,
        Some("high") => 5,
        Some(other) => {
            tracing::warn!(
                value = %other,
                "unknown search_context_size, defaulting to medium (3 results)"
            );
            3
        }
    }
}

/// Detect OpenAI Chat `web_search_options`.
///
/// **Consumes** the field: the returned trigger means the field has been
/// removed from `body`. Leaving it in would forward an OpenAI-native parameter
/// to an upstream that does not understand it (`OpenAIPassthrough` copies every
/// field it does not touch), letting the upstream decide between ignoring and
/// rejecting it.
pub fn detect_chat(body: &mut Value) -> Option<SearchTrigger> {
    let obj = body.as_object_mut()?;
    let options = obj.remove("web_search_options")?;
    let context_size = options.get("search_context_size").and_then(|v| v.as_str());
    Some(SearchTrigger {
        surface: TriggerSurface::Chat,
        requested_results: map_context_size(context_size),
    })
}

/// Detect OpenAI Responses `web_search` / `web_search_preview` tools.
///
/// Read-only: removal from the forwarded `tools[]` is [`normalize_responses_tools`]'s
/// existing job (the `other =>` arm), which already drops both types and
/// cascades the cleanup to `tool_choice`.
///
/// [`normalize_responses_tools`]: crate::adapter
pub fn detect_responses(tools: &[Value]) -> Option<SearchTrigger> {
    let hit = tools.iter().find(|t| {
        matches!(
            t.get("type").and_then(|v| v.as_str()),
            Some("web_search" | "web_search_preview")
        )
    })?;
    let context_size = hit.get("search_context_size").and_then(|v| v.as_str());
    Some(SearchTrigger {
        surface: TriggerSurface::Responses,
        requested_results: map_context_size(context_size),
    })
}

/// Detect Anthropic `web_search_20250305` tools.
///
/// Read-only; the tool is dropped from the forwarded tool list by the
/// `ClaudeToolDef` mapping (server tools never become upstream function tools).
///
/// `max_uses` counts *searches*, not results, and Anthropic defines no default
/// for it. This Stage performs exactly one search, so `max_uses` serves only as
/// an upper bound on the result count; absent, it falls back to medium.
pub fn detect_anthropic(tools: &[Value]) -> Option<SearchTrigger> {
    let hit = tools
        .iter()
        .find(|t| t.get("type").and_then(|v| v.as_str()) == Some("web_search_20250305"))?;
    let requested = hit
        .get("max_uses")
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .map(|n| n as usize)
        .unwrap_or_else(|| map_context_size(None));
    Some(SearchTrigger {
        surface: TriggerSurface::Anthropic,
        requested_results: requested,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detect_chat_web_search_options_hits() {
        let mut body = json!({"model": "m", "web_search_options": {}});
        let t = detect_chat(&mut body).expect("empty options object still counts");
        assert_eq!(t.surface, TriggerSurface::Chat);
        assert_eq!(
            t.requested_results, 3,
            "absent context size defaults to medium"
        );
        assert!(
            body.get("web_search_options").is_none(),
            "the field must be consumed, never forwarded upstream"
        );
    }

    #[test]
    fn detect_chat_reads_context_size() {
        let mut body = json!({"web_search_options": {"search_context_size": "low"}});
        assert_eq!(detect_chat(&mut body).unwrap().requested_results, 1);
        let mut body = json!({"web_search_options": {"search_context_size": "high"}});
        assert_eq!(detect_chat(&mut body).unwrap().requested_results, 5);
    }

    #[test]
    fn detect_chat_absent_is_none() {
        let mut body = json!({"model": "m", "messages": []});
        let before = body.clone();
        assert!(detect_chat(&mut body).is_none());
        assert_eq!(body, before, "no trigger must leave the body untouched");
    }

    #[test]
    fn detect_responses_web_search_and_preview_both_hit() {
        for ty in ["web_search", "web_search_preview"] {
            let tools = vec![json!({"type": ty})];
            let t = detect_responses(&tools).expect("both spellings hit");
            assert_eq!(t.surface, TriggerSurface::Responses);
            assert_eq!(t.requested_results, 3);
        }
    }

    #[test]
    fn detect_responses_other_server_tools_not_hit() {
        let tools = vec![
            json!({"type": "code_interpreter"}),
            json!({"type": "mcp", "server_label": "x"}),
            json!({"type": "function", "name": "f"}),
        ];
        assert!(detect_responses(&tools).is_none());
    }

    #[test]
    fn detect_anthropic_web_search_20250305_hits() {
        let tools =
            vec![json!({"type": "web_search_20250305", "name": "web_search", "max_uses": 5})];
        let t = detect_anthropic(&tools).unwrap();
        assert_eq!(t.surface, TriggerSurface::Anthropic);
        assert_eq!(t.requested_results, 5, "max_uses bounds the result count");
    }

    #[test]
    fn detect_anthropic_without_max_uses_defaults_medium() {
        let tools = vec![json!({"type": "web_search_20250305", "name": "web_search"})];
        assert_eq!(detect_anthropic(&tools).unwrap().requested_results, 3);
    }

    #[test]
    fn detect_anthropic_client_tool_not_hit() {
        let tools = vec![json!({
            "name": "get_weather",
            "input_schema": {"type": "object", "properties": {}}
        })];
        assert!(detect_anthropic(&tools).is_none());
    }

    #[test]
    fn context_size_maps_low_medium_high_and_unknown() {
        assert_eq!(map_context_size(Some("low")), 1);
        assert_eq!(map_context_size(Some("medium")), 3);
        assert_eq!(map_context_size(Some("high")), 5);
        assert_eq!(
            map_context_size(Some("gigantic")),
            3,
            "unknown degrades to medium"
        );
        assert_eq!(map_context_size(None), 3);
    }
}
