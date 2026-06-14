# ADR-0012 — Highest-value safe-task routing via ruvector-domain-expansion

- **Status:** Accepted
- **Date:** 2026-06-14
- **Task:** HFTASK-0018
- **Adopts:** `ruvector-domain-expansion` (RuVector crate — crates-only)
- **Pillar:** Capability Gain (the loop should pick the *best* safe task, not just the first)

## Context

`next_safe` (`hf/src/main.rs`) selects the next task by **topological order**: resume any
in-progress task, else the *first* backlog card whose dependencies are all `Done`. That is
correct but value-blind — among several ready safe tasks it always takes the lowest id, never
"the highest-value one for the current context." ADR-0001 R13 / RUVECTOR-RUNBOOK S1 §2 mapped
this need onto RuVector's **`ruvector-domain-expansion`** — a contextual-bandit / Thompson
router (the `domain-expansion` "trap" crate: it is a task router, not a domain generator).

## Decision

Adopt `ruvector-domain-expansion`'s **contextual Thompson bandit** primitive
(`transfer::{BetaParams, ContextBucket, ArmId}`) to pick the **highest-value safe task per
context**, exposed as **`hf claim --batch`**.

- **Dependency (crates-only):** `ruvector-domain-expansion = { path =
  "../../RuVector/crates/ruvector-domain-expansion", default-features = false }` + `rand =
  "0.8"` (matches RuVector's `rand`). default-features is empty (RVF integration stays
  optional), so the dep tree is just `serde`/`serde_json`/`thiserror`/`rand`. Path dep,
  standalone-CI-safe — handoff CI already clones `meta-ruvector` (see ADR-0011).
- **Model (`hf/src/routing.rs`):** each ready candidate is an **arm** (`ArmId(task.id)`),
  bucketed by **context** = priority tier × role (`ContextBucket{difficulty_tier, category}`).
  Each bucket carries a `BetaParams` value posterior; `route()` Thompson-**samples** each
  candidate's posterior and returns the highest — exploration/exploitation, not argmax.
- **`hf claim --batch`:** resumes an in-progress task if one exists (same precedence as
  `next_safe`); otherwise routes over the ready backlog candidates and claims the winner. The
  rng is seeded from the witnessed event count, so the draw is reproducible for a given ledger
  state and re-explores as history grows. The plain `hf claim <id>` path is unchanged.

### v1 scope (and the seam left for v2)

This cut seeds the value posterior from a **priority-context prior** (P0 → strong success
prior … P3 → weak), so routing is "prefer higher-priority ready tasks, with Thompson
exploration across contexts." The crate's `BetaParams::update(reward)` Bayesian step is
**not yet** wired to ledger outcomes (done = success, reopen = failure) — that posterior-from-
history learning is the noted next increment; `route()` already buckets per context so the
update seam drops in without an interface change.

## Consequences

- **+** The loop can pick the highest-value safe task, the core of an autonomous selector —
  via the real RuVector router, not a hand-rolled heuristic.
- **+** Crates-only, CI-safe, light dep tree; opt-in (`--batch`) so the deterministic
  `next_safe` path and all existing callers are untouched.
- **+** Deterministic + unit-tested (priority-monotonic posteriors, seeded reproducibility,
  P0-wins-the-majority over 200 draws, arm identity, empty-set).
- **−** v1 routing is priority-prior-only; it does not yet *learn* from outcomes. Mitigated:
  the contextual-bucket structure and `update` seam are in place for the v2 increment.
- **−** Thompson sampling is stochastic; reproducibility comes from the ledger-seeded rng, not
  from determinism of the algorithm itself.

## Alternatives considered

- **Keep `next_safe` only** — rejected: value-blind topological order isn't autonomous
  selection (the explicit R13 gap).
- **Hand-roll a priority sort** — rejected: duplicates what the mapped RuVector crate already
  provides (no-downgrade); loses the contextual-bandit exploration and the learning seam.
- **Use the full `DomainExpansionEngine`** — rejected: it is a heavyweight domain/policy-
  evolution system; only the `transfer` bandit primitive fits task routing.
