//! Process-level catalog cache (ADR-041).
//!
//! `mcp_tool_catalog` is the durable snapshot; this is the hot copy that keeps a
//! `tools/list` from touching the DB. It is a **process static**, not an
//! `AppState` field: the cache needs no per-instance configuration, each replica
//! gets its own (design §7), and adding a field would mean editing 39
//! `AppState { .. }` literals for no gain.
//!
//! The lock is `std::sync::Mutex`, so a guard must never be held across an
//! `.await` — that would block a tokio worker for the duration of an upstream
//! round trip. Every accessor here clones what it needs and drops the guard
//! before returning: the payload is an `Arc<Vec<McpToolRow>>`, so the clone is a
//! refcount bump, not a copy of the tool list.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::models::{McpToolRow, UpstreamTool};

/// Fallback TTL when the upstream's `server/discover` gave no `ttlMs`.
pub const DEFAULT_TTL_MS: i64 = 60_000;

#[derive(Debug, Clone)]
pub struct Entry {
    pub tools: Arc<Vec<McpToolRow>>,
    pub fetched_at: String,
    pub ttl_ms: i64,
}

impl Entry {
    /// True when the entry is past its TTL. An unparseable `fetched_at` counts as
    /// expired — serving an entry whose age nobody can state is worse than
    /// re-reading a cheap table.
    pub fn is_expired(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        chrono::DateTime::parse_from_rfc3339(&self.fetched_at)
            .map(|at| now.timestamp_millis() - at.timestamp_millis() >= self.ttl_ms)
            .unwrap_or(true)
    }

    pub fn upstream_tools(&self) -> Vec<UpstreamTool> {
        self.tools.iter().map(UpstreamTool::from_row).collect()
    }
}

fn cache() -> &'static Mutex<HashMap<i64, Entry>> {
    static CACHE: OnceLock<Mutex<HashMap<i64, Entry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<i64, Entry>> {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A fresh entry for `server_id`, or `None` when absent or past its TTL.
pub fn get(server_id: i64) -> Option<Entry> {
    let guard = lock();
    let entry = guard.get(&server_id)?;
    if entry.is_expired(chrono::Utc::now()) {
        return None;
    }
    Some(entry.clone())
}

/// Read without the TTL check — for callers that must show a stale snapshot
/// rather than nothing (the tools dialog after a failed re-probe).
pub fn get_stale(server_id: i64) -> Option<Entry> {
    lock().get(&server_id).cloned()
}

pub fn put(server_id: i64, tools: Vec<McpToolRow>, fetched_at: &str, ttl_ms: i64) {
    let entry = Entry {
        tools: Arc::new(tools),
        fetched_at: fetched_at.to_string(),
        ttl_ms,
    };
    lock().insert(server_id, entry);
}

pub fn invalidate(server_id: i64) {
    lock().remove(&server_id);
}

pub fn clear() {
    lock().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(server_id: i64, name: &str) -> McpToolRow {
        McpToolRow {
            id: 0,
            server_id,
            tool_name: name.to_string(),
            description: String::new(),
            input_schema: serde_json::json!({}),
            annotations: None,
            has_conflict: false,
            fetched_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn put_and_get_roundtrip() {
        clear();
        let now = chrono::Utc::now().to_rfc3339();
        put(1, vec![row(1, "search")], &now, DEFAULT_TTL_MS);
        let entry = get(1).expect("cached");
        assert_eq!(entry.tools.len(), 1);
        assert_eq!(entry.upstream_tools()[0].tool_name, "search");
        assert!(!entry.is_expired(chrono::Utc::now()));
        clear();
    }

    #[test]
    fn invalidate_removes_only_that_server() {
        clear();
        put(
            1,
            vec![row(1, "a")],
            &chrono::Utc::now().to_rfc3339(),
            DEFAULT_TTL_MS,
        );
        put(
            2,
            vec![row(2, "b")],
            &chrono::Utc::now().to_rfc3339(),
            DEFAULT_TTL_MS,
        );
        invalidate(1);
        assert!(get(1).is_none());
        assert!(get(2).is_some());
        clear();
        assert!(get(2).is_none());
    }

    #[test]
    fn expired_entries_are_not_served_but_stay_readable_as_stale() {
        clear();
        put(9, vec![row(9, "a")], "2020-01-01T00:00:00Z", 1);
        assert!(get(9).is_none(), "TTL long past");
        assert!(get_stale(9).is_some());
        clear();
    }

    #[test]
    fn unparseable_fetched_at_counts_as_expired() {
        let entry = Entry {
            tools: Arc::new(Vec::new()),
            fetched_at: "not-a-date".to_string(),
            ttl_ms: DEFAULT_TTL_MS,
        };
        assert!(entry.is_expired(chrono::Utc::now()));
    }
}
