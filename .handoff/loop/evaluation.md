# Run evaluation — 2026-06-21 seeded-card hardening (per-run scratch; LESSONS.md is durable)

Scope: hardening the completion-evidence gate for the three seeded durability/concurrency cards.
Shipped & witnessed: PR #102 (`1b0503c`, scoped test_commands), PR #103 (`6b91ed1`, `hf test` fails
closed on zero tests). Surfaced-not-fixed: orphaned `.handoff/ledger.db.rvf.lock` liveness wedge.

## Friction
- **Root cause that delayed detection (prior session):** a hand-committed card (#95) missing the
  required `intent_lock` was SILENTLY DROPPED by `load_tasks` (fail-open `if let Ok`), so it never
  showed in `hf status` — the loop reasoned over an incomplete backlog and didn't notice for a whole
  session. Largest friction event; root cause is the fail-open class.
- **Medium:** the test gate's exit-0 rubber-stamp meant a zero-match filter could stamp PASS — only
  caught when test_commands were hand-audited (PR #102 → #103).
- **Liveness:** the orphaned RVF lock wedges every `hf` invocation until a manual `rm` (safe/
  fail-closed, but a hard liveness stop with no automated recovery).

## Gate quality
- **Caught real defects:** PR #103 made `hf test` require positive evidence (`parse_tests_ran`:
  Some(0)=FAIL, None=degrade-with-note) — a genuine strengthening of the completion-evidence gate;
  5 unit tests + live positive/negative proof.
- **Slipped past (the lesson):** the completion-evidence gate previously accepted ANY exit-0 command
  — absence-as-pass. The card-load path slipped a malformed card past `hf status` entirely. Both are
  the same FAIL-OPEN class.
- **No false-blocks** of valid work observed; PR #102 kept locks hash-stable (test_commands ∉
  intent_lock), so no drift introduced.

## Coverage
- The three cards were correctly scoped (3/19/6 targeted tests) rather than the blanket workspace
  run. No items silently capped. The fail-open audit found additional candidate sites
  (`current_statuses` `unwrap_or_default`, `load_task_in().ok()?`, the `hf test` None-degrade
  surfacing, the verifier "green is one line" blind spot) — enumerated in `_workspace/10_evolution.md`.

## Human walls
- One liveness wall: the orphaned RVF lock required a manual `rm` — avoidable via a provably-dead
  reclaim (5th target). The fail-closed refusal to steal the lock was correct; the absence of an
  automated dead-holder reclaim is the gap.
- No genuine owner walls hit this run.

## Verdict
Strong, narrow, well-evidenced hardening run. The systemic teaching is a single class — **FAIL-OPEN**
— with ≥3 distinct instances in one run, so it escalates immediately (Phase 7-4). Upgrades routed in
`_workspace/10_evolution.md` and `_workspace/proposed-upgrades.md`: U1–U5 (auto-PR doc/skill/script
+ AGENTS.md fail-closed law) and two escalated structural items (loud `load_tasks`; the 5th-target
`hf doctor` invariant sweep + stale-lock self-heal). No guard weakened; the "raise the retry cap"
band-aid explicitly refused.
