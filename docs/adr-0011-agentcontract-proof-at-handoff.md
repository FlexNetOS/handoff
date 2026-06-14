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
claimed task using the **`lean-agentic`** dependent-type kernel, and **fail closed** — block
the handoff (no packet render, exit 1) when the contract cannot be proven.

### Why `lean-agentic`, not `ruvector-verified` (the dependency decision)

The card names `ruvector-verified`. That crate is **not publishable into handoff**:

| Option | Verdict |
|--------|---------|
| `ruvector-verified` as a **path dep** to `../RuVector/crates/ruvector-verified` | ❌ Breaks handoff's **standalone CI** — CI clones handoff alone; RuVector is a *separate meta repo* (meta-repo independence rule, root `CLAUDE.md`). |
| `ruvector-verified` as a **registry dep** | ❌ Not published — `index.crates.io` returns `NoSuchKey`. It is a path-only member of RuVector's own workspace. |
| `ruvector-verified` as a **git dep** on `ruvnet/ruvector` | ❌ Couples handoff CI to an external upstream at an unpinned ref; diverges from the local RuVector. |
| **`lean-agentic = "0.1.0"`** (registry) | ✅ **Published** (checksum `d3b6dcd…`, cached locally) and dependency-free. It is the dependent-type Lean kernel **`ruvector-verified` is itself built on** ("Formal verification layer for RuVector … using lean-agentic dependent types"). |

So we depend on the **same Lean kernel** `ruvector-verified` wraps, and build the
handoff-side contract proof directly on it. This is `ruvector-verified` *in substance* (the
machine-checked Lean conversion check) without importing an unpublishable crate. The task card
sets `allows_dependency_addition: true`; `lean-agentic` has zero transitive deps.

### What the proof proves

The intent-lock **is** the AgentContract. For the active claimed task (status ∈
`Claimed | Checkpointed | Active | Review`) the proof discharges these obligations against the
**`lean-agentic` definitional-equality checker** (`Converter::is_def_eq` over terms interned
in a `lean_agentic::Arena`) — a refl proof term is constructible **iff** the equality holds,
mirroring `ruvector-verified::prove_dim_eq`:

1. **objective integrity** — `recorded.objective_hash ≡ rederive(objective)`
2. **path_scope integrity** — `recorded.path_scope_hash ≡ rederive(path_scope)`
3. **acceptance integrity** — `recorded.acceptance_hash ≡ rederive(acceptance)`
4. **completion** *(only when the task is handed off as complete — status `Review`/`Done`)* —
   completion evidence exists: at least one witnessed checkpoint **and** a terminal status.
   Encoded as `is_def_eq(completion_flag, TRUE)`.

Re-derivation reuses `WorkOrder::compute_intent_lock` **exactly** (same blake3 canonicalization
the kernel mints with) so the proof is faithful to the live contract, not a parallel hash.

The successful proof yields a **`ContractProof` attestation** — proof-term count, a content
hash over the proof + environment state, and the `lean-agentic` verifier version — rendered
into the packet (witnessed, tamper-evident), following `ruvector-verified::ProofAttestation`.

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

- **+** Integrity pillar gains a *formal* (Lean-kernel-checked) artifact at the continuity
  boundary, not just a string compare; completion is now a proof obligation.
- **+** Standalone-CI-safe: one published, dep-free crate; no cross-meta-repo coupling.
- **+** Forward path to the full `ruvector-verified` AgentContract substrate (RUVECTOR-RUNBOOK
  §S1) — when `ruvector-verified` is published, `hf/src/contract.rs` swaps its proof backend
  without changing the gate.
- **−** `hf handoff` now does bounded proof work each cycle (sub-millisecond; proof terms are
  tiny). Acceptable for a per-cycle continuity verb.
- **−** A genuinely complete task that lacks a witnessed checkpoint will be blocked until it is
  checkpointed — which is the intended discipline (`hf checkpoint` before `hf handoff`).

## Alternatives considered

- **Keep only `hf drift`** — rejected: a string compare is not a proof and ignores completion.
- **Vendor `ruvector-verified` source into handoff** — rejected: duplicates an actively-developed
  sibling crate (no-downgrade/no-fork discipline); `lean-agentic` gives the same kernel cleanly.
- **Gate at `hf done`/`hf ship` instead of `hf handoff`** — rejected: the card specifies *at
  handoff*; handoff is the continuity-render boundary every cycle crosses, and `hf done` already
  feeds status that the handoff proof reads.
