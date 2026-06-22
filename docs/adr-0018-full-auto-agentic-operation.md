# ADR-0018 — Full-auto agentic operation: committed dotfiles, worktree-per-batch, context-budget loop, fleet-central hooks/relay, full `.kb` adoption

- **Status:** Accepted (owner-authorized 2026-06-21)
- **Deciders:** owner directive → handoff kernel
- **Amends:** ADR-0004 §3/§6 (ledger residency), ADR-0016 (durability/swallow-guard ignore set), ADR-0009/0010 (grit), ADR-0013 (meta conventions), ADR-0001 §4 (cycle-flush loop budget)
- **Supersedes (in part):** the *selective-ignore* half of the residency model — HFTASK-0035/0037/0048/0021/0053/0066 guards that gitignore `.handoff` derived/state files. The local ledger and derived views are no longer gitignored; see Decision 1.

## Context

**North Star (operating target):** *full-auto agentic implementation — the human in the loop is
replaced by a designated agent.* The user directs; the fleet builds, verifies, ships, and hands off
with no human gate. `NEEDS-HUMAN` shrinks to genuine owner-walls (physical/account/irreversible/
scope-expanding) and is otherwise a scaffold a model with the owner's skillset replaces (root
`NORTH-STAR.md`).

Reaching that target surfaced a set of concrete gaps the owner flagged (2026-06-21). This ADR records
the decision on each, supersedes the prior policy where it conflicts, and enumerates the tracked
HFTASK cards that implement them. **No decision here is free-handed into code — each ships as a
witnessed card (Implementation table).**

The motivating issue cluster:
1. handoff's residency model tells the fleet to **gitignore** `.handoff` state (ledger.db, active.md,
   packets, locks, deliveries). Across the fleet this means a fresh clone carries an almost-empty
   `.handoff` — continuity/derived state does not travel with git, defeating cross-fleet sync. The
   same selective-ignore mindset left `.idea/`, and `meta/.kb` adoption, half-wired.
2. The loop wraps after a fixed **4 tasks** (`cycle_flush`), regardless of how much context budget is
   actually left — it under-uses the window (wraps early) and gives the next session thin direction.
3. The pre/post hooks (`loop-entry`/`session-end`/typed `hooks.toml`) are minimal and not centrally
   formatted/deployed, so fleet members drift.
4. `session-relay-resume`/`-wrap-up` formatting is owned per-repo (harness_hub), not centrally by
   handoff, so cross-fleet handoffs are inconsistent.
5. grit and GitHub are wired but shallow; worktrees are not yet created per batch nor reaped on merge;
   the develop→trunk promotion still stalls and needs manual ff.

## Decision

**D1 — Commit all dotfiles and directories (reverse the selective `.handoff` ignore).**
Moving forward every dotfile/dotdir is **tracked in git**: `.handoff/` (including the ledger and
rendered views), `.idea/`, `.claude/`, `.github/`, `.kb/`, `.grit/` config. The kernel stops
*writing* residency-ignore guards and instead **ensures these paths are tracked**. Consequence: a
fresh clone of any member carries its full continuity state; cross-fleet sync rides git.
- `hf init`, `scripts/fleet-rollout.sh`, and `scripts/handoff-lib.sh` no longer emit the
  `.handoff/**/ledger.db` (+ wal/shm/rvf/active.md/locks/deliveries) ignore block; existing blocks
  are removed.
- `hf fleet status` P7 (HFTASK-0034) **inverts**: a *missing* committed continuity truth becomes the
  violation where committing ledger state was previously banned (see Resolution below for the exact
  gate).
- The redb cutover migration artifacts (`*.sqlite.bak`/`*.redb.tmp`, HFTASK-0053) are **relocated
  out-of-tree** rather than committed (already true for `hf migrate` since PR #114) — only durable
  state is committed, never scratch.
- **Binary-churn caveat (Consequences):** `ledger.db` is a binary redb file; committing it means
  parallel worktrees can conflict on it. The implementing card decides the conflict story (commit
  binary + merge=ours-then-replay, or commit a deterministic text export beside it).

> **Resolution (HFTASK-0067, implemented).** The conflict story is settled on the **deterministic
> text-export** branch — the engineering best-practice choice (never commit a churning binary DB):
> - **Committed continuity truth = `.handoff/ledger.events.jsonl`** — the deterministic, seq-ordered
>   JSONL export of the witnessed ledger (increment 1, PR #121). It diffs/merges in git, and a fresh
>   clone re-derives its binary cache via `hf import` (fail-closed witness re-verify).
> - **Binary `ledger.db` (+ `*.rvf` sidecar) stays a gitignored LOCAL REBUILD CACHE.** "Commit all
>   dotfiles" means all *durable* state; the binary is a cache (category: like `target/`), so the
>   binary-residency guards from HFTASK-0034/0035 **remain valid** (a tracked binary `.db` is still a
>   violation — commit the JSONL, not the binary).
> - **Rendered views (`packets/`, `active.md`, `deliveries/`) move from ignored → committed** so a
>   fresh clone / external observer sees rendered state without running `hf`. The durability taxonomy
>   (ADR-0016 / `durability.rs`) moves them DURABLE; `repair_gitignore` strips the retired view
>   ignores on migration; `swallow_report` flags any repo still ignoring them.
> - **P7 inversion (exact gate):** a member with a local ledger on disk whose `.handoff/
>   ledger.events.jsonl` is **NOT git-tracked** is the violation (`jsonl_export_missing` — run
>   `hf export` + commit). `hf export` is wired into the session-end hook so the committed truth stays
>   fresh. The binary stays out-of-tree, so worktree-per-batch (D10) never conflicts on it.

**D2 — Better pre/post hook design for agent automation.**
A single, centrally-formatted hook contract covering SessionStart/SessionResume/SessionEnd,
Pre/PostCommand, Pre/PostTest, PostHandoff (typed `hooks.rs` / `hooks.toml`, HFTASK-0052) with a
deployable canonical bundle. Hooks are idempotent, fail-closed, and identical fleet-wide because they
deploy from one handoff-central source (D5 mechanism).

**D3 — Context-budget loop wrap-up (~50% of the context window), not fixed task count.**
Replace "wrap after `cycle_flush` tasks" with "run until ~50% of the context window is consumed, then
checkpoint → handoff." `policy.toml [loop]` gains `context_budget_pct = 50` (and `wrap_strategy =
"context"`); `cycle_flush` remains an upper safety bound. Because the kernel binary cannot read the
agent's live context, enforcement is at the **loop-skill** layer (handoff-loop + session-relay-wrap-up
read the running token/context budget and trigger wrap at the threshold); the kernel exposes the
policy and the wrap verbs.

**D4 — More direction from handoff.**
`hf resume`/the rendered packet emit explicit next-action direction (the single next safe task, the
exact next command, the cycle/budget state, and the blocking walls) so a fresh agent needs zero
archaeology. The loop skill gives richer steering (decision rationale, "do this next", not just
"here's state").

**D5 — `session-relay-resume`/`-wrap-up`: handoff-central formatting + cross-fleet deployment.**
handoff owns the canonical format/templates for both relay skills; a deployment mechanism (the
`/handoff-loop-init` family, HFTASK-0065) pushes the canonical relay skills + format to every fleet
member so cross-fleet handoffs are byte-consistent. The relay skills render from the witnessed
ledger/packet, never hand-authored prose.

**D6 — Updated rules.** `handoff/.claude/rules/*` and the meta-level rules are updated to the
full-auto model (committed dotfiles, worktree-per-batch, context-budget, full `.kb` adoption,
grit+gh grounding). Rules are deployed fleet-wide via the same central mechanism (D5).

**D7 — Full adoption of `meta/.kb/AGENTS.md`.**
The FlexNetOS agent guide (`meta/.kb/AGENTS.md`, 765 lines: document-before-implement, context
docs, `git kb board`, traceability) is **fully adopted** in handoff: init the full `.kb` (handoff is
code-intelligence-only today), wire the create-first discipline into the loop, and bind the
planning↔execution seam (ADR-0003) both directions per the guide.

**D8 — Deeper grit + GitHub grounding.**
grit: the `hf claim → grit claim <file::symbol> → grit worktree → grit done` cycle (ADR-0009) is the
*default* path for every batch, with the shared backend (ADR-0010) advanced past degrade. GitHub:
fully automate develop→trunk (D11), ground the gatekeeper as a required status check (HFTASK-0010/0012),
and lean on gh-aw guardrails — no manual `gh api` ff.

**D9 — Real `.idea` integration.** `.idea/` is committed and actually used: shared run/debug
configurations, the Qodana inspection profile (`qodana.yaml`) wired as advisory CI, the Rust plugin
config. The single genuinely per-user file (`workspace.xml`) is the one debated item — committed per
D1 unless it proves to churn destructively, in which case the implementing card carves it out with a
recorded rationale (the only allowed exception to D1).

**D10 — Worktree per task batch; reaped on verified PR merge.**
Every task batch starts a **new worktree** (grit worktree, ADR-0009); it is removed **on verified PR
merge** (not before). Discarded/abandoned batches leave their worktree until reconciled. This makes
parallel batches truly isolated.

> **Resolution (HFTASK-0075) + D1 reconciliation.** The earlier rationale ("the precondition that
> makes D1's *committed binary* ledger safe") is reconciled with the **D1 Resolution (HFTASK-0067)**:
> committed continuity truth is the deterministic `.handoff/ledger.events.jsonl` export — the binary
> `ledger.db` (+`*.rvf`) is a **gitignored per-worktree rebuild cache**, never committed. So the D10
> isolation guarantee is *more* important, not less: each batch's worktree carries its **own** local
> `ledger.db` cache + checkout, so parallel batches never share a working ledger and never corrupt
> each other's witness chain/leases. The reap mechanism lives in `hf/src/session.rs`
> (`reap_decide`/`batch_merge_verified`/`retained_worktrees`): `session_end` consults `reap_decide`
> (decoupled from unconditional removal); `hf done --pr` reaps the open session on the witnessed
> `pr_merged`/`trunk_promoted` (the "removed ON verified PR merge" path, non-fatal); `hf session reap
> [--force]` sweeps retained (abandoned/in-flight) worktrees that have since merged. **Fail-closed:**
> an unconfirmed merge ⇒ `Keep`, never `Reap` — unmerged work is never destroyed; only an explicit
> `--reap`/`--force` reconcile override tears down a genuinely-abandoned batch. This also closes the
> open ADR-0009 follow-up ("wire the grit worktree lifecycle in `session.rs`").

**D11 — All PRs → `develop`; `develop` auto-merges to trunk.**
The pipeline is fixed: branch off `develop` → PR `--base develop` → on green, `develop`
auto-promotes to trunk with **no manual ff**. The stall that forced manual `gh api` ff
(`sync-master.yml`) is fixed (or replaced) so promotion is hands-off. **Naming:** the trunk is
currently `master`; the directive says `main`. The implementing card reconciles the name (standardize
on `main`, or keep `master` with `main` as the documented alias) — one decision, applied across
`policy.toml`, the workflows, and the docs.

**D12 — Full-auto target.** All of the above serve one end: the user directs; the fleet
implements/verifies/ships/hands-off autonomously; the human gate is a *designated agent* with the
owner's skillset. Witnessed verdicts (HFTASK-0014 gatekeeper) replace human approval; genuine
owner-walls still escalate.

## Consequences

- **Positive:** continuity travels with git (fresh clone = full state); cross-fleet handoffs are
  byte-consistent; the loop uses its budget instead of wrapping at 4; hooks/rules/relay are one
  canonical source deployed fleet-wide; parallel batches are worktree-isolated and reaped on merge;
  promotion is hands-off.
- **Negative / risks:** committing the binary `ledger.db` churns git and can conflict across
  worktrees (mitigated by D10 + rollup + the card's conflict story); committing `.idea/workspace.xml`
  may churn per-user (the one D1 exception); inverting `hf fleet status` P7 is a behavior change that
  every fleet member's conformance now depends on — must roll out atomically with the guard removal.
- **Migration:** the residency-guard removal (D1) and the P7 inversion ship together; until then
  members keep the old guards. The `/handoff-loop-init --fleet` path (HFTASK-0065) is the rollout
  vehicle for the new guards/hooks/rules/relay.

## Implementation (tracked HFTASK cards — seeded this ADR)

| Card | Decision | Scope |
|------|----------|-------|
| HFTASK-0067 | D1 | Reverse residency ignore → commit-all dotfiles/dirs; invert `hf fleet status` P7; `hf init`/`fleet-rollout`/`handoff-lib` ensure-tracked; ledger.db conflict story |
| HFTASK-0068 | D3 | `policy.toml [loop] context_budget_pct`/`wrap_strategy`; context-budget wrap in handoff-loop + session-relay-wrap-up; `cycle_flush` as safety bound |
| HFTASK-0069 | D2 | Central pre/post hook contract + deployable canonical bundle (all 8 events, idempotent, fail-closed) |
| HFTASK-0070 | D5 | handoff-central format/templates for `session-relay-resume`/`-wrap-up` + cross-fleet deploy |
| HFTASK-0071 | D4 | Richer `hf resume`/packet direction (next action/command/budget/walls) + loop steering |
| HFTASK-0072 | D7 | Full `meta/.kb/AGENTS.md` adoption: init full `.kb`, create-first discipline, ADR-0003 seam both ways |
| HFTASK-0073 | D8 | Deeper grit (default claim→grit→worktree→done) + GitHub gatekeeper-as-required-check grounding |
| HFTASK-0074 | D9 | Real `.idea` integration (run configs, Qodana advisory CI, Rust plugin); commit-all per D1 |
| HFTASK-0075 | D10 | Worktree-per-batch lifecycle; reap on verified PR merge; abandoned-batch handling |
| HFTASK-0076 | D11 | All-PRs→develop; hands-off develop→trunk auto-promotion (fix sync stall); master↔main naming reconcile |
| HFTASK-0077 | D6 | Update `handoff/.claude/rules/*` + meta rules to the full-auto model; fleet deploy |

Each card carries this ADR as its `kb_ref`/rationale. Cards are claimed and finished via the witnessed
flow (claim → checkpoint → `hf test` → develop-base PR → `hf done --pr`), worktree-per-batch (D10).
