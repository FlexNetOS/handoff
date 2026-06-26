//! Highest-value safe-task routing — HFTASK-0018 (ADR-0012).
//!
//! `next_safe` picks the next task by **topological order** (first backlog whose deps are
//! done). This module adopts RuVector's **`ruvector-domain-expansion`** contextual Thompson
//! bandit (`transfer::{BetaParams, ContextBucket, ArmId}`) to instead pick the **highest-value
//! safe task per context** — exploration/exploitation over the ready candidates, not just
//! dependency order. Used by `hf claim --batch`.
//!
//! The value posterior is a **priority-context prior** (`ContextBucket` = priority tier ×
//! role; `BetaParams` seeded so higher priority = stronger success prior) that is then
//! **Bayesian-updated from real ledger outcomes** (HFTASK-0043): `done` = success, a
//! reopen/release back to Backlog = failure. So the bandit LEARNS — a context that keeps
//! failing is explored less, a proven one exploited more — closing the keystone ADR-0001
//! §5.5 T5 co-learning loop. The ledger is the outcome store (no extra persistence).

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

/// Outcome history per context bucket: `(successes, failures)` observed in the ledger.
/// HFTASK-0043: this is what makes the bandit LEARN — posteriors start from the priority
/// prior and are then Bayesian-updated by real outcomes (`done` = reward 1.0, a reopen/
/// release back to Backlog = reward 0.0), closing the keystone ADR-0001 §5.5 T5 co-learning
/// loop. The ledger IS the outcome store; no extra persistence.
pub type History = std::collections::HashMap<ContextBucket, (u32, u32)>;

/// A bucket's value posterior: the priority prior, then `BetaParams::update`d once per
/// observed outcome. With no history this is exactly the v1 prior (back-compatible).
fn posterior_for(priority: Priority, bucket: &ContextBucket, history: &History) -> BetaParams {
    let mut beta = prior_for(priority);
    if let Some(&(successes, failures)) = history.get(bucket) {
        for _ in 0..successes {
            beta.update(1.0);
        }
        for _ in 0..failures {
            beta.update(0.0);
        }
    }
    beta
}

/// The witnessed routing decision: which arm (task) won, its context, and the sampled value.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub arm: ArmId,
    pub bucket: ContextBucket,
    pub value: f32,
}

/// Thompson-sample each candidate's value posterior (shared per context bucket) and return
/// the highest-sampled task + its decision, seeding each bucket's posterior from observed
/// ledger outcomes (`history`) before sampling — so a context that has been failing is
/// explored less and a proven one exploited more (HFTASK-0043). Pass an empty `History` for
/// the v1 prior-only behavior. Deterministic given `rng`.
pub fn route_with_history<'a>(
    candidates: &[&'a WorkOrder],
    history: &History,
    rng: &mut impl Rng,
) -> Option<(&'a WorkOrder, RoutingDecision)> {
    use std::collections::HashMap;
    // One shared posterior per context bucket (the "contextual" part — same priority/role
    // tasks draw from the same value distribution), seeded by real outcome history.
    let mut posteriors: HashMap<ContextBucket, BetaParams> = HashMap::new();
    let mut best: Option<(&WorkOrder, RoutingDecision)> = None;
    for &t in candidates {
        let bucket = bucket_of(t);
        let beta = posteriors
            .entry(bucket.clone())
            .or_insert_with(|| posterior_for(t.priority, &bucket, history));
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
    use rand::SeedableRng;
    use rand::rngs::StdRng;

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
        let (picked, decision) =
            route_with_history(&cands, &History::new(), &mut rng).expect("a candidate");
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
            route_with_history(&cands, &History::new(), &mut rng)
                .unwrap()
                .0
                .id
                .clone()
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
            if route_with_history(&cands, &History::new(), &mut rng)
                .unwrap()
                .0
                .id
                == "HI"
            {
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
        assert!(route_with_history(&[], &History::new(), &mut rng).is_none());
    }

    #[test]
    fn outcomes_shift_the_posterior() {
        // HFTASK-0043: observed failures pull a context's expected value BELOW its prior;
        // observed successes push it ABOVE. This is the bandit learning from the ledger.
        let t = task("T", Priority::P1, Some("implementer"));
        let b = bucket_of(&t);
        let prior = prior_for(Priority::P1).mean();
        let mut fails = History::new();
        fails.insert(b.clone(), (0, 20));
        assert!(
            posterior_for(Priority::P1, &b, &fails).mean() < prior,
            "failures must lower the posterior mean"
        );
        let mut wins = History::new();
        wins.insert(b.clone(), (20, 0));
        assert!(
            posterior_for(Priority::P1, &b, &wins).mean() > prior,
            "successes must raise the posterior mean"
        );
    }

    #[test]
    fn history_makes_a_failing_context_lose_to_a_proven_one() {
        // Two P1 tasks in different role-contexts: the one whose context has been failing
        // should win far less than the one whose context has been succeeding.
        let proven = task("PROVEN", Priority::P1, Some("good"));
        let failing = task("FAILING", Priority::P1, Some("bad"));
        let cands = [&failing, &proven];
        let mut hist = History::new();
        hist.insert(bucket_of(&proven), (30, 0));
        hist.insert(bucket_of(&failing), (0, 30));
        let mut proven_wins = 0;
        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            if route_with_history(&cands, &hist, &mut rng).unwrap().0.id == "PROVEN" {
                proven_wins += 1;
            }
        }
        assert!(
            proven_wins > 150,
            "proven context should dominate, got {proven_wins}/200"
        );
    }
}
