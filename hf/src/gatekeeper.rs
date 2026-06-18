//! HFTASK-0014: surgical AI gatekeeper foundation.
//!
//! This module adds a deterministic, witnessed `hf gatekeeper check <pr>` command. It is the
//! first slice of the §5b code-omniscient merge approver. Full code-intelligence (git-kb code
//! index / kb_callers / kb_impact / RuVector grounding) is not yet wired, so the gatekeeper
//! currently uses the signals available today:
//!   - PR changed files (via `gh`)
//!   - Local build/test gate (`cargo test --workspace`)
//!   - Lightweight impact scan (`git grep` for changed module names)
//!   - envctl secrets-engine merge-gate enforcement (when the `secrets` feature is enabled)

use std::collections::HashSet;
use std::path::PathBuf;

use crate::{ledger_path, now_ns, route::route_for_task, run_out, Ledger};

/// The result of a lightweight impact scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrImpact {
    pub changed: Vec<String>,
    pub impacted: Vec<String>,
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

/// Lightweight impact scan: for each changed `.rs` file, grep the repo for references to its
/// module name / path. Returns changed files and a deduplicated list of files that reference
/// them (the "blast radius" estimate).
pub fn impact_scan(files: &[String]) -> PrImpact {
    let changed: Vec<String> = files.to_vec();
    let mut impacted = HashSet::new();
    for f in &changed {
        for token in search_tokens(f) {
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
    PrImpact { changed, impacted }
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

#[cfg(not(feature = "secrets"))]
fn merge_gate_check() -> Result<bool, String> {
    Ok(false)
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
    let changed_files = pr_changed_files(pr).unwrap_or_default();
    let impact = impact_scan(&changed_files);

    let test_ok = run_test_gate().is_ok();
    let merge_gate_ok = merge_gate_check().unwrap_or(false);

    let mut reasons: Vec<String> = Vec::new();
    if !test_ok {
        reasons.push("cargo test failed".into());
    }
    if !merge_gate_ok {
        reasons.push("merge gate denied".into());
    }
    #[cfg(not(feature = "secrets"))]
    reasons.push("secrets-engine merge gate unavailable in this build".into());

    let verdict = if test_ok && merge_gate_ok && !changed_files.is_empty() {
        GatekeeperVerdict::Approve
    } else {
        GatekeeperVerdict::Deny
    };

    let payload = serde_json::json!({
        "pr": meta.url,
        "number": meta.number,
        "head": meta.head_ref_name,
        "base": meta.base_ref_name,
        "verdict": verdict.as_str(),
        "reasons": reasons,
        "changed_files": impact.changed,
        "impacted_files": impact.impacted,
        "task_id": task_id,
    })
    .to_string();

    led.append("gatekeeper_judgment", work_order_id, &payload, now_ns())
        .unwrap();

    match verdict {
        GatekeeperVerdict::Approve => {
            println!(
                "hf gatekeeper: approve PR #{} ({} changed, {} impacted)",
                meta.number,
                impact.changed.len(),
                impact.impacted.len()
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
    fn search_tokens_for_rust_file() {
        let toks = search_tokens("src/gatekeeper.rs");
        assert!(toks.contains(&"gatekeeper".to_string()));
        assert!(toks.contains(&"gatekeeper.rs".to_string()));
    }

    #[test]
    fn impact_scan_detects_reference() {
        // main.rs declares `mod route;`, so changing route.rs should show main.rs in the
        // impacted set when we grep for the module name.
        let impact = impact_scan(&["src/route.rs".into()]);
        assert!(
            impact.impacted.iter().any(|f| f.ends_with("main.rs")),
            "expected main.rs to reference route.rs; got {:?}",
            impact.impacted
        );
    }

    #[test]
    fn impact_scan_empty_for_unreferenced() {
        // Construct the path at runtime so the full token never appears as a literal
        // in any tracked file, guaranteeing an empty impacted set.
        let name = format!("zzzz{}nonexistent{}9999.rs", "_", "_");
        let impact = impact_scan(&[name]);
        assert!(impact.impacted.is_empty());
    }
}
