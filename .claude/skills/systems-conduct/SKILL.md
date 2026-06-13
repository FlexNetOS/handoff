---
name: systems-conduct
description: "How the meta workspace's integrated systems compose — meta loop (loop_lib), meta git, meta exec, grit, handoff (hf), .kb, ruvector — and the correct order to sequence cross-system operations. ALWAYS use when a task spans more than one system, when ordering cross-system steps, or when deciding the fleet's next-best task (pull-based, priority-gated). Do NOT use for single-system work (use that system's own skill)."
---

# systems-conduct — compose the systems in the right order

The meta workspace is composed systems, each owning one concern. Conducting them means
knowing the boundaries, sequencing operations across them correctly, and selecting the
next task by value — not letting every repo back-fill its queue.

## The systems & their boundaries

| System | Concern (owns) | Does NOT own |
|--------|----------------|--------------|
| **meta loop** (`loop_lib`) | running commands across dirs | git, coordination |
| **meta git** | cross-repo git (status/commit/snapshot/worktree) — **SSH remotes** (`git@github.com:…`, the `.meta.yaml` default; never HTTPS) | task state |
| **meta exec** | cross-repo command execution (parallel/filtered) | git semantics |
| **grit** | code coordination (AST locks, worktrees, conflict-free merge) | task continuity |
| **handoff** (`hf`) | continuity (ledger, resume, tasks, fleet status/render, drift, policy) | code locks |
| **.kb** (git-kb) | planning plane (tasks/specs/context, "what's next") | execution truth |
| **ruvector** | vector/witness substrate (rvf-crypto) under the ledger | CLI surface |

## The canonical cross-system sequence

For any task that changes code across the fleet, sequence the planes in order — never
skip one:

```
1. SELECT   .kb board / hf fleet status        → what's next (pull-based, §below)
2. LEASE    hf claim <TASK>                     → task-level continuity lock
3. LOCK     grit claim --mode … <symbols>       → code-level AST locks
4. ISOLATE  hf session (grit-enabled worktree)  → per-agent isolation
5. WORK     (edit within scope + claimed symbols)
6. MERGE    grit done                           → rebase + serialized conflict-free merge
7. WITNESS  hf checkpoint / hf done             → record proof in the ledger
8. SYNC     hf sync (.kb write-back) + meta git → planning plane + cross-repo land
9. RENDER   hf fleet render / hf handoff        → derived views (doc-updater)
```
State precedence across all of it: **Git > ledger > cards > prose.**

## Pull-based, priority-gated selection (the back-fill policy)

**Do not let every repo auto-work its backlog** — 60 parallel low-value loops swamp the
FLEET ledger. The hybrid policy (owner-decided 2026-06-13):

1. A repo session **orients**: surfaces its next safe task (capsule `next_command` /
   `hf resume`), no auto-claim.
2. **Auto-start only `ready`-flagged tasks** — a task whose capsule/card marks it
   `ready: true` (explicitly prioritized) may auto-claim on session start.
3. **Everything else waits for the pull**: the systems-orchestrator selects the
   highest-*value* task across the fleet (`hf fleet status`) under a budget/cycle gate,
   one at a time — the task that unblocks the most downstream sessions wins ties.

This keeps hot/ready work responsive without back-filling the long tail.

## Degradation (a system absent)

| Absent | Fall back to | Say so |
|--------|--------------|--------|
| grit | meta/git worktree + file-level care | "no symbol locks — serialize edits" |
| meta | per-repo git + manual loop | "no cross-repo parallelism" |
| git-kb / .kb | card-only minting | "planning plane offline" |
| envctl Phase 8 | local grit backend | "no cross-repo symbol coordination" |

Never fabricate a system's result; record the degradation as a finding.

## Conduct, don't reimplement

Each step is handed to its owning agent: SELECT/WITNESS → continuity-navigator;
LOCK/WORK/MERGE → kernel-implementer (via `grit-coordination`); SYNC → meta-sync-steward;
cross-repo land → fleet-steward; gate → code-omniscient-gatekeeper; docs → doc-updater.
The conductor sequences and arbitrates; it does not do the specialists' work.
