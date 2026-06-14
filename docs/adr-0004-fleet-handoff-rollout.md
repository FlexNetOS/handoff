# ADR-0004 — fleet `.handoff`: per-repo continuity layer + central coordination (policy P7)

**Status:** accepted (2026-06-12) · **REVISED 2026-06-13** (owner-directed: ledger residency §3.3 + policy
P7 §6 — per-repo gitignored ledger + central rollup; supersedes the original central-only rule) · **Owner:**
handoff kernel · **Derived from:** vision items 2/13 (UPGRADE-MISSION-PROMPT.md), the original design
bundle (`~/Downloads/tmp/handoff` — schemas, templates, capsule), ADR-0001 §2/§7 + R3/R9, ADR-0003,
open-questions #13 (session-ledger location), ARCHITECTURE-TRUTH.md census. **Revision research:**
`_workspace/research_adr-0004-rev.md` — cited: [RFC 6962 / Certificate
Transparency](https://www.rfc-editor.org/rfc/rfc6962.html) (local→aggregate append-only model), the git
object model (immutable objects + provenance), beads dual-store, grit per-worktree SQLite.

## Context

The census measured `.handoff/` in **1 of 58 repos**. Loop state that exists is split across rival
conventions: `_workspace/` (weave, prompt_hub, ECC, n8n, rusty-idd), `.lane-loop/`, `/wrap-up`
(.github_org), `.handoff/` (kernel only) — plus one genuinely broken handoff (lifeos, dead paths).
Meanwhile the original design bundle already specified the per-repo dotdir layout
(`.handoff/{tasks,packets,context,decisions}`, `context_capsule.v1`, `hooks.v1`, `policy.rules.v1`,
`session_event.v1`) — partially dropped for the kernel spike (R9). Vision item 2 mandates every repo
host `.handoff` under meta policy; item 13 asks how per-repo dirs coordinate with the central kernel.

## Decision

1. **Canonical dotdir = `.handoff/` fleet-wide.** Rival conventions are deprecated for *new* state;
   existing `_workspace/` content is migrated opportunistically (explicit migration list below), never
   bulk-deleted (history preserved).
2. **Tiered contents** (policy P7):
   - **Tier A canon + Tier B FlexNetOS tools (full):**
     `context/capsule.json` (REQUIRED — `handoff.context_capsule.v1`: `project_name`, `role`, `plane`,
     `northstar`, `next_command`; seeded from the census so any agent landing in any repo learns its
     place in one read), `tasks/` (minted cards only, per ADR-0003), `packets/` (resume packets),
     `README.md` (one-screen contract: what this dir is, precedence rule, pointer to the kernel).
     OPTIONAL per repo when it runs autonomous loops: `hooks/hooks.toml` + `policies/rules.toml`
     (the design-bundle templates, revived from the R9 drop **for the fleet layer** — the R9 decision
     stands for the kernel spike itself).
   - **Tier C forks (stub):** `context/capsule.json` + `README.md` only — exactly one commit,
     merge-safe across upstream syncs, **no CI/policy forcing** (POLICY v2 Tier C discipline).
   - **Tier D hubs/docs (stub):** same as C.
3. **Ledger residency — REVISED 2026-06-13 (owner-directed; supersedes the 2026-06-12 decision; settles
   open-questions #13).** Continuity is **per-repo-first with central rollup**, restoring the full beads
   dual-store. The 2026-06-12 rule conflated two different things; only one was ever the real lesson:
   - ❌ A **git-committed binary `.db`** stays **BANNED** (merge conflicts, bloat, not diff-able — the
     real beads lesson). Git-visible state is text only.
   - ✅ A **gitignored, local `.handoff/ledger.db`** is **LEGITIMATE and expected** — it is the
     **source of record for that repo's witnessed history**, so a repo cloned standalone travels with its
     own continuity (the 2026-06-12 central-only rule detached a repo's history from the repo: a lone
     clone of, e.g., prompt_hub lost its witnessed past, which lived only in `meta/.handoff`).

   **(a) Per-repo local ledger** (gitignored, never committed) = per-repo source of record.
   **(b) Central FLEET ledger** (`meta/.handoff/ledger.db`) = the rollup/merge point + canonical
   cross-repo board. **(c) Feed = repo → central, one-way:** `hf sync` rolls up each repo's NEW events
   (those past a per-repo sync cursor) by **re-appending** them through the normal witnessed `append()`
   path — chains are never "merged"; self-contained events are re-appended, each re-chained onto the
   central tail. Every rolled-up central event carries provenance `(origin_repo, origin_seq,
   origin_action_hash)` so the per-repo chain AND the central chain each verify independently and any
   central event traces back to its repo. Rollup is idempotent (UNIQUE `(origin_repo, origin_seq)` + a
   per-repo cursor, single central transaction; re-running `hf sync` is a no-op). Zero changes to
   `rvf-crypto`. **(d) Precedence:** Git (intent/shape) > central FLEET ledger (canonical joined/ordered
   view) > per-repo local ledger (per-repo source of record). Cross-repo order = central arrival/`seq`
   order; `ts_ns` is advisory (clocks skew). **(e) Worktrees** follow grit's per-worktree local-SQLite
   model: each worktree's cwd-relative `.handoff/ledger.db` rolls up to central with a composite origin id.

   This restores beads' full dual-store (local binary DB + text-in-git + deterministic rollup) and the
   model mirrors Certificate Transparency (RFC 6962): N self-verifying append-only logs, an aggregator
   re-appends self-contained entries into its own log; ordering = leaf index, not wall-clock. Session
   events adopt the **`handoff.session_event.v1`** vocabulary (HFTASK-0007, `hf session start|end`).
4. **Aggregation + integrity gate = `hf fleet status`.** Enumerate members from `../.meta.yaml`, read
   each repo's `.handoff` (capsule + cards), join with fleet-ledger events → one board. **Git is the sync
   transport** — no daemons, no new services; `meta git update` pulls fleet state naturally; precedence
   stays Git > ledger > cards. (Beads cross-validation: same transport choice, same derived-view
   discipline, plus our witness chain on top.) Per the §3.3 dual-store, `hf fleet status` verifies the
   three integrity layers (HFTASK-0033): **(i)** the central chain (`verify_witness_chain`), **(ii)** each
   member's per-repo chain standalone, and **(iii)** rollup-provenance faithfulness
   (`verify_rollup_provenance` re-derives each rolled-up row's action hash and byte-compares it to the
   stored `origin_action_hash`) — so both chains verify independently and any central event traces to its
   origin repo; a broken bridge surfaces as a warning.
5. **Card-sync rule** (fixes defect D3 permanently): cards are derived snapshots;
   `hf checkpoint --sync-cards` rewrites card status from ledger truth (ADR-0003 rule 4). First
   implementation pass refreshes the kernel's 22 stale cards and replaces dead `spike/**` path-scopes.
6. **Policy P7 — REVISED 2026-06-13.** Per-tier presence requirements; capsule REQUIRED fields;
   minted-cards-only rule; rival-convention deprecation for new state. **Ledger residency (flipped):** a
   *git-committed* binary ledger is BANNED; a *gitignored* local `.handoff/ledger.db` is LEGITIMATE and
   expected. Enforcement gates on **git-TRACKED** `.db` under `.handoff` (fail) + requires the
   `.handoff/**/ledger.db` `.gitignore` guard (fail if missing); a `.db` merely present on disk is **not**
   a violation. `hf fleet status` flags only a *tracked* per-repo ledger. **Cross-fleet:** this reverses
   envctl `ci/gates/p7.sh` Gate 3b (the "no per-repo *.db" check, PR #56) and the "no local ledger.db"
   member rule shipped to prompt_hub (#82/#83) and lane (#29) — coordinated follow-ups (see backlog).
7. **Rollout mechanics:** deterministic generator (census rows → capsules; no agent creativity in the
   payload), one branch + PR per repo (`chore: seed .handoff continuity layer (P7)`), auto-merge where
   armed, direct merge where a repo has no required checks; `meta git snapshot create` before the
   batch. Tier C/D = one-commit stubs. lifeos's broken `HANDOFF.md` is superseded by its capsule (D9).

## Migration list (opportunistic, not forced)

| Repo | From | Action |
|---|---|---|
| weave, prompt_hub, ECC, n8n, rusty-idd | `_workspace/` | keep history; new state → `.handoff/`; capsule points at old dir until moved |
| lane | `.lane-loop/` | same (loop is TERMINAL DONE — capsule records that verdict) |
| .github_org | `/wrap-up` | same; capsule notes the umbrella-dissolution task |
| lifeos | broken `HANDOFF.md` | capsule supersedes; dead paths noted (D9) |

## Consequences

- Items 2 + 13 close together: presence (every repo) and coordination (capsule + cards in git, events
  in the fleet ledger, `hf fleet status` as the join) are one design.
- loop_lib and every canon member gain the continuity layer (vision: "the original member is not left
  behind"); the autonomous-upgrade path for loop_lib itself is its capsule's `next_command`.
- New hf verbs to implement: `fleet status`, `task mint --from-kb`, `checkpoint --sync-cards`
  (+ HFTASK-0007 `session start|end` as already carded). All witnessed, no daemons.
- ~58 small PRs across the org (one per repo). Auto-merge + green checks gate each; forks take exactly
  one commit of divergence (recorded, merge-safe).

## Research / Cross-References

Design bundle (verified 2026-06-12): `~/Downloads/tmp/handoff/handoff/schemas/{task,session,packet}.schema.json`,
`templates/.handoff/{hooks/hooks.toml,policies/rules.toml,skills/session-resume.skill.md,tasks/TASK-0001.task.yaml}`,
`.handoff/context/capsule.json`, `roadmap/backlog.yaml` (LITE-check verdict: bundle = design ancestor,
zero code; kernel = the upgrade; absorption items tracked). ADR-0001 §2 (R3 worktree engine), §7
(ledger as read-model), R9 (hooks/policies/skills dropped for the spike — scope-limited here);
HFTASK-0007 card (session verb + policy.toml + sync preflight); open-questions #13 (worktree-ledger
pr_opened precedent); ARCHITECTURE-TRUTH.md (1/58 measurement, convention split, D3/D9);
beads — github.com/gastownhall/beads + steve-yegge.medium.com "Introducing Beads" (dual-store:
binary DB local, JSONL in git, deterministic export, ready-list computed for the agent — validates
rules 3–5); POLICY v2 tier model (policy-v2-meta-org); memoir: architecture-truth-census-2026-06-12.
