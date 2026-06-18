//! Integration tests for `rusty-idd spec status` / `spec next` — the schema
//! artifact-DAG surface, exercised through the compiled binary.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rusty-idd")
}

/// Build a temp OpenSpec change dir containing the given artifact files.
/// Returns (tempdir, change_dir). `adr` lives at the repo-level `adr/` reached
/// from the change dir via `../../../adr`.
fn make_change(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let change_dir = root.path().join("openspec/changes/demo");
    std::fs::create_dir_all(&change_dir).unwrap();
    for f in files {
        match *f {
            "proposal" => write(&change_dir.join("proposal.md"), "proposal"),
            "design" => write(&change_dir.join("design.md"), "design"),
            "tasks" => write(&change_dir.join("tasks.md"), "- [ ] 1.1 do it"),
            "specs" => write(&change_dir.join("specs/cap/spec.md"), "# Cap"),
            "adr" => write(&root.path().join("adr/0001-x.md"), "# 0001"),
            other => panic!("unknown artifact {other}"),
        }
    }
    (root, change_dir)
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(args: &[&str], change_dir: &Path) -> (i32, String) {
    let out = Command::new(bin())
        .args(args)
        .arg(change_dir)
        .output()
        .expect("run rusty-idd");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn status_reports_next_and_blockers() {
    let (_root, change_dir) = make_change(&["proposal", "specs"]);
    let (code, out) = run(&["spec", "status"], &change_dir);
    assert_eq!(code, 0);
    assert!(out.contains("[x] proposal"), "{out}");
    assert!(out.contains("[x] specs"), "{out}");
    assert!(
        out.contains("[ ] design") && out.contains("<- next"),
        "{out}"
    );
    assert!(
        out.contains("adr") && out.contains("blocked by: design"),
        "{out}"
    );
    assert!(out.contains("Archivable: no (2/5 artifacts done)"), "{out}");
    assert!(out.contains("Next: design"), "{out}");
}

#[test]
fn next_prints_first_ready_artifact() {
    let (_root, change_dir) = make_change(&[]);
    let (code, out) = run(&["spec", "next"], &change_dir);
    assert_eq!(code, 0);
    assert_eq!(out.trim(), "proposal", "nothing done → proposal is next");

    let (_root2, change_dir2) = make_change(&["proposal", "specs", "design"]);
    let (_c, out2) = run(&["spec", "next"], &change_dir2);
    assert_eq!(out2.trim(), "adr");
}

#[test]
fn status_archivable_when_all_done() {
    let (_root, change_dir) = make_change(&["proposal", "specs", "design", "adr", "tasks"]);
    let (code, out) = run(&["spec", "status"], &change_dir);
    assert_eq!(code, 0);
    assert!(
        out.contains("Archivable: yes (5/5 artifacts done)"),
        "{out}"
    );
    assert!(out.contains("ready to archive"), "{out}");

    let (_c, next_out) = run(&["spec", "next"], &change_dir);
    assert!(next_out.contains("ready to archive"), "{next_out}");
}

#[test]
fn status_missing_change_dir_errors() {
    let out = Command::new(bin())
        .args(["spec", "status", "/no/such/change"])
        .output()
        .expect("run rusty-idd");
    assert_eq!(out.status.code(), Some(1));
}
