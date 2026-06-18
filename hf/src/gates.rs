//! `hf drift` (HFTASK-0005) and `hf policy check-{claim,edit,handoff}` (HFTASK-0015)
//! — the two hard gates the `.handoff/hooks/hooks.toml` contract fires (PreEdit,
//! PreHandoff, TaskClaim). Both emit JSON for hook callers and exit non-zero on a
//! block so `fail_mode = block` hooks actually stop the loop. Fail-closed.

use crate::{current_statuses, load_tasks, status_of};
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

/// Tasks (by id) whose LATEST witnessed `test_result` is green (HFTASK-0045/0046).
fn tasks_with_green_tests() -> HashSet<String> {
    let mut green = HashSet::new();
    let path = Path::new(HF).join("ledger.db");
    if let Ok(led) = Ledger::open(&path.to_string_lossy()) {
        if let Ok(events) = led.all_events() {
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
    }
    green
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
        drift: vec![],
        required_actions: vec![],
        intent_changed: vec![],
    };

    // 1–3, 9–10) per-surface intent_lock drift across all FIVE surfaces (HFTASK-0047). The
    // constraint/northstar checks no-op on a legacy partial lock (empty fields), so old cards
    // are never spuriously flagged.
    let ns_rev = crate::current_northstar_revision();
    for t in &tasks {
        let c = t.intent_components(&ns_rev);
        let mut changed: Vec<&'static str> = vec![];
        if !c.objective {
            r.objective_hash_match = false;
            changed.push("objective");
            r.drift
                .push(format!("objective drift: {} (re-mint/reclaim)", t.id));
            r.required_actions
                .push(format!("re-lock {} objective", t.id));
        }
        if !c.path_scope {
            r.path_scope_match = false;
            changed.push("path_scope");
            r.drift
                .push(format!("path_scope drift: {} (re-lock)", t.id));
            r.required_actions
                .push(format!("re-lock {} path_scope", t.id));
        }
        if !c.acceptance {
            r.acceptance_hash_match = false;
            changed.push("acceptance");
            r.drift
                .push(format!("acceptance drift: {} (re-lock)", t.id));
            r.required_actions
                .push(format!("re-lock {} acceptance", t.id));
        }
        if !c.constraint {
            r.constraint_hash_match = false;
            changed.push("constraint");
            r.drift.push(format!(
                "constraint drift: {} — permission/dependency surface changed without re-lock (§12.1)",
                t.id
            ));
            r.required_actions
                .push(format!("re-lock {} constraint surface", t.id));
        }
        if !c.northstar {
            r.northstar_revision_match = false;
            changed.push("northstar");
            r.drift.push(format!(
                "northstar drift: {} — minted against a superseded doctrine revision (re-mint)",
                t.id
            ));
            r.required_actions
                .push(format!("re-mint {} against the current North Star", t.id));
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

    // 4) out-of-scope edits: changed files outside any claimed task's path_scope.
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

    // 5–6) evidence & acceptance↔test mapping for in-progress tasks.
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

    // 7) dependency satisfaction: an in-progress task must not depend on an unfinished task.
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

    // 8) undocumented architecture/decision change (ADVISORY — surfaced, not blocking): a
    //    decision-surface file changed without an accompanying ADR/decision in the same edit.
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
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&e.payload_json) {
                if let Some(sig) = v["observed"].as_str() {
                    last_sig.insert(e.work_order_id, sig.to_string());
                }
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
        let _ = led.append("task_intent_changed", id, &payload, crate::now_ns());
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
            "drift": r.drift,
            "required_actions": r.required_actions,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else if clean {
        println!("hf drift: clean — no intent, scope, evidence, or dependency drift");
        if !r.undocumented_decisions.is_empty() {
            println!(
                "  ⓘ advisory: {} decision-surface file(s) changed without an ADR — confirm a decision record",
                r.undocumented_decisions.len()
            );
        }
    } else {
        println!("hf drift: {} drift item(s):", r.drift.len());
        for i in &r.drift {
            println!("  ⚠ {i}");
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
    if let Ok(v) = text.parse::<toml::Value>() {
        if let Some(arr) = v
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
            if crate::next_safe(&tasks, &replay).is_none()
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
            if crate::next_safe(&tasks, &replay).is_none()
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
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
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
    use super::{glob_match, is_decision_surface};
    use work_order::WorkOrder;

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
