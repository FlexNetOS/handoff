# AGENTS.md

## First command

Run:

```bash
hf resume
```

## North Star (canon — two levels)

This repo carries TWO canonical layers; read both before planning:

**1. Kernel doctrine (local, authoritative for *how* this kernel governs change):**
[`NORTH-STAR.md`](NORTH-STAR.md) — a local-first, auditable, reversible, model-native
agentic OS where **every agent action increases verified capability without corrupting the
baseline**: Integrity · Reversibility · Capability Gain (no promotion without all three).
CECCA/NOA is the executive kernel; the **Gold World** is the protected baseline; failures
compress into evidence. The keystone ADR is [`docs/adr-0001-flexnetos-autopilot-keystone.md`](docs/adr-0001-flexnetos-autopilot-keystone.md).

**2. Fleet vision (the *why/where* — meta root):** **NO HUMAN IN THE LOOP — a multi-provider
agentic autopilot; the user gives direction, the system builds/verifies/delivers/operates;
`NEEDS-HUMAN` is a scaffold replaced by a model with the human's skillset; end-state = a
single-person conglomerate run by the system.**

- **Fleet vision:** `../NORTH-STAR.md` (mission, laws, planes, build-order, steward rubric)
- **Architecture:** `../ARCHITECTURE-TRUTH.md` (the 5 planes, verified vs code)
- **Runbook:** `../RUVECTOR-RUNBOOK.md` (the agentic pipeline + build-out)

The packet's North Star derives from `.handoff/context/capsule.json` (`northstar`), which
points here — not a hardcoded string (ADR-0006).

## Mission

Maintain this repository through the Continuity Ledger Kernel (`.handoff`) protocol. The repo is the source of truth. Chat history is not authoritative.

## Hard rules

- Do not edit files without a task claim.
- Do not write outside claimed path scope.
- Do not run a parallel write session against overlapping paths.
- Do not mark a task complete without tests or an explicit waiver.
- Do not stop without `hf checkpoint` and `hf handoff`.
- Do not make architecture changes without an ADR.
- Do not treat `.handoff/packets/latest.md` as more authoritative than Git, the ledger, or task cards.

## Required before stopping

```bash
hf checkpoint <ID> [note]
hf handoff
```

(`hf drift` and `hf policy check-{claim,edit,handoff}` are implemented — the
PreHandoff/TaskClaim/PreEdit hard gates. Run `hf drift` before any handoff.)

## Navigation order

0. Kernel doctrine + keystone: `NORTH-STAR.md` · `docs/adr-0001-flexnetos-autopilot-keystone.md`
1. `.handoff/active.md`
2. `.handoff/context/capsule.json`
3. `.handoff/packets/latest.md`
4. `.handoff/tasks/` (task cards) · `.handoff/decisions/` (ADRs)
5. `docs/Continuity_Ledger_Kernel_PRD.md`
6. Fleet canon (the why): `../NORTH-STAR.md` · `../ARCHITECTURE-TRUTH.md` · `../RUVECTOR-RUNBOOK.md`
