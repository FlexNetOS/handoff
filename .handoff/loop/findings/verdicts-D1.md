# Verdicts — D1 · ledger-witness-rvf-substrate

Verifier pass: 2026-06-25. Method: read both sides of every cited call; live `cargo tree`,
`cargo test -p ledger` (32 passed), grep for non-test `prev_hash` reads, and a RUNTIME tautology
demonstration driving the real `rvf-crypto` `create_witness_chain`→`verify_witness_chain` exactly as
`Ledger::verify_witness_chain` does. Target: `/home/drdave/Desktop/meta/handoff`.

Tally: CONFIRMED 12 · REFUTED 0 · QUALIFIED 0 · INCONCLUSIVE 0.
(Note: G1/G2/G3 are *gap* claims — "CONFIRMED" means the weakness is real as stated.)

---

## C1 — append-only write-time hash chain — CONFIRMED
Read `append` v1.rs:482-523: `action_hash = hash_action(...)` (489), `read_tail` *inside* the tx
(496), `next_seq = tail_seq+1` (497), `EventBody{action_hash, prev_hash,...}` (498-509), only
`events.insert` (509) — no delete/update verb on EVENTS exists. `hash_action` = SHA3-256 over
`event_type‖work_order_id‖payload` (291-297). prev_hash = previous row's action_hash (read_tail
returns `body.action_hash`, 443). Refutation (find a mutate/delete path, or a prev_hash that isn't
the prior action_hash) failed.

## C2 — ACID serializable single-writer (redb begin_write + OS lock) — CONFIRMED
Append/lease/rollup each in one `begin_write()` (491/549/743). Cross-process exclusion = `is_busy`
on `DatabaseError::DatabaseAlreadyOpen` (343-348). Module doc 391-399 states the BEGIN-IMMEDIATE
analogue. `cargo test -p ledger` green (32 passed) incl. the concurrency/cross-process tests.

## C3 — read-tail-in-tx invariant — CONFIRMED
`read_tail` (438-446) returns `(last seq, body.action_hash)`. append(496)/lease(575)/rollup(753)
all call it inside the open write tx. `self.prev_witness_hash` is set post-commit (521,597) and
is **never** consulted for chaining (grep: no read in any chain-link path) — the "dead state for
chaining" note is accurate (cosmetic, not a correctness defect).

## C4 — atomic in-ledger lease CAS — CONFIRMED
`try_acquire_lease` 538-604: one `begin_write()` (549); resolve holder in-tx (550-552); foreign
live holder → `drop(tx); return Conflict` with no commit (556-560); else chained `lease_acquired`
(564-595). `resolve_lease` honors release-by-holder + TTL (313-333). Refutation (a path that
writes before the conflict check, or commits on conflict) failed.

## C5 — rollup provenance bridge — CONFIRMED
`verify_rollup_provenance` 853-892 **recomputes** `hash_action(event_type, work_order_id, payload)`
(882) and **byte-compares** to stored `origin_action_hash` (883); `is_faithful = mismatched==0`
(281-283). `rollup_from` re-chains onto the central tail (753, 783) under two-phase commit (744),
idempotent via ORIGIN_INDEX. NOTE (contrast with G1): this is a REAL content re-derivation — rolled
rows ARE tamper-checkable; native local rows are not (see G1).

## C6 — v2 delegates all authoritative ops; recall fail-open — CONFIRMED (minor doc drift)
`append` v2.rs:304-338 calls `self.v1.append(...)` then `let _ = self.store.ingest_batch(...)` —
RVF error discarded by design. `all_events`/`events_after`/`verify_witness_chain`/
`verify_rollup_provenance`/`rollup_from` all delegate to v1 (377-393). Doc at v2.rs:302 still says
"SQLite append is authoritative" — stale (it is redb now); cosmetic, does not affect the claim.

## C7 — no-C default graph — CONFIRMED (live)
`cargo tree -p ledger` → grep `rusqlite|libsqlite|-sys` = NONE. `--features legacy-sqlite` →
`rusqlite v0.31.0 → libsqlite3-sys v0.28.0`. Matches Cargo.toml: `default=["v2"]`,
`v2=["redb-store",rvf-*]`, `legacy-sqlite=["redb-store","dep:rusqlite"]` (only feature naming
rusqlite, `optional=true`). redb pure-Rust (Cargo.toml:14).

## C8 — JSONL export = committed truth; rebuild re-derives, fail-closed on count — CONFIRMED
export.rs:43-60 (seq-ordered), 66-91 (rebuild re-appends via authoritative `append` 76-81, so
action_hash is recomputed from payload), count gate 84-89, "recomputes and re-verifies it, never
trusts" the hash (10-13/26-28). The claim is exactly as written. IMPORTANT (ties to G1): the gate
is **count-only** — because `verify_witness_chain` is a tautology, `verified` always equals the
appended count, so a tampered JSONL payload yields a different-but-still-"valid" chain that rebuild
ACCEPTS; detection there rests on git diff/review, not the witness routine.

## C9 — legacy-SQLite fail-closed guard — CONFIRMED
`file_is_legacy_sqlite` 379-389 checks `"SQLite format 3\0"`; `open` guards at 405-407 → returns
`LegacySqlite` (actionable, points at `hf migrate`) before any `Database::create`. Verified the
wiring directly.

---

## G1 — `verify_witness_chain` is a tautology; cannot detect binary-cache tampering — CONFIRMED (RUNTIME-PROVEN)
This is the load-bearing finding and it is **proven concretely**, not just by reading.

Mechanism (read both sides):
- `Ledger::verify_witness_chain` v1.rs:829-846 builds `WitnessEntry`s with `prev_hash:[0u8;32]`
  (837, stored linkage discarded) and `action_hash: body.action_hash` (838, **stored value, never
  recomputed from payload via `hash_action`**), then `create_witness_chain` → `verify_witness_chain`.
- `rvf-crypto` `create_witness_chain` (witness.rs:66-79) OVERWRITES `linked.prev_hash = prev_hash`
  (running shake), ignoring the input prev_hash, and emits a chain that is internally consistent BY
  CONSTRUCTION. `verify_witness_chain` (85-111) only checks that exact internal consistency. So
  verify ALWAYS returns `Ok(len)` for any well-formed entry vector ⇒ the method reduces to
  `Ok(events.len())`.
- Stored `EventBody.prev_hash` is read ONLY in tests (grep: v1.rs:1146, 1647) — zero production
  reads. `read_tail` returns `action_hash`, not `prev_hash` (443), so even live chaining never
  re-checks stored linkage.

Runtime tamper test (drove real `rvf-crypto` exactly as the routine does, 3 entries):
```
A honest, prev=zeros           -> verified=3
B tampered action_hash[1]      -> verified=3   (coordinated payload+hash edit undetected)
C garbage action_hashes        -> verified=3   (no binding to any payload)
D honest hashes, BOGUS prev    -> verified=3   (stored prev_hash ignored)
```
Every scenario returns the event count. The routine cannot distinguish honest from
tampered/garbage action_hashes and ignores stored prev_hash. The analyst's predicted outcome
("still returns N, no error") is exactly reproduced. **Tamper-evidence at verification time is
materially weaker than the doctrine asserts**: the real integrity controls are (i) write-time
chaining (correct but never re-verified) and (ii) git history over the JSONL export — NOT
`verify_witness_chain`. (`verify_rollup_provenance`, C5, IS a real content check — but only for
rolled rows, not native events.)

## G2 — `.expect("witness chain must verify")` is a panic, not fail-closed — CONFIRMED
Literal `.expect(...)` at v1.rs:844. It is a process-abort path rather than a returned `Err`.
Unreachable for `create_witness_chain` output (per G1) ⇒ low operational impact, but the claim is
literally true.

## G3 — busy-retry asymmetry (informational) — CONFIRMED
append/lease/cursor wrap in `with_busy_retry` (361-374; 464/490/548). `rollup_from` deliberately
does NOT — the in-source comment (740-742) confirms: single all-or-nothing two-phase tx, contention
blocks rather than retries. Correct by design; not a defect.

---

## Synthesis-facing notes
- Flows to synthesis (CONFIRMED, usable as fact): C1–C9 + the gap findings G1–G3.
- The headline qualifier the synthesizer MUST carry: the ledger is append-only + hash-chained +
  ACID + no-C **at write time**, but its runtime verification surface (`verify_witness_chain`, what
  `hf doctor`/`hf import` would lean on) is a tautology and does NOT detect content tampering of the
  binary redb cache. Doctrine wording ("tamper-evident", v1.rs:827 "tamper-evidence") OVERSTATES the
  verification-time guarantee — a doc-vs-code overclaim, exactly the kind of finding to surface.
- G1/G2 are clean candidate kernel tasks: a true `verify_witness_chain` would recompute
  `hash_action` per row from payload AND re-check stored `prev_hash[i] == action_hash[i-1]`, and
  return `Err` instead of `.expect`.
