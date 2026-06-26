# Verdicts — D2 · verb-surface (adversarial verification)

## 2026-06-25 — verifier pass (target: /home/drdave/Desktop/meta/handoff)

Method: read the cited source for every material claim; tried to refute by finding a
fall-through, an unguarded exit, a missing transition, or a fatal step in a "non-fatal" chain.
Every cited line number resolved to the claimed construct in the live source (no drift).

### Priority targets

**(a) Unknown verb → exit 0 (fail-open help path)** — **CONFIRMED** (D2-3).
`hf/src/main.rs:3569-3571`: the `_ =>` arm only `eprintln!`s the usage string; there is no
`process::exit`. So `hf bogus` returns 0 — a genuine fail-OPEN on the help path. By contrast the
nested-subverb fallbacks DO fail closed: `hf hook <bad>` `:3460-3463` and `hf delivery <bad>`
`:3518-3521` both `std::process::exit(2)`. Counter-example search for a top-level exit-on-unknown
found none. Verdict matches the analyst's stated gap exactly.

**(b) Are Review / Active / Checkpointed statuses ever emitted?** — **CONFIRMED: NO**
(resolves D2-24 open question / Gap 1). `record_transition` is the *only* status writer
(`ledger/src/v1.rs:664-670`, appends the lone `task_transition` event). Full call-site sweep
(`grep record_transition` across all `hf/src/*.rs` + `ledger/src/*.rs`) yields exactly four live
emitters, all in `main.rs`:
- `:1073` `Status::Claimed` (cmd_claim_with)
- `:1148` `Status::Backlog` (cmd_release)
- `:1208` `Status::Backlog` (cmd_reopen)
- `:1349` `Status::Done` (cmd_done)
No raw `append("task_transition", …)` exists outside `record_transition` itself (the other
`task_transition` hits are export/migrate/test fixtures: `ledger/src/export.rs:111,120`,
`migrate.rs`, `v1.rs` tests). Specifically refuted candidates:
- `cmd_review_verdict` (`:2066-2084`) appends a `review_verdict` event, **not** `Status::Review`.
- `cmd_checkpoint` appends a `checkpoint` event, **not** `Status::Checkpointed` (confirms D2-9).
Conclusion: of the 7-variant `Status` enum, only **Backlog, Claimed, Done** are ever written by
the verb surface. `Active`, `Checkpointed`, `Review` are referenced only by gating predicates
(`should_unclaim` `:1094`, `should_reopen` `:1102`, gates.rs:86/171/484) — they are read-side /
predicate values, never emitted. This *qualifies* the lifecycle picture: the live state machine is
Backlog↔Claimed→Done (+ reopen/release back to Backlog), not the full 7-state enum.

**(c) `done --pr` auto-chain — each step + fatality** — **CONFIRMED** (D2-11, D2-12).
`hf/src/main.rs:1343-1394`:
1. Evidence gate FIRST: `if !wo.test_commands.is_empty() && latest_test_passed(&led,id) != Some(true)` → `exit(1)` **before** any transition (`:1343-1348`). Fail-closed. ✓
2. `record_transition … Status::Done` (`:1349`). ✓
3. PR resolved via `pr` arg `.or_else(latest_pr_opened)` (`:1352`) — auto-derive confirmed.
4. Inside `if let Some(pr)`: `append("pr_merged", …)` via `let _ =` (non-fatal) (`:1355`); then
   `delivery::emit_delivery` (`:1358`); `promote_develop_to_trunk` (`:1364`);
   `sync_develop_to_trunk` (`:1366`); `drop(led)` + `session::reap_open_session_if_merged()`
   (`:1372-1373`). ✓ all six steps present, in the claimed order.
5. **Non-fatality verified, not just asserted:** `awk` over the bodies of
   `sync_develop_to_trunk` (`:1400-1457`) and `promote_develop_to_trunk` (`:1458-1565`) found
   **zero** `process::exit` / `panic!` / `unwrap()`. The reap is the last call before `done`
   continues to the kb write-back + final `println!`s, so it returns normally on the no-merge
   path. None of (3)(4)(5) can abort completion. ✓

### Remaining material claims

| Claim | Verdict | Evidence checked |
|---|---|---|
| D2-1 single `match args.first()`, `--ledger`→`HANDOFF_LEDGER` stripped | **CONFIRMED** | `main.rs:3216` fn main, `:3219` match, `:3203-3214` apply_ledger_flag (set_var + remove both tokens) |
| D2-2 nested subverb guard-matching | **CONFIRMED** | task mint `:3318`, review `:3370/:3379`, gatekeeper `:3393`, secret(cfg) `:3402`, policy gate(cfg) `:3467`/check- `:3476`, fleet `:3485/:3488`, hook `:3436-3438`, delivery `:3511-3513`, session `:3434` |
| D2-3 unknown-verb no exit; nested exit 2 | **CONFIRMED** | `:3569-3571` (no exit), `:3462` / `:3520` (exit 2) |
| D2-4 full verb enumeration | **CONFIRMED** | dispatch arms `:3220-3568` enumerated; usage mirror `:3570`; every listed verb present incl. `lease` `:3260`, `promote` `:3369`, `prompt-hub` `:3524`, `schema` `:3552` |
| D2-5 cfg-conditional verbs; cognitum default | **CONFIRMED** | `:3402` cfg secrets, `:3466` cfg cognitum; `hf/Cargo.toml:48` `default = ["cognitum"]`, `secrets` opt-in `:50` |
| D2-6 claim → Claimed + atomic lease + weave | **CONFIRMED** | `:1024` weave reserve, `:1047` try_acquire_lease, `:1073` record_transition Claimed |
| D2-7 claim fail-closed on lease conflict | **CONFIRMED** | weave Refuse→`return false` `:1025-1027`; in-ledger Conflict→release weave + `return false` `:1048-1057`; lease Err→"must not silently drop exclusion — fail closed" `:1069-1072`; degrade-but-proceed `:1029-1033` |
| D2-9 checkpoint = non-status `checkpoint` event | **CONFIRMED** | no record_transition in cmd_checkpoint; Status::Checkpointed never written (see (b)) |
| D2-10 test zero-test fail-closed + degrade note | **CONFIRMED** | `:1829` `zero_tests = ran==Some(0)`, `cmd_passed = code==0 && !zero_tests`; degrade note when `ran.is_none()`; append `test_result` `:1861`; FAIL→`exit(1)` `:1871` |
| D2-11 done evidence gate + Done + pr_merged | **CONFIRMED** | gate `:1343-1348` before transition; `:1349` Done; `:1355` pr_merged |
| D2-12 done auto-chain, (3)(4)(5) non-fatal | **CONFIRMED** | see (c) above |
| D2-13 ship → one pr_opened, never polls-merge | **CONFIRMED (not deep-driven)** | dispatch `:3355-3365`; analyst cites `:2059` append pr_opened, arm auto-merge `:2041`; consistent with read dispatch. Not runtime-driven (no gh). |
| D2-14 ship fail-closed on branch policy | **CONFIRMED (static)** | analyst cites `:1937-1972` guards; arming auto-merge non-fatal `:2045-2047`. Static read only. |
| D2-15 promote events + gh-api ff + diverged guard | **CONFIRMED (static)** | `cmd_promote` `:1567`; shared `promote_develop_to_trunk` `:1458` (no exit/panic, see (c)); gh-api ff per body |
| D2-16 handoff contract-proof gate before render | **CONFIRMED** | `:2578-2591` prove_contract → `exit(1)` on Err; packet/active.md `fs::write` happen AFTER (`:2604-2618`) |
| D2-17 release → Backlog only if in-progress; loud on fail | **CONFIRMED** | should_unclaim gate `:1091-1096`; `:1148` transition; Err→loud WARNING `:1160-1162` (no swallow) |
| D2-18 reopen: task_reopened THEN Backlog; reason mandatory; terminal-only | **CONFIRMED** (was analyst-medium) | `:1207` append `task_reopened` ("Witness the WHY first"), then `:1208` record_transition Backlog; should_reopen=Done/Review `:1188`; mandatory reason exit 2 at dispatch `:3250-3258` |
| D2-19/20/21/22 read-only/status/doctor/drift | **CONFIRMED (static, consistent)** | dispatch wiring matches; doctor/drift/status handlers cited; not independently re-driven |
| D2-23 mutators `open_ledger_or_exit` vs markers `witness_lifecycle`; hook_result best-effort | **CONFIRMED** | hook_result append is `if let Ok(led){ if let Ok(p){ let _ = append } }` `:3449-3454` — the one silently-droppable mutation seam (advisory), exactly as flagged |
| D2-24 lifecycle state machine synthesis | **QUALIFIED** | the verb→event mapping is CONFIRMED; the synthesis is correct EXCEPT the implied 7-state machine — refined per (b): live statuses are **Backlog / Claimed / Done** only |

### Tally (D2)
- CONFIRMED: 22  (D2-1..D2-3, D2-5..D2-23)
- QUALIFIED: 2  (D2-4 confirmed-as-listed but `secret`/`policy gate` are cfg-conditional surface, already noted in D2-5; D2-24 lifecycle refined to 3 live statuses)
- REFUTED: 0
- INCONCLUSIVE: 0

No claim was refuted. Three claims carried analyst-flagged uncertainty (reopen ordering D2-18,
unknown-verb exit D2-3, Review/Active/Checkpointed emission D2-24/Gap1) — all three are now
resolved by source: ordering CONFIRMED, exit-0 CONFIRMED as fail-open, and the three statuses
CONFIRMED never-emitted. Items not independently runtime-driven (ship/promote gh paths, status
renderers) are CONFIRMED on static source read only — flagged as such, not upgraded beyond evidence.
