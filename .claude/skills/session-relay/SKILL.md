---
name: session-relay
description: "Seamless multi-system handoff + resume for the continuity kernel: hand off and pick up work across handoff (hf ledger/packet), grit (symbol locks + worktree), the fleet, and .kb — in one protocol. ALWAYS use at session start (resume), session end (hand off), at a cycle/budget boundary, or when picking up another session's/repo's work. Triggers on 'hand off', 'resume', 'pick up where we left off', 'checkpoint the loop', 'continue in a new session'. Do NOT use for kb planning-doc authoring (that's the kb workflow)."
---

# session-relay — one handoff across all the systems

Sessions die and context compacts; the next session must resume from witnessed truth,
not chat. With multiple systems now in play (handoff + grit + fleet + .kb), a clean
handoff must release/record state in **all** of them so the next agent picks up
seamlessly. This is that protocol (extends ADR-0003/0004; companions: `handoff-loop`,
`grit-coordination`, `systems-conduct`).

## State precedence (settle every conflict)

**Git > FLEET/KERNEL ledger > task cards > `.kb` > packets/prose.** Derived views
(cards, packets, capsules, `.kb` overridable slugs) are regenerated, never trusted over
the ledger.

## RESUME (session start) — orient, don't auto-backfill

1. `hf resume` (the SessionStart hook injects a compact packet). Read it as a *claim*,
   then verify against live state.
2. `hf fleet status` — where the fleet stands; which repos have `ready` work.
3. Check this repo's grit state: `grit status` (any locks held? stale? `grit gc` if so)
   and `grit worktree list` (orphaned agent worktrees from a crashed session).
4. **Selection (hybrid policy, owner-decided 2026-06-13):** auto-claim ONLY a task whose
   capsule/card flags `ready: true`. Otherwise **orient and wait** — the
   systems-orchestrator pulls the next-best task across the fleet. Do not auto-work the
   backlog (back-fill flood).
5. Reconcile drift before acting (`hf drift`; `drift-reconcile` skill).
6. **`icm recall`** prior decisions/errors/preferences for what you're resuming
   (mandatory cross-session memory — `icm-memory` skill). The packet says *what
   happened*; ICM says *what was decided/learned*.

## HAND OFF (session end) — release + record in every system

Release and witness across all planes so nothing is left half-held:

| System | Release / record |
|--------|------------------|
| **grit** | `grit done --agent <id>` for each active agent (merges + releases locks); `grit gc` strays; confirm `grit status` clean |
| **handoff** | `hf checkpoint <ID> "<what landed, verified, next>"` → `hf handoff` (re-render packet from the real ledger) |
| **fleet** | `hf fleet render <repo>` for any repo whose state changed |
| **.kb** | `hf sync` (one-way ledger→kb: active/progress) — when envctl injection is up |
| **ICM** | `icm store` the cycle's decisions/errors-resolved/completion (mandatory cross-session memory — `icm-memory` skill) before stopping |
| **leases** | release the weave task lease (`hf release <ID>` / session end) |

The rendered packet IS the next-session prompt. The SessionEnd hook runs
`hf checkpoint --auto && hf handoff` as a safety net — but a clean hand off does the
grit release + sync explicitly.

## CYCLE / BUDGET boundary

At `cycle_flush` (or budget): finish the in-flight `grit done` + `hf checkpoint`, then
hand off. Never leave a grit lock held or a worktree dirty across the boundary — the
next session would block on it.

## Picking up ANOTHER session's / repo's work

1. `hf fleet status` → find the repo + its open cards.
2. Check that repo's grit `status`/`queue` — is a symbol you need held? `--queue` or
   `--wait` rather than stomping.
3. Resume from that repo's capsule + packet; never trust a stale hand-written prompt
   (those are deprecated — ADR-0004 §1).

## Anti-patterns (each has burned a session)

- Trusting a packet without verifying against git/ledger — recalled state predates merges.
- Ending a session with grit locks still held → next session blocks; always `grit done`/`gc`.
- Auto-backfilling every repo's queue → FLEET ledger swamp; pull by value instead.
- Hand-editing a packet/card/capsule → overwritten on next render; fix the source.
- Regenerating `active.md` from a worktree's gitignored ledger → wrong counts; render
  from the repo that owns the ledger.
