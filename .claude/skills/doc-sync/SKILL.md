---
name: doc-sync
description: "Keep docs in sync with code/decisions after any landed change: FLEET_GUIDE, ADRs, AGENTS.md, READMEs, CLAUDE.md change history, and (by regeneration) capsules/packets/cards. ALWAYS use after a verb/flag/ADR/agent/skill lands, when a doc says 'planned/TODO' for something that now exists, or before a handoff so rendered views reflect reality. Do NOT hand-edit derived views — regenerate them."
---

# doc-sync — bring docs back to truth, minimally

Docs rot silently and then mislead the next cold-start agent. After any change, sync the
affected docs — accurately, by regenerating derived views, never by hand where a
generator exists.

## What to touch (by change type)

| Change | Update |
|--------|--------|
| new/changed `hf` or `grit` verb/flag | `FLEET_GUIDE.md` verb tables, `AGENTS.md` verb list, usage strings — verify against `hf --help` / `grit --help` |
| "planned / not yet implemented / TODO" now true | flip it to implemented; cite the PR |
| new ADR | link it from related ADRs/tasks; add to FLEET_GUIDE cross-refs |
| new agent/skill | **CLAUDE.md change-history row** (date/change/target/reason) |
| blocked-on-X | name the *real* blocker (e.g. "BLOCKED on envctl Phase 8"), not vague |

## Regenerate, never hand-write (derived views)

| View | Regenerate with | Never |
|------|-----------------|-------|
| task cards | `hf checkpoint --sync-cards` / `hf sync-cards` | hand-edit status |
| packet + active.md | `hf handoff` (from the **real** ledger — run in the repo that owns it) | hand-edit |
| a fleet repo's packet | `hf fleet render <repo>` | hand-write |
| `.kb` active/progress | `hf sync` (one-way ledger→kb) | edit kb then call it truth |

Gotcha: regenerate `active.md`/packets from the **repo that owns the ledger** — a
worktree has its own gitignored ledger and will render wrong counts (verified failure).

## Procedure

1. Read the landed change (diff/PR) + the cycle's agent outputs.
2. For each affected doc, update it — verify every claim against the live surface (run
   the `--help`, read the code). No unverified assertions in docs (process rule).
3. Regenerate derived views from truth; leave markdown-fallback + a note when a
   generator verb is unbuilt.
4. Add the CLAUDE.md change-history row.
5. Write `_workspace/09_docsync_<scope>.md`: every doc touched + any doc↔code mismatch
   (a finding for the verifier/gatekeeper).

## Principles

- Write for a cold-start reader: scannable, why over what, match surrounding voice.
- Don't bloat — delete stale lines, don't pile new ones on top.
- Archive over delete; never remove a doc you didn't author without surfacing it.
- A doc that contradicts the code is a finding, not a silent overwrite — flag it.
