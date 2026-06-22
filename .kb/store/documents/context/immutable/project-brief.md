---
id: 019eedcb-75db-7b90-845b-12a05a77e972
slug: context/immutable/project-brief
title: "Project Brief — Continuity Ledger Kernel"
type: brief
status: draft
priority: medium
---

## Vision
handoff is the **Continuity Ledger Kernel** — a local-first, auditable, reversible, model-native agentic OS where the repo (the witnessed ledger) is the source of truth, never chat history. First command in any session: `hf resume`.

## Core purpose
Advance the kernel one **witnessed** task per cycle: reconcile drift → research → implement → verify → autonomous code-omniscient gate → ship → handoff. Packets are RENDERED from the witnessed ledger, never hand-written (ADR-0006).

## Foundational law
**Integrity · Reversibility · Capability Gain — no promotion without all three.** Every action must increase verified capability without corrupting the protected Gold World baseline.

## Key constraints
- Fail-closed: a guard that cannot confirm its precondition must STOP, never proceed (LESSONS L7–L10).
- No-C trust boundary (ADR-0001): the continuity substrate links no bundled C (ledger ported to pure-Rust redb, ADR-0017).
- Scope law: edits stay within a claimed task's path_scope + intent_lock; scope expansion escalates, never silently widens.
- NO HUMAN IN THE LOOP fleet vision: user directs; the system builds/operates; NEEDS-HUMAN is a scaffold for a model with the human's skillset.