---
name: fleet-handoff
description: "Repo-per-.handoff control: roll out and maintain the .handoff continuity protocol across the fleet of sibling repos as a GIT-TEXT-ONLY layer (no per-repo ledger.db; events live in the FLEET ledger meta/.handoff — ADR-0004 §3, policy P7). ALWAYS use to install .handoff into a target repo, audit fleet conformance, reconcile a fleet repo's drift, or eject the kernel harness into a repo. Do NOT use for the handoff repo's own task loop (that's handoff-loop)."
---

# fleet-handoff — one conforming .handoff per repo, all witnessed

The kernel proven in `handoff/` is rolled out so **every fleet repo carries its own
git-text `.handoff` control surface** (ADR-0004 fleet rollout, policy P7). Same
continuity guarantees, repo by repo — but the witnessed *events* live in the shared
FLEET ledger (`meta/.handoff/ledger.db`), never in the repo. This skill installs,
maintains, and reconciles that surface.

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

## Ledger residency — read this before anything (ADR-0004 §3, settled)

**Per-repo `.handoff/` is git-committed TEXT ONLY — never a `ledger.db`, never binary
state.** This is the beads lesson (binary DB never in git; JSON/JSONL text is the
git-visible state) and it is *decided*, not optional. There is **one witnessed
ledger per orchestration home**:

| Ledger | Path | Holds | A repo's events go here |
|--------|------|-------|-------------------------|
| **FLEET** | `meta/.handoff/ledger.db` | fleet/member events (run `hf` from `meta/`) | ✅ fleet repos checkpoint here |
| **KERNEL** | `meta/handoff/.handoff/ledger.db` | handoff's own self-dev (23 HFTASK) | kernel work only |

A fleet repo (flexnetos_runner, envctl, …) has **no ledger of its own**; its
witnessed events are checkpointed into the FLEET ledger, and its `packets/` are
compiled centrally by `hf fleet status` (see below) — never rendered from a per-repo
ledger. Creating a `<repo>/.handoff/ledger.db` is a **policy P7 violation** — remove
it on sight.

## Conformance: a conforming per-repo `.handoff` (Tier A/B = full)

| Component | Path (per repo) | Notes |
|-----------|-----------------|-------|
| Capsule | `<repo>/.handoff/context/capsule.json` | **REQUIRED** (`handoff.context_capsule.v1`: project_name, role, plane, …) — git text |
| Tasks | `<repo>/.handoff/tasks/*.task.json` | minted cards (git text); status synced from the FLEET ledger via `hf checkpoint --sync-cards` |
| Packets | `<repo>/.handoff/packets/` | compiled **centrally** by `hf fleet status` (unbuilt) — not locally rendered |
| README | `<repo>/.handoff/README.md` | git text |
| Hooks/Policies/Skills | `<repo>/.handoff/{hooks,policies,skills}/` | **OPTIONAL** (only when the repo runs autonomous loops) — static declarative text |
| **NO ledger.db** | — | per ADR-0004 §3: no binary state in a per-repo `.handoff/` |

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
   `.meta.yaml` default; never `https://` (it fails the workspace's auth). **Do NOT run
   `hf init`/`hf seed` in the repo** — those create a per-repo `ledger.db` (forbidden)
   and `seed` stamps handoff's own backlog.
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
1. Has a git-text `.handoff/` (REQUIRED capsule.json present)? **Does it wrongly
   contain a `ledger.db` / binary state?** → policy P7 violation, remove it.
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

## The core invariant: no per-repo ledger; events live at the orchestration home

A fleet repo's git-committed text (capsule + cards + README) is its visible state;
its witnessed *events* live in the **FLEET ledger** (`meta/.handoff/ledger.db`), not
in the repo. There is exactly one ledger per orchestration home (FLEET at `meta/`,
KERNEL at `meta/handoff/`). `hf fleet status` is the join that compiles a board (and
each repo's packet) from `../.meta.yaml` members + their capsules/cards + fleet-ledger
events; **Git is the sync transport**. State precedence stays **Git > ledger >
cards**. If you ever find a `<repo>/.handoff/ledger.db`, it is a P7 violation — remove
it (the events belong in the FLEET ledger).
