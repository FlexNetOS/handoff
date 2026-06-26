# ADR-0019 — P2 reconciliation: spec-vs-code honesty + gatekeeper merge policy

- **Status:** Accepted
- **Date:** 2026-06-26
- **Related:** code-research report `.handoff/loop/reports/hf-kernel-architecture-capability-map.md` (findings D6, D5-C23); ADR-0018 (full-auto agentic operation, D8 gatekeeper-as-required-check); PRD §7.2/§9/§12.3
- **Resolves the P2 tier of the code-research findings** (P0 = `verify_witness_chain` hardening #140; P1 = unknown-verb fail-closed #141 + cognitum wiring #142; drift §12.3 content-match #143; `hf index`/`hf plan` build #144).

## Context

The deep code-research run confirmed the kernel's core guarantees are sound but surfaced a tier of
**honesty gaps** — places where the docs/spec claimed more than the code delivers, plus one
deliberate config the research flagged as a weakness. These are not bugs in the running kernel; they
are documentation/decision-record debt. Left unrecorded, they read as overclaims.

## Decisions

### D1 — Architecture spec is aspirational; record the shipped reality (PRD §7.2)
The PRD's 12-crate `handoff-*` workspace / resolver-3 / edition-2024 / `[workspace.lints]` block is a
*target*, not the shipped state. The kernel ships **3** crates (`work-order`, `ledger`, `hf`),
resolver 2, edition 2021, and **no** enforced workspace lints. A "Current implementation status"
callout was added to PRD §7.2 so the spec is not read as a claim of present fact. The 12-crate
decomposition, the 2024 edition move, and lint adoption remain **tracked future work** (see D4).

### D2 — `crates/{cli,core,runner,spec,tui}` are a separate co-located toolkit
These are the independent "Intent Driven Development / rusty-idd" toolkit (`crates/core/src/lib.rs`
self-describes it), **not** the PRD's `handoff-*` crates. They compile in the workspace but are
**never invoked** by `hf`/`ledger`/`work-order` (zero references — verified). Recorded so the
orphaned members are not mistaken for kernel crates.

### D3 — `hf index` / `hf plan` are now built (closes the false-Done)
HFTASK-0050 was Done before either verb existed. PR #144 implements both for real
(`.handoff/maps/*`, the task DAG), making the PRD §8/§9 verb-table rows and the card's Done
genuinely true. No further reconciliation needed for these verbs.

### D4 — Gatekeeper merge policy: `enforce_admins = false` is INTENTIONAL
The research (D5-C23) noted the AI Gatekeeper *is* a required status check on `develop` but is
**admin-bypassable** (`enforce_admins = false` + the documented `gh pr merge --admin --squash`
flow). This is a **deliberate** choice, not an oversight: the 60-repo org shares one GitHub Actions
runner cap, so required checks routinely sit `queued` for long periods. The standing flow is
*local-verify the exact CI gates, then admin-squash to bypass the stuck queue* (see
`handoff/CLAUDE.md` "Standing fast merge flow"). Flipping `enforce_admins = true` would deadlock the
pipeline under runner contention — a regression. The gatekeeper stays **advisory-required** (present,
bypassable) until the runner-cap constraint changes.

### D5 — Deferred future work (tracked here, not silently dropped)
The following were acknowledged gaps at P2 time. **All have since been DELIVERED** (2026-06-26
burndown; HFTASK-0079–0083), so this section now records *done* rather than *deferred*:
1. **Drift §12.3 #8** (work contradicts a decision record): **DONE** (#151) — `hf/src/gates.rs`
   now parses `drift-guard` markers in ADRs/decision records and surfaces real
   `decision_contradictions` (e.g. a forbidden `bincode =` reintroduced in a manifest).
2. **Gatekeeper AST grounding**: **DONE** (#152) — `impact_scan` now unions the code-intelligence
   call graph (`git kb code impact`) with the grep safety-net and records `impact_grounding`.
3. **`[workspace.lints]` adoption** (`unwrap_used`/`expect_used`/`panic` = deny): **DONE**
   (HFTASK-0080 #159 kernel + HFTASK-0082 #164 toolkit) — the whole tree enforces the deny;
   production sites propagate or carry a justified per-site `#[allow]`; tests allowed under
   `#[cfg(test)]`.
4. **12-crate decomposition + edition 2024 / resolver 3**: **DONE** — edition 2024 + resolver 3
   landed (#153); the decomposition reached **16 kernel crates** (HFTASK-0081 #154/#160/#161/#162
   leaf set; HFTASK-0083 #165–#173 the coupled set). The hf binary is now a thin orchestrator over
   `handoff-{core,policy,schema,lease,hooks,index,fleet,drift,route,test-support,secrets,gatekeeper,intake}`
   + `work-order` + `ledger`. The 4 card-named coupled modules (drift/gatekeeper/fleet/intake) are
   all peeled — gatekeeper's shared `GhPrView`/secrets-gate were extracted (`handoff-secrets`,
   optional-feature), and intake's `cmd_claim_with` dispatch coupling was **inverted** via an
   injected `claim` closure rather than lifted, so no crate reaches back into the binary.

## Consequences

- The PRD and the code no longer disagree silently: every overclaim is either built (D3), corrected
  to record reality (D1/D2), or recorded as an explicit deferred decision (D4/D5).
- No code behavior changes from this ADR (it is a decision/doc record); the substantive P2 code
  landed in #143 (drift) and #144 (index/plan).
- The deferred items (D5) are the natural next backlog if/when the owner prioritizes them.
