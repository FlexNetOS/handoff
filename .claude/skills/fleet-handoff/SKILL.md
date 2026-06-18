---
name: fleet-handoff
description: "Repo-per-.handoff control: roll out and maintain the .handoff continuity protocol across the fleet of sibling repos (ADR-0004 §3/§6 rev, policy P7). A per-repo .handoff/ledger.db is LEGITIMATE when gitignored (local source of record that rolls up to the FLEET ledger); only a git-TRACKED .db or a missing .gitignore guard is a violation. ALWAYS use to install .handoff into a target repo, audit fleet conformance, reconcile a fleet repo's drift, or eject the kernel harness into a repo. Do NOT use for the handoff repo's own task loop (that's handoff-loop)."
---

# fleet-handoff — one conforming .handoff per repo, all witnessed

The kernel proven in `handoff/` is rolled out so **every fleet repo carries its own
`.handoff` control surface** (ADR-0004 fleet rollout, policy P7). Same
continuity guarantees, repo by repo — the witnessed *events* roll up into the shared
FLEET ledger (`meta/.handoff/ledger.db`). A repo may keep a **gitignored** local
`.handoff/ledger.db` as its source of record; only a *git-tracked* `.db` is banned.
This skill installs, maintains, and reconciles that surface.

## The fleet

Repos staged under `.handoff/fleet/`: Archon, ECC, RuVector, claude-code,
claude-plugins, codex, grit, hermes-agent, icm, kasetto, n8n, obscura,
obsidian-mind, oh-my-claudecode, oh-my-pi, rtk-tokenkill, ruflo, shimmy, teri, vox.
Each is an **independent git repo** (meta-repo, not monorepo) — one git-text
`.handoff` per repo; the witnessed ledger is shared (FLEET, at `meta/.handoff/`).

## Pilot scope gate (read FIRST, every rollout)

Before rolling out to ANY repo, read `.handoff/fleet/PILOT.toml`. If `active = true`,
rollout is **restricted to `targets` only** — do not touch the rest of the fleet,
even on a broad "roll out the fleet" request. The current pilot target is
**`flexnetos_runner`** (clean single-commit husk, minimal blast radius). Audits
(read-only conformance scans) may still cover the whole fleet; only *rollout*
(writes) is gated.

Widening the pilot (`active = false` or adding `targets`) **expands scope** → it
requires a witnessed gatekeeper verdict (`[promotion] requires_verdict = true`).
Never self-promote past the pilot.

## Ledger residency — read this before anything (ADR-0004 §3/§6 rev, settled)

**Per-repo `.handoff/` is git-committed TEXT ONLY for cards/capsule/README — never a
*tracked* `ledger.db` or binary state.** This is the beads lesson (binary DB never in
git; JSON/JSONL text is the git-visible state) and it is *decided*, not optional.
There is **one witnessed ledger per orchestration home**, plus optional gitignored
per-repo source ledgers that roll up into it:

| Ledger | Path | Holds | A repo's events go here |
|--------|------|-------|-------------------------|
| **FLEET** | `meta/.handoff/ledger.db` | fleet/member events (run `hf` from `meta/`) | ✅ fleet repos checkpoint here |
| **KERNEL** | `meta/handoff/.handoff/ledger.db` | handoff's own self-dev (23 HFTASK) | kernel work only |
| **per-repo (gitignored)** | `<repo>/.handoff/ledger.db` | local source of record, rolls up to FLEET | ✅ legitimate when guarded by `.gitignore` |

A fleet repo's witnessed events are **rolled up** into the FLEET ledger, and its
`packets/` are compiled centrally by `hf fleet status`. A `<repo>/.handoff/ledger.db`
that is **gitignored** is LEGITIMATE (the rollup model). The P7 violations are:
(a) a *git-tracked* `.handoff/*.db`, and (b) a missing `.handoff/**/ledger.db`
`.gitignore` guard that would allow a future commit.

## Conformance: a conforming per-repo `.handoff` (Tier A/B = full)

| Component | Path (per repo) | Notes |
|-----------|-----------------|-------|
| Capsule | `<repo>/.handoff/context/capsule.json` | **REQUIRED** (`handoff.context_capsule.v1`: project_name, role, plane, …) — git text |
| Tasks | `<repo>/.handoff/tasks/*.task.json` | minted cards (git text); status synced from the FLEET ledger via `hf checkpoint --sync-cards` |
| Packets | `<repo>/.handoff/packets/` | compiled **centrally** by `hf fleet status` (unbuilt) — not locally rendered |
| README | `<repo>/.handoff/README.md` | git text |
| Hooks/Policies/Skills | `<repo>/.handoff/{hooks,policies,skills}/` | **OPTIONAL** (only when the repo runs autonomous loops) — static declarative text |
| **gitignored ledger.db** | — | per ADR-0004 §6 rev: a *gitignored* local ledger is legitimate; only a *tracked* one is banned |

**Tiers (policy P7):** Tier A canon + Tier B FlexNetOS tools = full set above. Tier C
forks + Tier D hubs/docs = **capsule.json + README only**, one commit, merge-safe,
no CI/policy forcing.

## Rollout procedure (install .handoff into a target repo)

1. **Snapshot first** (multi-repo safety): `meta git snapshot create fleet-rollout-<repo>`.
2. **Ensure the repo is present + registered.** It must be in `.meta.yaml` +
   `.gitignore` (meta conventions). `meta git update` clones if missing.
3. **Eject the contract, don't fork — git-text only.** Write the repo's REQUIRED
   `context/capsule.json` (project_name/role/plane), `tasks/` + `packets/` dirs, and
   `README.md`; add OPTIONAL `hooks/policies/skills` (from the design-bundle
   templates at `~/Downloads/tmp/handoff/handoff/templates/.handoff/`) only if the
   repo runs autonomous loops. Adapt any `policy.toml` `[remote]` to that repo's
   origin/branches — **SSH form** (`git@github.com:FlexNetOS/<repo>.git`), matching the
   `.meta.yaml` default; never `https://` (it fails the workspace's auth). **If you run
   `hf init`/`hf seed` in the repo**, ensure the resulting `.handoff/ledger.db` is
   **gitignored** (ADR-0004 §6 rev); the fleet rollout script adds the guard for you.
4. **Events go to the FLEET ledger; packets are compiled centrally.** Witnessed
   events for the repo are checkpointed into `meta/.handoff/ledger.db` (run `hf`
   from `meta/`). The repo's `packets/` are compiled by `hf fleet status` (kernel
   verb, not yet built) — until it lands, the repo's git-text capsule+cards+README
   ARE the cold-start package (markdown-fallback). Card status syncs from the FLEET
   ledger via `hf checkpoint --sync-cards`. Never copy `handoff/`'s cards into a repo.
5. **Route through the gate.** Any change that lands in a fleet repo goes through the
   `code-omniscient-gatekeeper` (witnessed verdict) before merge.

## Maintenance / audit procedure

For each repo (or the named target), produce a conformance row:
1. Has a git-text `.handoff/` (REQUIRED capsule.json present)? **Is any `ledger.db`
   under `.handoff` git-TRACKED, or is the `.handoff/**/ledger.db` `.gitignore` guard
   missing?** → policy P7 violation (ADR-0004 §6 rev); fix by removing the tracked
   binary or adding the guard. A gitignored ledger on disk is legitimate.
2. Reconcile per-repo drift, scoped to one repo, with the FLEET ledger as the event
   source: **Git > FLEET ledger (`meta/.handoff`) > the repo's cards > its (centrally
   compiled) packet** → re-sync cards (`hf checkpoint --sync-cards`); packets await
   `hf fleet status`.
3. Avoid the drift other repos fell into (HFTASK-0016): conform to FlexNetOS meta
   conventions; use `meta git` / `meta exec` for cross-repo ops, never raw loops.
4. Write `_workspace/06_fleet_<scope>.md`: a per-repo table (capsule present? any
   forbidden ledger.db? cards in sync with the FLEET ledger? tier? action taken) +
   any repo needing escalation.

## Safety (multi-repo amplifies blast radius)

- **Snapshot before destructive ops**; `meta git snapshot restore <name>` to recover.
- **Target precisely** with `--include <repo>` — never blanket-operate the fleet.
- **Preview** with `meta --dry-run exec -- <cmd>` first.
- A repo unreachable/uncloned → `meta git update` once; still absent → mark PENDING,
  continue with the rest, note the omission (never silently drop a repo).

## The core invariant: git-text visible state; events roll up to the orchestration home

A fleet repo's git-committed text (capsule + cards + README) is its visible state.
Its witnessed *events* live in the **FLEET ledger** (`meta/.handoff/ledger.db`), either
written directly or rolled up from a **gitignored** per-repo `.handoff/ledger.db`
(ADR-0004 §6 rev). `hf fleet status` is the join that compiles a board (and each
repo's packet) from `../.meta.yaml` members + their capsules/cards + fleet-ledger
events; **Git is the sync transport**. State precedence stays **Git > ledger >
cards**. If you find a *git-tracked* `<repo>/.handoff/ledger.db`, that is a P7
violation — remove it. A gitignored ledger on disk is legitimate.
