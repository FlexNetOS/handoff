---
id: 019eedcc-3045-7213-b143-2c07520808e2
slug: context/overridable/active
title: "Active Context"
type: context
status: draft
priority: medium
---

## Current focus
HFTASK-0072 (ADR-0018 D7): full adoption of the FlexNetOS agent guide (`meta/.kb/AGENTS.md`) in handoff — init the full `.kb`, wire create-first + board + traceability into the loop, and bind the planning↔execution seam (ADR-0003) both ways.

## Recent changes
- Initialized handoff's own durable `.kb` (`git kb init`) with the seven context documents.
- Residency: commit `.kb/store/**` (durable text); gitignore `.kb/.cache/` (binary cache) + `.kb/workspaces/` (ephemeral) + `.kb/config.toml` (per-user) — the HFTASK-0067 text-vs-binary precedent.

## Immediate next steps
- Surface the seam verbs + create-first discipline in `.claude/rules/knowledge-management.md` + AGENTS.md.
- Verify `hf task mint --from-kb` (IN) and claim/checkpoint/done write-back (OUT) against the fresh `.kb`.
- NOTE: this doc is the human-authored kb plane; the AUTHORITATIVE live loop state is the `hf` ledger (`hf resume`/`hf status`). kb never overrides execution truth (ADR-0003).