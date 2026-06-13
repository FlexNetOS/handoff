//! `hf session start|end [--recycle]` — worktree-isolated loop sessions (HFTASK-0007, ADR-0001 §2).
//!
//! A session is the loop's unit of isolation: a fresh worktree branched off
//! `origin/<base_branch>`, a weave path-scope lease, and witnessed `session_start` /
//! `session_end` events. It refuses to start on a drifted tree (the prior weave-loop
//! failure lesson) and reuses the meta worktree engine via the `meta git worktree` CLI
//! (which calls `meta_git_lib` under the hood) — not a crate dependency, so `handoff`
//! stays an independently-cloneable repo. Falls back to plain `git worktree` when meta
//! is unavailable (standalone clone / CI).

use std::path::{Path, PathBuf};
use std::process::Command;

use ledger::Ledger;

use crate::lease::{Leaser, WeaveCli};
use crate::policy::Policy;
use crate::{ledger_path, now_ns, HF};

/// Sessions run longer than a single claim; the lease TTL is heartbeat-extended.
const SESSION_TTL_SECS: u64 = 28_800; // 8h

/// Lease key for a session's worktree path scope. Slash-free so weave's path-hierarchy
/// conflict detection reduces to exact-match (one holder per session branch).
pub fn session_resource(branch: &str) -> String {
    format!("handoff:session:{branch}")
}

/// Deterministic session branch name from the loop prefix + a wall-clock second.
/// Pure so it is unit-testable without a clock.
pub fn session_branch(prefix: &str, epoch_secs: u64) -> String {
    format!("{prefix}{epoch_secs}")
}

/// Outcome of the start-time drift preflight (pure, testable in isolation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightDecision {
    Pass,
    Refuse(String),
}

/// Decide whether a session may start, given git facts. Kept pure: the IO (git status,
/// git fetch) is done by the caller and passed in, so the policy is fully testable.
pub fn preflight_decide(
    require_clean: bool,
    porcelain: &str,
    require_synced: bool,
    base_in_sync: bool,
) -> PreflightDecision {
    if require_clean {
        let dirty = porcelain.lines().filter(|l| !l.trim().is_empty()).count();
        if dirty > 0 {
            return PreflightDecision::Refuse(format!(
                "working tree not clean ({dirty} uncommitted change(s)) — commit, stash, or `hf ship` first"
            ));
        }
    }
    if require_synced && !base_in_sync {
        return PreflightDecision::Refuse(
            "base branch behind/diverged from origin (or origin unreachable) — fetch + fast-forward before starting".into(),
        );
    }
    PreflightDecision::Pass
}

/// Run a subprocess in a specific directory with explicit argv (no shell), capturing
/// trimmed stdout. Mirrors `crate::run_out` but lets us drive `meta` from the meta root.
fn run_out_in(dir: &Path, bin: &str, args: &[&str]) -> Result<String, String> {
    match Command::new(bin).args(args).current_dir(dir).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(format!(
            "{bin} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("{bin} not runnable: {e}")),
    }
}

/// The meta workspace root that owns this repo, if any: the parent dir that holds a
/// `.meta.yaml`. `None` for a standalone clone (then we use plain `git worktree`).
fn meta_root(repo_root: &Path) -> Option<PathBuf> {
    let parent = repo_root.parent()?;
    if parent.join(".meta.yaml").exists() || parent.join(".meta").exists() {
        Some(parent.to_path_buf())
    } else {
        None
    }
}

fn meta_available() -> bool {
    Command::new("meta")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// grit on PATH?
fn grit_available() -> bool {
    Command::new("grit")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Make a freshly-created session worktree grit-coordinated (ADR-0009): `grit init`
/// indexes the symbols so the session's parallel agents `grit claim` AST symbols
/// (each gets `.grit/worktrees/agent-N`) + `grit done` for conflict-free merge.
/// Best-effort — never wall session creation on it. NOTE: we deliberately do NOT use
/// `grit session start` here — it is broken in grit 0.3.0 (`git checkout -b grit/<n> --`
/// with an empty base, fails in any repo). The working primitives are init/claim/done.
fn grit_enable(worktree: &Path) {
    if grit_available() && !worktree.join(".grit").is_dir() {
        let _ = run_out_in(worktree, "grit", &["init"]);
    }
}

/// Create the session worktree (ADR-0009). The worktree DIR gives real isolation +
/// concurrency (`meta git worktree`, separate dir; or plain git when standalone); then
/// `grit init` makes it grit-coordinated so the session's parallel agents lock AST
/// symbols rather than colliding at the file level. Engines: `meta git worktree`
/// (separate dir) when in a meta workspace, else plain `git worktree` standalone —
/// both are then grit-enabled.
fn create_worktree(repo_root: &Path, branch: &str, from_ref: &str) -> Result<PathBuf, String> {
    if meta_available() {
        if let Some(root) = meta_root(repo_root) {
            run_out_in(
                &root,
                "meta",
                &[
                    "git",
                    "worktree",
                    "create",
                    "--repo",
                    "handoff",
                    "--branch",
                    branch,
                    "--from-ref",
                    from_ref,
                ],
            )?;
            let wt = root.join(".worktrees").join(branch).join("handoff");
            grit_enable(&wt);
            return Ok(wt);
        }
    }
    // Standalone fallback: sibling worktree dir next to the repo.
    let dest = repo_root
        .parent()
        .unwrap_or(repo_root)
        .join(format!(".handoff-wt-{branch}"));
    crate::run_out(
        "git",
        &[
            "worktree",
            "add",
            "-b",
            branch,
            &dest.to_string_lossy(),
            from_ref,
        ],
    )?;
    grit_enable(&dest);
    Ok(dest)
}

/// `hf session <start|end> [--recycle] [--base BRANCH]`
pub fn cmd_session(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("start") => {
            let base = flag(args, "--base");
            session_start(base.as_deref(), &WeaveCli::from_env());
        }
        Some("end") => {
            let recycle = args.iter().any(|a| a == "--recycle");
            let base = flag(args, "--base");
            session_end(recycle, base.as_deref(), &WeaveCli::from_env());
        }
        _ => eprintln!("usage: hf session <start|end> [--recycle] [--base BRANCH]"),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn session_start(base_override: Option<&str>, leaser: &dyn Leaser) {
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let policy = Policy::load(Path::new(HF));
    let base = base_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| policy.remote.base_branch.clone());

    // --- drift preflight (IO → pure decision) ---
    let porcelain = crate::run_out("git", &["status", "--porcelain"]).unwrap_or_default();
    let fetched = crate::run_out("git", &["fetch", "origin", &base]).is_ok();
    let base_ref = format!("origin/{base}");
    let base_resolves =
        crate::run_out("git", &["rev-parse", "--verify", "--quiet", &base_ref]).is_ok();
    let base_in_sync = fetched && base_resolves;

    match preflight_decide(
        policy.preflight.require_clean_tree,
        &porcelain,
        policy.preflight.require_synced_base,
        base_in_sync,
    ) {
        PreflightDecision::Refuse(reason) => {
            let payload = serde_json::json!({ "phase": "preflight", "reason": reason }).to_string();
            if let Ok(mut led) = Ledger::open(&ledger_path()) {
                let _ = led.append("preflight_refuse", "session", &payload, now_ns());
            }
            eprintln!("hf session start: REFUSED — {reason}");
            return;
        }
        PreflightDecision::Pass => {}
    }

    // --- worktree + lease ---
    let epoch_secs = now_ns() / 1_000_000_000;
    let branch = session_branch(&policy.loop_cfg.worktree_prefix, epoch_secs);
    let resource = session_resource(&branch);
    match crate::lease::gate(leaser.reserve(&resource, SESSION_TTL_SECS, "hf session")) {
        crate::lease::ClaimGate::Refuse(reason) => {
            eprintln!("hf session start: BLOCKED — {resource} held by another peer ({reason})");
            return;
        }
        crate::lease::ClaimGate::ProceedDegraded => {
            eprintln!("hf session start: weave lease unavailable — proceeding ledger-only")
        }
        crate::lease::ClaimGate::Proceed => {
            println!("hf session start: reserved {resource}")
        }
    }

    let worktree = match create_worktree(&repo_root, &branch, &base_ref) {
        Ok(p) => p,
        Err(e) => {
            // fail-closed: release the lease we just took, record nothing as started
            let _ = leaser.release(&resource);
            eprintln!("hf session start: worktree creation failed — {e}");
            return;
        }
    };

    let payload = serde_json::json!({
        "branch": branch, "base": base, "worktree": worktree.to_string_lossy(),
    })
    .to_string();
    if let Ok(mut led) = Ledger::open(&ledger_path()) {
        let _ = led.append("session_start", "session", &payload, now_ns());
    }
    println!("hf session start: {branch} off {base_ref}");
    println!("  worktree: {}", worktree.display());
    println!(
        "  next: cd into the worktree, then `hf claim --batch {}`",
        policy.loop_cfg.cycle_flush
    );
}

fn session_end(recycle: bool, base_override: Option<&str>, leaser: &dyn Leaser) {
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let policy = Policy::load(Path::new(HF));

    // Find the most recent un-ended session_start from the ledger to know what to tear down.
    let branch = latest_open_session_branch().unwrap_or_default();
    if branch.is_empty() {
        eprintln!("hf session end: no open session found in the ledger");
        return;
    }
    let resource = session_resource(&branch);

    // Remove the worktree (best-effort; never wall on cleanup). The worktree's `.grit`
    // is inside the worktree dir, so it is torn down with it (ADR-0009).
    if meta_available() {
        if let Some(root) = meta_root(&repo_root) {
            let _ = run_out_in(&root, "meta", &["git", "worktree", "remove", &branch]);
        }
    }
    let _ = leaser.release(&resource);

    let payload = serde_json::json!({ "branch": branch, "recycle": recycle }).to_string();
    if let Ok(mut led) = Ledger::open(&ledger_path()) {
        let _ = led.append("session_end", "session", &payload, now_ns());
    }
    println!("hf session end: closed {branch} (lease released, worktree removed)");

    if recycle {
        println!("hf session end: --recycle → starting a fresh session");
        session_start(base_override.or(Some(&policy.remote.base_branch)), leaser);
    }
}

/// Loop session read-model: which session (if any) is open, and how many checkpoints
/// have landed in it (the cycle counter that drives `hf ship` at `cycle_flush`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct LoopSessionState {
    pub open_branch: Option<String>,
    pub cycle: u32,
}

/// Pure reducer over `(event_type, branch_for_session_events)` pairs — testable without
/// a database. A `session_start` opens a session and zeroes the cycle; each `checkpoint`
/// while open increments it; the matching `session_end` closes it.
pub(crate) fn session_state_from_events(events: &[(String, Option<String>)]) -> LoopSessionState {
    let mut open: Option<String> = None;
    let mut cycle = 0u32;
    for (event_type, branch) in events {
        match event_type.as_str() {
            "session_start" => {
                open = branch.clone();
                cycle = 0;
            }
            "checkpoint" if open.is_some() => cycle += 1,
            "session_end" if open.is_some() && open == *branch => {
                open = None;
                cycle = 0;
            }
            _ => {}
        }
    }
    LoopSessionState {
        open_branch: open,
        cycle,
    }
}

/// IO wrapper: replay the ledger into a `LoopSessionState`.
pub(crate) fn open_session_and_cycle() -> LoopSessionState {
    let events = Ledger::open(&ledger_path())
        .ok()
        .and_then(|l| l.all_events().ok())
        .unwrap_or_default();
    let mapped: Vec<(String, Option<String>)> = events
        .iter()
        .map(|e| {
            let branch = if e.event_type == "session_start" || e.event_type == "session_end" {
                serde_json::from_str::<serde_json::Value>(&e.payload_json)
                    .ok()
                    .and_then(|v| v.get("branch").and_then(|b| b.as_str()).map(String::from))
            } else {
                None
            };
            (e.event_type.clone(), branch)
        })
        .collect();
    session_state_from_events(&mapped)
}

/// The branch of the currently-open session, if any (by ledger replay).
fn latest_open_session_branch() -> Option<String> {
    open_session_and_cycle().open_branch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_refuses_dirty_tree() {
        let d = preflight_decide(true, " M src/main.rs\n?? new.rs\n", true, true);
        assert!(matches!(d, PreflightDecision::Refuse(_)));
    }

    #[test]
    fn preflight_passes_clean_synced() {
        assert_eq!(
            preflight_decide(true, "   \n", true, true),
            PreflightDecision::Pass
        );
    }

    #[test]
    fn preflight_refuses_unsynced_base() {
        let d = preflight_decide(true, "", true, false);
        assert!(matches!(d, PreflightDecision::Refuse(_)));
    }

    #[test]
    fn preflight_can_disable_checks() {
        // dirty tree + unsynced base but both checks disabled → pass
        assert_eq!(
            preflight_decide(false, " M x", false, false),
            PreflightDecision::Pass
        );
    }

    #[test]
    fn branch_and_resource_are_deterministic() {
        assert_eq!(
            session_branch("handoff-", 1_700_000_000),
            "handoff-1700000000"
        );
        assert_eq!(
            session_resource("handoff-1700000000"),
            "handoff:session:handoff-1700000000"
        );
    }

    fn ev(t: &str, b: Option<&str>) -> (String, Option<String>) {
        (t.to_string(), b.map(String::from))
    }

    #[test]
    fn cycle_counter_tracks_checkpoints_within_open_session() {
        let events = [
            ev("session_start", Some("handoff-1")),
            ev("checkpoint", None),
            ev("checkpoint", None),
            ev("task_transition", None), // not a checkpoint → ignored
        ];
        let st = session_state_from_events(&events);
        assert_eq!(st.open_branch.as_deref(), Some("handoff-1"));
        assert_eq!(st.cycle, 2);
    }

    #[test]
    fn session_end_closes_and_resets_cycle() {
        let events = [
            ev("session_start", Some("handoff-1")),
            ev("checkpoint", None),
            ev("session_end", Some("handoff-1")),
            ev("checkpoint", None), // after close → no open session, not counted
        ];
        let st = session_state_from_events(&events);
        assert_eq!(st.open_branch, None);
        assert_eq!(st.cycle, 0);
    }

    #[test]
    fn recycled_session_starts_fresh_cycle() {
        let events = [
            ev("session_start", Some("a")),
            ev("checkpoint", None),
            ev("session_end", Some("a")),
            ev("session_start", Some("b")),
            ev("checkpoint", None),
        ];
        let st = session_state_from_events(&events);
        assert_eq!(st.open_branch.as_deref(), Some("b"));
        assert_eq!(st.cycle, 1);
    }
}
