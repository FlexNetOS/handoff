# Verdicts — D3 · contract-proof-gate (adversarial verification)

**Date:** 2026-06-25
**Verifier method:** read every cited `path:line` against the real source; ran the contract
test suite as a live oracle. Default-skeptical, fail-closed.
**Target root:** `/home/drdave/Desktop/meta/handoff` · `[RVV]` =
`/home/drdave/Desktop/meta/RuVector/crates/ruvector-verified/src`

**Runtime oracle:** `cargo test -p hf contract` → **10 passed; 0 failed** (9 `contract::tests::*`
+ 1 hooks). Includes `drifted_intent_blocks_handoff`, `complete_task_without_checkpoint_is_unproven`,
`constraint_drift_on_full_lock_blocks_handoff`, `northstar_revision_drift_blocks_handoff`,
`full_lock_proves_five_intent_obligations`, `legacy_partial_lock_raises_no_extra_obligations`,
`attestation_is_deterministic`. The real `ruvector-verified` receipt asserts
`verifier_version == 0x0001_0000` (lean-agentic 0.1.0) — i.e. the test binds to the genuine crate.

---

## C1 — exit(1) BEFORE any packet/active.md write  → **CONFIRMED** (load-bearing)
Refutation attempt: find a disk write that precedes or races the proof gate. **Failed.**
Line-by-line `main.rs:2570-2633`:
- `2578` `let proof = active_task(...).map(|active| { ... })`. `Option::map` runs the closure
  **eagerly/synchronously** at this statement (not lazy like an iterator adapter). On
  `Err`, `eprintln!` + `std::process::exit(1)` (`2587-2588`) — `process::exit` cannot fall
  through.
- The only operations between `2571` and `2591` are **reads**: `load_tasks()`,
  `current_statuses()`, `status_of` (pure), `checkpoint_count()` (ledger read, `2553-2568`),
  and `current_northstar_revision()` → `capsule_field("northstar")` (capsule **read**, verified
  at `2251-2253`). No `fs::write`/`create_dir_all` in that span.
- First disk writes are `fs::create_dir_all` (`2607`), `fs::write(packet_path())` (`2608`),
  `fs::write(active.md)` (`2609`) — strictly after the closure returned.
Counter-example search (a write hidden in `render_packet_md`, `next_safe`, or the witness
verify at `2598-2602`) — those run after `2591` but BEFORE `2607`, and on a blocked proof the
process already exited at `2588`, so they never run. Ordering holds. **CONFIRMED.**

## C2 — proof goes through the REAL ruvector-verified crate, not a reimpl  → **CONFIRMED**
`hf/Cargo.toml:33`: `ruvector-verified = { path = "../../RuVector/crates/ruvector-verified",
default-features = false }` — genuine sibling path dep (mirrors `ledger`'s rvf-crypto; CI clones
the sibling). APIs exist as cited: `ProofEnvironment` struct `[RVV]/lib.rs:52`, `new()` `:80`
(pre-registers builtins via `register_builtin_symbols`), `alloc_term()` `:93`, `require_symbol()`
`:110-115` (returns `Err(DeclarationNotFound)` when absent), `EQ_REFL = "Eq.refl"`
`[RVV]/invariants.rs:11` (registered as a builtin decl `:50-54`). `contract.rs:29-30,111-114,123`
call exactly these. The passing tests link the real crate (verifier_version assertion). **CONFIRMED.**

## C3 — Eq.refl term minted ONLY on equality  → **CONFIRMED**
`discharge` (`contract.rs:102-115`): `if recorded != rederived { return Ok(None) }` — no term on
inequality; only on string-equality does it `require_symbol(EQ_REFL)` + `alloc_term()`. `prove_contract`
maps `Ok(None)` → `Err(ProofError::IntentDrift)` (`155-166`). Refutation attempt: can an unequal hash
yield a "proven" obligation? No path does. Live-proven by `drifted_intent_blocks_handoff` (PASS).
**CONFIRMED.**

## C4 — re-derivation reuses compute_intent_lock EXACTLY  → **CONFIRMED**
`prove_contract` re-derives via `WorkOrder::compute_intent_lock(&task.objective, &task.path_scope,
&task.acceptance_criteria)` (`contract.rs:128-132`) — the identical mint constructor
(`work-order/src/lib.rs:156-168`), single blake3 source `b3(s)="blake3:"+blake3::hash(s).to_hex()`
(`:119-121`). `constraint_hash()` (`:171-178`) and `full_intent_lock` (`:182-188`) confirm the 5-field
surface. No parallel/second hash exists. **CONFIRMED.**

## C5 — obligations: 3 base + 2 conditional (0047) + 1 conditional completion  → **CONFIRMED**
`contract.rs:135-242`: 3 base `intent:*` always raised; `intent:constraint`/`intent:northstar` skip
on `rec.is_empty()` (`197-198`); `completion` only when `status ∈ {Review,Done}` (`220`). Tests pin
exactly 3 / 5 / 4 obligations (`341-355`, `409-427`, `373-381`) and legacy-raises-no-extras
(`463-480`) — all PASS. The dimension brief's "4" is correctly flagged as an under-count of the
current surface (a doc nit, not a code defect). **CONFIRMED.**

## C6 — completion requires ≥1 witnessed checkpoint from the ledger  → **CONFIRMED**
`contract.rs:220-242`: completion flag `"1"` iff `evidence.checkpoints > 0`, else
`ProofError::UnprovenCompletion`. `checkpoint_count` (`main.rs:2553-2568`) opens the ledger and counts
`task_transition` events whose payload `status` deserializes to `Status::Checkpointed`. Refutation:
can a Done-but-never-checkpointed task hand off? No — `complete_task_without_checkpoint_is_unproven`
(PASS) proves the block. **CONFIRMED.**

## C7 — all three ProofError variants fail the handoff closed  → **CONFIRMED**
`ProofError` = `IntentDrift | UnprovenCompletion | EnvironmentBroken` (`contract.rs:39-48`); every
construction site is an `Err` return; the sole caller `cmd_handoff` maps any `Err` → `exit(1)`
(`main.rs:2586-2589`). `EnvironmentBroken` fires iff `require_symbol(EQ_REFL)` errors
(`contract.rs:111,167-171`) — fail-closed on a tampered env. Note (not a refutation): since
`ProofEnvironment::new()` always registers `Eq.refl`, `EnvironmentBroken` is effectively
unreachable on the happy path — but the *structure* is fail-closed, which is the claim. **CONFIRMED.**

## C8 — attestation tamper-evident + bound to THIS contract's hashes  → **CONFIRMED**
`create_attestation` (`[RVV]/proof_store.rs:127-159`) hashes real proof state (proof_id,
terms_allocated, all stats) + all symbol names via siphash-256 — header comment: "not placeholder
values (SEC-002 fix)". `content_hash` (`contract.rs:257-272`) additionally binds task id + obligation
names/terms + all 5 recorded lock hashes, closing the gap the analyst describes (`93-95`).
`attestation_is_deterministic` (PASS) confirms determinism. **CONFIRMED.**

## C9 — proof receipt witnessed into the rendered packet  → **CONFIRMED**
`render_proof_section(p)` is `push_str`'d into `md` at `main.rs:2603-2604` (after the gate passed —
a blocked proof already exited), and `md` is written to `packet_path()` at `2608`. Section content
(`contract.rs:275-297`) emits obligations, proof-term count, proof-hash, content binding, verifier
version. Durable artifact in the committed packet. **CONFIRMED.**

---

## Gap claims (caveats) — all accurate
- **G1 (no active task ⇒ packet written with NO proof)** → **CONFIRMED.** `active_task` returns
  `None` outside `{Claimed,Checkpointed,Active,Review}` (`main.rs:2542-2549`); `None.map(...)` = `None`;
  `if let Some(p) = &proof` (`2603`, `2624`) is skipped; the unconditional writes at `2607-2618` still
  run. So `hf handoff` writes a packet/active.md with no contract proof when nothing is in flight.
  This is an honest, correctly-scoped caveat (nothing to prove), NOT a fail-open of the gate — the
  gate is only claimed to fire "when work is in flight," which holds.
- **G2 (completion only Review|Done)** → **CONFIRMED** (`contract.rs:220`). Mid-work handoff proves
  only intent obligations; the finding does not over-claim "proves the task is done."
- **G3 (runtime not run by analyst)** → **UPGRADED.** I ran the suite: 10/10 PASS, so the
  fail-closed behavior is now backed by a live oracle, not only static reading. A full end-to-end
  `hf handoff` drive against an on-disk drifted card was not performed, but the unit tests exercise
  `prove_contract`'s drift/no-checkpoint blocks directly.
- **G4 (northstar faithfulness depends on capsule integrity)** → **NOTED, out of D3 scope.** Accurate
  cross-dimension hook; not a material D3 claim.

---

## Tally
- CONFIRMED: 9 material claims (C1–C9) + 3 accurate caveats (G1, G2, G4-note); G3 upgraded with runtime evidence
- QUALIFIED: 0
- REFUTED: 0
- INCONCLUSIVE: 0

Every cited line was checked and supports its claim; no counter-example found. Only CONFIRMED
claims flow to synthesis (all of D3 qualifies).
