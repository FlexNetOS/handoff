// HFTASK-0080 (ADR-0019 D5 #3): this whole crate is a test; unwrap/expect are idiomatic here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! HFTASK-0080: CLI-contract integration tests that drive the REAL `hf` binary.
//!
//! Exit codes set via `std::process::exit` cannot be observed from a unit test in `main.rs`
//! (the process would terminate the test runner), so the unknown-verb fail-closed contract is
//! proven here by spawning the actual compiled binary — the differential-drive doctrine
//! (HFTASK-0078): drive the real CLI, assert on its exit code + output.

use std::process::Command;

fn hf() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hf"))
}

fn fixture_name(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[test]
fn fixture_names_are_portable_for_windows_temp_paths() {
    assert_eq!(
        fixture_name("prompt-hub-vibe---scope-a:b\\c/d?*"),
        "prompt-hub-vibe---scope-a_b_c_d__"
    );
}

fn temp_repo(name: &str) -> std::path::PathBuf {
    let name = fixture_name(name);
    let dir = std::env::temp_dir().join(format!(
        "hf-cli-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join(".handoff/tasks")).expect("mkdir fixture");
    dir
}

fn temp_empty(name: &str) -> std::path::PathBuf {
    let name = fixture_name(name);
    let dir = std::env::temp_dir().join(format!(
        "hf-cli-empty-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("mkdir empty fixture");
    dir
}

fn snapshot_files(root: &std::path::Path) -> Vec<String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read fixture dir") {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            let rel = path
                .strip_prefix(root)
                .expect("fixture-relative path")
                .display()
                .to_string()
                .replace('\\', "/");
            files.push(rel);
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    files.sort();
    files
}

fn write_minimal_task(repo: &std::path::Path, id: &str) {
    let card = serde_json::json!({
        "schema": "handoff.task.v1",
        "id": id,
        "title": "lease release fixture",
        "status": "backlog",
        "priority": "P1",
        "objective": "prove done releases claim lease",
        "path_scope": ["hf/tests/**"],
        "acceptance_criteria": ["done releases claim lease"],
        "test_commands": [],
        "dependencies": [],
        "blocked_by": [],
        "allows_network": false,
        "allows_dependency_addition": false,
        "correlation_id": "lease-release-fixture",
        "role": null,
        "intent_lock": {
            "objective_hash": "fixture-objective",
            "path_scope_hash": "fixture-scope",
            "acceptance_hash": "fixture-acceptance"
        }
    });
    std::fs::write(
        repo.join(".handoff/tasks").join(format!("{id}.task.json")),
        serde_json::to_string_pretty(&card).expect("task json"),
    )
    .expect("write task");
}

/// An UNKNOWN verb (e.g. a typo like `hf promot`) MUST fail closed with exit 2, not the prior
/// fail-OPEN exit 0 that made a typo look like it succeeded.
#[test]
fn unknown_verb_exits_2_fail_closed() {
    let out = hf()
        .arg("definitely-not-a-verb")
        .output()
        .expect("spawn hf");
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unknown verb must fail closed with exit 2, got {:?}",
        out.status.code()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown command"),
        "stderr should name the unknown command, got: {stderr}"
    );
}

/// Bare `hf` (no subcommand) stays a usage/help path at exit 0 — unchanged behavior, so the fix
/// is a strict upgrade scoped to the unknown-verb case only (no regression for the help path).
#[test]
fn bare_invocation_prints_usage_exit_0() {
    let out = hf().output().expect("spawn hf");
    assert_eq!(
        out.status.code(),
        Some(0),
        "bare `hf` is the help path, exit 0, got {:?}",
        out.status.code()
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("hf [--ledger PATH]"),
        "bare `hf` should print the usage line, got: {stderr}"
    );
}

#[test]
fn top_level_help_paths_exit_0() {
    for args in [["--help"].as_slice(), ["help"].as_slice()] {
        let out = hf().args(args).output().expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(0),
            "`hf {}` should be a successful help path",
            args.join(" ")
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("usage: hf [--ledger PATH] <command>"),
            "top-level help should print agent navigation usage, got: {stdout}"
        );
    }
}

#[test]
fn grouped_help_paths_exit_0_and_stay_focused() {
    let cases = [
        (
            ["fleet", "--help"].as_slice(),
            "usage: hf fleet <status|sync|render>",
        ),
        (
            ["help", "fleet"].as_slice(),
            "usage: hf fleet <status|sync|render>",
        ),
        (
            ["task", "--help"].as_slice(),
            "usage: hf task mint --from-kb SLUG",
        ),
        (
            ["help", "task"].as_slice(),
            "usage: hf task mint --from-kb SLUG",
        ),
        (
            ["prompt-hub", "--help"].as_slice(),
            "usage: hf prompt-hub \"<vibe>\"",
        ),
        (
            ["help", "prompt-hub"].as_slice(),
            "usage: hf prompt-hub \"<vibe>\"",
        ),
    ];
    for (args, expected) in cases {
        let out = hf().args(args).output().expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(0),
            "`hf {}` should be a successful focused help path",
            args.join(" ")
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(expected),
            "`hf {}` should print focused usage `{expected}`, got: {stdout}",
            args.join(" ")
        );
        assert!(
            out.stderr.is_empty(),
            "successful help should not look like an error, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn common_help_topics_exit_0_without_contradicting_top_level_guidance() {
    let cases = [
        ("resume", "usage: hf resume"),
        ("status", "usage: hf status"),
        ("claim", "usage: hf claim"),
        ("checkpoint", "usage: hf checkpoint"),
        ("test", "usage: hf test"),
        ("done", "usage: hf done"),
        ("drift", "usage: hf drift"),
        ("release", "usage: hf release"),
        ("reopen", "usage: hf reopen"),
        ("handoff", "usage: hf handoff"),
        ("ship", "usage: hf ship"),
        ("lease", "usage: hf lease"),
        ("version", "usage: hf version"),
        ("policy", "usage: hf policy"),
    ];
    for (topic, expected) in cases {
        for args in [vec!["help", topic], vec![topic, "--help"]] {
            let out = hf().args(&args).output().expect("spawn hf");
            assert_eq!(
                out.status.code(),
                Some(0),
                "`hf {}` should be a successful focused help path",
                args.join(" ")
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                stdout.contains(expected),
                "`hf {}` should print focused usage `{expected}`, got: {stdout}",
                args.join(" ")
            );
            assert!(
                out.stderr.is_empty(),
                "successful help should not look like an error, stderr: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn documented_command_help_is_side_effect_free() {
    let topics = [
        "version",
        "init",
        "seed",
        "status",
        "index",
        "plan",
        "session",
        "claim",
        "doctor",
        "gitignore",
        "reconcile",
        "export",
        "import",
        "migrate",
        "release",
        "reopen",
        "checkpoint",
        "sync-cards",
        "sync",
        "done",
        "test",
        "task",
        "intake",
        "prompt-hub",
        "dispatch",
        "delivery",
        "ship",
        "promote",
        "review",
        "drift",
        "policy",
        "gatekeeper",
        "hook",
        "lease",
        "fleet",
        "schema",
        "handoff",
        "resume",
    ];

    for topic in topics {
        for args in [vec!["help", topic], vec![topic, "--help"]] {
            let repo = temp_empty(topic);
            let before = snapshot_files(&repo);
            let ledger = repo.join(".handoff/ledger.db");
            let out = hf()
                .current_dir(&repo)
                .env("HANDOFF_LEDGER", &ledger)
                .args(&args)
                .output()
                .expect("spawn hf");
            assert_eq!(
                out.status.code(),
                Some(0),
                "`hf {}` should be documented help, stdout: {}, stderr: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(
                String::from_utf8_lossy(&out.stdout).contains("usage: hf"),
                "`hf {}` should print focused usage, got: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stdout)
            );
            assert_eq!(
                snapshot_files(&repo),
                before,
                "`hf {}` help must not create or mutate files",
                args.join(" ")
            );
        }
    }
}

#[test]
fn index_unknown_flag_fails_closed_without_writing_maps() {
    let repo = temp_empty("index-unknown-flag");
    let before = snapshot_files(&repo);
    let out = hf()
        .current_dir(&repo)
        .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
        .args(["index", "--intent-aware"])
        .output()
        .expect("spawn hf");
    assert_eq!(
        out.status.code(),
        Some(2),
        "unsupported index flags must fail closed, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown flag '--intent-aware'"),
        "stderr should name the unsupported flag, got: {stderr}"
    );
    assert_eq!(
        snapshot_files(&repo),
        before,
        "unsupported `hf index` flags must not write .handoff/maps"
    );
}

#[test]
fn setup_commands_reject_unknown_args_without_writes() {
    for (command, args) in [
        ("init", vec!["init", "--definitely-unsupported-flag"]),
        ("seed", vec!["seed", "--definitely-unsupported-flag"]),
    ] {
        let repo = temp_empty(&format!("{command}-unknown-arg"));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(&args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject unsupported args before setup writes, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!(
                "hf {command}: unknown flag '--definitely-unsupported-flag'"
            )),
            "`hf {}` stderr should name the unsupported flag and command, got: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create .handoff files before rejecting unsupported args",
            args.join(" ")
        );
    }

    for flag in ["--name", "--northstar", "--role", "--plane"] {
        let repo = temp_empty(&format!("init-missing-{}", flag.trim_start_matches("--")));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(["init", flag])
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf init {flag}` should reject missing values before setup writes"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("requires a value"),
            "missing init option value should be explicit, got: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf init {flag}` must not create files before rejecting the missing value"
        );
    }

    let repo = temp_empty("init-supported-options");
    let out = hf()
        .current_dir(&repo)
        .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
        .args([
            "init",
            "--name",
            "agent-nav",
            "--northstar",
            "agent navigation",
            "--role",
            "kernel",
            "--plane",
            "orchestration",
        ])
        .output()
        .expect("spawn hf init");
    assert_eq!(
        out.status.code(),
        Some(0),
        "supported init options should still work, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let capsule = std::fs::read_to_string(repo.join(".handoff/context/capsule.json"))
        .expect("init should write capsule");
    assert!(
        capsule.contains("\"project_name\": \"agent-nav\"")
            && capsule.contains("\"northstar\": \"agent navigation\"")
            && capsule.contains("\"role\": \"kernel\"")
            && capsule.contains("\"plane\": \"orchestration\""),
        "supported init options should populate capsule, got: {capsule}"
    );
}

#[test]
fn prompt_hub_rejects_malformed_args_without_writes() {
    for args in [
        ["prompt-hub", "vibe", "--definitely-unsupported-flag"].as_slice(),
        ["prompt-hub", "vibe", "--scope"].as_slice(),
        ["prompt-hub", "vibe", "--scope", "--json"].as_slice(),
        ["prompt-hub", "vibe", "extra"].as_slice(),
    ] {
        let repo = temp_empty(&format!("prompt-hub-malformed-{}", args.join("-")));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject malformed args before minting, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create task/bundle artifacts before rejecting malformed args",
            args.join(" ")
        );
    }

    let repo = temp_empty("prompt-hub-supported");
    let out = hf()
        .current_dir(&repo)
        .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
        .args(["prompt-hub", "audit supported path", "--scope", "hf/src/**"])
        .output()
        .expect("spawn hf prompt-hub");
    assert_eq!(
        out.status.code(),
        Some(0),
        "supported prompt-hub args should still mint, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let files = snapshot_files(&repo);
    assert!(
        files.iter().any(|p| p.starts_with(".handoff/tasks/"))
            && files.iter().any(|p| p.starts_with(".handoff/bundles/")),
        "supported prompt-hub should mint task and bundle artifacts, files: {files:?}"
    );
}

#[test]
fn nested_readonly_commands_reject_unknown_flags_without_writes() {
    for (command, args) in [
        (
            "hook list",
            vec!["hook", "list", "--definitely-unsupported-flag"],
        ),
        (
            "delivery list",
            vec!["delivery", "list", "--definitely-unsupported-flag"],
        ),
        (
            "policy check-edit",
            vec!["policy", "check-edit", "--definitely-unsupported-flag"],
        ),
    ] {
        let repo = temp_empty(&format!("nested-{}", command.replace(' ', "-")));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(&args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject unsupported nested flags, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!(
                "hf {command}: unknown flag '--definitely-unsupported-flag'"
            )),
            "`hf {}` stderr should name the nested command and unsupported flag, got: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create files before rejecting unsupported nested flags",
            args.join(" ")
        );
    }

    for args in [
        ["hook", "list", "--json"].as_slice(),
        ["hook", "run", "PreEdit", "--payload", "{}", "--json"].as_slice(),
        ["delivery", "list", "--json"].as_slice(),
        ["delivery", "get", "missing-correlation", "--json"].as_slice(),
        ["policy", "check-edit", "--json"].as_slice(),
        ["policy", "check-handoff", "--json"].as_slice(),
    ] {
        let repo = temp_empty(&format!("nested-supported-{}", args.join("-")));
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(args)
            .output()
            .expect("spawn hf");
        assert_ne!(
            out.status.code(),
            Some(2),
            "`hf {}` is a supported nested command shape and should not be rejected as unsupported args, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn high_impact_commands_reject_unknown_flags_before_work() {
    let rejected_cases = vec![
        (
            "fleet status",
            vec!["fleet", "status", "--definitely-unsupported-flag"],
        ),
        (
            "fleet sync",
            vec![
                "fleet",
                "sync",
                "--definitely-unsupported-flag",
                "--dry-run",
            ],
        ),
        (
            "fleet render",
            vec![
                "fleet",
                "render",
                "handoff",
                "--definitely-unsupported-flag",
            ],
        ),
        (
            "sync",
            vec!["sync", "--definitely-unsupported-flag", "--dry-run"],
        ),
        ("export", vec!["export", "--definitely-unsupported-flag"]),
        (
            "dispatch",
            vec!["dispatch", "missing-cid", "--definitely-unsupported-flag"],
        ),
        ("intake", vec!["intake", "--definitely-unsupported-flag"]),
        (
            "task mint",
            vec!["task", "mint", "--definitely-unsupported-flag"],
        ),
        (
            "review request",
            vec!["review", "request", "123", "--definitely-unsupported-flag"],
        ),
        (
            "gatekeeper check",
            vec![
                "gatekeeper",
                "check",
                "123",
                "--definitely-unsupported-flag",
            ],
        ),
        #[cfg(feature = "cognitum")]
        (
            "policy gate",
            vec!["policy", "gate", "Claim", "--definitely-unsupported-flag"],
        ),
    ];

    for (command, args) in rejected_cases {
        let repo = temp_empty(&format!("high-impact-{}", command.replace(' ', "-")));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(&args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject unsupported flags before work, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!(
                "hf {command}: unknown flag '--definitely-unsupported-flag'"
            )),
            "`hf {}` stderr should name command and unsupported flag, got: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create files before rejecting unsupported flags",
            args.join(" ")
        );
    }

    let supported_shapes = vec![
        ["fleet", "status", "--json"].as_slice(),
        ["fleet", "sync", "--dry-run", "--json"].as_slice(),
        ["sync", "--dry-run", "--json"].as_slice(),
        ["dispatch", "missing-cid", "--next"].as_slice(),
        ["intake", "--bundle", "missing.json", "--scope", "hf/src/**"].as_slice(),
        ["task", "mint", "--from-kb", "missing-slug"].as_slice(),
        ["review", "request", "123", "--task", "TASK-123"].as_slice(),
        ["gatekeeper", "check", "123", "--task", "TASK-123"].as_slice(),
        #[cfg(feature = "cognitum")]
        ["policy", "gate", "Claim", "--task", "TASK-123"].as_slice(),
    ];

    for args in supported_shapes {
        let repo = temp_empty(&format!("high-impact-supported-{}", args.join("-")));
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(args)
            .output()
            .expect("spawn hf");
        assert_ne!(
            out.status.code(),
            Some(2),
            "`hf {}` is a supported command shape and should not be rejected as unsupported args, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn fleet_status_dry_run_requires_fix_before_work() {
    let repo = temp_empty("fleet-status-dry-run-without-fix");
    let before = snapshot_files(&repo);
    let out = hf()
        .current_dir(&repo)
        .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
        .args(["fleet", "status", "--dry-run", "--json"])
        .output()
        .expect("spawn hf");
    assert_eq!(
        out.status.code(),
        Some(2),
        "hf fleet status --dry-run should fail closed without --fix, stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--dry-run is only valid with --fix"),
        "stderr should explain the valid dry-run route, got: {stderr}"
    );
    assert_eq!(
        snapshot_files(&repo),
        before,
        "hf fleet status --dry-run must not create files before rejecting the no-op dry-run"
    );
}

#[test]
fn remaining_stateful_commands_reject_unknown_args_before_work() {
    for (command, args) in [
        (
            "claim",
            vec!["claim", "TASK-X", "--definitely-unsupported-flag"],
        ),
        (
            "claim --next",
            vec!["claim", "--next", "--definitely-unsupported-flag"],
        ),
        (
            "done",
            vec!["done", "TASK-X", "--definitely-unsupported-flag"],
        ),
        ("done --pr", vec!["done", "TASK-X", "--pr"]),
        (
            "test",
            vec!["test", "TASK-X", "--definitely-unsupported-flag"],
        ),
        ("migrate", vec!["migrate", "--definitely-unsupported-flag"]),
        (
            "reopen",
            vec![
                "reopen",
                "TASK-X",
                "reason",
                "--definitely-unsupported-flag",
            ],
        ),
        (
            "ship",
            vec!["ship", "TASK-X", "--definitely-unsupported-flag"],
        ),
        ("ship --base", vec!["ship", "TASK-X", "--base"]),
        ("promote", vec!["promote", "--definitely-unsupported-flag"]),
        (
            "session start",
            vec!["session", "start", "--definitely-unsupported-flag"],
        ),
        ("session start --base", vec!["session", "start", "--base"]),
        (
            "session end",
            vec!["session", "end", "--definitely-unsupported-flag"],
        ),
        (
            "session reap",
            vec!["session", "reap", "--definitely-unsupported-flag"],
        ),
        (
            "schema",
            vec!["schema", "--check", "--definitely-unsupported-flag"],
        ),
    ] {
        let repo = temp_empty(&format!(
            "remaining-stateful-{}",
            command.replace(' ', "-").replace("--", "")
        ));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(&args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject unsupported or malformed args before work, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("unknown") || stderr.contains("requires a value"),
            "`hf {}` stderr should explain the argument problem, got: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create files before rejecting bad args",
            args.join(" ")
        );
    }
}

#[test]
fn side_effecting_no_arg_commands_reject_unknown_args_without_writes() {
    for (command, args) in [
        ("plan", vec!["plan", "--definitely-unsupported-flag"]),
        ("handoff", vec!["handoff", "--definitely-unsupported-flag"]),
        (
            "reconcile",
            vec!["reconcile", "--definitely-unsupported-flag"],
        ),
        (
            "sync-cards",
            vec!["sync-cards", "--definitely-unsupported-flag"],
        ),
    ] {
        let repo = temp_empty(&format!("{command}-unknown-arg"));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(&args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject unsupported arguments before side effects, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!(
                "hf {command}: unknown flag '--definitely-unsupported-flag'"
            )),
            "`hf {}` stderr should name the unsupported flag and command, got: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create or mutate files before rejecting unsupported args",
            args.join(" ")
        );
    }
}

#[test]
fn read_only_lifecycle_commands_reject_unknown_args_without_writes() {
    for (command, args) in [
        ("version", vec!["version", "--definitely-unsupported-flag"]),
        ("status", vec!["status", "--definitely-unsupported-flag"]),
        ("resume", vec!["resume", "--definitely-unsupported-flag"]),
        ("drift", vec!["drift", "--definitely-unsupported-flag"]),
        ("release", vec!["release", "--definitely-unsupported-flag"]),
        (
            "checkpoint",
            vec!["checkpoint", "--definitely-unsupported-flag"],
        ),
    ] {
        let repo = temp_empty(&format!("{command}-unknown-arg"));
        let before = snapshot_files(&repo);
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(&args)
            .output()
            .expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` should reject unsupported arguments before work, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains(&format!(
                "hf {command}: unknown flag '--definitely-unsupported-flag'"
            )),
            "`hf {}` stderr should name the unsupported flag and command, got: {stderr}",
            args.join(" ")
        );
        assert_eq!(
            snapshot_files(&repo),
            before,
            "`hf {}` must not create or mutate files before rejecting unsupported args",
            args.join(" ")
        );
    }

    for args in [
        ["version", "--json"].as_slice(),
        ["status", "--json"].as_slice(),
        ["resume", "--json"].as_slice(),
        ["resume", "--compact"].as_slice(),
        ["drift", "--json"].as_slice(),
        ["lease", "--json"].as_slice(),
        ["doctor", "--json"].as_slice(),
        ["checkpoint", "--auto", "--quiet"].as_slice(),
        ["release", "NOT-A-HELD-TASK"].as_slice(),
    ] {
        let repo = temp_empty(&format!("supported-{}", args.join("-")));
        let out = hf()
            .current_dir(&repo)
            .env("HANDOFF_LEDGER", repo.join(".handoff/ledger.db"))
            .args(args)
            .output()
            .expect("spawn hf");
        assert_ne!(
            out.status.code(),
            Some(2),
            "`hf {}` is a supported lifecycle path and should not be rejected as an unsupported arg, stdout: {}, stderr: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn unknown_command_still_exits_2_after_help_expansion() {
    for args in [
        ["definitely-not-a-verb"].as_slice(),
        ["help", "definitely-not-a-verb"].as_slice(),
        ["definitely-not-a-verb", "--help"].as_slice(),
    ] {
        let out = hf().args(args).output().expect("spawn hf");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`hf {}` must fail closed with exit 2, got {:?}",
            args.join(" "),
            out.status.code()
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("unknown command") || stderr.contains("unknown help topic"),
            "stderr should name the unknown command/topic, got: {stderr}"
        );
    }
}

#[test]
fn done_releases_claim_lease_so_agents_see_no_false_holder() {
    let repo = temp_repo("done-release");
    let ledger = repo.join(".handoff/ledger.db");
    let task_id = format!(
        "TASK-LEASE-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    write_minimal_task(&repo, &task_id);

    let claim = hf()
        .current_dir(&repo)
        .args([
            "--ledger",
            ledger.to_str().expect("ledger path"),
            "claim",
            &task_id,
        ])
        .output()
        .expect("spawn hf claim");
    assert_eq!(
        claim.status.code(),
        Some(0),
        "claim should succeed, stderr: {}",
        String::from_utf8_lossy(&claim.stderr)
    );

    let lease_before = hf()
        .current_dir(&repo)
        .args([
            "--ledger",
            ledger.to_str().expect("ledger path"),
            "lease",
            "--json",
        ])
        .output()
        .expect("spawn hf lease before");
    assert_eq!(lease_before.status.code(), Some(0));
    let before: serde_json::Value =
        serde_json::from_slice(&lease_before.stdout).expect("lease json before done");
    let held_before = before["held"].as_array().expect("held array");
    assert!(
        held_before
            .iter()
            .any(|h| h["resource"] == format!("handoff:claim:{task_id}")),
        "claimed task should be visible as held before done: {before}"
    );

    let done = hf()
        .current_dir(&repo)
        .args([
            "--ledger",
            ledger.to_str().expect("ledger path"),
            "done",
            &task_id,
        ])
        .output()
        .expect("spawn hf done");
    assert_eq!(
        done.status.code(),
        Some(0),
        "done should succeed, stderr: {}",
        String::from_utf8_lossy(&done.stderr)
    );

    let lease_after = hf()
        .current_dir(&repo)
        .args([
            "--ledger",
            ledger.to_str().expect("ledger path"),
            "lease",
            "--json",
        ])
        .output()
        .expect("spawn hf lease after");
    assert_eq!(lease_after.status.code(), Some(0));
    let after: serde_json::Value =
        serde_json::from_slice(&lease_after.stdout).expect("lease json after done");
    let held_after = after["held"].as_array().expect("held array");
    assert!(
        held_after
            .iter()
            .all(|h| h["resource"] != format!("handoff:claim:{task_id}")),
        "done task must not remain visible as a held lease: {after}"
    );
}
