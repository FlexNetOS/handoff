// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! handoff-core — shared continuity primitives extracted from the `hf` monolith.
//!
//! The first peeled-off crate of the 12-crate decomposition (ADR-0019 D5 #4, PRD §7.2). It holds
//! the leaf primitives every feature module shares: the `.handoff` control-plane location, the
//! ledger/task-dir path resolution, the wall-clock witness timestamp, status replay, and the
//! subprocess helper. Behavior-preserving move — `hf` re-exports these so existing `crate::…`
//! references are unchanged; future feature crates depend on `handoff-core` directly.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ledger::Ledger;
use work_order::{Status, WorkOrder};

/// The `.handoff` control-plane directory (repo-relative).
pub const HF: &str = ".handoff";

/// Wall-clock nanoseconds since the Unix epoch — the witness timestamp. Never panics (a clock
/// before the epoch yields 0).
pub fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// The task-card directory (`.handoff/tasks`).
pub fn tasks_dir() -> PathBuf {
    Path::new(HF).join("tasks")
}

/// The session capsule path (`.handoff/context/capsule.json`).
pub fn capsule_path() -> PathBuf {
    Path::new(HF).join("context").join("capsule.json")
}

/// Read one string field from the session capsule, or `None` if absent/unreadable/non-string.
/// HFTASK-0083: lifted from hf so the drift/gate crate can resolve the North-Star revision.
pub fn capsule_field(key: &str) -> Option<String> {
    let s = fs::read_to_string(capsule_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

/// HFTASK-0047: the current North-Star doctrine revision = blake3 of the capsule `northstar`.
/// An empty/absent capsule yields an empty revision (no northstar obligation is raised).
pub fn current_northstar_revision() -> String {
    work_order::northstar_revision(&capsule_field("northstar").unwrap_or_default())
}

/// HFTASK-0054: ledger location is overridable via the `HANDOFF_LEDGER` environment variable
/// (set by the `--ledger <path>` global flag). This lets a member repo render its Tier-A packet
/// against a shared ledger from its own CWD without a per-repo ledger.db. When unset, the default
/// is the local `<cwd>/.handoff/ledger.db`.
pub fn ledger_path() -> String {
    if let Ok(p) = std::env::var("HANDOFF_LEDGER")
        && !p.is_empty()
    {
        return p;
    }
    Path::new(HF)
        .join("ledger.db")
        .to_string_lossy()
        .into_owned()
}

/// Replay the latest witnessed status per task from the ledger. Fail-open WARN (never panic): a
/// replay failure on a present ledger logs a stale-status warning and falls back to card defaults.
pub fn current_statuses() -> Vec<(String, Status)> {
    match Ledger::open(&ledger_path()).and_then(|l| l.replay_latest_status()) {
        Ok(v) => v,
        Err(e) => {
            if Path::new(&ledger_path()).exists() {
                eprintln!(
                    "hf: WARNING — ledger present at {} but replay failed ({e}); statuses fall back to card defaults and may be stale (run `hf doctor`)",
                    ledger_path()
                );
            }
            Vec::new()
        }
    }
}

/// The replayed status for `id`, falling back to the card's own `status` when the ledger has no
/// transition for it.
pub fn status_of(id: &str, replay: &[(String, Status)], card: &WorkOrder) -> Status {
    replay
        .iter()
        .find(|(k, _)| k == id)
        .map(|(_, s)| *s)
        .unwrap_or(card.status)
}

/// Run a subprocess and capture trimmed stdout; `Err` on a non-zero exit (with stderr) or a spawn
/// failure. The shared shell-out used by the git/gh/cargo-driving feature modules.
pub fn run_out(bin: &str, args: &[&str]) -> Result<String, String> {
    match std::process::Command::new(bin).args(args).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        Ok(o) => Err(format!(
            "{bin} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("{bin} not runnable: {e}")),
    }
}

/// HFTASK-0080: witness a PRIMARY lifecycle transition FAIL-CLOSED. For these events the witnessed
/// record IS the operation (claim/checkpoint/done/test_result/ship/verdict/reopen/gatekeeper), so a
/// failed append/transition must abort loudly with a clean message — never panic on a bare
/// `.unwrap()`, and never proceed as if it had succeeded (that is the FAIL-OPEN class, LESSONS
/// L7–L10). `unwrap_or_else` with a diverging arm keeps the value on success and exits on failure.
///
/// HFTASK-0083 (ADR-0019 D5 #4): lifted from `hf` so the peeled feature crates (gatekeeper, fleet,
/// …) can witness lifecycle events without depending back on the `hf` binary crate.
pub fn must_witness<T>(r: ledger::Result<T>, what: &str) -> T {
    r.unwrap_or_else(|e| {
        eprintln!(
            "hf: FATAL — could not witness {what} ({e}); continuity event NOT recorded, aborting (fail-closed)"
        );
        std::process::exit(1);
    })
}

/// HFTASK-0080: pretty-print a value as JSON for human/CLI output. INFALLIBLE for the kernel's own
/// `#[derive(Serialize)]` view structs (owned fields, string map keys, no failing custom
/// serializer), so the single justified `expect` here replaces the bare `.unwrap()` call sites.
///
/// HFTASK-0083 (ADR-0019 D5 #4): lifted from `hf` so the peeled feature crates render JSON output
/// through one shared, justified helper.
pub fn pretty_json<T: serde::Serialize>(v: &T) -> String {
    #[allow(clippy::expect_used)]
    {
        serde_json::to_string_pretty(v).expect("serialize JSON view for CLI output")
    }
}

// --- card load/save (HFTASK-0083: lifted from hf so feature crates load/save cards through the
// SAME loud, schema-validated path; the FAIL-OPEN bug that hid card #95 stays fixed everywhere). ---

/// Parse + schema-validate one card file, returning a reason string on any failure.
pub fn try_parse_card(p: &Path) -> Result<WorkOrder, String> {
    let s = fs::read_to_string(p).map_err(|e| format!("unreadable: {e}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&s).map_err(|e| format!("invalid JSON: {e}"))?;
    handoff_schema::validate_card(&value).map_err(|v| format!("schema violation [{v}]"))?;
    serde_json::from_value::<WorkOrder>(value).map_err(|e| format!("deserialize: {e}"))
}

/// Load a card LOUDLY: on any failure emit a fail-closed WARNING (the card is never silently
/// dropped — the bug that hid card #95 for a whole session) and return None. `pub` so the fleet
/// member-card loader reuses the SAME loud, schema-validated path (fail-open-audit R1).
pub fn parse_card_file(p: &Path) -> Option<WorkOrder> {
    match try_parse_card(p) {
        Ok(wo) => Some(wo),
        Err(reason) => {
            eprintln!(
                "hf: WARNING — card {} failed to load: {reason} (NOT in status; fix or remove it)",
                p.display()
            );
            None
        }
    }
}

/// HFTASK-0064: enumerate every card file on disk and return the non-conforming ones with a
/// reason, QUIETLY (no eprintln — `hf doctor` formats the report). A non-empty result is a
/// fail-closed health violation: a card that can't load is invisible to `hf status` and must
/// surface as a hard failure, never hide.
pub fn scan_card_conformance() -> Vec<(String, String)> {
    let mut bad = vec![];
    let Ok(rd) = fs::read_dir(tasks_dir()) else {
        return bad;
    };
    let mut paths: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    for p in paths {
        if let Err(reason) = try_parse_card(&p) {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            bad.push((name, reason));
        }
    }
    bad
}

/// Load every conforming card from the default tasks dir (`.handoff/tasks`).
pub fn load_tasks() -> Vec<WorkOrder> {
    let mut v = vec![];
    if let Ok(rd) = fs::read_dir(tasks_dir()) {
        let mut paths: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        for p in paths {
            if let Some(wo) = parse_card_file(&p) {
                v.push(wo);
            }
        }
    }
    v
}

/// Save a card into the default tasks dir.
pub fn save_task(wo: &WorkOrder) {
    save_task_in(&tasks_dir(), wo);
}

/// Save a card into an explicit tasks dir (routing-aware: a per-task op writes the card to the
/// same home as the ledger it appends to — ADR-0004 §3). Creates the dir.
pub fn save_task_in(tasks_dir: &Path, wo: &WorkOrder) {
    let _ = fs::create_dir_all(tasks_dir);
    let _ = fs::write(tasks_dir.join(format!("{}.task.json", wo.id)), wo.to_json());
}

/// Load the card for one id from an explicit tasks dir (routing-aware read for the resolved home).
/// An ABSENT file is a silent None ("no such card here"); a PRESENT-but-invalid file is a loud
/// WARNING (fail-closed) so a broken card never silently disappears from a per-task lookup.
pub fn load_task_in(tasks_dir: &Path, id: &str) -> Option<WorkOrder> {
    let p = tasks_dir.join(format!("{id}.task.json"));
    if !p.exists() {
        return None;
    }
    parse_card_file(&p)
}

/// Next safe task: resume the in-progress task first (Claimed/Checkpointed/Active/Review); else the
/// first backlog card whose dependencies are all Done.
pub fn next_safe<'a>(tasks: &'a [WorkOrder], replay: &[(String, Status)]) -> Option<&'a WorkOrder> {
    let done = |id: &str| replay.iter().any(|(k, s)| k == id && *s == Status::Done);
    if let Some(t) = tasks.iter().find(|t| {
        matches!(
            status_of(&t.id, replay, t),
            Status::Claimed | Status::Checkpointed | Status::Active | Status::Review
        )
    }) {
        return Some(t);
    }
    tasks.iter().find(|t| {
        status_of(&t.id, replay, t) == Status::Backlog && t.dependencies.iter().all(|d| done(d))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_dir_under_hf() {
        assert!(tasks_dir().ends_with("tasks"));
        assert!(tasks_dir().starts_with(HF));
    }

    #[test]
    fn now_ns_is_monotonicish_and_nonzero() {
        assert!(now_ns() > 0);
    }

    fn minimal_card(id: &str, status: Status) -> WorkOrder {
        WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: "t".into(),
            status,
            priority: work_order::Priority::P1,
            objective: "o".into(),
            path_scope: vec![],
            acceptance_criteria: vec![],
            test_commands: vec![],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: String::new(),
            role: None,
            intent_lock: WorkOrder::compute_intent_lock("o", &[], &[]),
        }
    }

    #[test]
    fn status_of_falls_back_to_card() {
        let card = minimal_card("T1", Status::Backlog);
        // empty replay → card default
        assert_eq!(status_of("T1", &[], &card), Status::Backlog);
        // replay overrides
        let replay = vec![("T1".to_string(), Status::Done)];
        assert_eq!(status_of("T1", &replay, &card), Status::Done);
    }

    #[test]
    fn run_out_captures_stdout_and_errs_nonzero() {
        assert_eq!(run_out("true", &[]).ok(), Some(String::new()));
        assert!(run_out("false", &[]).is_err());
        assert!(run_out("definitely-not-a-binary-xyz", &[]).is_err());
    }

    #[test]
    fn try_parse_card_accepts_valid_and_names_violations() {
        // HFTASK-0064 (lifted from hf in HFTASK-0083): the doctor card-conformance core — a valid
        // card loads; every failure mode returns a concise reason (never a silent None) so
        // `hf doctor` can fail closed.
        let dir = std::env::temp_dir().join(format!("hf-card-{}-{}", std::process::id(), now_ns()));
        std::fs::create_dir_all(&dir).unwrap();
        let valid = r#"{"schema":"handoff.task.v1","id":"HFTASK-9001","title":"t","status":"backlog","priority":"P2","objective":"o","path_scope":[],"acceptance_criteria":[],"test_commands":[],"correlation_id":"c","intent_lock":{"objective_hash":"a","path_scope_hash":"b","acceptance_hash":"c"}}"#;
        let ok = dir.join("ok.json");
        std::fs::write(&ok, valid).unwrap();
        assert!(try_parse_card(&ok).is_ok(), "a complete card must load");

        // missing intent_lock → schema violation naming the field (the card #95 bug).
        let bad = valid.replace(
            r#","intent_lock":{"objective_hash":"a","path_scope_hash":"b","acceptance_hash":"c"}"#,
            "",
        );
        let bp = dir.join("missing_lock.json");
        std::fs::write(&bp, &bad).unwrap();
        let e = try_parse_card(&bp).unwrap_err();
        assert!(e.contains("intent_lock"), "reason must name the field: {e}");

        // invalid JSON → distinct reason.
        let gp = dir.join("garbage.json");
        std::fs::write(&gp, "{not json").unwrap();
        assert!(try_parse_card(&gp).unwrap_err().contains("invalid JSON"));

        // free-form id → schema violation.
        let badid = valid.replace("HFTASK-9001", "nope");
        let ip = dir.join("bad_id.json");
        std::fs::write(&ip, badid).unwrap();
        assert!(try_parse_card(&ip).unwrap_err().contains("schema"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
