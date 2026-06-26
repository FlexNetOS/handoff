# Findings — D3 · contract-proof-gate

**Dimension question:** Does `hf handoff` really fail closed on an unprovable contract? Trace
`prove_contract` → `ruvector-verified` `Eq.refl` proof + `ProofAttestation`; the obligations; the
re-derivation faithfulness (reuses `compute_intent_lock`); the exit-before-write wiring.

**Verdict (1 line):** YES — `hf handoff` proves the active task's intent-lock contract through the
real `ruvector-verified` crate and `std::process::exit(1)`s *before* any packet/`active.md` write on
any `ProofError`; the gate is genuinely fail-closed for an active task, with two honest scope caveats
(no active task ⇒ no proof; completion obligation only when status Review/Done).

Citations are `path:line` against target root `/home/drdave/Desktop/meta/handoff` unless noted
`[RVV]` = `/home/drdave/Desktop/meta/RuVector/crates/ruvector-verified/src`.

---

## Claims

### C1 — The gate exits(1) BEFORE any packet/active.md write (the load-bearing claim)
**Statement:** In `cmd_handoff`, the contract proof is discharged eagerly via
`active_task(...).map(|active| { ... prove_contract ... })` at `hf/src/main.rs:2578-2591`; on `Err`
it prints `BLOCKED (fail-closed, ADR-0011)` and calls `std::process::exit(1)` at
`hf/src/main.rs:2587-2588`. The first filesystem writes (`fs::create_dir_all`, `fs::write(packet_path())`,
`fs::write(active.md)`) do not occur until `hf/src/main.rs:2607-2618` — strictly after the closure
returns. `.map` on `Option` evaluates the closure synchronously at that statement, so a blocked proof
terminates the process before any write. Nothing between line 2571 and 2591 writes to disk (only
`load_tasks`, `current_statuses`, and ledger reads).
**Evidence:** `hf/src/main.rs:2570-2591` (gate), `:2607-2618` (writes).
**Confidence:** High. Static control flow is unambiguous; `process::exit` cannot fall through.

### C2 — The proof is discharged through the REAL ruvector-verified crate, not a reimplementation
**Statement:** `prove_contract` constructs `ruvector_verified::ProofEnvironment::new()`
(`contract.rs:123`), and each obligation is discharged by `discharge()` which calls
`env.require_symbol(EQ_REFL)` then `env.alloc_term()` (`contract.rs:111-114`). These are the genuine
crate APIs: `ProofEnvironment` is defined at `[RVV]/lib.rs:52`, `new()` at `[RVV]/lib.rs:80`,
`alloc_term` at `[RVV]/lib.rs:93`, `require_symbol` at `[RVV]/lib.rs:110`, and `EQ_REFL = "Eq.refl"`
at `[RVV]/invariants.rs:11`. The dep is a path dep to the real sibling crate:
`hf/Cargo.toml:33` `ruvector-verified = { path = "../../RuVector/crates/ruvector-verified", ... }`.
**Evidence:** `hf/src/contract.rs:29-30,111-114,123`; `[RVV]/lib.rs:52,80,93,110`; `[RVV]/invariants.rs:11`; `hf/Cargo.toml:33`.
**Confidence:** High.

### C3 — The proof term is a real Eq.refl reflexive-equality term, minted only on equality
**Statement:** `discharge(env, recorded, rederived)` returns `Ok(None)` when `recorded != rederived`
(`contract.rs:107-109`) — no proof term is minted on inequality. Only on string equality does it
require the `Eq.refl` symbol and allocate a term (`contract.rs:110-114`). `prove_contract` turns an
`Ok(None)` into a fail-closed `ProofError::IntentDrift` (`contract.rs:161-166`), so an unequal hash
can never yield a "proven" obligation. This mirrors `ruvector_verified::prove_dim_eq`'s discipline but
over a full-string compare (sounder than a `u32` dimension reduction) per the module doc
`contract.rs:10-12`.
**Evidence:** `hf/src/contract.rs:102-115,155-166`.
**Confidence:** High.

### C4 — Re-derivation reuses compute_intent_lock EXACTLY (faithful, not a parallel hash)
**Statement:** `prove_contract` re-derives the lock with the identical constructor the kernel mints
cards with: `WorkOrder::compute_intent_lock(&task.objective, &task.path_scope,
&task.acceptance_criteria)` (`contract.rs:128-132`), then full-string-compares each field against the
recorded `task.intent_lock` (`contract.rs:133-154`). `compute_intent_lock` is the single blake3 source:
`objective_hash = b3(objective)`, `path_scope_hash = b3(path_scope.join("\n"))`,
`acceptance_hash = b3(acceptance.join("\n"))` (`work-order/src/lib.rs:156-168`), where
`b3(s) = "blake3:" + blake3::hash(s).to_hex()` (`work-order/src/lib.rs:119-121`). Because the proof
re-runs the exact mint function, the proof is faithful to the live contract surface — a parallel/second
hash is impossible.
**Evidence:** `hf/src/contract.rs:127-154`; `work-order/src/lib.rs:119-121,156-168`.
**Confidence:** High.

### C5 — Obligations: 3 base intent + 2 conditional (0047) + 1 conditional completion
**Statement:** The dimension brief says "4 obligations"; the code now raises **up to 6**, conditionally:
(1) `intent:objective`, (2) `intent:path_scope`, (3) `intent:acceptance` — always raised
(`contract.rs:135-173`); (4) `intent:constraint`, (5) `intent:northstar` — raised ONLY when the
recorded lock actually carries that surface (`rec.is_empty()` ⇒ `continue`, `contract.rs:196-217`);
(6) `completion` — raised ONLY when replayed status is `Review`/`Done` (`contract.rs:219-242`). The
"4" in the brief = the legacy 3-field lock (constraint/northstar empty) + completion. The 5-field form
(HFTASK-0047) adds constraint+northstar. So a mid-work checkpointed legacy card proves exactly 3; a
full-lock mid-work card proves 5; a Done full-lock card proves 6.
**Evidence:** `hf/src/contract.rs:135-242`; tests at `:341-355` (3), `:409-427` (5), `:373-381` (4),
`:463-480` (legacy raises no extras).
**Confidence:** High. (Minor: brief's "4" is an under-count of the current surface — flagged, not a defect.)

### C6 — Completion obligation requires ≥1 witnessed checkpoint, read from the ledger
**Statement:** When status is `Review|Done`, completion holds iff `evidence.checkpoints > 0`
(`contract.rs:220-242`); otherwise `ProofError::UnprovenCompletion` blocks. The checkpoint count is
real ledger evidence: `checkpoint_count(id)` opens the ledger and counts `task_transition` events whose
payload status deserializes to `Status::Checkpointed` (`hf/src/main.rs:2553-2568`). So "done but never
checkpointed" cannot hand off — confirmed by test `complete_task_without_checkpoint_is_unproven`
(`contract.rs:384-392`).
**Evidence:** `hf/src/contract.rs:219-242`; `hf/src/main.rs:2553-2568`.
**Confidence:** High.

### C7 — The three ProofError variants each fail the handoff closed
**Statement:** `ProofError` has exactly three variants — `IntentDrift`, `UnprovenCompletion`,
`EnvironmentBroken` (`contract.rs:39-48`). All three are returned as `Err` from `prove_contract`, and
the single caller (`cmd_handoff`) maps any `Err` to `exit(1)` (`main.rs:2586-2589`). Notably
`EnvironmentBroken` fires if `require_symbol(EQ_REFL)` fails (`contract.rs:111,167-171`) — i.e. a
malformed/tampered proof environment fails closed rather than silently passing. `require_symbol`
genuinely returns `Err(DeclarationNotFound)` when the symbol is absent (`[RVV]/lib.rs:110-115`), and
`ProofEnvironment::new()` pre-registers builtins incl. `Eq.refl` (`[RVV]/lib.rs:80-90`,
`invariants.rs:51`), so the happy path resolves and the broken path errors.
**Evidence:** `hf/src/contract.rs:39-48,111,167-171`; `hf/src/main.rs:2586-2589`; `[RVV]/lib.rs:110-115`.
**Confidence:** High.

### C8 — The attestation is tamper-evident and bound to THIS contract's hashes
**Statement:** `prove_contract` mints `ruvector_verified::proof_store::create_attestation(&env,
last_proof_id)` (`contract.rs:244`). `create_attestation` hashes actual proof + environment state
(proof_id, terms_allocated, stats; all symbol names) via siphash-256 — explicitly "not placeholder
values" (`[RVV]/proof_store.rs:123-159`). Separately, `content_hash()` binds the receipt to task id +
obligation names + all five recorded lock hashes (`contract.rs:255-272`), closing the gap that the
attestation's own hashes cover proof/env state but not the specific recorded contract hashes
(`contract.rs:93-95`). Determinism is tested: `attestation_is_deterministic` (`contract.rs:394-407`).
**Evidence:** `hf/src/contract.rs:244-272`; `[RVV]/proof_store.rs:22-30,123-159`.
**Confidence:** High.

### C9 — The proof receipt is witnessed into the rendered packet
**Statement:** On success, `render_proof_section(p)` is appended to the packet markdown
(`main.rs:2603-2605`), emitting the obligation list, proof-term count, proof-hash, content binding, and
verifier version (`contract.rs:275-297`). So the proof is a durable artifact in the committed packet,
not an ephemeral check. (This write only happens after the gate passed — consistent with C1.)
**Evidence:** `hf/src/main.rs:2602-2605,2624-2632`; `hf/src/contract.rs:275-297`.
**Confidence:** High.

---

## Gaps / honest scope caveats

- **G1 (scope, not a defect):** If there is **no active task** (`active_task` returns `None`,
  `main.rs:2542-2549`), `proof` is `None` and `cmd_handoff` writes the packet/active.md with NO
  contract proof (`main.rs:2578,2603,2624`). The gate only fires when a task is in
  `Claimed|Checkpointed|Active|Review`. This is by design (nothing to prove) but means "handoff" is not
  universally proof-gated — only proof-gated *when work is in flight*. Confidence: High.
- **G2 (scope):** The completion obligation is conditional on `Review|Done` status
  (`contract.rs:220`). A task handed off mid-work (`Active`/`Claimed`/`Checkpointed`) proves only the
  intent obligations — completion is intentionally not asserted. Correct, but worth stating so the gate
  isn't over-claimed as "proves the task is done." Confidence: High.
- **G3 (verification not run):** All claims are from static source reading. I did NOT execute
  `hf handoff` against a drifted card to observe the exit(1) at runtime. A verifier could confirm by:
  (a) the existing unit tests (`drifted_intent_blocks_handoff` etc. `contract.rs:357-371`) already prove
  `prove_contract` returns `Err` on drift; (b) a live drive — mutate a Claimed card's objective on
  disk without re-locking, run `hf handoff`, assert exit code 1 and that `packets/latest.md` mtime is
  unchanged. Confidence on the *static* claim: High; runtime observation: not captured.
- **G4 (cross-dimension hook):** `current_northstar_revision()` reads the capsule `northstar` field
  (`main.rs:2251-2253`); the northstar obligation's faithfulness depends on capsule integrity (ADR-0006
  packet-renders-from-capsule). Out of D3 scope but relevant to a capsule/identity dimension.

## Cross-dimension hooks for synthesizer
- The gate is reached on the **session-end lifecycle path** (`session-end.sh` runs `hf handoff`),
  so the contract proof is on the autonomous loop's critical path — ties to the agent/loop dimension.
- `compute_intent_lock` is shared by the **drift gate** (`hf drift`, `work-order/src/lib.rs:194-220`)
  and this proof gate — same blake3 surface, two enforcement points (drift = warn/scan,
  handoff-proof = hard fail-closed). Ties to any "drift detection" dimension.
