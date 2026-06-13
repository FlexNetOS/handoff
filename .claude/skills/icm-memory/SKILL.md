---
name: icm-memory
description: "MANDATORY persistent memory for every harness agent — recall relevant memories BEFORE deciding, and store on the required triggers (errors resolved, architecture/design decisions, user preferences, significant completions, ~20 tool calls). ALWAYS use at session/cycle start (recall) and after any decision, fix, or completion (store). Do NOT skip — ICM is the cross-session memory the owner requires; chat context is lost, ICM is not."
---

# icm-memory — recall before you decide, store on every trigger

ICM (Infinite Context Memory) is the harness's cross-session memory and it is
**mandatory** (owner directive; global CLAUDE.md). Context windows reset and compact;
ICM does not. Every agent recalls relevant memory before acting and stores on the
defined triggers — *before* responding, not later.

## RECALL — before starting any work

```bash
icm recall "query"                     # search memories
icm recall "query" -t "topic-name"     # filter by topic
icm recall-context "query" --limit 5   # formatted for prompt injection
```
Recall at: session/cycle start (orient), before a decision or verdict, before
researching (prior errors-resolved + decisions save re-work), before touching a system
you haven't this session. Search only what's relevant — do not dump everything.

## STORE — you MUST store when any trigger fires (before responding)

| Trigger | topic | importance |
|---------|-------|------------|
| Error resolved | `errors-resolved` | high |
| Architecture/design decision | `decisions-handoff` (or `decisions-{project}`) | high |
| User preference / correction | `preferences` | critical |
| Significant task completed (feature/fix/PR) | `context-handoff` | high |
| ~20 tool calls without a store | progress summary → `context-handoff` | high |

```bash
icm store -t errors-resolved   -c "<what broke + the fix>" -i high   -k "kw1,kw2"
icm store -t decisions-handoff -c "<decision + why>"       -i high   -k "kw1,kw2"
icm store -t preferences       -c "<the preference>"       -i critical -k "kw1,kw2"
```
Do NOT store: trivia, anything already in CLAUDE.md, ephemeral state (build logs, git
status). Prefer the MCP tools (`mcp__icm__icm_memory_recall` / `..._store`) when
available; fall back to the `icm` CLI.

## Per-agent touchpoints (harness wiring)

| Agent | Recall | Store |
|-------|--------|-------|
| continuity-navigator | prior decisions/drift on orient | — |
| kernel-researcher | errors-resolved + decisions before researching | new findings worth keeping |
| code-omniscient-gatekeeper | prior verdicts/decisions on the surface | the verdict + rationale (decisions) |
| systems-orchestrator | cross-system context before sequencing | cross-system decisions |
| doc-updater | — | significant completion summary at cycle close |
| (handoff-loop) | Phase 2 recall | Phase 5 store (completion/progress) |

## Why mandatory

A false "I don't remember that decision" re-litigates settled architecture, repeats a
resolved error, or violates a known preference. Recall is cheap; re-work is not. The
ledger records *what happened*; ICM records *what we learned and decided* — keep both.
