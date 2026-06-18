# ADR-0014 — cognitum-gate as the witnessed hf policy decision engine

**Status:** accepted (2026-06-18) · **Owner:** handoff kernel · **Derived from:** HFTASK-0017, ADR-0001 R13, HFTASK-0015.

## Context

HFTASK-0015 wired a flat `rules.toml` denylist for protected-file and claim/handoff gates. ADR-0001 R13 calls for a richer, witnessed action governor: RuVector's `cognitum-gate-tilezero` (`~/Desktop/meta/RuVector/crates/cognitum-gate-tilezero`) provides `GateDecision::{Permit,Defer,Deny}`, signed `PermitToken`s, and a hash-chained `WitnessReceipt` log. This is the coherence-gate arbiter, distinct from the envctl secrets-engine broker (HFTASK-0013) which governs credentials/egress. The two gates compose: cognitum answers "may this action be attempted?"; envctl broker answers "may this credential/merge egress proceed?".

## Decision

1. Add an optional `cognitum` feature to the `hf` crate that depends on `cognitum-gate-tilezero` and `tokio` (for the async gate runtime).
2. Add `hf/src/cognitum.rs` implementing:
   - `evaluate_action(action_id, action_type, agent_id, path) -> DecisionRecord`
   - `cmd_policy_gate(action, task_id)` invoked as `hf policy gate <action> [--task <id>]`
   - A witnessed `cognitum_decision` ledger event (schema `handoff.cognitum_decision.v1`) containing the action, decision, token sequence, and base64-encoded signed token.
3. Wire the new verb into `hf/src/main.rs` behind `#[cfg(feature = "cognitum")]`.
4. Default build remains lean (`default-features = []`); CI and the local pre-push hook can build without the feature. A separate CI job may build/test with `--features cognitum` once the gate is fully integrated.
5. Keep the existing flat `rules.toml` denylist gates (`hf policy check-*`) intact; cognitum-gate is an additional, richer layer, not a replacement in this slice.

## Consequences

- `hf policy gate` is unavailable unless `hf` is built with `--features cognitum`.
- The gate returns `permit`, `defer`, or `deny`; `defer` and `deny` exit nonzero so callers fail closed.
- Each verdict is signed and witnessed, giving an audit trail of what the loop was permitted to do.
- Future work can feed real worker-tile reports (structural/shift/evidence filters) into the gate rather than relying on default thresholds.
