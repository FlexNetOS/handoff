//! `work-order` — the handoff.task.v1 work-order envelope (S1 spike).
//!
//! Validates the front-door seam: a prompt_hub `SwarmBundle` is converted into one or
//! more provable `WorkOrder`s (handoff.task.v1). The `workflow_id` is carried as
//! `correlation_id` — the single cross-reference handle that closes gap #1 (task-truth)
//! and gap #3 (integration contract). Intent/scope/acceptance are hashed (blake3) so a
//! downstream verifier (`ruvector-verified`) can treat the order as a provable contract.

pub mod intake;
pub use intake::{synthesize_spec, Intent, SynthSpec};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Backlog,
    Active,
    Claimed,
    Blocked,
    Checkpointed,
    Review,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

/// The handoff.task.v1 envelope (mirrors `~/Downloads/tmp/handoff/handoff/schemas/task.schema.json`),
/// plus provenance fields that link it back to the front door and make it provable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkOrder {
    pub schema: String, // const "handoff.task.v1"
    pub id: String,     // ^TASK-[0-9]{4,}$
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub objective: String,
    pub path_scope: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub test_commands: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub allows_network: bool,
    #[serde(default)]
    pub allows_dependency_addition: bool,

    // --- provenance / contract extensions (S1) ---
    /// = SwarmBundle.workflow_id. The cross-ref handle weave Job.correlation_id syncs to.
    pub correlation_id: String,
    /// which role-prompt in the bundle minted this order (None = whole-bundle order).
    #[serde(default)]
    pub role: Option<String>,
    /// blake3 intent-lock (the drift sentinel anchor; ruvector-verified can prove against it).
    pub intent_lock: IntentLock,
}

/// blake3 hashes of the immutable contract surface — the .handoff drift-sentinel model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentLock {
    pub objective_hash: String,
    pub path_scope_hash: String,
    pub acceptance_hash: String,
}

fn b3(s: &str) -> String {
    format!("blake3:{}", blake3::hash(s.as_bytes()).to_hex())
}

impl WorkOrder {
    pub fn compute_intent_lock(
        objective: &str,
        path_scope: &[String],
        acceptance: &[String],
    ) -> IntentLock {
        IntentLock {
            objective_hash: b3(objective),
            path_scope_hash: b3(&path_scope.join("\n")),
            acceptance_hash: b3(&acceptance.join("\n")),
        }
    }

    /// Recompute the intent-lock from current fields and report whether it still matches
    /// (the core drift check: did objective/scope/acceptance mutate without a new order?).
    pub fn intent_unchanged(&self) -> bool {
        Self::compute_intent_lock(&self.objective, &self.path_scope, &self.acceptance_criteria)
            == self.intent_lock
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialize WorkOrder")
    }
}

// --- integration contract: mirror of prompt_hub's `SwarmBundle` ---
//
// Field-for-field against `prompt_hub/prompt-hub/src/models.rs:528`:
//   pub struct SwarmBundle {
//       pub workflow_id: Uuid,                      // 530
//       pub role_prompts: HashMap<Role, String>,    // 532
//       pub handoff_template: String,               // 534
//       pub consistency_report: Vec<Conflict>,      // 536
//       pub evolution_suggestions: Vec<String>,     // 538
//   }
//
// This is a *contract mirror*, not a path-dependency: prompt-hub's Cargo uses
// `version.workspace`/`edition.workspace` inheritance, so a path-dep from this workspace
// risks a workspace-inheritance build break (HFTASK-0003 research §A.1/§B.3). Mirroring the
// shape keeps the dependency a documented contract with no cross-repo build edge.
//
// Representation notes (decouple from upstream wire churn, stay deterministic):
//   - `workflow_id: Uuid` is carried as a `String` (the `correlation_id` handle).
//   - `role_prompts: HashMap<Role,String>` is carried as an ordered `Vec<(String,String)>`
//     (role token, prompt). A Vec — not a map — so intake order, and therefore minted ids,
//     are deterministic. `Role` is a string token (matches the enum's serde repr + `Custom`).
//   - `consistency_report: Vec<Conflict>` is reduced to `Vec<String>` (human-readable
//     conflict summaries) — the conflict detail is not needed to synthesize a WorkOrder.
//   - `#[serde(default)]` on the trailing three fields so older 3-field bundle JSON
//     (the S1 spike shape) still deserializes — backward compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmBundle {
    /// = prompt_hub `workflow_id: Uuid`, as string. Becomes each order's `correlation_id`.
    pub workflow_id: String,
    /// = prompt_hub `role_prompts: HashMap<Role,String>`, ordered for determinism.
    #[serde(default)]
    pub role_prompts: Vec<(String, String)>,
    /// = prompt_hub `handoff_template: String` (Handlebars skeleton).
    #[serde(default)]
    pub handoff_template: String,
    /// = prompt_hub `consistency_report: Vec<Conflict>`, reduced to summary strings.
    #[serde(default)]
    pub consistency_report: Vec<String>,
    /// = prompt_hub `evolution_suggestions: Vec<String>`.
    #[serde(default)]
    pub evolution_suggestions: Vec<String>,
}

/// THE SEAM: prompt_hub SwarmBundle -> Vec<WorkOrder> (one provable handoff.task.v1 per role).
///
/// Each order's verifiable fields (`path_scope`, `acceptance_criteria`, `test_commands`) are
/// **synthesized deterministically** from a vibe `Intent` via [`synthesize_spec`] — closing
/// the HFTASK-0003 gap where the spike emitted `path_scope: ["."]` + `test_commands: []`
/// (unverifiable by the drift gate). The per-role Intent is, in precedence:
///   1. `intent_override` (the `--vibe`/`--intent` whole-bundle intent), else
///   2. `Intent::classify(role_prompt)` (deterministic, mirrors prompt_hub's classifier).
///
/// `objective = "<TaskType>: <prompt>"` (≥10 chars, schema minLength), `correlation_id =
/// workflow_id` (the cross-ref handle), and `intent_lock` is computed over the synthesized
/// triple. Pure: same `(bundle, intent_override, scope_override)` → byte-identical orders.
pub fn work_orders_from_bundle_with(
    bundle: &SwarmBundle,
    intent_override: Option<&Intent>,
    scope_override: Option<&[String]>,
) -> Vec<WorkOrder> {
    bundle
        .role_prompts
        .iter()
        .enumerate()
        .map(|(i, (role, prompt))| {
            let id = format!("TASK-{:04}", i + 1);
            let classified;
            let intent = match intent_override {
                Some(it) => it,
                None => {
                    classified = Intent::classify(prompt);
                    &classified
                }
            };
            let spec = synthesize_spec(intent, Some(role), scope_override);
            let objective = compose_objective(&intent.task_type, prompt);
            let intent_lock = WorkOrder::compute_intent_lock(
                &objective,
                &spec.path_scope,
                &spec.acceptance_criteria,
            );
            WorkOrder {
                schema: "handoff.task.v1".to_string(),
                id,
                title: format!("[{role}] {}", first_line(prompt)),
                status: Status::Backlog,
                priority: Priority::P1,
                objective,
                path_scope: spec.path_scope,
                acceptance_criteria: spec.acceptance_criteria,
                test_commands: spec.test_commands,
                dependencies: vec![],
                blocked_by: vec![],
                allows_network: false,
                allows_dependency_addition: false,
                correlation_id: bundle.workflow_id.clone(),
                role: Some(role.clone()),
                intent_lock,
            }
        })
        .collect()
}

/// Back-compat convenience: synthesize with a per-role classified Intent and default scope.
pub fn work_orders_from_bundle(bundle: &SwarmBundle) -> Vec<WorkOrder> {
    work_orders_from_bundle_with(bundle, None, None)
}

/// Compose a schema-valid objective (`minLength: 10`) from the task_type + prompt. When the
/// prompt is empty (prod `role_prompts` can be empty) a descriptive fallback is used.
fn compose_objective(task_type: &str, prompt: &str) -> String {
    let p = prompt.trim();
    let composed = if p.is_empty() {
        format!("{task_type}: work order synthesized from SwarmBundle (no role prompt)")
    } else {
        let verb = task_type
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + &task_type[1..])
            .unwrap_or_else(|| task_type.to_string());
        format!("{verb}: {p}")
    };
    if composed.len() < 10 {
        format!("{composed} (handoff work order)")
    } else {
        composed
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").chars().take(60).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_bundle() -> SwarmBundle {
        SwarmBundle {
            workflow_id: "wf-0001".to_string(),
            role_prompts: vec![
                (
                    "architect".to_string(),
                    "Design the storefront schema in rust".to_string(),
                ),
                (
                    "coder".to_string(),
                    "Implement the checkout flow in rust".to_string(),
                ),
            ],
            handoff_template: "standard".to_string(),
            consistency_report: vec![],
            evolution_suggestions: vec![],
        }
    }

    #[test]
    fn seam_bundle_to_workorders() {
        let orders = work_orders_from_bundle(&sample_bundle());
        assert_eq!(orders.len(), 2);
        // every order carries the workflow_id as correlation_id (the cross-ref handle)
        assert!(orders.iter().all(|o| o.correlation_id == "wf-0001"));
        assert_eq!(orders[0].id, "TASK-0001");
        assert_eq!(orders[0].role.as_deref(), Some("architect"));
        assert_eq!(orders[0].schema, "handoff.task.v1");
    }

    #[test]
    fn synthesized_orders_are_verifiable_no_junk_defaults() {
        // HFTASK-0003 acceptance #1: never path_scope ["."], never empty test_commands.
        for o in work_orders_from_bundle(&sample_bundle()) {
            assert!(!o.path_scope.is_empty());
            assert!(
                !o.path_scope.iter().any(|s| s == "." || s == "./"),
                "{}: path_scope must be narrower than repo root, got {:?}",
                o.id,
                o.path_scope
            );
            assert!(
                !o.test_commands.is_empty(),
                "{}: test_commands must be non-empty",
                o.id
            );
            // rust prompts → cargo test present
            assert!(o.test_commands.iter().any(|c| c == "cargo test"));
            // objective satisfies schema minLength 10
            assert!(o.objective.len() >= 10);
            // acceptance is non-empty and intent_lock is fresh
            assert!(!o.acceptance_criteria.is_empty());
            assert!(o.intent_unchanged());
        }
    }

    #[test]
    fn intake_is_deterministic_same_ids_same_locks() {
        // HFTASK-0003 acceptance #3: re-running yields identical ids + intent_lock hashes.
        let a = work_orders_from_bundle(&sample_bundle());
        let b = work_orders_from_bundle(&sample_bundle());
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.id, y.id);
            assert_eq!(x.intent_lock, y.intent_lock);
            assert_eq!(x.correlation_id, y.correlation_id);
            assert_eq!(x.objective, y.objective);
        }
    }

    #[test]
    fn intent_lock_detects_drift() {
        let mut o = work_orders_from_bundle(&sample_bundle()).remove(0);
        assert!(o.intent_unchanged(), "fresh order must match its lock");
        o.objective = "Redesign the entire architecture".to_string(); // goal drift
        assert!(!o.intent_unchanged(), "objective drift must be detected");
    }

    #[test]
    fn roundtrips_through_json() {
        let o = work_orders_from_bundle(&sample_bundle()).remove(0);
        let j = o.to_json();
        let back: WorkOrder = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id, o.id);
        assert_eq!(back.intent_lock, o.intent_lock);
    }
}
