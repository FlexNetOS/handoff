// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `hf fleet status` — fleet aggregation (ADR-0004 §4).
//!
//! HFTASK-0083 (ADR-0019 D5 #4): peeled into the `handoff-fleet` crate after the card loader +
//! PrioStr moved out of hf. `hf` aliases it as `fleet` so `fleet::cmd_fleet_status` /
//! `fleet::find_meta_root` / `fleet::parse_members` / `fleet::render_member_packet` stay valid.
//! Deps: handoff-core + ledger + work-order + serde_json.
//!
//! Enumerate members from the meta root's `.meta.yaml`, read each repo's git-text
//! `.handoff` (capsule + cards), and join with the FLEET ledger events into one board.
//! **Git is the sync transport** — no daemons. State precedence stays Git > ledger >
//! cards.
//!
//! Residency (ADR-0004 §3.3/§6, REVISED 2026-06-13; ADR-0018 D1 / HFTASK-0067): continuity is
//! per-repo-first with a central rollup. The committed continuity truth is now the deterministic
//! **`.handoff/ledger.events.jsonl`** text export (ADR-0018 D1); the binary `.handoff/ledger.db`
//! stays a **gitignored local rebuild cache** (re-derived via `hf import`). The P7 violations
//! `hf fleet status` surfaces are:
//!   (a) **NEW (ADR-0018 D1):** a member with a local ledger on disk whose committed
//!       `.handoff/ledger.events.jsonl` is **NOT git-tracked** — the durable truth is missing
//!       (run `hf export` + commit). This is the *inversion*: committed ledger continuity is now
//!       REQUIRED where the binary form was banned.
//!   (b) a git-**TRACKED** binary `.db` under `.handoff` — the binary stays a cache, so committing
//!       it (instead of the JSONL text) is still banned (HFTASK-0034).
//!   (c) a continuity member missing the `.handoff/**/ledger.db` `.gitignore` guard — the binary
//!       cache could be committed (HFTASK-0034/0035).
//! A binary `.db` merely present on disk (gitignored) is LEGITIMATE.

use ledger::{Ledger, RollupProvenance};
use std::path::{Path, PathBuf};
use std::process::Command;
use work_order::PrioStr;
use work_order::{Status, WorkOrder};

/// Walk up from the current directory to the meta root (the dir holding `.meta.yaml`).
pub fn find_meta_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".meta.yaml").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Member names from the `projects:` block of `.meta.yaml`. Dependency-free: `hf`
/// carries no YAML crate (and the pure-Rust/no-C trust-boundary gate discourages
/// adding one for this), and we only need the member directory names. The format is
/// controlled — members are 2-space-indented bare `name:` keys under `projects:`.
pub fn parse_members(meta_yaml: &str) -> Vec<String> {
    let mut out = vec![];
    let mut in_projects = false;
    for line in meta_yaml.lines() {
        let body = line.trim_start();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        let indent = line.len() - body.len();
        if indent == 0 {
            in_projects = body.starts_with("projects:");
            continue;
        }
        // A member is a 2-space-indented key with no inline value: `name:`.
        if in_projects
            && indent == 2
            && let Some(name) = body.strip_suffix(':')
            && !name.is_empty()
            && !name.contains(char::is_whitespace)
        {
            out.push(name.to_string());
        }
    }
    out
}

fn capsule_field(repo: &Path, key: &str) -> Option<String> {
    let s = std::fs::read_to_string(repo.join(".handoff/context/capsule.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

fn count_cards(repo: &Path) -> usize {
    std::fs::read_dir(repo.join(".handoff/tasks"))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .count()
        })
        .unwrap_or(0)
}

struct Row {
    name: String,
    present: bool,
    has_handoff: bool,
    /// HFTASK-0088: a present `.meta.yaml` member with no `.handoff` is not healthy;
    /// it needs first-time onboarding via portable `hf init` + the normal deploy bits.
    onboarding_missing: bool,
    cards: usize,
    project_name: Option<String>,
    role: Option<String>,
    plane: Option<String>,
    /// ADR-0018 D1 (HFTASK-0067): a member with a local ledger on disk whose committed
    /// `.handoff/ledger.events.jsonl` text export is NOT git-tracked — the durable continuity
    /// truth is missing (the new primary P7 gate; run `hf export` + commit it).
    jsonl_export_missing: bool,
    /// HFTASK-0034 (ADR-0004 §6 rev): a git-TRACKED ledger DB under `.handoff` — the banned
    /// committed-binary-ledger violation (NOT "a ledger present on disk"). The binary stays a
    /// gitignored cache; the JSONL text is the committed form (ADR-0018 D1).
    tracked_ledger: bool,
    /// HFTASK-0034: a continuity member (has `.handoff`) whose `.gitignore` lacks the
    /// `.handoff/**/ledger.db` residency guard — its local ledger could be committed.
    ledger_guard_missing: bool,
    /// HFTASK-0035 upgrade: a continuity member (has `.handoff`) whose `.gitignore` lacks the
    /// `.handoff/**/*.db-wal` / `.handoff/**/*.db-shm` side-car guard.
    walshm_guard_missing: bool,
    /// HFTASK-0091: a member whose local `.handoff/ledger.db` is the retired C-SQLite
    /// format. This is a typed remediation condition: default no-C `hf` must never attempt
    /// to open it as redb (or treat the failure as empty).
    legacy_sqlite_ledger: bool,
    /// HFTASK-0033: this member's own per-repo ledger chain, verified independently of the
    /// central rollup. `Some((events, witnessed))` when `<member>/.handoff/ledger.db` exists
    /// and its witness chain was checked; `None` when the member carries no local ledger.
    /// (The rollup model — ADR-0004 §3.3 rev — keeps each member's gitignored local ledger
    /// as the *source* the central FLEET ledger rolls up; this proves that source stands
    /// alone.)
    per_repo_chain: Option<PerRepoChain>,
}

/// HFTASK-0033: a member's independently-verified per-repo ledger chain.
struct PerRepoChain {
    events: usize,
    witnessed: usize,
}

/// HFTASK-0033: open `<repo>/.handoff/ledger.db` (if present) and verify its witness chain
/// standalone — proving the per-repo (source) chain is intact independent of the central
/// rollup. `None` when the member has no local ledger (git-text-only / not yet seeded).
fn per_repo_chain_stats(repo: &Path) -> Option<PerRepoChain> {
    let p = repo.join(".handoff").join("ledger.db");
    if !p.is_file() {
        return None;
    }
    let lp = p.to_string_lossy().into_owned();
    let events = Ledger::open(&lp)
        .and_then(|l| l.all_events())
        .map(|e| e.len())
        .unwrap_or(0);
    let witnessed = Ledger::open(&lp)
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);
    Some(PerRepoChain { events, witnessed })
}

/// HFTASK-0034 (ADR-0004 §6 rev): a *git-tracked* ledger DB under `.handoff` is the P7
/// violation — committing binary ledger state is BANNED (merge conflicts, bloat, the beads
/// lesson). A `.db` merely *present on disk* (gitignored) is LEGITIMATE — it's the repo's
/// local source of record (the rollup model). So we ask Git, not the filesystem: does any
/// `*.db` / `*.db-wal` / `*.db-shm` under `.handoff` appear in `git ls-files`?
fn git_tracks_handoff_db(repo: &Path) -> bool {
    Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "ls-files", "--", ".handoff"])
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).lines().any(|l| {
                let l = l.trim();
                l.ends_with(".db") || l.ends_with(".db-wal") || l.ends_with(".db-shm")
            })
        })
        .unwrap_or(false)
}

/// ADR-0018 D1 (HFTASK-0067): does this repo git-track its `.handoff/ledger.events.jsonl` text
/// export — the committed continuity truth? Asks Git (`ls-files`), not the filesystem.
fn git_tracks_jsonl_export(repo: &Path) -> bool {
    Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "ls-files",
            "--",
            ".handoff/ledger.events.jsonl",
        ])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// ADR-0018 D1: is there a local binary ledger on disk for this repo? A repo that carries a
/// local ledger MUST commit its `.handoff/ledger.events.jsonl` export (the durable text truth).
fn local_ledger_on_disk(repo: &Path) -> bool {
    repo.join(".handoff").join("ledger.db").is_file()
}

fn legacy_sqlite_ledger(repo: &Path) -> bool {
    let p = repo.join(".handoff").join("ledger.db");
    p.to_str()
        .map(ledger::file_is_legacy_sqlite)
        .unwrap_or(false)
}

/// HFTASK-0034 (ADR-0004 §6 rev): the `.gitignore` residency guard must exist so a local
/// ledger can never be committed. `git check-ignore -q .handoff/ledger.db` exits 0 iff the
/// path is ignored — true for both `/.handoff/ledger.db` and `.handoff/**/ledger.db`
/// (gitignore `**` matches zero segments, so it covers the top-level path too). Returns
/// false when the repo isn't a git repo or the guard is absent.
fn ledger_guard_present(repo: &Path) -> bool {
    Command::new("git")
        .args([
            "-C",
            &repo.to_string_lossy(),
            "check-ignore",
            "-q",
            ".handoff/ledger.db",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// HFTASK-0035 upgrade (ADR-0004 §3.3/§6 rev): the ledger side-car files (`*.db-wal`,
/// `*.db-shm`) must also be gitignored. `git check-ignore -q .handoff/ledger.db-wal`
/// (and `-shm`) is true when the standard `.handoff/**/*.db-wal` / `.handoff/**/*.db-shm`
/// patterns are present.
fn walshm_guard_present(repo: &Path) -> bool {
    [".handoff/ledger.db-wal", ".handoff/ledger.db-shm"]
        .iter()
        .all(|path| {
            Command::new("git")
                .args(["-C", &repo.to_string_lossy(), "check-ignore", "-q", path])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
}

fn collect_rows(root: &Path, members: &[String]) -> Vec<Row> {
    members
        .iter()
        .map(|name| {
            let repo = root.join(name);
            let present = repo.is_dir();
            let has_handoff = repo.join(".handoff").is_dir();
            let onboarding_missing = present && !has_handoff;
            // HFTASK-0034: ask Git, not the filesystem. A tracked .db is the violation; a
            // present-but-gitignored .db is legitimate. The guard is required for any repo
            // that carries a .handoff continuity layer.
            let tracked_ledger = present && git_tracks_handoff_db(&repo);
            // ADR-0018 D1: a repo with a local ledger must commit its JSONL text export.
            let jsonl_export_missing = present
                && has_handoff
                && local_ledger_on_disk(&repo)
                && !git_tracks_jsonl_export(&repo);
            let ledger_guard_missing = present && has_handoff && !ledger_guard_present(&repo);
            let walshm_guard_missing = present
                && has_handoff
                && ledger_guard_present(&repo)
                && !walshm_guard_present(&repo);
            let legacy_sqlite_ledger = present && has_handoff && legacy_sqlite_ledger(&repo);
            Row {
                name: name.clone(),
                present,
                has_handoff,
                onboarding_missing,
                cards: count_cards(&repo),
                project_name: capsule_field(&repo, "project_name"),
                role: capsule_field(&repo, "role"),
                plane: capsule_field(&repo, "plane"),
                jsonl_export_missing,
                tracked_ledger,
                ledger_guard_missing,
                walshm_guard_missing,
                legacy_sqlite_ledger,
                per_repo_chain: if legacy_sqlite_ledger {
                    None
                } else {
                    per_repo_chain_stats(&repo)
                },
            }
        })
        .collect()
}

fn migration_command(root: &Path, member: &str) -> String {
    let ledger = root.join(member).join(".handoff").join("ledger.db");
    format!(
        "cd {} && cargo run -p hf --features legacy-sqlite -- migrate {}",
        root.join("handoff").display(),
        ledger.display()
    )
}

fn migration_plan_json(root: &Path, member: &str) -> serde_json::Value {
    let ledger = root.join(member).join(".handoff").join("ledger.db");
    serde_json::json!({
        "member": member,
        "ledger_path": ledger.to_string_lossy(),
        "command": migration_command(root, member),
        "backup": "out-of-tree via HANDOFF_LEDGER_BACKUP_DIR, XDG_DATA_HOME, or ~/.local/share/handoff-ledger-backups",
        "requires_feature": "legacy-sqlite",
    })
}

/// FLEET ledger event count + witness-chain verification (0/0 if absent).
fn fleet_ledger_stats(root: &Path) -> (usize, usize, bool) {
    let p = root.join(".handoff").join("ledger.db");
    if !p.is_file() {
        return (0, 0, false);
    }
    let lp = p.to_string_lossy().into_owned();
    let events = Ledger::open(&lp)
        .and_then(|l| l.all_events())
        .map(|e| e.len())
        .unwrap_or(0);
    let witness = Ledger::open(&lp)
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);
    (events, witness, true)
}

/// HFTASK-0033: verify the FLEET ledger's rollup provenance bridge — every rolled-up
/// central row reproduces its stored `origin_action_hash`. `None` when the central ledger
/// is absent (nothing to verify).
fn fleet_provenance(root: &Path) -> Option<RollupProvenance> {
    let p = root.join(".handoff").join("ledger.db");
    if !p.is_file() {
        return None;
    }
    let lp = p.to_string_lossy().into_owned();
    Ledger::open(&lp)
        .and_then(|l| l.verify_rollup_provenance())
        .ok()
}

pub fn cmd_fleet_status(json: bool) {
    let Some(root) = find_meta_root() else {
        eprintln!("hf fleet status: no .meta.yaml found from the current directory upward");
        std::process::exit(1);
    };
    let meta_yaml = std::fs::read_to_string(root.join(".meta.yaml")).unwrap_or_default();
    let members = parse_members(&meta_yaml);
    let rows = collect_rows(&root, &members);
    let (events, witness, ledger_present) = fleet_ledger_stats(&root);
    // HFTASK-0033: (iii) provenance faithfulness over the central ledger, and (ii) the count
    // of members whose own per-repo chain we verified independently.
    let provenance = fleet_provenance(&root);
    let per_repo_verified = rows.iter().filter(|r| r.per_repo_chain.is_some()).count();

    let with_handoff = rows.iter().filter(|r| r.has_handoff).count();
    // HFTASK-0034 (ADR-0004 §6 rev): two distinct P7 conditions.
    let mut warnings: Vec<String> = Vec::new();
    // ADR-0018 D1 (HFTASK-0067): the NEW primary gate — a repo with a local ledger must commit its
    // `.handoff/ledger.events.jsonl` text export (the durable continuity truth).
    for r in rows.iter().filter(|r| r.onboarding_missing) {
        warnings.push(format!(
            "{}: present in `.meta.yaml` but missing `.handoff` — new repo needs continuity onboarding (HFTASK-0088; run `hf fleet sync`)",
            r.name
        ));
    }
    for r in rows.iter().filter(|r| r.jsonl_export_missing) {
        warnings.push(format!(
            "{}: has a local ledger but its `.handoff/ledger.events.jsonl` text export is NOT git-tracked — the committed continuity truth is missing (ADR-0018 D1; run `hf export` and commit it)",
            r.name
        ));
    }
    for r in rows.iter().filter(|r| r.tracked_ledger) {
        warnings.push(format!(
            "{}: a ledger DB under .handoff is git-TRACKED — policy-P7 violation (ADR-0004 §6); committed binary ledger state is banned (gitignore it)",
            r.name
        ));
    }
    for r in rows.iter().filter(|r| r.ledger_guard_missing) {
        warnings.push(format!(
            "{}: missing the `.handoff/**/ledger.db` .gitignore guard (ADR-0004 §6); a local ledger could be committed",
            r.name
        ));
    }
    for r in rows.iter().filter(|r| r.walshm_guard_missing) {
        warnings.push(format!(
            "{}: missing the `.handoff/**/*.db-wal` / `.handoff/**/*.db-shm` .gitignore guard (ADR-0004 §6; HFTASK-0035); WAL/SHM sidecars could be committed",
            r.name
        ));
    }
    for r in rows.iter().filter(|r| r.legacy_sqlite_ledger) {
        warnings.push(format!(
            "{}: legacy C-SQLite `.handoff/ledger.db` blocks redb rollup — run `hf fleet sync` or migration command: {}",
            r.name,
            migration_command(&root, &r.name)
        ));
    }
    // HFTASK-0033: a broken provenance bridge is an integrity alarm, not a style nit —
    // surface it as a warning so the loop's drift/gate sees it.
    if let Some(p) = &provenance
        && !p.is_faithful()
    {
        warnings.push(format!(
                "FLEET ledger: rollup provenance BROKEN — {} of {} rolled-up row(s) do not reproduce their origin_action_hash (ADR-0004 §3.3)",
                p.mismatched,
                p.total()
            ));
    }

    if json {
        let out = serde_json::json!({
            "schema": "handoff.fleet_status.v1",
            "meta_root": root.to_string_lossy(),
            "fleet_ledger": {
                "path": root.join(".handoff").join("ledger.db").to_string_lossy(),
                "present": ledger_present,
                "events": events,
                "witnessed_verified": witness,
                // HFTASK-0033 (iii): the provenance bridge over rolled-up rows.
                "rollup_provenance": provenance.as_ref().map(|p| serde_json::json!({
                    "faithful": p.is_faithful(),
                    "verified": p.verified,
                    "mismatched": p.mismatched,
                    "rolled_up_total": p.total(),
                    "per_repo": p.per_repo.iter()
                        .map(|(repo, n)| serde_json::json!({ "origin_repo": repo, "verified": n }))
                        .collect::<Vec<_>>(),
                })),
            },
            // HFTASK-0033 (ii): how many members' own per-repo chains verified independently.
            "per_repo_chains_verified": per_repo_verified,
            "members_total": rows.len(),
            "members_with_handoff": with_handoff,
            "members": rows.iter().map(|r| serde_json::json!({
                "name": r.name,
                "present": r.present,
                "has_handoff": r.has_handoff,
                "onboarding_missing": r.onboarding_missing,
                "cards": r.cards,
                "project_name": r.project_name,
                "role": r.role,
                "plane": r.plane,
                "jsonl_export_missing": r.jsonl_export_missing,
                "tracked_ledger": r.tracked_ledger,
                "ledger_guard_missing": r.ledger_guard_missing,
                "walshm_guard_missing": r.walshm_guard_missing,
                "legacy_sqlite_ledger": r.legacy_sqlite_ledger,
                "migration_plan": r.legacy_sqlite_ledger.then(|| migration_plan_json(&root, &r.name)),
                // HFTASK-0033 (ii): this member's own ledger chain, verified standalone.
                "per_repo_chain": r.per_repo_chain.as_ref().map(|c| serde_json::json!({
                    "events": c.events,
                    "witnessed_verified": c.witnessed,
                })),
            })).collect::<Vec<_>>(),
            "warnings": warnings,
        });
        println!("{}", handoff_core::pretty_json(&out));
        return;
    }

    println!(
        "=== hf fleet status ===  (meta root: {})",
        root.to_string_lossy()
    );
    println!(
        "FLEET ledger: {} ({} events · {} witnessed-verified)",
        if ledger_present { "present" } else { "ABSENT" },
        events,
        witness
    );
    // HFTASK-0033 (iii): provenance faithfulness of the central rollup.
    match &provenance {
        Some(p) if p.total() == 0 => {
            println!("  rollup provenance: n/a (no rolled-up rows)")
        }
        Some(p) => println!(
            "  rollup provenance: {} ({}/{} rolled-up rows trace to origin across {} repo(s))",
            if p.is_faithful() {
                "FAITHFUL ✓"
            } else {
                "BROKEN ✗"
            },
            p.verified,
            p.total(),
            p.per_repo.len()
        ),
        None => {}
    }
    println!(
        "members: {} total · {} with .handoff · {} per-repo chain(s) verified\n",
        rows.len(),
        with_handoff,
        per_repo_verified
    );
    println!(
        "  {:<26} {:<8} {:<6} capsule (role/plane)",
        "member", ".handoff", "cards"
    );
    for r in &rows {
        let hand = if !r.present {
            "MISSING"
        } else if r.has_handoff {
            "yes"
        } else {
            "—"
        };
        let cards = if r.has_handoff {
            r.cards.to_string()
        } else {
            "—".into()
        };
        let id = match (&r.role, &r.plane) {
            (Some(role), Some(plane)) => format!("{role}/{plane}"),
            (Some(role), None) => role.clone(),
            _ => r.project_name.clone().unwrap_or_default(),
        };
        // ADR-0018 D1 + HFTASK-0034/0035: flag a missing JSONL export (the new primary gate),
        // a git-TRACKED binary ledger, and/or missing binary-cache guards.
        let flag = match (
            r.onboarding_missing,
            r.jsonl_export_missing,
            r.tracked_ledger,
            r.ledger_guard_missing,
            r.walshm_guard_missing,
            r.legacy_sqlite_ledger,
        ) {
            (true, _, _, _, _, _) => "  ⚠ missing .handoff (onboard)",
            (false, _, _, _, _, true) => "  ⚠ legacy SQLite ledger (migration required)",
            (false, true, _, _, _, _) => "  ⚠ no committed ledger.events.jsonl (P7)",
            (false, false, true, _, _, _) => "  ⚠ tracked ledger.db (P7)",
            (false, false, false, true, _, _) => "  ⚠ no ledger .gitignore guard (P7)",
            (false, false, false, false, true, _) => "  ⚠ no WAL/SHM .gitignore guard (P7)",
            (false, false, false, false, false, false) => "",
        };
        // HFTASK-0033 (ii): this member's own per-repo chain, verified independently.
        let chain = match &r.per_repo_chain {
            Some(c) => format!("  · chain {}✓/{}ev", c.witnessed, c.events),
            None => String::new(),
        };
        println!(
            "  {:<26} {:<8} {:<6} {}{}{}",
            r.name, hand, cards, id, flag, chain
        );
    }
    if !warnings.is_empty() {
        println!("\nwarnings:");
        for w in &warnings {
            println!("  ⚠ {w}");
        }
    }
}

// ===========================================================================
// HFTASK-0087 (ADR-0018 D1 / automation rung 3): `hf fleet sync` — REMEDIATE the
// non-conformant members `hf fleet status` detects, instead of only REPORTING them.
//
// HFTASK-0088 extends the detection set to first-time onboarding: a present `.meta.yaml`
// member with no `.handoff` is a remediation target, not a clean/no-op member.
//
// For each member `collect_rows` flags (onboarding_missing / jsonl_export_missing / tracked_ledger /
// ledger_guard_missing / walshm_guard_missing), drive the idempotent loop-init deploy
// bits (`scripts/handoff-loop-init.sh <member-dir>` — ensure_ledger_guard, deploy_hooks,
// deploy_diff_drive, deploy_session_relay, deploy_rules, + the HFTASK-0085 staleness
// rebuild), then RE-evaluate that member's row and judge success by the AFTER flags —
// NEVER by the script's exit code. The loop-init script ends in an unconditional `exit 0`
// and a per-member failure does `FAIL+=1; continue` WITHOUT changing the exit code, so
// trusting it would be a FAIL-OPEN trap (LESSONS L7–L10). Fail-closed per member: one
// member's failure never aborts the sweep; the verb exits non-zero iff any flagged member
// is still non-conformant after remediation.
// ===========================================================================

/// A member needs remediation iff it lacks onboarding or any P7 conformance flag is set.
fn member_needs_sync(r: &Row) -> bool {
    r.onboarding_missing
        || r.jsonl_export_missing
        || r.tracked_ledger
        || r.ledger_guard_missing
        || r.walshm_guard_missing
        || r.legacy_sqlite_ledger
}

/// The onboarding/P7 conformance flags as a JSON object (shared by the before/after snapshots).
fn flags_json(r: &Row) -> serde_json::Value {
    serde_json::json!({
        "onboarding_missing": r.onboarding_missing,
        "jsonl_export_missing": r.jsonl_export_missing,
        "tracked_ledger": r.tracked_ledger,
        "ledger_guard_missing": r.ledger_guard_missing,
        "walshm_guard_missing": r.walshm_guard_missing,
        "legacy_sqlite_ledger": r.legacy_sqlite_ledger,
    })
}

/// The planned conformance state after a successful remediation.
///
/// In a real run `flags_after` is measured from disk after the remediation script executes. In
/// a dry run, the script must not mutate member repos, so a second collection would necessarily
/// keep the same unresolved flags and make the preview look like a no-op. For agent navigation,
/// `flags_before` remains the measured problem set and `flags_after` is the intended clean state
/// that the real remediation is expected to produce.
fn planned_resolved_flags_json() -> serde_json::Value {
    serde_json::json!({
        "onboarding_missing": false,
        "jsonl_export_missing": false,
        "tracked_ledger": false,
        "ledger_guard_missing": false,
        "walshm_guard_missing": false,
        "legacy_sqlite_ledger": false,
    })
}

/// The loop-init remediation script. `HANDOFF_LOOP_INIT` overrides (a non-standard kernel
/// home, or tests pointing at a stub); else the canonical
/// `<meta_root>/handoff/scripts/handoff-loop-init.sh` (DR2-verified: it accepts a single
/// member-directory positional and deploys to just that member, no `--fleet` required).
fn loop_init_script(root: &Path) -> PathBuf {
    match std::env::var("HANDOFF_LOOP_INIT") {
        Ok(p) if !p.is_empty() => PathBuf::from(p),
        _ => root.join("handoff/scripts/handoff-loop-init.sh"),
    }
}

/// Convert a Rust/OS path into an argument that Git Bash can open.
///
/// Windows CI executes this fleet-sync seam through Git Bash. Passing a raw
/// `C:\...` path to `bash <script> <member>` lets Bash interpret backslashes as
/// escapes, so the remediation stub exits before it can prove the after-state.
/// Keep the native drive prefix (`C:/...`) and only normalize separators: Git
/// Bash accepts drive-qualified forward-slash paths both as the script path and
/// as member-directory arguments from a native Rust process.
fn bash_path_arg(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Pick the Bash executable used for remediation stubs. On Windows CI, `Command::new("bash")`
/// can resolve to the wrong compatibility shim even though the workflow shell is Git Bash.
/// Prefer the explicit Git-for-Windows install path there, then fall back to PATH elsewhere.
fn bash_program() -> std::ffi::OsString {
    #[cfg(windows)]
    {
        let candidates = [
            std::env::var_os("GIT_BASH").map(std::path::PathBuf::from),
            std::env::var_os("ProgramFiles")
                .map(std::path::PathBuf::from)
                .map(|p| p.join("Git").join("bin").join("bash.exe")),
            std::env::var_os("ProgramW6432")
                .map(std::path::PathBuf::from)
                .map(|p| p.join("Git").join("bin").join("bash.exe")),
            std::env::var_os("ProgramFiles(x86)")
                .map(std::path::PathBuf::from)
                .map(|p| p.join("Git").join("bin").join("bash.exe")),
            Some(std::path::PathBuf::from(
                r"C:\Program Files\Git\bin\bash.exe",
            )),
        ];
        for candidate in candidates.into_iter().flatten() {
            if candidate.is_file() {
                return candidate.into_os_string();
            }
        }
    }
    std::ffi::OsString::from("bash")
}

/// Per-member remediation outcome.
struct MemberSync {
    name: String,
    /// Was this member non-conformant before the sweep?
    flagged: bool,
    /// "ok" (not flagged) · "would-remediate" (dry-run) · "remediate" (real run).
    action: &'static str,
    /// The loop-init script's exit code, when spawned. NOT used to judge success.
    script_exit: Option<i32>,
    /// Judged by the AFTER flags (a real run only): conformant now.
    resolved: bool,
    flags_before: serde_json::Value,
    flags_after: serde_json::Value,
    /// Set iff the member could not be remediated (spawn error, missing script, or still
    /// non-conformant after the deploy ran).
    failure: Option<String>,
}

/// The result of a fleet-sync sweep.
struct SyncReport {
    dry_run: bool,
    script: PathBuf,
    script_present: bool,
    members: Vec<MemberSync>,
}

impl SyncReport {
    /// A real run is OK iff every flagged member resolved. A dry run is a preview — never fails.
    fn ok(&self) -> bool {
        self.dry_run || self.members.iter().all(|m| m.resolved)
    }
    /// How many members were flagged (i.e. acted on / would be acted on).
    fn remediated(&self) -> usize {
        self.members.iter().filter(|m| m.flagged).count()
    }
}

/// The core sweep (no stdout, no ledger writes) — testable with a stub script. For each
/// member: collect its row; if not flagged, skip (resolved). If flagged, run the loop-init
/// script for that member's directory (passing `--dry-run` through), then RE-collect and judge
/// `resolved` by the AFTER flags. Per-member fail-closed: a spawn error, a missing script, or a
/// still-flagged AFTER row is a failure recorded on that member; the sweep always continues.
fn run_fleet_sync(root: &Path, members: &[String], dry_run: bool, script: &Path) -> SyncReport {
    let script_present = script.is_file();
    let mut out: Vec<MemberSync> = Vec::with_capacity(members.len());
    for name in members {
        let Some(b) = collect_rows(root, std::slice::from_ref(name))
            .into_iter()
            .next()
        else {
            continue;
        };
        let flags_before = flags_json(&b);
        if !member_needs_sync(&b) {
            out.push(MemberSync {
                name: name.clone(),
                flagged: false,
                action: "ok",
                script_exit: None,
                resolved: true,
                flags_after: flags_before.clone(),
                flags_before,
                failure: None,
            });
            continue;
        }
        let action = if dry_run {
            "would-remediate"
        } else {
            "remediate"
        };
        // Fail-closed: a flagged member with no remediation script cannot be fixed.
        if !script_present {
            out.push(MemberSync {
                name: name.clone(),
                flagged: true,
                action,
                script_exit: None,
                resolved: false,
                flags_after: flags_before.clone(),
                flags_before,
                failure: Some(format!(
                    "loop-init script not found at {} — cannot remediate",
                    script.display()
                )),
            });
            continue;
        }
        let mut cmd = Command::new(bash_program());
        #[cfg(windows)]
        {
            // The args we pass are already in the Git Bash-compatible form (`C:/...`).
            // Keep MSYS from rewriting them a second time before the stub sees them.
            cmd.env("MSYS_NO_PATHCONV", "1");
        }
        cmd.arg(bash_path_arg(script));
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.arg(bash_path_arg(&root.join(name)));
        let (script_exit, spawn_err) = match cmd.output() {
            Ok(o) => (o.status.code(), None),
            Err(e) => (None, Some(format!("failed to spawn remediation: {e}"))),
        };
        // Judge by the AFTER state, NEVER the script exit code (it always exits 0).
        let after = collect_rows(root, std::slice::from_ref(name));
        let still_flagged = after.first().map(member_needs_sync).unwrap_or(true);
        let flags_after = if dry_run && spawn_err.is_none() {
            planned_resolved_flags_json()
        } else {
            after
                .first()
                .map(flags_json)
                .unwrap_or_else(|| flags_before.clone())
        };
        let resolved = !dry_run && spawn_err.is_none() && !still_flagged;
        let failure = if let Some(e) = spawn_err {
            Some(e)
        } else if !dry_run && still_flagged {
            Some("still non-conformant after remediation".to_string())
        } else {
            None
        };
        out.push(MemberSync {
            name: name.clone(),
            flagged: true,
            action,
            script_exit,
            resolved,
            flags_before,
            flags_after,
            failure,
        });
    }
    SyncReport {
        dry_run,
        script: script.to_path_buf(),
        script_present,
        members: out,
    }
}

/// HFTASK-0087: `hf fleet sync` (and the `hf fleet status --fix` alias). Resolve the meta root +
/// members, run the remediation sweep, witness a `fleet_sync` event into the FLEET ledger
/// (fail-closed when present, loud-degrade when absent), print the report, and exit non-zero iff
/// any flagged member is still non-conformant after remediation — so a meta-level cron can gate
/// the fleet's self-healing on a clean exit code.
pub fn cmd_fleet_sync(json: bool, dry_run: bool) {
    let Some(root) = find_meta_root() else {
        eprintln!("hf fleet sync: no .meta.yaml found from the current directory upward");
        std::process::exit(1);
    };
    let meta_yaml = std::fs::read_to_string(root.join(".meta.yaml")).unwrap_or_default();
    let members = parse_members(&meta_yaml);
    let script = loop_init_script(&root);
    let report = run_fleet_sync(&root, &members, dry_run, &script);

    // Witness the remediation centrally (real runs that actually acted only). FLEET ledger
    // present → fail-closed (`must_witness` aborts if the append fails — we took an action we
    // could not record); absent → loud-degrade (there is genuinely nowhere central to witness).
    if !dry_run && report.remediated() > 0 {
        let fleet_db = root.join(".handoff").join("ledger.db");
        if fleet_db.is_file() {
            let payload = serde_json::json!({
                "remediated": report.remediated(),
                "all_resolved": report.ok(),
                "members": report.members.iter().filter(|m| m.flagged).map(|m| serde_json::json!({
                    "name": m.name, "resolved": m.resolved, "script_exit": m.script_exit,
                })).collect::<Vec<_>>(),
            })
            .to_string();
            let lp = fleet_db.to_string_lossy().into_owned();
            handoff_core::must_witness(
                Ledger::open(&lp).and_then(|mut l| {
                    l.append("fleet_sync", "FLEET", &payload, handoff_core::now_ns())
                }),
                "fleet_sync",
            );
        } else {
            eprintln!(
                "hf fleet sync: WARNING — no FLEET ledger at {}; remediation not centrally witnessed (loud-degrade)",
                fleet_db.display()
            );
        }
    }

    if json {
        let out = serde_json::json!({
            "schema": "handoff.fleet_sync.v1",
            "meta_root": root.to_string_lossy(),
            "dry_run": dry_run,
            "script": report.script.to_string_lossy(),
            "script_present": report.script_present,
            "remediated": report.remediated(),
            "all_resolved": report.ok(),
            "members": report.members.iter().map(|m| serde_json::json!({
                "name": m.name,
                "flagged": m.flagged,
                "action": m.action,
                "script_exit": m.script_exit,
                "resolved": m.resolved,
                "flags_before": m.flags_before,
                "flags_after": m.flags_after,
                "migration_plan": m.flags_before["legacy_sqlite_ledger"].as_bool().unwrap_or(false).then(|| migration_plan_json(&root, &m.name)),
                "failure": m.failure,
            })).collect::<Vec<_>>(),
            "failures": report.members.iter().filter_map(|m| {
                m.failure.as_ref().map(|f| serde_json::json!({ "name": m.name, "reason": f }))
            }).collect::<Vec<_>>(),
        });
        println!("{}", handoff_core::pretty_json(&out));
    } else {
        println!(
            "=== hf fleet sync ===  (meta root: {}){}",
            root.to_string_lossy(),
            if dry_run {
                "  [DRY-RUN — no changes]"
            } else {
                ""
            }
        );
        println!(
            "  script: {}{}",
            report.script.display(),
            if report.script_present {
                ""
            } else {
                "  (MISSING)"
            }
        );
        for m in report.members.iter().filter(|m| m.flagged) {
            let state = if dry_run {
                "would remediate"
            } else if m.resolved {
                "RESOLVED ✓"
            } else {
                "STILL NON-CONFORMANT ✗"
            };
            println!("  {:<26} {}", m.name, state);
            if let Some(f) = &m.failure {
                println!("      ⚠ {f}");
            }
        }
        let flagged = report.remediated();
        if flagged == 0 {
            println!("  all members conformant — nothing to remediate");
        } else {
            println!(
                "\n{} member(s) {}; all_resolved: {}",
                flagged,
                if dry_run {
                    "would be remediated"
                } else {
                    "remediated"
                },
                report.ok()
            );
        }
    }

    if !report.ok() {
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Fleet-aware packet rendering (ADR-0004 §4) — compile a member's packet from the
// FLEET ledger + that member's git-text capsule/cards, NOT from a per-repo ledger
// (there is none). Capsule-driven: the North Star comes from the member's capsule,
// never hardcoded (the cmd_handoff hardcode is the ADR-0006 portability bug this
// renderer deliberately avoids).
// ---------------------------------------------------------------------------

fn load_member_tasks(repo: &Path) -> Vec<WorkOrder> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(repo.join(".handoff/tasks")) {
        let mut paths: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        paths.sort();
        for p in paths {
            // fail-open-audit R1: previously a member card that failed to read/parse was silently
            // dropped (`if let Ok(wo) = ..`), so a broken member card vanished from the fleet
            // rollup the same way card #95 vanished from `hf status`. Reuse the kernel's LOUD,
            // schema-validated loader so a non-conforming member card surfaces a WARNING instead
            // of disappearing.
            if let Some(wo) = handoff_core::parse_card_file(&p) {
                v.push(wo);
            }
        }
    }
    v
}

/// Render `<member>/.handoff/packets/latest.md` from the FLEET ledger + the member's
/// capsule/cards. Pure-ish: the markdown is built by `compose_member_packet` (unit
/// tested); this wrapper does the I/O. Returns the written path.
pub fn render_member_packet(root: &Path, member: &str) -> Result<PathBuf, String> {
    let repo = root.join(member);
    if !repo.is_dir() {
        return Err(format!(
            "member '{member}' not present at {}",
            repo.display()
        ));
    }
    let capsule_project =
        capsule_field(&repo, "project_name").unwrap_or_else(|| member.to_string());
    let northstar = capsule_field(&repo, "northstar")
        .unwrap_or_else(|| "(no northstar in capsule — seed context/capsule.json)".into());

    let tasks = load_member_tasks(&repo);

    // FLEET ledger replay (events keyed by work_order_id); a member card's status is
    // the ledger truth where present, else the card's stored status.
    let fleet_db = root.join(".handoff").join("ledger.db");
    let (replay, witness) = if fleet_db.is_file() {
        let lp = fleet_db.to_string_lossy().into_owned();
        let r = Ledger::open(&lp)
            .and_then(|l| l.replay_latest_status())
            .unwrap_or_default();
        let w = Ledger::open(&lp)
            .and_then(|l| l.verify_witness_chain())
            .unwrap_or(0);
        (r, w)
    } else {
        (vec![], 0)
    };

    let md = compose_member_packet(
        member,
        &capsule_project,
        &northstar,
        &tasks,
        &replay,
        witness,
    );
    let packets = repo.join(".handoff").join("packets");
    std::fs::create_dir_all(&packets).map_err(|e| e.to_string())?;
    let out = packets.join("latest.md");
    std::fs::write(&out, &md).map_err(|e| e.to_string())?;
    Ok(out)
}

fn member_status_of(card: &WorkOrder, replay: &[(String, Status)]) -> Status {
    replay
        .iter()
        .find(|(k, _)| k == &card.id)
        .map(|(_, s)| *s)
        .unwrap_or(card.status)
}

/// Build the member packet markdown. Pure over its inputs → unit-testable.
fn compose_member_packet(
    member: &str,
    project: &str,
    northstar: &str,
    tasks: &[WorkOrder],
    replay: &[(String, Status)],
    witness: usize,
) -> String {
    let done = tasks
        .iter()
        .filter(|t| member_status_of(t, replay) == Status::Done)
        .count();
    let remaining: Vec<&WorkOrder> = tasks
        .iter()
        .filter(|t| member_status_of(t, replay) != Status::Done)
        .collect();
    let mut md = String::new();
    md.push_str("# Handoff Packet (latest) — handoff.packet.v2\n\n");
    md.push_str(&format!("> Compiled by `hf fleet render {member}` from the FLEET ledger (meta/.handoff) + this repo's git-text capsule/cards. Not rendered from a per-repo ledger (ADR-0004 §3).\n\n"));
    md.push_str(&format!("## 1. North Star ({project})\n{northstar}\n\n"));
    md.push_str("## 2. State Precedence\nGit > FLEET ledger (meta/.handoff/ledger.db) > tasks/*.task.json > this packet.\n\n");
    md.push_str(&format!(
        "## 3. Progress\nDone: {}/{}.  FLEET tamper-evident events verified: {}.\n\n",
        done,
        tasks.len(),
        witness
    ));
    md.push_str("## 4. Remaining\n");
    if remaining.is_empty() {
        md.push_str("- (no open cards)\n");
    }
    for t in &remaining {
        md.push_str(&format!(
            "- [{}] **{}** — {}\n",
            t.priority_str(),
            t.id,
            t.title
        ));
    }
    md.push('\n');
    md
}

#[cfg(test)]
mod tests {
    use super::parse_members;

    // ---- HFTASK-0087: hf fleet sync remediation ----

    /// Build a `Row` with the four P7 conformance flags set as given (other fields neutral).
    fn row_with(flags: (bool, bool, bool, bool)) -> super::Row {
        super::Row {
            name: "m".into(),
            present: true,
            has_handoff: true,
            onboarding_missing: false,
            cards: 0,
            project_name: None,
            role: None,
            plane: None,
            jsonl_export_missing: flags.0,
            tracked_ledger: flags.1,
            ledger_guard_missing: flags.2,
            walshm_guard_missing: flags.3,
            legacy_sqlite_ledger: false,
            per_repo_chain: None,
        }
    }

    /// Isolated temp directory (pid + nanos), never the real workspace.
    fn unique_tmp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "hf-fleetsync-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn bash_path_arg_preserves_windows_drive_prefix_and_normalizes_separators() {
        assert_eq!(
            super::bash_path_arg(std::path::Path::new(
                r"C:\Users\runneradmin\AppData\Local\Temp\stub.sh"
            )),
            "C:/Users/runneradmin/AppData/Local/Temp/stub.sh"
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn bash_program_falls_back_to_path_bash_off_windows() {
        assert_eq!(super::bash_program(), std::ffi::OsString::from("bash"));
    }

    #[test]
    fn sync_selects_only_flagged_members() {
        assert!(!super::member_needs_sync(&row_with((
            false, false, false, false
        ))));
        assert!(super::member_needs_sync(&row_with((
            true, false, false, false
        ))));
        assert!(super::member_needs_sync(&row_with((
            false, true, false, false
        ))));
        assert!(super::member_needs_sync(&row_with((
            false, false, true, false
        ))));
        assert!(super::member_needs_sync(&row_with((
            false, false, false, true
        ))));
        let mut legacy = row_with((false, false, false, false));
        legacy.legacy_sqlite_ledger = true;
        assert!(super::member_needs_sync(&legacy));
    }

    /// THE load-bearing guarantee (verifier's constraint): the loop-init script exits 0 even
    /// when it remediated nothing, so success MUST be judged by the AFTER state. A stub that
    /// exits 0 but changes nothing leaves the member flagged → resolved=false, failure recorded.
    #[test]
    fn sync_judges_by_after_state_not_script_exit() {
        let root = unique_tmp("after");
        std::fs::create_dir_all(root.join("memberx/.handoff/tasks")).unwrap();
        let stub = root.join("noop.sh");
        std::fs::write(&stub, "#!/usr/bin/env bash\nexit 0\n").unwrap();

        let report = super::run_fleet_sync(&root, &["memberx".to_string()], false, &stub);
        let m = &report.members[0];
        assert!(
            m.flagged,
            "a non-git .handoff member is flagged (ledger_guard_missing)"
        );
        assert_eq!(m.script_exit, Some(0), "stub exited 0");
        assert!(
            !m.resolved,
            "exit 0 must NOT mean resolved when the after-state is still flagged"
        );
        assert!(m.failure.is_some());
        assert!(
            !report.ok(),
            "an unresolved flagged member makes the run fail-closed"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// A dry run is a preview: it passes `--dry-run` through to the script and never fails.
    #[test]
    fn sync_dry_run_passes_through_and_never_fails() {
        let root = unique_tmp("dry");
        std::fs::create_dir_all(root.join("memberx/.handoff/tasks")).unwrap();
        let marker = root.join("args.txt");
        let stub = root.join("rec.sh");
        std::fs::write(
            &stub,
            format!(
                "#!/usr/bin/env bash\necho \"$@\" > {}\nexit 0\n",
                super::bash_path_arg(&marker)
            ),
        )
        .unwrap();

        let report = super::run_fleet_sync(&root, &["memberx".to_string()], true, &stub);
        assert!(
            report.dry_run && report.ok(),
            "dry-run is a preview, never fails"
        );
        assert_eq!(report.members[0].action, "would-remediate");
        let recorded = std::fs::read_to_string(&marker).unwrap_or_default();
        assert!(
            recorded.contains("--dry-run"),
            "dry-run must pass --dry-run; got {recorded:?}"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// A dry run is an agent-facing preview: `flags_before` is the measured unresolved state
    /// and `flags_after` is the planned clean state, not a misleading second read of unchanged
    /// disk state.
    #[test]
    fn fleet_sync_dry_run_reports_planned_resolution() {
        let root = unique_tmp("dryplanned");
        std::fs::create_dir_all(root.join("memberx/.handoff/tasks")).unwrap();
        let stub = root.join("noop.sh");
        std::fs::write(&stub, "#!/usr/bin/env bash\nexit 0\n").unwrap();

        let report = super::run_fleet_sync(&root, &["memberx".to_string()], true, &stub);
        let m = &report.members[0];
        assert_eq!(m.action, "would-remediate");
        assert!(
            m.flags_before["ledger_guard_missing"]
                .as_bool()
                .unwrap_or(false),
            "test fixture should start with a measured missing ledger guard"
        );
        assert!(
            !m.flags_after["ledger_guard_missing"]
                .as_bool()
                .unwrap_or(true),
            "dry-run flags_after should show the planned resolved state"
        );
        assert!(
            !m.flags_after["jsonl_export_missing"]
                .as_bool()
                .unwrap_or(true)
        );
        assert!(
            !m.flags_after["legacy_sqlite_ledger"]
                .as_bool()
                .unwrap_or(true)
        );
        assert!(report.ok(), "dry-run remains a non-mutating preview");
        std::fs::remove_dir_all(&root).ok();
    }

    /// HFTASK-0088: a present member with no `.handoff` is not a clean/no-op member;
    /// it is selected for first-time onboarding. A stub that creates `.handoff` plus the
    /// ledger-cache guards proves the after-state, not script exit alone, resolves it.
    #[test]
    fn sync_onboards_present_member_missing_handoff() {
        use std::process::Command;
        let root = unique_tmp("onboard");
        let member = root.join("memberx");
        std::fs::create_dir_all(&member).unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&member)
            .output()
            .unwrap();
        let stub = root.join("init.sh");
        std::fs::write(
            &stub,
            "#!/usr/bin/env bash\nset -e\ndir=\"${@: -1}\"\nmkdir -p \"$dir/.handoff/tasks\"\ncat > \"$dir/.gitignore\" <<'EOF'\n.handoff/**/ledger.db\n.handoff/**/*.db-wal\n.handoff/**/*.db-shm\nEOF\nexit 0\n",
        )
        .unwrap();

        let report = super::run_fleet_sync(&root, &["memberx".to_string()], false, &stub);
        assert_eq!(report.remediated(), 1, "missing .handoff must be selected");
        assert!(
            report.ok(),
            "stub-created .handoff + guards resolves the member"
        );
        let m = &report.members[0];
        assert_eq!(m.action, "remediate");
        assert!(
            m.flags_before["onboarding_missing"]
                .as_bool()
                .unwrap_or(false)
        );
        assert!(
            !m.flags_after["onboarding_missing"]
                .as_bool()
                .unwrap_or(true)
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// Idempotence: a conformant already-onboarded fleet yields an empty remediation set
    /// and a clean exit — running sync on a healthy fleet is a no-op.
    #[test]
    fn sync_clean_fleet_is_noop() {
        use std::process::Command;
        let root = unique_tmp("clean");
        let member = root.join("memberx");
        std::fs::create_dir_all(member.join(".handoff/tasks")).unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&member)
            .output()
            .unwrap();
        std::fs::write(
            member.join(".gitignore"),
            ".handoff/**/ledger.db\n.handoff/**/*.db-wal\n.handoff/**/*.db-shm\n",
        )
        .unwrap();
        let stub = root.join("noop.sh");
        std::fs::write(&stub, "#!/usr/bin/env bash\nexit 0\n").unwrap();

        let report = super::run_fleet_sync(&root, &["memberx".to_string()], false, &stub);
        assert_eq!(report.remediated(), 0);
        assert!(report.ok());
        assert_eq!(report.members[0].action, "ok");
        std::fs::remove_dir_all(&root).ok();
    }

    /// Fail-closed: a flagged member with no remediation script on disk is a recorded failure,
    /// not a silent skip.
    #[test]
    fn sync_missing_script_fails_closed() {
        let root = unique_tmp("noscript");
        std::fs::create_dir_all(root.join("memberx/.handoff/tasks")).unwrap();
        let missing = root.join("does-not-exist.sh");

        let report = super::run_fleet_sync(&root, &["memberx".to_string()], false, &missing);
        assert!(!report.script_present);
        let m = &report.members[0];
        assert!(m.flagged && !m.resolved);
        assert!(m.failure.as_deref().unwrap_or("").contains("not found"));
        assert!(!report.ok());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn parses_member_keys_under_projects_only() {
        let yaml = "\
defaults:
  parallel: true

projects:
  handoff:
    repo: git@example/handoff.git
    tags: [orchestration, handoff]
  # a comment
  loop_lib:
    repo: git@example/loop_lib.git
    provides: [loop-lib]

other:
  not_a_member:
    x: y
";
        let m = parse_members(yaml);
        assert_eq!(m, vec!["handoff".to_string(), "loop_lib".to_string()]);
    }

    #[test]
    fn member_packet_is_capsule_driven_not_hardcoded() {
        // No tasks; the North Star must come from the capsule arg, never the kernel's
        // hardcoded "Adopt RuVector…" string (the ADR-0006 portability bug).
        let md = super::compose_member_packet(
            "flexnetos_runner",
            "flexnetos_runner (ops/execution plane)",
            "A local runner+app to connect all of meta seamlessly.",
            &[],
            &[],
            7,
        );
        assert!(md.contains("flexnetos_runner (ops/execution plane)"));
        assert!(md.contains("A local runner+app to connect all of meta seamlessly."));
        assert!(!md.contains("Adopt RuVector"));
        assert!(md.contains("FLEET ledger"));
        assert!(md.contains("events verified: 7"));
    }

    /// HFTASK-0033: a temp meta-root with a central FLEET ledger and one member's per-repo
    /// ledger rolled into it — `per_repo_chain_stats` verifies the member's chain (ii)
    /// standalone and `fleet_provenance` verifies the rollup bridge (iii); tampering breaks it.
    #[test]
    fn fleet_status_verifies_per_repo_chain_and_provenance() {
        use ledger::Ledger;

        // Isolated temp meta-root (never the real workspace).
        let root = std::env::temp_dir().join(format!(
            "hf-fleet-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let central_path = root.join(".handoff").join("ledger.db");
        let member_repo = root.join("memberx");
        let member_path = member_repo.join(".handoff").join("ledger.db");
        std::fs::create_dir_all(central_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(member_path.parent().unwrap()).unwrap();

        // Seed the member's own ledger with 3 native events.
        {
            let mut m = Ledger::open(member_path.to_str().unwrap()).unwrap();
            for i in 0..3 {
                m.append("checkpoint", &format!("WO-{i}"), "{}", 1_000 + i)
                    .unwrap();
            }
        }
        // Roll the member up into the central FLEET ledger.
        {
            let rows = Ledger::open(member_path.to_str().unwrap())
                .unwrap()
                .events_after(0)
                .unwrap();
            let mut c = Ledger::open(central_path.to_str().unwrap()).unwrap();
            c.rollup_from("memberx", &rows, 1).unwrap();
        }

        // (ii) the member's per-repo chain verifies standalone.
        let chain = super::per_repo_chain_stats(&member_repo).expect("member has a local ledger");
        assert_eq!(chain.events, 3);
        assert_eq!(chain.witnessed, 3);
        // A member with no ledger → None.
        assert!(super::per_repo_chain_stats(&root.join("absent")).is_none());

        // (iii) provenance is faithful over the central rollup. (The failure direction —
        // tampering breaks faithfulness — is proven in the `ledger` crate's own
        // `verify_rollup_provenance_detects_tampered_row`, which can reach the private conn.)
        let prov = super::fleet_provenance(&root).expect("central ledger present");
        assert!(prov.is_faithful());
        assert_eq!(prov.verified, 3);
        assert_eq!(prov.per_repo, vec![("memberx".to_string(), 3)]);

        std::fs::remove_dir_all(&root).ok();
    }

    /// HFTASK-0034 (ADR-0004 §6 rev): the P7 flip — only a git-TRACKED ledger DB is a
    /// violation; a gitignored one is legitimate; the `.gitignore` guard must exist.
    #[test]
    fn p7_flip_tracked_ledger_and_guard_detection() {
        use std::process::Command;
        let repo = std::env::temp_dir().join(format!(
            "hf-p7-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(repo.join(".handoff")).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(["-C", repo.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);

        // Before any guard: no tracked db, and the guard is absent.
        assert!(!super::git_tracks_handoff_db(&repo));
        assert!(
            !super::ledger_guard_present(&repo),
            "no .gitignore yet → ledger guard absent"
        );
        assert!(
            !super::walshm_guard_present(&repo),
            "no .gitignore yet → WAL/SHM guard absent"
        );

        // Add only the ledger.db guard (pre-HFTASK-0035 state). The `**` pattern also covers
        // the top-level path.
        std::fs::write(repo.join(".gitignore"), ".handoff/**/ledger.db\n").unwrap();
        assert!(
            super::ledger_guard_present(&repo),
            "ledger guard present after writing .gitignore"
        );
        assert!(
            !super::walshm_guard_present(&repo),
            "WAL/SHM guard still absent (only ledger.db guard present)"
        );

        // Add the WAL/SHM side-car guards (HFTASK-0035 upgrade).
        std::fs::write(
            repo.join(".gitignore"),
            ".handoff/**/ledger.db\n.handoff/**/*.db-wal\n.handoff/**/*.db-shm\n",
        )
        .unwrap();
        assert!(
            super::walshm_guard_present(&repo),
            "WAL/SHM guard present after writing patterns"
        );

        // A gitignored ledger present on disk is LEGITIMATE — not tracked.
        std::fs::write(repo.join(".handoff/ledger.db"), b"x").unwrap();
        git(&["add", "-A"]);
        assert!(
            !super::git_tracks_handoff_db(&repo),
            "a gitignored ledger on disk must NOT count as tracked (legitimate)"
        );

        // Force-tracking the ledger is the actual P7 violation.
        git(&["add", "-f", ".handoff/ledger.db"]);
        assert!(
            super::git_tracks_handoff_db(&repo),
            "a git-TRACKED ledger.db IS the P7 violation"
        );

        std::fs::remove_dir_all(&repo).ok();
    }

    /// ADR-0018 D1 (HFTASK-0067): the inverted primary gate — a repo with a local ledger on disk
    /// must commit its `.handoff/ledger.events.jsonl` text export. Missing = violation; tracked =
    /// conformant.
    #[test]
    fn p7_inversion_requires_tracked_jsonl_export() {
        use std::process::Command;
        let repo = std::env::temp_dir().join(format!(
            "hf-p7-jsonl-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(repo.join(".handoff")).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(["-C", repo.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);

        // No ledger on disk yet → nothing to export.
        assert!(!super::local_ledger_on_disk(&repo));
        assert!(!super::git_tracks_jsonl_export(&repo));

        // A local ledger exists but the JSONL export is not committed → the violation condition.
        std::fs::write(repo.join(".handoff/ledger.db"), b"x").unwrap();
        assert!(super::local_ledger_on_disk(&repo));
        assert!(
            !super::git_tracks_jsonl_export(&repo),
            "ledger on disk, no committed JSONL export → durable truth missing"
        );

        // Commit the JSONL export → conformant.
        std::fs::write(repo.join(".handoff/ledger.events.jsonl"), "{}\n").unwrap();
        git(&["add", "-f", ".handoff/ledger.events.jsonl"]);
        assert!(
            super::git_tracks_jsonl_export(&repo),
            "committed ledger.events.jsonl satisfies the inverted P7 gate"
        );

        std::fs::remove_dir_all(&repo).ok();
    }

    /// HFTASK-0091: a legacy SQLite member ledger is a first-class, machine-readable
    /// remediation condition. Once migrated to redb and exported to tracked JSONL, the same
    /// member is healthy; the legacy file was never opened as an empty redb ledger.
    #[test]
    fn legacy_sqlite_member_has_migration_plan_then_becomes_healthy() {
        use ledger::Ledger;
        use std::process::Command;

        let root = unique_tmp("legacy-plan");
        let member = root.join("memberx");
        std::fs::create_dir_all(member.join(".handoff/tasks")).unwrap();
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&member)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "t@example.com"])
            .current_dir(&member)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "t"])
            .current_dir(&member)
            .output()
            .unwrap();
        std::fs::write(
            member.join(".gitignore"),
            ".handoff/**/ledger.db\n.handoff/**/*.db-wal\n.handoff/**/*.db-shm\n",
        )
        .unwrap();
        let ledger_path = member.join(".handoff").join("ledger.db");
        std::fs::write(&ledger_path, b"SQLite format 3\0legacy fixture").unwrap();

        let before = super::collect_rows(&root, &["memberx".to_string()])
            .pop()
            .unwrap();
        assert!(before.legacy_sqlite_ledger);
        assert!(super::member_needs_sync(&before));
        assert!(before.per_repo_chain.is_none());
        let plan = super::migration_plan_json(&root, "memberx");
        assert!(
            plan["command"]
                .as_str()
                .unwrap_or_default()
                .contains("--features legacy-sqlite")
        );
        assert_eq!(
            plan["ledger_path"].as_str().unwrap_or_default(),
            ledger_path.to_string_lossy()
        );

        // Simulate the safe migration result: the binary is now redb, its chain verifies,
        // and its deterministic JSONL export is staged/tracked as the durable git truth.
        std::fs::remove_file(&ledger_path).unwrap();
        let mut led = Ledger::open(ledger_path.to_str().unwrap()).unwrap();
        led.append("checkpoint", "LEGACY-MIGRATED", "{}", 1)
            .unwrap();
        let events = led.all_events().unwrap();
        let jsonl = ledger::export_jsonl(&events).unwrap();
        drop(led);
        std::fs::write(member.join(".handoff/ledger.events.jsonl"), jsonl).unwrap();
        Command::new("git")
            .args(["add", "-f", ".handoff/ledger.events.jsonl"])
            .current_dir(&member)
            .output()
            .unwrap();

        let after = super::collect_rows(&root, &["memberx".to_string()])
            .pop()
            .unwrap();
        assert!(!after.legacy_sqlite_ledger);
        assert!(
            !super::member_needs_sync(&after),
            "migrated redb ledger + tracked JSONL + guards is healthy"
        );
        let chain = after.per_repo_chain.expect("migrated redb chain verifies");
        assert_eq!(chain.events, 1);
        assert_eq!(chain.witnessed, 1);

        std::fs::remove_dir_all(&root).ok();
    }
}
