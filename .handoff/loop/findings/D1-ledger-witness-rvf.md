# D1 · ledger-witness-rvf-substrate — findings

Dimension question: Is the ledger genuinely append-only, hash-chained, tamper-evident, and ACID?
Verify redb-tx-per-append + read-tail-in-tx; witness chain; atomic lease CAS; rollup provenance;
v2 RVF recall overlay; the no-C trust boundary.

Verdict (1 line): The ledger is genuinely append-only, hash-chained at WRITE time, ACID
(redb serializable single-writer), with correct atomic lease CAS and rollup provenance, and a
verified no-C default graph — **but the runtime `verify_witness_chain` is a tautology that does
not actually validate stored chain linkage or payload→hash binding**, so tamper-evidence at
verification time is materially weaker than the doctrine claims.

Evidence root: `/home/drdave/Desktop/meta/handoff` (target). `cargo tree` run live in target.

---

## Claims

### C1 — Append is append-only with a write-time hash chain (prev_hash = previous action_hash)
- **Statement:** Each event is inserted at `tail_seq + 1` (monotonic, no update/delete path) and
  stores `prev_hash` = the previous row's `action_hash`, forming a genuine hash chain on disk.
- **Evidence:** `ledger/src/v1.rs:482-523` (`append`): computes `action_hash =
  hash_action(...)` (`v1.rs:489`), re-reads `(tail_seq, prev_hash)` via `read_tail` *inside* the
  tx (`v1.rs:496`), sets `next_seq = tail_seq + 1` (`v1.rs:497`), writes `EventBody{ action_hash,
  prev_hash, ... }` (`v1.rs:498-509`). `hash_action` = SHA3-256 over
  `event_type‖work_order_id‖payload` (`v1.rs:291-297`). Only `insert` is ever used; there is no
  delete/update verb on `EVENTS`.
- **Confidence:** high.

### C2 — ACID, serializable, single-writer (redb begin_write + OS file lock)
- **Statement:** Each append/lease/rollup is ONE `begin_write()` transaction; redb gives ACID
  single-writer serializability in-process and the OS file lock excludes a second process.
- **Evidence:** `ledger/src/v1.rs:1-14` (module doc: "redb's `begin_write()` is a serializable
  single-writer critical section — the exact analogue of SQLite's `BEGIN IMMEDIATE`"); append
  tx `v1.rs:491-516`; cross-process exclusion surfaces as `DatabaseError::DatabaseAlreadyOpen`
  (`v1.rs:337-348`, `is_busy`). Test `c2_cross_process_exclusion` asserted at `v1.rs:1156-1159`.
- **Confidence:** high.

### C3 — read-tail-in-tx invariant holds (no fork off a stale open-time cache)
- **Statement:** The seq + prev_hash used for chaining are re-read from the DB tail INSIDE the
  write tx on every append/lease/rollup, never the value cached at `open()`, so two concurrent
  writers cannot both chain off the same prev_hash.
- **Evidence:** `read_tail` returns `(seq, body.action_hash)` of `events.last()` (`v1.rs:438-446`).
  append (`v1.rs:496`), lease (`v1.rs:575`), rollup (`v1.rs:753`) each call `read_tail` inside the
  open write tx. The struct's cached `self.prev_witness_hash` (`v1.rs:234`) is updated post-commit
  (`v1.rs:521,597,801`) but is **never consulted** for the next chain link — chaining always uses
  the in-tx `read_tail`. Concurrency test asserts contiguous 1..=N seqs and a contiguous stored
  prev_hash chain after many parallel writers (`v1.rs:1123-1150`).
- **Confidence:** high. (Minor note: `self.prev_witness_hash` is effectively dead state for
  chaining — cosmetic, not a correctness issue.)

### C4 — Atomic in-ledger lease CAS (check-then-write in one tx; conflict = no write)
- **Statement:** `try_acquire_lease` resolves the current holder and conditionally appends
  `lease_acquired` inside a SINGLE `begin_write()`; a live foreign holder returns
  `Conflict` with no write (tx dropped uncommitted). This is real mutual exclusion independent of
  weave.
- **Evidence:** `ledger/src/v1.rs:538-604`: opens write tx (`v1.rs:549`), resolves holder in-tx
  via `lease_events_in_tx` + pure `resolve_lease` (`v1.rs:550-552`), on foreign live holder
  `drop(tx); return Conflict` (`v1.rs:556-560`), else appends chained `lease_acquired`
  (`v1.rs:564-595`). `resolve_lease` honors release-by-holder and TTL expiry (`v1.rs:313-333`).
  Tests: `try_acquire_lease_is_atomic_first_holder_wins` (`v1.rs:946-960`),
  `resolve_lease_free_held_released_expired` (`v1.rs:917-944`).
- **Confidence:** high.

### C5 — Rollup provenance bridge is real and verifiable (CT/RFC6962-style re-chaining)
- **Statement:** `rollup_from` re-appends each source row onto the central tail (fresh seq,
  re-chained prev_hash), recomputes `action_hash` from the same inputs (byte-identical to source),
  stores it as both `action_hash` and `origin_action_hash`, and is idempotent via `ORIGIN_INDEX`.
  `verify_rollup_provenance` recomputes the hash and byte-compares to `origin_action_hash`;
  `mismatched == 0` is the faithfulness gate.
- **Evidence:** `ledger/src/v1.rs:730-803` (`rollup_from`; two-phase commit on, `v1.rs:744`;
  idempotency skip on pre-existing `ORIGIN_INDEX` key, `v1.rs:759-762`; cursor advance in same tx,
  `v1.rs:789-793`). `verify_rollup_provenance` `v1.rs:853-892` (recompute + byte-compare
  `v1.rs:882-883`). `RollupProvenance::is_faithful` = `mismatched == 0` (`v1.rs:281-283`).
- **Confidence:** high.

### C6 — v2 RVF overlay delegates ALL authoritative ops to v1; recall is best-effort (fail-open, recall-only)
- **Statement:** The default `v2` feature layers `rvf-runtime::RvfStore` for HNSW semantic recall
  but every authoritative method (append/lease/rollup/replay/verify) delegates to v1; the RVF
  vector ingest is best-effort and its error is discarded.
- **Evidence:** `ledger/src/v2.rs:26-30` (`Ledger { v1, store, dim }`); `append` calls
  `self.v1.append(...)` then `let _ = self.store.ingest_batch(...)` (`v2.rs:311-337`) — RVF
  failure is swallowed by design (`v2.rs:301-303` doc); delegations `all_events`/`events_after`/
  `verify_witness_chain`/`verify_rollup_provenance`/`rollup_from` (`v2.rs:377-393`).
  `query_by_intent` is pure read over RVF + v1 (`v2.rs:342-371`). Embeddings are deterministic
  hash-based pseudo-embeddings — no model/network (`v2.rs:42-56`).
- **Confidence:** high. (The fail-open ingest is acceptable: it affects only recall, never the
  authoritative event record. Doc string at `v2.rs:302` still says "SQLite append is
  authoritative" — stale; it is redb now. Minor doc drift.)

### C7 — No-C trust boundary: the default graph links zero rusqlite/-sys
- **Statement:** The default ledger build pulls no C (no `rusqlite`, no `libsqlite3-sys`, no
  `-sys`); the only C-pulling path is the opt-in, never-default `legacy-sqlite` migration feature.
- **Evidence (live `cargo tree`):** `cargo tree -p ledger` → grep `rusqlite|libsqlite|-sys`
  returns **NONE**. `cargo tree -p ledger --features legacy-sqlite` → `rusqlite v0.31.0 →
  libsqlite3-sys v0.28.0`. Matches `ledger/Cargo.toml:28-37`: `default = ["v2"]`,
  `v2 = ["redb-store", rvf-*]`, `legacy-sqlite = ["redb-store", "dep:rusqlite"]` (the only feature
  naming `rusqlite`, gated `optional = true`). redb is pure-Rust (`Cargo.toml:13-14`).
- **Confidence:** high.

### C8 — Deterministic JSONL export is the committed truth; rebuild re-derives via append and fails closed on count mismatch
- **Statement:** `export_jsonl` emits seq-ordered JSON (one object/event); `rebuild_from_jsonl`
  re-appends every line through the authoritative `Ledger::append` (recomputing action_hash from
  payload) and aborts if the rebuilt chain's verified count != imported count. The exported
  `action_hash` hex is audit-only and never trusted on import.
- **Evidence:** `ledger/src/export.rs:43-60` (export), `:66-91` (rebuild; re-append `:76-81`;
  count gate `:84-89`), `:10-13` (import "recomputes and re-verifies it, never trusts" the hash).
  Tests `jsonl_round_trips_witness_faithfully` and `corrupt_line_fails_closed`
  (`export.rs:104-153`).
- **Confidence:** high.

### C9 — Legacy-SQLite fail-closed guard (refuses to treat a SQLite file as an empty redb store)
- **Statement:** `Ledger::open` magic-byte-detects a pre-port C-SQLite `ledger.db` and returns an
  actionable `LegacySqlite` error pointing at `hf migrate`, instead of silently creating/empty.
- **Evidence:** `file_is_legacy_sqlite` (`v1.rs:379-389`, checks `"SQLite format 3\0"`), guard in
  `open` (`v1.rs:405-407`), error text (`v1.rs:75-81`).
- **Confidence:** high.

---

## GAPS / WEAKNESSES (the load-bearing finding)

### G1 — `Ledger::verify_witness_chain` does NOT validate the on-disk chain; it is effectively a tautology
- **Statement:** The runtime tamper-evidence routine does NOT verify (a) the stored
  `EventBody.prev_hash` linkage, nor (b) that `action_hash == hash_action(payload)`. It builds
  fresh `WitnessEntry`s with `prev_hash: [0u8;32]`, lets `create_witness_chain` RECOMPUTE the
  links from the (trusted) stored `action_hash` values, then `verify_witness_chain` checks *that
  freshly-built* chain — which, for any well-formed set of rows, always passes. So the method
  reduces to `Ok(events.len())` and detects neither a coordinated edit (payload + matching
  action_hash) NOR even a payload-only edit (because action_hash is never recomputed from payload).
- **Evidence:**
  - `ledger/src/v1.rs:829-846` (`verify_witness_chain`): pushes entries with `prev_hash: [0u8;32]`
    (`v1.rs:837`) and `action_hash: body.action_hash` (stored, not recomputed, `v1.rs:838`), then
    `create_witness_chain(&entries)` (`v1.rs:843`) and `verify_witness_chain(&chain)` (`v1.rs:844`).
  - `rvf-crypto` `create_witness_chain` overwrites `linked.prev_hash = prev_hash` and recomputes
    each link (`RuVector/crates/rvf/rvf-crypto/src/witness.rs:66-79`); `verify_witness_chain`
    only checks the chain it was just handed is internally consistent (`witness.rs:85-111`). Since
    create always emits a consistent chain, verify always returns `Ok` for well-formed input.
  - The stored `EventBody.prev_hash` is **only ever read in tests** (`v1.rs:1144-1150`,
    `v1.rs:1647`), never by any production verify path (grep: zero non-test `.prev_hash` reads).
  - `read_tail` returns `body.action_hash`, not `prev_hash` (`v1.rs:438-446`) — so even the live
    chaining input is the action_hash, and the stored prev_hash linkage is never re-checked.
- **Why it matters:** The doctrine ("witnessed, hash-chained, tamper-evident") is true at WRITE
  time, but the *verification surface* an auditor/`hf doctor`/`hf import` would call cannot catch
  tampering of the binary redb cache. The genuine integrity control is therefore (i) the
  write-time chaining (correct, but unverified after the fact) and (ii) git history over the JSONL
  export — NOT `verify_witness_chain`.
- **Caveat (steelman):** The binary `ledger.db` is a gitignored, rebuildable cache; committed
  truth is the JSONL, and `rebuild_from_jsonl` re-appends through `append` so the rebuilt chain's
  action_hashes are derived from the committed payloads. But rebuild only gates on *count*
  (C8) — a tampered JSONL payload yields a different yet still-"valid" chain that rebuild accepts;
  detection there relies on git diff/review, not the witness routine.
- **Confidence:** high that the routine does not validate stored linkage or payload→hash binding
  (read directly from both sides of the call). medium on operational impact severity (depends on
  whether any caller is expected to treat `verify_witness_chain` as tamper-detection — the doc
  comment at `v1.rs:827` claims "tamper-evidence," which overstates what the code does).
- **For the verifier to run:** open a ledger, append N events, then directly mutate one
  `EventBody.payload_json` (or both payload + action_hash) in the redb store and call
  `verify_witness_chain()` — predict it still returns N (no error). A true tamper test would
  recompute `hash_action` per row and re-check stored `prev_hash[i] == action_hash[i-1]`.

### G2 — `.expect("witness chain must verify")` is a panic, not fail-closed
- **Statement:** `verify_witness_chain` unwraps the inner verify with `.expect(...)`
  (`v1.rs:844`). For well-formed input it is unreachable (per G1), but it is a panic path rather
  than a returned `Err`, i.e. a process abort instead of a graceful fail-closed error.
- **Evidence:** `ledger/src/v1.rs:844`.
- **Confidence:** high (it is a literal `.expect`). Low operational impact given G1.

### G3 — Busy-retry asymmetry (informational, not a defect)
- **Statement:** append/lease/cursor wrap writes in `with_busy_retry` (re-opens + re-reads tail,
  so a retry cannot fork — `v1.rs:350-374`), but `rollup_from` deliberately does NOT
  (`v1.rs:740-743`): the whole batch is one all-or-nothing tx and contention simply blocks. This
  is correct by design but means a rollup under sustained cross-process contention surfaces the
  error rather than retrying.
- **Evidence:** `ledger/src/v1.rs:740-743`, `:361-374`.
- **Confidence:** high.

---

## Cross-dimension hooks (for the synthesizer)
- The contract-proof gate (`hf/src/contract.rs`) is the OTHER integrity surface (intent-lock
  blake3 + `ruvector-verified`) and is what actually fail-closes `hf handoff` — pairs with this
  dimension to answer "is completion provable?" (D-contract).
- Fleet rollup provenance (C5) is consumed by `hf fleet status` (`hf/src/fleet.rs`) as its P7
  integrity gate — a fleet-dimension hook.
- G1/G2 are candidate kernel tasks: a real `verify_witness_chain` (recompute action_hash from
  payload + check stored prev_hash linkage) would close the tamper-evidence gap and make
  `hf doctor` a true integrity sweep.
