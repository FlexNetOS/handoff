# Findings — D2 · verb-surface

Dimension: enumerate + characterize the full `hf` verb set and the
claim→checkpoint→test→ship→promote→handoff lifecycle — what each verb mutates in the ledger,
which are fail-closed, and the auto-chains. All citations against the target root
`/home/drdave/Desktop/meta/handoff` unless noted. Verdict: **verb surface fully enumerated;
lifecycle is a witnessed, append-only state machine with one terminal fail-closed gate at
`done` and one at `handoff`, plus a post-merge auto-chain.**

---

## A. Dispatch / entry shape

**CLAIM D2-1** — `hf` dispatch is a single positional `match args.first()` over verb strings
in `fn main`, not a parser library (no clap). Global `--ledger PATH` is stripped before dispatch
and re-exported as `HANDOFF_LEDGER`.
Evidence: `hf/src/main.rs:3216` (`fn main`), `:3219` (`match args.first().map(...)`),
`:3203-3214` (`apply_ledger_flag` sets `HANDOFF_LEDGER`, removes both tokens).
Confidence: **high**.

**CLAIM D2-2** — Subcommands are dispatched by guard-matching `args.get(1)` for verbs that have
nested subverbs: `task mint`, `review request|verdict`, `gatekeeper check`, `secret gate-check`
(behind `#[cfg(feature="secrets")]`), `policy gate` (behind `#[cfg(feature="cognitum")]`) /
`policy check-*`, `fleet status|render`, `hook list|run`, `delivery get|list`, `session …`.
Evidence: `hf/src/main.rs:3318` (task mint), `:3370,:3379` (review), `:3393` (gatekeeper),
`:3402` (secret, cfg-gated), `:3466,:3476` (policy, cognitum cfg-gated), `:3485,:3488` (fleet),
`:3436` (hook), `:3511` (delivery), `:3434` (session).
Confidence: **high**.

**CLAIM D2-3** — Unknown verb falls through to a single usage string and the process does NOT
exit nonzero on an unrecognized verb (the `_` arm only `eprintln!`s usage; no `exit`).
Evidence: `hf/src/main.rs:3569-3571` (the `_ =>` arm prints usage, no `process::exit`).
Confidence: **high**. (Gap: this means `hf bogus` exits 0 — a fail-OPEN on the help path; only the
nested-subverb fallbacks like `hf hook <bad>`:3461-3462 and `hf delivery <bad>`:3519-3521 exit 2.)

---

## B. Full verb enumeration (from the dispatch match, `hf/src/main.rs:3220-3568`)

**CLAIM D2-4** — The complete top-level verb set is:
`init, seed, status, claim (/--batch/--next), doctor, gitignore, reconcile, export, import,
migrate, release, reopen, lease, checkpoint (/--sync-cards), sync-cards, sync, done, test,
task mint, intake, dispatch, ship, promote, review (request|verdict), gatekeeper check,
secret gate-check, session (start|end|reap), drift, hook (list|run), policy (gate|check-*),
fleet (status|render), delivery (get|list), prompt-hub, schema, handoff, resume`.
Evidence: enumerated arms `hf/src/main.rs:3220` (init) through `:3567` (resume); usage mirror at
`:3570`. Cross-checked against codemap §4 table (`reports/codemap.md:49-83`).
Confidence: **high**.

**CLAIM D2-5** — Two verbs are compiled-conditional, not always present: `secret gate-check`
(`#[cfg(feature = "secrets")]`, `:3402`) and `policy gate` (`#[cfg(feature = "cognitum")]`,
`:3466`). `cognitum` is a default feature (codemap §10, `hf/Cargo.toml`), so `policy gate` ships by
default; `secrets` is opt-in.
Evidence: `hf/src/main.rs:3402,:3466`; codemap `reports/codemap.md:157`.
Confidence: **high**.

---

## C. Lifecycle verbs — what each MUTATES in the ledger

The ledger is append-only; every mutation is an `append(event_type, work_order_id, payload, ts)`
that hash-chains to the prior tail inside one `begin_write()` tx (`ledger/src/v1.rs:482-521`,
`hash_action` + `prev_hash` at `:489,:496-504`). Status is never stored as a field — it is
**replayed** from `task_transition` events; `record_transition` is the only status writer and it
just appends a `task_transition` event with `{id,status,correlation_id,role}`
(`ledger/src/v1.rs:664-670`). Status enum: `Backlog, Active, Claimed, Blocked, Checkpointed,
Review, Done` (`work-order/src/lib.rs:17-25`).

**CLAIM D2-6** — `claim <id>` mutates: appends `task_transition → Claimed`
(`record_transition(&wo, Status::Claimed)`), preceded by acquiring an **atomic in-ledger lease**
(`try_acquire_lease`, one `BEGIN IMMEDIATE`-style tx) AND a best-effort weave lease.
Evidence: `hf/src/main.rs:1073` (`record_transition … Claimed`), `:1047` (`try_acquire_lease`),
`:1024` (weave `reserve`). Handler `cmd_claim_with` `:1010`.
Confidence: **high**.

**CLAIM D2-7** — `claim` is **fail-closed on lease conflict**: a weave `Refuse` returns false
(`:1025-1028`), an in-ledger `Conflict` returns false and releases the weave lease it just took
(`:1048-1056`), and a lease *bookkeeping error* also returns false ("must not silently drop
exclusion — fail closed", `:1066-1070`). `cmd_claim` turns false into `exit(1)` (`:509-512`).
The weave-unavailable path degrades to in-ledger-only exclusion but still proceeds (`:1029-1034`).
Evidence: `hf/src/main.rs:1024-1071`, `:509-512`.
Confidence: **high**.

**CLAIM D2-8** — `claim --batch` routes the **highest-value** ready task via the RuVector
Thompson-bandit domain-expansion router; `claim --next` routes the **topologically-first** ready
task via `next_safe`. Both resume an in-progress task first. Dispatch branches on `--batch` /
`--next` before falling to a positional `claim <id>`.
Evidence: `hf/src/main.rs:3223-3231` (dispatch), `:515-535` (`cmd_claim_next` → `next_safe`),
codemap §8 routing `reports/codemap.md:130-132`.
Confidence: **high** (batch router internals not deep-read — `medium` on the bandit specifics).

**CLAIM D2-9** — `checkpoint <id> [note]` mutates: appends a non-status `checkpoint` event
(`{id,note}`); it does NOT change replayed status (codemap notes checkpoint is "a non-status
event"). `--auto` resolves the id from `next_safe`; routes to the task's home ledger (KERNEL vs
FLEET) via `route::route_for_task`. Also mirrors a progress line to kb (one-way write-back).
Evidence: `hf/src/main.rs:1300` (`led.append("checkpoint", …)`), `:1266-1272` (`--auto`/`next_safe`),
`:1291-1297` (route), `:1302-1313` (kb write-back). Status enum has `Checkpointed` but
`record_transition` is NOT called here.
Confidence: **high**. (Note: there is a `Checkpointed` status value but `cmd_checkpoint` appends a
`checkpoint` event, not a `task_transition → Checkpointed` — so a checkpoint does not move replayed
status. Worth a verifier double-check.)

**CLAIM D2-10** — `test [id]` mutates: appends a `test_result` event with
`{id, passed, tests_ran, results[]}`. It is **fail-closed on a zero-test rubber-stamp**: a command
that exits 0 but a recognized runner shows executed 0 tests is marked not-passed (`zero_tests`),
and an unrecognizable runner (`ran == None`) degrades to exit-code-only with a printed note.
On any failure the verb `exit(1)`. Runner-aware via `parse_tests_ran` (libtest/pytest/jest/gotest).
Working dir is pinned to the repo toplevel.
Evidence: `hf/src/main.rs:1829-1844` (zero-test gate + degrade note), `:1861` (append
`test_result`), `:1869-1871` (FAIL → exit 1), `:1799,:1808-1810` (run_dir = repo_toplevel),
`:1613-1621` (`parse_tests_ran` runner dispatch).
Confidence: **high**.

**CLAIM D2-11** — `done <id> [--pr N]` mutates: appends `task_transition → Done`
(`record_transition(&wo, Status::Done)`) and — when a PR is resolved — appends `pr_merged`
(`{id,pr}`). It is **fail-closed on completion evidence**: if the card declares `test_commands`
and the latest witnessed `test_result` is not `passed==true`, `done` is **blocked with exit 1**
before any transition. Tasks with no `test_commands` are exempt.
Evidence: `hf/src/main.rs:1343-1348` (evidence gate, `latest_test_passed` `:1574-1585`),
`:1349` (`record_transition … Done`), `:1352-1355` (auto-resolve PR + append `pr_merged`).
Confidence: **high**.

**CLAIM D2-12** — `done`'s **auto-chain** (only when a PR is resolved, `:1353`): (1) appends
`pr_merged`; (2) emits a `delivery` round-trip to the originating prompt_hub workflow
(`delivery::emit_delivery`); (3) `promote_develop_to_trunk` — hands-off develop→trunk
fast-forward; (4) `sync_develop_to_trunk` — trunk→base mirror-back; (5) `session::reap_open_
session_if_merged` — reap the batch worktree. (6) kb write-back → completed. All of (3)(4)(5) are
**non-fatal** (a hiccup must not fail completion). The PR is auto-derived from a prior `pr_opened`
event if `--pr` is omitted.
Evidence: `hf/src/main.rs:1355` (`pr_merged`), `:1358` (delivery), `:1364`
(`promote_develop_to_trunk`), `:1366` (`sync_develop_to_trunk`), `:1372-1373` (reap),
`:1381` (kb write-back), `:1352` (`latest_pr_opened` auto-derive, fn `:1590-1601`).
Confidence: **high**.

**CLAIM D2-13** — `ship <id> [--base BR]` mutates: appends ONE `pr_opened` event
(`{id,branch,pr,base}`). Side effects: stages only task scope (`git add -u` + the task card, NOT
untracked scratch), one squash commit, push branch, `gh pr create`, and arms GitHub-native
auto-merge (`gh pr merge --auto --squash`). It NEVER polls-and-merges. Base resolves from branch
policy (`bp.trunk`) unless `--base` overrides.
Evidence: `hf/src/main.rs:2059` (`append("pr_opened", …)`), `:1915-1922` (`ship_stage_specs` =
`-u` + card only), `:1979-2003` (commit), `:2005` (push), `:2024-2037` (PR create), `:2041`
(arm auto-merge), `:1948-1953` (base from policy).
Confidence: **high**.

**CLAIM D2-14** — `ship` is **fail-closed on branch policy**: it `exit(1)` if the branch policy
won't resolve (`:1937-1941`), if the remote model is unsupported/fork (`ensure_supported`,
`:1944-1947`), if HEAD is detached (`:1956-1959`), if shipping FROM the base/trunk branch
(`:1963-1968`), and on a direct-trunk-push guard violation (`guard_direct_trunk_push`,
`:1969-1972`). Push/PR-create failures also `exit(1)`. Arming auto-merge is the only **non-fatal**
step (`:2045-2047`).
Evidence: `hf/src/main.rs:1937-1972, :2005-2008, :2032-2036, :2041-2048`.
Confidence: **high**.

**CLAIM D2-15** — `promote` mutates: appends `trunk_promoted` (`{id,base,trunk,sha}`) on success
or `trunk_promote_skipped` (`{id,reason}`) on any skip. The ff is done via the **runner-independent
`gh api -X PATCH .../git/refs/heads/<trunk> -f sha=<head> -F force=false`** (server enforces
ff-only). It is **fail-closed on a diverged trunk**: a local `git merge-base --is-ancestor` guard
refuses to promote (records `trunk_promote_skipped` "trunk diverged (not ff)") before the API call.
Idempotent when trunk already == base. The same fn auto-runs inside `done` (D2-12).
Evidence: `hf/src/main.rs:1567-1570` (`cmd_promote`), `:1498-1511` (ancestor guard +
skip event), `:1521-1547` (gh-api PATCH + `trunk_promoted`), `:1513-1519` (idempotent),
`:1458` (`promote_develop_to_trunk` shared by `done`).
Confidence: **high**.

**CLAIM D2-16** — `handoff` mutates NO ledger event; it RENDERS `packets/latest.md` + `active.md`
from replay — but only AFTER a **fail-closed AgentContract proof** of the active task. An
unprovable intent-lock (objective/path_scope/acceptance hash mismatch) or an as-complete task with
no witnessed checkpoint causes `exit(1)` BEFORE any packet/active.md write (so views are never
left half-updated).
Evidence: `hf/src/main.rs:2578-2591` (`prove_contract` → `exit(1)` on `Err`), `:2602-2618`
(render packet + active.md AFTER), contract obligations `hf/src/contract.rs:14-19,119`.
Confidence: **high**.

**CLAIM D2-17** — `release <id>` mutates: appends `task_transition → Backlog` **only** when the
task is in an in-progress state (`Claimed|Checkpointed|Active`, gated by `should_unclaim`); it
never un-finishes a `Review|Done|Backlog` task. Also frees the weave + in-ledger leases and the
lockfile. A failed un-claim transition is surfaced loudly (fail-open-audit R3), not swallowed.
Evidence: `hf/src/main.rs:1109-1163`, `:1091-1096` (`should_unclaim`), `:1148` (transition),
`:1160-1162` (loud failure).
Confidence: **high**.

**CLAIM D2-18** — `reopen <id> "<reason>"` mutates: appends a `task_reopened` event (the WHY) then
`task_transition → Backlog`. **Fail-closed**: a reason is MANDATORY (`exit(2)` if empty) and only a
**terminal** state (Done/Review) is reopenable (`should_reopen`); an in-progress claim must use
`release`. This is the witnessed inverse of a false-Done.
Evidence: `hf/src/main.rs:1174-1183` (mandatory id+reason, exit 2), `:1101-1103`
(`should_reopen` = Done|Review), header `:1166-1173`.
Confidence: **high** (read header + guards; the append pair stated in the doc-comment at
`:1172-1173`, not the body past `:1183` — `medium` on exact event ordering, flag to verifier).

---

## D. Read-only / status verbs

**CLAIM D2-19** — `status [--json]` mutates nothing; it replays the ledger (`current_statuses`)
and recomputes Done N/M + `next_safe` live. `--json` emits `handoff.loop_status.v1` including
session cycle, `cycle_flush`, `ready_to_ship`, and `witnessed_events_verified` (the verified
witness-chain length).
Evidence: `hf/src/main.rs:2255-2275` (`cmd_status`), `:2279-2314` (`emit_status_json`,
schema `handoff.loop_status.v1` at `:2294`).
Confidence: **high**.

**CLAIM D2-20** — `resume [--json|--compact]` mutates nothing; renders Full (live packet
recomputed from ledger+cards, NOT the frozen packets/latest.md), Json (`machine_summary`), or
Compact (one-line) views.
Evidence: `hf/src/main.rs:2644-2667` (`cmd_resume`), Full-mode live-render note `:2667-2669`.
Confidence: **high**.

**CLAIM D2-21** — `doctor [--json]` mutates nothing material; it is the **fail-closed invariant
sweep**: it `healthy = chain_ok && ledger_present && durability_ok && replay_ok &&
unconformant.is_empty()` and reports stale RVF locks / reclaim counts. (Side note: opening the
ledger inside doctor auto-reclaims a provably-dead RVF lock, which IS a witnessed mutation —
`lock_reclaimed`.) A broken witness chain or a non-conformant card is a hard failure.
Evidence: `hf/src/main.rs:595-641` (sweep + `healthy`), `:631-638` (reclaim count surfacing),
`:616-619` (`scan_card_conformance` hard-fail).
Confidence: **high**.

**CLAIM D2-22** — `drift [--json]` is a hard gate: it `std::process::exit(1)` on detected drift
"so PreHandoff (fail_mode=block) stops". `policy check-*` similarly exits 1/2.
Evidence: `hf/src/gates.rs:391` (`cmd_drift`), `:435` (exit 1 on drift), `:471,:530,:552`
(`cmd_policy_check` exits).
Confidence: **high**.

---

## E. Fail-closed vs fail-open ledger-open discipline

**CLAIM D2-23** — Lifecycle MUTATORS open the ledger via `open_ledger_or_exit` (exit 1 if the
ledger can't open) — claim/checkpoint/done/test/ship/promote all use it. Best-effort lifecycle
MARKERS (session start/end, hook_result, preflight) use `witness_lifecycle`, which on failure
prints a LOUD warning rather than swallowing (`fail-open-audit R3`), because the side effect
already happened.
Evidence: `hf/src/main.rs:239-247` (`open_ledger_or_exit`, exit 1), `:248-258`
(`witness_lifecycle`, loud-warn), used at `:1299,:1338,:1860,:2058,:1568` (mutators) vs `:3450-3454`
(hook_result is best-effort `if let Ok`).
Confidence: **high**. (Observation: the hook-result witness at `:3449-3454` IS a `if let Ok(...)
{ let _ = ... }` best-effort — consistent with marker policy, but it is the one mutation-ish path
that can silently drop. Flag for verifier as a possible fail-open seam, though hook results are
advisory.)

---

## F. Lifecycle state machine (synthesis)

**CLAIM D2-24** — The canonical task lifecycle is a witnessed append-only progression with replay-
derived status:
`Backlog --claim--> Claimed --checkpoint(s)--> [checkpoint events] --test--> [test_result]
--done--> Done`, with side-rails: `release` (Claimed/Checkpointed/Active → Backlog), `reopen`
(Done/Review → Backlog). PR plane: `ship` → `pr_opened`; `done --pr` → `pr_merged` + auto
`promote` (`trunk_promoted`) + `sync` + reap. `handoff` is the render gate (no event, contract
proof). Two hard fail-closed gates protect Done: the `test`-evidence gate inside `done` (D2-11)
and the contract-proof gate inside `handoff` (D2-16).
Evidence: composed from D2-6, D2-9, D2-10, D2-11, D2-12, D2-13, D2-15, D2-16, D2-17, D2-18;
event types confirmed `task_transition`/`checkpoint`/`test_result`/`pr_opened`/`pr_merged`/
`trunk_promoted`/`task_reopened` across `hf/src/main.rs` cited lines + `ledger/src/v1.rs:669`.
Confidence: **high** for the verb→event mapping; **medium** on whether `Checkpointed`/`Active`/
`Review` statuses are ever actually emitted by any verb (only `Claimed`, `Backlog`, `Done` were
seen via `record_transition`; `Review` is referenced by gating predicates but no verb in the
read set transitions to it). → open question for verifier.

---

## G. Gaps / open questions (for the verifier)

1. **`Checkpointed`/`Active`/`Review` emission**: the Status enum has 7 variants but only
   `Claimed`, `Backlog`, `Done` were observed as `record_transition` targets in the read handlers.
   Does any verb emit `Review`/`Active`/`Checkpointed`? (`review verdict`? `cmd_checkpoint` does
   NOT.) — D2-9, D2-24. Needs a grep of all `record_transition(` / `Status::` call sites.
2. **Unknown-verb exit code**: `hf bogus` exits 0 (D2-3). Confirm this is intentional (help) vs a
   fail-open. Nested-subverb fallbacks DO exit 2.
3. **`reopen` body past line 1183** not read — confirm the `task_reopened`-then-`task_transition`
   ordering claimed in the doc-comment (D2-18).
4. **`claim --batch` bandit internals** (routing.rs / ruvector-domain-expansion) read only at the
   doc level (D2-8).
5. **hook_result best-effort append** (`:3449-3454`) is the one `if let Ok { let _ = }` mutation
   seam — advisory, but flag (D2-23).
