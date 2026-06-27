// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Front-door intake/dispatch verbs (HFTASK-0003).
//!
//! HFTASK-0083 (ADR-0019 D5 #4): the FOURTH and last named coupled module, peeled into
//! `handoff-intake`. The one dispatch coupling (`crate::cmd_claim_with`) was inverted — `cmd_dispatch`
//! now takes a `claim` closure the hf binary injects (capturing the weave leaser), so intake no
//! longer reaches back into the binary. `hf` aliases it as `intake`. Deps: handoff-core + work-order.
//!
//! `hf intake --bundle <file>` parses a real-shape prompt_hub `SwarmBundle` (mirrored as the
//! integration contract in `work_order`), deterministically synthesizes a verifiable
//! `WorkOrder` per role via `work_order::synthesize_spec`, and persists each as a
//! `.handoff/tasks/<id>.task.json` card (via `handoff_core::save_task`). `hf dispatch <workflow_id>`
//! claims/activates the synthesized orders for a workflow by the `correlation_id` join.
//!
//! IO-only: all synthesis logic is pure in the `work_order` crate (unit-tested there). This
//! module reads a file (no network — honors `allows_network: false`) and writes cards.

use std::path::PathBuf;

use work_order::{Intent, SwarmBundle, WorkOrder, work_orders_from_bundle_with};

/// Parse a SwarmBundle from JSON file content. Pure (testable without IO).
pub fn parse_bundle(json: &str) -> Result<SwarmBundle, String> {
    serde_json::from_str::<SwarmBundle>(json).map_err(|e| format!("invalid SwarmBundle JSON: {e}"))
}

/// Parse an explicit Intent from JSON file content. Pure.
pub fn parse_intent(json: &str) -> Result<Intent, String> {
    serde_json::from_str::<Intent>(json).map_err(|e| format!("invalid Intent JSON: {e}"))
}

/// Resolve the whole-bundle Intent override from flags (precedence: --intent file > --vibe
/// classification > none). `None` means: classify each role prompt individually downstream.
fn resolve_intent(vibe: Option<&str>, intent_json: Option<&str>) -> Result<Option<Intent>, String> {
    if let Some(path) = intent_json {
        let s = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read --intent {path}: {e}"))?;
        return Ok(Some(parse_intent(&s)?));
    }
    if let Some(text) = vibe {
        return Ok(Some(Intent::classify(text)));
    }
    Ok(None)
}

/// Synthesize the work orders for a bundle + optional intent/scope. Pure (no IO). Exposed so
/// tests can assert the verifiable-card invariants without touching the filesystem.
pub fn synthesize_orders(
    bundle: &SwarmBundle,
    intent: Option<&Intent>,
    scope: Option<&[String]>,
) -> Result<Vec<WorkOrder>, String> {
    if bundle.role_prompts.is_empty() {
        // Prod reality: role_prompts is empty. With a whole-bundle intent we can still mint a
        // single order; without one we refuse rather than emit a junk card.
        match intent {
            Some(it) => {
                // Synthesize one whole-bundle order (role: None) from the intent.
                let synth = work_order::synthesize_spec(it, None, scope);
                let objective = if it.raw_text.trim().len() >= 10 {
                    it.raw_text.trim().to_string()
                } else {
                    format!(
                        "{}: whole-bundle work order (workflow {})",
                        it.task_type, bundle.workflow_id
                    )
                };
                let intent_lock = WorkOrder::compute_intent_lock(
                    &objective,
                    &synth.path_scope,
                    &synth.acceptance_criteria,
                );
                Ok(vec![WorkOrder {
                    schema: "handoff.task.v1".to_string(),
                    // HFTASK-0084: derive a bundle-scoped id (was a hardcoded "TASK-0001" that
                    // silently clobbered any existing TASK-0001 card on a second intake run).
                    id: work_order::synthesized_task_id(&bundle.workflow_id, 1),
                    title: format!("[bundle] {}", first_line(&it.raw_text)),
                    status: work_order::Status::Backlog,
                    priority: work_order::Priority::P1,
                    objective,
                    path_scope: synth.path_scope,
                    acceptance_criteria: synth.acceptance_criteria,
                    test_commands: synth.test_commands,
                    dependencies: vec![],
                    blocked_by: vec![],
                    allows_network: false,
                    allows_dependency_addition: false,
                    correlation_id: bundle.workflow_id.clone(),
                    role: None,
                    intent_lock,
                }])
            }
            None => Err(
                "bundle has empty role_prompts and no --vibe/--intent given — cannot synthesize \
                 a work order from nothing (provide --vibe \"<request>\" or --intent <file>)"
                    .to_string(),
            ),
        }
    } else {
        Ok(work_orders_from_bundle_with(bundle, intent, scope))
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(60).collect()
}

/// `hf intake --bundle <file> [--vibe "<text>"] [--intent <file>] [--scope a,b]`.
pub fn cmd_intake(
    bundle_path: Option<&str>,
    vibe: Option<&str>,
    intent_json: Option<&str>,
    scope: Option<&[String]>,
) {
    let Some(bundle_path) = bundle_path else {
        eprintln!(
            "usage: hf intake --bundle <bundle.json> [--vibe \"<text>\"] [--intent <intent.json>] [--scope glob,glob]"
        );
        return;
    };
    let raw = match std::fs::read_to_string(bundle_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("hf intake: cannot read --bundle {bundle_path}: {e}");
            return;
        }
    };
    let bundle = match parse_bundle(&raw) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("hf intake: {e}");
            return;
        }
    };
    let intent = match resolve_intent(vibe, intent_json) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("hf intake: {e}");
            return;
        }
    };
    let orders = match synthesize_orders(&bundle, intent.as_ref(), scope) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("hf intake: {e}");
            return;
        }
    };
    if orders.is_empty() {
        eprintln!("hf intake: no orders synthesized (empty bundle?)");
        return;
    }
    let mut ids = Vec::new();
    for wo in &orders {
        handoff_core::save_task(wo);
        ids.push(wo.id.clone());
    }
    println!(
        "hf intake: minted {} order(s) from workflow {} -> {}",
        ids.len(),
        bundle.workflow_id,
        ids.join(", ")
    );
    println!("  correlation_id = {} (workflow_id)", bundle.workflow_id);
    println!("  next: hf dispatch {}", bundle.workflow_id);
}

/// `hf dispatch <correlation_id|workflow_id> [--next]` — claim/activate the synthesized
/// orders for a workflow by the `correlation_id` join. `--next` claims only the first
/// unclaimed order (one at a time); default claims all orders for the workflow.
/// `claim` is the witnessed claim path injected by the caller (HFTASK-0083: this inverts the old
/// `crate::cmd_claim_with` dependency so intake no longer reaches back into the hf binary). It
/// claims one order by id and returns whether the claim succeeded (a blocked order returns false
/// and is skipped, NOT exited — HFTASK-0029 Defect C).
pub fn cmd_dispatch(correlation_id: Option<&str>, next_only: bool, claim: &dyn Fn(&str) -> bool) {
    let Some(cid) = correlation_id.filter(|c| !c.is_empty() && !c.starts_with("--")) else {
        eprintln!("usage: hf dispatch <correlation_id|workflow_id> [--next]");
        return;
    };
    let tasks = handoff_core::load_tasks();
    let mut matching: Vec<&WorkOrder> = tasks.iter().filter(|t| t.correlation_id == cid).collect();
    matching.sort_by(|a, b| a.id.cmp(&b.id));
    if matching.is_empty() {
        eprintln!(
            "hf dispatch: no orders with correlation_id '{cid}' — run `hf intake --bundle <file>` first"
        );
        return;
    }
    let to_claim: Vec<&WorkOrder> = if next_only {
        matching.into_iter().take(1).collect()
    } else {
        matching
    };
    // Reuse the caller's witnessed claim path (lease + ledger transition). A blocked order is
    // skipped (claim returns false) so dispatch keeps claiming the rest of the workflow's orders.
    let mut dispatched = 0usize;
    for wo in &to_claim {
        if claim(&wo.id) {
            dispatched += 1;
        }
    }
    println!("hf dispatch: dispatched {dispatched} order(s) for workflow {cid}");
}

/// Resolve a default bundle path under `.handoff/` (used only by the help text / future seam).
#[allow(dead_code)]
fn default_bundle_path() -> PathBuf {
    std::path::Path::new(handoff_core::HF)
        .join("bundles")
        .join("incoming.bundle.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE_JSON: &str = r#"{
        "workflow_id": "wf-front-door-0003",
        "role_prompts": [
            ["architect", "Design the intake schema in rust"],
            ["coder", "Fix the panic in the rust ledger replay"]
        ],
        "handoff_template": "standard",
        "consistency_report": [],
        "evolution_suggestions": ["consider adding a dispatch verb"]
    }"#;

    #[test]
    fn parses_real_shape_five_field_bundle() {
        let b = parse_bundle(BUNDLE_JSON).expect("parse");
        assert_eq!(b.workflow_id, "wf-front-door-0003");
        assert_eq!(b.role_prompts.len(), 2);
        assert_eq!(b.evolution_suggestions.len(), 1);
    }

    #[test]
    fn parses_legacy_three_field_bundle() {
        // backward compat: trailing fields default
        let legacy = r#"{"workflow_id":"wf-x","role_prompts":[["coder","build it in rust"]],"handoff_template":"std"}"#;
        let b = parse_bundle(legacy).expect("parse legacy");
        assert_eq!(b.workflow_id, "wf-x");
        assert!(b.consistency_report.is_empty());
    }

    #[test]
    fn synthesized_orders_pass_the_gate_invariants() {
        let b = parse_bundle(BUNDLE_JSON).unwrap();
        let orders = synthesize_orders(&b, None, None).unwrap();
        assert_eq!(orders.len(), 2);
        for o in &orders {
            // acceptance #2: never repo-root scope, never empty test_commands, objective >= 10
            assert!(!o.path_scope.iter().any(|s| s == "."));
            assert!(!o.path_scope.is_empty());
            assert!(!o.test_commands.is_empty());
            assert!(o.objective.len() >= 10);
            assert!(!o.acceptance_criteria.is_empty());
            // acceptance #3 (per-order): correlation_id == workflow_id, fresh lock
            assert_eq!(o.correlation_id, "wf-front-door-0003");
            assert!(o.intent_unchanged());
        }
        // rust prompts → cargo test present
        assert!(orders[0].test_commands.iter().any(|c| c == "cargo test"));
    }

    #[test]
    fn intake_is_deterministic() {
        let b = parse_bundle(BUNDLE_JSON).unwrap();
        let a = synthesize_orders(&b, None, None).unwrap();
        let c = synthesize_orders(&b, None, None).unwrap();
        for (x, y) in a.iter().zip(c.iter()) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.intent_lock, y.intent_lock);
        }
    }

    #[test]
    fn empty_role_prompts_without_intent_is_refused() {
        let empty = r#"{"workflow_id":"wf-empty","role_prompts":[],"handoff_template":""}"#;
        let b = parse_bundle(empty).unwrap();
        let err = synthesize_orders(&b, None, None).unwrap_err();
        assert!(err.contains("empty role_prompts"));
    }

    #[test]
    fn empty_role_prompts_with_vibe_mints_one_order() {
        let empty = r#"{"workflow_id":"wf-empty","role_prompts":[],"handoff_template":""}"#;
        let b = parse_bundle(empty).unwrap();
        let it = Intent::classify("fix the rust regression in the ledger");
        let orders = synthesize_orders(&b, Some(&it), None).unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].role, None);
        assert!(!orders[0].test_commands.is_empty());
        assert!(!orders[0].path_scope.iter().any(|s| s == "."));
        assert!(orders[0].objective.len() >= 10);
        assert!(orders[0].intent_unchanged());
    }

    #[test]
    fn scope_override_flows_through() {
        let b = parse_bundle(BUNDLE_JSON).unwrap();
        let scope = vec!["spike/**".to_string()];
        let orders = synthesize_orders(&b, None, Some(&scope)).unwrap();
        assert_eq!(orders[0].path_scope, vec!["spike/**".to_string()]);
    }

    #[test]
    fn default_bundle_path_is_not_under_tasks_dir() {
        let path = default_bundle_path();
        assert_eq!(
            path,
            std::path::Path::new(handoff_core::HF)
                .join("bundles")
                .join("incoming.bundle.json")
        );
        assert!(
            !path.starts_with(handoff_core::tasks_dir()),
            "bundle JSON is not a task card and must not live under tasks/: {}",
            path.display()
        );
    }
}
