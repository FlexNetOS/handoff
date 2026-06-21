# ADR-0017 — Pure-Rust ledger store: redb authoritative + native-RVF recall (retire C-SQLite)

- **Status:** Accepted (design) — implements **HFTASK-0053** (Issue #71.1)
- **Date:** 2026-06-21
- **Supersedes for the storage engine:** the `rusqlite`/`bundled` (C-SQLite) backend of `ledger` v1
- **Related:** HFTASK-0006 (RVF ledger v2), HFTASK-0028 (`BEGIN IMMEDIATE` serialization), HFTASK-0048 (lease state machine), HFTASK-0059 (bounded busy-retry), HFTASK-0062 (RVF stale-lock reclaim), ADR-0001 R-no-C trust boundary
- **Note on numbering:** this is the next canonical `docs/adr-*` (0016 → 0017). It is unrelated to `.handoff/decisions/adr-0017-cognitum-gate.md`, which lives in the separate in-loop decision log (numbering-space reconciliation tracked as a doc-sync item, not part of this ADR).

## Context

The kernel's North Star requires a **no-C trust boundary** (ADR-0001): nothing in the
authoritative path may link a C library. The 2026-06-21 whole-repo audit (@`develop 163db9f`)
found this **unmet**: `ledger/Cargo.toml` declares

```toml
default = ["v2"]
v2 = ["v1", "dep:rvf-runtime", "dep:rvf-index", "dep:rvf-types"]
v1 = ["dep:rusqlite", "dep:rvf-crypto"]
rusqlite = { version = "0.31", features = ["bundled"] }
```

so the **default `hf` build compiles and links bundled C-SQLite**. The RVF "v2" layer
(HFTASK-0006) was built as a *semantic-recall overlay on top of* the v1 rusqlite store, not as a
replacement — every authoritative method (`append`, `try_acquire_lease`, `rollup_from`,
`verify_witness_chain`, `verify_rollup_provenance`, `sync_cursor_*`, `replay_latest_status`)
delegates to v1, which is rusqlite. HFTASK-0053 was marked Done but the C dependency remained, so
0053 was reopened (witnessed Done→Backlog) to actually achieve the boundary.

Two design questions had to be settled, both with evidence (three parallel research threads,
2026-06-21):

1. **Can the authoritative store be native-RVF** (eliminate the structured store entirely)?
2. **If a pure-Rust KV store is still needed, redb or sled?**

## What the authoritative store must do (requirements, from the v1 surface map)

All of these live in `ledger/src/v1.rs` today; `v2.rs` adds only vector ingest + recall and
delegates everything below to v1:

1. **Open + idempotent self-migration**, safe on fresh / pre-migration / migrated DBs; `:memory:`
   ephemeral mode for tests.
2. **Append-with-hash-chain, atomically** — read the authoritative tail `(seq, action_hash)`,
   set `prev_hash := tail.action_hash`, insert at `seq := tail+1`, **all in one serialized write
   critical section** (the cached open-time tail is deliberately NOT trusted inside the tx).
3. **Atomic compare-and-set lease** — replay the resource's lease events, resolve the live holder
   (TTL + release aware), and conditionally append `lease_acquired` in **one tx**; no write on
   conflict.
4. **Keyed / ordered reads** — all events by seq; events `WHERE seq > ?`; events by
   `work_order_id` + type (lease/status replay); tail `ORDER BY seq DESC LIMIT 1`.
5. **Rollup re-append with provenance, transactional batch** — re-chain source rows onto the
   central tail stamping `origin_repo/origin_seq/origin_action_hash`, **skip-on-duplicate** (the
   partial-unique idempotency backstop), and advance `sync_cursor` **in the same tx** (crash-safe).
6. **`sync_cursor` get / upsert** (per-origin high-water mark).
7. **Witness-chain & rollup-provenance verification** — ordered scans feeding `rvf-crypto`
   (witness) and a per-row re-hash compare (provenance).

### Hard, non-negotiable constraints

- **C-1 Atomic read-modify-write append** is one serialized write tx (SQLite `BEGIN IMMEDIATE`):
  two concurrent writers can never fork the witness chain or duplicate a `seq`.
- **C-2 Cross-process single-writer exclusion on one file** — the ledger is touched by multiple OS
  processes (two `hf` sessions, or a session + a PostEdit checkpoint hook), proven by
  `concurrent_writers_serialize_no_lock_no_fork` (8 threads × 25 appends, each its own handle).
  Not just an in-process Mutex.
- **C-3 Bounded transient-contention retry** (today `with_busy_retry`, ≤100 attempts, linear
  backoff) — correct only because each attempt re-reads the tail in a fresh tx.
- **C-4 Distinguishable unique-violation** for rollup idempotency (skip-and-count).
- **C-5 "No row" → documented default** (empty ledger → `(0, [0u8;32])`; absent cursor → `None`).
- **C-6 Crash-safe rollup batch** — all re-appends + the cursor advance commit together or not at
  all.

## Decision

**Hybrid, pure-Rust: `redb` is the authoritative transactional store; native-RVF stays the
semantic-recall overlay. Retire C-SQLite.**

### 1. Native-RVF: keep as the recall overlay, do NOT make it authoritative

RVF (`rvf-runtime`/`rvf-index`/`rvf-crypto`) is an **append-only vector-segment store with
ANN-only retrieval**. Evidence-backed limits that disqualify it as the system-of-record:

- **No transactional read-modify-write / CAS** — append-only; cannot guarantee C-1's non-forked
  chain or C-3's lease CAS.
- **No point lookup by key, no range/seq scan** — only ANN `query`. (v2's own `query_by_intent`
  gets ids from RVF then **re-reads the bodies from v1** — proof of the gap.)
- **Witness chain is write-side only** — `append_witness` updates an in-RAM `last_witness_hash`;
  `boot()` never reloads/verifies it and compaction resets it to genesis. It is a *signal*, not an
  authority.

RVF keeps doing exactly what it already does well: per-event 384-d vector ingest + HNSW
`query_by_intent` semantic recall. Optionally it continues to mirror a `WITNESS_SEG` as a
secondary tamper-evident signal. **No change to RVF's role.**

### 2. redb (not sled) for the authoritative transactional store

| Axis | redb | sled | Decisive? |
|------|------|------|-----------|
| Maturity (2026) | **4.1.0 stable, actively maintained** (release 2026-04) | 1.0.0-alpha.124, **last release 2024-10**, README out-of-sync, rewrite unshipped | ✔ redb |
| Txn model | **MVCC, single-writer, serializable**; `begin_write` blocks until prior commit = clean `BEGIN IMMEDIATE` analogue | optimistic `TransactionalTree`, closures must be idempotent/re-runnable | ✔ redb (matches C-1) |
| Crash safety | COW B+trees, `set_two_phase_commit(true)`, `set_quick_repair(true)`, per-tx `Durability` | log-structured, pre-1.0 durability in flux | ✔ redb (C-6) |
| On-disk format | **stable, documented upgrade path** | **"will change, manual migrations" pre-1.0** | ✔ redb |
| Keyed + range reads | `get`, `range`, `iter`, `first`/`last` (zero-copy) | `get`, `range`, `scan_prefix` | tie |
| Secondary index | typed `MultimapTableDefinition` | multiple trees | ✔ redb (slight) |
| Pure-Rust / no C / no `-sys` | yes | yes | tie (both pass no-C) |
| License | MIT/Apache-2.0 | MIT/Apache-2.0 | tie |

A tamper-evident continuity ledger cannot sit on a pre-1.0 store with an explicitly unstable
on-disk format. redb's single-writer **serializable** transaction is a near-exact semantic match
for the `BEGIN IMMEDIATE` critical section the integrity guarantee depends on.

## Schema mapping (SQLite → redb)

redb is a typed KV B-tree, so SQL tables/indices become explicit typed tables maintained **inside
the same write tx** as the append (so indices never drift):

| SQLite | redb |
|--------|------|
| `events(seq PK, …, action_hash, prev_hash, origin_*)` | `EVENTS: TableDefinition<u64, &[u8]>` — key = `seq` (big-endian for ordered range scans), value = a `serde`/bincode-encoded `EventRow` (all columns incl. the three `Option` origin fields) |
| `idx_events_origin (origin_repo, origin_seq) UNIQUE WHERE origin_repo IS NOT NULL` | `ORIGIN_INDEX: TableDefinition<(&str, u64), u64>` — key `(origin_repo, origin_seq) → seq`; **insert-if-absent inside the tx yields the partial-unique semantics** (only rolled-up rows are indexed; native rows are simply not inserted → unconstrained). A pre-existing key = the C-4 "duplicate" signal → skip+count. |
| tail `ORDER BY seq DESC LIMIT 1` | `EVENTS.last()` |
| `WHERE work_order_id = ? AND type IN(...)` (lease/status replay) | secondary `BY_WORK_ORDER: MultimapTableDefinition<&str, u64>` (`work_order_id → seq`), maintained in-tx; replay = multimap scan → point-get rows |
| `sync_cursor(origin_repo PK, last_seq, updated_ns)` | `SYNC_CURSOR: TableDefinition<&str, (u64, u64)>` — point get/upsert |

`hash_action`, `resolve_lease`, `LeaseOutcome`, `RollupStat`, `RollupProvenance`, the witness-chain
crypto (`rvf-crypto`), and **all of `v2.rs`'s vector logic are backend-agnostic and port
unchanged** — only the storage calls and the error type change.

## How each hard constraint is met on redb

- **C-1 / C-3 (atomic RMW + lease CAS):** one `db.begin_write()` per append/lease op; read
  `EVENTS.last()` → compute hash → insert; serializable isolation guarantees no interleave/fork.
- **C-2 (cross-process single-writer on one file):** redb takes an **OS file lock** on the database
  file — a second process's `Database::open` for write is excluded, satisfying the multi-process
  test. (Documented + asserted in the port's concurrency test, mirroring
  `concurrent_writers_serialize_no_lock_no_fork`.)
- **C-3 (bounded retry):** keep the `with_busy_retry` wrapper; classify redb's
  lock-contention/`DatabaseAlreadyOpen`/transaction errors as transient and retry the whole closure
  (each attempt re-reads the tail). Same shape, new error matcher.
- **C-4 (idempotency):** `ORIGIN_INDEX` insert-if-absent; pre-existing key → skip+count.
- **C-5 (no-row defaults):** `Option`-returning `get`/`last` map directly to the existing defaults.
- **C-6 (crash-safe rollup):** all re-appends + index inserts + cursor advance in **one
  `begin_write`**, `commit()` once; `set_two_phase_commit(true)` for the chain-critical commits.

## Migration / cutover plan (no-downgrade, fail-closed)

1. **Introduce `redb` behind the SAME `ledger` public API.** Introduce a `ledger`-owned error type
   (`LedgerError`) aliased so `hf` and the fleet-rollup callers are source-unchanged; v2 delegations
   and the two `rusqlite::Error` constructions in `v2.rs` re-point to it.
2. **Differential / golden tests BEFORE cutover:** run the same event stream (append, lease
   acquire/heartbeat/release, rollup, verify) through the old SQLite path and the new redb path and
   assert identical `seq`, `action_hash`, `prev_hash`, lease outcomes, rollup-provenance verdicts,
   and witness-chain verification counts. Port the existing v1 test suite (incl.
   `concurrent_writers_serialize_no_lock_no_fork`, `old_schema_db_migrates`, `migration_is_idempotent`)
   onto redb.
3. **One-time data migration** for any existing on-disk `ledger.db`: a `legacy-sqlite` **non-default**
   feature compiles a read-only rusqlite importer that streams events → redb in seq order,
   re-verifying the witness chain on the way in (fail-closed on mismatch). Local ledgers are
   gitignored and small; central/fleet ledger migrates once.
4. **Flip the default + drop the C dep:** `default = ["redb-store"]`; remove `rusqlite` from the
   default graph (kept only under the opt-in `legacy-sqlite` migration feature, or removed entirely
   once migration is complete).
5. **Prove the boundary:** `cargo tree -e no-dev` (and the network-control-style `no-c` check)
   shows **zero `rusqlite`/`*-sys`/C crates** in the default `hf` build. Add a CI assertion.

## Cargo changes

```toml
[dependencies]
redb = "4"                       # pure Rust, no -sys/C deps, ACID single-writer serializable
# rusqlite moves OUT of the default graph:
rusqlite = { version = "0.31", features = ["bundled"], optional = true }

[features]
# Default keeps the v2 RVF recall overlay (parity with the pre-port default) — now redb-backed,
# C-free. Dropping the overlay from the default would be a capability downgrade.
default = ["v2"]
redb-store = ["dep:redb", "dep:rvf-crypto"]                                # authoritative store, no overlay
v2 = ["redb-store", "dep:rvf-runtime", "dep:rvf-index", "dep:rvf-types"]   # recall overlay (default)
legacy-sqlite = ["redb-store", "dep:rusqlite"]                             # one-time migration importer ONLY; never default
```

## Acceptance criteria (the rebuilt HFTASK-0053)

- [ ] Default `cargo build -p hf` links **no** `rusqlite`/C/`-sys` crate (`cargo tree` proof + CI gate).
- [ ] `ledger` public API unchanged (callers in `hf`/fleet compile without edits beyond the error alias).
- [ ] All ported v1 tests pass on redb, **including** the multi-process concurrency, old-schema
      migration, and idempotency tests.
- [ ] Differential test: SQLite-path vs redb-path produce byte-identical `action_hash`/`prev_hash`
      chains + identical lease/rollup/verify outcomes on a shared event stream.
- [ ] Witness-chain verify + rollup-provenance verify pass on a redb ledger.
- [ ] One-time importer migrates an existing `ledger.db` with fail-closed chain re-verification.
- [ ] RVF recall (`query_by_intent`) still works (overlay unchanged).
- [ ] `fmt` + `clippy --all-targets -D warnings` + full test suite green; PR `--base develop`.

## Consequences

- **+** The no-C trust boundary is actually met by the default build; the integrity guarantee moves
  to a stable, format-stable, actively-maintained pure-Rust store with a serializable single-writer
  transaction that matches the existing `BEGIN IMMEDIATE` semantics.
- **+** RVF keeps its proven role; no behavior is downgraded.
- **−** SQL queries become explicit table walks + maintained secondary indices (more code in
  `v1.rs`/the new store module); the rollup re-derivation moves from SQL to Rust iteration — the
  main place correctness bugs could hide, mitigated by the differential/golden tests.
- **−** redb major-version format upgrades require a gated migration step (pin `redb = "4"`, record
  a format version in the ledger header).
