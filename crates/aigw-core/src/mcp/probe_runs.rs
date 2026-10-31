//! Progress registry for the "re-probe every enabled server" run (Stage 148).
//!
//! The old `POST /admin/mcp/reload` ran every probe inline and returned once the
//! last one finished — up to `PROBE_TIMEOUT` per server with nothing to look at
//! in between. This module is the state that makes that run observable: one
//! process-level snapshot the operator can poll while the probes are still going.
//!
//! # Why a process static and not `AppState`
//!
//! Same reasoning as [`super::catalog_cache`] (ADR-041): the registry has no
//! per-instance configuration, every replica keeps its own, and adding a field
//! would mean editing every `AppState { .. }` literal for a value nobody can
//! derive anything else from.
//!
//! # Locking discipline
//!
//! The lock is a `std::sync::Mutex`, so a guard must never be held across an
//! `.await`. The probe loop takes the lock once per state transition, mutates,
//! clones what it needs and drops it — the run is a few dozen small records, and
//! the clone happens only when a caller asks for a snapshot.
//!
//! # `done` and `status` cannot disagree
//!
//! `done` increments exactly once per item, on the transition into a terminal
//! state, and `status` is derived from the items rather than stored. A run whose
//! footer says "7/7" must therefore contain seven terminal items — the classic
//! "counter says finished, list says still running" bug has no field to live in.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemState {
    Pending,
    Running,
    Ok,
    Failed,
}

impl ItemState {
    fn is_terminal(self) -> bool {
        matches!(self, ItemState::Ok | ItemState::Failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Ok,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeRunItem {
    pub server_id: i64,
    pub alias: String,
    pub state: ItemState,
    /// Set on `Ok`; `None` while the probe has not produced a catalog yet.
    pub tool_count: Option<usize>,
    pub protocol_version: Option<String>,
    /// One human-readable string, not a structured error: the operator wants to
    /// read what the upstream said, not parse it.
    pub error: Option<String>,
}

impl ProbeRunItem {
    pub fn pending(server_id: i64, alias: impl Into<String>) -> Self {
        Self {
            server_id,
            alias: alias.into(),
            state: ItemState::Pending,
            tool_count: None,
            protocol_version: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeRun {
    pub run_id: String,
    pub total: usize,
    pub done: usize,
    pub status: RunStatus,
    pub items: Vec<ProbeRunItem>,
}

impl ProbeRun {
    /// A fresh run: every item `Pending`, nothing counted as done.
    ///
    /// A run with no servers is `Ok` immediately — an empty job that reports
    /// `Running` forever would make the UI poll a finished run.
    pub fn new(run_id: impl Into<String>, items: Vec<ProbeRunItem>) -> Self {
        let mut run = Self {
            run_id: run_id.into(),
            total: items.len(),
            done: 0,
            status: RunStatus::Running,
            items,
        };
        run.recompute();
        run
    }

    fn recompute(&mut self) {
        self.done = self.items.iter().filter(|i| i.state.is_terminal()).count();
        self.status = if self.done < self.total {
            RunStatus::Running
        } else if self.items.iter().any(|i| i.state == ItemState::Failed) {
            RunStatus::Partial
        } else {
            RunStatus::Ok
        };
    }

    fn item_mut(&mut self, server_id: i64) -> Option<&mut ProbeRunItem> {
        self.items.iter_mut().find(|i| i.server_id == server_id)
    }
}

/// The run already in flight, returned by [`registry::start`] so the caller can
/// answer `409` with the id the operator should attach to instead of guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInProgress {
    pub run_id: String,
}

pub mod registry {
    use super::*;

    static RUN: OnceLock<Mutex<Option<ProbeRun>>> = OnceLock::new();

    fn cell() -> &'static Mutex<Option<ProbeRun>> {
        RUN.get_or_init(|| Mutex::new(None))
    }

    fn with<R>(f: impl FnOnce(&mut Option<ProbeRun>) -> R) -> R {
        let mut guard = cell().lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    /// Claim the registry for a new run.
    ///
    /// The "is something running?" check and the write happen under one lock, so
    /// two concurrent reloads cannot both observe an idle registry and both
    /// start — the losing caller gets the winner's `run_id`.
    ///
    /// A finished run is overwritten rather than kept: the previous result is
    /// stale the moment a new run starts, and a `run_id` that still answers
    /// would show an operator numbers that describe a different sweep.
    pub fn start(run: ProbeRun) -> Result<(), RunInProgress> {
        with(|slot| {
            if let Some(existing) = slot.as_ref() {
                if existing.status == RunStatus::Running {
                    return Err(RunInProgress {
                        run_id: existing.run_id.clone(),
                    });
                }
            }
            *slot = Some(run);
            Ok(())
        })
    }

    pub fn item_running(server_id: i64) {
        with(|slot| {
            if let Some(run) = slot.as_mut() {
                if let Some(item) = run.item_mut(server_id) {
                    item.state = ItemState::Running;
                }
            }
        })
    }

    pub fn item_ok(server_id: i64, tool_count: usize, protocol_version: String) {
        with(|slot| {
            if let Some(run) = slot.as_mut() {
                if let Some(item) = run.item_mut(server_id) {
                    item.state = ItemState::Ok;
                    item.tool_count = Some(tool_count);
                    item.protocol_version = Some(protocol_version);
                    item.error = None;
                }
                run.recompute();
            }
        })
    }

    pub fn item_failed(server_id: i64, error: impl Into<String>) {
        with(|slot| {
            if let Some(run) = slot.as_mut() {
                if let Some(item) = run.item_mut(server_id) {
                    item.state = ItemState::Failed;
                    item.error = Some(error.into());
                }
                run.recompute();
            }
        })
    }

    pub fn latest() -> Option<ProbeRun> {
        with(|slot| slot.clone())
    }

    pub fn get(run_id: &str) -> Option<ProbeRun> {
        with(|slot| slot.as_ref().filter(|run| run.run_id == run_id).cloned())
    }

    /// Test-only: the registry is a process static, so cases share it.
    pub fn reset() {
        with(|slot| *slot = None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry is one process-wide static, so these cases must not overlap:
    /// a test that starts a run would otherwise make its neighbour's `start` a
    /// `409`. Poisoning is ignored — a failed case should fail on its own
    /// assertion, not by poisoning everyone else's guard.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn items(n: usize) -> Vec<ProbeRunItem> {
        (1..=n as i64)
            .map(|id| ProbeRunItem::pending(id, format!("srv-{id}")))
            .collect()
    }

    fn fresh(run_id: &str, n: usize) -> ProbeRun {
        registry::reset();
        ProbeRun::new(run_id, items(n))
    }

    #[test]
    fn a_fresh_run_is_all_pending_and_not_done() {
        let _guard = serial();
        let run = fresh("run-1", 3);
        assert_eq!(run.done, 0);
        assert_eq!(run.total, 3);
        assert_eq!(run.status, RunStatus::Running);
        assert!(run.items.iter().all(|i| i.state == ItemState::Pending));
    }

    #[test]
    fn done_advances_only_on_the_terminal_transition() {
        let _guard = serial();
        let run = fresh("run-1", 2);
        registry::start(run).expect("first start");

        registry::item_running(1);
        assert_eq!(registry::latest().unwrap().done, 0);

        registry::item_ok(1, 7, "2026-07-28".to_string());
        let after = registry::latest().unwrap();
        assert_eq!(after.done, 1);
        assert_eq!(after.items[0].tool_count, Some(7));
        assert_eq!(
            after.items[0].protocol_version.as_deref(),
            Some("2026-07-28")
        );
        assert_eq!(after.status, RunStatus::Running);
    }

    #[test]
    fn a_failed_item_counts_as_done_and_carries_its_error() {
        let _guard = serial();
        let run = fresh("run-1", 2);
        registry::start(run).expect("first start");

        registry::item_running(1);
        registry::item_failed(1, "502 upstream exploded");
        let after = registry::latest().unwrap();
        assert_eq!(after.done, 1);
        assert_eq!(after.items[0].state, ItemState::Failed);
        assert_eq!(
            after.items[0].error.as_deref(),
            Some("502 upstream exploded")
        );
    }

    #[test]
    fn every_item_ok_makes_the_run_ok() {
        let _guard = serial();
        let run = fresh("run-1", 2);
        registry::start(run).expect("first start");
        registry::item_ok(1, 1, "v1".into());
        registry::item_ok(2, 2, "v1".into());

        let after = registry::latest().unwrap();
        assert_eq!(after.done, 2);
        assert_eq!(after.status, RunStatus::Ok);
    }

    #[test]
    fn one_failure_makes_the_run_partial() {
        let _guard = serial();
        let run = fresh("run-1", 2);
        registry::start(run).expect("first start");
        registry::item_ok(1, 1, "v1".into());
        registry::item_failed(2, "timeout");

        assert_eq!(registry::latest().unwrap().status, RunStatus::Partial);
    }

    #[test]
    fn starting_a_second_run_replaces_the_first() {
        let _guard = serial();
        let mut first = fresh("run-1", 1);
        first.items[0].state = ItemState::Ok;
        first.recompute();
        registry::start(first).expect("first start");

        registry::start(ProbeRun::new("run-2", items(1))).expect("second start");

        assert_eq!(registry::latest().unwrap().run_id, "run-2");
        assert!(
            registry::get("run-1").is_none(),
            "the superseded run must stop answering"
        );
    }

    #[test]
    fn starting_while_running_is_refused_and_leaves_the_run_untouched() {
        let _guard = serial();
        registry::reset();
        registry::start(ProbeRun::new("run-1", items(2))).expect("first start");

        let err = registry::start(ProbeRun::new("run-2", items(2))).expect_err("must refuse");
        assert_eq!(err.run_id, "run-1");
        assert_eq!(registry::latest().unwrap().run_id, "run-1");
    }

    #[test]
    fn reset_empties_the_registry() {
        let _guard = serial();
        registry::reset();
        registry::start(ProbeRun::new("run-1", items(1))).expect("start");
        registry::reset();
        assert!(registry::latest().is_none());
    }

    #[test]
    fn a_run_with_no_servers_is_ok_not_running() {
        let _guard = serial();
        registry::reset();
        let run = ProbeRun::new("run-empty", Vec::new());
        assert_eq!(run.total, 0);
        assert_eq!(run.done, 0);
        assert_eq!(run.status, RunStatus::Ok);
        registry::start(run).expect("an empty run must be startable");
    }

    #[test]
    fn an_item_finished_before_it_was_marked_running_still_counts() {
        let _guard = serial();
        // Both the timeout path and the transport-error path can report a
        // terminal state without ever setting `Running`; `done` must not depend
        // on having seen the intermediate state.
        let run = fresh("run-1", 1);
        registry::start(run).expect("start");
        registry::item_failed(1, "never connected");
        assert_eq!(registry::latest().unwrap().done, 1);
    }
}
