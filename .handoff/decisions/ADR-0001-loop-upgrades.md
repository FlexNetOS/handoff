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

**Worktrees are managed through the meta worktree engine**, not raw
`git worktree`, so handoff's isolation is tracked by the meta workspace (one
authoritative worktree *set*, never ad-hoc trees — see the Lessons section for
why this matters). Two integration surfaces, in order of preference:

1. **Depend on `meta_git_lib` directly** (the Rust crate behind `meta git
   worktree`). Research (see Research §R3) found it already provides the entire
   engine `hf` needs: `worktree::git_ops::{git_worktree_add, git_worktree_remove,
   remove_worktree_repos}`, a locked worktree **registry with TTL/ephemeral
   cleanup** (`worktree::store`, `~/.meta/worktree.json`), **lifecycle hooks**
   `worktree::hooks::{fire_post_create, fire_post_destroy, fire_post_prune}` (the
   natural attach point for hf's PR-create/review steps), `resolve_branch`,
   `git_ahead_behind`/`git_fetch_branch` (fast-forward signal), `snapshot`
   capture/restore (per-repo branch+SHA rollback), and
   `ensure_worktrees_in_gitignore`. **Reuse this wholesale** — do not reimplement.
2. Fall back to shelling out to `meta git worktree create|add|remove|list|status|
   prune` when the library dependency isn't wired yet.

**Naming resolved (Research §R2):** the dotdir is **`.handoff`** (canonical;
`.hf` exists in no repo). `hf` is only the binary name. Worktrees live as a
tracked meta set under `.worktrees/` at the meta root.

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

- **`hf review request <pr#>`** → enqueue into weave's review queue (WL-020, the
  human-facing PR list), open a **permission ask** (WL-021), and dispatch the
  reviewer per `merge.reviewer`. The reviewer's `approve`/`deny(reason)` verdict
  is carried in the **permission answer body** and recorded as a `review_verdict`
  event in **hf's own ledger** (authoritative) — **not** in `weave review`, which
  has no verdict field (Research §R6). hf enforces the gate; weave only records.
- **`hf merge <pr#>`** reads the review verdict + permission verdict:
  - `approved` AND permission granted AND `auto_merge = on_approve`
    → `gh pr merge --squash`; emit `pr_merged`; fast-forward develop;
  - `denied` → emit `pr_changes_requested`; re-open the task(s) for a fix cycle;
  - permission pending → **wait (retryable)**; never auto-merge without it.
- Merge is the **one gate that blocks** — Phase 1 a human/permission verdict
  (consistent with the `gh repo create` wall in HFTASK-0001 NEEDS-HUMAN),
  Phase 2 the swarm verdict.

#### 5a. Guardrails adopted from gh-aw (Research §R4)

GitHub's own agentic-workflow system (`gh-aw`) independently arrived at this
exact worker/reviewer/merge split and hardened it. We adopt its model:

- **Separation of privilege (the #1 rule).** The implementer/worker agent runs
  **read-only** — it never holds a write or merge token. It emits the change as
  a **structured intent** (branch + diff + task ids); a **separate, trusted,
  narrowly-scoped job** performs the actual `gh pr create`/push. The reviewer
  likewise emits an **approve/deny decision as data**, and a separate gate job
  acts on it. Agents never hold the merge token — even in the Phase-2 swarm.
- **The reviewer verdict stays OUT-OF-BAND** — recorded in the weave
  review/permission state, **not** as a native GitHub `APPROVE`. A bot APPROVE
  silently counts toward branch-protection required-reviews and would *defeat*
  the gate (gh-aw issue #25439). Native reviews default to `COMMENT`.
- **Merge is a non-agent, Environment-gated job** with its own scoped token.
  This makes the human→swarm transition a change to *who approves the
  Environment*, not a change to any agent's capabilities.
- **A detection/validation step runs between agent output and any write** —
  scan the diff for secrets, protected-file edits, and policy violations before
  the PR is created or merged.
- **Created PRs are draft-by-default**, with a **protected-files denylist**
  (CI config under `.github/`, the loop's own `.handoff/policy.toml` + ADRs,
  credential/manifest files) so a confused or compromised agent — especially
  once the human gate is lifted — cannot rewrite its own guardrails.
- **Least-privilege, per-action tokens**; no broad `write-all`. Network egress
  allowlisting for any agent that touches the internet. Explicit `noop`/
  `missing-tool` reporting so a stuck stage fails loudly, never silently.

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
- The **PR-write side is greenfield** (Research R3): PR create/merge, the
  review-gate policy, develop→master/ff, and fork support have no existing impl;
  `flexnetos_github_app` is an empty placeholder. Worktree mechanics are *not*
  greenfield — reuse `meta_git_lib`.
- More verbs and config surface to maintain and test.

## Research & Cross-References (evidence base)

> Per process rule: every ADR must be backed by deep web + codebase research,
> cross-referenced. This section records the evidence behind the decisions above.

### R1 — Prior-session design lineage (codebase)

This ADR continues a same-day research line; the upstream artifacts (all under
`~/Desktop/meta/`):

- `RUVECTOR-RUNBOOK.md` — RuVector crate-map runbook (314 crates walked with an
  agentic-role lens). Establishes that `rvAgent` (`rvagent-core/subagents/a2a`)
  is the **agent-swarm substrate** for the Phase-2 `swarm_local` reviewer (§5).
- `RUVECTOR-META-MAPPING-S1.md` — maps the 12-crate Ark Handoff Ledger onto
  existing RuVector/meta capabilities; ~8/12 needs already have production
  equivalents → "adopt what's built." Source of the `handoff.task.v1` envelope
  and the Git > ledger > task-cards precedence.
- `STACK-INTEGRATION-PLANS.md` — three integration plans (contract-/store-/
  door-first); the real blocker is *connectors*, not more tools.
- `SESSION-HANDOFF.md` — locks the "Continuity Ledger Kernel" / `.handoff`
  naming and seeds HFTASK-0001..0006.

### R2 — Naming + dotdir audit (codebase, 65 repos under `~/Desktop/meta`)

- **`.handoff` is canonical**; `.hf` exists in **no** repo. Resolves the open
  question: binary = `hf`, dotdir = `.handoff`.
- Worktrees are an established meta concept: `.worktrees/` at the meta root
  holds ~38 tracked branch sets; `.meta` / `.meta-snapshots` / `.looprc` are
  meta-CLI/loop owned — confirming §2's "tracked set, not ad-hoc tree."
- Drift flagged (not handoff's to fix, logged for the workspace): `.agent`
  (singular) vs `.agents`; `.codex-plugin` coexisting with `.codex` in ECC;
  `grit/.rtk` vs expected `.roo`.

### R3 — Reuse map (codebase: meta_git_lib / meta_git_cli / loop_lib)

Walk of the named projects (note: `meta_git` and `meta_projects_cli` do **not**
exist — actual crates are `meta_git_cli/lib`, `meta_project_cli`):

| hf need | Reuse from | Symbol |
|---|---|---|
| worktree add/remove/prune | `meta_git_lib` | `worktree::git_ops::*`, `meta_git_cli/commands/worktree/create.rs:handle_create` |
| worktree registry + TTL/ephemeral | `meta_git_lib` | `worktree::store` (`~/.meta/worktree.json`, locked) |
| lifecycle hooks (attach PR steps) | `meta_git_lib` | `worktree::hooks::{fire_post_create,fire_post_destroy,fire_post_prune}` |
| branch resolution / fetch / ahead-behind | `meta_git_lib` | `worktree::helpers::resolve_branch`, `git_ops::{git_fetch_branch,git_ahead_behind}` |
| snapshot checkpoint/rollback | `meta_git_lib` | `snapshot::{capture_repo_state,restore_repo_state}` |
| gitignore worktrees | `meta_git_lib` | `worktree::helpers::ensure_worktrees_in_gitignore` |
| start work from a PR | `meta_git_lib` | `worktree::helpers::resolve_from_pr` (`gh pr view --json headRefName`) |
| parallel cross-repo fan-out | `loop_lib` | `run`, `run_commands`, `JsonOutput` |

**Greenfield (no existing impl anywhere):** PR *create*, PR *merge* (squash/ff),
the review-gate policy, develop→master + fast-forward enforcement, fork-vs-clone.
`flexnetos_github_app` is an **empty placeholder repo** (remote configured, zero
commits) — a natural future home for the trusted PR-writer / merge-gate (§5a),
but it must be authored from scratch.

### R4 — External best practice (web: GitHub Agentic Workflows, `gh-aw`)

`github/gh-aw` (GitHub Next) independently validates the worker/reviewer/merge
split and supplies the §5a guardrails. Load-bearing findings:

- **Separation of privilege:** the agent job is read-only and emits structured
  "safe-output" intents; separate scoped-write jobs execute them after a
  threat-detection pass. "Even a fully compromised agent cannot directly modify
  repository state."
- **No merge safe-output exists by design** — gh-aw deliberately keeps PR merge
  a human/branch-protection decision. → our merge stays a non-agent gated job.
- **Bot-approval bypass (issue #25439):** a `github-actions[bot]` `APPROVE`
  counts toward required-reviews; keep the reviewer verdict out-of-band.
- Draft PRs by default; protected-files guard (blocks `.github/`, `CLAUDE.md`,
  manifests); Environments as the human-in-the-loop gate primitive (swap the
  approver to flip human→swarm); network egress allowlist; keep source +
  compiled artifact in sync.
- Sources: github.com/github/gh-aw · githubnext.github.io/gh-aw (overview,
  architecture, safe-outputs, safe-outputs-pull-requests, triggers, tokens) ·
  gh-aw issue #25439.

### R5 — Phase-2 swarm reviewer feasibility (codebase: RuVector/rvAgent, ruflo)

The `swarm_local` reviewer (§5 Phase 2) is **feasible but partly greenfield**:

- **Working & reusable:** A2A transport (signed `AgentCard`, `PeerRegistry`,
  HTTP JSON-RPC, per-node budget + `recursion_guard`) — `examples/a2a-swarm` is
  a passing acceptance test that spawns/discovers/dispatches/tears-down real
  `rvagent a2a serve` processes. Reusable verdict TYPES:
  `rvagent-middleware/hitl.rs::ApprovalDecision{Approve,Deny,ApproveWithModification}`;
  `verified-applications/agent_contracts.rs::GateResult{allowed,reason,receipt}`
  (**provable** via `ruvector-verified` Lean contracts);
  `cognitum-gate-tilezero/replay.rs::GateDecision{Permit,Deny,Defer}` (+ witness
  replay). Reviewer-output hardening: `SubAgentResultValidator` (injection/length
  guards). Role "lenses" exist as persona cards in `.ruv/agents/rvagent-{security,
  tester,coder,queen}.md`.
- **Missing (build it):** the **N-reviewers → one approve/deny reducer** (quorum
  / any-blocker-vetoes / weighted-by-lens) — no such function exists; ~50–100
  LOC over the existing types. Also: `SubAgentOrchestrator::spawn_sync` is a
  STUB (use the process-level `rvagent a2a serve` path instead), lens→model
  wiring, and a diff→reviewer-input adapter.
- Seam options: depend on the Rust crates, or the `rvagent-mcp` MCP server, or
  the `rvagent` CLI (process-per-lens). For provable+witnessed verdicts, pair
  `enforce_contract` with the `.handoff` `rvf-crypto` WitnessChain.

### R6 — Out-of-band verdict mechanism (codebase: weave review/permission) — GAP

§5/§5a assume weave can hold the review verdict + the merge-permission verdict
out-of-band. Verification (weave-mcp-daemon-tools):

- **`weave permission` (WL-021): ✓ sufficient.** `weave_ask_permission` +
  `weave_permission_status` record an out-of-band verdict
  `Pending|Approved|Denied|Timeout` (Approved iff the answer body == "approve",
  case-insensitive; unanswered past `timeout_secs` → Timeout = denied). Stored in
  the `asks`+`messages` tables, **not** a GitHub approval — exactly the
  bypass-avoiding channel §5a wants.
- **`weave review` (WL-020): ✗ no verdict field.** `ReviewItem` tracks only
  `state{Open,Merged,Closed}` + `reviewed_at` + `reviewed_by` — i.e. *reviewed
  vs not*, with **no Approve/RequestChanges/Deny**. It also does not *enforce*
  anything (records, doesn't gate), and permission asks carry no PR/branch ref.
- **Resolution (decided):** do NOT depend on `weave review` for the verdict.
  Carry the reviewer's approve/deny **in the `weave permission` answer body**
  (e.g. `approve` / `deny: <reason>`) and/or record it as a `review_verdict`
  **event in hf's own ledger** (the authoritative store). Use `weave review` only
  as the human-facing PR queue. Optionally file a separate weave task to add a
  `verdict` column to `ReviewItem` later. The merge gate is enforced by **hf**
  reading the permission status + its own `review_verdict` event — weave does not
  gate.

### R7 — `.kb` + meta sync mechanics (codebase: .meta.yaml, .gitignore, git kb)

§6/HFTASK-0011 verified against the live workspace:

- **Part A already done:** `handoff` is **already** registered in
  `~/Desktop/meta/.meta.yaml` (`projects.handoff.repo = git@github.com:FlexNetOS/handoff.git`)
  and ignored in `~/Desktop/meta/.gitignore` (with a **duplicate** `handoff/`
  line to clean). There is **no `meta project add`** (the `project` plugin is
  read-only: list/check/dependents) → registration is a guarded file edit. So
  `hf sync` Part A = **idempotent ensure/repair**, grep-guarded, never blind
  append.
- **Part B — no upsert:** `git kb create` errors on an existing slug. The
  idempotent push is **show-or-create → checkout → full-overwrite body →
  commit**, scoped to the two generated slugs `context/overridable/active` and
  `context/overridable/progress` (both already exist, type `context`). Preserve
  frontmatter `id` (checkout first; never `rm`+recreate — it breaks UUID/version
  lineage). The DB (`.kb/store/`) is the source of truth; `.kb/workspaces/` is
  scratch (already git-ignored).
- **One-way discipline (structural, hf-enforced):** hf only ever *writes* those
  generated slugs, **full-overwrites** from a ledger-derived body, **never reads
  `.kb` back as truth**, tags them `generated`, and never touches
  `context/immutable/*`, `context/extensible/*`, or `tasks/*`. `git kb status`
  before checkout on shared slugs; never `--force` on shared docs. MCP `kb_*`
  tools are an optional nicety — the `git kb` CLI is the portable contract.

## Task breakdown

| Task | Pillar | Quick-note items |
|------|--------|------------------|
| **HFTASK-0007** | `hf session` via `meta git worktree` + `policy.toml` + sync **preflight** | 2, 3, 7 |
| **HFTASK-0008** | branch/remote policy engine (develop↔master, clone/fork) | 6 |
| **HFTASK-0009** | batch checkout (3–5 tasks) + cycle counter → `hf ship` (one squash commit/PR) | 4 |
| **HFTASK-0010** | PR review/merge automation — phased cloud_ultra→swarm_local + permission gate | 5 |
| **HFTASK-0011** | `hf sync` — `.meta.yaml` + `.gitignore` + `.kb` mirror | 1 |

Dependencies: 0008 → 0007; 0009 → 0007/0008; 0010 → 0009; 0011 → 0007.
