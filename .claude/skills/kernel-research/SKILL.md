---
name: kernel-research
description: "The mandatory deep web + codebase research + cross-reference protocol for any kernel decision or ADR. ALWAYS use before architecture changes, dependency additions, contract changes, or writing/updating an ADR — produces a cited dossier and an ADR-ready Research section. Do NOT skip for 'obvious' decisions; shallow analysis is a process violation here."
---

# kernel-research — no decision without grounded, cross-referenced research

The project's standing process rule: **every decision and every ADR must rest on
deep web + codebase research with cross-referencing — no detail overlooked.** An
ADR without a Research / Cross-References section is incomplete and the gatekeeper
will deny it. This skill is how that research is done and recorded.

## Three legs (do all three)

### 1. Codebase research (AST, not grep)
Use code intelligence over the whole repo and its dependencies, not text search:
- `git-kb code symbols --json` / `kb_symbols` — find definitions + signatures.
- `git-kb code callers <sym> --json` / `kb_callers` — who calls it (blast radius).
- `git-kb code callees <sym> --json` / `kb_callees` — what it calls.
- `git-kb code impact <path> --json` / `kb_impact` — transitive dependents.
- `git-kb code dead --json` / `kb_dead_code` — zero-caller symbols.
- Index first if empty: `git-kb code index <dir>`.
Grep is only for config files, string literals, and error messages.

The kernel spans `hf` + `ledger` + `work-order` and depends on
`../../RuVector/crates/rvf/rvf-crypto` (witness chain) — research across that
boundary, not just the local crate.

### 2. Web research (primary sources, current year)
For any external basis — Rust/crate facts, protocol/MCP specs, prior art, security
guidance — fan out searches and **fetch the primary source**. Quote load-bearing
claims with their URL; do not rely on memory for version/pricing/API facts.

### 3. Cross-reference (the part that's usually skipped)
Reconcile the three views and treat every mismatch as a finding:
- **code vs docs/ADRs/cards** — does the code actually do what the card claims?
  (The spike note in HFTASK-0003 — "work_orders_from_bundle is test-only + uses a
  MIRRORED SwarmBundle" — is exactly this kind of cross-ref finding.)
- **live repo vs fuller original** — cross-check against `~/Downloads/tmp/handoff`
  or `/tmp/handoff` for a richer variant. Principle: **never downgrade, always
  upgrade and automate.** Flag any place the live repo looks like a lite version.
- **stated intent vs implementation** — e.g. secrets must flow via envctl injection
  (`envctl run -- <tool>`, `crates/secrets-engine/src/inject.rs`, kasetto
  `agent-env.toml sync --locked`), never raw `export` — research the seam, don't
  assume the doc is current.

## Output: the dossier

Write `_workspace/02_research_<TASKID>.md`:

```markdown
# Research dossier — <TASKID>
## Findings (cited)
- [code] <fact> — <symbol/file:line, via kb_callers/impact>
- [web]  <fact> — <URL>
- [inferred] <claim> — LABELED as inference, not fact
## Cross-references / mismatches
- <code vs doc/card/intent mismatch> → finding
## Blast radius
- callers: …  | impact: …  | risk: low/med/high
## Recommended approach (with trade-offs)
## ADR Research section (only if the task carries an architecture change)
```

## ADR Research-section format (when the task needs an ADR)

Every ADR this project produces must include:
- **Research** — the cited web + codebase findings above.
- **Cross-References** — the reconciliations and mismatches, with links to related
  ADRs/tasks/code symbols.
Architecture changes require an ADR (hard rule) — never make one inline; route the
dossier to the ADR.

## Confidence discipline

Separate **verified** (cited) from **inferred** (labeled). State a confidence level;
when below 100%, list exactly what additional research would raise it. The
gatekeeper denies decisions built on unlabeled inference or stale facts.
