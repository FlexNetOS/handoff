---
id: 019eedcb-c122-78b3-95ff-9f8d15cbdaf3
slug: context/immutable/architecture
title: "Architecture"
type: architecture
status: draft
priority: medium
---

## Workspace
A Rust workspace: `hf` (CLI), `ledger` (pure-Rust redb event store + RVF v2 overlay), `work-order` (the `handoff.task.v1` schema).

## Data flow
```
prompt_hub SwarmBundle ──hf intake──▶ handoff.task.v1 cards
        .kb task ──hf task mint --from-kb──▶ KBTASK card (correlation_id = slug)
   hf claim/checkpoint/done ──ledger append──▶ witnessed event chain ──render──▶ packet/active.md
   hf claim/checkpoint/done ──kb::write_back──▶ .kb task status (ONE-WAY, ADR-0003)
   hf handoff ──ruvector-verified proof──▶ AgentContract attestation (fail-closed)
   hf ship ──develop PR──▶ hf promote (develop→trunk ff)
```

## Integration points
- weave (A2A leases), grit (AST-symbol locks + worktrees, ADR-0009/0010), envctl (secret relay), cognitum-gate (witnessed action policy), git-kb (planning plane + code intelligence).

## .kb seam module
`hf/src/kb.rs` — `kb_root` resolves the `.kb` dir; `cmd_mint_from_kb` (IN); `write_back`/`KbTransition` (OUT). Both degrade fail-soft when `.kb`/`git-kb` are absent.