---
name: drift-reconcile
description: "Reconciles the Continuity Ledger Kernel's truth: when active.md / packets / cards disagree with git or the ledger, settle it by precedence and RE-RENDER the derived views (never hand-edit). ALWAYS use when the packet looks stale, 'Done N/M' seems wrong, a card body doesn't match its intent_lock, or before picking the next task. Do NOT use to write new code (that's the implementer)."
---

# drift-reconcile — make the rendered views match the truth

Sessions ship PRs but the derived views (`active.md`, `packets/latest.md`, task
cards) only update when explicitly re-rendered. So they drift: the packet says
"Done 0/22" while git shows four merged HFTASKs. This skill settles every such
conflict and regenerates the views from witnessed truth.

## The precedence ladder (non-negotiable)

```
Git  >  .handoff/ledger.db  >  tasks/*.task.json  >  active.md  >  packets/latest.md
```

A lower tier never overrides a higher one. When they disagree, the higher tier is
correct and the lower tier is **regenerated**, not hand-edited. Hand-editing a
derived view forks continuity truth — the next sync overwrites it and the lie
silently returns.

## Procedure

1. **Read git truth.** `git log --oneline -30` and merged PRs
   (`gh pr list --state merged --limit 30`). Each merged "ship HFTASK-XXXX" /
   "feat: ... HFTASK-XXXX" is a *done* signal that the packet may not reflect.
2. **Read ledger truth.** Pick the ledger by plane (ADR-0004 §3, two orchestration
   homes): **KERNEL** = `handoff/.handoff/ledger.db` (handoff self-dev, the HFTASK
   backlog — the default for loop work in this repo); **FLEET** = `meta/.handoff/
   ledger.db` (fleet/member events — use this when reconciling a fleet repo). Per-repo
   `.handoff/` dirs carry **no ledger.db** — their events live in the FLEET ledger.
   Both are SQLite/WAL + SHA3 witness chain; no `sqlite3` CLI — read with python:
   ```bash
   python3 -c "import sqlite3,sys; c=sqlite3.connect('.handoff/ledger.db'); \
   [print(r) for r in c.execute('select seq,event_type,work_order_id from events order by seq')]"
   ```
   Replay events to derive each task's real status. **Verify the witness chain**
   (the kernel's own `verify` path / `hf resume --json` count) — a broken chain is
   a P0 finding, stop and escalate.
3. **Read card + view truth.** Compare `tasks/*.task.json` status against the
   replayed ledger status; compare `active.md` `Done X/Y` + `packets/latest.md`
   against git+ledger. Each mismatch is a drift item.
4. **Check intent_lock integrity.** Each card carries
   `intent_lock {objective_hash, path_scope_hash, acceptance_hash}` (blake3). If a
   card body changed but the lock didn't (or vice-versa) → drift item.
5. **Reconcile by re-rendering — never by hand:**

   | Drift | Fix | Fallback if verb missing |
   |-------|-----|--------------------------|
   | Cards stale vs ledger | `hf checkpoint --sync-cards` (or `hf sync-cards`) | leave stale; log gap; NEVER hand-edit |
   | `active.md` / packet stale | `hf handoff` (re-renders packet + active) | `hf resume` output IS the prompt; log gap |
   | Ledger missing a known-merged ship | checkpoint the ship into the ledger so truth is captured | record the omission as a finding |
   | `.kb` mirror stale (post-merge) | `hf sync --auto` (HFTASK-0011) — see the `meta-kb-sync` skill for the one-way seam rules | note gap; one-way ledger→.kb only, never read kb back |

6. **Report** the reconciliation: every drift item, the precedence call, the verb
   run, and the resulting state. Output `_workspace/01_navigator_truth.md` (the
   navigator's truth report) with the ledger-verified done/remaining list.

## Why re-render, never hand-edit

Cards and packets are *compiled views* of the ledger. Editing them by hand is like
editing build output: correct for a moment, then clobbered by the next
`hf checkpoint`/`hf handoff`, and indistinguishable from a genuine render in
between. If a needed re-render verb doesn't exist yet, that is a missing kernel
feature — record it as a task (e.g. HFTASK-0005 `hf drift`), don't paper over it.

## Known gaps to watch (current kernel state)

- `hf drift` (HFTASK-0005) — the automatic drift gate — is **not yet implemented**;
  until it lands, run this reconciliation manually each cycle.
- `hf policy check-*` (HFTASK-0015), `hf fleet status`, `hf sync` are referenced by
  `.handoff/hooks/hooks.toml` but may be hollow — verify before relying on a hook.

## Selecting the next safe task (after reconciling)

From the ledger-verified remaining list, pick the highest-priority task whose
`dependencies` and `blocked_by` are all `done` and whose status is not `done`.
Emit the exact `hf claim <ID>` command. Never pick a task whose card body conflicts
with its intent_lock until that drift is resolved.
