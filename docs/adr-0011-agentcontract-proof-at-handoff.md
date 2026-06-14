# ADR-0011 — ruvector-verified AgentContract proof at `hf handoff`

- **Status:** Accepted
- **Date:** 2026-06-13
- **Task:** HFTASK-0004
- **Depends on:** HFTASK-0001 (work-order intent-lock), HFTASK-0005 (`hf drift`)
- **Pillar:** Integrity (NORTH-STAR: *no promotion without Integrity · Reversibility ·
  Capability Gain*)

## Context

The kernel already records a blake3 **intent-lock** for every work-order — the immutable
contract surface `(objective, path_scope, acceptance)` hashed at mint time
(`work-order/src/lib.rs::IntentLock`, `WorkOrder::compute_intent_lock`). `hf drift`
re-derives those hashes and string-compares them (`Task::intent_unchanged()`), hard-failing
handoff on drift. That is a *comparison*, not a *proof*, and it says nothing about whether a
task handed off as **complete** actually has completion evidence.

The North Star (RUVECTOR-RUNBOOK §S1) names the end-state substrate: a
**`ruvector-verified` AgentContract** — a *formally-verified* contract proven on completion,
not merely compared. `work-order/src/lib.rs:62` already anticipates this verbatim: *"blake3
intent-lock (the drift sentinel anchor; ruvector-verified can prove against it)."* HFTASK-0004
is that proof.

## Decision

At `hf handoff`, construct and machine-check an **AgentContract proof** for the active
claimed task using the **`ruvector-verified`** formal-verification crate, and **fail closed**
— block the handoff (no packet render, exit 1) when the contract cannot be proven.

### The dependency decision — use the real `ruvector-verified` crate (path dep)

> **Correction (2026-06-14).** The first cut of this ADR depended on raw `lean-agentic`
> because I believed a path dep to `ruvector-verified` "breaks handoff's standalone CI — CI
> clones handoff alone." **That premise was false**, and building a thin proof layer on the
> bare kernel duplicated what `ruvector-verified` already provides (the *never-downgrade*
> rule). The corrected decision below uses the real crate.

The card names `ruvector-verified` and the kernel already establishes the pattern:

- **`ledger` already path-deps a RuVector crate** — `rvf-crypto = { path =
  "../../RuVector/crates/rvf/rvf-crypto" }` (the witness chain). So handoff is **not**
  RuVector-independent; the meta layout (RuVector as a sibling checkout) is assumed.
- **Handoff CI already clones it.** `.github/workflows/ci.yml` runs *“Clone rvf-crypto
  provider (meta-ruvector)” — `git clone --depth 1 https://github.com/FlexNetOS/meta-ruvector.git
  RuVector`* in **all three** jobs (test / clippy / format). A path dep to
  `../../RuVector/crates/ruvector-verified` resolves in CI exactly like `rvf-crypto`.
- RuVector's correct remote after the account transfer is **`FlexNetOS/meta-ruvector`**
  (origin), tracking `ruvnet/RuVector` (upstream); the local checkout is synced up to upstream.

So we depend on the **real crate**:

```toml
ruvector-verified = { path = "../../RuVector/crates/ruvector-verified", default-features = false }
```

`default-features = false` keeps it minimal (its default feature set is empty — only
`lean-agentic` + `thiserror`, no `ruvector-core`/NAPI). The card sets
`allows_dependency_addition: true`.

### What the proof proves

The intent-lock **is** the AgentContract. For the active claimed task (status ∈
`Claimed | Checkpointed | Active | Review`) the proof discharges these obligations through the
**`ruvector_verified::ProofEnvironment`** (pre-loaded with RuVector's type declarations incl.
the `Eq.refl` reflexivity rule). Mirroring `ruvector_verified::prove_dim_eq`, the equality is
decided in Rust and an `Eq.refl` proof term is minted only when it holds — but on a
**collision-free full-string comparison** of the recorded vs re-derived hash (strictly sounder
than reducing the hash to a `u32` dimension, which a `prove_dim_eq` shortcut would force):

1. **objective integrity** — `recorded.objective_hash == rederive(objective)`
2. **path_scope integrity** — `recorded.path_scope_hash == rederive(path_scope)`
3. **acceptance integrity** — `recorded.acceptance_hash == rederive(acceptance)`
4. **completion** *(only when the task is handed off as complete — status `Review`/`Done`)* —
   completion evidence exists: at least one witnessed checkpoint. Decided on the flag, then
   witnessed with an `Eq.refl` term.

Re-derivation reuses `WorkOrder::compute_intent_lock` **exactly** (same blake3 canonicalization
the kernel mints with) so the proof is faithful to the live contract, not a parallel hash.

The successful proof yields the real **`ruvector_verified::ProofAttestation`** (via
`proof_store::create_attestation`) — a tamper-evident SipHash-2-4 receipt over the proof-term
and environment state, stamped with the lean-agentic verifier version, **serializable into an
RVF `WITNESS_SEG` (82 bytes)** for the witness chain. `ContractProof` additionally carries a
`content_hash` binding the receipt to the specific recorded intent-lock hashes. The attestation
is rendered into the packet (witnessed).

### Fail-closed semantics

- **No active claim** → no contract to prove → handoff proceeds (vacuous pass).
- **Active task, no drift, mid-work** (`Claimed`/`Checkpointed`/`Active`) → obligations 1–3
  prove → handoff proceeds. Normal cycles are **never** blocked.
- **Intent drift** (obligation 1–3 fails) → `ProofError::IntentDrift` → **exit 1 before
  writing the packet**. (Complements `hf drift`; here it is a failed *proof*, not a compare.)
- **Complete-claimed task without completion evidence** (obligation 4 fails) →
  `ProofError::UnprovenCompletion` → **exit 1**. This is the *new* guarantee: *"block handoff
  on unproven completion."*

The gate runs **before** `cmd_handoff` writes `packets/latest.md`/`active.md`, so a blocked
handoff leaves the rendered views untouched (no half-written packet).

## Consequences

- **+** Integrity pillar gains a *formal* (lean-agentic-kernel-backed) verification receipt at
  the continuity boundary, not just a string compare; completion is now a proof obligation.
- **+** Uses the **real** RuVector formal layer — the `ProofAttestation` is RVF-`WITNESS_SEG`-
  serializable, so the contract proof can flow into the witness chain (RUVECTOR-RUNBOOK §S1)
  with no rework. No duplication of the proof crate.
- **+** Consistent with the kernel's existing RuVector coupling (`ledger`/`rvf-crypto`); CI
  already provisions the sibling checkout, so no new CI surface.
- **−** Adds a path dep on a sibling meta repo (`RuVector/crates/ruvector-verified`). Mitigated:
  `ledger` already does the same; CI clones `meta-ruvector` in every job; `default-features
  = false` keeps the dep tree minimal (`lean-agentic` + `thiserror`).
- **−** `hf handoff` now does bounded proof work each cycle (sub-millisecond; proof terms are
  tiny). Acceptable for a per-cycle continuity verb.
- **−** A genuinely complete task that lacks a witnessed checkpoint will be blocked until it is
  checkpointed — which is the intended discipline (`hf checkpoint` before `hf handoff`).

## Alternatives considered

- **Keep only `hf drift`** — rejected: a string compare is not a proof and ignores completion.
- **Build on bare `lean-agentic`** *(the first cut — superseded)* — rejected: it reimplemented a
  thin proof layer the `ruvector-verified` crate already provides (`ProofEnvironment`,
  `ProofAttestation`, the RuVector symbol environment), on the false premise that a path dep
  breaks CI. Duplication = downgrade. The real crate is used instead.
- **Vendor / git-pin `ruvector-verified`** — rejected: the path dep already works (CI clones the
  sibling), so vendoring would only fork an actively-developed crate.
- **Gate at `hf done`/`hf ship` instead of `hf handoff`** — rejected: the card specifies *at
  handoff*; handoff is the continuity-render boundary every cycle crosses, and `hf done` already
  feeds status that the handoff proof reads.
