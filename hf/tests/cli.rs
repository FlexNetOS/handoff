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

fn temp_repo(name: &str) -> std::path::PathBuf {
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
    let task_id = "TASK-LEASE-0001";
    write_minimal_task(&repo, task_id);

    let claim = hf()
        .current_dir(&repo)
        .args([
            "--ledger",
            ledger.to_str().expect("ledger path"),
            "claim",
            task_id,
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
            task_id,
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
