//! `hf fleet status` — fleet aggregation (ADR-0004 §4).
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

use crate::PrioStr;
use ledger::{Ledger, RollupProvenance};
use std::path::{Path, PathBuf};
use std::process::Command;
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
pub(crate) fn parse_members(meta_yaml: &str) -> Vec<String> {
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
        if in_projects && indent == 2 {
            if let Some(name) = body.strip_suffix(':') {
                if !name.is_empty() && !name.contains(char::is_whitespace) {
                    out.push(name.to_string());
                }
            }
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
            Row {
                name: name.clone(),
                present,
                has_handoff,
                cards: count_cards(&repo),
                project_name: capsule_field(&repo, "project_name"),
                role: capsule_field(&repo, "role"),
                plane: capsule_field(&repo, "plane"),
                jsonl_export_missing,
                tracked_ledger,
                ledger_guard_missing,
                walshm_guard_missing,
                per_repo_chain: per_repo_chain_stats(&repo),
            }
        })
        .collect()
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
    // HFTASK-0033: a broken provenance bridge is an integrity alarm, not a style nit —
    // surface it as a warning so the loop's drift/gate sees it.
    if let Some(p) = &provenance {
        if !p.is_faithful() {
            warnings.push(format!(
                "FLEET ledger: rollup provenance BROKEN — {} of {} rolled-up row(s) do not reproduce their origin_action_hash (ADR-0004 §3.3)",
                p.mismatched,
                p.total()
            ));
        }
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
                "cards": r.cards,
                "project_name": r.project_name,
                "role": r.role,
                "plane": r.plane,
                "jsonl_export_missing": r.jsonl_export_missing,
                "tracked_ledger": r.tracked_ledger,
                "ledger_guard_missing": r.ledger_guard_missing,
                "walshm_guard_missing": r.walshm_guard_missing,
                // HFTASK-0033 (ii): this member's own ledger chain, verified standalone.
                "per_repo_chain": r.per_repo_chain.as_ref().map(|c| serde_json::json!({
                    "events": c.events,
                    "witnessed_verified": c.witnessed,
                })),
            })).collect::<Vec<_>>(),
            "warnings": warnings,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
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
            r.jsonl_export_missing,
            r.tracked_ledger,
            r.ledger_guard_missing,
            r.walshm_guard_missing,
        ) {
            (true, _, _, _) => "  ⚠ no committed ledger.events.jsonl (P7)",
            (false, true, _, _) => "  ⚠ tracked ledger.db (P7)",
            (false, false, true, _) => "  ⚠ no ledger .gitignore guard (P7)",
            (false, false, false, true) => "  ⚠ no WAL/SHM .gitignore guard (P7)",
            (false, false, false, false) => "",
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
            if let Some(wo) = crate::parse_card_file(&p) {
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
}
