// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! HFTASK-0083 (ADR-0019 D5 #4): the drift-audit + policy-check engine peeled into `handoff-drift`
//! after the North-Star + card/next-safe helpers moved to handoff-core. `hf` aliases it as `gates`
//! so `gates::cmd_drift` / `gates::cmd_policy_check` stay valid. Deps: handoff-core + ledger +
//! work-order.
//!
//! `hf drift` (HFTASK-0005) and `hf policy check-{claim,edit,handoff}` (HFTASK-0015)
//! — the two hard gates the `.handoff/hooks/hooks.toml` contract fires (PreEdit,
//! PreHandoff, TaskClaim). Both emit JSON for hook callers and exit non-zero on a
//! block so `fail_mode = block` hooks actually stop the loop. Fail-closed.

use handoff_core::{current_statuses, load_tasks, status_of};
use ledger::Ledger;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use work_order::{Status, WorkOrder};

const HF: &str = ".handoff";

// --- shared helpers ---------------------------------------------------------

fn run_git(args: &[&str]) -> String {
    Command::new("git")
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Working-tree changed files (staged + unstaged + untracked), repo-relative.
fn changed_files() -> Vec<String> {
    run_git(&["status", "--porcelain"])
        .lines()
        .filter_map(|l| l.get(3..).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Minimal glob match for the path_scope / protected-file forms we use:
/// `**` (any), `prefix/**` (under prefix), `*.ext` / `**/*.ext` (suffix), exact, and
/// `dir/**`-style prefixes. Good enough for the controlled card/rules patterns.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == "**" || pattern == "." || pattern == "./**" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    if let Some(suffix) = pattern.strip_prefix("**/") {
        // **/*.ext  or  **/name
        if let Some(ext) = suffix.strip_prefix("*.") {
            return path.ends_with(&format!(".{ext}"));
        }
        return path.ends_with(suffix);
    }
    if let Some(ext) = pattern.strip_prefix("*.") {
        return path.ends_with(&format!(".{ext}"));
    }
    if let Some(prefix) = pattern.strip_suffix("/*") {
        // one level under prefix
        return path.starts_with(&format!("{prefix}/")) && !path[prefix.len() + 1..].contains('/');
    }
    pattern == path
}

/// The repo's directory name (cwd), used to reconcile meta-root-relative card scopes
/// (e.g. `handoff/**`) with repo-relative git paths (e.g. `hf/src/main.rs`).
fn repo_dir_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// True if `path` is in any scope. Tolerates both bases: a repo-relative git path and
/// the same path prefixed with the repo name (how cards from the meta root express it).
fn in_any_scope(path: &str, scopes: &[String]) -> bool {
    let prefixed = format!("{}/{}", repo_dir_name(), path);
    scopes
        .iter()
        .any(|p| glob_match(p, path) || glob_match(p, &prefixed))
}

/// Tasks currently held (Claimed/Active/Checkpointed/Review) → their union path_scope.
fn claimed_scopes(tasks: &[WorkOrder], replay: &[(String, Status)]) -> (Vec<String>, Vec<String>) {
    let mut ids = vec![];
    let mut scopes = vec![];
    for t in tasks {
        if matches!(
            status_of(&t.id, replay, t),
            Status::Claimed | Status::Active | Status::Checkpointed | Status::Review
        ) {
            ids.push(t.id.clone());
            scopes.extend(t.path_scope.iter().cloned());
        }
    }
    (ids, scopes)
}

// --- hf drift (HFTASK-0005, expanded to the §12.3 sentinel by HFTASK-0046) -------

/// A decision/architecture surface: changing it should be accompanied by an ADR/decision
/// record. Pure (HFTASK-0046, the §12.3 "undocumented architecture change" sentinel).
fn is_decision_surface(path: &str) -> bool {
    const PATHS: [&str; 4] = [
        ".handoff/policy",
        ".handoff/policies/",
        ".handoff/hooks/",
        ".github/",
    ];
    PATHS.iter().any(|p| path.contains(p))
}

/// PRD §12.3 #8 — a machine-checkable constraint declared INSIDE a decision record (an ADR, the
/// rusty-idd security-advisory ledger, or `.handoff/decisions/*`). A decision states "pattern X is
/// forbidden / required in files matching <glob>"; the sentinel then proves new work hasn't
/// contradicted it. This turns #8 from "approximated by #9's decision-surface advisory" into a
/// REAL content check whose source of truth IS the recorded decisions.
///
/// Marker form (an HTML comment, invisible in rendered markdown):
/// ```text
/// <!-- drift-guard id="no-foo" forbid="foo =,bar =" path="**/*.toml" reason="ADR-xxxx: ..." -->
/// <!-- drift-guard id="keeps-baz" require="baz" path="src/lib.rs" reason="..." -->
/// ```
#[derive(Debug, Clone, PartialEq)]
struct DecisionGuard {
    id: String,
    source: String,       // the decision-record file the guard came from
    forbid: Vec<String>,  // literal substrings that MUST NOT appear in any governed file
    require: Vec<String>, // literal substrings that MUST appear in at least one governed file
    path_glob: String,    // `glob_match` pattern selecting the governed files
    reason: String,
}

/// Pure: parse every `drift-guard` marker out of one decision record's text. A marker missing a
/// `path` or having neither `forbid` nor `require` is a no-op and skipped (never a false finding).
fn parse_decision_guards(text: &str, source: &str) -> Vec<DecisionGuard> {
    let mut out = vec![];
    for raw in text.split("<!-- drift-guard").skip(1) {
        let Some(end) = raw.find("-->") else { continue };
        let body = &raw[..end];
        let attr = |key: &str| -> Option<String> {
            let pat = format!("{key}=\"");
            let i = body.find(&pat)? + pat.len();
            let j = body[i..].find('"')? + i;
            Some(body[i..j].to_string())
        };
        let csv = |s: Option<String>| -> Vec<String> {
            s.map(|v| {
                v.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default()
        };
        let path_glob = attr("path").unwrap_or_default();
        let forbid = csv(attr("forbid"));
        let require = csv(attr("require"));
        if path_glob.is_empty() || (forbid.is_empty() && require.is_empty()) {
            continue;
        }
        out.push(DecisionGuard {
            id: attr("id").unwrap_or_else(|| "unnamed".into()),
            source: source.to_string(),
            forbid,
            require,
            path_glob,
            reason: attr("reason").unwrap_or_default(),
        });
    }
    out
}

/// Pure: check each guard against `files` (path, content) and return contradiction findings.
/// `forbid` → any governed file containing the substring is a contradiction; `require` → at least
/// one governed file must contain it (only enforced when the guard actually governs ≥1 file).
fn check_guards(guards: &[DecisionGuard], files: &[(String, String)]) -> Vec<String> {
    let mut out = vec![];
    for g in guards {
        let governed: Vec<&(String, String)> = files
            .iter()
            .filter(|(p, _)| glob_match(&g.path_glob, p))
            .collect();
        for (path, content) in &governed {
            for pat in &g.forbid {
                if content.contains(pat.as_str()) {
                    out.push(format!(
                        "decision contradiction [{}]: {} contains forbidden `{}` ({}: {})",
                        g.id, path, pat, g.source, g.reason
                    ));
                }
            }
        }
        for pat in &g.require {
            if !governed.is_empty() && !governed.iter().any(|(_, c)| c.contains(pat.as_str())) {
                out.push(format!(
                    "decision contradiction [{}]: no file matching `{}` contains required `{}` ({}: {})",
                    g.id, g.path_glob, pat, g.source, g.reason
                ));
            }
        }
    }
    out
}

/// Load all decision-record guards from the tracked tree (ADRs, the security-advisory ledger, and
/// `.handoff/decisions/*`). Best-effort: an unreadable record contributes no guards.
fn load_decision_guards() -> Vec<DecisionGuard> {
    let mut guards = vec![];
    for f in run_git(&["ls-files", "docs", ".handoff/decisions"]).lines() {
        let f = f.trim();
        let is_record = f.ends_with(".md")
            && (f.contains("adr-")
                || f.contains("/decisions/")
                || f.ends_with("security-advisories.md"));
        if !is_record {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(f) {
            guards.extend(parse_decision_guards(&text, f));
        }
    }
    guards
}

/// Check every decision guard against the tracked tree (excluding the vendored third-party tree —
/// guards govern OUR work, not upstream forks). Returns advisory contradiction findings.
fn scan_decision_contradictions(guards: &[DecisionGuard]) -> Vec<String> {
    if guards.is_empty() {
        return vec![];
    }
    let files: Vec<(String, String)> = run_git(&["ls-files"])
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|p| !p.is_empty() && !p.starts_with("vendor/"))
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|c| (p, c)))
        .collect();
    check_guards(guards, &files)
}

/// Tasks (by id) whose LATEST witnessed `test_result` is green (HFTASK-0045/0046).
fn tasks_with_green_tests() -> HashSet<String> {
    let mut green = HashSet::new();
    let path = Path::new(HF).join("ledger.db");
    if let Ok(led) = Ledger::open(&path.to_string_lossy())
        && let Ok(events) = led.all_events()
    {
        for e in events {
            if e.event_type != "test_result" {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(&e.payload_json)
                .ok()
                .and_then(|v| v["passed"].as_bool())
            {
                Some(true) => {
                    green.insert(e.work_order_id);
                }
                Some(false) => {
                    green.remove(&e.work_order_id); // latest-wins: a later failure un-greens
                }
                None => {}
            }
        }
    }
    green
}

/// Tasks that have at least one witnessed `checkpoint` event (PRD §12.3 #10 input). Mirrors
/// `tasks_with_green_tests`'s ledger-read shape; advisory-only, so a ledger-open failure
/// degrades to "no checkpoints seen" rather than changing any blocking verdict.
fn tasks_with_checkpoints() -> HashSet<String> {
    let mut seen = HashSet::new();
    let path = Path::new(HF).join("ledger.db");
    if let Ok(led) = Ledger::open(&path.to_string_lossy())
        && let Ok(events) = led.all_events()
    {
        for e in events {
            if e.event_type == "checkpoint" {
                seen.insert(e.work_order_id);
            }
        }
    }
    seen
}

/// The full drift sentinel result (HFTASK-0046, schema `handoff.drift_report.v1`).
/// `drift` is the BLOCKING human-readable union; `undocumented_decisions` is advisory
/// (surfaced for the gatekeeper, not hard-blocking, so the loop's own ADR'd policy work
/// isn't deadlocked). `clean()` ⇔ no blocking items.
struct DriftReport {
    objective_hash_match: bool,
    path_scope_match: bool,
    acceptance_hash_match: bool,
    constraint_hash_match: bool,
    northstar_revision_match: bool,
    out_of_scope_files: Vec<String>,
    missing_evidence: Vec<String>,
    acceptance_without_tests: Vec<String>,
    dependency_unsatisfied: Vec<String>,
    undocumented_decisions: Vec<String>,
    /// PRD §12.3 #8 (work contradicts a decision record) — ADVISORY: a `drift-guard` declared
    /// inside a decision record (ADR / security-advisory / `.handoff/decisions`) is violated by
    /// the tracked tree (a forbidden pattern reappeared, or a required one is missing). A real
    /// content check (no longer "approximated by #9"); surfaced for the gatekeeper, not blocking.
    decision_contradictions: Vec<String>,
    /// PRD §12.3 #1 (task-active) — informational: the tasks currently in an active state. An
    /// empty list while changes are staged is already BLOCKED by the §12.3-#6 deny_without_claim
    /// path; this surfaces the active set explicitly so the check is observable, not implicit.
    active_tasks: Vec<String>,
    /// PRD §12.3 #10 (handoff-state-updated) — ADVISORY (never blocking): an active task with
    /// material (changed) files but no witnessed checkpoint yet — handoff state not refreshed
    /// after material changes. Surfaced as a reminder, not a gate (like `undocumented_decisions`).
    handoff_state_stale: Vec<String>,
    drift: Vec<String>,
    required_actions: Vec<String>,
    /// Tasks whose 5-surface intent_lock drifted this run → the `task_intent_changed` witnesses
    /// `cmd_drift` will append (deduped). (id, changed surface names, observed-lock signature).
    intent_changed: Vec<(String, Vec<&'static str>, String)>,
}

impl DriftReport {
    fn clean(&self) -> bool {
        self.drift.is_empty()
    }
}

fn detect() -> DriftReport {
    let tasks = load_tasks();
    let replay = current_statuses();
    let in_progress = |t: &WorkOrder| {
        matches!(
            status_of(&t.id, &replay, t),
            Status::Claimed | Status::Active | Status::Checkpointed | Status::Review
        )
    };
    let mut r = DriftReport {
        objective_hash_match: true,
        path_scope_match: true,
        acceptance_hash_match: true,
        constraint_hash_match: true,
        northstar_revision_match: true,
        out_of_scope_files: vec![],
        missing_evidence: vec![],
        acceptance_without_tests: vec![],
        dependency_unsatisfied: vec![],
        undocumented_decisions: vec![],
        decision_contradictions: vec![],
        active_tasks: vec![],
        handoff_state_stale: vec![],
        drift: vec![],
        required_actions: vec![],
        intent_changed: vec![],
    };

    // PRD §12.3 #1 (task-active): surface the currently-active task set explicitly (informational).
    r.active_tasks = tasks
        .iter()
        .filter(|t| in_progress(t))
        .map(|t| t.id.clone())
        .collect();

    // PRD §12.3 #2 (objective), #3 (path_scope), #4 (acceptance), #5 (constraints): per-surface
    // intent_lock drift across all FIVE surfaces (HFTASK-0047). `northstar` is a kernel ADDITION
    // beyond PRD §12.3 (doctrine-revision drift). The constraint/northstar checks no-op on a
    // legacy partial lock (empty fields), so old cards are never spuriously flagged.
    let ns_rev = handoff_core::current_northstar_revision();
    for t in &tasks {
        let c = t.intent_components(&ns_rev);
        let mut changed: Vec<&'static str> = vec![];
        if !c.objective {
            r.objective_hash_match = false;
            changed.push("objective");
            r.drift
                .push(format!("objective drift: {} (re-mint/reclaim)", t.id));
            r.required_actions.push(format!(
                "re-lock {0} objective: `hf relock {0} \"<reason>\"`",
                t.id
            ));
        }
        if !c.path_scope {
            r.path_scope_match = false;
            changed.push("path_scope");
            r.drift
                .push(format!("path_scope drift: {} (re-lock)", t.id));
            r.required_actions.push(format!(
                "re-lock {0} path_scope: `hf relock {0} \"<reason>\"`",
                t.id
            ));
        }
        if !c.acceptance {
            r.acceptance_hash_match = false;
            changed.push("acceptance");
            r.drift
                .push(format!("acceptance drift: {} (re-lock)", t.id));
            r.required_actions.push(format!(
                "re-lock {0} acceptance: `hf relock {0} \"<reason>\"`",
                t.id
            ));
        }
        if !c.constraint {
            r.constraint_hash_match = false;
            changed.push("constraint");
            r.drift.push(format!(
                "constraint drift: {} — permission/dependency surface changed without re-lock (§12.1)",
                t.id
            ));
            r.required_actions.push(format!(
                "re-lock {0} constraint surface: `hf relock {0} \"<reason>\"`",
                t.id
            ));
        }
        if !c.northstar {
            r.northstar_revision_match = false;
            changed.push("northstar");
            r.drift.push(format!(
                "northstar drift: {} — minted against a superseded doctrine revision (re-mint)",
                t.id
            ));
            r.required_actions.push(format!(
                "re-mint {0} against the current North Star: `hf relock {0} \"<reason>\"` (full lock re-binds northstar)",
                t.id
            ));
        }
        if !changed.is_empty() {
            // observed signature = the live recomputed 5-surface lock, so repeated `hf drift`
            // runs over the same mutated card dedupe to one witnessed event.
            let live = t.full_intent_lock(&ns_rev);
            let sig = format!(
                "{}|{}|{}|{}|{}",
                live.objective_hash,
                live.path_scope_hash,
                live.acceptance_hash,
                live.constraint_hash,
                live.northstar_revision
            );
            r.intent_changed.push((t.id.clone(), changed, sig));
        }
    }

    // PRD §12.3 #6 (edit outside path scope) + #1 (task-active, the deny_without_claim arm):
    // changed files outside any claimed task's path_scope.
    let (claimed, scopes) = claimed_scopes(&tasks, &replay);
    let changed = changed_files();
    if !changed.is_empty() {
        if claimed.is_empty() {
            r.out_of_scope_files = changed.clone();
            r.drift.push(format!(
                "out-of-scope: {} changed file(s) with no task claimed (deny_without_claim)",
                changed.len()
            ));
            r.required_actions
                .push("claim a task before editing".into());
        } else {
            for f in &changed {
                if !in_any_scope(f, &scopes) {
                    r.out_of_scope_files.push(f.clone());
                    r.drift.push(format!(
                        "out-of-scope write: {f} not in claimed scope {claimed:?}"
                    ));
                }
            }
            if !r.out_of_scope_files.is_empty() {
                r.required_actions
                    .push("widen path_scope or revert out-of-scope edits".into());
            }
        }
    }

    // PRD §12.3 #7 (tests map to acceptance criteria) + a kernel ADDITION (missing green test
    // evidence) for in-progress tasks.
    let green = tasks_with_green_tests();
    for t in tasks.iter().filter(|t| in_progress(t)) {
        if t.acceptance_criteria.iter().any(|a| !a.trim().is_empty()) && t.test_commands.is_empty()
        {
            r.acceptance_without_tests.push(t.id.clone());
            r.drift.push(format!(
                "acceptance↔test gap: {} has acceptance criteria but no test_commands",
                t.id
            ));
            r.required_actions
                .push(format!("add test_commands to {}", t.id));
        }
        if !t.test_commands.is_empty() && !green.contains(&t.id) {
            r.missing_evidence.push(t.id.clone());
            r.drift.push(format!(
                "missing test evidence: {} has no green witnessed test_result",
                t.id
            ));
            r.required_actions.push(format!("run `hf test {}`", t.id));
        }
    }

    // Kernel ADDITION (beyond PRD §12.3): dependency satisfaction — an in-progress task must not
    // depend on an unfinished task.
    let done = |id: &str| replay.iter().any(|(k, s)| k == id && *s == Status::Done);
    for t in tasks.iter().filter(|t| in_progress(t)) {
        for dep in &t.dependencies {
            if !done(dep) {
                r.dependency_unsatisfied.push(format!("{} → {}", t.id, dep));
                r.drift.push(format!(
                    "dependency unsatisfied: {} depends on {} which is not Done",
                    t.id, dep
                ));
                r.required_actions
                    .push(format!("finish {dep} before {}", t.id));
            }
        }
    }

    // PRD §12.3 #9 (undocumented architecture change) + approximates #8 (work contradicting a
    //    decision record) (ADVISORY — surfaced, not blocking): a decision-surface file changed
    //    without an accompanying ADR/decision in the same edit. (A fuller semantic #8 — diffing
    //    against recorded decisions — is tracked as future work, not yet a content check.)
    let touches_adr = changed
        .iter()
        .any(|f| f.contains("docs/adr-") || f.contains(".handoff/decisions/"));
    if !touches_adr {
        for f in &changed {
            if is_decision_surface(f) {
                r.undocumented_decisions.push(f.clone());
            }
        }
        if !r.undocumented_decisions.is_empty() {
            r.required_actions
                .push("record an ADR/decision for the policy/CI change".into());
        }
    }

    // PRD §12.3 #8 (work contradicts a decision record) — ADVISORY, now a REAL content check (was
    // "approximated by #9"): prove the tracked tree against the machine-checkable `drift-guard`
    // constraints declared inside the decision records themselves. A violation names the ADR.
    r.decision_contradictions = scan_decision_contradictions(&load_decision_guards());
    if !r.decision_contradictions.is_empty() {
        r.required_actions.push(
            "resolve the decision-record contradiction (revert the change, or amend the ADR if the decision itself changed)"
                .into(),
        );
    }

    // 10) handoff-state-updated (PRD §12.3 #10) — ADVISORY: an active task with material (changed)
    //     files but no witnessed checkpoint yet → handoff state not refreshed after material
    //     changes. Surfaced (not pushed to `drift`), so it never blocks mid-work.
    if !changed.is_empty() {
        let checkpointed = tasks_with_checkpoints();
        for t in tasks.iter().filter(|t| in_progress(t)) {
            if !checkpointed.contains(&t.id) {
                r.handoff_state_stale.push(t.id.clone());
            }
        }
        if !r.handoff_state_stale.is_empty() {
            r.required_actions.push(
                "checkpoint material changes (`hf checkpoint`) so handoff state reflects them"
                    .into(),
            );
        }
    }

    r
}

/// Kept for `hf policy check-handoff` (HFTASK-0015): the blocking drift items + clean flag.
fn detect_drift() -> (Vec<String>, bool) {
    let r = detect();
    (r.drift.clone(), r.clean())
}

/// HFTASK-0047: witness each newly-observed intent mutation as a `handoff.task_intent_changed.v1`
/// event, deduped by the observed 5-surface lock signature so repeated `hf drift` runs over the
/// same mutated card append at most one event per distinct mutation. Best-effort (a witness, not
/// a gate): a ledger-open failure never changes the drift verdict.
fn emit_intent_changed(report: &DriftReport) {
    if report.intent_changed.is_empty() {
        return;
    }
    let path = Path::new(HF).join("ledger.db");
    let Ok(mut led) = Ledger::open(&path.to_string_lossy()) else {
        return;
    };
    // latest observed signature already witnessed per task
    let mut last_sig: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Ok(events) = led.all_events() {
        for e in events {
            if e.event_type != "task_intent_changed" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&e.payload_json)
                && let Some(sig) = v["observed"].as_str()
            {
                last_sig.insert(e.work_order_id, sig.to_string());
            }
        }
    }
    for (id, surfaces, sig) in &report.intent_changed {
        if last_sig.get(id).map(|s| s == sig).unwrap_or(false) {
            continue; // already witnessed this exact mutation
        }
        let payload = serde_json::json!({
            "schema": "handoff.task_intent_changed.v1",
            "id": id,
            "changed_surfaces": surfaces,
            "observed": sig,
        })
        .to_string();
        let _ = led.append("task_intent_changed", id, &payload, handoff_core::now_ns());
    }
}

pub fn cmd_drift(json: bool) {
    let r = detect();
    let clean = r.clean();
    emit_intent_changed(&r);
    if json {
        let out = serde_json::json!({
            "schema": "handoff.drift_report.v1",
            "clean": clean,
            "objective_hash_match": r.objective_hash_match,
            "path_scope_match": r.path_scope_match,
            "acceptance_hash_match": r.acceptance_hash_match,
            "constraint_hash_match": r.constraint_hash_match,
            "northstar_revision_match": r.northstar_revision_match,
            "intent_changed": r.intent_changed.iter().map(|(id, s, _)| serde_json::json!({"id": id, "changed_surfaces": s})).collect::<Vec<_>>(),
            "out_of_scope_files": r.out_of_scope_files,
            "missing_evidence": r.missing_evidence,
            "acceptance_without_tests": r.acceptance_without_tests,
            "dependency_unsatisfied": r.dependency_unsatisfied,
            "undocumented_decisions": r.undocumented_decisions,
            "decision_contradictions": r.decision_contradictions,
            "active_tasks": r.active_tasks,
            "handoff_state_stale": r.handoff_state_stale,
            "drift": r.drift,
            "required_actions": r.required_actions,
        });
        println!("{}", handoff_core::pretty_json(&out));
    } else if clean {
        println!("hf drift: clean — no intent, scope, evidence, or dependency drift");
        if !r.undocumented_decisions.is_empty() {
            println!(
                "  ⓘ advisory: {} decision-surface file(s) changed without an ADR — confirm a decision record",
                r.undocumented_decisions.len()
            );
        }
        if !r.decision_contradictions.is_empty() {
            println!(
                "  ⚠ advisory (PRD §12.3 #8): {} decision-record contradiction(s):",
                r.decision_contradictions.len()
            );
            for c in &r.decision_contradictions {
                println!("    • {c}");
            }
        }
        if !r.handoff_state_stale.is_empty() {
            println!(
                "  ⓘ advisory: {} active task(s) with material changes not yet checkpointed — refresh handoff state (PRD §12.3 #10)",
                r.handoff_state_stale.len()
            );
        }
    } else {
        println!("hf drift: {} drift item(s):", r.drift.len());
        for i in &r.drift {
            println!("  ⚠ {i}");
        }
        // PRD §12.3 #8 advisory: surface decision-record contradictions here too (not only in the
        // clean branch) so the detail accompanies its required_action when drift is non-clean for
        // another reason. JSON already carries it; this keeps the human text complete.
        if !r.decision_contradictions.is_empty() {
            println!(
                "advisory (PRD §12.3 #8): {} decision-record contradiction(s):",
                r.decision_contradictions.len()
            );
            for c in &r.decision_contradictions {
                println!("  • {c}");
            }
        }
        if !r.required_actions.is_empty() {
            println!("required actions:");
            for a in &r.required_actions {
                println!("  → {a}");
            }
        }
    }
    if !clean {
        std::process::exit(1); // hard-fail so PreHandoff (fail_mode=block) stops
    }
}

// --- hf policy check-{claim,edit,handoff} (HFTASK-0015) ---------------------

fn protected_patterns() -> Vec<String> {
    // Read [merge.protected_files].patterns from policies/rules.toml; fall back to the
    // compiled denylist if the file is absent.
    let text = std::fs::read_to_string(Path::new(HF).join("policies").join("rules.toml"))
        .unwrap_or_default();
    if let Ok(v) = text.parse::<toml::Value>()
        && let Some(arr) = v
            .get("merge")
            .and_then(|m| m.get("protected_files"))
            .and_then(|p| p.get("patterns"))
            .and_then(|a| a.as_array())
    {
        let pats: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        if !pats.is_empty() {
            return pats;
        }
    }
    vec![
        ".github/**".into(),
        ".handoff/policy.toml".into(),
        ".handoff/policies/**".into(),
        ".handoff/hooks/**".into(),
        ".handoff/decisions/**".into(),
    ]
}

pub fn cmd_policy_check(kind: &str, json: bool) {
    let tasks = load_tasks();
    let replay = current_statuses();
    let mut blocks: Vec<String> = vec![];

    match kind {
        "check-claim" => {
            // A claim is permitted; the gate just confirms the kernel can resolve a
            // next-safe target (else there is nothing legitimately claimable).
            if handoff_core::next_safe(&tasks, &replay).is_none()
                && !tasks.iter().any(|t| {
                    matches!(
                        status_of(&t.id, &replay, t),
                        Status::Claimed | Status::Active | Status::Checkpointed
                    )
                })
            {
                blocks.push("no claimable next-safe task (all done or blocked)".into());
            }
        }
        "check-edit" => {
            // deny_without_claim + out-of-scope + protected files.
            let (claimed, scopes) = claimed_scopes(&tasks, &replay);
            let changed = changed_files();
            let protected = protected_patterns();
            if !changed.is_empty() && claimed.is_empty() {
                blocks.push("deny_without_claim: edits present with no task claimed".into());
            }
            for f in &changed {
                if !claimed.is_empty() && !in_any_scope(f, &scopes) {
                    blocks.push(format!("out-of-scope write: {f}"));
                }
                if protected.iter().any(|p| glob_match(p, f)) {
                    blocks.push(format!("protected-file write: {f}"));
                }
            }
        }
        "check-handoff" => {
            // require_drift_audit + require_next_command (checkpoint/test evidence are
            // witnessed in the ledger; we assert drift-clean + a resolvable next).
            let (items, clean) = detect_drift();
            if !clean {
                blocks.push(format!(
                    "require_drift_audit: {} drift item(s)",
                    items.len()
                ));
            }
            if handoff_core::next_safe(&tasks, &replay).is_none()
                && tasks
                    .iter()
                    .all(|t| status_of(&t.id, &replay, t) == Status::Done)
            {
                // all done is fine; only block if next is unresolved AND not all-done
            }
        }
        other => {
            eprintln!(
                "hf policy: unknown check '{other}' (use check-claim|check-edit|check-handoff)"
            );
            std::process::exit(2);
        }
    }

    let pass = blocks.is_empty();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.policy_check.v1",
            "check": kind,
            "pass": pass,
            "blocks": blocks,
        });
        println!("{}", handoff_core::pretty_json(&out));
    } else if pass {
        println!("hf policy {kind}: PASS");
    } else {
        println!("hf policy {kind}: BLOCK");
        for b in &blocks {
            println!("  ✗ {b}");
        }
    }
    if !pass {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DriftReport, check_guards, glob_match, is_decision_surface, parse_decision_guards,
    };
    use work_order::WorkOrder;

    #[test]
    fn parse_decision_guard_forbid_and_require() {
        // PRD §12.3 #8: a decision record declares machine-checkable guards via HTML markers.
        let text = r#"
# ADR
prose prose
<!-- drift-guard id="no-foo" forbid="foo =,bar =" path="**/*.toml" reason="banned" -->
more prose
<!-- drift-guard id="keep-baz" require="baz!" path="src/lib.rs" reason="must stay" -->
<!-- drift-guard path="x" -->  (malformed: no forbid/require → skipped)
"#;
        let g = parse_decision_guards(text, "docs/adr-x.md");
        assert_eq!(
            g.len(),
            2,
            "two well-formed guards; the empty one is skipped"
        );
        assert_eq!(g[0].id, "no-foo");
        assert_eq!(g[0].forbid, vec!["foo =".to_string(), "bar =".to_string()]);
        assert_eq!(g[0].path_glob, "**/*.toml");
        assert_eq!(g[0].source, "docs/adr-x.md");
        assert_eq!(g[1].require, vec!["baz!".to_string()]);
    }

    #[test]
    fn check_guards_forbid_fires_only_on_governed_files() {
        let guards = parse_decision_guards(
            r#"<!-- drift-guard id="no-bincode" forbid="bincode =" path="**/*.toml" reason="r" -->"#,
            "src.md",
        );
        // A governed manifest that reintroduces the forbidden dep → contradiction.
        let bad = vec![("crates/x/Cargo.toml".into(), "bincode = \"1\"\n".into())];
        let found = check_guards(&guards, &bad);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("no-bincode") && found[0].contains("crates/x/Cargo.toml"));
        // The same string in a NON-governed file (a .md) does not fire (path glob scopes it).
        let ok_scope = vec![("docs/note.md".into(), "bincode = is mentioned".into())];
        assert!(check_guards(&guards, &ok_scope).is_empty());
        // A clean manifest → no contradiction.
        let clean = vec![("crates/x/Cargo.toml".into(), "postcard = \"1\"\n".into())];
        assert!(check_guards(&guards, &clean).is_empty());
    }

    #[test]
    fn check_guards_require_missing_fires() {
        let guards = parse_decision_guards(
            r#"<!-- drift-guard id="keep" require="MARKER" path="*.rs" reason="r" -->"#,
            "s.md",
        );
        // governs a .rs file that LACKS the required marker → contradiction
        let missing = vec![("a.rs".into(), "fn main() {}".into())];
        assert_eq!(check_guards(&guards, &missing).len(), 1);
        // present → clean
        let present = vec![("a.rs".into(), "// MARKER\nfn main() {}".into())];
        assert!(check_guards(&guards, &present).is_empty());
        // governs NOTHING (no .rs file) → require is not enforced (no false positive)
        let none = vec![("a.toml".into(), "x = 1".into())];
        assert!(check_guards(&guards, &none).is_empty());
    }

    #[test]
    fn advisory_checks_never_block_clean() {
        // PRD §12.3 #1 (active_tasks) and #10 (handoff_state_stale) are ADVISORY: populating
        // them must NOT make the report unclean (clean() ⇔ blocking `drift` is empty). This
        // guards the no-regression contract — the new PRD checks can never false-block the loop.
        let mut r = DriftReport {
            objective_hash_match: true,
            path_scope_match: true,
            acceptance_hash_match: true,
            constraint_hash_match: true,
            northstar_revision_match: true,
            out_of_scope_files: vec![],
            missing_evidence: vec![],
            acceptance_without_tests: vec![],
            dependency_unsatisfied: vec![],
            undocumented_decisions: vec!["..handoff/policy.toml".into()],
            decision_contradictions: vec!["decision contradiction [x]: ...".into()],
            active_tasks: vec!["HFTASK-0001".into()],
            handoff_state_stale: vec!["HFTASK-0001".into()],
            drift: vec![],
            required_actions: vec!["checkpoint".into()],
            intent_changed: vec![],
        };
        assert!(
            r.clean(),
            "advisory active_tasks/handoff_state_stale must not block clean()"
        );
        // A genuine blocking item DOES flip clean() — proving the gate still bites.
        r.drift.push("objective drift: HFTASK-0001".into());
        assert!(
            !r.clean(),
            "a blocking drift item must make the report unclean"
        );
    }

    #[test]
    fn decision_surface_classification() {
        // HFTASK-0046: policy/hooks/CI are decision surfaces; ordinary source is not.
        assert!(is_decision_surface(".handoff/policy.toml"));
        assert!(is_decision_surface(".handoff/policies/rules.toml"));
        assert!(is_decision_surface(".handoff/hooks/hooks.toml"));
        assert!(is_decision_surface(".github/workflows/ci.yml"));
        assert!(!is_decision_surface("hf/src/main.rs"));
        assert!(!is_decision_surface("ledger/src/lib.rs"));
    }

    #[test]
    fn intent_components_detect_each_edit() {
        // A freshly-locked card matches on all five surfaces; mutating one surface trips only
        // that surface (HFTASK-0046 granularity, extended to the §12.1 constraint surface by
        // HFTASK-0047).
        let mk = |obj: &str, scope: &[&str], acc: &[&str]| {
            let path_scope: Vec<String> = scope.iter().map(|s| s.to_string()).collect();
            let acceptance: Vec<String> = acc.iter().map(|s| s.to_string()).collect();
            let mut wo = WorkOrder {
                schema: "handoff.task.v1".into(),
                id: "T".into(),
                title: "t".into(),
                status: work_order::Status::Claimed,
                priority: work_order::Priority::P1,
                objective: obj.into(),
                path_scope: path_scope.clone(),
                acceptance_criteria: acceptance.clone(),
                test_commands: vec![],
                dependencies: vec![],
                blocked_by: vec![],
                allows_network: false,
                allows_dependency_addition: false,
                correlation_id: String::new(),
                role: None,
                intent_lock: WorkOrder::compute_intent_lock(obj, &path_scope, &acceptance),
            };
            // promote to a full 5-surface lock so constraint/northstar are under contract
            wo.intent_lock = wo.full_intent_lock("blake3:rev-1");
            wo
        };
        let good = mk("obj", &["handoff/**"], &["ok"]);
        assert!(good.intent_components("blake3:rev-1").all_match());
        let mut tampered = good.clone();
        tampered.objective = "changed".into();
        let c = tampered.intent_components("blake3:rev-1");
        assert!(!c.objective && c.path_scope && c.acceptance && c.constraint && c.northstar);
        let mut acc_tampered = good.clone();
        acc_tampered.acceptance_criteria = vec!["different".into()];
        let c = acc_tampered.intent_components("blake3:rev-1");
        assert!(c.objective && c.path_scope && !c.acceptance);
        // constraint surface drift
        let mut con_tampered = good.clone();
        con_tampered.allows_network = true;
        assert!(!con_tampered.intent_components("blake3:rev-1").constraint);
    }

    #[test]
    fn glob_forms() {
        assert!(glob_match("**", "anything/here.rs"));
        assert!(glob_match("handoff/**", "handoff/hf/src/main.rs"));
        assert!(glob_match("hf/src/**", "hf/src/gates.rs"));
        assert!(!glob_match("hf/src/**", "ledger/src/lib.rs"));
        assert!(glob_match("**/Cargo.toml", "hf/Cargo.toml"));
        assert!(glob_match("*.lock", "Cargo.lock"));
        assert!(glob_match(
            ".handoff/policies/**",
            ".handoff/policies/rules.toml"
        ));
        assert!(!glob_match(".github/**", ".handoff/hooks/x"));
    }
}
