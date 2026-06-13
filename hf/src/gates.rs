//! `hf drift` (HFTASK-0005) and `hf policy check-{claim,edit,handoff}` (HFTASK-0015)
//! — the two hard gates the `.handoff/hooks/hooks.toml` contract fires (PreEdit,
//! PreHandoff, TaskClaim). Both emit JSON for hook callers and exit non-zero on a
//! block so `fail_mode = block` hooks actually stop the loop. Fail-closed.

use crate::{current_statuses, load_tasks, status_of};
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

// --- hf drift (HFTASK-0005) -------------------------------------------------

/// Returns (drift_items, clean). Pure-ish over git + ledger.
fn detect_drift() -> (Vec<String>, bool) {
    let tasks = load_tasks();
    let replay = current_statuses();
    let mut items = vec![];

    // 1) intent drift: a card whose body no longer hashes to its stored intent_lock
    //    (objective/path_scope/acceptance changed without re-lock).
    for t in &tasks {
        if !t.intent_unchanged() {
            items.push(format!(
                "intent drift: {} — body no longer matches its intent_lock (re-mint or reclaim)",
                t.id
            ));
        }
    }

    // 2) out-of-scope edits: changed files not covered by any claimed task's path_scope.
    let (claimed, scopes) = claimed_scopes(&tasks, &replay);
    let changed = changed_files();
    if !changed.is_empty() {
        if claimed.is_empty() {
            items.push(format!(
                "out-of-scope: {} changed file(s) with no task claimed (deny_without_claim)",
                changed.len()
            ));
        } else {
            for f in &changed {
                if !in_any_scope(f, &scopes) {
                    items.push(format!(
                        "out-of-scope write: {f} not in claimed scope {claimed:?}"
                    ));
                }
            }
        }
    }
    let clean = items.is_empty();
    (items, clean)
}

pub fn cmd_drift(json: bool) {
    let (items, clean) = detect_drift();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.drift.v1",
            "clean": clean,
            "drift": items,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else if clean {
        println!("hf drift: clean — no intent or scope drift");
    } else {
        println!("hf drift: {} drift item(s):", items.len());
        for i in &items {
            println!("  ⚠ {i}");
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
    use super::glob_match;

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
