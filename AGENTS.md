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

## Knowledge base — the planning plane (`.kb`, ADR-0003 / ADR-0018 D7)

handoff fully adopts the FlexNetOS agent guide (`../.kb/AGENTS.md`) with its OWN durable
`.kb/`. Detect KB state and load context BEFORE planning non-trivial work:

```bash
git kb list --path context/      # PATH A (empty) / PATH B (populated) / PATH C (resume)
git kb checkout --path context/  # load the seven context documents
git kb board                     # task kanban
```

- **Create-first discipline:** for a non-trivial feature/bug, create the kb document
  FIRST (`git kb create task|incident …`) — the document IS your plan — then explore.
  Trivial typo/one-liner changes are exempt.
- **Traceability:** commit messages reference the task (`Implements [[tasks/<slug>]]` /
  the HFTASK id); tasks link incidents/specs; children link parents.
- **Two-way seam (ONE-WAY authority — kb never overrides execution truth):**
  `hf task mint --from-kb <slug>` mints a witnessed card IN; `hf claim`/`checkpoint`/
  `done`/`release` mirror the transition back OUT (active/+progress/completed/backlog).
  `hf status` / `hf resume` (the ledger) is authoritative; the kb informs the plan.
- **Residency:** `.kb/store/**` (durable text) is committed; `.kb/.cache/` (binary
  cache) + `.kb/workspaces/` (ephemeral) + `.kb/config.toml` are gitignored.

See `.claude/rules/knowledge-management.md` for the full lifecycle.

## Hard rules

- Do not edit files without a task claim.
- Do not write outside claimed path scope.
- Do not run a parallel write session against overlapping paths.
- Do not mark a task complete without tests or an explicit waiver.
- Do not stop without `hf checkpoint` and `hf handoff`.
- Do not make architecture changes without an ADR.
- Do not treat `.handoff/packets/latest.md` as more authoritative than Git, the ledger, or task cards.

### Fail-closed law (the FAIL-OPEN ban — L7)

A guard, loader, or evidence-check that **cannot confirm its precondition must STOP,
not proceed.** In any continuity-gating path (card load, ledger read, status
derivation, completion-evidence, lock acquisition, policy gate) the following
fail-OPEN patterns are **banned** — each must instead fail closed with a *surfaced
diagnostic*:

- a silent `if let Ok(_) { … }` / `match … { Ok => …, Err => /* skip */ }` that drops
  the error case without surfacing it (e.g. silently skipping an unparseable card);
- `.ok()?` on a card or ledger read (swallows the failure and short-circuits as if
  empty);
- `unwrap_or_default()` feeding a status/derivation (an empty default reported as
  truth — `current_statuses()` returning `{}` must be a FAIL, not "nothing done");
- **exit 0 ⇒ pass** — treating a zero exit / empty result / zero rows / `None`
  runner as evidence the criterion was met (require the *positive* count or artifact);
- **retry-then-quietly-give-up** — exhausting a retry cap and proceeding as if the
  operation succeeded (the stale-lock wedge), instead of surfacing the wall.

Every such site must emit a diagnostic to stderr (or a `hf doctor`/`hf status`
surfacing) and propagate a non-zero/Err outcome. *Absence of failure is not evidence
of success.* When in doubt, fail closed and surface — the kernel's founding promise
is witnessed + fail-closed.

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
6. Planning plane: `git kb checkout --path context/` (the `.kb` context documents) · `git kb board`
7. Fleet canon (the why): `../NORTH-STAR.md` · `../ARCHITECTURE-TRUTH.md` · `../RUVECTOR-RUNBOOK.md`
