//! Ledger/tasks routing (ADR-0004 §3 two-ledger residency).
//!
//! `hf` historically resolved `.handoff/ledger.db` + `.handoff/tasks/` CWD-relative
//! with no anchoring. Running a per-task op from `meta/handoff/` therefore wrote
//! *any* task — including kb-minted envctl-domain KBTASK cards — into handoff's own
//! KERNEL ledger, contaminating it and violating the settled two-ledger model.
//!
//! Residency (ADR-0004 §3):
//! - **KERNEL ledger** = the LOCAL repo's `<cwd>/.handoff/ledger.db` + `tasks/`.
//!   This is handoff's own HFTASK self-dev when `hf` is run from `handoff/`.
//! - **FLEET ledger**  = `<meta-root>/.handoff/ledger.db` + `tasks/`, located by
//!   walking up to the dir holding `.meta.yaml` (`fleet::find_meta_root`).
//!
//! A per-task op resolves *which* home a task lives in by where its card file is,
//! then opens THAT ledger and saves the card THERE. If the task is resident in
//! neither home, the op **fails closed** — it must NOT conjure a ledger into
//! existence (the contamination mechanism).

use std::path::{Path, PathBuf};

use crate::fleet;
use crate::HF;

/// `(ledger_db, tasks_dir)` for a given `.handoff` base directory.
fn homes_for(base: &Path) -> (PathBuf, PathBuf) {
    let hf = base.join(HF);
    (hf.join("ledger.db"), hf.join("tasks"))
}

/// The LOCAL (cwd) home: `<cwd>/.handoff/ledger.db` + `<cwd>/.handoff/tasks`.
/// This is the KERNEL ledger when `hf` runs from `handoff/`, and the only home for
/// a standalone repo (no meta root). Mirrors the historical CWD-relative behavior.
pub fn local_home() -> (PathBuf, PathBuf) {
    homes_for(Path::new("."))
}

/// The FLEET home: `<meta-root>/.handoff/ledger.db` + `<meta-root>/.handoff/tasks`,
/// or `None` when no `.meta.yaml` is found from the cwd upward (standalone checkout).
pub fn fleet_home() -> Option<(PathBuf, PathBuf)> {
    fleet::find_meta_root().map(|root| homes_for(&root))
}

/// True if a card `<id>.task.json` exists in `tasks_dir`.
fn task_card_exists(tasks_dir: &Path, id: &str) -> bool {
    tasks_dir.join(format!("{id}.task.json")).is_file()
}

/// Resolve the `(ledger, tasks)` home a per-task op must use for `id`, fail-closed.
///
/// Resolution order (ADR-0004 §3):
///   a. LOCAL card present  → `local_home`  (a kernel HFTASK run from `handoff/`).
///   b. else FLEET card present (and a meta root exists) → `fleet_home`.
///   c. else → `Err(..)`. The caller MUST NOT open/create a ledger or write an
///      event on `Err`; it prints the message and exits nonzero. Fabricating a
///      home here is exactly the contamination bug this routing prevents.
pub fn route_for_task(id: &str) -> Result<(PathBuf, PathBuf), String> {
    let (local_db, local_tasks) = local_home();
    if task_card_exists(&local_tasks, id) {
        return Ok((local_db, local_tasks));
    }
    if let Some((fleet_db, fleet_tasks)) = fleet_home() {
        if task_card_exists(&fleet_tasks, id) {
            return Ok((fleet_db, fleet_tasks));
        }
    }
    Err(format!(
        "hf: task {id} not found in this repo's .handoff/tasks or the FLEET ledger (meta/.handoff) — mint it first"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fixture meta workspace: `<tmp>/.meta.yaml` + `<tmp>/.handoff/tasks/` (FLEET)
    /// and a member `<tmp>/handoff/.handoff/tasks/` (the KERNEL home / cwd). Returns
    /// the member dir (used as the process cwd) and the meta root.
    struct Fixture {
        _tmp: PathBuf,
        member: PathBuf,
        root: PathBuf,
    }

    fn write_card(tasks_dir: &Path, id: &str) {
        fs::create_dir_all(tasks_dir).unwrap();
        // Minimal card file — routing only checks for the file's existence by id.
        fs::write(tasks_dir.join(format!("{id}.task.json")), "{}").unwrap();
    }

    fn make_fixture(tag: &str) -> Fixture {
        let mut tmp = std::env::temp_dir();
        tmp.push(format!(
            "hf-route-{}-{}-{}",
            tag,
            std::process::id(),
            crate::now_ns()
        ));
        let root = tmp.clone();
        fs::create_dir_all(&root).unwrap();
        // Canonicalize so the fixture root matches what `find_meta_root()` returns:
        // it walks up from `current_dir()`, which macOS resolves through symlinks
        // (`/var` -> `/private/var`), so a raw temp_dir() path would mismatch.
        let root = root.canonicalize().unwrap();
        fs::write(
            root.join(".meta.yaml"),
            "projects:\n  handoff:\n    repo: git@example/handoff.git\n",
        )
        .unwrap();
        // FLEET home tasks dir.
        fs::create_dir_all(root.join(".handoff").join("tasks")).unwrap();
        // Member (KERNEL home) = the cwd we route from.
        let member = root.join("handoff");
        fs::create_dir_all(member.join(".handoff").join("tasks")).unwrap();
        Fixture {
            _tmp: tmp,
            member,
            root,
        }
    }

    use crate::test_support::cwd_lock;

    #[test]
    fn local_card_routes_to_kernel_home() {
        let _g = cwd_lock();
        let fx = make_fixture("local");
        write_card(&fx.member.join(".handoff").join("tasks"), "HFTASK-9001");
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&fx.member).unwrap();
        let res = route_for_task("HFTASK-9001");
        std::env::set_current_dir(&prev).unwrap();

        let (db, tasks) = res.expect("local card must route");
        // KERNEL home = LOCAL (cwd-relative), resolving under the member when cwd is
        // the member — NOT the meta root's .handoff. `local_home` is intentionally
        // cwd-relative, so compare via canonicalization against the absolute member.
        let member_db = fx.member.join(".handoff").join("ledger.db");
        // db is "./.handoff/ledger.db" relative to the (member) cwd.
        assert_eq!(db, super::local_home().0);
        assert_eq!(tasks, super::local_home().1);
        // It must NOT be the FLEET (meta-root) home.
        assert_ne!(db, fx.root.join(".handoff").join("ledger.db"));
        // And resolved against the member cwd it equals the member's kernel db.
        assert_eq!(fx.member.join(db.strip_prefix(".").unwrap()), member_db);
    }

    #[test]
    fn fleet_card_routes_to_fleet_home_when_no_local_card() {
        let _g = cwd_lock();
        let fx = make_fixture("fleet");
        write_card(&fx.root.join(".handoff").join("tasks"), "KBTASK-DEMO");
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&fx.member).unwrap();
        let res = route_for_task("KBTASK-DEMO");
        std::env::set_current_dir(&prev).unwrap();

        let (db, tasks) = res.expect("fleet card must route");
        // FLEET home = the meta root's .handoff, NOT the member's.
        assert_eq!(db, fx.root.join(".handoff").join("ledger.db"));
        assert_eq!(tasks, fx.root.join(".handoff").join("tasks"));
        assert!(!db.starts_with(fx.member.join(".handoff")));
    }

    #[test]
    fn local_card_wins_over_fleet_card_of_same_id() {
        let _g = cwd_lock();
        let fx = make_fixture("both");
        write_card(&fx.member.join(".handoff").join("tasks"), "DUP-1");
        write_card(&fx.root.join(".handoff").join("tasks"), "DUP-1");
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&fx.member).unwrap();
        let res = route_for_task("DUP-1");
        std::env::set_current_dir(&prev).unwrap();

        let (db, _tasks) = res.expect("must route");
        // LOCAL (cwd-relative) home wins; it must NOT be the FLEET (meta-root) home.
        assert_eq!(db, super::local_home().0);
        assert_ne!(db, fx.root.join(".handoff").join("ledger.db"));
    }

    #[test]
    fn unknown_task_fails_closed() {
        let _g = cwd_lock();
        let fx = make_fixture("unknown");
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&fx.member).unwrap();
        let res = route_for_task("NOPE-404");
        std::env::set_current_dir(&prev).unwrap();

        let err = res.expect_err("unknown id must fail closed");
        assert!(err.contains("NOPE-404"));
        assert!(err.contains("mint it first"));
        // Fail-closed contract: NO ledger.db was created in either home.
        assert!(!fx.member.join(".handoff").join("ledger.db").exists());
        assert!(!fx.root.join(".handoff").join("ledger.db").exists());
    }
}
