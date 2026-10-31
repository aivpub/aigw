//! Tool aggregation for the shared `/mcp` endpoint (Phase 55, Stage 144).
//!
//! One gateway endpoint fronts N upstream servers, so every tool name on the
//! wire is `{alias}__{tool}`. Splitting always takes the FIRST `__`: an upstream
//! tool may itself contain `_` (`get_user_issue`), and a greedy split would turn
//! `github__get_user_issue` into `github__get` + `user_issue`. The alias rules
//! (`validate_alias` forbids `__` and a trailing `_`) make that first `__` the
//! real separator by construction.
//!
//! Ordering is by `(alias, tool)` and nothing else. A list that reorders between
//! two calls invalidates the client's prompt-cache prefix, which is a cost
//! multiplier rather than a cosmetic wobble.
//!
//! Pagination belongs to the gateway, not to any upstream. Echoing an upstream's
//! `nextCursor` would hand the client a cursor that walks one upstream's
//! remainder, leaving the other servers' tools permanently unreachable; the
//! cursor here is the index of the next entry in the merged list.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::models::UpstreamTool;

pub const SEPARATOR: &str = "__";
pub const DEFAULT_PAGE_SIZE: usize = 100;

/// One merged tool: the prefix it is served under and the upstream tool behind it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolEntry {
    pub name: String,
    pub alias: String,
    pub tool: UpstreamTool,
}

impl ToolEntry {
    pub fn conflicted(&self) -> bool {
        self.tool.has_conflict
    }

    /// The MCP `Tool` object as the client sees it. The upstream name never
    /// appears here — the prefix replaces it, once, at the aggregation boundary.
    pub fn to_wire(&self) -> Value {
        let mut wire = json!({
            "name": self.name,
            "description": self.tool.description,
            "inputSchema": self.tool.input_schema,
        });
        if let Some(annotations) = &self.tool.annotations {
            wire["annotations"] = annotations.clone();
        }
        wire
    }
}

pub fn prefixed(alias: &str, tool_name: &str) -> String {
    format!("{alias}{SEPARATOR}{tool_name}")
}

/// Split `{alias}__{tool}` on the first `__`. `None` when either half would be
/// empty — that is a name the gateway never issues, so it is not a tool call.
pub fn split_prefixed(name: &str) -> Option<(&str, &str)> {
    let (alias, tool) = name.split_once(SEPARATOR)?;
    (!alias.is_empty() && !tool.is_empty()).then_some((alias, tool))
}

/// The one `token_owner` an aggregate request can report.
///
/// `anonymous` when no upstream was contacted at all (no credential left the
/// gateway — that statement is true). Otherwise the shared value, or `mixed`
/// when they disagree: picking the weakest would call a mostly-credentialed
/// fan-out anonymous, and picking the strongest would claim credentials that
/// never left, so neither can be derived from the others.
pub fn merge_owners(owners: &[&str]) -> &'static str {
    let Some((first, rest)) = owners.split_first() else {
        return "anonymous";
    };
    if rest.iter().all(|o| o == first) {
        return match *first {
            "server" => "server",
            "user" => "user",
            _ => "anonymous",
        };
    }
    "mixed"
}

/// Merge every server's tools into one deterministically ordered list.
///
/// A name that still collides after prefixing is flagged on every colliding
/// entry and kept: dropping one would silently point the client's name at the
/// other implementation, and overwriting would do the same with less evidence.
pub fn merge(sources: impl IntoIterator<Item = (String, Vec<UpstreamTool>)>) -> Vec<ToolEntry> {
    let mut entries: Vec<ToolEntry> = Vec::new();
    for (alias, tools) in sources {
        for tool in tools {
            entries.push(ToolEntry {
                name: prefixed(&alias, &tool.tool_name),
                alias: alias.clone(),
                tool,
            });
        }
    }

    entries.sort_by(|a, b| {
        (a.alias.as_str(), a.tool.tool_name.as_str())
            .cmp(&(b.alias.as_str(), b.tool.tool_name.as_str()))
    });

    let mut counts: HashMap<String, usize> = HashMap::new();
    for entry in &entries {
        *counts.entry(entry.name.clone()).or_default() += 1;
    }
    for entry in entries.iter_mut() {
        if counts.get(&entry.name).copied().unwrap_or(0) > 1 {
            entry.tool.has_conflict = true;
        }
    }
    entries
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub entries: Vec<ToolEntry>,
    pub next_cursor: Option<String>,
}

/// One gateway page. `cursor` is a gateway sequence number, never an upstream
/// cursor; anything unparseable is treated as past the end rather than as zero,
/// because restarting a client's pagination is an endless loop while a short
/// listing is at least visible.
pub fn page(entries: &[ToolEntry], cursor: Option<&str>, page_size: usize) -> Page {
    let size = page_size.max(1);
    let start = match cursor.map(str::trim) {
        None | Some("") => 0,
        Some(raw) => raw.parse::<usize>().unwrap_or(usize::MAX),
    }
    .min(entries.len());
    let end = start.saturating_add(size).min(entries.len());
    let next_cursor = if end < entries.len() {
        Some(end.to_string())
    } else {
        None
    };
    Page {
        entries: entries[start..end].to_vec(),
        next_cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> UpstreamTool {
        UpstreamTool {
            tool_name: name.to_string(),
            description: format!("{name} tool"),
            input_schema: json!({"type": "object"}),
            annotations: None,
            has_conflict: false,
        }
    }

    fn source(alias: &str, names: &[&str]) -> (String, Vec<UpstreamTool>) {
        (
            alias.to_string(),
            names.iter().map(|n| tool(n)).collect::<Vec<_>>(),
        )
    }

    fn names(entries: &[ToolEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn two_servers_merge_with_their_prefixes() {
        let merged = merge([
            source("github", &["search", "get_issue"]),
            source("wiki", &["read", "edit"]),
        ]);
        assert_eq!(merged.len(), 4);
        assert_eq!(
            names(&merged),
            vec![
                "github__get_issue",
                "github__search",
                "wiki__edit",
                "wiki__read"
            ]
        );
        assert_eq!(merged[0].alias, "github");
        assert_eq!(merged[0].tool.tool_name, "get_issue");
        assert!(merged.iter().all(|e| !e.conflicted()));
    }

    #[test]
    fn underscore_in_an_upstream_tool_name_is_not_a_separator() {
        let merged = merge([source("github", &["get_user_issue"])]);
        assert_eq!(merged[0].name, "github__get_user_issue");
        assert_eq!(
            split_prefixed(&merged[0].name),
            Some(("github", "get_user_issue"))
        );
    }

    #[test]
    fn split_takes_the_first_separator_only() {
        assert_eq!(split_prefixed("a__b__c"), Some(("a", "b__c")));
        assert_eq!(split_prefixed("a__b"), Some(("a", "b")));
        assert_eq!(split_prefixed("plain"), None);
        assert_eq!(split_prefixed("__leading"), None);
        assert_eq!(split_prefixed("trailing__"), None);
    }

    #[test]
    fn every_tool_from_every_source_survives_the_merge() {
        let merged = merge([
            source("a", &["one", "two", "three"]),
            source("b", &["four", "five"]),
        ]);
        assert_eq!(merged.len(), 5);
        assert_eq!(
            names(&merged),
            vec!["a__one", "a__three", "a__two", "b__five", "b__four"]
        );
    }

    #[test]
    fn ordering_is_identical_across_calls() {
        let build = || {
            merge([
                source("wiki", &["read", "edit", "search"]),
                source("github", &["search", "get_issue"]),
            ])
        };
        assert_eq!(names(&build()), names(&build()));
    }

    #[test]
    fn gateway_pages_do_not_overlap_and_the_cursor_is_a_sequence_number() {
        let merged = merge([source("a", &["t1", "t2", "t3", "t4", "t5"])]);
        let first = page(&merged, None, 2);
        assert_eq!(names(&first.entries), vec!["a__t1", "a__t2"]);
        assert_eq!(first.next_cursor.as_deref(), Some("2"));

        let second = page(&merged, first.next_cursor.as_deref(), 2);
        assert_eq!(names(&second.entries), vec!["a__t3", "a__t4"]);
        assert_eq!(second.next_cursor.as_deref(), Some("4"));

        let third = page(&merged, second.next_cursor.as_deref(), 2);
        assert_eq!(names(&third.entries), vec!["a__t5"]);
        assert_eq!(third.next_cursor, None);

        let overlap: Vec<&str> = first
            .entries
            .iter()
            .filter(|e| second.entries.iter().any(|s| s.name == e.name))
            .map(|e| e.name.as_str())
            .collect();
        assert!(overlap.is_empty(), "pages overlap: {overlap:?}");
    }

    #[test]
    fn a_single_page_has_no_cursor_and_an_empty_list_is_empty() {
        let merged = merge([source("a", &["t1"])]);
        let only = page(&merged, None, DEFAULT_PAGE_SIZE);
        assert_eq!(only.entries.len(), 1);
        assert_eq!(only.next_cursor, None);

        let none: Vec<ToolEntry> = Vec::new();
        let empty = page(&none, None, DEFAULT_PAGE_SIZE);
        assert!(empty.entries.is_empty());
        assert_eq!(empty.next_cursor, None);
    }

    #[test]
    fn an_unparseable_cursor_ends_the_listing_instead_of_restarting_it() {
        let merged = merge([source("a", &["t1", "t2"])]);
        let bad = page(&merged, Some("not-a-number"), DEFAULT_PAGE_SIZE);
        assert!(bad.entries.is_empty());
        assert_eq!(bad.next_cursor, None);

        let past_end = page(&merged, Some("99"), DEFAULT_PAGE_SIZE);
        assert!(past_end.entries.is_empty());
        assert_eq!(past_end.next_cursor, None);
    }

    #[test]
    fn a_name_that_still_collides_is_flagged_and_kept() {
        let mut dup = tool("search");
        dup.description = "the other one".to_string();
        let merged = merge([source("a", &["search"]), ("a".to_string(), vec![dup])]);
        assert_eq!(merged.len(), 2, "nothing is dropped");
        assert!(merged.iter().all(ToolEntry::conflicted));
    }

    #[test]
    fn a_probe_level_conflict_survives_into_the_merged_entry() {
        let mut flagged = tool("search");
        flagged.has_conflict = true;
        let merged = merge([("a".to_string(), vec![flagged])]);
        assert!(merged[0].conflicted());
    }

    #[test]
    fn wire_form_carries_the_prefixed_name_and_the_upstream_schema() {
        let merged = merge([source("github", &["search"])]);
        let wire = merged[0].to_wire();
        assert_eq!(wire["name"], "github__search");
        assert_eq!(wire["description"], "search tool");
        assert_eq!(wire["inputSchema"]["type"], "object");
        assert!(wire.get("annotations").is_none());
    }

    #[test]
    fn wire_form_keeps_annotations_when_the_upstream_sent_them() {
        let mut annotated = tool("search");
        annotated.annotations = Some(json!({"readOnlyHint": true}));
        let merged = merge([("a".to_string(), vec![annotated])]);
        assert_eq!(merged[0].to_wire()["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn no_upstream_contacted_reports_anonymous() {
        assert_eq!(merge_owners(&[]), "anonymous");
    }

    #[test]
    fn a_uniform_fan_out_reports_the_shared_value() {
        assert_eq!(merge_owners(&["server", "server", "server"]), "server");
        assert_eq!(merge_owners(&["user"]), "user");
        assert_eq!(merge_owners(&["anonymous", "anonymous"]), "anonymous");
    }

    #[test]
    fn disagreeing_owners_report_mixed_rather_than_a_direction() {
        assert_eq!(merge_owners(&["server", "anonymous"]), "mixed");
        assert_eq!(merge_owners(&["user", "server", "user"]), "mixed");
    }

    #[test]
    fn page_restarts_from_zero_when_no_cursor_is_given() {
        let entries = merge([source("a", &["1", "2", "3"])]);
        let first = page(&entries, None, 2);
        assert_eq!(first.entries.len(), 2);
        assert_eq!(first.next_cursor.as_deref(), Some("2"));
        let second = page(&entries, first.next_cursor.as_deref(), 2);
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.next_cursor, None);
    }
}
