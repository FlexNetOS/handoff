//! `hf fleet status` — fleet aggregation (ADR-0004 §4).
//!
//! Enumerate members from the meta root's `.meta.yaml`, read each repo's git-text
//! `.handoff` (capsule + cards), and join with the FLEET ledger events into one board.
//! **Git is the sync transport** — no daemons. State precedence stays Git > ledger >
//! cards.
//!
//! Residency (ADR-0004 §3, settled): there is one witnessed ledger per orchestration
//! home — the FLEET ledger lives at `<meta-root>/.handoff/ledger.db`. A per-repo
//! `.handoff/` carries **no `ledger.db` / no binary state** (git text only); a stray
//! per-repo ledger is a policy-P7 violation, surfaced here as a warning.

use crate::PrioStr;
use ledger::Ledger;
use std::path::{Path, PathBuf};
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
fn parse_members(meta_yaml: &str) -> Vec<String> {
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
    forbidden_ledger: bool,
}

/// Orchestration homes legitimately carry a ledger (ADR-0004 §3 / envctl ADR-0001
/// §23): the FLEET home is the meta root itself (not a member, so never in this list)
/// and the KERNEL home is `handoff/`. Every OTHER member must be git-text-only, so a
/// `ledger.db` there is a policy-P7 violation.
fn is_orchestration_home(name: &str) -> bool {
    name == "handoff"
}

fn collect_rows(root: &Path, members: &[String]) -> Vec<Row> {
    members
        .iter()
        .map(|name| {
            let repo = root.join(name);
            let present = repo.is_dir();
            let has_handoff = repo.join(".handoff").is_dir();
            let has_ledger = repo.join(".handoff/ledger.db").is_file();
            Row {
                name: name.clone(),
                present,
                has_handoff,
                cards: count_cards(&repo),
                project_name: capsule_field(&repo, "project_name"),
                role: capsule_field(&repo, "role"),
                plane: capsule_field(&repo, "plane"),
                forbidden_ledger: has_ledger && !is_orchestration_home(name),
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

pub fn cmd_fleet_status(json: bool) {
    let Some(root) = find_meta_root() else {
        eprintln!("hf fleet status: no .meta.yaml found from the current directory upward");
        std::process::exit(1);
    };
    let meta_yaml = std::fs::read_to_string(root.join(".meta.yaml")).unwrap_or_default();
    let members = parse_members(&meta_yaml);
    let rows = collect_rows(&root, &members);
    let (events, witness, ledger_present) = fleet_ledger_stats(&root);

    let with_handoff = rows.iter().filter(|r| r.has_handoff).count();
    let warnings: Vec<String> = rows
        .iter()
        .filter(|r| r.forbidden_ledger)
        .map(|r| {
            format!(
                "{}: carries a per-repo .handoff/ledger.db — policy-P7 violation (ADR-0004 §3); events belong in the FLEET ledger",
                r.name
            )
        })
        .collect();

    if json {
        let out = serde_json::json!({
            "schema": "handoff.fleet_status.v1",
            "meta_root": root.to_string_lossy(),
            "fleet_ledger": {
                "path": root.join(".handoff").join("ledger.db").to_string_lossy(),
                "present": ledger_present,
                "events": events,
                "witnessed_verified": witness,
            },
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
                "forbidden_ledger": r.forbidden_ledger,
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
    println!(
        "members: {} total · {} with .handoff\n",
        rows.len(),
        with_handoff
    );
    println!("  {:<26} {:<8} {:<6} capsule (role/plane)", "member", ".handoff", "cards");
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
        let flag = if r.forbidden_ledger { "  ⚠ stray ledger.db (P7)" } else { "" };
        println!("  {:<26} {:<8} {:<6} {}{}", r.name, hand, cards, id, flag);
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
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(wo) = serde_json::from_str::<WorkOrder>(&s) {
                    v.push(wo);
                }
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
        return Err(format!("member '{member}' not present at {}", repo.display()));
    }
    let capsule_project = capsule_field(&repo, "project_name").unwrap_or_else(|| member.to_string());
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

    let md = compose_member_packet(member, &capsule_project, &northstar, &tasks, &replay, witness);
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
        md.push_str(&format!("- [{}] **{}** — {}\n", t.priority_str(), t.id, t.title));
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
}
