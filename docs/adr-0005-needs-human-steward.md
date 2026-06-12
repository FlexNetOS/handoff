# ADR-0005 — the NEEDS-HUMAN steward (replacing the human with a witnessed agent role)

**Status:** accepted (2026-06-12) · **Owner:** handoff kernel · **Derived from:** the owner directive
"human-in-the-loop approvals are ALL replaced by a surgical AI gatekeeper with full code knowledge"
(memoir 01KTQRS2…, 2026-06-09), NORTH-STAR.md (meta root), the 2026-06-12 classifier boundary
discovery, and the starving NEEDS-HUMAN queue (0/7 items actioned across two verification passes).

## Context

NEEDS-HUMAN.md exists so genuine walls reach a human — but in practice the queue starves: 7 items, two
verification passes, zero actioned. Most items are not genuine walls; they are *decisions* (archive a
stub? register a fork? rename a repo?) that an agent with the right faculties can take. The owner's
standing directive mandates exactly this: approvals move to a surgical, code-omniscient AI gate. The
faculties now exist: **perfect recall** (ICM memories + the `system-architecture` memoir, ~110
concepts), **vision custody** (NORTH-STAR.md, distilled from the owner's 15-item register), **full
code knowledge** (gitkb code intelligence: callers/impact/symbols over the indexed estate), **a
witnessed verdict channel** (`review_verdict` ledger events, proven in production on PR #3), and **a
proven actuation loop** (worktree → PR → required checks → GitHub-native auto-merge — five merges
landed this way on 2026-06-12 alone).

Empirical boundary (2026-06-12): the permission classifier **denied** a 52-repo sweep that included
third-party forks and RuVector/ruflo, and **approved** the same operation narrowed to 21
FlexNetOS-owned repos. That denial/approval pair is treated as ground truth for where autonomous
mutation ends.

## Decision

1. **The steward is a standing role**, not a new service: any session (or a dedicated
   `.claude/agents/steward.md` agent) may assume it by following the protocol below. The role's
   contract lives in this ADR + NORTH-STAR.md.
2. **Protocol (every steward decision):**
   recall (ICM + memoir) → reason against NORTH-STAR.md laws + rubric → decide →
   **record a witnessed `review_verdict`** in the fleet ledger (`hf review verdict <id> <pr>
   <approve|deny> --by steward`) *before* acting → act via the proven loop → re-segment
   NEEDS-HUMAN.md (decided items move out; new walls move in, each with exact commands).
3. **Rubric (from NORTH-STAR.md):** steward decides reversible repo-scoped changes, registration/
   tagging, docs/capsule/card hygiene, CI/dependency upkeep on org-owned repos, reversible archiving
   of zero-dependency stubs, sequencing. Steward escalates: physical actions, account/billing/auth,
   irreversible deletion, visibility changes, mass mutation across third-party forks, changes to
   NORTH-STAR itself, and anything a classifier denies twice (one denial → narrow and retry once;
   two → escalate verbatim with the attempted command).
4. **Merge authority** remains the AI-gatekeeper-as-required-status-check (HFTASK-0010); the steward
   never bot-approves and never merges red. Cross-peer decisions additionally ride a weave
   permission-ask (ADR-0002 surface 5).
5. **Interim fleet-capsule registry:** repos that cannot yet host an in-repo `.handoff` (third-party
   forks pending the owner's in-repo-stub decision, and RuVector/ruflo by law) get their
   census-derived capsule at **`handoff/.handoff/fleet/<repo>/capsule.json`** — committed text in the
   org-owned kernel repo, zero fork divergence, discoverable by `hf fleet status` exactly like
   in-repo capsules (ADR-0004 §4 reads both).

## First demonstrated decision (recorded)

**Question** (deferred by the classifier today): how do Tier C/D repos get their continuity layer?
**Steward verdict — split (approve, `--by steward`):**
- **Org-owned Tier D hubs** (template_hub, tool_hub, plugin_hub, mcp_hub, vault_hub, harness_hub,
  commands, hooks_hub, database_hub, flow_hub, network_hub): in-repo stubs **proceed** — they are
  FlexNetOS originals, the same class as the approved 21-repo batch; reversible one-commit PRs
  through the normal loop. (.github_org excluded: its protection requires a human review — wall.)
- **Third-party forks + RuVector/ruflo**: **central capsules now** (rule 5), in-repo stubs
  **escalated to the owner** — the vision says "every repo hosts `.handoff`", the classifier and the
  never-casually-modify law counsel restraint on forks; the owner arbitrates with full context
  (NEEDS-HUMAN entry carries both options and the exact driver command).
- Rationale anchors: NORTH-STAR laws 2 (extend, don't pollute), 7 (fork discipline), 8 (blast-radius
  research); the classifier's empirical line; ADR-0004 §2 tier table.

## Consequences

- NEEDS-HUMAN.md becomes two sections: **steward-owned** (decided autonomously, witnessed) and
  **genuine walls** (physical/auth/irreversible/owner-intent). The starvation problem dissolves for
  the first class.
- Every steward decision leaves a ledger event *before* action — auditable, replayable, hardware-
  anchorable once the Cognitum Seed's data port is connected (COGNITUM-SEED.md integration target 1).
- The role composes with HFTASK-0010 (separate-role reviewer) and 0014 (swarm reviewer): the steward
  decides *queue items*; the gatekeeper judges *code*; they are different gates that share the
  verdict channel.

## Research / Cross-References

Memoir 01KTQRS2HJF3GV5KRDJ1T14M8Q (the forgotten directive: envctl relay integration, never-downgrade,
code-omniscient gatekeeper); NORTH-STAR.md (laws + rubric, v1); NEEDS-HUMAN.md starvation evidence
(VERIFICATION-REPORT.md delta 4: 0/7 actioned); classifier denial/approval pair 2026-06-12 (52-repo
sweep denied → 21 org-owned approved; reason text preserved in session log); `hf review verdict`
(hf/src/main.rs cmd_review_verdict, shipped in PR#3's bootstrap); ADR-0002 §5 (out-of-band verdicts);
ADR-0004 §2/§4 (tier table, fleet aggregation reads); gh-aw #25439 (bot-approve bypass — why the
steward never approves natively); ICM/memoir as recall substrate (icm_memoir system-architecture,
~110 concepts); gitkb code-intel tools (kb_callers/kb_impact — the "full code knowledge" faculty).
