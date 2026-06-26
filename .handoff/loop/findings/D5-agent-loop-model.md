# D5 — Agent-loop model

**Dimension question:** Is there a real autonomous agent loop — session worktree isolation +
weave lease, `next_safe` topological routing, RuVector bandit value-routing, cognitum action
governance, the typed hook contract, the kb planning↔execution seam — and is "no human in the
loop" actually *enforced* (witnessed gatekeeper verdicts replacing human approval)?

**Verdict (1 line):** The loop is real and mechanically enforced *inside the kernel* (fail-closed
gates wired through a typed 14-event hook contract, witnessed to an append-only ledger), but
"no human in the loop" is enforced **locally**, not at the GitHub merge boundary — the cognitum
action governor and the AI gatekeeper exist and are witnessed but are **not wired into the loop's
firing contract**, and the merge gate degrades to "rely on branch protection" / admin-bypass.

Target root for all citations: `/home/drdave/Desktop/meta/handoff` (read via the
`.worktrees/code-research/handoff` checkout; paths are repo-relative).

---

## CLAIMS

### Session worktree isolation + weave lease

**C1 — A "session" is a real isolation unit: fresh worktree off `origin/<base>` + a weave
path-scope lease + witnessed `session_start`/`session_end` events.** `session_start`
(`hf/src/session.rs:236-306`) loads policy, runs a drift preflight, reserves a lease via
`leaser.reserve(&resource, SESSION_TTL_SECS, …)` (`session.rs:271`), creates the worktree
(`session.rs:284`), and witnesses `session_start` (`session.rs:299`). The lease resource is
slash-free so weave's path-hierarchy conflict detection reduces to exact-match
(`session_resource`, `session.rs:25-27`). TTL = 8h, heartbeat-extended (`SESSION_TTL_SECS`,
`session.rs:21`). **Confidence: high.**

**C2 — The session refuses to start on a drifted tree (the "weave-loop failure" lesson),
fail-closed.** `preflight_decide` (`session.rs:79-99`) is a pure decision over git facts: a dirty
working tree (`require_clean_tree`) or a base behind/diverged from origin (`require_synced_base`)
returns `Refuse`, which `session_start` honors by witnessing `preflight_refuse` and returning
without taking a worktree (`session.rs:251-265`). The IO (git status/fetch/rev-parse) is done in
the caller and passed in, so the policy is unit-tested (`session.rs:643-670`). **Confidence: high.**

**C3 — Worktree creation reuses the meta engine but degrades to plain `git worktree` so handoff
stays independently-cloneable.** `create_worktree` (`session.rs:161-202`) calls
`meta git worktree create --repo handoff …` when `meta` is on PATH and a `.meta.yaml` parent
exists (`meta_root`, `session.rs:117-124`), else falls back to `git worktree add` in a sibling
`.handoff-wt-<branch>` dir. `meta_git_lib` is deliberately **not** a crate dependency (module doc
`session.rs:6-9`). **Confidence: high.**

**C4 — Each session worktree is grit-enabled for AST-symbol-level parallel coordination, best-
effort.** `grit_enable` (`session.rs:149-153`) runs `grit init` in a fresh worktree; it
deliberately avoids `grit session start` (broken in grit 0.3.0 per the comment) and never walls
session creation on grit availability. **Confidence: high.**

**C5 — Worktree reaping is fail-closed: a worktree is reaped ONLY on a witnessed verified merge
(`pr_merged`/`trunk_promoted`) or an explicit `--reap`/`--force` override; an unconfirmable merge
KEEPS the worktree.** `reap_decide` (`session.rs:64-75`) returns `Reap` only on `force ||
merge_verified`, else `Keep`. `batch_merge_verified` (`session.rs:405-420`) scans from the last
`session_start` for the branch and returns true iff a merge event appears *after* it (a merge
before the session belongs to a prior batch). The IO replay returns an empty vec on any ledger
read failure → `false` → KEEP (`replay_event_branches`, `session.rs:496-509`). 8 unit tests pin
the invariant, including `reap_decide_keeps_when_unmerged` (`session.rs:736-742`). **Confidence:
high.**

**C6 — Reaping is triggered from three witnessed paths that share one teardown.** `hf done --pr`
calls `reap_open_session_if_merged` (`session.rs:614-637`, wired at `main.rs:1373`), `hf session
reap` sweeps retained worktrees (`cmd_session_reap`, `session.rs:568-608`), and `hf session end`
decides at close (`session_end`, `session.rs:308-362`). All converge on `remove_worktree`
(`session.rs:537-543`) and witness `worktree_reaped`. **Confidence: high.**

### Loop cadence / cycle counter

**C7 — The loop's cycle counter is a pure ledger reducer, not wall state.**
`session_state_from_events` (`session.rs:375-396`): a `session_start` opens a session + zeroes the
cycle, each `checkpoint` while open increments it, the matching `session_end` closes it. This is
the counter that drives `hf ship` at `cycle_flush` (doc `session.rs:364-365`). Tested
(`session.rs:688-726`). **Confidence: high.** *(Note: ADR-0018 D3 proposes replacing the fixed
`cycle_flush` with a ~50%-context-budget wrap; that is a spec decision, not yet observed wired in
this module.)*

### Task routing — topological `next_safe` vs RuVector bandit

**C8 — The authoritative "next task" pointer is topological, NOT the bandit.** `next_safe`
(`main.rs:319-334`): (1) resume any already-in-progress task (Claimed/Checkpointed/Active/Review),
else (2) the first Backlog task whose `dependencies` are all Done. This is what `cmd_handoff` uses
to render the "Next:" line (`main.rs:2597`) and what the policy gates check
(`gates.rs:480,518`). **Confidence: high.**

**C9 — The RuVector contextual Thompson bandit is real but scoped to `hf claim --batch` only — it
does not drive the `next_safe` pointer.** `route_with_history` (`routing.rs:81-110`) Thompson-
samples one shared Beta posterior per `ContextBucket` (priority-tier × role) and returns the
highest-sampled candidate. Its only caller is `cmd_claim_batch` (`main.rs:988`); the candidate set
is the *same* ready backlog (deps Done) `next_safe` step 2 uses (`main.rs:968-974`). So the bandit
chooses *among* topologically-safe tasks; it never overrides dependency safety. **Confidence:
high** (single call site confirmed by grep + read).

**C10 — The bandit genuinely learns from ledger outcomes (not just a static prior).**
`posterior_for` (`routing.rs:55-66`) seeds each bucket from a priority prior
(`prior_for`, P0=(8,1)…P3=(1,4), `routing.rs:36-44`) then Bayesian-updates from a `History` of
`(successes, failures)`; `routing_history(&tasks)` derives that from the ledger (`main.rs:902`,
wired at `main.rs:987`). The draw is seeded from the witnessed-event count so it is reproducible
for a ledger state and re-explores as history grows (`main.rs:981-984`). Tests prove a failing
context loses to a proven one (`routing.rs:244-265`) and monotonic priority value
(`routing.rs:156-163`). **Confidence: high.**

### Cognitum action governance

**C11 — A cognitum action governor exists as a default-feature verb that returns Permit/Defer/Deny
and witnesses the verdict, fail-closed on non-permit.** `cmd_policy_gate` (`cognitum.rs:102-164`)
evaluates an action through `cognitum-gate-tilezero` (`evaluate_action_impl`, `cognitum.rs:37-74`),
appends a `cognitum_decision` ledger event (`cognitum.rs:137`), prints permit and **exits 1 on
`defer` or `deny`** (`cognitum.rs:147-161`). The token is a base64 signed permit
(`DecisionRecord.token_b64`, `cognitum.rs:18-19`). Without the `cognitum` feature the verb errors
out (exit 2, `cognitum.rs:108-114`). **Confidence: high.**

**C12 — GAP: the cognitum gate is NOT wired into the loop's firing contract.** `hf policy gate` /
`cognitum` appears in **no** hook in `.handoff/hooks/hooks.toml` and in no `.handoff/hooks/*.sh`
loop script (grep over both returned zero matches). The gates the loop actually fires are
`hf policy check-{claim,edit}`, `hf drift`, and `hf checkpoint` (see C14). So the per-action
Permit/Defer/Deny arbiter is an available verb an agent *may* call, not an enforced step the
autonomous loop runs. **Confidence: high** (negative result, grep-confirmed).

### The hard gates the loop actually fires (`gates.rs`)

**C13 — The enforced hard gates are `hf drift` and `hf policy check-{claim,edit,handoff}`, both
exit-nonzero on block so `fail_mode=block` hooks stop the loop.** `cmd_drift` (`gates.rs:391-437`)
exits 1 when not clean (`gates.rs:434-436`); `cmd_policy_check` (`gates.rs:471-554`) exits 1 on any
block (`gates.rs:551-553`). The `check-edit` gate enforces `deny_without_claim` (edits with no task
claimed → block, `gates.rs:496-497`), out-of-scope writes vs the claimed `path_scope`
(`gates.rs:499-502`), and protected-file writes (`gates.rs:503-505`). **Confidence: high.**

**C14 — `hf drift` is a 10-check intent-integrity sentinel, not a stub.** `detect`
(`gates.rs:165-342`) checks all five intent_lock surfaces (objective/path_scope/acceptance/
constraint/northstar, `gates.rs:194-255`), out-of-scope edits (`gates.rs:258-283`), acceptance↔test
gaps + missing green test evidence (`gates.rs:286-306`), unsatisfied dependencies
(`gates.rs:310-322`), and an advisory undocumented-decision-surface sentinel (`gates.rs:326-339`).
Drift-detected intent mutations are themselves witnessed as `task_intent_changed.v1`, deduped by
observed lock signature (`emit_intent_changed`, `gates.rs:354-389`). **Confidence: high.**

### The typed hook contract (`hooks.rs`)

**C15 — There is a typed 14-event lifecycle contract with `handoff.hook_event.v1` /
`handoff.hook_result.v1` envelopes; a `fail_mode=block` failure exits non-zero (fail-closed) and
every run is witnessed.** `CONTRACT_EVENTS` is exactly 14 (`hooks.rs:32-47`). `cmd_hook_run`
(`hooks.rs:260-318`) runs each bound command, witnesses each `HookResult`, and returns 1 if any
hook didn't pass (`hooks.rs:286,313-317`). `severity_for` (`hooks.rs:149-155`) maps
(succeeded,fail_mode)→(severity,pass): only `(false,"block")` fails the loop; a warn failure is
advisory. **Confidence: high.**

**C16 — The contract is fail-closed against config drift: a `hooks.toml` event not in
`CONTRACT_EVENTS` is surfaced loudly, never silently dropped.** `HooksConfig::unknown_events`
(`hooks.rs:109-117`) returns non-contract events; `load` prints a WARNING when any exist
(`hooks.rs:89-98`); `cmd_hook_list` emits a `✗ DANGLING` line + JSON `unknown_events`/`conformant`
(`hooks.rs:247-254,225-227`). An unknown event to `hf hook run` is a usage error, exit 2
(`hooks.rs:270-273`). This closed a prior fail-OPEN drift (HFTASK-0069). **Confidence: high.**

**C17 — The deployed `hooks.toml` wires the gates onto lifecycle events; block-mode is used for
the gating events.** `.handoff/hooks/hooks.toml`: `TaskClaim`→`hf policy check-claim` (block,
:21-24), `PreEdit`→`hf policy check-edit` (block, :26-30), `PreHandoff`→`hf drift && hf policy
check-handoff` (block, :38-42), `PreSessionStart`→`hf session preflight` (block, :56-62),
`PostTest`→`hf drift` (block, :103-108), `SessionEnd`→`hf checkpoint && hf handoff && hf export &&
hf sync` (warn, :48-52). The file's own header states this is "how the loop runs with no human in
the loop" (`hooks.toml:5-6`). **Confidence: high.** Note divergence: `PreCommand`/`PreTest` are
`warn` here (`:82-101`) though they run gating checks — advisory at those points.

**C18 — The Claude Code harness auto-invokes the loop on session start and runs the safety net on
session end.** `.claude/settings.json`: `SessionStart`→`loop-entry.sh` (+ a `rusty-idd next` front
door, `:14-33`), `SessionEnd`→`session-end.sh` (`:4-13`). `loop-entry.sh` rehydrates via
`hf resume --compact`, then emits a directive to run the `handoff-loop` skill **only when the
ledger has a safe next task** (no work → no forced loop). **Confidence: high.**

### kb planning↔execution seam (`kb.rs`, ADR-0003)

**C19 — The kb seam is one-way by construction: git-kb plan → handoff card (IN), and execution
progress mirrored back to kb (OUT), but kb is never read back as execution truth.** IN:
`cmd_mint_from_kb` (`kb.rs:166-195`) reads a kb doc read-only, builds a `WorkOrder` with
`correlation_id = <slug>` as the cross-ref handle (`work_order_from_kb_doc`, `kb.rs:111-143`). OUT:
`write_back` (`kb.rs:261-283`) flips kb status on claim/checkpoint/done/release; it is best-effort
and degrades to a silent no-op when the correlation_id is not slug-shaped, no `.kb` exists, the
slug is not a live kb task, or git-kb is absent. `is_kb_slug` (`kb.rs:222-224`) ensures handoff's
own cards (`handoff-buildout`) and intake UUIDs are never written back. **Confidence: high.**

**C20 — The seam is plane-aware and anti-contamination: a local-`.kb` slug lands in the repo's own
`.handoff/tasks/`; a meta-`.kb` slug lands in the FLEET tasks dir, never the cwd.** `kb_root`
(`kb.rs:19-29`) prefers the repo's own `.kb` then the meta-root `.kb`; `mint_target`
(`kb.rs:155-163`) routes LOCAL→`tasks_dir()`, meta-origin→`<meta-root>/.handoff/tasks` to prevent
"envctl-domain KBTASK cards landing in handoff's KERNEL ledger" (HFTASK-0072, tests
`kb.rs:328-411`). **Confidence: high.**

### "No human in the loop" — is it enforced?

**C21 — The continuity-gating verbs structurally replace a human gate inside the kernel.**
`cmd_handoff` (`main.rs:2570-2633`) proves the active task's AgentContract via the real
`ruvector-verified` crate BEFORE writing any packet and **exits 1 on an unprovable contract**
(`main.rs:2584-2590`, contract logic `contract.rs:119+`). `cmd_done` (`main.rs:1322-1374`) blocks
completion of a test-declaring task until a green witnessed `test_result` exists
(`main.rs:1343-1348`), then auto-witnesses `pr_merged`, auto-promotes develop→trunk
(`promote_develop_to_trunk`, `main.rs:1364`), and auto-reaps the worktree on verified merge
(`main.rs:1373`). No human approval is read anywhere in these paths. **Confidence: high.**

**C22 — The AI gatekeeper produces a witnessed Approve/Deny verdict from deterministic signals and
fails closed.** `cmd_gatekeeper_check` (`gatekeeper.rs:199-299`) gathers PR changed files
(`gh`), a `cargo test --workspace` gate, a `git grep` impact scan, and an optional secrets merge-
gate, then `verdict_from_signals` (`gatekeeper.rs:140-189`) denies on empty diff, failing tests,
protected-file hits without steward clearance, or merge-gate=false. A deny exits 1
(`gatekeeper.rs:291-297`). The verdict is witnessed as `gatekeeper_judgment`
(`gatekeeper.rs:279`). ADR-0018 states "Witnessed verdicts (HFTASK-0014 gatekeeper) replace human
approval" (`docs/adr-0018-…:150-151`). **Confidence: high.**

**C23 — QUALIFIED: the gatekeeper is shallow and its merge-gate enforcement degrades to "rely on
branch protection," and it is NOT a GitHub branch-protection *required* check yet.** The module's
own header admits "Full code-intelligence … is not yet wired" — it uses `git grep` for impact, not
the AST call graph (`gatekeeper.rs:1-10,57-83`). When the `secrets` feature is unlinked (the
default CI build), `merge_gate_signal` is `None` and the verdict reason is "merge gate unavailable
… relying on required GitHub check + branch protection" (`gatekeeper.rs:171-174`); a `None`
merge-gate does **not** hard-fail (`verdict_from_signals` hard-fail set excludes `None`,
`gatekeeper.rs:179-182`; test `default_required_check_can_approve_when_merge_gate_is_unlinked`,
`gatekeeper.rs:337-346`). Making the gatekeeper a branch-protection-*required* check is ADR-0018 D8
/ HFTASK-0073 — and HFTASK-0078's own objective text states that is "NOT done here (account-level
wall)" (`.handoff/tasks/HFTASK-0078.task.json:7`). The card `HFTASK-0073.task.json` reads
`status: done`, but its scope is the local grit/gatekeeper grounding, and the documented merge flow
is `gh pr merge --admin --squash` (admin **bypass** after local verify, per repo `CLAUDE.md`), so
the GitHub-side hard requirement is not demonstrably the enforcement point. **Confidence: medium**
(card-status-vs-ledger ambiguity noted; the degradation path and the admin-bypass flow are
high-confidence from code + docs).

**C24 — Human review still has a witnessed verb but it is a record, not a loop-blocking gate.**
`cmd_review_verdict` (`main.rs:2066-2084`) appends a `review_verdict` event with a `--by` actor; it
neither blocks nor is referenced by any hook in `hooks.toml`. So a human verdict can be *recorded*
but the autonomous loop does not wait on one — consistent with the "designated agent replaces the
human gate" model, and consistent with genuine owner-walls being handled out-of-band (NEEDS-HUMAN,
ADR-0018 context `docs/adr-0018-…:10-13`). **Confidence: high.**

---

## GAPS / open questions (for the verifier)

- **G1 (verify C12):** Confirm by grep across the whole repo that `hf policy gate`/cognitum is
  invoked by *no* loop driver (hooks.toml, `.handoff/hooks/*.sh`, the `handoff-loop` skill body,
  `scripts/*`). I confirmed hooks + `.handoff/hooks/*.sh` only. If a skill markdown calls it, C12
  weakens.
- **G2 (verify C23):** Determine the *ledger* truth (not card frontmatter) for HFTASK-0073 and
  whether a GitHub branch-protection required status check named "AI Gatekeeper" actually exists on
  `develop`. Static reading cannot establish the live GitHub branch-protection config — this needs
  `gh api repos/FlexNetOS/handoff/branches/develop/protection`. Marked medium for that reason.
- **G3:** The bandit (C9/C10) and cognitum (C11) both depend on RuVector sibling crates resolved as
  path deps; whether they compile/run in a standalone clone (no sibling `RuVector/`) was not
  exercised here — runtime behavior in CI vs standalone is a build-surface question (cross-ref D
  build dimension).
- **G4:** ADR-0018 D3 (context-budget loop wrap) is spec, not observed in `session.rs`
  (`cycle_flush` is still the cadence per C7) — confirm no D3 wiring exists yet.

## Cross-dimension hooks
- The **contract-proof gate** (C21) belongs to D-contract/integrity; cited here only as the loop's
  fail-closed handoff step.
- The **fleet rollup** consumes `pr_merged`/`trunk_promoted` (C5/C21) — D-fleet.
- The **witness chain** under every event (`witness_lifecycle`, ledger append) — D-ledger.

**Claim count: 24 (C1–C24). High: 20 · Medium: 1 (C23) · negative/gap-flagged: C12, C24.**
