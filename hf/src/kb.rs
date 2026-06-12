//! kb ↔ handoff seam (ADR-0003): mint handoff cards FROM git-kb tasks — the planning plane
//! (git-kb) feeds the execution plane (.handoff). Read-only on the kb; writes only a local
//! card, stamping `correlation_id = <slug>` as the cross-reference handle. One-way by
//! construction (the kb is never read back as execution truth).

use std::path::{Path, PathBuf};
use std::process::Command;

use work_order::{Priority, Status, WorkOrder};

/// The meta workspace root holding `.kb/` (where `git-kb` operates). `None` standalone.
fn kb_root(repo_root: &Path) -> Option<PathBuf> {
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

/// `hf task mint --from-kb <slug>` — mint a handoff card from a kb task (planning → execution).
pub fn cmd_mint_from_kb(slug: &str) {
    if slug.is_empty() {
        eprintln!("usage: hf task mint --from-kb <kb-slug>");
        return;
    }
    let repo_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let Some(root) = kb_root(&repo_root) else {
        eprintln!("hf task mint: no meta `.kb/` found (need a meta workspace) — cannot mint");
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
    crate::save_task(&wo);
    println!("hf task mint: {id} minted from kb {slug} (correlation_id = kb_ref = {slug})");
    println!("  next: hf claim {id}");
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
    fn mints_a_provable_card_with_correlation_id() {
        let wo = work_order_from_kb_doc("tasks/fleet-handoff-rollout", DOC);
        assert_eq!(wo.id, "KBTASK-FLEET-HANDOFF-ROLLOUT");
        assert_eq!(wo.correlation_id, "tasks/fleet-handoff-rollout");
        assert_eq!(wo.priority, Priority::P1);
        assert!(wo.objective.contains("Roll .handoff"));
        // intent_lock is computed so a downstream verifier can detect drift
        assert!(!wo.intent_lock.objective_hash.is_empty());
    }
}
