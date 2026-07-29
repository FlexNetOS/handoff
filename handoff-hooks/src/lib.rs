// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Typed hook contract (HFTASK-0052, PRD §18).
//!
//! HFTASK-0083 (ADR-0019 D5 #4): the FIRST coupled feature module peeled into its own crate
//! (`handoff-hooks`) after the shared helpers were lifted to handoff-core. `hf` aliases it as
//! `hooks` so `hooks::cmd_hook_list` / `hooks::cmd_hook_run` stay valid. Depends only on
//! handoff-core (for `pretty_json`) + serde/serde_json/toml — zero hf-binary coupling.
//!
//! `.handoff/hooks/hooks.toml` lists the lifecycle hooks the agent harness fires. Before this
//! task those were *stringly-typed shell* — a command + a `fail_mode` string, with no typed
//! envelope around the input or the verdict. This module adds the PRD §18 gate contract:
//!
//! - `handoff.hook_event.v1` — the typed event fed to a hook (event name, payload, the
//!   resolved command, timeout, fail_mode).
//! - `handoff.hook_result.v1` — the typed verdict a hook returns: `severity`
//!   (`block`/`warn`/`info`), `pass`, the command's exit code, and any `required_actions`
//!   surfaced from a structured (`*.v1`) command output (e.g. `hf drift --json`).
//!
//! `hf hook run <event>` resolves the event against the contract, runs each matching command,
//! and emits the typed result — fail-closed: a `block`-severity failure exits non-zero so the
//! harness actually stops the loop. Every run is witnessed as a `hook_result` ledger event.

use serde::{Deserialize, Serialize};
use std::path::Path;

const HF: &str = ".handoff";

/// The 14 lifecycle events of the contract (PRD §18). The first six were wired by HFTASK-0015;
/// HFTASK-0052 added `SessionResume`, `PreCommand`, `PostCommand`, `PreTest`, `PostTest`,
/// `PostHandoff`; HFTASK-0069 (ADR-0018 D2) reconciles the last two drifted events into the
/// contract so `hooks.toml` no longer references events the contract rejects: `SessionEnd` (the
/// canonical lifecycle-end event — matches `.claude/settings.json` and replaces the old, dangling
/// `SessionStop` name) and `PostMerge` (HFTASK-0011's one-way `.kb`/meta sync after a merge lands).
///
/// The contract is the single source of truth for which events a hook may bind to. A `hooks.toml`
/// that names an event absent from this list is a **fail-closed drift** — see [`unknown_events`],
/// which surfaces it loudly rather than letting `hf hook list`/`run` silently drop the hook.
pub const CONTRACT_EVENTS: [&str; 14] = [
    "SessionStart",
    "PreSessionStart",
    "SessionResume",
    "SessionEnd",
    "TaskClaim",
    "PreEdit",
    "PostEdit",
    "PreCommand",
    "PostCommand",
    "PreTest",
    "PostTest",
    "PreHandoff",
    "PostHandoff",
    "PostMerge",
];

/// One hook declaration parsed from `hooks.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct HookDef {
    pub event: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_fail_mode")]
    pub fail_mode: String,
}
fn default_timeout() -> u64 {
    30
}
fn default_fail_mode() -> String {
    "warn".to_string()
}

/// The parsed `handoff.hooks.v1` config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub hooks: Vec<HookDef>,
}

impl HooksConfig {
    pub fn load(hf_dir: &Path) -> Self {
        let path = hf_dir.join("hooks").join("hooks.toml");
        let cfg = match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str::<HooksConfig>(&s).unwrap_or_else(|e| {
                eprintln!(
                    "hf hook: {} parse error ({e}); no hooks loaded",
                    path.display()
                );
                HooksConfig::default()
            }),
            Err(_) => HooksConfig::default(),
        };
        // Fail-closed (HFTASK-0069): a hook bound to an event the contract does not define is
        // silently un-runnable (`hf hook run` rejects it; `hf hook list` never shows it). Surface
        // it loudly at load so a drifted `hooks.toml` is a visible error, never an invisible gap.
        let unknown = cfg.unknown_events();
        if !unknown.is_empty() {
            eprintln!(
                "hf hook: WARNING — {} hooks bind to non-contract events {:?}; they will not fire. \
                 Reconcile {} against handoff.hooks.v1 (CONTRACT_EVENTS).",
                unknown.len(),
                unknown,
                path.display()
            );
        }
        cfg
    }

    pub fn for_event<'a>(&'a self, event: &str) -> Vec<&'a HookDef> {
        self.hooks.iter().filter(|h| h.event == event).collect()
    }

    /// Fail-closed conformance check: the set of declared hook events that are NOT contract
    /// events (deduped, source order). Empty == the config conforms to `handoff.hooks.v1`.
    /// Pure so the drift detection is unit-testable without touching disk.
    pub fn unknown_events(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for h in &self.hooks {
            if !CONTRACT_EVENTS.contains(&h.event.as_str()) && !seen.contains(&h.event) {
                seen.push(h.event.clone());
            }
        }
        seen
    }
}

/// The typed input envelope (`handoff.hook_event.v1`).
#[derive(Debug, Clone, Serialize)]
pub struct HookEvent {
    pub schema: &'static str,
    pub event: String,
    pub command: String,
    pub timeout_seconds: u64,
    pub fail_mode: String,
    pub payload: serde_json::Value,
}

/// The typed verdict envelope (`handoff.hook_result.v1`).
#[derive(Debug, Clone, Serialize)]
pub struct HookResult {
    pub schema: &'static str,
    pub event: String,
    pub command: String,
    /// `block` (a fail_mode=block hook failed → hard gate), `warn` (a fail_mode=warn hook
    /// failed → advisory), or `info` (succeeded).
    pub severity: String,
    /// True iff the loop may proceed (a `warn` failure still passes; only `block` fails it).
    pub pass: bool,
    pub exit_code: i32,
    /// Actions surfaced from the command's structured output (drift/policy `*.v1` JSON), if any.
    pub required_actions: Vec<String>,
}

/// Pure severity policy: map (succeeded, fail_mode) → (severity, pass). Split out so the gate
/// semantics are unit-testable without spawning a command.
pub fn severity_for(succeeded: bool, fail_mode: &str) -> (&'static str, bool) {
    match (succeeded, fail_mode) {
        (true, _) => ("info", true),
        (false, "block") => ("block", false),
        (false, _) => ("warn", true), // warn (or any non-block) failure is advisory
    }
}

/// Pull `required_actions` out of a command's stdout when it is a structured `*.v1` envelope
/// (e.g. `hf drift --json`). Pure + best-effort: non-JSON or absent field → empty.
pub fn extract_required_actions(stdout: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .ok()
        .and_then(|v| {
            v.get("required_actions")
                .and_then(|a| a.as_array())
                .cloned()
        })
        .map(|a| {
            a.into_iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Build the typed `handoff.hook_event.v1` envelope for a hook def + payload.
fn to_event(def: &HookDef, payload: &serde_json::Value) -> HookEvent {
    HookEvent {
        schema: "handoff.hook_event.v1",
        event: def.event.clone(),
        command: def.command.clone(),
        timeout_seconds: def.timeout_seconds,
        fail_mode: def.fail_mode.clone(),
        payload: payload.clone(),
    }
}

/// Run one typed hook event's command and build its typed result.
fn run_one(event: &HookEvent) -> HookResult {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&event.command)
        .output();
    let (exit_code, stdout) = match output {
        Ok(o) => (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        ),
        Err(_) => (-1, String::new()),
    };
    let succeeded = exit_code == 0;
    let (severity, pass) = severity_for(succeeded, &event.fail_mode);
    HookResult {
        schema: "handoff.hook_result.v1",
        event: event.event.clone(),
        command: event.command.clone(),
        severity: severity.to_string(),
        pass,
        exit_code,
        required_actions: extract_required_actions(&stdout),
    }
}

/// `hf hook list [--json]` — print the typed contract (the 12 events + which are wired).
pub fn cmd_hook_list(json: bool) {
    let cfg = HooksConfig::load(Path::new(HF));
    let unknown = cfg.unknown_events();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.hooks.v1",
            "contract_events": CONTRACT_EVENTS,
            "hooks": cfg.hooks.iter().map(|h| serde_json::json!({
                "event": h.event, "command": h.command,
                "timeout_seconds": h.timeout_seconds, "fail_mode": h.fail_mode,
            })).collect::<Vec<_>>(),
            // Fail-closed (HFTASK-0069): non-contract events are reported, never hidden.
            "unknown_events": unknown,
            "conformant": unknown.is_empty(),
        });
        println!("{}", handoff_core::pretty_json(&out));
        return;
    }
    println!(
        "hf hook: handoff.hooks.v1 — {} contract events",
        CONTRACT_EVENTS.len()
    );
    for ev in CONTRACT_EVENTS {
        let wired = cfg.for_event(ev);
        if wired.is_empty() {
            println!("  ○ {ev} (no hook)");
        } else {
            for h in wired {
                println!("  ● {ev} → `{}` [{}]", h.command, h.fail_mode);
            }
        }
    }
    // Surface any drifted hooks bound to non-contract events (they will not fire).
    for ev in &unknown {
        for h in cfg.for_event(ev) {
            println!(
                "  ✗ {ev} → `{}` [DANGLING: not a contract event]",
                h.command
            );
        }
    }
}

/// `hf hook run <event> [--payload <json>] [--json]` — fire every hook bound to `event`,
/// emit a typed `handoff.hook_result.v1` per hook, witness each, and exit non-zero if any
/// `block`-severity hook failed (fail-closed). An unknown event is a usage error (exit 2).
pub fn cmd_hook_run(
    event: &str,
    payload_json: Option<&str>,
    json: bool,
    witness: impl Fn(&HookResult),
) -> i32 {
    if event.is_empty() {
        eprintln!("hf hook run: missing <event> (one of {CONTRACT_EVENTS:?})");
        return 2;
    }
    if !CONTRACT_EVENTS.contains(&event) {
        eprintln!("hf hook run: '{event}' is not a contract event {CONTRACT_EVENTS:?}");
        return 2;
    }
    let payload: serde_json::Value = payload_json
        .and_then(|p| serde_json::from_str(p).ok())
        .unwrap_or(serde_json::Value::Null);
    let cfg = HooksConfig::load(Path::new(HF));
    let defs = cfg.for_event(event);
    let results: Vec<HookResult> = defs
        .iter()
        .map(|d| run_one(&to_event(d, &payload)))
        .collect();
    for r in &results {
        witness(r);
    }
    let blocked = results.iter().any(|r| !r.pass);
    if json {
        let out = serde_json::json!({
            "schema": "handoff.hook_result.v1",
            "event": event,
            "pass": !blocked,
            "results": results,
        });
        println!("{}", handoff_core::pretty_json(&out));
    } else if results.is_empty() {
        println!("hf hook run: {event} — no hook bound (no-op)");
    } else {
        for r in &results {
            let glyph = match r.severity.as_str() {
                "block" => "✗",
                "warn" => "⚠",
                _ => "✓",
            };
            println!(
                "hf hook run: {glyph} {event} `{}` → {} (exit {})",
                r.command, r.severity, r.exit_code
            );
            for a in &r.required_actions {
                println!("    → {a}");
            }
        }
    }
    if blocked { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_has_all_fourteen_events() {
        assert_eq!(CONTRACT_EVENTS.len(), 14);
        // ADR-0018 D2 (HFTASK-0069): the 8 events the full-auto loop must cover robustly.
        for e in [
            "SessionStart",
            "SessionResume",
            "SessionEnd",
            "PreCommand",
            "PostCommand",
            "PreTest",
            "PostTest",
            "PostHandoff",
        ] {
            assert!(
                CONTRACT_EVENTS.contains(&e),
                "{e} missing from the contract"
            );
        }
        // The two events reconciled out of `hooks.toml` drift are now first-class.
        assert!(CONTRACT_EVENTS.contains(&"SessionEnd"));
        assert!(CONTRACT_EVENTS.contains(&"PostMerge"));
        // The dangling pre-0069 name must NOT be a contract event (it was renamed to SessionEnd).
        assert!(!CONTRACT_EVENTS.contains(&"SessionStop"));
        // No duplicate event names in the contract.
        let mut uniq = CONTRACT_EVENTS.to_vec();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(
            uniq.len(),
            CONTRACT_EVENTS.len(),
            "duplicate contract event"
        );
    }

    #[test]
    fn unknown_events_is_failclosed_drift_detector() {
        // A config that names a non-contract event is flagged (deduped, source order) — it would
        // otherwise be silently un-runnable.
        let drifted: HooksConfig = toml::from_str(
            r#"
            schema = "handoff.hooks.v1"
            [[hooks]]
            event = "SessionStop"
            command = "hf checkpoint --auto"
            [[hooks]]
            event = "SessionStop"
            command = "hf handoff"
            [[hooks]]
            event = "PostTest"
            command = "hf drift --json"
        "#,
        )
        .unwrap();
        assert_eq!(drifted.unknown_events(), vec!["SessionStop".to_string()]);
        // A fully-canonical config (every event a contract event) reports zero drift.
        let canonical: HooksConfig = toml::from_str(
            r#"
            schema = "handoff.hooks.v1"
            [[hooks]]
            event = "SessionEnd"
            command = "hf checkpoint --auto && hf handoff && hf sync --auto"
            [[hooks]]
            event = "PostMerge"
            command = "hf sync --auto"
        "#,
        )
        .unwrap();
        assert!(canonical.unknown_events().is_empty());
    }

    #[test]
    fn severity_policy() {
        assert_eq!(severity_for(true, "block"), ("info", true));
        assert_eq!(severity_for(true, "warn"), ("info", true));
        // a block hook that fails is a hard gate
        assert_eq!(severity_for(false, "block"), ("block", false));
        // a warn hook that fails is advisory — the loop still proceeds
        assert_eq!(severity_for(false, "warn"), ("warn", true));
        assert_eq!(severity_for(false, "anything-else"), ("warn", true));
    }

    #[test]
    fn required_actions_extracted_from_structured_output() {
        let drift = r#"{"schema":"handoff.drift_report.v1","clean":false,
            "required_actions":["claim a task before editing","run hf test X"]}"#;
        assert_eq!(
            extract_required_actions(drift),
            vec!["claim a task before editing", "run hf test X"]
        );
        // non-JSON / absent field → empty
        assert!(extract_required_actions("not json").is_empty());
        assert!(extract_required_actions(r#"{"clean":true}"#).is_empty());
    }

    #[test]
    fn config_parses_and_filters_by_event() {
        let toml = r#"
            schema = "handoff.hooks.v1"
            [[hooks]]
            event = "PreTest"
            command = "hf drift --json"
            timeout_seconds = 10
            fail_mode = "block"
            [[hooks]]
            event = "PostTest"
            command = "hf checkpoint --auto"
        "#;
        let cfg: HooksConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.hooks.len(), 2);
        let pre = cfg.for_event("PreTest");
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].fail_mode, "block");
        // missing keys fall back to defaults
        let post = cfg.for_event("PostTest");
        assert_eq!(post[0].timeout_seconds, 30);
        assert_eq!(post[0].fail_mode, "warn");
        assert!(cfg.for_event("Nonexistent").is_empty());
    }

    #[test]
    fn canonical_lifecycle_hooks_bind_reap_to_session_end_and_post_merge() {
        // HFTASK-0089: worktree/branch hygiene must be a lifecycle hook, not agent memory.
        // Pin the committed hook contract so drift is caught by normal cargo test.
        let cfg: HooksConfig =
            toml::from_str(include_str!("../../.handoff/hooks/hooks.toml")).unwrap();
        assert!(
            cfg.unknown_events().is_empty(),
            "canonical hooks.toml must not contain dangling lifecycle events"
        );
        let session_end = cfg.for_event("SessionEnd");
        assert_eq!(
            session_end.len(),
            1,
            "SessionEnd must be bound exactly once"
        );
        assert!(
            session_end[0].command.contains("hf session reap"),
            "SessionEnd must reap retained task worktrees"
        );
        let post_merge = cfg.for_event("PostMerge");
        assert_eq!(post_merge.len(), 1, "PostMerge must be bound exactly once");
        assert!(
            post_merge[0].command.contains("hf session reap"),
            "PostMerge must reap retained task worktrees after merge"
        );
    }

    #[test]
    fn canonical_session_end_shell_runs_envctl_reap_with_safe_apply() {
        // HFTASK-0089: the shell hook used by .claude/settings must surface the real envctl
        // worktree/branch reap output and preserve the script's safety rails.
        let script = include_str!("../../.handoff/hooks/session-end.sh");
        assert!(
            script.contains("envctl/scripts/reap-worktrees.sh"),
            "SessionEnd shell hook must call the envctl reap script"
        );
        assert!(
            script.contains("--apply"),
            "SessionEnd shell hook must run the reap script in apply mode"
        );
        assert!(
            !script
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .any(|line| line.contains("--force")),
            "SessionEnd shell hook must never pass --force"
        );
    }
}
