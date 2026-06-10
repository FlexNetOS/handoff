//! `work-order` — the handoff.task.v1 work-order envelope (S1 spike).
//!
//! Validates the front-door seam: a prompt_hub `SwarmBundle` is converted into one or
//! more provable `WorkOrder`s (handoff.task.v1). The `workflow_id` is carried as
//! `correlation_id` — the single cross-reference handle that closes gap #1 (task-truth)
//! and gap #3 (integration contract). Intent/scope/acceptance are hashed (blake3) so a
//! downstream verifier (`ruvector-verified`) can treat the order as a provable contract.

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
    pub fn compute_intent_lock(objective: &str, path_scope: &[String], acceptance: &[String]) -> IntentLock {
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

// --- minimal mirror of prompt_hub's SwarmBundle (prompt_hub/prompt-hub/src/models.rs) ---
// Real: { workflow_id: Uuid, role_prompts: HashMap<Role,String>, handoff_template,
//         consistency_report: Vec<Conflict>, evolution_suggestions: Vec<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmBundle {
    pub workflow_id: String, // Uuid as string for the spike
    pub role_prompts: Vec<(String, String)>, // (role, prompt) — HashMap flattened for determinism
    pub handoff_template: String,
}

/// THE SEAM: prompt_hub SwarmBundle -> Vec<WorkOrder> (one provable handoff.task.v1 per role).
/// This is the single connector gap #2 (front door) and gap #3 (envelope) meet at.
pub fn work_orders_from_bundle(bundle: &SwarmBundle) -> Vec<WorkOrder> {
    bundle
        .role_prompts
        .iter()
        .enumerate()
        .map(|(i, (role, prompt))| {
            let id = format!("TASK-{:04}", i + 1);
            let path_scope = vec![".".to_string()];
            let acceptance = vec![format!("{role} deliverable accepted via test_commands + drift audit")];
            let objective = prompt.clone();
            let intent_lock = WorkOrder::compute_intent_lock(&objective, &path_scope, &acceptance);
            WorkOrder {
                schema: "handoff.task.v1".to_string(),
                id,
                title: format!("[{role}] {}", first_line(prompt)),
                status: Status::Backlog,
                priority: Priority::P1,
                objective,
                path_scope,
                acceptance_criteria: acceptance,
                test_commands: vec![],
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
                ("architect".to_string(), "Design the storefront schema".to_string()),
                ("coder".to_string(), "Implement the checkout flow".to_string()),
            ],
            handoff_template: "standard".to_string(),
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
