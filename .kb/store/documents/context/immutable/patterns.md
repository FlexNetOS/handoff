---
id: 019eedcb-c10f-7fc0-a5e0-a155c2a7e40e
slug: context/immutable/patterns
title: "System Patterns"
type: patterns
status: draft
priority: medium
---

## Design patterns
- **Witnessed ledger** — every state transition is an append-only, hash-chained event; rendered views (packets/active.md/cards) are DERIVED, never authoritative. Reconcile by precedence + re-render (never hand-edit).
- **Two-plane seam (ADR-0003)** — planning plane (git-kb `.kb`) feeds execution plane (`.handoff` ledger) ONE-WAY: kb minting IN (`hf task mint --from-kb`) and execution write-back OUT (claim→active, checkpoint→progress, done→completed). The kb is never read back as execution truth.
- **Two-ledger residency (ADR-0004 §3)** — FLEET ledger at `meta/.handoff`, KERNEL ledger at `meta/handoff/.handoff`; per-repo gitignored `ledger.db` is a legitimate local cache that rolls up.
- **Text-vs-binary durability (ADR-0016 / D1 / D7)** — commit the durable TEXT (ledger.events.jsonl, `.kb/store/**`); gitignore the binary rebuild cache (`ledger.db`, `.kb/.cache/gitkb.db*`).
- **Fail-closed guards** — proof-gated handoff (ruvector-verified AgentContract), drift audit gate, schema-validated card load.

## Implementation patterns
- Pure, unit-testable helpers split from I/O (e.g. `writeback_args`, `work_order_from_kb_doc`, `mint_target`).
- Additive/idempotent seeding (`hf seed` only writes MISSING cards; never clobbers live status).
- intent_lock (blake3 of objective/path_scope/acceptance + constraint/northstar surfaces) is the tamper-evident contract.