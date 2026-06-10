# ADR-0001 — Handoff Loop v2: worktree-isolated, cycle-batched, review-gated shipping

- **Status:** Accepted (refined after review 2026-06-09)
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
cycle_flush     = 4               # tasks checked out + completed per cycle (3..5)
batch_checkout  = true            # claim 3-5 tasks at once so the loop never stalls
worktree_prefix = "handoff-"      # meta worktree set name prefix

[merge]
require_review  = true            # a review verdict is required before merge
reviewer        = "cloud_ultra"   # Phase 1: "cloud_ultra" (/code-review ultra)
                                  # Phase 2: "swarm_local" (ruvector/ruflo swarm)
auto_merge      = "on_approve"    # "on_approve" | "never" | "manual"
permission_gate = true            # TRANSITIONAL human gate; lifted once swarm is trusted
```

### 2. Session lifecycle (quick-note items 2, 3, 7)

**Worktrees are managed through `meta git worktree`**, not raw `git worktree`,
so handoff's isolation is tracked by the meta workspace (one authoritative
worktree *set*, never ad-hoc trees — see the Lessons section for why this
matters). The installed integration surface is
`meta git worktree create|add|remove|list|status|prune`.

- **`hf session start [--task-slug X]`**
  1. `git fetch origin` (the meta worktree create branches off the fresh remote base)
  2. `meta git worktree create <prefix><slug> --repo handoff` off
     `origin/<base_branch>`
  3. reserve a weave lease on the worktree **path scope** (extends the
     per-task claim lease to the whole tree → two sessions never share a tree)
  4. emit `session_start` event (worktree set, branch, base SHA); reset the
     cycle counter.
- **`hf session end [--recycle]`**
  1. require clean/merged; release the path lease
  2. `meta git worktree remove <set>` (+ `meta git worktree prune` for orphans);
     emit `session_end`
  3. with `--recycle`, immediately `session start` a fresh set
     (item 7: "delete after PR merge and new worktree created").
- **Recovery:** `session start` is idempotent — if the set exists and the lease
  is ours, adopt it instead of failing.

### 3. Branch & remote policy (item 6)

A `policy` module resolves clone-vs-fork, base (`develop`), and trunk
(`master`). Enforced invariants:

- never branch off a local ref — always `origin/<base>` after a fetch;
- never push to trunk directly; PRs target trunk;
- after merge, fast-forward `develop` to trunk (`git push origin master:develop`)
  so `develop` is always == trunk (never ahead).
- **fork model:** `origin` = the fork; PRs are cross-repo into upstream.
  Deferred behind `remote.model = "fork"` (clone is the default path).

### 4. Cycle-batched shipping (item 4) — batch checkout, squash-the-cycle commit

**Cycle model (decided):** a session **checks out 3–5 tasks at once** (a batch
claim — each task still gets its own weave lease) so the loop never stalls
between single tasks, works them all to checkpoint, then produces **one squashed
commit for the whole cycle** and ships a single PR.

- The per-session **cycle counter** is ledger-derived: count `checkpoint`
  events since the last `session_start`.
- `hf status` surfaces `cycles: n/flush`; when `n >= cycle_flush` (default 4),
  `next_command` becomes `hf ship`.
- **`hf claim --batch N`** (up to `cycle_flush` tasks) reserves a lease per task
  and opens the cycle.
- **`hf ship`**
  1. `git add -A && git commit` — **one commit** whose message lists every
     `HFTASK-id` completed in the cycle;
  2. `git push origin <branch>`;
  3. `gh pr create --base <trunk>` → emit `pr_opened` with the PR number.
  - This is an **outward action**; it is gated by the permission system (§5).
    On a "not yet" permission verdict it records the PR intent and **waits
    (retryable)** — it must not hard-wall the loop.
  - Merge squashes again, so per-task commits would be lost anyway — one commit
    per cycle keeps trunk history clean and matches the cycle boundary.

### 5. Review + merge automation with a *separate* agent (item 5) — phased

The reviewer is **always a separate role** from the implementer. How that role
is filled is **phased**, set by `merge.reviewer`:

- **Phase 1 — `cloud_ultra` (now):** `hf review request <pr#>` kicks off
  `/code-review ultra` (the multi-agent cloud review) on the PR branch; its
  verdict drives merge. Chosen because it works as-is with no new build.
- **Phase 2 — `swarm_local` (after ruvector/ruflo integration):** replace the
  cloud reviewer with a **local agent-swarm reviewer** built on ruvector/ruflo's
  swarm design (which already models exactly this approve/deny panel). No cloud
  dependency, fully in-mesh.

**Vision:** the permission gate is **transitional**. The end state is a
*fully-automated loop with no human in the loop* — the agent swarm's verdict
*is* the gate. Phase 1's human permission ask is the safety net while the swarm
verdict is being trusted, designed to be lifted (`permission_gate = false`)
once Phase 2 is proven.

- **`hf review request <pr#>`** → enqueue into weave's review queue (WL-020),
  open a **permission ask** (WL-021), and dispatch the reviewer per
  `merge.reviewer`. The reviewer records `approve` / `deny(reason)` through weave.
- **`hf merge <pr#>`** reads the review verdict + permission verdict:
  - `approved` AND permission granted AND `auto_merge = on_approve`
    → `gh pr merge --squash`; emit `pr_merged`; fast-forward develop;
  - `denied` → emit `pr_changes_requested`; re-open the task(s) for a fix cycle;
  - permission pending → **wait (retryable)**; never auto-merge without it.
- Merge is the **one gate that blocks** — Phase 1 a human/permission verdict
  (consistent with the `gh repo create` wall in HFTASK-0001 NEEDS-HUMAN),
  Phase 2 the swarm verdict.

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

## Lessons baked in (from the prior weave loop failure)

The earlier weave-driven loop **did not work reliably**, with evidence pointing
to two root causes. This design is shaped to avoid both:

1. **Multiple trees / branches / remotes drifted out of sync.** A loop spread
   across ad-hoc worktrees, branches, and remotes lost a single source of
   truth. → Mitigations: one authoritative base (`origin/<develop>` *after a
   fetch*, never a local ref); `develop` kept `==` trunk; **all** worktrees are
   tracked *sets* via `meta git worktree` (§2), never hand-rolled `git worktree`;
   a `hf session start` **preflight** that verifies tree/branch/remote sync
   before any work and refuses to start on drift (ties into the HFTASK-0005
   drift gate).
2. **It used the old `repowire` + `mcp-broker` hooks.** Those are deprecated and
   were implicated in the breakage. → This design depends **only on the current
   lease-capable `weave`** (the build that exposes `weave lease` and the
   review/permission queues). `hf` must detect and refuse the legacy
   repowire/mcp-broker path rather than silently coordinate through it.

These are correctness requirements, not nice-to-haves: HFTASK-0007/0008 must
land the sync preflight, and HFTASK-0010 must target current-weave queues only.

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
| **HFTASK-0007** | `hf session` via `meta git worktree` + `policy.toml` + sync **preflight** | 2, 3, 7 |
| **HFTASK-0008** | branch/remote policy engine (develop↔master, clone/fork) | 6 |
| **HFTASK-0009** | batch checkout (3–5 tasks) + cycle counter → `hf ship` (one squash commit/PR) | 4 |
| **HFTASK-0010** | PR review/merge automation — phased cloud_ultra→swarm_local + permission gate | 5 |
| **HFTASK-0011** | `hf sync` — `.meta.yaml` + `.gitignore` + `.kb` mirror | 1 |

Dependencies: 0008 → 0007; 0009 → 0007/0008; 0010 → 0009; 0011 → 0007.
