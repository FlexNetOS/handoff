# ADR-0004 — fleet `.handoff`: per-repo continuity layer + central coordination (policy P7)

**Status:** accepted (2026-06-12) · **Owner:** handoff kernel · **Derived from:** vision items 2/13
(UPGRADE-MISSION-PROMPT.md), the original design bundle (`~/Downloads/tmp/handoff` — schemas, templates,
capsule), ADR-0001 §2/§7 + R3/R9, ADR-0003, open-questions #13 (session-ledger location),
ARCHITECTURE-TRUTH.md census.

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
3. **Ledger residency (settles open-questions #13).** There is **one witnessed ledger per orchestration
   home**: `handoff/.handoff/ledger.db` is the meta-fleet ledger. Per-repo `.handoff/` carries **no
   ledger.db and no binary state — git-committed text only** (the beads lesson: binary DB never in git;
   JSONL/JSON text is the git-visible state). Worktree/session ledgers are ephemeral; canonical events
   are checkpointed into the fleet ledger (the proven `pr_opened` pattern from the 2026-06-12 loop
   proof). Session events adopt the **`handoff.session_event.v1`** vocabulary from the design bundle
   (12 event types: session_started/resumed, task_claimed, lease_heartbeat, command_run, files_changed,
   checkpoint_created, tests_run, drift_audited, handoff_created, lease_released, session_stopped) —
   implemented by HFTASK-0007 (`hf session start|end`), which also owns `.handoff/policy.toml`
   (merging the design bundle's `rules.toml` sections with R3's remote/loop/merge keys).
4. **Aggregation = `hf fleet status`.** Enumerate members from `../.meta.yaml`, read each repo's
   `.handoff` (capsule + cards), join with fleet-ledger events → one board. **Git is the sync
   transport** — no daemons, no new services; `meta git update` pulls fleet state naturally; precedence
   stays Git > ledger > cards. (Beads cross-validation: same transport choice, same derived-view
   discipline, plus our witness chain on top.)
5. **Card-sync rule** (fixes defect D3 permanently): cards are derived snapshots;
   `hf checkpoint --sync-cards` rewrites card status from ledger truth (ADR-0003 rule 4). First
   implementation pass refreshes the kernel's 22 stale cards and replaces dead `spike/**` path-scopes.
6. **Policy P7** (added to META-ORG-POLICY.md): per-tier presence requirements; capsule REQUIRED
   fields; minted-cards-only rule; ledger residency; no binary state in git; rival-convention
   deprecation for new state.
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
