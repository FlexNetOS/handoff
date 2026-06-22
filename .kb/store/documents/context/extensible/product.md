---
id: 019eedcb-fb88-7a70-b8ca-542457c4ed22
slug: context/extensible/product
title: "Product Context"
type: context
status: draft
priority: medium
---

## Problem being solved
Agent context periodically reinitializes; chat history is lost. handoff makes the **repo** the durable memory so any session — human or model — resumes with zero archaeology via `hf resume`. It is the continuity substrate for a no-human-in-the-loop multi-provider autopilot.

## Users
- The autonomous kernel loop (the primary "user"): claims, builds, verifies, ships one witnessed task per cycle.
- Fleet repos: each gets a `.handoff` rolled out (ADR-0004 §3/§6, P7 conformance).
- The owner: directs at a high level; designated agents replace the human gate (ADR-0018).

## Product principles
- Witnessed over narrative — notes are STATE, not story.
- Reversible + auditable — every action is on the chain and undoable.
- Fail-closed by default — absence is a failure, never a silent pass.