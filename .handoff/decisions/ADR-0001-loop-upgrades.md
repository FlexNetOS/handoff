# ADR-0001 — Handoff Loop v2: worktree-isolated, cycle-batched, review-gated shipping

- **Status:** Proposed
- **Date:** 2026-06-09
- **Deciders:** drdave (+ Claude)
- **Scope:** `hf` CLI, `.handoff/` contract, weave integration
- **Supersedes:** none

## Context

Today the loop is **single-checkout, single-task**: a session claims one task
(now mesh-coordinated via a weave lease — see HFTASK-0002), edits the shared
working tree in place, and commits when told. Shipping is ad-hoc and the
hand-off mechanism is prose in `HANDOFF.md`.

The sibling autonomous loops already running against this machine
(`idd-merge`, `n8n`, `weave-mcp-daemon-tools`) have **independently converged**
on a richer pattern, visible in their relay broadcasts and in
`weave-mcp-daemon-tools/CLAUDE.md`:

- a **fresh git worktree per session**, branched off a freshly-fetched
  long-lived base branch (`develop`), never a stale local ref;
- a **cycle budget** (3–5 tasks) completed before a session hands off;
- **PRs into a protected trunk** (`master`), with `develop` kept
  fast-forwarded to it;
- a **separate review + permission gate** before merge.

We want the Continuity Ledger Kernel to **own this lifecycle as first-class
`hf` verbs**, recorded as witnessed ledger events, so every loop inherits it
instead of re-implementing it in per-repo prose. This ADR captures the design
requested in the 2026-06-09 quick-note (the seven loop-upgrade items).

## Decision

Add a **session lifecycle** (worktrees) and a **shipping lifecycle** (cycle →
ship → review → merge → sync) to `hf`, configured by `.handoff/policy.toml`,
recorded in the ledger, and coordinated through weave (path-scope leases +
review/permission queues).

### 1. Configuration — `.handoff/policy.toml`

```toml
[remote]
model        = "clone"            # "clone" | "fork"
origin       = "FlexNetOS/handoff"
base_branch  = "develop"          # worktrees branch off origin/<base_branch>
trunk_branch = "master"           # PR target / protected
develop_mirrors_trunk = true      # ff develop -> trunk after each merge

[loop]
cycle_flush     = 4               # ship after N completed tasks (range 3..5)
worktree_root   = "../"
worktree_prefix = "handoff-"

[merge]
require_review  = true            # a separate agent must approve before merge
auto_merge      = "on_approve"    # "on_approve" | "never" | "manual"
permission_gate = true            # outward merge needs an approved permission ask
```

### 2. Session lifecycle (quick-note items 2, 3, 7)

- **`hf session start [--task-slug X]`**
  1. `git fetch origin`
  2. `git worktree add <root><prefix><slug> -b <branch> origin/<base_branch>`
  3. reserve a weave lease on the worktree **path scope** (extends the
     per-task claim lease to the whole tree → two sessions never share a tree)
  4. emit `session_start` event (worktree path, branch, base SHA); reset the
     cycle counter.
- **`hf session end [--recycle]`**
  1. require clean/merged; release the path lease
  2. `git worktree remove <path>`; emit `session_end`
  3. with `--recycle`, immediately `session start` a fresh worktree
     (item 7: "delete after PR merge and new worktree created").
- **Recovery:** `session start` is idempotent — if the worktree exists and the
  lease is ours, adopt it instead of failing.

### 3. Branch & remote policy (item 6)

A `policy` module resolves clone-vs-fork, base (`develop`), and trunk
(`master`). Enforced invariants:

- never branch off a local ref — always `origin/<base>` after a fetch;
- never push to trunk directly; PRs target trunk;
- after merge, fast-forward `develop` to trunk (`git push origin master:develop`)
  so `develop` is always == trunk (never ahead).
- **fork model:** `origin` = the fork; PRs are cross-repo into upstream.
  Deferred behind `remote.model = "fork"` (clone is the default path).

### 4. Cycle-batched shipping (item 4)

- The per-session **cycle counter** is ledger-derived: count `checkpoint`
  events since the last `session_start`.
- `hf status` surfaces `cycles: n/flush`; when `n >= cycle_flush`,
  `next_command` becomes `hf ship`.
- **`hf ship`**
  1. `git add -A && git commit` referencing every task id since the last ship
     (squash vs per-task is configurable);
  2. `git push origin <branch>`;
  3. `gh pr create --base <trunk>` → emit `pr_opened` with the PR number.
  - This is an **outward action**; it is gated by the permission system (§5).
    On a "not yet" permission verdict it records the PR intent and **waits
    (retryable)** — it must not hard-wall the loop.

### 5. Review + merge automation with a *separate* agent (item 5)

- **`hf review request <pr#>`** → enqueue into weave's review queue (WL-020)
  and open a **permission ask** (WL-021) addressed to a `reviewer` session.
- A **separate review agent** (distinct session/role, not the implementer)
  runs `/code-review` (or `/code-review ultra`) on the PR and records a verdict
  through weave: `approve` or `deny(reason)`.
- **`hf merge <pr#>`** reads the review verdict + permission verdict:
  - `approved` AND permission granted AND `auto_merge = on_approve`
    → `gh pr merge --squash`; emit `pr_merged`; fast-forward develop;
  - `denied` → emit `pr_changes_requested`; re-open the task(s) for a fix cycle;
  - permission pending → **wait (retryable)**; never auto-merge without it.
- Merge is the **one mandatory human/permission gate** — consistent with the
  wall already encountered on `gh repo create` (HFTASK-0001 NEEDS-HUMAN).

### 6. `.kb` + meta sync (item 1)

- **`hf sync`**
  - **meta:** ensure this repo is registered in the parent `../.meta.yaml`
    projects and listed in `../.gitignore` (the meta-repo rule for new crates).
    Idempotent; emits `meta_registered`.
  - **.kb:** push a context document (brief / active / progress) into FlexNetOS
    `.kb` via `git kb`, so the knowledge base mirrors the ledger's active
    state. **One-way (ledger → kb)** to keep Git authoritative.
  - Runs at `session end` / after `pr_merged`.

### 7. New witnessed ledger event types

`session_start`, `session_end`, `pr_opened`, `pr_merged`,
`pr_changes_requested`, `review_verdict`, `permission_verdict`,
`meta_registered` — all part of the tamper-evident chain like existing events.

### 8. Weave integration summary

- **Leases:** per-worktree path-scope leases (generalizes the HFTASK-0002 claim
  lease).
- **Review/permission:** `weave review` + `weave permission` drive §5.
- **Broadcasts:** on `session_start` / `pr_opened` / `pr_merged`, so sibling
  loops observe activity (the relay traffic already on the mesh).

## Consequences

**Positive**
- Every loop gets worktree isolation, cycle batching, and a review/merge gate
  for free; `HANDOFF.md` prose becomes a *compiled view*, not the mechanism.
- Concurrent sessions stop colliding on a shared tree.
- The whole lifecycle is a witnessed event stream — replayable and auditable.

**Negative / risks**
- Requires the **lease-capable weave** as the default install (today the
  installed `~/.cargo/bin/weave` is older; `hf` degrades but isn't coordinated).
- Depends on `gh` + branch protection existing; fork model adds cross-repo PR
  edge cases (deferred).
- More verbs and config surface to maintain and test.

## Task breakdown

| Task | Pillar | Quick-note items |
|------|--------|------------------|
| **HFTASK-0007** | `hf session` worktree lifecycle + `policy.toml` | 2, 3, 7 |
| **HFTASK-0008** | branch/remote policy engine (develop↔master, clone/fork) | 6 |
| **HFTASK-0009** | cycle-budget batching → `hf ship` (commit/push/PR) | 4 |
| **HFTASK-0010** | PR review/merge automation w/ separate agent + permission gate | 5 |
| **HFTASK-0011** | `hf sync` — `.meta.yaml` + `.gitignore` + `.kb` mirror | 1 |

Dependencies: 0008 → 0007; 0009 → 0007/0008; 0010 → 0009; 0011 → 0007.
