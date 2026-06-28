//! Front-door prompt_hub seam (HFTASK-0022).
//!
//! `hf prompt-hub "<vibe>"` turns a natural-language request into a verifiable handoff
//! `WorkOrder` via the existing intake pipeline (HFTASK-0003). It is the RuVocal chat
//! surface's input endpoint: a vibe comes in, a bundle + task card are minted, and (with
//! `--dispatch`) the first safe order is claimed immediately. The resulting
//! `correlation_id` can be round-tripped through `hf status` and `hf delivery` to surface
//! loop state and delivery back in chat (HFTASK-0020 / HFTASK-0021).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use work_order::{Intent, SwarmBundle};

use crate::intake;

/// Deterministic 64-bit FNV-1a hash used to derive a stable, vibe-derived handle.
fn stable_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn bundle_inbox_dir() -> PathBuf {
    Path::new(crate::HF).join("bundles")
}

fn prompt_hub_bundle_path(workflow_id: &str) -> PathBuf {
    bundle_inbox_dir().join(format!("prompt_hub.{workflow_id}.bundle.json"))
}

/// `hf prompt-hub "<vibe>" [--scope glob,glob] [--dispatch] [--json]`
///
/// Generates a `SwarmBundle` with empty `role_prompts` (production reality), classifies the
/// vibe as the whole-bundle intent, mints one `WorkOrder`, and optionally dispatches the
/// first order from the bundle.
pub fn cmd_prompt_hub(vibe: &str, scope: Option<&[String]>, dispatch: bool, json: bool) {
    if vibe.trim().is_empty() {
        eprintln!("hf prompt-hub: vibe cannot be empty");
        return;
    }

    let workflow_id = format!("vibe-{:016x}-{}", stable_hash(vibe), now_ns());
    let bundle = SwarmBundle {
        workflow_id: workflow_id.clone(),
        role_prompts: vec![],
        handoff_template: "standard".to_string(),
        consistency_report: vec![],
        evolution_suggestions: vec![],
    };

    let bundle_dir = bundle_inbox_dir();
    if let Err(e) = std::fs::create_dir_all(&bundle_dir) {
        eprintln!("hf prompt-hub: cannot create bundle inbox: {e}");
        return;
    }

    let bundle_path = prompt_hub_bundle_path(&workflow_id);
    let bundle_json = match serde_json::to_string_pretty(&bundle) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("hf prompt-hub: bundle serialization failed: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(&bundle_path, bundle_json) {
        eprintln!("hf prompt-hub: cannot write bundle: {e}");
        return;
    }

    let intent = Intent::classify(vibe);
    let orders = match intake::synthesize_orders(&bundle, Some(&intent), scope) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("hf prompt-hub: {e}");
            return;
        }
    };

    let mut ids = Vec::new();
    for wo in &orders {
        crate::save_task(wo);
        ids.push(wo.id.clone());
    }

    let dispatched = if dispatch && !ids.is_empty() {
        let before: std::collections::HashSet<String> = crate::load_tasks()
            .into_iter()
            .filter(|t| t.correlation_id == workflow_id && t.status == work_order::Status::Claimed)
            .map(|t| t.id)
            .collect();
        // HFTASK-0083: inject the witnessed claim path (see the main dispatch arm).
        let leaser = crate::lease::WeaveCli::from_env();
        intake::cmd_dispatch(Some(&workflow_id), true, &|id| {
            crate::cmd_claim_with(id, &leaser)
        });
        let after: std::collections::HashSet<String> = crate::load_tasks()
            .into_iter()
            .filter(|t| t.correlation_id == workflow_id && t.status == work_order::Status::Claimed)
            .map(|t| t.id)
            .collect();
        after.difference(&before).count()
    } else {
        0
    };

    if json {
        let out = serde_json::json!({
            "schema": "handoff.prompt_hub.v1",
            "correlation_id": workflow_id,
            "bundle_path": bundle_path,
            "minted": ids,
            "dispatched": dispatched,
        });
        println!("{}", crate::pretty_json(&out));
    } else {
        println!("hf prompt-hub: minted {} order(s) from vibe", ids.len());
        println!("  correlation_id = {workflow_id}");
        println!("  bundle_path    = {}", bundle_path.display());
        println!("  minted         = {}", ids.join(", "));
        if dispatch {
            println!("  dispatched     = {dispatched} order(s)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_is_deterministic() {
        let a = stable_hash("fix the windows test");
        let b = stable_hash("fix the windows test");
        let c = stable_hash("fix the windows tests");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn vibe_bundle_has_empty_role_prompts_and_workflow_id() {
        let vibe = "design a front door";
        let workflow_id = format!("vibe-{:016x}-{}", stable_hash(vibe), 0);
        let bundle = SwarmBundle {
            workflow_id: workflow_id.clone(),
            role_prompts: vec![],
            handoff_template: "standard".to_string(),
            consistency_report: vec![],
            evolution_suggestions: vec![],
        };
        assert!(bundle.role_prompts.is_empty());
        assert_eq!(bundle.workflow_id, workflow_id);
    }

    #[test]
    fn synthesize_from_vibe_mints_one_order() {
        let vibe = "fix the rust regression in the ledger";
        let workflow_id = format!("vibe-{:016x}-{}", stable_hash(vibe), 0);
        let bundle = SwarmBundle {
            workflow_id,
            role_prompts: vec![],
            handoff_template: "standard".to_string(),
            consistency_report: vec![],
            evolution_suggestions: vec![],
        };
        let intent = Intent::classify(vibe);
        let orders = intake::synthesize_orders(&bundle, Some(&intent), None).unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].role, None);
        assert!(!orders[0].test_commands.is_empty());
        assert!(!orders[0].path_scope.iter().any(|s| s == "."));
        assert!(orders[0].objective.len() >= 10);
        assert!(orders[0].intent_unchanged());
    }

    #[test]
    fn prompt_hub_bundle_path_is_not_under_tasks_dir() {
        let path = prompt_hub_bundle_path("wf-123");
        assert_eq!(
            path,
            Path::new(crate::HF)
                .join("bundles")
                .join("prompt_hub.wf-123.bundle.json")
        );
        assert!(
            !path.starts_with(crate::tasks_dir()),
            "bundle JSON is not a task card and must not live under tasks/: {}",
            path.display()
        );
    }
}
