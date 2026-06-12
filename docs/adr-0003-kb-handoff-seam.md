# ADR-0003 — the kb ↔ handoff seam (planning plane ↔ execution plane)

**Status:** accepted (2026-06-12) · **Owner:** handoff kernel · **Derived from:** ADR-0001 §8 (R7),
ARCHITECTURE-TRUTH.md census (2026-06-12), defect D3 (22 stale cards), the gitkb plugin contract.

## Context

Two task registries coexist in the meta estate and are **disjoint today**: git-kb documents
(`/kb-board`, `/kb-tasks`, `/kb-commit`, `/kb-status` — `.claude/commands/kb-*.md` over `git kb`) and
`.handoff/tasks/` cards (`handoff.task.v1`). The 2026-06-12 census found the kb board carrying 3 ACTIVE
tasks while `.handoff` carried 22 cards — zero cross-references between them, and all 22 cards stale at
`backlog` with dead `spike/**` path-scopes despite shipped work (D3). ADR-0001 R7 established the only
link: one-way, hf → kb context doc, never read back. Without a seam contract, every new loop tool
re-decides where "what's next" lives, and the two surfaces drift apart — the exact failure D3 shows.

## Decision — five rules

1. **Plane charter.** git-kb owns the **planning plane**: what/why/when-next, human+agent readable
   (context docs, task documents, incidents, specs; `/kb-board` is *the* planning board). `.handoff`
   owns the **execution plane**: claims, leases, witnessed evidence, resume packets. State precedence
   within execution is unchanged: **Git > ledger > cards** (decision-log-2026-06-09).
2. **Minting rule.** Fleet-level execution work starts by minting a `handoff.task.v1` card **from** a kb
   task: `hf task mint --from-kb <slug>` copies objective/criteria, stamps `kb_ref: <slug>` and
   `correlation_id: <slug>`, computes the IntentLock. Kernel-internal work may mint without `kb_ref`,
   but anything another agent could pick up plans in kb first (the AGENTS.md "document before
   implementing" rule, now mechanized).
3. **Write-back rule.** `hf checkpoint` and `hf handoff` append a progress line to the referenced kb
   task document (ledger → kb, the R7 direction made automatic); `hf claim` flips the kb task to
   `active`; terminal `done` flips it to `completed` **with evidence** (commit hashes, test results).
   kb is still never read back into the witnessed ledger — the planning plane informs, never overrides,
   execution truth.
4. **Single-registry rule.** The kb board owns "what's next." `.handoff/tasks/` cards are **derived
   execution snapshots** of minted work, refreshed deterministically from ledger truth at checkpoint
   (`hf checkpoint --sync-cards`). This is the beads dual-store lesson (SQLite ↔ JSONL with
   deterministic export): a derived text view in git, never a second source of truth — and it fixes the
   D3 drift class permanently.
5. **Binding convention.** Commit messages carry both `[[tasks/<kb-slug>]]` (kb wikilink, already
   mandated by knowledge-management rules) and the card id; weave jobs carry `correlation_id`. One
   identifier chain: kb slug → card `correlation_id` → weave job → PR → merge commit.

## Consequences

- `/kb-board` becomes authoritative for planning across the fleet; `hf fleet status` (ADR-0004) joins
  it with execution truth instead of competing with it.
- The 22 stale kernel cards get refreshed by the first `--sync-cards` implementation pass and re-linked
  to kb tasks where they have planning relevance (HFTASK hygiene item, folded into ADR-0004 rollout).
- New verbs to implement: `hf task mint --from-kb`, `hf checkpoint --sync-cards`, kb write-back inside
  checkpoint/handoff (extends the R7 one-way push). Small, witnessed, no daemons.
- gitkb remains optional per repo: where a repo has no `.kb`, minting falls back to card-only with a
  warning (degrade-and-say-so, the ClaimGate convention).

## Research / Cross-References

ADR-0001 §8 + R7 (one-way kb push, "no upsert" finding); ARCHITECTURE-TRUTH.md "The KB ↔ handoff seam"
(census evidence of disjoint registries, D3); decision-log-2026-06-09 (state precedence);
knowledge-management rule file (wikilink mandate); beads — github.com/gastownhall/beads (git-backed
agent issue tracker: SQLite local index + JSONL git-visible state + deterministic export; validates
derived-view cards and git-as-transport); .claude/commands/kb-{board,tasks,commit,status}.md (the
planning-plane commands, verified present 2026-06-12); memoir: architecture-truth-census-2026-06-12,
adr-2026-06-11-open-questions (r2).
