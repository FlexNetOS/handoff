// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! handoff-core — shared continuity primitives extracted from the `hf` monolith.
//!
//! The first peeled-off crate of the 12-crate decomposition (ADR-0019 D5 #4, PRD §7.2). It holds
//! the leaf primitives every feature module shares: the `.handoff` control-plane location, the
//! ledger/task-dir path resolution, the wall-clock witness timestamp, status replay, and the
//! subprocess helper. Behavior-preserving move — `hf` re-exports these so existing `crate::…`
//! references are unchanged; future feature crates depend on `handoff-core` directly.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ledger::Ledger;
use work_order::{Status, WorkOrder};

/// The `.handoff` control-plane directory (repo-relative).
pub const HF: &str = ".handoff";

/// Wall-clock nanoseconds since the Unix epoch — the witness timestamp. Never panics (a clock
/// before the epoch yields 0).
pub fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// The task-card directory (`.handoff/tasks`).
pub fn tasks_dir() -> PathBuf {
    Path::new(HF).join("tasks")
}

/// HFTASK-0054: ledger location is overridable via the `HANDOFF_LEDGER` environment variable
/// (set by the `--ledger <path>` global flag). This lets a member repo render its Tier-A packet
/// against a shared ledger from its own CWD without a per-repo ledger.db. When unset, the default
/// is the local `<cwd>/.handoff/ledger.db`.
pub fn ledger_path() -> String {
    if let Ok(p) = std::env::var("HANDOFF_LEDGER")
        && !p.is_empty()
    {
        return p;
    }
    Path::new(HF)
        .join("ledger.db")
        .to_string_lossy()
        .into_owned()
}

/// Replay the latest witnessed status per task from the ledger. Fail-open WARN (never panic): a
/// replay failure on a present ledger logs a stale-status warning and falls back to card defaults.
pub fn current_statuses() -> Vec<(String, Status)> {
    match Ledger::open(&ledger_path()).and_then(|l| l.replay_latest_status()) {
        Ok(v) => v,
        Err(e) => {
            if Path::new(&ledger_path()).exists() {
                eprintln!(
                    "hf: WARNING — ledger present at {} but replay failed ({e}); statuses fall back to card defaults and may be stale (run `hf doctor`)",
                    ledger_path()
                );
            }
            Vec::new()
        }
    }
}

/// The replayed status for `id`, falling back to the card's own `status` when the ledger has no
/// transition for it.
pub fn status_of(id: &str, replay: &[(String, Status)], card: &WorkOrder) -> Status {
    replay
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, s)| *s)
        .unwrap_or(card.status)
}

/// Run a subprocess and capture trimmed stdout; `Err` on a non-zero exit (with stderr) or a spawn
/// failure. The shared shell-out used by the git/gh/cargo-driving feature modules.
pub fn run_out(bin: &str, args: &[&str]) -> Result<String, String> {
    match std::process::Command::new(bin).args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(format!(
            "{bin} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("{bin} not runnable: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_dir_under_hf() {
        assert!(tasks_dir().ends_with("tasks"));
        assert!(tasks_dir().starts_with(HF));
    }

    #[test]
    fn now_ns_is_monotonicish_and_nonzero() {
        assert!(now_ns() > 0);
    }

    fn minimal_card(id: &str, status: Status) -> WorkOrder {
        WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: "t".into(),
            status,
            priority: work_order::Priority::P1,
            objective: "o".into(),
            path_scope: vec![],
            acceptance_criteria: vec![],
            test_commands: vec![],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: String::new(),
            role: None,
            intent_lock: WorkOrder::compute_intent_lock("o", &[], &[]),
        }
    }

    #[test]
    fn status_of_falls_back_to_card() {
        let card = minimal_card("T1", Status::Backlog);
        // empty replay → card default
        assert_eq!(status_of("T1", &[], &card), Status::Backlog);
        // replay overrides
        let replay = vec![("T1".to_string(), Status::Done)];
        assert_eq!(status_of("T1", &replay, &card), Status::Done);
    }

    #[test]
    fn run_out_captures_stdout_and_errs_nonzero() {
        assert_eq!(run_out("true", &[]).ok(), Some(String::new()));
        assert!(run_out("false", &[]).is_err());
        assert!(run_out("definitely-not-a-binary-xyz", &[]).is_err());
    }
}
