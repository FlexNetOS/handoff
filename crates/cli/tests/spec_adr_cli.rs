//! Integration tests for `rusty-idd spec adr` — the ADR supersession-graph
//! surface, exercised through the compiled binary.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rusty-idd")
}

/// Build a temp `adr/` dir with a small supersession chain + a proposed ADR +
/// a non-ADR file that must be ignored. Returns (tempdir, adr_dir).
fn make_adr_dir() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let adr = root.path().join("adr");
    std::fs::create_dir_all(&adr).unwrap();
    std::fs::write(
        adr.join("0001-use-postgres.md"),
        "# 0001. Use Postgres\n\n- Status: accepted\n- Date: 2026-01-01\n\n## Context\nx\n",
    )
    .unwrap();
    std::fs::write(
        adr.join("0002-switch-sqlite.md"),
        "# 0002. Switch to SQLite\n\n- Status: accepted, supersedes ADR-0001\n- Date: 2026-02-01\n\n## Context\ny\n",
    )
    .unwrap();
    std::fs::write(
        adr.join("0003-redis.md"),
        "# 0003. Maybe Redis\n\n- Status: proposed\n- Date: 2026-03-01\n\n## Context\nz\n",
    )
    .unwrap();
    std::fs::write(adr.join("README.md"), "not an ADR\n").unwrap();
    (root, adr)
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("run rusty-idd");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn adr_list_shows_only_in_force() {
    let (_root, adr) = make_adr_dir();
    let (code, out) = run(&["spec", "adr", "list", adr.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.contains("In-force ADRs (1)"), "{out}");
    assert!(
        out.contains("ADR-0002") && out.contains("Switch to SQLite"),
        "{out}"
    );
    // Superseded + proposed are NOT in the in-force list.
    assert!(!out.contains("ADR-0001"), "{out}");
    assert!(!out.contains("ADR-0003"), "{out}");
}

#[test]
fn adr_list_all_shows_status_for_each() {
    let (_root, adr) = make_adr_dir();
    let (code, out) = run(&["spec", "adr", "list", adr.to_str().unwrap(), "--all"]);
    assert_eq!(code, 0);
    assert!(
        out.contains("ADR-0001") && out.contains("superseded(by 0002)"),
        "{out}"
    );
    assert!(
        out.contains("ADR-0002") && out.contains("in-force"),
        "{out}"
    );
    assert!(
        out.contains("ADR-0003") && out.contains("proposed"),
        "{out}"
    );
    // README.md is not an ADR and must be ignored.
    assert!(!out.contains("README"), "{out}");
}

#[test]
fn adr_next_is_max_plus_one_zero_padded() {
    let (_root, adr) = make_adr_dir();
    let (code, out) = run(&["spec", "adr", "next", adr.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "0004");
}

#[test]
fn adr_next_on_missing_dir_starts_at_0001() {
    let (code, out) = run(&["spec", "adr", "next", "/no/such/adr/dir"]);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "0001");
}
