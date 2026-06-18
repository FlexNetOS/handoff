//! Integration tests for `rusty-idd spec scaffold` / `spec new` — the scaffold
//! (minijinja) surface, exercised through the compiled binary.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rusty-idd")
}

#[test]
fn scaffold_proposal_injects_change_and_has_no_jinja() {
    let out = Command::new(bin())
        .args(["spec", "scaffold", "proposal", "--change", "add-json"])
        .output()
        .expect("run rusty-idd");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("# add-json\n"), "{text}");
    assert!(text.contains("## Why") && text.contains("## What Changes"));
    assert!(
        !text.contains("{{") && !text.contains("}}"),
        "no unrendered vars"
    );
}

#[test]
fn scaffold_adr_injects_number_title_date() {
    let out = Command::new(bin())
        .args([
            "spec",
            "scaffold",
            "adr",
            "--number",
            "0009",
            "--title",
            "Use queues",
            "--date",
            "2026-06-04",
        ])
        .output()
        .expect("run rusty-idd");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("# 0009. Use queues\n"), "{text}");
    assert!(text.contains("- Date: 2026-06-04"), "{text}");
}

#[test]
fn scaffold_unknown_artifact_errors() {
    let out = Command::new(bin())
        .args(["spec", "scaffold", "nope"])
        .output()
        .expect("run rusty-idd");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn new_creates_proposal_and_refuses_overwrite() {
    let base = tempfile::tempdir().unwrap();
    let out = Command::new(bin())
        .args(["spec", "new", "add-export", "--base"])
        .arg(base.path())
        .output()
        .expect("run rusty-idd");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let proposal = base.path().join("openspec/changes/add-export/proposal.md");
    assert!(proposal.is_file());
    let body = std::fs::read_to_string(&proposal).unwrap();
    assert!(body.starts_with("# add-export\n"), "{body}");

    // Re-running refuses to overwrite.
    let again = Command::new(bin())
        .args(["spec", "new", "add-export", "--base"])
        .arg(base.path())
        .output()
        .expect("run rusty-idd");
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("refusing to overwrite"));
}

/// The scaffolded `spec` delta stub must parse via the engine and `spec show`.
#[test]
fn scaffolded_spec_stub_is_show_able() {
    let dir = tempfile::tempdir().unwrap();
    let spec_path = dir.path().join("spec.md");
    let out = Command::new(bin())
        .args(["spec", "scaffold", "spec"])
        .output()
        .expect("run rusty-idd");
    assert!(out.status.success());
    std::fs::write(&spec_path, &out.stdout).unwrap();

    let show = Command::new(bin())
        .args(["spec", "show"])
        .arg(&spec_path)
        .output()
        .expect("run rusty-idd");
    assert!(
        show.status.success(),
        "spec show must handle a scaffolded stub"
    );
}
