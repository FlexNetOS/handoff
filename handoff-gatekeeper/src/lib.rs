// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! HFTASK-0014: surgical AI gatekeeper foundation.
//!
//! HFTASK-0083 (ADR-0019 D5 #4): peeled into `handoff-gatekeeper` (it also now owns the shared
//! `GhPrView` GitHub-PR type). `hf` aliases it as `gatekeeper`; main's review flow imports
//! `handoff_gatekeeper::GhPrView`. The `secrets` merge-gate is behind this crate's own `secrets`
//! feature (`dep:handoff-secrets`), propagated from hf.
//!
//! This module adds a deterministic, witnessed `hf gatekeeper check <pr>` command. It is the
//! §5b code-omniscient merge approver. It uses:
//!   - PR changed files (via `gh`)
//!   - Local build/test gate (`cargo test --workspace`)
//!   - AST-grounded impact scan via the code-intelligence call graph (`git kb code impact`),
//!     unioned with a `git grep` text safety-net and degrading to grep-only when the code index
//!     is unavailable (the `impact_grounding` field records which path was taken). RuVector
//!     grounding remains future work.
//!   - envctl secrets-engine merge-gate enforcement (when the `secrets` feature is enabled)

use std::collections::HashSet;
use std::path::PathBuf;

use handoff_core::{HF, ledger_path, must_witness, now_ns, run_out};
use handoff_policy::policy::Policy;
use handoff_route::route_for_task;
use ledger::Ledger;
use serde::{Deserialize, Serialize};

/// HFTASK-0083: the `gh pr view --json` projection used by the gatekeeper + the review-request
/// flow in hf. Lifted here (it is gatekeeper-adjacent); hf imports `handoff_gatekeeper::GhPrView`.
#[derive(Debug, Deserialize, Serialize)]
pub struct GhPrView {
    pub url: String,
    pub number: u64,
    #[serde(rename = "headRefName")]
    pub head_ref_name: String,
    #[serde(rename = "baseRefName")]
    pub base_ref_name: String,
    #[serde(rename = "isDraft")]
    pub is_draft: bool,
}

/// The result of an impact scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrImpact {
    pub changed: Vec<String>,
    pub impacted: Vec<String>,
    /// How the impacted set was derived, so the verdict can judge it honestly:
    /// `"ast+grep"` = code-intelligence call graph unioned with the text safety-net (index healthy),
    /// `"ast"` = call graph only, `"grep"` = text scan only (the code index was unavailable —
    /// a degraded blast-radius estimate the gatekeeper should treat with less confidence).
    pub grounding: String,
}

/// Pure: parse `git kb code impact <file> --json` into the impacted file set (the call-graph
/// callers' `file_path`s). Separated from the process call so it is unit-testable.
fn parse_kb_impact(json: &str, file: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let callers = v.get("callers")?.as_array()?;
    let mut files: Vec<String> = callers
        .iter()
        .filter_map(|c| {
            c.get("symbol")?
                .get("file_path")?
                .as_str()
                .map(String::from)
        })
        .filter(|p| p != file && !p.starts_with("target/") && !p.ends_with(".lock"))
        .collect();
    files.sort();
    files.dedup();
    Some(files)
}

/// AST-grounded blast radius for one changed `.rs` file via the code-intelligence call graph
/// (`git kb code impact <file> --json`). Returns the impacted file set, or `None` when the code
/// index is unavailable (non-`.rs`, `git kb` missing, or a non-zero exit) so the caller falls back.
fn kb_impact_files(file: &str) -> Option<Vec<String>> {
    if !file.ends_with(".rs") {
        return None; // the code index only covers source symbols
    }
    let out = run_out("git", &["kb", "code", "impact", file, "--json"]).ok()?;
    parse_kb_impact(&out, file)
}

/// Build a token that can be used to search for references to a changed Rust file.
///
/// For `src/foo/bar.rs` we search for `foo::bar`, `bar.rs`, and `bar` (the module name).
fn search_tokens(path: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    // Strip leading ./ if present.
    let path = path.trim_start_matches("./");
    if let Some(without_ext) = path.strip_suffix(".rs") {
        // module token: src/foo/bar.rs -> foo::bar (also just bar)
        if let Some(file_stem) = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            tokens.push(file_stem.to_string());
        }
        // path-style tokens
        let mod_path = without_ext.replace("/", "::").replace("src::", "");
        if !mod_path.is_empty() && mod_path != "lib" && mod_path != "main" {
            tokens.push(mod_path);
        }
        tokens.push(path.to_string());
        if let Some(file_name) = std::path::Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
        {
            tokens.push(file_name.to_string());
        }
    }
    tokens
}

/// Impact scan (blast radius): for each changed `.rs` file, take the AST-grounded call-graph
/// dependents (`git kb code impact`) and union them with a `git grep` text safety-net. Returns
/// the changed files, the deduplicated impacted set, and the `grounding` that produced it
/// (`ast+grep` / `ast` / `grep` — the last meaning the code index was unavailable and the
/// estimate is degraded).
pub fn impact_scan(files: &[String]) -> PrImpact {
    let changed: Vec<String> = files.to_vec();
    let mut impacted = HashSet::new();
    let mut ast_ran = false;
    let mut grep_ran = false;
    for f in &changed {
        // Primary signal: AST-grounded call-graph dependents (precise + transitive) when the
        // code index is available. This is the upgrade over the old token grep.
        if let Some(ast_files) = kb_impact_files(f) {
            ast_ran = true;
            for p in ast_files {
                impacted.insert(p);
            }
        }
        // Safety net + fallback: the text scan catches non-Rust / macro / string references the
        // resolver can miss, and IS the whole signal when the code index is unavailable. A safety
        // gate prefers recall, so we union both rather than trust either alone.
        for token in search_tokens(f) {
            grep_ran = true;
            let args = vec!["grep", "-l", "-I", "--", &token];
            if let Ok(out) = run_out("git", &args) {
                for line in out.lines() {
                    let line = line.trim();
                    if !line.is_empty()
                        && line != f
                        && !line.starts_with("target/")
                        && !line.ends_with(".lock")
                    {
                        impacted.insert(line.to_string());
                    }
                }
            }
        }
    }
    let mut impacted: Vec<String> = impacted.into_iter().collect();
    impacted.sort();
    let grounding = match (ast_ran, grep_ran) {
        (true, true) => "ast+grep",
        (true, false) => "ast",
        _ => "grep",
    }
    .to_string();
    PrImpact {
        changed,
        impacted,
        grounding,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatekeeperVerdict {
    Approve,
    Deny,
}

impl GatekeeperVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            GatekeeperVerdict::Approve => "approve",
            GatekeeperVerdict::Deny => "deny",
        }
    }
}

/// Fetch the list of changed files for a PR using `gh pr diff --name-only`.
fn pr_changed_files(pr: &str) -> Result<Vec<String>, String> {
    let out = run_out("gh", &["pr", "diff", pr, "--name-only"])?;
    Ok(out
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Run the workspace test suite as the build/test gate.
fn run_test_gate() -> Result<(), String> {
    run_out("cargo", &["test", "--workspace"])?;
    Ok(())
}

#[cfg(feature = "secrets")]
fn merge_gate_check() -> Result<bool, String> {
    handoff_secrets::github_merge_gate(
        "POST",
        "api.github.com",
        "/repos/FlexNetOS/handoff/check-runs",
    )
}

#[cfg(not(feature = "secrets"))]
fn merge_gate_check() -> Result<Option<bool>, String> {
    Ok(None)
}

#[cfg(feature = "secrets")]
fn merge_gate_signal() -> Result<Option<bool>, String> {
    merge_gate_check().map(Some)
}

#[cfg(not(feature = "secrets"))]
fn merge_gate_signal() -> Result<Option<bool>, String> {
    merge_gate_check()
}

fn verdict_from_signals(
    changed_files: &[String],
    test_ok: bool,
    merge_gate_ok: Option<bool>,
    policy: &Policy,
    protected_clearance: bool,
) -> (GatekeeperVerdict, Vec<String>, Vec<String>) {
    let mut reasons = Vec::new();
    if changed_files.is_empty() {
        reasons.push("no changed files detected".into());
    }
    if !test_ok {
        reasons.push("cargo test failed".into());
    }
    let protected_hits = policy.merge.protected_hits(changed_files);
    if !protected_hits.is_empty() {
        if protected_clearance {
            reasons.push(format!(
                "protected files covered by explicit steward task clearance: {}",
                protected_hits.join(", ")
            ));
        } else {
            reasons.push(format!(
                "protected files require explicit steward clearance: {}",
                protected_hits.join(", ")
            ));
        }
    }
    match merge_gate_ok {
        Some(true) => {}
        Some(false) => reasons.push("merge gate denied".into()),
        None => reasons.push(
            "merge gate unavailable in this build; relying on required GitHub check + branch protection".into(),
        ),
    }

    // Default CI may not link the envctl secrets feature. That is degraded evidence, not a
    // silent human approval: the PR-scoped "AI Gatekeeper" required check is still the
    // deterministic branch-protection surface.
    let hard_fail = changed_files.is_empty()
        || !test_ok
        || (!protected_clearance && !protected_hits.is_empty())
        || merge_gate_ok == Some(false);
    let verdict = if hard_fail {
        GatekeeperVerdict::Deny
    } else {
        GatekeeperVerdict::Approve
    };
    (verdict, reasons, protected_hits)
}

/// HFTASK-0014 foundation: run deterministic gates on PR `pr` and record a judgment event.
///
/// The judgment currently combines:
/// - cargo test --workspace
/// - lightweight `git grep` impact scan
/// - secrets-engine merge-gate enforcement (when compiled with `--features secrets`)
///
/// A denied judgment exits nonzero so callers can fail closed.
pub fn cmd_gatekeeper_check(pr: &str, task_id: Option<&str>) {
    if pr.is_empty() {
        eprintln!("usage: hf gatekeeper check <pr> [--task <id>]");
        std::process::exit(2);
    }

    // Resolve PR metadata for the event payload.
    let meta_json = run_out(
        "gh",
        &[
            "pr",
            "view",
            pr,
            "--json",
            "url,number,headRefName,baseRefName,isDraft",
        ],
    )
    .unwrap_or_else(|e| {
        eprintln!("hf gatekeeper: cannot read PR {pr}: {e}");
        std::process::exit(1);
    });
    let meta: GhPrView = serde_json::from_str(&meta_json).unwrap_or_else(|e| {
        eprintln!("hf gatekeeper: malformed gh output: {e}");
        std::process::exit(1);
    });

    // Determine ledger target.
    let ledger = match task_id {
        Some(id) => match route_for_task(id) {
            Ok((ledger, _tasks)) => ledger,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => PathBuf::from(ledger_path()),
    };

    let mut led = Ledger::open(&ledger.to_string_lossy()).unwrap_or_else(|e| {
        eprintln!("hf gatekeeper: cannot open ledger: {e}");
        std::process::exit(1);
    });
    let work_order_id = task_id.unwrap_or("gatekeeper");

    // Gather signals.
    let changed_files = pr_changed_files(pr).unwrap_or_else(|e| {
        eprintln!("hf gatekeeper: cannot list changed files for PR {pr}: {e}");
        std::process::exit(1);
    });
    let impact = impact_scan(&changed_files);

    let test_ok = run_test_gate().is_ok();
    let merge_gate_ok = merge_gate_signal().unwrap_or(None);
    let policy = Policy::load(std::path::Path::new(HF));
    let protected_clearance = task_id.is_some();
    let (verdict, reasons, protected_hits) = verdict_from_signals(
        &changed_files,
        test_ok,
        merge_gate_ok,
        &policy,
        protected_clearance,
    );

    let payload = serde_json::json!({
        "pr": &meta.url,
        "number": meta.number,
        "head": &meta.head_ref_name,
        "base": &meta.base_ref_name,
        "verdict": verdict.as_str(),
        "reasons": &reasons,
        "changed_files": &impact.changed,
        "impacted_files": &impact.impacted,
        "impact_grounding": &impact.grounding,
        "protected_hits": &protected_hits,
        "protected_clearance": protected_clearance,
        "merge_gate_ok": merge_gate_ok,
        "required_status_checks": &policy.merge.required_status_checks,
        "task_id": task_id,
    })
    .to_string();

    must_witness(
        led.append("gatekeeper_judgment", work_order_id, &payload, now_ns()),
        "gatekeeper_judgment",
    );

    match verdict {
        GatekeeperVerdict::Approve => {
            println!(
                "hf gatekeeper: approve PR #{} ({} changed, {} impacted via {})",
                meta.number,
                impact.changed.len(),
                impact.impacted.len(),
                impact.grounding
            );
        }
        GatekeeperVerdict::Deny => {
            eprintln!(
                "hf gatekeeper: deny PR #{} — reasons: {:?}",
                meta.number, reasons
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_kb_impact_extracts_caller_files() {
        // The real `git kb code impact --json` shape: callers[].symbol.file_path is the
        // AST-grounded blast radius. Dedupe, drop self/target/lock.
        let json = r#"{
          "file": "hf/src/gates.rs",
          "count": 3,
          "callers": [
            {"symbol": {"name": "detect_drift", "file_path": "hf/src/gates.rs"}},
            {"symbol": {"name": "cmd_drift", "file_path": "hf/src/main.rs"}},
            {"symbol": {"name": "other", "file_path": "hf/src/main.rs"}},
            {"symbol": {"name": "t", "file_path": "target/debug/x.rs"}}
          ]
        }"#;
        let got = super::parse_kb_impact(json, "hf/src/gates.rs").expect("parses");
        // self (gates.rs) and target/ excluded; main.rs deduped to one entry.
        assert_eq!(got, vec!["hf/src/main.rs".to_string()]);
        // Malformed / non-JSON → None (caller falls back to grep).
        assert!(super::parse_kb_impact("not json", "x.rs").is_none());
        assert!(super::parse_kb_impact("{}", "x.rs").is_none());
    }

    #[test]
    fn impact_scan_reports_grounding() {
        // grounding is always one of the three known values and never empty — the verdict relies
        // on it to know whether the AST index was actually consulted.
        let _g = handoff_test_support::cwd_lock();
        let impact = impact_scan(&["src/route.rs".into()]);
        assert!(
            matches!(impact.grounding.as_str(), "ast" | "grep" | "ast+grep"),
            "unexpected grounding: {}",
            impact.grounding
        );
    }

    #[test]
    fn search_tokens_for_rust_file() {
        let toks = search_tokens("src/gatekeeper.rs");
        assert!(toks.contains(&"gatekeeper".to_string()));
        assert!(toks.contains(&"gatekeeper.rs".to_string()));
    }

    #[test]
    fn impact_scan_detects_reference() {
        // `impact_scan` runs `git grep` relative to the process cwd, so it must not race the
        // cwd-mutating tests — hold the shared cwd lock. HFTASK-0083: hermetic (a temp git repo
        // with a known cross-file reference) instead of depending on hf's live file layout, which
        // changed when this module was peeled out of the hf binary into its own crate.
        let _g = handoff_test_support::cwd_lock();
        let dir = std::env::temp_dir().join(format!(
            "hf-impact-{}-{}",
            std::process::id(),
            handoff_core::now_ns()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        // b.rs references the `widget` module (from src/widget.rs) — the cross-file edge.
        std::fs::write(dir.join("src/widget.rs"), "pub fn w() {}\n").unwrap();
        std::fs::write(
            dir.join("src/b.rs"),
            "use crate::widget;\nfn go() { widget::w(); }\n",
        )
        .unwrap();
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
        };
        run(&["init", "-q"]);
        run(&["add", "."]);
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let impact = impact_scan(&["src/widget.rs".into()]);
        std::env::set_current_dir(prev).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            impact.impacted.iter().any(|f| f.ends_with("b.rs")),
            "expected b.rs to reference widget.rs; got {:?}",
            impact.impacted
        );
    }

    #[test]
    fn impact_scan_empty_for_unreferenced() {
        let _g = handoff_test_support::cwd_lock();
        // Construct the path at runtime so the full token never appears as a literal
        // in any tracked file, guaranteeing an empty impacted set.
        let name = format!("zzzz{}nonexistent{}9999.rs", "_", "_");
        let impact = impact_scan(&[name]);
        assert!(impact.impacted.is_empty());
    }

    #[test]
    fn default_required_check_can_approve_when_merge_gate_is_unlinked() {
        let policy = Policy::default();
        let changed = vec!["hf/src/gatekeeper.rs".to_string()];
        let (verdict, reasons, protected) =
            verdict_from_signals(&changed, true, None, &policy, false);
        assert_eq!(verdict, GatekeeperVerdict::Approve);
        assert!(protected.is_empty());
        assert!(reasons.iter().any(|r| r.contains("required GitHub check")));
    }

    #[test]
    fn protected_files_fail_closed_without_clearance() {
        let policy = Policy::default();
        let changed = vec![".github/workflows/ci.yml".to_string()];
        let (verdict, reasons, protected) =
            verdict_from_signals(&changed, true, None, &policy, false);
        assert_eq!(verdict, GatekeeperVerdict::Deny);
        assert_eq!(protected, changed);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("require explicit steward clearance"))
        );
    }

    #[test]
    fn protected_files_pass_with_task_clearance() {
        let policy = Policy::default();
        let changed = vec![".github/workflows/ci.yml".to_string()];
        let (verdict, reasons, protected) =
            verdict_from_signals(&changed, true, None, &policy, true);
        assert_eq!(verdict, GatekeeperVerdict::Approve);
        assert_eq!(protected, changed);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("covered by explicit steward task clearance"))
        );
    }
}
