# AGENTS.md

## First command

Run:

```bash
hf resume
```

## North Star (canon)

This kernel serves the fleet north-star: **NO HUMAN IN THE LOOP — a multi-provider
agentic autopilot where the user gives direction and the system synthesizes, builds,
verifies, and delivers**; `NEEDS-HUMAN` walls are a *scaffold* to be replaced by a model
with the human's skillset; end-state = a single-person conglomerate run by the system.
The authoritative vision/architecture/runbook live at the meta root — read them, don't
re-derive:

- **Vision / compass:** `../NORTH-STAR.md` (mission, laws, planes, build-order, steward rubric)
- **Architecture:** `../ARCHITECTURE-TRUTH.md` (the 5 planes, verified vs code)
- **Runbook:** `../RUVECTOR-RUNBOOK.md` (the agentic pipeline + build-out)

The rendered north-star in every packet derives from `.handoff/context/capsule.json`
(`northstar`), which points here — not from a hardcoded string (ADR-0006).

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

1. `.handoff/active.md`
2. `.handoff/context/capsule.json`
3. `.handoff/packets/latest.md`
4. `.handoff/tasks/` (task cards) · `.handoff/decisions/` (ADRs)
5. `docs/Continuity_Ledger_Kernel_PRD.md`
6. Fleet canon (the why): `../NORTH-STAR.md` · `../ARCHITECTURE-TRUTH.md` · `../RUVECTOR-RUNBOOK.md`
