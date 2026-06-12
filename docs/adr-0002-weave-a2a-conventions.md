# ADR-0002 — weave a2a conventions for loop tools

**Status:** accepted (2026-06-12) · **Owner:** handoff kernel · **Derived from:** the proven `hf` `Leaser`/`WeaveCli` integration (HFTASK-0002, hf/src/lease.rs) + seam-spec-weave-a2a-2026-06-11 + ship-loop-proof-2026-06-12.

## Context

Every loop tool (hf today; prompt_hub dispatch, mission-control, delivery next) needs agent-to-agent coordination: mutual exclusion, durable work hand-over, messaging, approvals. weave is the sanctioned mesh substrate (SQLite broker, owner-only writes). Two integration styles exist: CLI subprocess and MCP stdio (weave-mcp, 50+ tools). The hf lease integration proved the subprocess seam; this ADR freezes the conventions so each new tool doesn't re-derive them.

## Decision — the five-surface contract

1. **Identity**: resolve as flag > `$WEAVE_SESSION` > `basename(cwd)`. Never hardcode peer names. (`weave whoami` to confirm.)
2. **Mutual exclusion — leases** (WL-024/029): reserve `<domain>:<verb>:<id>` (slash-free ⇒ exact-match, e.g. `handoff:claim:HFTASK-0009`) **before** recording local state; TTL ≤ 1h with re-reserve heartbeat; release on done/abort. Cross-peer conflict ⇒ **refuse**; same-holder ⇒ TTL extend; weave absent/legacy ⇒ **degrade to local-only and say so** (`ClaimGate::{Refuse, Proceed, ProceedDegraded}` is the canonical gate type).
3. **Durable work — jobs** (P3, poll-only): `job_create` (the `correlation_id`/prompt carries the `handoff.task.v1` id) → `job_claim` (capture `attempt_id`; ALL updates are fenced by it) → `job_update` progress notes → terminal `job_update {state, result}`. weave Jobs are the **coordination view**, never source of truth — the hf witnessed ledger is operational truth (state precedence: Git > ledger > cards).
4. **Messaging**: `send`/`inbox`/`thread` for peer chatter; broadcasts are never injected. Use for FYI, not for state.
5. **Approvals — out-of-band verdicts** (WL-021 + R6): verdicts ride a weave **permission ask** answer body (`approve`/`deny`) **plus** a `review_verdict` event in the consumer's own ledger. Never a native GitHub APPROVE (bot-approval bypasses protection, gh-aw #25439). The consumer (hf) enforces; weave records.

## Transport rule

CLI subprocess is the sanctioned pattern for Rust loop tools: `Command::new(weave_bin)` with **explicit argv, no shell**, parse stdout, treat non-zero as degrade-or-refuse per surface (the `Leaser` trait + `WeaveCli::from_env()` template). Resolve the binary as `$HF_WEAVE_BIN`-style env override > `weave` on PATH, and **require a lease-capable build** (`weave lease --help` probe; the ~/.cargo/bin binary predates WL-024 — stale-binary refusal is part of the preflight). MCP stdio (weave-mcp) is the seam for agent harnesses (Claude/codex sessions), not for compiled loop tools.

## Consequences

- New loop verbs (ship, review, intake, session) reuse `WeaveCli` instead of fresh plumbing; new tools copy the `Leaser` trait shape (mockable, unit-tested gate fn).
- The weave workspace is INTERIM (ship-loop-proof supersession): work within the existing 4 crates; do not grow new weave crates for these conventions.
- Bridging weave(mesh) ↔ rvAgent(A2A) remains an explicit open design item (open-questions #3); hf stays the junction until decided.

## Research / Cross-References

hf/src/lease.rs (Leaser, WeaveCli, gate(), claim_resource()); weave-core/src/model.rs:951 (Job/attempt_id), :1199 (Lease), :1114 (ReviewItem — NO verdict field, verified); memoir: seam-spec-weave-a2a-2026-06-11, ship-loop-proof-2026-06-12 (weave PR #61 = protected fail-closed merge proof), decision-log-2026-06-09 (state precedence); ADR-0001 §5/§5a/§7; gh-aw #25439 (bot-approval bypass).
