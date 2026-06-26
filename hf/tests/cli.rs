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
