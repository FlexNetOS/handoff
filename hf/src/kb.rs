//! kb ↔ handoff seam (ADR-0003): mint handoff cards FROM git-kb tasks — the planning plane
//! (git-kb) feeds the execution plane (.handoff). Read-only on the kb; writes only a local
//! card, stamping `correlation_id = <slug>` as the cross-reference handle. One-way by
//! construction (the kb is never read back as execution truth).

use std::path::{Path, PathBuf};
use std::process::Command;

use work_order::{Priority, Status, WorkOrder};

/// The directory holding the `.kb/` that `git-kb` should operate in. Pure (takes the cwd,
/// probes the filesystem). Resolution order (HFTASK-0072 / ADR-0018 D7):
///   1. the repo's OWN `.kb/` (`<repo_root>/.kb`) — handoff now has a full local `.kb`, so
///      the seam binds to the repo's own planning plane first;
///   2. the meta workspace `.kb/` (`<repo_root>/../.kb`) — the original FLEET behavior, kept
///      as a no-downgrade fallback for repos that only have the meta-root kb.
///
/// `None` only when neither exists (standalone, no kb at all → the seam degrades to a no-op).
pub fn kb_root(repo_root: &Path) -> Option<PathBuf> {
    if repo_root.join(".kb").exists() {
        return Some(repo_root.to_path_buf());
    }
    let parent = repo_root.parent()?;
    if parent.join(".kb").exists() {
        Some(parent.to_path_buf())
    } else {
        None
    }
}

/// Run `git-kb` in `dir` with explicit argv (no shell), capturing stdout.
fn run_kb_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    match Command::new("git-kb").args(args).current_dir(dir).output() {
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).to_string()),
        Ok(o) => Err(format!(
            "git-kb {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("git-kb not runnable: {e}")),
    }
}

/// Read a scalar `key: value` from a doc's YAML frontmatter (quotes stripped). Pure.
pub fn frontmatter_value(doc: &str, key: &str) -> Option<String> {
    let mut lines = doc.lines();
    if lines.next().map(str::trim) != Some("---") {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        let t = line.trim();
        if t == "---" {
            break;
        }
        if let Some(rest) = t.strip_prefix(&prefix) {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// The document body after the frontmatter block (the objective text). Pure.
pub fn frontmatter_body(doc: &str) -> String {
    let mut lines = doc.lines();
    if lines.next().map(str::trim) != Some("---") {
        return doc.trim().to_string(); // no frontmatter at all
    }
    let mut in_fm = true;
    let mut body = String::new();
    for line in lines {
        if in_fm {
            if line.trim() == "---" {
                in_fm = false;
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    body.trim().to_string()
}

/// Map a kb priority string to a handoff Priority.
pub fn map_priority(p: Option<&str>) -> Priority {
    match p.unwrap_or("medium") {
        "critical" | "highest" => Priority::P0,
        "high" => Priority::P1,
        "medium" => Priority::P2,
        _ => Priority::P3,
    }
}

/// Deterministic card id from a kb slug: `KBTASK-<UPPER-SANITIZED-TAIL>`. Pure.
pub fn card_id_from_slug(slug: &str) -> String {
    let tail = slug.rsplit('/').next().unwrap_or(slug);
    let san: String = tail
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '-'
            }
        })
        .collect();
    format!("KBTASK-{san}")
}

/// Build a handoff WorkOrder from a kb task document (pure, testable without git-kb).
pub fn work_order_from_kb_doc(slug: &str, doc: &str) -> WorkOrder {
    let title = frontmatter_value(doc, "title").unwrap_or_else(|| slug.to_string());
    let priority = map_priority(frontmatter_value(doc, "priority").as_deref());
    let body = frontmatter_body(doc);
    let objective = if body.is_empty() {
        format!("Minted from kb task {slug}")
    } else {
        body
    };
    let path_scope = vec![".".to_string()];
    let acceptance = vec![format!(
        "{title}: delivered + tests green + drift-audited (kb_ref {slug})"
    )];
    let intent_lock = WorkOrder::compute_intent_lock(&objective, &path_scope, &acceptance);
    WorkOrder {
        schema: "handoff.task.v1".into(),
        id: card_id_from_slug(slug),
        title,
        status: Status::Backlog,
        priority,
        objective,
        path_scope,
        acceptance_criteria: acceptance,
        test_commands: vec![],
        dependencies: vec![],
        blocked_by: vec![],
        allows_network: false,
        allows_dependency_addition: true,
        correlation_id: slug.to_string(), // the kb_ref ↔ card cross-reference handle
        role: Some("implementer".into()),
        intent_lock,
    }
}

/// Where a kb-minted card is written, keyed to the PLANE the kb slug came from. Pure
/// (testable without git-kb / without touching the real ledgers). The second tuple element
/// is a human-readable plane label for the success message.
///
/// HFTASK-0072 / ADR-0018 D7: now that handoff has its OWN local `.kb`, the card must land in
/// the SAME plane as its source kb. `local_kb` is true when the slug was read from the repo's
/// own `.kb` (`kb_root` == cwd): the card belongs in the repo's local `.handoff/tasks/`. When
/// the slug came from the META `.kb` it is fleet-pickup-able by definition and belongs in the
/// FLEET tasks dir (`<meta-root>/.handoff/tasks/`) — NEVER the cwd `.handoff/` (the historical
/// contamination bug: envctl-domain KBTASK cards landing in handoff's KERNEL ledger).
fn mint_target(local_kb: bool, meta_root: Option<PathBuf>) -> (PathBuf, &'static str) {
    if local_kb {
        return (crate::tasks_dir(), "LOCAL");
    }
    match meta_root {
        Some(root) => (root.join(crate::HF).join("tasks"), "FLEET"),
        None => (crate::tasks_dir(), "standalone (no meta root)"),
    }
}

/// `hf task mint --from-kb <slug>` — mint a handoff card from a kb task (planning → execution).
pub fn cmd_mint_from_kb(slug: &str) {
    if slug.is_empty() {
        eprintln!("usage: hf task mint --from-kb <kb-slug>");
        return;
    }
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(root) = kb_root(&repo_root) else {
        eprintln!("hf task mint: no `.kb/` found (need a local or meta-root kb) — cannot mint");
        return;
    };
    let doc = match run_kb_in(&root, &["show", slug]) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("hf task mint: {e}");
            return;
        }
    };
    let wo = work_order_from_kb_doc(slug, &doc);
    let id = wo.id.clone();
    // The card lands in the SAME plane as its source kb: LOCAL kb → repo `.handoff/tasks/`;
    // META kb → FLEET tasks dir. `kb_root == repo_root` means the slug came from the repo's
    // own `.kb` (HFTASK-0072). Routing a meta-domain slug to FLEET (never the cwd) preserves
    // the anti-contamination invariant.
    let local_kb = root == repo_root;
    let (where_dir, plane) = mint_target(local_kb, crate::fleet::find_meta_root());
    crate::save_task_in(&where_dir, &wo);
    println!("hf task mint: {id} minted from kb {slug} (correlation_id = kb_ref = {slug})");
    println!("  wrote card to {} [{plane}]", where_dir.display());
    println!("  next: hf claim {id}");
}

// --- ADR-0003 rule 3: kb task write-back (OUT direction, HFTASK-0042) -------------------
//
// The seam above is INward (kb plan → handoff card). This is the OUTward write-back: as the
// execution plane advances a card minted from a kb task, it flips that kb task's status and
// appends a progress line — so the planning plane reflects execution. STILL ONE-WAY: the kb is
// never read back as execution truth (ADR-0003); we only *inform* it. Best-effort + degrading:
// a card whose `correlation_id` is not a kb slug, an absent meta `.kb/`, or an absent `git-kb`
// all make write-back a silent no-op (exactly how the weave-lease bridge degrades).

/// The execution transition being mirrored back to the kb task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KbTransition {
    /// `hf claim` → kb status `active`.
    Claimed,
    /// `hf checkpoint`/`hf handoff` → append a progress line (status unchanged).
    Progress(String),
    /// `hf done` → kb status `completed`, with an evidence progress line.
    Done(String),
    /// `hf release` (HFTASK-0038) → kb status `backlog` (revert an in-progress claim).
    Released,
}

/// True iff `correlation_id` is shaped like a kb slug (e.g. `tasks/foo`). The seam stamps the
/// slug as the card's `correlation_id`; handoff's own cards use `handoff-buildout` and intake
/// cards use a workflow UUID — neither contains '/', so they are never written back.
pub fn is_kb_slug(correlation_id: &str) -> bool {
    correlation_id.contains('/') && !correlation_id.trim().is_empty()
}

/// Pure: map a transition to the `git-kb set` field assignments + the commit message. Split
/// out so the write-back contract is unit-testable without git-kb. `+progress=` appends to the
/// frontmatter `progress` array (git-kb's `+field:value` array-add), so progress accrues.
pub fn writeback_args(slug: &str, t: &KbTransition) -> (Vec<String>, String) {
    match t {
        KbTransition::Claimed => (
            vec!["status=active".to_string()],
            format!("handoff write-back: {slug} claimed → active"),
        ),
        KbTransition::Progress(note) => (
            vec![format!("+progress={}", sanitize(note))],
            format!("handoff write-back: {slug} progress"),
        ),
        KbTransition::Done(evidence) => (
            vec![
                "status=completed".to_string(),
                format!("+progress=completed: {}", sanitize(evidence)),
            ],
            format!("handoff write-back: {slug} done → completed"),
        ),
        KbTransition::Released => (
            vec!["status=backlog".to_string()],
            format!("handoff write-back: {slug} released → backlog"),
        ),
    }
}

/// Collapse newlines/control chars so a progress line stays a single frontmatter value.
fn sanitize(s: &str) -> String {
    s.replace(['\n', '\r'], " ").trim().to_string()
}

/// Mirror a card transition back to its kb task (ADR-0003 rule 3). Best-effort + one-way:
/// returns `true` if the kb was updated, `false` if write-back did not apply (not a kb card,
/// no meta `.kb/`, the slug is not a live kb task, or git-kb is unavailable/failed).
pub fn write_back(correlation_id: &str, t: &KbTransition) -> bool {
    if !is_kb_slug(correlation_id) {
        return false;
    }
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(root) = kb_root(&repo_root) else {
        return false;
    };
    // Confirm the slug is a real kb task before mutating (and so a stray slug-shaped
    // correlation_id can never spuriously create kb churn).
    if run_kb_in(&root, &["show", correlation_id]).is_err() {
        return false;
    }
    let (sets, msg) = writeback_args(correlation_id, t);
    let mut argv: Vec<&str> = vec!["set", correlation_id];
    argv.extend(sets.iter().map(|s| s.as_str()));
    if run_kb_in(&root, &argv).is_err() {
        return false;
    }
    // Persist the workspace edit as a kb commit (set is workspace-first per git-kb).
    let _ = run_kb_in(&root, &["commit", "-m", &msg]);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\nid: 019ebd79\nslug: tasks/fleet-handoff-rollout\ntitle: \"Fleet .handoff rollout (P7)\"\ntype: task\nstatus: draft\npriority: high\n---\n\n## Overview\nRoll .handoff across the fleet.\n";

    #[test]
    fn parses_frontmatter_scalars() {
        assert_eq!(
            frontmatter_value(DOC, "title").as_deref(),
            Some("Fleet .handoff rollout (P7)")
        );
        assert_eq!(frontmatter_value(DOC, "priority").as_deref(), Some("high"));
        assert_eq!(frontmatter_value(DOC, "status").as_deref(), Some("draft"));
        assert_eq!(frontmatter_value(DOC, "missing"), None);
    }

    #[test]
    fn extracts_body_after_frontmatter() {
        let body = frontmatter_body(DOC);
        assert!(body.starts_with("## Overview"));
        assert!(body.contains("Roll .handoff across the fleet."));
        assert!(!body.contains("slug:")); // frontmatter excluded
    }

    #[test]
    fn slug_to_card_id_is_deterministic() {
        assert_eq!(
            card_id_from_slug("tasks/fleet-handoff-rollout"),
            "KBTASK-FLEET-HANDOFF-ROLLOUT"
        );
        assert_eq!(card_id_from_slug("add-providers"), "KBTASK-ADD-PROVIDERS");
    }

    #[test]
    fn priority_mapping() {
        assert_eq!(map_priority(Some("critical")), Priority::P0);
        assert_eq!(map_priority(Some("high")), Priority::P1);
        assert_eq!(map_priority(Some("medium")), Priority::P2);
        assert_eq!(map_priority(None), Priority::P2);
        assert_eq!(map_priority(Some("low")), Priority::P3);
    }

    #[test]
    fn mint_target_is_fleet_when_meta_root_exists() {
        let root = if cfg!(windows) {
            PathBuf::from("C:\\some\\meta\\root")
        } else {
            PathBuf::from("/some/meta/root")
        };
        // Not a local-kb mint → a meta-root slug routes to FLEET (never the cwd).
        let (dir, plane) = super::mint_target(false, Some(root.clone()));
        // FLEET tasks dir = <meta-root>/.handoff/tasks — NOT a cwd-relative path.
        assert_eq!(dir, root.join(crate::HF).join("tasks"));
        assert_eq!(plane, "FLEET");
        assert!(dir.is_absolute());
    }

    #[test]
    fn mint_target_falls_back_to_cwd_standalone() {
        let (dir, plane) = super::mint_target(false, None);
        // Standalone fallback = the cwd-relative local tasks dir (edge case only).
        assert_eq!(dir, crate::tasks_dir());
        assert!(plane.starts_with("standalone"));
    }

    #[test]
    fn mint_target_is_local_when_slug_came_from_repo_kb() {
        // HFTASK-0072: a slug read from the repo's OWN `.kb` lands in the LOCAL `.handoff/tasks`,
        // not FLEET — even when a meta root exists. local_kb wins over the meta root.
        let meta = if cfg!(windows) {
            PathBuf::from("C:\\some\\meta\\root")
        } else {
            PathBuf::from("/some/meta/root")
        };
        let (dir, plane) = super::mint_target(true, Some(meta));
        assert_eq!(dir, crate::tasks_dir());
        assert_eq!(plane, "LOCAL");
    }

    #[test]
    fn kb_root_prefers_local_kb_then_meta_then_none() {
        // HFTASK-0072: kb_root discovers the repo's OWN `.kb` first, the meta-root `.kb`
        // second, and returns None when neither exists. Build a fixture tree to prove it.
        let mut base = std::env::temp_dir();
        base.push(format!("hf-kbroot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let meta = base.join("meta");
        let repo = meta.join("handoff");
        std::fs::create_dir_all(&repo).unwrap();

        // (1) neither kb exists → None
        assert_eq!(super::kb_root(&repo), None);

        // (2) only the meta-root kb exists → meta root (the FLEET fallback, no downgrade)
        std::fs::create_dir_all(meta.join(".kb")).unwrap();
        assert_eq!(super::kb_root(&repo), Some(meta.clone()));

        // (3) the repo's OWN kb exists → the repo wins (the local planning plane)
        std::fs::create_dir_all(repo.join(".kb")).unwrap();
        assert_eq!(super::kb_root(&repo), Some(repo.clone()));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn minted_card_is_written_into_the_fleet_tasks_dir() {
        // End-to-end of the write step (no git-kb): build a card from a doc, resolve
        // the FLEET target against a fixture meta root, save it, and assert it landed
        // in <meta-root>/.handoff/tasks — never a cwd `.handoff/`.
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("hf-mint-{}-fleet", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let wo = work_order_from_kb_doc("tasks/demo-fleet", DOC);
        let (dir, plane) = super::mint_target(false, Some(tmp.clone()));
        crate::save_task_in(&dir, &wo);

        let expected = tmp
            .join(crate::HF)
            .join("tasks")
            .join(format!("{}.task.json", wo.id));
        assert!(expected.is_file(), "card not written to FLEET tasks dir");
        assert_eq!(plane, "FLEET");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mints_a_provable_card_with_correlation_id() {
        let wo = work_order_from_kb_doc("tasks/fleet-handoff-rollout", DOC);
        assert_eq!(wo.id, "KBTASK-FLEET-HANDOFF-ROLLOUT");
        assert_eq!(wo.correlation_id, "tasks/fleet-handoff-rollout");
        assert_eq!(wo.priority, Priority::P1);
        assert!(wo.objective.contains("Roll .handoff"));
        // intent_lock is computed so a downstream verifier can detect drift
        assert!(!wo.intent_lock.objective_hash.is_empty());
    }

    // --- HFTASK-0042 write-back ---

    #[test]
    fn only_kb_slug_correlation_ids_write_back() {
        // kb-minted cards carry a slug; handoff's own + intake cards do not.
        assert!(is_kb_slug("tasks/fleet-handoff-rollout"));
        assert!(is_kb_slug("context/overridable/active"));
        assert!(!is_kb_slug("handoff-buildout")); // handoff seed cards
        assert!(!is_kb_slug("550e8400-e29b-41d4-a716-446655440000")); // intake workflow uuid
        assert!(!is_kb_slug(""));
    }

    #[test]
    fn writeback_args_map_each_transition() {
        let slug = "tasks/demo";
        let (sets, msg) = writeback_args(slug, &KbTransition::Claimed);
        assert_eq!(sets, vec!["status=active"]);
        assert!(msg.contains("claimed → active"));

        let (sets, _) = writeback_args(slug, &KbTransition::Done("pr 64 merged".into()));
        assert_eq!(sets[0], "status=completed");
        assert!(sets[1].starts_with("+progress=completed: pr 64 merged"));

        // progress lines append (git-kb `+field` array-add) and stay single-line
        let (sets, _) = writeback_args(slug, &KbTransition::Progress("did x\nthen y".into()));
        assert_eq!(sets, vec!["+progress=did x then y"]);

        // HFTASK-0038 gap-hunt: release reverts a kb-minted card to backlog.
        let (sets, msg) = writeback_args(slug, &KbTransition::Released);
        assert_eq!(sets, vec!["status=backlog"]);
        assert!(msg.contains("released → backlog"));
    }

    #[test]
    fn write_back_is_noop_without_a_kb_card() {
        // A non-slug correlation_id never touches git-kb (returns false, no side effects).
        assert!(!write_back("handoff-buildout", &KbTransition::Claimed));
        assert!(!write_back("", &KbTransition::Done("x".into())));
    }
}
