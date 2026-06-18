//! Typed hook contract (HFTASK-0052, PRD §18).
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

/// The 12 lifecycle events of the contract (PRD §18). The first six were wired by HFTASK-0015;
/// HFTASK-0052 completes the set with `SessionResume`, `PreCommand`, `PostCommand`, `PreTest`,
/// `PostTest`, `PostHandoff`.
pub const CONTRACT_EVENTS: [&str; 12] = [
    "SessionStart",
    "PreSessionStart",
    "SessionResume",
    "TaskClaim",
    "PreEdit",
    "PostEdit",
    "PreCommand",
    "PostCommand",
    "PreTest",
    "PostTest",
    "PreHandoff",
    "PostHandoff",
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
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str::<HooksConfig>(&s).unwrap_or_else(|e| {
                eprintln!(
                    "hf hook: {} parse error ({e}); no hooks loaded",
                    path.display()
                );
                HooksConfig::default()
            }),
            Err(_) => HooksConfig::default(),
        }
    }

    pub fn for_event<'a>(&'a self, event: &str) -> Vec<&'a HookDef> {
        self.hooks.iter().filter(|h| h.event == event).collect()
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
    if json {
        let out = serde_json::json!({
            "schema": "handoff.hooks.v1",
            "contract_events": CONTRACT_EVENTS,
            "hooks": cfg.hooks.iter().map(|h| serde_json::json!({
                "event": h.event, "command": h.command,
                "timeout_seconds": h.timeout_seconds, "fail_mode": h.fail_mode,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
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
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
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
    if blocked {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_has_all_twelve_events() {
        assert_eq!(CONTRACT_EVENTS.len(), 12);
        for e in [
            "SessionResume",
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
}
