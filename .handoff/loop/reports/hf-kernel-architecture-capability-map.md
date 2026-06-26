# `hf` Continuity Ledger Kernel — Architecture & Capability Map (decision-grade)

**Run:** code-research, handoff repo · **Synthesized:** 2026-06-25
**Target root:** `/home/drdave/Desktop/meta/handoff`
**Evidence basis:** CONFIRMED/QUALIFIED claims only, from `verdicts-D1..D6.md` (verifier passes
2026-06-25). REFUTED/INCONCLUSIVE items are excluded from the verdict and listed under Gaps.
**Verifier tally across dimensions:** D1 12C/0R · D2 22C/2Q/0R · D3 9C+3 caveats/0R · D4 19C/1Q/0R ·
D5 9C/1Q/1 sub-claim-REFUTED · D6 11C/3Q/0R/1 INCONCLUSIVE.

---

## 1. Verdict

The `hf` kernel **is what it claims structurally to be — a local-first, append-only, witnessed
continuity ledger with a fail-closed formal-proof gate on handoff — and its *write-side* guarantees
are genuinely strong, but one load-bearing *read-side* guarantee is materially weaker than the
doctrine asserts.** Concretely: every state transition is an append-only, SHA3-hash-chained event in
a pure-Rust ACID redb store (no C in the default build, live-verified), the develop→trunk pipeline is
a real witnessed auto-chain (`claim→checkpoint→ship→promote→handoff→done`), `hf handoff` will not
render a packet unless the active task's intent-lock is formally proven through the **real**
`ruvector-verified` crate (10/10 contract tests pass against the genuine verifier), and fleet rollup
re-derives content hashes to detect tampering of rolled rows. **The caveat that gates the trust
rating:** the runtime `verify_witness_chain` routine is a count-only tautology — it was *runtime-proven*
to return the event count for honest, tampered, and garbage hashes alike — so the kernel's actual
audit control against binary-cache tampering is **git history over the committed JSONL export, not the
witness routine the doctrine names as "tamper-evident."** Net: trustworthy as an append-only,
git-anchored, proof-gated continuity substrate; **over-stated** as a runtime tamper-*detecting* one.
Confidence: **High** on architecture and write-side guarantees; **High** on the witness-verification
caveat (concretely reproduced).

---

## 2. Architecture + capability map

### 2.1 Entry points & dispatch
- **`hf` binary** — `fn main()` at `hf/src/main.rs:3217`; single positional dispatch
  `match args.first()` at `:3219` over the verb strings (`:3220-3568`). Global `--ledger PATH` /
  `HANDOFF_LEDGER` are stripped before dispatch via `apply_ledger_flag` (`:3203-3214`, set_var +
  remove both tokens). [D2-1 CONFIRMED]
- **`hf-mcp` binary** — `hf/src/bin/hf-mcp.rs`, a JSON-RPC-over-stdio MCP server exposing the verbs
  as tools by shelling to `hf` (codemap §3). PRD's documented `hf mcp serve` invocation differs — it
  is a *separate binary*, not a subverb [D6 CLAIM-B5 QUALIFIED: naming drift, capability present].

### 2.2 Ledger / witness / RVF substrate (`ledger/`)
- **Append-only, write-time hash chain.** `append` (`v1.rs:482-523`): computes
  `action_hash = hash_action(...)` (`:489`), reads the tail *inside* the same write tx (`:496`),
  sets `next_seq = tail_seq+1`, chains `prev_hash` = prior row's `action_hash`. There is **no
  delete/update verb on EVENTS**. `hash_action` = SHA3-256 over `event_type‖work_order_id‖payload`
  (`v1.rs:291-297`). [D1-C1 CONFIRMED]
- **ACID, serializable, single-writer.** append/lease/rollup each run in one `begin_write()`
  (`:491/:549/:743`); cross-process exclusion via `is_busy` on `DatabaseAlreadyOpen` (`:343-348`);
  `cargo test -p ledger` green (32 passed) incl. concurrency/cross-process tests. [D1-C2 CONFIRMED]
- **Atomic in-ledger lease CAS.** `try_acquire_lease` (`:538-604`): one `begin_write()`, resolves
  holder in-tx, foreign-live-holder → `drop(tx); return Conflict` with **no commit**, else chains a
  `lease_acquired` event. [D1-C4 CONFIRMED]
- **No-C default build (live).** `cargo tree -p ledger` shows no `rusqlite|libsqlite|-sys`; the C
  dependency appears **only** under the non-default `legacy-sqlite` feature. redb is pure-Rust.
  Legacy SQLite files **fail closed** via `file_is_legacy_sqlite` magic-byte guard (`:379-389`),
  pointing the operator at `hf migrate`. [D1-C7, D1-C9 CONFIRMED]
- **Committed-truth text export.** `export.rs` writes a seq-ordered JSONL (`:43-60`); `rebuild_from_jsonl`
  re-appends through the authoritative `append` so the hash is recomputed from payload, with a
  fail-closed count gate (`:84-89`). The binary `ledger.db`(+`.rvf`) is a gitignored rebuild cache.
  [D1-C8 CONFIRMED — but see Trust §3 caveat (a): the gate is count-only]
- **v2 overlay.** v2 delegates all authoritative ops to v1 and ingests an RVF semantic-recall batch
  with the RVF error *discarded by design* (`v2.rs:304-338`); recall is fail-open, continuity is not.
  [D1-C6 CONFIRMED, minor stale doc]

### 2.3 Verb surface & the full lifecycle auto-chain
The live state machine is **Backlog ↔ Claimed → Done** (+ reopen/release back to Backlog) — *not*
the full 7-variant `Status` enum (see Trust caveat (e)). [D2-24 QUALIFIED, D2-(b) CONFIRMED]

- **`claim`** → `Status::Claimed` only after a weave reserve (`:1024`) **and** an atomic in-ledger
  lease (`:1047`); fail-closed on either conflict — lease `Err` is explicitly "must not silently
  drop exclusion — fail closed" (`:1069-1072`). [D2-6, D2-7 CONFIRMED]
- **`checkpoint`** → witnessed `checkpoint` event (NOT a `Status::Checkpointed` transition).
  [D2-9 CONFIRMED]
- **`test`** → fails closed on **zero executed tests** (`zero_tests = ran==Some(0)`, `:1829`); FAIL →
  `exit(1)` (`:1871`); emits a degrade note when the runner count is `None`. [D2-10 CONFIRMED]
- **`ship`** → opens exactly one PR (`pr_opened` event), never polls for merge; fail-closed on branch
  policy; arming auto-merge is non-fatal. [D2-13, D2-14 CONFIRMED static]
- **`promote`** → hands-off develop→trunk fast-forward via gh-api PATCH (runner-independent); shared
  `promote_develop_to_trunk` (`:1458`) contains **zero** `exit/panic/unwrap`. [D2-15 CONFIRMED static]
- **`done --pr` auto-chain** (`:1343-1394`) — verified step-by-step and for fatality:
  1. **Evidence gate FIRST** — if the card has `test_commands` and the latest test did not pass,
     `exit(1)` **before any transition** (`:1343-1348`). Fail-closed. [D2-11 CONFIRMED]
  2. `record_transition … Status::Done` (`:1349`).
  3. PR resolved via arg `.or_else(latest_pr_opened)` (`:1352`) — auto-derived.
  4. `pr_merged` append (non-fatal `let _`) → `emit_delivery` → `promote_develop_to_trunk` →
     `sync_develop_to_trunk` → `reap_open_session_if_merged`. **Non-fatality proven**, not asserted:
     `awk` over both promote/sync bodies found zero `exit/panic/unwrap`; none of these can abort
     completion. [D2-12 CONFIRMED]
- **`handoff`** → the contract-proof gate (below) runs and may `exit(1)` **before** any packet /
  `active.md` write (`fs::write` at `:2604-2618` is strictly after the gate). [D2-16, D3-C1 CONFIRMED]

### 2.4 Contract-proof gate (`hf/src/contract.rs`) — the fail-closed kernel guarantee
- **Real proof crate, not a reimpl.** `hf/Cargo.toml:33` path-deps `ruvector-verified`; the passing
  tests assert `verifier_version == 0x0001_0000`, binding to the genuine crate (`ProofEnvironment`,
  `EQ_REFL="Eq.refl"`). [D3-C2 CONFIRMED]
- **Exit(1) before any disk write.** `Option::map` runs the proof closure eagerly at `main.rs:2578`;
  on `Err` → `process::exit(1)` (`:2587-2588`); the only ops before the gate are reads. [D3-C1 CONFIRMED]
- **Eq.refl term minted ONLY on hash equality** (`contract.rs:102-115`); unequal → `Ok(None)` →
  `ProofError::IntentDrift` → blocked. Re-derivation reuses `WorkOrder::compute_intent_lock` **exactly**
  (single blake3 source) — no parallel hash. [D3-C3, D3-C4 CONFIRMED]
- **Obligations:** 3 base intent obligations always; +2 conditional (constraint/northstar, skipped on
  empty lock); +1 completion obligation, only when status ∈ {Review, Done}, requiring ≥1 witnessed
  checkpoint else `UnprovenCompletion`. Tests pin 3/5/4 obligations exactly. [D3-C5, D3-C6 CONFIRMED]
- **All three `ProofError` variants fail the handoff closed**; attestation is tamper-evident
  (siphash-256 over real proof state + bound to this contract's 5 lock hashes) and is witnessed into
  the rendered packet. **Live oracle: `cargo test -p hf contract` → 10 passed, 0 failed**, incl.
  `drifted_intent_blocks_handoff`, `complete_task_without_checkpoint_is_unproven`,
  `attestation_is_deterministic`. [D3-C7, D3-C8, D3-C9 CONFIRMED — G3 upgraded with runtime evidence]
- **Honest scope:** with no active task, `active_task` returns `None`, so `hf handoff` writes a
  packet with no proof section — correctly scoped (nothing in flight to prove), not a fail-open of
  the gate. [D3-G1 CONFIRMED caveat]

### 2.5 Fleet rollup (`hf/src/fleet.rs`) — daemonless, git-transport
- **Daemonless, git-transport, hand-rolled YAML.** `cmd_fleet_status` is straight-line
  filesystem/git/ledger I/O (no spawn/thread); `part_c_rollup` is a one-shot `for member` loop.
  [D4-1/2/3 CONFIRMED]
- **Rollup provenance re-derivation (a real content check).** `verify_rollup_provenance`
  (`v1.rs:853-892`) recomputes `hash_action(event_type, work_order_id, payload_json)` (`:882`) and
  byte-compares to the stored `origin_action_hash` (`:883`); mismatch **or** `None` origin →
  counted (fail-closed); `is_faithful = mismatched==0`; broken bridge → WARNING (not swallowed).
  Refutation that it's a self-comparison **failed** — it re-derives from content. [D4-13/14/15 CONFIRMED]
- **P7 residency = git-decided, not filesystem.** Every verdict predicate calls `git ls-files` /
  `git check-ignore` (`fleet.rs:148-202`); disk presence is only the precondition. A *tracked* binary
  `.db` is banned; a gitignored on-disk `.db` is legitimate; a committed `ledger.events.jsonl` is
  required. [D4-17/18/19 CONFIRMED]
- **Three independent integrity layers:** central chain verify, per-repo chain verify, and rollup
  provenance verify — each independent, broken bridge surfaced as WARNING. [D4-15 CONFIRMED]
- **Member packets render from the FLEET ledger + member capsule**, North Star from the capsule (not
  hardcoded), avoiding the ADR-0006 bug. [D4-21 QUALIFIED: substance holds; cited subpath imprecise —
  actual read is `.handoff/context/capsule.json`]

### 2.6 Agent / loop model
- **Authoritative "next task" is topological `next_safe`** (`main.rs:319-334`): resume in-progress,
  else first Backlog card with all deps Done; 14 call sites incl. `cmd_handoff` and both policy
  gates. The Thompson-bandit (`route_with_history`) is scoped to **`hf claim --batch` only**
  (one non-test caller, `main.rs:988`) and chooses *among* topologically-safe candidates — it never
  overrides dependency safety. [D5-C8, D5-C9 CONFIRMED]
- **Continuity verbs replace the human gate inside the kernel:** `cmd_handoff` proves the contract
  and `exit(1)`s before rendering; `cmd_done` blocks on a missing green `test_result`, then
  auto-witnesses `pr_merged` + promote/sync/reap — no human-approval read in these paths.
  [D5-C21 CONFIRMED]
- **AI Gatekeeper is a witnessed, fail-closed status check** (`cmd_gatekeeper_check`,
  `gatekeeper.rs:199-299`): gathers gh changed files + `cargo test --workspace` + impact scan +
  optional merge-gate, Deny → `exit(1)`, witnesses `gatekeeper_judgment`. **It IS a required
  branch-protection check on `develop`** (live `gh api` → contexts include "AI Gatekeeper") produced
  by `.github/workflows/ai-gatekeeper.yml`. [D5-C22 CONFIRMED; D5-C23(c) — see Trust caveat (c)]
- **Typed 14-event lifecycle-hook contract** (`hooks.rs:32-47`, count confirmed); `hooks.toml` wires
  the hard gates (`check-claim`/`check-edit`/`drift && check-handoff`/`preflight`/`checkpoint`) —
  header: "how the loop runs with no human in the loop." [D5-C15, D5-C17 CONFIRMED]

---

## 3. Trust assessment

### Genuinely strong (CONFIRMED)
- **Write-side append-only SHA3 hash chain.** No mutate/delete verb on EVENTS; tail read in-tx;
  `prev_hash` = prior `action_hash`. [D1-C1]
- **ACID single-writer redb**, green concurrency tests; atomic lease CAS that does not commit on
  conflict. [D1-C2, D1-C4]
- **No-C trust boundary, live-verified** — C only under non-default `legacy-sqlite`; legacy files
  fail closed. [D1-C7, D1-C9]
- **Fail-closed contract gate with a real `ruvector-verified` Eq.refl proof**, exits before any disk
  write, 10/10 contract tests pass against the genuine verifier. [D3-C1..C9]
- **Rollup provenance re-derivation** — a true content re-hash of rolled rows, fail-closed on
  mismatch or null. [D4-13/14/15]
- **P7 git-truth residency** — residency decided by `git ls-files`/`check-ignore`, not the
  filesystem. [D4-17/18/19]
- **`done --pr` evidence gate + non-fatal auto-chain** — completion blocked without a green test
  result; promote/sync/reap proven incapable of aborting completion. [D2-11, D2-12]

### Material caveats (CONFIRMED weaknesses — carry these into any decision)
- **(a) `verify_witness_chain` is a count-only tautology — RUNTIME-PROVEN.** The routine builds
  `WitnessEntry`s with `prev_hash:[0u8;32]` (stored linkage discarded) and the **stored**
  `action_hash` (never recomputed from payload), then calls `rvf-crypto` which **overwrites**
  `prev_hash` with its own running shake and verifies only internal consistency — so it reduces to
  `Ok(events.len())`. A 3-entry tamper drive returned `verified=3` for honest, tampered, garbage,
  and bogus-prev hashes alike. Stored `EventBody.prev_hash` is read **only in tests** (zero
  production reads). **Consequence:** the count-only JSONL rebuild gate (C8) accepts a tampered
  payload, and `hf doctor`/`hf import` lean on this routine. The real audit control is **git history
  over the JSONL export + write-time chaining (correct but never re-verified)** — NOT
  `verify_witness_chain`. The doctrine's "tamper-evident" (`v1.rs:827`) **overstates** the
  verification-time guarantee. `G2`: the `.expect("witness chain must verify")` at `v1.rs:844` is a
  panic, not a returned `Err` (low impact, but literally true). [D1-G1, D1-G2 CONFIRMED]
  *(Note the contrast: `verify_rollup_provenance` (C5/D4-14) IS a real content check — but only for
  rolled fleet rows, not native local events.)*
- **(b) The cognitum action governor is wired into NO hook/loop.** `hf policy gate` is a real
  fail-closed verb (exits 1 on defer/deny, witnesses `cognitum_decision`), but a grep-exhaustive
  sweep of `hooks.toml`, `scripts/*`, and `.claude/*` returns **zero** firings. Nothing invokes it;
  the deployed hooks wire `check-claim`/`check-edit`/`check-handoff`/`drift`/`preflight`/`checkpoint`
  instead. It is built-but-dormant. [D5-C12 CONFIRMED]
- **(c) The AI Gatekeeper required check is admin-bypassable.** It IS a required status check on
  `develop` (live) — but `enforce_admins.enabled = false` and the documented standing flow is
  `gh pr merge <n> --admin --squash` (CLAUDE.md:42). So the check exists and is enforced
  structurally, **yet routinely bypassed by the owner via `--admin`**; the check itself is also
  shallow (git-grep impact scan, not the call graph) and its merge-gate degrades to non-blocking on
  `None`. The "no human at the GitHub boundary" claim is *structurally enforced yet operationally
  bypassed*. [D5-C23: (a)(b) CONFIRMED, (c) "not a required check" REFUTED — bypass is the real hole]
- **(d) Unknown verb exits 0 (fail-open help path).** `main.rs:3569-3571`: the `_ =>` arm only
  `eprintln!`s usage with no `process::exit`, so `hf bogus` returns 0. By contrast nested subverbs
  *do* fail closed (`hf hook <bad>` / `hf delivery <bad>` → `exit(2)`). [D2-3 CONFIRMED]
- **(e) Only 3 of 7 task statuses are ever emitted.** `record_transition` is the only status writer;
  the four live emitters write only `Claimed`, `Backlog` (×2), `Done`. `Active`, `Checkpointed`,
  `Review` are read-side/predicate values only — never written. The lifecycle is **Backlog ↔
  Claimed → Done** (+ reopen/release), not the implied 7-state machine. [D2-(b), D2-24 CONFIRMED]

---

## 4. Doc-vs-code overclaims (D6 CONFIRMED — the PRD/INTEGRATION docs over-advertise the shipped CLI)

Every material overclaim the analyst flagged survived adversarial refutation (0 false accusations):

- **`hf index` is unbuilt** — no dispatch arm, usage omits it; PRD §9 advertises it. `.handoff/maps/*`
  (its output) is absent. [D6-B1, D6-B3 CONFIRMED]
- **`hf plan` is unbuilt** — no dispatch arm; the DAG is computed ad-hoc in `routing.rs` instead.
  [D6-B2 CONFIRMED]
- **Drift sentinel is a count-match, not a content-match of PRD §12.3.** PRD checks #1 (task-active),
  #8 (contradicts-a-decision-record), #10 (handoff-state-updated) have **no** branch in `gates::detect()`;
  the "8→10" history is a count match while a *different* 10 (northstar-revision, dependency-unsatisfied,
  missing-test-evidence) was swapped in. The PRD was never amended. [D6-A1, D6-A2 CONFIRMED]
- **`crates/*` (rusty-idd) is orphaned from `hf`.** Zero refs from `hf/`, `ledger/`, `work-order/`
  (manifests and source); `crates/core/src/lib.rs` self-describes as an independent IDD toolkit. It
  compiles alongside `hf` but `hf` never invokes it — a plan (INTEGRATION-RUSTY-IDD.md promises
  `hf index --intent-aware`, capsule merge — all unbuilt), not a feature. [D6-D1, D6-D2 CONFIRMED]
- **Root `Cargo.toml` reality:** `resolver = "2"`, `edition = "2021"`, members = 3 functional crates
  (`work-order, ledger, hf`) + 5 rusty-idd (`crates/{cli,core,runner,spec,tui}`). The PRD's "12
  `handoff-*` crates / resolver 3 / edition 2024" is unrealized. **No `[workspace.lints]`** despite
  PRD's mandated `unwrap_used = "deny"`; `.unwrap()` is in fact used (`gates.rs:413,542`). [D6-C1, D6-C2 CONFIRMED]
- **Naming drift (capability present):** `hf start` → `hf session start`; `hf mcp serve` → separate
  `hf-mcp` binary; capsule `next_command` default is `hf resume` not PRD's `hf claim --next`.
  [D6-B4, D6-B5, D6-E1 QUALIFIED/CONFIRMED]
- **Not an overclaim:** redb superseding PRD's SQLite is ADR-0017-sanctioned evolution, correctly
  flagged as legitimate. [D6-C3]

---

## 5. Confidence, gaps, and recommendations

### Confidence
- **Overall verdict: High.** The strong write-side guarantees and the fail-closed proof gate are
  backed by live oracles (`cargo test -p ledger` 32 passed, `cargo test -p hf contract` 10 passed)
  and direct source reads of both sides of every cited call. The headline caveat (a) is not inferred
  — it was **reproduced at runtime** driving the real `rvf-crypto`.
- **Per-area:** ledger write-side / contract gate / fleet provenance / P7 residency — High. Verb
  surface lifecycle — High (D2 fully source-verified). Agent/loop firing contract — Medium-High
  (cognitum-dormant and gatekeeper-required-but-bypassable both settled; several session/gates
  internals not re-verified this pass, see gaps).

### Named gaps (INCONCLUSIVE or unexamined — NOT asserted as fact)
- **D5 session/gates internals (INCONCLUSIVE this pass):** `session.rs` lease/preflight/worktree/grit/
  reap/cycle-counter (C1–C7), `gates.rs` exit codes + the 10-check drift internals (C13/C14), the
  `unknown_events`/envelope internals of the hook contract (C16 detail), `.claude/settings.json`
  auto-invoke exact wiring (C18), the `kb.rs` one-way seam + plane routing (C19/C20), and whether
  `review verdict` is record-only vs blocking (C24). Reap-on-merge is corroborated *downstream* via
  `cmd_done`, but the `session.rs` bodies were not opened. A deeper pass should drive these.
- **Bandit math (CONFIRMED wiring / plausible math):** `route_with_history` wiring is confirmed but
  the Bayesian-update internals (`routing.rs:55-66`) were not re-derived line-by-line. [D5-C10]
- **Packet renderer (INCONCLUSIVE):** the "17 packet sections" claim was analyst-self-flagged and not
  verified — diff `render_packet_md` headings vs PRD §14 in a follow-up. [D6-E2]
- **Not deep-driven (static-only):** `ship`/`promote` gh paths and the status/doctor/drift renderers
  were read statically but not runtime-driven (no `gh` in the harness). [D2-13/14/15/19-22]

### Recommendation list (candidate kernel-hardening tasks — flagged as recommendation, not fact)
Prioritized by trust impact:

1. **[P0] Harden `verify_witness_chain` to a real content check.** Recompute `hash_action` per row
   from the stored payload AND assert `prev_hash[i] == action_hash[i-1]` from the stored linkage,
   returning `Err` (not `.expect`/panic) on mismatch. This converts the load-bearing audit routine
   from a tautology into a genuine tamper detector and closes the count-only JSONL rebuild gap
   (C8 would then reject tampered payloads, not just count them). [closes D1-G1, D1-G2]
2. **[P1] Resolve cognitum: wire it or document it as dormant.** Either fire `hf policy gate` from
   `hooks.toml` at the claim/edit/handoff seams (making the "action governor" claim true), or
   explicitly mark it built-but-unwired in the ADR/CLAUDE.md so the doctrine stops over-stating it.
   [closes D5-C12]
3. **[P1] Close the unknown-verb fail-open.** Make the top-level `_ =>` arm `process::exit(2)` to
   match the nested-subverb behavior, so `hf bogus` cannot silently return success. [closes D2-3]
4. **[P2] Reconcile the drift checks and the command contract to the PRD — or amend the PRD.** Either
   build PRD §12.3 checks #1/#8/#10 (and `hf index`/`hf plan`, or remove them from the contract), or
   update the PRD to the shipped set; add `[workspace.lints]` with `unwrap_used = "deny"` (and fix
   the two `.unwrap()` sites) or drop that PRD mandate. Decide rusty-idd's fate: integrate per
   INTEGRATION-RUSTY-IDD.md or document `crates/*` as an orphaned/separate control plane.
   [closes D6-A1/A2, B1/B2, C1/C2, D1/D2]
5. **[P2] Decide the gatekeeper admin-bypass.** Either set `enforce_admins=true` (no `--admin`
   squash) to make "no human at the boundary" operationally true, or document the bypass as an
   intentional owner escape hatch — and deepen the impact scan from git-grep to the call graph.
   [addresses D5-C23 caveat]

---

### Most decision-relevant single finding
`verify_witness_chain` is a **count-only tautology, runtime-proven** to accept tampered and garbage
hashes (`verified == events.len()` regardless) — so the kernel's real defense against binary-cache
tampering is git history over the committed JSONL export, and the "tamper-evident" doctrine
overstates the runtime guarantee. Recommendation #1 fixes it.
