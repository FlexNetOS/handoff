//! Integration tests for the `rusty-idd spec` subcommands, run against the
//! compiled binary and the oracle fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rusty-idd")
}

/// Repo root = three levels up from `crates/cli/tests` → the workspace root,
/// where `docs/rusty-idd/oracle-fixtures/` lives.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixture(name: &str) -> PathBuf {
    repo_root()
        .join("docs/rusty-idd/oracle-fixtures")
        .join(name)
}

#[test]
fn spec_validate_base_fixture_is_valid_json() {
    let out = Command::new(bin())
        .args(["spec", "validate"])
        .arg(fixture("01-base-spec.md"))
        .arg("--json")
        .output()
        .expect("run rusty-idd");

    assert!(out.status.success(), "base spec should validate (exit 0)");

    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout must be valid JSON");
    let item = &json["items"][0];
    assert_eq!(item["type"], "spec");
    assert_eq!(item["valid"], true);
    // The only issue is the Purpose-too-brief WARNING (matching oracle 04).
    assert_eq!(item["issues"][0]["level"], "WARNING");
    assert_eq!(item["issues"][0]["path"], "overview");
    assert_eq!(json["summary"]["totals"]["passed"], 1);
    assert_eq!(json["summary"]["totals"]["failed"], 0);
    assert_eq!(json["version"], "1.0");
}

#[test]
fn spec_validate_no_scenario_is_invalid_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("spec.md");
    std::fs::write(
        &spec,
        "# no-scenario Specification\n\n## Purpose\n\
         A sufficiently long purpose so the brevity warning does not fire here.\n\n\
         ## Requirements\n\n### Requirement: Some rule\nThe system SHALL do a thing.\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["spec", "validate"])
        .arg(&spec)
        .arg("--json")
        .output()
        .expect("run rusty-idd");

    assert_eq!(
        out.status.code(),
        Some(1),
        "a no-scenario spec must exit nonzero"
    );

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let item = &json["items"][0];
    assert_eq!(item["valid"], false);
    let err = &item["issues"][0];
    assert_eq!(err["level"], "ERROR");
    assert_eq!(err["path"], "requirements.0.scenarios");
    assert!(err["message"]
        .as_str()
        .unwrap()
        .contains("at least one scenario"));
    assert_eq!(json["summary"]["totals"]["failed"], 1);
}

#[test]
fn spec_validate_strict_fails_on_warning() {
    // The base fixture is valid but has a WARNING; --strict must fail it.
    let out = Command::new(bin())
        .args(["spec", "validate"])
        .arg(fixture("01-base-spec.md"))
        .arg("--strict")
        .output()
        .expect("run rusty-idd");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn spec_validate_strict_human_summary_matches_exit_code() {
    // Regression: the human summary must agree with the exit code under --strict.
    // The base fixture is valid-with-WARNING: a strict run exits 1, so the
    // printed summary must read FAILED (not the old "1 passed, 0 failed").
    let strict = Command::new(bin())
        .args(["spec", "validate"])
        .arg(fixture("01-base-spec.md"))
        .arg("--strict")
        .output()
        .expect("run rusty-idd");
    assert_eq!(strict.status.code(), Some(1));
    let text = String::from_utf8_lossy(&strict.stdout);
    assert!(
        text.contains("0 passed, 1 failed"),
        "strict summary must report the WARNING item as failed, got:\n{text}"
    );
    assert!(
        text.contains("INVALID"),
        "strict per-item status must read INVALID, got:\n{text}"
    );

    // Without --strict the same spec passes: summary reads passed, exit 0.
    let lax = Command::new(bin())
        .args(["spec", "validate"])
        .arg(fixture("01-base-spec.md"))
        .output()
        .expect("run rusty-idd");
    assert!(lax.status.success());
    let lax_text = String::from_utf8_lossy(&lax.stdout);
    assert!(
        lax_text.contains("1 passed, 0 failed"),
        "non-strict summary must report passed, got:\n{lax_text}"
    );
}

#[test]
fn spec_validate_json_is_strict_blind() {
    // --strict must NOT alter the JSON payload (oracle parity): a warning-only
    // spec still serializes valid:true / passed:1 regardless of --strict, while
    // the process still exits 1 under --strict.
    let out = Command::new(bin())
        .args(["spec", "validate"])
        .arg(fixture("01-base-spec.md"))
        .args(["--json", "--strict"])
        .output()
        .expect("run rusty-idd");
    assert_eq!(out.status.code(), Some(1), "strict still exits 1");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        json["items"][0]["valid"], true,
        "JSON payload stays strict-blind"
    );
    assert_eq!(json["summary"]["totals"]["passed"], 1);
    assert_eq!(json["summary"]["totals"]["failed"], 0);
}

#[test]
fn spec_validate_all_finds_all_specs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let openspec = root.join("openspec");
    let specs = openspec.join("specs");
    let changes = openspec.join("changes");
    std::fs::create_dir_all(&specs).unwrap();
    std::fs::create_dir_all(changes.join("change-1/specs/cap-1")).unwrap();

    // Valid base spec
    std::fs::write(
        specs.join("base.md"),
        "# Base Specification\n\n## Purpose\nThis is a long enough purpose to pass the brevity warning.\n\n## Requirements\n### Requirement: Req\nThe system SHALL do it.\n#### Scenario: Scen\nStep 1.\n",
    ).unwrap();

    // Valid change spec
    std::fs::write(
        changes.join("change-1/specs/cap-1/spec.md"),
        "## ADDED Requirements\n### Requirement: Req B\nThe system SHALL also do this.\n#### Scenario: Scen B\nStep 1.\n",
    ).unwrap();

    let out = Command::new(bin())
        .current_dir(root)
        .args(["spec", "validate", "--all", "--json"])
        .output()
        .expect("run rusty-idd");

    if !out.status.success() {
        eprintln!("STDOUT: {}", String::from_utf8_lossy(&out.stdout));
        eprintln!("STDERR: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(out.status.success(), "batch validate should succeed");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["items"].as_array().unwrap().len(), 2);

    let items = json["items"].as_array().unwrap();
    let types: Vec<&str> = items.iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert!(types.contains(&"spec"));
    assert!(types.contains(&"change"));
    assert_eq!(json["summary"]["totals"]["items"], 2);
    assert_eq!(json["summary"]["totals"]["passed"], 2);
}
