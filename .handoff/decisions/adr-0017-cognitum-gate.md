# ADR-0017: Adopt cognitum-gate-tilezero as the witnessed `hf policy` action governor

## Status
Accepted — implemented in HFTASK-0017.

## Context
HFTASK-0015 built the initial `hf policy check-{claim,edit,handoff}` gates as a flat
`rules.toml` denylist. ADR-0001 R13 and the runbook map the in-loop action governor to
RuVector's `cognitum-gate-tilezero`, which produces a signed `Permit`/`Defer`/`Deny`
verdict and a hash-chained `WitnessReceipt`. The envctl broker (HFTASK-0013/R10) remains
a separate credential/egress gate; the two compose.

## Decision
1. Make the `cognitum` feature default in `hf/Cargo.toml` so the action governor is
   compiled into every `hf` binary.
2. Keep a `--no-default-features` CI job to ensure the fallback path (command refuses
   with a clear message) stays buildable and tested.
3. Expose the gate through `hf policy gate <action> [--task <id>]`, which evaluates the
   action, appends a `cognitum_decision` event to the ledger, and exits:
   - `0` on `permit`
   - `1` on `defer` (human review required) or `deny`
   - `2` on usage/feature errors

## Consequences
- The loop can now ask for a witnessed permit before performing an action.
- Default build times increase because `cognitum-gate-tilezero`, `tokio`, and their
  crypto dependencies are now part of the default dependency set.
- The no-default-features path is preserved for constrained builds that do not need
  the gate.
