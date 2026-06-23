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

/// Outcome of the end-time reap decision (ADR-0018 D10), pure + testable.
///
/// D10 (post-ADR-0018-D1 / HFTASK-0067 reconciliation): a session worktree is the
/// per-batch unit of isolation. Each worktree carries its own gitignored local
/// `ledger.db` rebuild cache + checkout — committed continuity truth is the
/// deterministic `.handoff/ledger.events.jsonl` export (D1), the binary is a
/// per-worktree cache — so parallel batches never share a working ledger and never
/// corrupt each other's witness chain/leases. The worktree is reaped ONLY on a
/// witnessed verified PR merge (`pr_merged`/`trunk_promoted`); abandoned/in-flight
/// batches KEEP their worktree until reconciled. NEVER reap on an unconfirmed merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapDecision {
    Reap,
    Keep(String),
}

/// Decide whether a session worktree may be reaped. Pure: the merge signal is replayed
/// from the witnessed ledger by the caller and passed in. Fail-closed — a `false`
/// `merge_verified` (no confirmed merge, or a ledger read that could not confirm one)
/// yields `Keep`, never `Reap`, so an abandoned/discarded batch retains its worktree.
/// `force` (the explicit `--reap`/reconcile override) is the ONLY way to reap without a
/// verified merge — a deliberate human/loop teardown of a genuinely-abandoned batch.
pub fn reap_decide(merge_verified: bool, force: bool) -> ReapDecision {
    if force {
        return ReapDecision::Reap;
    }
    if merge_verified {
        return ReapDecision::Reap;
    }
    ReapDecision::Keep(
        "no verified PR merge for this batch — worktree kept until reconciled (ADR-0018 D10)"
            .into(),
    )
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

/// `hf session <start|end|reap> [--recycle] [--reap] [--force] [--base BRANCH]`
pub fn cmd_session(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("start") => {
            let base = flag(args, "--base");
            session_start(base.as_deref(), &WeaveCli::from_env());
        }
        Some("end") => {
            let recycle = args.iter().any(|a| a == "--recycle");
            // ADR-0018 D10: `--recycle` does NOT imply force-reap — a recycled but unmerged
            // batch keeps its old worktree. `--reap` is the explicit force-teardown override.
            let force = args.iter().any(|a| a == "--reap");
            let base = flag(args, "--base");
            session_end(recycle, force, base.as_deref(), &WeaveCli::from_env());
        }
        Some("reap") => {
            let force = args.iter().any(|a| a == "--force" || a == "--reap");
            cmd_session_reap(force);
        }
        _ => eprintln!(
            "usage: hf session <start|end|reap> [--recycle] [--reap] [--force] [--base BRANCH]"
        ),
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
            // fail-open-audit R3: surface a lost witness loudly instead of a silent `if let Ok`.
            crate::witness_lifecycle("preflight_refuse", "session", &payload);
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
    // fail-open-audit R3: surface a lost witness loudly instead of a silent `if let Ok`.
    crate::witness_lifecycle("session_start", "session", &payload);
    println!("hf session start: {branch} off {base_ref}");
    println!("  worktree: {}", worktree.display());
    println!(
        "  next: cd into the worktree, then `hf claim --batch {}`; for each claimed task run `scripts/grit-shared.sh claim <file::symbol>` before editing and `scripts/grit-shared.sh done` before ship",
        policy.loop_cfg.cycle_flush
    );
}

fn session_end(recycle: bool, force: bool, base_override: Option<&str>, leaser: &dyn Leaser) {
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let policy = Policy::load(Path::new(HF));

    // Find the most recent un-ended session_start from the ledger to know what to tear down.
    let branch = latest_open_session_branch().unwrap_or_default();
    if branch.is_empty() {
        eprintln!("hf session end: no open session found in the ledger");
        return;
    }
    let resource = session_resource(&branch);

    // ADR-0018 D10: the session is logically CLOSED here (lease released, session_end
    // witnessed) regardless of the worktree decision. But the worktree is reaped ONLY on a
    // verified PR merge for this batch (or the explicit `--reap` force) — an abandoned/
    // in-flight batch KEEPS its worktree until reconciled (`hf session reap`). Fail-closed:
    // an unconfirmable merge ⇒ Keep, so unmerged work is never destroyed at session end.
    let merge_verified = batch_merge_verified(&replay_event_branches(), &branch);
    let decision = reap_decide(merge_verified, force);

    let _ = leaser.release(&resource);

    let reaped = matches!(decision, ReapDecision::Reap);
    match &decision {
        ReapDecision::Reap => {
            // The worktree's `.grit` is inside the worktree dir, torn down with it (ADR-0009).
            remove_worktree(&repo_root, &branch);
            let payload =
                serde_json::json!({ "branch": branch, "recycle": recycle, "reaped": true })
                    .to_string();
            // fail-open-audit R3: surface a lost witness loudly instead of a silent `if let Ok`.
            crate::witness_lifecycle("session_end", "session", &payload);
            println!("hf session end: closed {branch} (lease released, worktree reaped)");
        }
        ReapDecision::Keep(reason) => {
            let payload = serde_json::json!({
                "branch": branch, "recycle": recycle, "reaped": false, "keep_reason": reason,
            })
            .to_string();
            crate::witness_lifecycle("session_end", "session", &payload);
            println!(
                "hf session end: closed {branch} (lease released) — worktree RETAINED: {reason}"
            );
            if worktree_dir_exists(&repo_root, &branch) {
                println!("  retained worktree for {branch} — reconcile with `hf session reap` after merge, or `hf session end --reap` to force-remove");
            }
        }
    }
    let _ = reaped; // intent recorded in the witnessed payload above

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

/// Pure reducer (ADR-0018 D10): over ledger-ordered `(event_type, branch)` pairs (the
/// same mapping shape as `session_state_from_events`), is `branch`'s most-recent batch
/// verified-merged? Scan from the LAST `session_start` whose branch == `branch`; return
/// true iff a `pr_merged` OR `trunk_promoted` event appears in that window (after that
/// session_start). A merge witnessed BEFORE the session opened does not count — it
/// belongs to a prior batch. Fail-closed: no session_start for `branch`, or no merge
/// in the window, ⇒ false (KEEP).
pub(crate) fn batch_merge_verified(events: &[(String, Option<String>)], branch: &str) -> bool {
    // Find the index of the last session_start for this branch.
    let mut start_idx: Option<usize> = None;
    for (i, (event_type, b)) in events.iter().enumerate() {
        if event_type == "session_start" && b.as_deref() == Some(branch) {
            start_idx = Some(i);
        }
    }
    let Some(start) = start_idx else {
        return false;
    };
    events
        .iter()
        .skip(start + 1)
        .any(|(et, _)| et == "pr_merged" || et == "trunk_promoted")
}

/// Pure reducer (ADR-0018 D10): branches whose worktree was RETAINED at session end —
/// a `session_end` witnessed with `reaped:false` and NO later `worktree_reaped` for the
/// same branch. These are the abandoned/in-flight batches `hf session reap` reconciles.
/// Takes `(event_type, branch, reaped_flag)` triples (reaped_flag only meaningful for
/// `session_end`). A later `worktree_reaped` for a branch clears its retained status.
pub(crate) fn retained_worktrees(events: &[(String, Option<String>, Option<bool>)]) -> Vec<String> {
    let mut retained: Vec<String> = Vec::new();
    for (event_type, branch, reaped) in events {
        let Some(b) = branch else { continue };
        match event_type.as_str() {
            "session_end" => {
                // reaped:false ⇒ worktree kept; record it (dedup, keep latest intent).
                retained.retain(|r| r != b);
                if *reaped == Some(false) {
                    retained.push(b.clone());
                }
            }
            "worktree_reaped" => {
                retained.retain(|r| r != b);
            }
            _ => {}
        }
    }
    retained
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

/// Map a ledger event's payload `branch` field, when the event type carries one. Used to
/// build the `(event_type, branch)` pairs the pure reducers consume. `pr_merged`/
/// `trunk_promoted` carry no branch field (they are task-scoped) — they map to `None`,
/// which the reducers treat positionally (they only check the event TYPE for a merge).
fn branch_of(event_type: &str, payload_json: &str) -> Option<String> {
    if matches!(
        event_type,
        "session_start" | "session_end" | "worktree_reaped"
    ) {
        serde_json::from_str::<serde_json::Value>(payload_json)
            .ok()
            .and_then(|v| v.get("branch").and_then(|b| b.as_str()).map(String::from))
    } else {
        None
    }
}

/// IO wrapper: replay the ledger into `(event_type, branch)` pairs (the shape the pure
/// reducers consume). On any ledger read failure this returns an EMPTY vec — which makes
/// `batch_merge_verified` return false (KEEP), the fail-closed default for the reap go/
/// no-go: an unconfirmable merge must never reap.
fn replay_event_branches() -> Vec<(String, Option<String>)> {
    Ledger::open(&ledger_path())
        .ok()
        .and_then(|l| l.all_events().ok())
        .unwrap_or_default()
        .iter()
        .map(|e| {
            (
                e.event_type.clone(),
                branch_of(&e.event_type, &e.payload_json),
            )
        })
        .collect()
}

/// IO wrapper: replay the ledger into `(event_type, branch, reaped_flag)` triples for
/// `retained_worktrees`. The `reaped` flag is read from a `session_end` payload's
/// `reaped` field. Empty on read failure (no retained worktrees surfaced — fail-closed).
fn replay_event_triples() -> Vec<(String, Option<String>, Option<bool>)> {
    Ledger::open(&ledger_path())
        .ok()
        .and_then(|l| l.all_events().ok())
        .unwrap_or_default()
        .iter()
        .map(|e| {
            let branch = branch_of(&e.event_type, &e.payload_json);
            let reaped = if e.event_type == "session_end" {
                serde_json::from_str::<serde_json::Value>(&e.payload_json)
                    .ok()
                    .and_then(|v| v.get("reaped").and_then(|r| r.as_bool()))
            } else {
                None
            };
            (e.event_type.clone(), branch, reaped)
        })
        .collect()
}

/// Remove a session worktree by branch (best-effort, both engines). Mirrors the teardown
/// in `session_end` so the reap paths (`hf session reap`, the `cmd_done` post-merge reap)
/// share one implementation.
fn remove_worktree(repo_root: &Path, branch: &str) {
    if meta_available() {
        if let Some(root) = meta_root(repo_root) {
            let _ = run_out_in(&root, "meta", &["git", "worktree", "remove", branch]);
        }
    }
}

/// Does a retained session worktree dir for `branch` still exist on disk? Used by
/// `hf session reap` to only act on worktrees that were not already removed out-of-band.
fn worktree_dir_exists(repo_root: &Path, branch: &str) -> bool {
    if let Some(root) = meta_root(repo_root) {
        if root
            .join(".worktrees")
            .join(branch)
            .join("handoff")
            .is_dir()
        {
            return true;
        }
    }
    repo_root
        .parent()
        .map(|p| p.join(format!(".handoff-wt-{branch}")).is_dir())
        .unwrap_or(false)
}

/// `hf session reap [--force]` (ADR-0018 D10): sweep RETAINED worktrees (closed with
/// `reaped:false`) whose dir still exists and whose batch is NOW verified-merged (or
/// `--force`), reaping each + witnessing `worktree_reaped`. Fail-closed: an unmerged,
/// un-forced retained worktree is reported Kept-with-reason, never removed.
pub fn cmd_session_reap(force: bool) {
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let retained = retained_worktrees(&replay_event_triples());
    if retained.is_empty() {
        println!("hf session reap: no retained worktrees to reap");
        return;
    }
    let pairs = replay_event_branches();
    let mut reaped_any = false;
    for branch in retained {
        if !worktree_dir_exists(&repo_root, &branch) {
            // Dir already gone (removed out-of-band) — record the reap so it leaves the
            // retained set, keeping the read-model honest.
            let payload =
                serde_json::json!({ "branch": branch, "note": "worktree dir absent" }).to_string();
            crate::witness_lifecycle("worktree_reaped", "session", &payload);
            println!("hf session reap: {branch} — worktree dir already gone, marked reaped");
            reaped_any = true;
            continue;
        }
        let merge_verified = batch_merge_verified(&pairs, &branch);
        match reap_decide(merge_verified, force) {
            ReapDecision::Reap => {
                remove_worktree(&repo_root, &branch);
                let payload = serde_json::json!({
                    "branch": branch, "merge_verified": merge_verified, "forced": force,
                })
                .to_string();
                crate::witness_lifecycle("worktree_reaped", "session", &payload);
                println!("hf session reap: reaped {branch} (merge_verified={merge_verified}, forced={force})");
                reaped_any = true;
            }
            ReapDecision::Keep(reason) => {
                println!("hf session reap: KEPT {branch} — {reason}");
            }
        }
    }
    if !reaped_any {
        println!("hf session reap: nothing reaped (no verified merge; pass --force to override)");
    }
}

/// Post-merge reap hook for `hf done --pr` (ADR-0018 D10): the "removed ON verified PR
/// merge" path. Find the open/most-recent session branch; if its batch is now
/// `batch_merge_verified`, remove that worktree + witness `worktree_reaped`. NON-FATAL
/// and fail-closed: if no session/merge can be confirmed, do nothing (KEEP the worktree).
pub fn reap_open_session_if_merged() {
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let pairs = replay_event_branches();
    // The open session's branch, if a session is open; else the most-recent session_start.
    let branch = open_session_and_cycle().open_branch.or_else(|| {
        pairs
            .iter()
            .rev()
            .find(|(et, b)| et == "session_start" && b.is_some())
            .and_then(|(_, b)| b.clone())
    });
    let Some(branch) = branch else {
        return; // no session to reap
    };
    if !batch_merge_verified(&pairs, &branch) {
        return; // fail-closed: no confirmed merge for this batch
    }
    remove_worktree(&repo_root, &branch);
    let payload =
        serde_json::json!({ "branch": branch, "merge_verified": true, "trigger": "done" })
            .to_string();
    crate::witness_lifecycle("worktree_reaped", "session", &payload);
    println!("hf done: reaped session worktree {branch} on verified PR merge (ADR-0018 D10)");
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

    // --- ADR-0018 D10: reap decision + batch-merge reducers ---

    #[test]
    fn reap_decide_reaps_on_verified_merge() {
        assert_eq!(reap_decide(true, false), ReapDecision::Reap);
    }

    #[test]
    fn reap_decide_keeps_when_unmerged() {
        // THE load-bearing fail-closed invariant: no verified merge ⇒ never reap.
        match reap_decide(false, false) {
            ReapDecision::Keep(reason) => assert!(reason.contains("no verified PR merge")),
            ReapDecision::Reap => panic!("must NOT reap an unmerged (abandoned/in-flight) batch"),
        }
    }

    #[test]
    fn reap_decide_force_overrides_unmerged() {
        // The explicit `--reap`/reconcile override is the ONLY way to reap without a merge.
        assert_eq!(reap_decide(false, true), ReapDecision::Reap);
    }

    #[test]
    fn batch_merge_verified_true_when_merge_in_window() {
        let events = [
            ev("session_start", Some("b1")),
            ev("checkpoint", None),
            ev("pr_merged", None),
        ];
        assert!(batch_merge_verified(&events, "b1"));
    }

    #[test]
    fn batch_merge_verified_true_on_trunk_promoted() {
        let events = [ev("session_start", Some("b1")), ev("trunk_promoted", None)];
        assert!(batch_merge_verified(&events, "b1"));
    }

    #[test]
    fn batch_merge_verified_false_when_merge_before_session() {
        // A merge that landed BEFORE this batch's session_start belongs to a prior batch.
        let events = [
            ev("pr_merged", None),
            ev("session_start", Some("b1")),
            ev("checkpoint", None),
        ];
        assert!(!batch_merge_verified(&events, "b1"));
    }

    #[test]
    fn batch_merge_verified_false_when_no_merge() {
        let events = [ev("session_start", Some("b1")), ev("checkpoint", None)];
        assert!(!batch_merge_verified(&events, "b1"));
    }

    #[test]
    fn batch_merge_verified_false_for_unknown_branch() {
        // No session_start for this branch ⇒ fail-closed false.
        let events = [ev("session_start", Some("other")), ev("pr_merged", None)];
        assert!(!batch_merge_verified(&events, "b1"));
    }

    fn ev3(t: &str, b: Option<&str>, r: Option<bool>) -> (String, Option<String>, Option<bool>) {
        (t.to_string(), b.map(String::from), r)
    }

    #[test]
    fn retained_worktrees_lists_kept_then_clears_on_reap() {
        let events = [
            ev3("session_start", Some("a"), None),
            ev3("session_end", Some("a"), Some(false)), // kept
            ev3("session_start", Some("b"), None),
            ev3("session_end", Some("b"), Some(true)), // reaped at end → not retained
            ev3("session_start", Some("c"), None),
            ev3("session_end", Some("c"), Some(false)), // kept
            ev3("worktree_reaped", Some("c"), None),    // later reaped → cleared
        ];
        let retained = retained_worktrees(&events);
        assert_eq!(retained, vec!["a".to_string()]);
    }
}
