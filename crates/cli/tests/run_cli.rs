//! Smoke test for `rusty-idd run`: a change whose tasks are all already checked
//! must finish cleanly without spawning any agent process.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_rusty-idd")
}

#[test]
fn run_finishes_when_all_tasks_checked() {
    let root = tempfile::tempdir().unwrap();
    let change = root.path().join("openspec/changes/done-change");
    std::fs::create_dir_all(&change).unwrap();
    // All tasks checked -> completed >= total -> the worker reports Finished
    // without ever invoking the configured command.
    std::fs::write(
        change.join("tasks.md"),
        "# Tasks\n\n- [x] First task\n- [x] Second task\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["run", "done-change"])
        .current_dir(root.path())
        .output()
        .expect("run rusty-idd");

    assert!(
        out.status.success(),
        "run should finish successfully (exit 0): stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("completed successfully"),
        "should report success: {stdout}"
    );
}
