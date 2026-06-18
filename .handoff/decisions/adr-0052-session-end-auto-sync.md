# ADR-0052: Session-end auto-sync and `pr_merged` ledger marker

## Status
Accepted — implemented in HFTASK-0052.

## Context
The fleet continuity model (ADR-0004 §3.3 rev) keeps a per-repo `ledger.db` as the
witnessed source of record and rolls those events into a central FLEET ledger at
`meta/.handoff/ledger.db`. The rollup is implemented by `hf sync` Part C
(HFTASK-0032), but until now it only ran when an agent manually invoked `hf sync`.
As a result, member-repo ledgers were not reliably reflected in the central ledger.

Separately, `hf done --pr N` records a `pr_merged` event, but agents often run
`hf done <id>` immediately after GitHub auto-merges a PR and forget the `--pr`
flag. The merged marker is useful for delivery round-trips (HFTASK-0021) and for
reading the packet history, so missing it creates an incomplete audit trail.

## Decision
1. Wire `hf sync --auto` into the `SessionStop` lifecycle hook so every session
   end rolls per-repo ledgers into the central FLEET ledger automatically.
2. Make `hf done` automatically derive the merged PR from the most recent
   `pr_opened` event recorded by `hf ship`, falling back to an explicit `--pr N`
   override when provided.

## Consequences
- `meta/.handoff/ledger.db` stays up-to-date without requiring a separate manual
  `hf sync` invocation.
- Every shipped task receives a `pr_merged` ledger marker, closing the audit gap.
- These changes touch `.handoff/hooks/hooks.toml` and
  `.handoff/hooks/session-end.sh`, which are protected files under
  `merge.protected_files`. They are intentionally limited to hook wiring and are
  covered by this ADR.
- The `promote-verify.yml` CI checkout is adjusted to mirror `ci.yml`: checkout
  into a `handoff/` path and clone `RuVector`/`envctl` as sibling checkouts. This
  fixes the master-target PR gate, which previously failed to resolve the
  relative `../../RuVector/...` path dependency used by `ledger`.
