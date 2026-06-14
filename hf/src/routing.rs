//! Highest-value safe-task routing — HFTASK-0018 (ADR-0012).
//!
//! `next_safe` picks the next task by **topological order** (first backlog whose deps are
//! done). This module adopts RuVector's **`ruvector-domain-expansion`** contextual Thompson
//! bandit (`transfer::{BetaParams, ContextBucket, ArmId}`) to instead pick the **highest-value
//! safe task per context** — exploration/exploitation over the ready candidates, not just
//! dependency order. Used by `hf claim --batch`.
//!
//! v1 (this cut): the value posterior is a **priority-context prior** (`ContextBucket` =
//! priority tier × role; `BetaParams` seeded so higher priority = stronger success prior),
//! Thompson-sampled per candidate. Updating posteriors from ledger outcome history (Bayesian
//! `update` on done/reopen) is the noted next increment — the seam is already here.

use rand::Rng;
use ruvector_domain_expansion::transfer::{ArmId, BetaParams, ContextBucket};
use work_order::{Priority, WorkOrder};

/// The context bucket of a task: difficulty tier from priority, category from role.
pub fn bucket_of(t: &WorkOrder) -> ContextBucket {
    ContextBucket {
        difficulty_tier: match t.priority {
            Priority::P0 => "p0",
            Priority::P1 => "p1",
            Priority::P2 => "p2",
            Priority::P3 => "p3",
        }
        .to_string(),
        category: t.role.clone().unwrap_or_else(|| "default".to_string()),
    }
}

/// Value posterior for a task's context: higher priority → stronger success prior (more
/// "value"). Seeds `BetaParams::from_observations(successes, failures)`.
fn prior_for(priority: Priority) -> BetaParams {
    let (successes, failures) = match priority {
        Priority::P0 => (8.0, 1.0),
        Priority::P1 => (5.0, 2.0),
        Priority::P2 => (3.0, 3.0),
        Priority::P3 => (1.0, 4.0),
    };
    BetaParams::from_observations(successes, failures)
}

/// The witnessed routing decision: which arm (task) won, its context, and the sampled value.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub arm: ArmId,
    pub bucket: ContextBucket,
    pub value: f32,
}

/// Thompson-sample each candidate's value posterior (shared per context bucket) and return
/// the highest-sampled task + its decision. Deterministic given `rng` (tests seed it; the
/// loop seeds from ledger size so a fresh sample is drawn as history grows).
pub fn route<'a>(
    candidates: &[&'a WorkOrder],
    rng: &mut impl Rng,
) -> Option<(&'a WorkOrder, RoutingDecision)> {
    use std::collections::HashMap;
    // One shared posterior per context bucket (the "contextual" part — same priority/role
    // tasks draw from the same value distribution).
    let mut posteriors: HashMap<ContextBucket, BetaParams> = HashMap::new();
    let mut best: Option<(&WorkOrder, RoutingDecision)> = None;
    for &t in candidates {
        let bucket = bucket_of(t);
        let beta = posteriors
            .entry(bucket.clone())
            .or_insert_with(|| prior_for(t.priority));
        let value = beta.sample(rng);
        let better = best.as_ref().map(|(_, d)| value > d.value).unwrap_or(true);
        if better {
            best = Some((
                t,
                RoutingDecision {
                    arm: ArmId(t.id.clone()),
                    bucket,
                    value,
                },
            ));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn task(id: &str, priority: Priority, role: Option<&str>) -> WorkOrder {
        let objective = format!("obj-{id}");
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec!["done".to_string()];
        let intent_lock = WorkOrder::compute_intent_lock(&objective, &path_scope, &acceptance);
        WorkOrder {
            schema: "handoff.task.v1".to_string(),
            id: id.to_string(),
            title: id.to_string(),
            status: work_order::Status::Backlog,
            priority,
            objective,
            path_scope,
            acceptance_criteria: acceptance,
            test_commands: vec![],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: String::new(),
            role: role.map(|r| r.to_string()),
            intent_lock,
        }
    }

    #[test]
    fn bucket_reflects_priority_and_role() {
        let t = task("T", Priority::P1, Some("implementer"));
        let b = bucket_of(&t);
        assert_eq!(b.difficulty_tier, "p1");
        assert_eq!(b.category, "implementer");
        // No role → default category.
        assert_eq!(
            bucket_of(&task("U", Priority::P2, None)).category,
            "default"
        );
    }

    #[test]
    fn higher_priority_has_higher_expected_value() {
        // The prior mean must be monotonic in priority (P0 > P1 > P2 > P3).
        let m = |p| prior_for(p).mean();
        assert!(m(Priority::P0) > m(Priority::P1));
        assert!(m(Priority::P1) > m(Priority::P2));
        assert!(m(Priority::P2) > m(Priority::P3));
    }

    #[test]
    fn route_returns_a_candidate_with_its_arm() {
        let a = task("HFTASK-A", Priority::P2, None);
        let b = task("HFTASK-B", Priority::P0, None);
        let cands = [&a, &b];
        let mut rng = StdRng::seed_from_u64(42);
        let (picked, decision) = route(&cands, &mut rng).expect("a candidate");
        assert_eq!(decision.arm.0, picked.id);
        assert!(cands.iter().any(|c| c.id == picked.id));
    }

    #[test]
    fn route_is_deterministic_for_a_fixed_seed() {
        let a = task("HFTASK-A", Priority::P2, None);
        let b = task("HFTASK-B", Priority::P1, None);
        let cands = [&a, &b];
        let pick = || {
            let mut rng = StdRng::seed_from_u64(7);
            route(&cands, &mut rng).unwrap().0.id.clone()
        };
        assert_eq!(pick(), pick());
    }

    #[test]
    fn route_favors_high_priority_over_many_draws() {
        // Over many seeds, P0 should win the majority vs P3 (exploitation dominates).
        let hi = task("HI", Priority::P0, None);
        let lo = task("LO", Priority::P3, None);
        let cands = [&lo, &hi];
        let mut hi_wins = 0;
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            if route(&cands, &mut rng).unwrap().0.id == "HI" {
                hi_wins += 1;
            }
        }
        assert!(
            hi_wins > 120,
            "P0 should win most of the time, got {hi_wins}/200"
        );
    }

    #[test]
    fn route_none_when_empty() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!(route(&[], &mut rng).is_none());
    }
}
