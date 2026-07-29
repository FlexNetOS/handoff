//! `ledger` — the .handoff operational-truth tier (authoritative store).
//!
//! **Pure-Rust store (ADR-0017 / HFTASK-0053):** the authoritative event ledger is backed by
//! [`redb`] — a pure-Rust, ACID, single-writer **serializable** embedded KV store (no `-sys`,
//! no C in the trust boundary). It replaces the previous bundled C-SQLite (`rusqlite`) backend
//! while preserving every integrity invariant byte-for-byte: the witnessed append-only
//! hash-chain, replay, the `BEGIN IMMEDIATE`-style atomic read-modify-write append
//! (HFTASK-0028), the in-ledger lease CAS (HFTASK-0048), and rollup provenance
//! (HFTASK-0031/0032/0033). Tamper-evidence still comes from `rvf-crypto`'s `WitnessChain`.
//!
//! redb's `begin_write()` is a serializable single-writer critical section — the exact analogue
//! of SQLite's `BEGIN IMMEDIATE`. Each append/lease/rollup op opens ONE write transaction,
//! re-reads the authoritative tail `(seq, action_hash)` *inside* the tx, and chains off it, so
//! two concurrent writers can never fork the chain or duplicate a `seq`.

use redb::{
    Builder, Database, MultimapTableDefinition, ReadableDatabase, ReadableMultimapTable,
    ReadableTable, TableDefinition, backends::InMemoryBackend,
};
use rvf_crypto::witness::{WitnessEntry, create_witness_chain, verify_witness_chain};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use work_order::{Status, WorkOrder};

// ---------------------------------------------------------------------------
// Error type (ADR-0017 §"API contract"): a `ledger`-owned error so callers and
// the v2 overlay are decoupled from the storage backend. Replaces the previous
// `rusqlite::Error` / `rusqlite::Result` surface.
// ---------------------------------------------------------------------------

/// The `ledger` crate error. Wraps redb's transactional/storage errors plus a few
/// ledger-domain conditions, so the public API never leaks the storage backend's type.
#[derive(Debug)]
pub enum LedgerError {
    /// Opening/creating the database file failed (incl. cross-process single-writer exclusion:
    /// a second process holding the OS file lock surfaces here).
    Database(redb::DatabaseError),
    /// A write/read transaction could not be started.
    Transaction(redb::TransactionError),
    /// A table could not be opened within a transaction.
    Table(redb::TableError),
    /// A keyed read/write storage operation failed.
    Storage(redb::StorageError),
    /// Commit failed.
    Commit(redb::CommitError),
    /// A stored value could not be decoded (corrupt row).
    Decode(String),
    /// A query was called with an argument of the wrong shape (e.g. wrong-dim intent vector).
    /// `(expected, got)` — mirrors the old `InvalidParameterCount` guard used by v2.
    InvalidParameterCount(usize, usize),
    /// An overlay (RVF) operation failed; carries the formatted cause.
    Overlay(String),
    /// The on-disk file is a legacy bundled-C-SQLite ledger (ADR-0017), not a redb store.
    /// Fail-closed (never silently treat it as empty/redb): the holder must run the one-time
    /// `hf migrate` importer. Carries the offending path.
    LegacySqlite(String),
    /// The witness chain failed verification: a stored event no longer matches its own
    /// content hash, or its `prev_hash` does not link to the prior event's `action_hash`.
    /// Tamper-evidence (HFTASK-0079): proves the binary cache was altered after commit.
    /// `seq` = the offending event sequence; `field` = `"action_hash"` (content tampered),
    /// `"prev_hash"` (linkage broken: reorder/delete/splice), or `"rvf_segment"` (structural).
    WitnessTampered { seq: u64, field: &'static str },
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::Database(e) => write!(f, "ledger database error: {e}"),
            LedgerError::Transaction(e) => write!(f, "ledger transaction error: {e}"),
            LedgerError::Table(e) => write!(f, "ledger table error: {e}"),
            LedgerError::Storage(e) => write!(f, "ledger storage error: {e}"),
            LedgerError::Commit(e) => write!(f, "ledger commit error: {e}"),
            LedgerError::Decode(s) => write!(f, "ledger decode error: {s}"),
            LedgerError::InvalidParameterCount(exp, got) => {
                write!(
                    f,
                    "ledger invalid parameter count: expected {exp}, got {got}"
                )
            }
            LedgerError::Overlay(s) => write!(f, "ledger overlay error: {s}"),
            LedgerError::LegacySqlite(p) => write!(
                f,
                "legacy C-SQLite ledger detected at {p} — this binary uses the pure-Rust redb \
                 store (ADR-0017). Run the one-time importer `hf migrate {p}` (a binary built \
                 with `--features legacy-sqlite`) to convert it to redb; refusing to proceed \
                 (fail-closed) rather than treat it as an empty ledger."
            ),
            LedgerError::WitnessTampered { seq, field } => write!(
                f,
                "witness chain verification failed at seq {seq}: {field} mismatch — the stored \
                 event no longer matches its committed hash/linkage. The binary ledger cache has \
                 been altered after commit; rebuild it from the committed JSONL export \
                 (`hf import`) and audit git history (fail-closed)."
            ),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<redb::DatabaseError> for LedgerError {
    fn from(e: redb::DatabaseError) -> Self {
        LedgerError::Database(e)
    }
}
impl From<redb::TransactionError> for LedgerError {
    fn from(e: redb::TransactionError) -> Self {
        LedgerError::Transaction(e)
    }
}
impl From<redb::TableError> for LedgerError {
    fn from(e: redb::TableError) -> Self {
        LedgerError::Table(e)
    }
}
impl From<redb::StorageError> for LedgerError {
    fn from(e: redb::StorageError) -> Self {
        LedgerError::Storage(e)
    }
}
impl From<redb::CommitError> for LedgerError {
    fn from(e: redb::CommitError) -> Self {
        LedgerError::Commit(e)
    }
}

/// The `ledger` crate result alias (ADR-0017): callers used to write `rusqlite::Result<T>`
/// transparently through `?`; they now flow through this without source edits.
pub type Result<T> = std::result::Result<T, LedgerError>;

// ---------------------------------------------------------------------------
// redb schema (ADR-0017 §"Schema mapping"). All typed tables, maintained inside
// the SAME write tx as the append so secondary indices never drift.
// ---------------------------------------------------------------------------

/// `EVENTS: seq → bincode-ish(EventBody)` — the authoritative event log. The `u64` key is
/// stored big-endian by redb so `last()`/range scans are in seq order (the tail = `last()`).
const EVENTS: TableDefinition<u64, &[u8]> = TableDefinition::new("events");

/// `ORIGIN_INDEX: (origin_repo, origin_seq) → seq` — the partial-unique rollup idempotency
/// guard. Only rolled-up rows are inserted (native rows are never indexed → unconstrained),
/// so a pre-existing key is the C-4 "already rolled up" signal → skip + count.
const ORIGIN_INDEX: TableDefinition<(&str, u64), u64> = TableDefinition::new("origin_index");

/// `BY_WORK_ORDER: work_order_id → {seq}` — secondary index for lease/status replay
/// (multimap scan → point-get rows), maintained in-tx.
const BY_WORK_ORDER: MultimapTableDefinition<&str, u64> =
    MultimapTableDefinition::new("by_work_order");

/// `SYNC_CURSOR: origin_repo → (last_seq, updated_ns)` — per-origin rollup high-water mark.
const SYNC_CURSOR: TableDefinition<&str, (u64, u64)> = TableDefinition::new("sync_cursor");

/// The persisted shape of one event row (all `events` columns, incl. the three optional
/// origin/provenance fields). Encoded with `serde_json` (already a dep) into the `EVENTS`
/// value blob. The witness/provenance hashes are computed from the ORIGINAL string fields
/// (never from this blob), so the encoding is storage-only and not security-load-bearing.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventBody {
    ts_ns: u64,
    event_type: String,
    work_order_id: String,
    payload_json: String,
    #[serde(with = "hash32")]
    action_hash: [u8; 32],
    #[serde(with = "hash32")]
    prev_hash: [u8; 32],
    /// `None` for a native (local) event; `Some(repo)` for a rolled-up central row.
    origin_repo: Option<String>,
    origin_seq: Option<u64>,
    #[serde(default, with = "opt_hash32")]
    origin_action_hash: Option<[u8; 32]>,
}

/// serde helpers so `[u8; 32]` round-trips compactly (as a byte array, not a struct).
mod hash32 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        serde_bytes_like(v, s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let v: Vec<u8> = Vec::deserialize(d)?;
        let mut out = [0u8; 32];
        if v.len() == 32 {
            out.copy_from_slice(&v);
        }
        Ok(out)
    }
    fn serde_bytes_like<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(32))?;
        for b in v {
            seq.serialize_element(b)?;
        }
        seq.end()
    }
}

mod opt_hash32 {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &Option<[u8; 32]>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(h) => super::hash32::serialize(h, s),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 32]>, D::Error> {
        let v: Option<Vec<u8>> = Option::deserialize(d)?;
        Ok(v.map(|bytes| {
            let mut out = [0u8; 32];
            if bytes.len() == 32 {
                out.copy_from_slice(&bytes);
            }
            out
        }))
    }
}

/// A rolled-up (provenance-bearing) row, decoded for `verify_rollup_provenance`. Keeps the
/// verifier readable (avoids a 6-tuple) — `origin_seq` is only used to order the per-repo
/// breakdown deterministically.
struct RolledRow {
    origin_repo: String,
    origin_seq: u64,
    event_type: String,
    work_order_id: String,
    payload_json: String,
    origin_action_hash: Option<[u8; 32]>,
}

fn encode_body(b: &EventBody) -> Vec<u8> {
    // INFALLIBLE: `EventBody` is an all-owned struct (String/Option/[u8;32]) with derived
    // `Serialize` and no non-string map keys, so `to_vec` cannot error. Keeping it non-fallible
    // here keeps the storage hot path simple. Justified per-site (HFTASK-0080).
    #[allow(clippy::expect_used)]
    serde_json::to_vec(b).expect("EventBody serializes")
}

fn decode_body(bytes: &[u8]) -> Result<EventBody> {
    serde_json::from_slice(bytes).map_err(|e| LedgerError::Decode(e.to_string()))
}

// ---------------------------------------------------------------------------
// Public value types (backend-agnostic — UNCHANGED from the SQLite impl).
// ---------------------------------------------------------------------------

pub struct Ledger {
    db: Database,
    seq: u64,
    prev_witness_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct EventRow {
    pub seq: u64,
    pub ts_ns: u64,
    pub event_type: String,
    pub work_order_id: String,
    pub payload_json: String,
    /// The source ledger's own `action_hash` for this event. HFTASK-0032: the central
    /// ledger recomputes the same hash on rollup (inputs are identical) and stores it as
    /// `origin_action_hash` — the provenance bridge proving the rolled row IS this event.
    pub action_hash: [u8; 32],
}

/// HFTASK-0032: outcome of rolling one source repo's events into the central ledger.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RollupStat {
    /// Newly re-appended events (re-chained onto the central tail, provenance stamped).
    pub appended: usize,
    /// Events skipped because they were already rolled up (idempotency: the partial
    /// `ORIGIN_INDEX` key already existed, or the cursor already covered them).
    pub skipped_existing: usize,
}

/// HFTASK-0033 (ADR-0004 §3.3 rev): outcome of verifying the rollup *provenance bridge* —
/// that each rolled-up central row faithfully reproduces the source event it claims to
/// mirror. Computed by [`Ledger::verify_rollup_provenance`]. `mismatched == 0` is the
/// faithfulness gate: every rolled row's recomputed action hash equals its stored
/// `origin_action_hash`, so any central event traces back to its origin repo.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RollupProvenance {
    /// Rolled-up rows whose recomputed `hash_action(event_type, work_order_id, payload_json)`
    /// byte-matched the stored `origin_action_hash` (the proof bridge holds).
    pub verified: usize,
    /// Rolled-up rows whose recomputed hash did NOT match `origin_action_hash` (or whose
    /// `origin_action_hash` was NULL/malformed) — provenance broken; the gate must fail.
    pub mismatched: usize,
    /// Per-origin breakdown of verified rows: `(origin_repo, verified_count)`, sorted by repo.
    pub per_repo: Vec<(String, usize)>,
}

impl RollupProvenance {
    /// True iff every rolled-up row's provenance held (no mismatches). Native (NULL-origin)
    /// rows are not rolled-up rows and are out of scope, so a ledger with zero rollups is
    /// vacuously faithful.
    pub fn is_faithful(&self) -> bool {
        self.mismatched == 0
    }

    /// Total rolled-up rows examined (verified + mismatched).
    pub fn total(&self) -> usize {
        self.verified + self.mismatched
    }
}

pub fn hash_action(event_type: &str, work_order_id: &str, payload: &str) -> [u8; 32] {
    let mut h = Sha3_256::new();
    h.update(event_type.as_bytes());
    h.update(work_order_id.as_bytes());
    h.update(payload.as_bytes());
    h.finalize().into()
}

/// HFTASK-0048: outcome of an atomic in-ledger lease acquisition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseOutcome {
    /// Lease was free → we now hold it (carries the witnessed seq).
    Acquired { seq: u64 },
    /// We already held it → the lease was extended (heartbeat).
    Heartbeat { seq: u64 },
    /// Another live holder owns it → no write occurred.
    Conflict { holder: String },
}

/// Pure lease state machine: replay this resource's `lease_acquired`/`lease_released` events
/// (in seq order) and resolve the current live holder, honoring release and TTL expiry against
/// `now_ns`. Free/expired → `None`. Split out so the policy is unit-testable without a DB.
pub fn resolve_lease(events: &[(String, String)], now_ns: u64) -> Option<String> {
    let mut held: Option<(String, u64)> = None; // (holder, expiry_ns)
    for (etype, payload) in events {
        let v: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
        let holder = v["holder"].as_str().unwrap_or_default().to_string();
        match etype.as_str() {
            "lease_acquired" => {
                let acq = v["acquired_ns"].as_u64().unwrap_or(0);
                let ttl = v["ttl_secs"].as_u64().unwrap_or(0);
                let expiry = acq.saturating_add(ttl.saturating_mul(1_000_000_000));
                held = Some((holder, expiry));
            }
            "lease_released" if held.as_ref().is_some_and(|(h, _)| *h == holder) => {
                // a release only clears the lease when it names the current holder
                held = None;
            }
            _ => {}
        }
    }
    held.and_then(|(h, expiry)| (expiry > now_ns).then_some(h))
}

/// True if a ledger error is a transient lock/contention condition safe to retry.
///
/// On redb the cross-process single-writer exclusion surfaces as
/// `DatabaseError::DatabaseAlreadyOpen` when a second OS process (or a second in-process
/// handle) tries to open the file for writing while another holds it. That is the redb
/// analogue of SQLITE_BUSY/LOCKED: the contender simply lost the race and should retry the
/// whole closure (which re-opens / re-reads the tail). Storage-level lock contention surfaces
/// as `StorageError`; we treat the lock-acquisition class as transient.
fn is_busy(e: &LedgerError) -> bool {
    matches!(
        e,
        LedgerError::Database(redb::DatabaseError::DatabaseAlreadyOpen)
    )
}

/// Run a write operation, retrying the WHOLE closure on transient lock contention.
///
/// HFTASK-0059 (ported to redb, ADR-0017 C-3): redb serializes in-process writers on a single
/// `Database` handle, but a second *process* (two `hf` sessions, or a session + a PostEdit
/// checkpoint hook) opening the same file for write is excluded by the OS file lock and
/// surfaces `DatabaseAlreadyOpen`. Retrying is safe for every ledger write because each
/// attempt re-opens the database and re-reads the authoritative tail (seq + prev_hash) inside
/// a fresh `begin_write()`, so no fork or duplicate seq can result. A short linear backoff that
/// grows with the attempt (capped) keeps concurrent processes from re-colliding in lockstep.
/// Non-transient errors return immediately; the attempt cap bounds worst-case latency so a
/// genuinely stuck lock still surfaces as an error rather than hanging.
fn with_busy_retry<T>(mut op: impl FnMut() -> Result<T>) -> Result<T> {
    const MAX_ATTEMPTS: u32 = 100;
    let mut attempt: u32 = 0;
    loop {
        match op() {
            Err(e) if is_busy(&e) && attempt + 1 < MAX_ATTEMPTS => {
                attempt += 1;
                let backoff_ms = (attempt as u64).min(10);
                std::thread::sleep(std::time::Duration::from_millis(backoff_ms));
            }
            other => return other,
        }
    }
}

/// True iff the file at `path` exists and begins with the SQLite-3 magic
/// (`"SQLite format 3\0"`). Used to fail-closed on a legacy pre-redb ledger (ADR-0017). A
/// missing/short/unreadable file is NOT legacy (open proceeds to create a fresh redb store).
pub fn file_is_legacy_sqlite(path: &str) -> bool {
    use std::io::Read;
    const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
    match std::fs::File::open(path) {
        Ok(mut f) => {
            let mut buf = [0u8; 16];
            f.read_exact(&mut buf).is_ok() && &buf == SQLITE_MAGIC
        }
        Err(_) => false,
    }
}

impl Ledger {
    /// Open (or create) the ledger at `path`. `":memory:"` for an ephemeral in-RAM store
    /// (tests), mapped onto redb's `InMemoryBackend`.
    ///
    /// ADR-0017 / HFTASK-0028: redb's `begin_write()` is a serializable single-writer critical
    /// section (the `BEGIN IMMEDIATE` analogue) and the file is OS-locked, so two concurrent
    /// writers serialize: the append path reads the latest tail *inside* the write tx, never
    /// trusting the open-time cache, so they can never both chain off the same prev_hash (which
    /// would fork the witness chain).
    pub fn open(path: &str) -> Result<Self> {
        // Fail-closed legacy guard (ADR-0017 cutover): a pre-port `ledger.db` is a bundled
        // C-SQLite file. `Database::create` would reject it with an opaque "invalid data";
        // detect the SQLite magic first and return an ACTIONABLE error pointing at `hf migrate`,
        // so a format mismatch can never be silently mistaken for an empty/new ledger.
        if path != ":memory:" && file_is_legacy_sqlite(path) {
            return Err(LedgerError::LegacySqlite(path.to_string()));
        }
        // HFTASK-0090: the OPEN itself must retry on the transient `DatabaseAlreadyOpen`, not
        // just writes. redb refuses a second concurrent open of one file IN THE SAME PROCESS;
        // when a second handle (another thread, or — under the test harness's shared process cwd
        // — a concurrent test resolving the same cwd-relative ledger) opens while the first is
        // still live, `Database::create` returns `DatabaseAlreadyOpen`. That is transient (the
        // other handle WILL drop), so retry the open with the SAME bounded backoff as writes
        // instead of failing closed — otherwise `open_ledger_or_exit` aborts the whole process.
        let db = if path == ":memory:" {
            Builder::new().create_with_backend(InMemoryBackend::new())?
        } else {
            with_busy_retry(|| Database::create(path).map_err(LedgerError::from))?
        };
        // Ensure all tables exist (idempotent self-migration: open == create-if-absent for
        // every table). A no-op on an already-initialized DB.
        {
            let tx = db.begin_write()?;
            tx.open_table(EVENTS)?;
            tx.open_table(ORIGIN_INDEX)?;
            tx.open_multimap_table(BY_WORK_ORDER)?;
            tx.open_table(SYNC_CURSOR)?;
            tx.commit()?;
        }
        // Resume seq + prev hash from the tail (replay-safe).
        let (seq, prev_witness_hash) = {
            let tx = db.begin_read()?;
            let events = tx.open_table(EVENTS)?;
            Self::read_tail(&events)?
        };
        Ok(Self {
            db,
            seq,
            prev_witness_hash,
        })
    }

    /// Read the authoritative tail `(seq, action_hash)` from an open EVENTS table. No row →
    /// the documented default `(0, [0u8; 32])` (ADR-0017 C-5).
    fn read_tail(events: &impl ReadableTable<u64, &'static [u8]>) -> Result<(u64, [u8; 32])> {
        match events.last()? {
            None => Ok((0, [0u8; 32])),
            Some((k, v)) => {
                let body = decode_body(v.value())?;
                Ok((k.value(), body.action_hash))
            }
        }
    }

    /// HFTASK-0031: read a source repo's rollup high-water mark from the central ledger's
    /// `SYNC_CURSOR` (the last per-repo `seq` already rolled up). `None` = never synced.
    pub fn sync_cursor_get(&self, origin_repo: &str) -> Result<Option<u64>> {
        let tx = self.db.begin_read()?;
        let cursor = tx.open_table(SYNC_CURSOR)?;
        Ok(cursor.get(origin_repo)?.map(|v| v.value().0))
    }

    /// HFTASK-0031: upsert a source repo's rollup high-water mark (last rolled-up per-repo
    /// `seq`) into the central ledger's `SYNC_CURSOR`.
    pub fn sync_cursor_set(
        &mut self,
        origin_repo: &str,
        last_seq: u64,
        updated_ns: u64,
    ) -> Result<()> {
        with_busy_retry(|| {
            let tx = self.db.begin_write()?;
            {
                let mut cursor = tx.open_table(SYNC_CURSOR)?;
                cursor.insert(origin_repo, (last_seq, updated_ns))?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Append a witnessed event. `ts_ns` is passed in (deterministic in tests).
    ///
    /// ADR-0017 / HFTASK-0028: the seq + prev_hash are read from the DB tail *inside* the
    /// `begin_write()` transaction (rather than trusting the values cached at `open()`), so
    /// concurrent writers serialize: the second writer's `begin_write()` blocks until the first
    /// commits, then reads the now-current tail and chains off it. Two writers can therefore
    /// never both chain off the same prev_hash (no forked witness chain).
    pub fn append(
        &mut self,
        event_type: &str,
        work_order_id: &str,
        payload_json: &str,
        ts_ns: u64,
    ) -> Result<u64> {
        let action_hash = hash_action(event_type, work_order_id, payload_json);
        let next_seq = with_busy_retry(|| {
            let tx = self.db.begin_write()?;
            let next_seq = {
                let mut events = tx.open_table(EVENTS)?;
                // Re-read the authoritative tail from the DB (not the cached open()-time values):
                // a concurrent writer may have advanced it since this handle was opened.
                let (tail_seq, prev_hash) = Self::read_tail(&events)?;
                let next_seq = tail_seq + 1;
                let body = EventBody {
                    ts_ns,
                    event_type: event_type.to_string(),
                    work_order_id: work_order_id.to_string(),
                    payload_json: payload_json.to_string(),
                    action_hash,
                    prev_hash,
                    origin_repo: None,
                    origin_seq: None,
                    origin_action_hash: None,
                };
                events.insert(next_seq, encode_body(&body).as_slice())?;
                next_seq
            };
            {
                let mut by_wo = tx.open_multimap_table(BY_WORK_ORDER)?;
                by_wo.insert(work_order_id, next_seq)?;
            }
            tx.commit()?;
            Ok(next_seq)
        })?;
        // Keep the in-memory cache consistent with what we just committed.
        self.seq = next_seq;
        self.prev_witness_hash = action_hash;
        Ok(next_seq)
    }

    /// HFTASK-0048: **atomically** acquire an advisory lease on `resource`, in-ledger.
    ///
    /// The whole check-then-write runs inside ONE `begin_write()` transaction, so two
    /// concurrent acquirers serialize on the write lock (HFTASK-0028's invariant): the second
    /// blocks until the first commits, then re-reads the now-current lease state. This is a
    /// **no-downgrade superset** of the weave advisory lease — it gives real mutual exclusion
    /// even when weave is absent. `lease_acquired` / `lease_released` events are witnessed
    /// exactly like any other event (chained prev_hash), keyed by `work_order_id = resource`
    /// so replay is a single indexed (multimap) scan.
    ///
    /// - free, or held by `holder` (heartbeat/extend) → append `lease_acquired`, return
    ///   `Acquired`/`Heartbeat`.
    /// - held by another live holder → no write, return `Conflict { holder }`.
    pub fn try_acquire_lease(
        &mut self,
        resource: &str,
        holder: &str,
        ttl_secs: u64,
        now_ns: u64,
    ) -> Result<LeaseOutcome> {
        // HFTASK-0059: retry the whole check-then-write on transient lock contention. Each
        // attempt re-resolves the lease state and re-reads the tail inside a fresh
        // begin_write(), so a retry can never fork the chain or double-acquire.
        with_busy_retry(|| {
            let tx = self.db.begin_write()?;
            // Resolve the current holder from this resource's lease history, INSIDE the write tx.
            let events = Self::lease_events_in_tx(&tx, resource)?;
            let current = resolve_lease(&events, now_ns);
            let heartbeat = match &current {
                Some(h) if h == holder => true, // we already hold it → extend
                Some(other) => {
                    let other = other.clone();
                    // read-only path: drop the write tx without committing.
                    drop(tx);
                    return Ok(LeaseOutcome::Conflict { holder: other });
                }
                None => false, // free
            };

            // Append the witnessed `lease_acquired` event, chaining off the live tail.
            let payload = serde_json::json!({
                "resource": resource,
                "holder": holder,
                "ttl_secs": ttl_secs,
                "acquired_ns": now_ns,
            })
            .to_string();
            let action_hash = hash_action("lease_acquired", resource, &payload);
            let next_seq = {
                let mut events = tx.open_table(EVENTS)?;
                let (tail_seq, prev_hash) = Self::read_tail(&events)?;
                let next_seq = tail_seq + 1;
                let body = EventBody {
                    ts_ns: now_ns,
                    event_type: "lease_acquired".to_string(),
                    work_order_id: resource.to_string(),
                    payload_json: payload,
                    action_hash,
                    prev_hash,
                    origin_repo: None,
                    origin_seq: None,
                    origin_action_hash: None,
                };
                events.insert(next_seq, encode_body(&body).as_slice())?;
                next_seq
            };
            {
                let mut by_wo = tx.open_multimap_table(BY_WORK_ORDER)?;
                by_wo.insert(resource, next_seq)?;
            }
            tx.commit()?;
            self.seq = next_seq;
            self.prev_witness_hash = action_hash;
            Ok(if heartbeat {
                LeaseOutcome::Heartbeat { seq: next_seq }
            } else {
                LeaseOutcome::Acquired { seq: next_seq }
            })
        })
    }

    /// Read this resource's lease events (`lease_acquired`/`lease_released`) in seq order from
    /// INSIDE an open write tx — a multimap scan over `BY_WORK_ORDER` then point-gets, filtered
    /// to lease event types.
    fn lease_events_in_tx(
        tx: &redb::WriteTransaction,
        resource: &str,
    ) -> Result<Vec<(String, String)>> {
        let by_wo = tx.open_multimap_table(BY_WORK_ORDER)?;
        let events = tx.open_table(EVENTS)?;
        let mut seqs: Vec<u64> = Vec::new();
        for item in by_wo.get(resource)? {
            seqs.push(item?.value());
        }
        seqs.sort_unstable();
        let mut out = Vec::with_capacity(seqs.len());
        for seq in seqs {
            if let Some(v) = events.get(seq)? {
                let body = decode_body(v.value())?;
                if body.event_type == "lease_acquired" || body.event_type == "lease_released" {
                    out.push((body.event_type, body.payload_json));
                }
            }
        }
        Ok(out)
    }

    /// HFTASK-0048: release a lease we hold on `resource` (append `lease_released`). Idempotent
    /// and witnessed; uses the normal `append` path (no conditional needed — `resolve_lease`
    /// only clears the lease when the released holder matches the current one).
    pub fn release_lease(&mut self, resource: &str, holder: &str, now_ns: u64) -> Result<u64> {
        let payload = serde_json::json!({ "resource": resource, "holder": holder }).to_string();
        self.append("lease_released", resource, &payload, now_ns)
    }

    /// HFTASK-0048: the current live holder of `resource`, or `None` if free/expired. Pure read
    /// over the witnessed history (used by `hf lease` and the claim gate's degraded path).
    pub fn lease_holder(&self, resource: &str, now_ns: u64) -> Result<Option<String>> {
        let tx = self.db.begin_read()?;
        let by_wo = tx.open_multimap_table(BY_WORK_ORDER)?;
        let events = tx.open_table(EVENTS)?;
        let mut seqs: Vec<u64> = Vec::new();
        for item in by_wo.get(resource)? {
            seqs.push(item?.value());
        }
        seqs.sort_unstable();
        let mut lease_events = Vec::with_capacity(seqs.len());
        for seq in seqs {
            if let Some(v) = events.get(seq)? {
                let body = decode_body(v.value())?;
                if body.event_type == "lease_acquired" || body.event_type == "lease_released" {
                    lease_events.push((body.event_type, body.payload_json));
                }
            }
        }
        Ok(resolve_lease(&lease_events, now_ns))
    }

    /// Convenience: record a work-order state transition.
    pub fn record_transition(&mut self, wo: &WorkOrder, status: Status, ts_ns: u64) -> Result<u64> {
        let payload = serde_json::json!({
            "id": wo.id, "status": status, "correlation_id": wo.correlation_id, "role": wo.role
        })
        .to_string();
        self.append("task_transition", &wo.id, &payload, ts_ns)
    }

    pub fn all_events(&self) -> Result<Vec<EventRow>> {
        let tx = self.db.begin_read()?;
        let events = tx.open_table(EVENTS)?;
        let mut rows = Vec::new();
        for item in events.iter()? {
            let (k, v) = item?;
            rows.push(Self::body_to_row(k.value(), &decode_body(v.value())?));
        }
        Ok(rows)
    }

    /// HFTASK-0032: read the source ledger's events whose `seq > after_seq`, ordered by `seq`
    /// — the rollup pull. `after_seq` is the central ledger's `SYNC_CURSOR` value for this
    /// source repo (0 = never synced → all events). Self-contained rows (incl. `action_hash`)
    /// so the central ledger can re-append with provenance without re-opening the source
    /// mid-transaction.
    pub fn events_after(&self, after_seq: u64) -> Result<Vec<EventRow>> {
        let tx = self.db.begin_read()?;
        let events = tx.open_table(EVENTS)?;
        let mut rows = Vec::new();
        // u64 keys are big-endian ordered in redb → range scan is seq order.
        for item in events.range((after_seq + 1)..)? {
            let (k, v) = item?;
            rows.push(Self::body_to_row(k.value(), &decode_body(v.value())?));
        }
        Ok(rows)
    }

    /// Project a stored `EventBody` (+ its seq key) onto the public `EventRow`.
    fn body_to_row(seq: u64, body: &EventBody) -> EventRow {
        EventRow {
            seq,
            ts_ns: body.ts_ns,
            event_type: body.event_type.clone(),
            work_order_id: body.work_order_id.clone(),
            payload_json: body.payload_json.clone(),
            action_hash: body.action_hash,
        }
    }

    /// HFTASK-0032 (ADR-0004 §3.3 rev): roll one source repo's events into THIS (central)
    /// ledger via **append-with-provenance re-chaining** (CT/RFC6962 model). The whole batch
    /// plus the cursor advance commit in ONE transaction (ADR-0017 C-6: crash-safe, both or
    /// neither, with two-phase commit on for the chain-critical batch).
    ///
    /// For each row (ordered by source `seq`):
    /// - Re-append into `EVENTS` re-chaining `prev_hash` onto the CURRENT central tail (read
    ///   inside the tx, like `append()`), allocating a fresh central `seq`. The central
    ///   `action_hash` is recomputed from `(event_type, work_order_id, payload_json)` —
    ///   byte-identical to the source's `action_hash` — and stored in BOTH `action_hash` and
    ///   `origin_action_hash` (the provenance bridge).
    /// - Stamp `origin_repo` = the member dir name, `origin_seq` = the source `seq`, and insert
    ///   `(origin_repo, origin_seq) → seq` into `ORIGIN_INDEX`.
    /// - On a pre-existing `ORIGIN_INDEX` key (already rolled up), SKIP and count it —
    ///   idempotency backstop independent of the cursor (ADR-0017 C-4).
    ///
    /// After the batch, advance `SYNC_CURSOR[origin_repo]` to the max source `seq` seen (incl.
    /// skipped rows), in the SAME transaction. Chains are NEVER merged.
    pub fn rollup_from(
        &mut self,
        origin_repo: &str,
        rows: &[EventRow],
        updated_ns: u64,
    ) -> Result<RollupStat> {
        let mut stat = RollupStat::default();
        if rows.is_empty() {
            return Ok(stat);
        }
        // Not wrapped in with_busy_retry: a partial-batch retry is unnecessary because the
        // whole batch is one tx (all-or-nothing). On contention the single begin_write blocks
        // until the prior writer commits (serializable), as with append.
        let mut tx = self.db.begin_write()?;
        tx.set_two_phase_commit(true); // ADR-0017 C-6: chain-critical batch.
        let committed_tail;
        let committed_prev;
        {
            let mut events = tx.open_table(EVENTS)?;
            let mut origin_idx = tx.open_table(ORIGIN_INDEX)?;
            let mut by_wo = tx.open_multimap_table(BY_WORK_ORDER)?;

            // Authoritative central tail, read INSIDE the write tx: re-chain onto it.
            let (mut tail_seq, mut prev_hash) = Self::read_tail(&events)?;
            let mut max_origin_seq = 0u64;

            for row in rows {
                max_origin_seq = max_origin_seq.max(row.seq);
                // Idempotency: pre-existing (origin_repo, origin_seq) → skip + count.
                if origin_idx.get((origin_repo, row.seq))?.is_some() {
                    stat.skipped_existing += 1;
                    continue;
                }
                // Recompute the central action_hash from the SAME inputs → identical to source.
                let action_hash =
                    hash_action(&row.event_type, &row.work_order_id, &row.payload_json);
                let next_seq = tail_seq + 1;
                let body = EventBody {
                    ts_ns: row.ts_ns,
                    event_type: row.event_type.clone(),
                    work_order_id: row.work_order_id.clone(),
                    payload_json: row.payload_json.clone(),
                    action_hash,
                    prev_hash,
                    origin_repo: Some(origin_repo.to_string()),
                    origin_seq: Some(row.seq),
                    origin_action_hash: Some(action_hash),
                };
                events.insert(next_seq, encode_body(&body).as_slice())?;
                origin_idx.insert((origin_repo, row.seq), next_seq)?;
                by_wo.insert(row.work_order_id.as_str(), next_seq)?;
                // Only a successfully appended row advances the central tail/chain.
                tail_seq = next_seq;
                prev_hash = action_hash;
                stat.appended += 1;
            }

            // Advance the cursor to the max source seq covered by this batch (incl. skips),
            // in the SAME transaction — crash-safe. MAX with any existing value.
            {
                let mut cursor = tx.open_table(SYNC_CURSOR)?;
                let existing = cursor.get(origin_repo)?.map(|v| v.value().0).unwrap_or(0);
                cursor.insert(origin_repo, (existing.max(max_origin_seq), updated_ns))?;
            }
            committed_tail = tail_seq;
            committed_prev = prev_hash;
        }
        tx.commit()?;

        // Keep the in-memory cache consistent with the committed central tail.
        self.seq = committed_tail;
        self.prev_witness_hash = committed_prev;
        Ok(stat)
    }

    /// REPLAY (state-precedence tier 2): reconstruct the latest status per work order id.
    pub fn replay_latest_status(&self) -> Result<Vec<(String, Status)>> {
        let tx = self.db.begin_read()?;
        let events = tx.open_table(EVENTS)?;
        let mut map: std::collections::BTreeMap<String, Status> = Default::default();
        for item in events.iter()? {
            let (_, v) = item?;
            let body = decode_body(v.value())?;
            if body.event_type != "task_transition" {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body.payload_json)
                && let Some(s) = val.get("status")
                && let Ok(st) = serde_json::from_value::<Status>(s.clone())
            {
                map.insert(body.work_order_id, st);
            }
        }
        Ok(map.into_iter().collect())
    }

    /// Verify the witness chain over all events — genuine, fail-closed tamper-evidence.
    /// Returns the number of verified entries, or `WitnessTampered` on the first break.
    ///
    /// HFTASK-0079: the prior implementation was a tautology. It rebuilt a fresh chain from the
    /// *trusted* stored `action_hash` values (forcing `prev_hash` to 0 so `create_witness_chain`
    /// recomputed every link) and then verified that just-built chain — which therefore always
    /// passed, returning `events.len()` for honest, content-tampered, and garbage rows alike, and
    /// `.expect`-panicked on the impossible failure. It never recomputed `action_hash` from the
    /// payload nor checked the on-disk `prev_hash` linkage, so it could not detect tampering of
    /// the binary redb cache.
    ///
    /// This walks events in `seq` order and enforces the SAME two invariants `append`
    /// establishes (v1.rs `append`), so any post-commit edit of the cache fails closed:
    /// 1. **Content** — re-derive `hash_action(event_type, work_order_id, payload_json)` and
    ///    byte-compare to the stored `action_hash`. A mutated payload/type/work-order no longer
    ///    matches its hash.
    /// 2. **Linkage** — the stored `prev_hash` must equal the prior event's stored `action_hash`
    ///    (genesis `[0u8; 32]`), so reordering, deleting, or splicing rows breaks the chain.
    ///
    /// The RVF witness-segment round-trip is retained as a secondary structural check (now over
    /// the REAL stored linkage), but it is no longer the sole — nor a load-bearing — gate.
    pub fn verify_witness_chain(&self) -> Result<usize> {
        let tx = self.db.begin_read()?;
        let events = tx.open_table(EVENTS)?;
        let mut entries: Vec<WitnessEntry> = Vec::new();
        let mut expected_prev = [0u8; 32];
        for item in events.iter()? {
            let (k, v) = item?;
            let seq = k.value();
            let body = decode_body(v.value())?;
            // (1) Content integrity — the load-bearing tamper check.
            let recomputed = hash_action(&body.event_type, &body.work_order_id, &body.payload_json);
            if recomputed != body.action_hash {
                return Err(LedgerError::WitnessTampered {
                    seq,
                    field: "action_hash",
                });
            }
            // (2) Linkage integrity — chains exactly as `append` did.
            if body.prev_hash != expected_prev {
                return Err(LedgerError::WitnessTampered {
                    seq,
                    field: "prev_hash",
                });
            }
            expected_prev = body.action_hash;
            entries.push(WitnessEntry {
                prev_hash: body.prev_hash,
                action_hash: body.action_hash,
                timestamp_ns: body.ts_ns,
                witness_type: 0x02, // COMPUTATION
            });
        }
        // (3) Secondary structural check: RVF witness-segment continuity. Fail-closed, no panic.
        let chain = create_witness_chain(&entries);
        let verified = verify_witness_chain(&chain).map_err(|_| LedgerError::WitnessTampered {
            seq: entries.len() as u64,
            field: "rvf_segment",
        })?;
        Ok(verified.len())
    }

    /// HFTASK-0033 (ADR-0004 §3.3 rev): verify the rollup *provenance bridge*. For every
    /// rolled-up central row (`origin_repo` set), re-derive the action hash from the stored
    /// content via the SAME [`hash_action`] used on append/rollup, and byte-compare it to the
    /// persisted `origin_action_hash`. A match proves the central row IS the source event it
    /// claims to mirror (CT/RFC6962 model). Native (NULL-origin) local events are out of scope.
    pub fn verify_rollup_provenance(&self) -> Result<RollupProvenance> {
        let tx = self.db.begin_read()?;
        let events = tx.open_table(EVENTS)?;
        // Collect rolled rows then sort by (origin_repo, origin_seq) to mirror the old
        // `ORDER BY origin_repo, origin_seq` deterministic per-repo breakdown.
        let mut rolled: Vec<RolledRow> = Vec::new();
        for item in events.iter()? {
            let (_, v) = item?;
            let body = decode_body(v.value())?;
            if let Some(origin_repo) = body.origin_repo {
                rolled.push(RolledRow {
                    origin_repo,
                    origin_seq: body.origin_seq.unwrap_or(0),
                    event_type: body.event_type,
                    work_order_id: body.work_order_id,
                    payload_json: body.payload_json,
                    origin_action_hash: body.origin_action_hash,
                });
            }
        }
        rolled.sort_by(|a, b| {
            a.origin_repo
                .cmp(&b.origin_repo)
                .then(a.origin_seq.cmp(&b.origin_seq))
        });

        let mut prov = RollupProvenance::default();
        let mut per: std::collections::BTreeMap<String, usize> = Default::default();
        for r in rolled {
            let recomputed = hash_action(&r.event_type, &r.work_order_id, &r.payload_json);
            if r.origin_action_hash == Some(recomputed) {
                prov.verified += 1;
                *per.entry(r.origin_repo).or_default() += 1;
            } else {
                prov.mismatched += 1;
            }
        }
        prov.per_repo = per.into_iter().collect();
        Ok(prov)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use work_order::{SwarmBundle, work_orders_from_bundle};

    fn bundle() -> SwarmBundle {
        SwarmBundle {
            workflow_id: "wf-42".to_string(),
            role_prompts: vec![
                ("architect".to_string(), "Design storefront".to_string()),
                ("coder".to_string(), "Build checkout".to_string()),
            ],
            handoff_template: "standard".to_string(),
            consistency_report: vec![],
            evolution_suggestions: vec![],
        }
    }

    // --- HFTASK-0048: atomic in-ledger lease ---------------------------------

    const SEC: u64 = 1_000_000_000;

    #[test]
    fn resolve_lease_free_held_released_expired() {
        // free
        assert_eq!(resolve_lease(&[], 100), None);
        // held within ttl
        let acq = |h: &str, acq: u64, ttl: u64| {
            (
                "lease_acquired".to_string(),
                format!("{{\"holder\":\"{h}\",\"acquired_ns\":{acq},\"ttl_secs\":{ttl}}}"),
            )
        };
        let rel = |h: &str| {
            (
                "lease_released".to_string(),
                format!("{{\"holder\":\"{h}\"}}"),
            )
        };
        let ev = vec![acq("a", 0, 10)];
        assert_eq!(resolve_lease(&ev, 5 * SEC), Some("a".into()));
        // expired
        assert_eq!(resolve_lease(&ev, 20 * SEC), None);
        // released by holder
        let ev = vec![acq("a", 0, 100), rel("a")];
        assert_eq!(resolve_lease(&ev, 5 * SEC), None);
        // a release by a NON-holder does not free it
        let ev = vec![acq("a", 0, 100), rel("b")];
        assert_eq!(resolve_lease(&ev, 5 * SEC), Some("a".into()));
    }

    #[test]
    fn try_acquire_lease_is_atomic_first_holder_wins() {
        let mut led = Ledger::open(":memory:").unwrap();
        let res = "handoff:claim:HFTASK-0048";
        // free → acquired
        assert!(matches!(
            led.try_acquire_lease(res, "alice", 3600, SEC).unwrap(),
            LeaseOutcome::Acquired { .. }
        ));
        // a different holder is refused while alice holds it (no write)
        assert_eq!(
            led.try_acquire_lease(res, "bob", 3600, 2 * SEC).unwrap(),
            LeaseOutcome::Conflict {
                holder: "alice".into()
            }
        );
        // same holder re-acquires → heartbeat (extend)
        assert!(matches!(
            led.try_acquire_lease(res, "alice", 3600, 3 * SEC).unwrap(),
            LeaseOutcome::Heartbeat { .. }
        ));
        assert_eq!(
            led.lease_holder(res, 4 * SEC).unwrap(),
            Some("alice".into())
        );
        // after alice releases, bob can acquire
        led.release_lease(res, "alice", 5 * SEC).unwrap();
        assert_eq!(led.lease_holder(res, 6 * SEC).unwrap(), None);
        assert!(matches!(
            led.try_acquire_lease(res, "bob", 3600, 7 * SEC).unwrap(),
            LeaseOutcome::Acquired { .. }
        ));
        assert_eq!(led.lease_holder(res, 8 * SEC).unwrap(), Some("bob".into()));
    }

    #[test]
    fn try_acquire_lease_respects_ttl_expiry() {
        let mut led = Ledger::open(":memory:").unwrap();
        let res = "r";
        // alice holds a 10s lease at t=0
        led.try_acquire_lease(res, "alice", 10, 0).unwrap();
        // bob is refused before expiry…
        assert_eq!(
            led.try_acquire_lease(res, "bob", 10, 5 * SEC).unwrap(),
            LeaseOutcome::Conflict {
                holder: "alice".into()
            }
        );
        // …but acquires once alice's lease has expired
        assert!(matches!(
            led.try_acquire_lease(res, "bob", 10, 20 * SEC).unwrap(),
            LeaseOutcome::Acquired { .. }
        ));
    }

    #[test]
    fn lease_events_keep_the_witness_chain_intact() {
        let mut led = Ledger::open(":memory:").unwrap();
        led.try_acquire_lease("r", "alice", 60, 1).unwrap();
        led.release_lease("r", "alice", 2).unwrap();
        led.try_acquire_lease("r", "bob", 60, 3).unwrap();
        // the chain over the lease events must verify like any other witnessed history
        assert_eq!(led.verify_witness_chain().unwrap(), 3);
    }

    #[test]
    fn end_to_end_seam_ledger_witness_replay() {
        // 1. front-door seam: SwarmBundle -> provable work orders
        let orders = work_orders_from_bundle(&bundle());
        assert_eq!(orders.len(), 2);

        // 2. ledger: drive each order through a lifecycle, witnessed
        let mut led = Ledger::open(":memory:").unwrap();
        let mut ts = 1_000u64;
        for wo in &orders {
            for st in [Status::Claimed, Status::Checkpointed, Status::Done] {
                led.record_transition(wo, st, ts).unwrap();
                ts += 1;
            }
        }

        // 3. replay -> both orders end at Done
        let latest = led.replay_latest_status().unwrap();
        assert_eq!(latest.len(), 2);
        assert!(latest.iter().all(|(_, s)| *s == Status::Done));

        // 4. tamper-evidence: the RVF witness chain over all events verifies
        let n = led.verify_witness_chain().unwrap();
        assert_eq!(n, 6); // 2 orders x 3 transitions
    }

    /// HFTASK-0079: tampering an event's payload in the binary cache (leaving its `action_hash`
    /// stale) MUST be caught. The old tautological `verify_witness_chain` returned the event
    /// count here; the hardened one fails closed with `WitnessTampered { field: "action_hash" }`.
    #[test]
    fn tampering_a_payload_fails_witness_verification() {
        let mut led = Ledger::open(":memory:").unwrap();
        led.append("checkpoint", "HFTASK-T", "{\"k\":1}", 1)
            .unwrap();
        led.append("checkpoint", "HFTASK-T", "{\"k\":2}", 2)
            .unwrap();
        led.append("checkpoint", "HFTASK-T", "{\"k\":3}", 3)
            .unwrap();
        assert_eq!(led.verify_witness_chain().unwrap(), 3); // honest baseline verifies

        // Mutate seq 2's payload directly in redb; its stored action_hash now no longer matches.
        {
            let tx = led.db.begin_write().unwrap();
            {
                let mut events = tx.open_table(EVENTS).unwrap();
                let bytes = events.get(2u64).unwrap().unwrap().value().to_vec();
                let mut body = decode_body(&bytes).unwrap();
                body.payload_json = "{\"k\":999}".to_string();
                events.insert(2u64, encode_body(&body).as_slice()).unwrap();
            }
            tx.commit().unwrap();
        }
        match led.verify_witness_chain() {
            Err(LedgerError::WitnessTampered { seq, field }) => {
                assert_eq!(seq, 2);
                assert_eq!(field, "action_hash");
            }
            other => panic!("expected WitnessTampered/action_hash, got {other:?}"),
        }
    }

    /// HFTASK-0079: breaking the `prev_hash` linkage (reorder/splice/delete) MUST be caught even
    /// when the row's own content stays self-consistent — fails closed on `field: "prev_hash"`.
    #[test]
    fn breaking_prev_hash_linkage_fails_closed() {
        let mut led = Ledger::open(":memory:").unwrap();
        led.append("checkpoint", "HFTASK-L", "{}", 1).unwrap();
        led.append("checkpoint", "HFTASK-L", "{}", 2).unwrap();
        assert_eq!(led.verify_witness_chain().unwrap(), 2);

        // Corrupt only seq 2's prev_hash; payload/action_hash stay self-consistent (content ok).
        {
            let tx = led.db.begin_write().unwrap();
            {
                let mut events = tx.open_table(EVENTS).unwrap();
                let bytes = events.get(2u64).unwrap().unwrap().value().to_vec();
                let mut body = decode_body(&bytes).unwrap();
                body.prev_hash = [9u8; 32]; // no longer == seq 1's action_hash
                events.insert(2u64, encode_body(&body).as_slice()).unwrap();
            }
            tx.commit().unwrap();
        }
        match led.verify_witness_chain() {
            Err(LedgerError::WitnessTampered { seq, field }) => {
                assert_eq!(seq, 2);
                assert_eq!(field, "prev_hash");
            }
            other => panic!("expected WitnessTampered/prev_hash, got {other:?}"),
        }
    }

    /// Isolated temp dir for a file-backed ledger (NEVER the real .handoff/ledger.db).
    fn temp_db() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let uniq = format!(
            "hf-ledger-test-{}-{:?}-{}",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        p.push(uniq);
        std::fs::create_dir_all(&p).unwrap();
        p.push("ledger.redb");
        p
    }

    /// ADR-0017 C-2 / HFTASK-0028 AC1+AC2: many concurrent writers append to the SAME file
    /// ledger and the witness chain still verifies end-to-end with a contiguous prev_hash chain
    /// (no fork, no duplicate/missing seq).
    ///
    /// redb's cross-process exclusion (OS file lock) forbids two open write handles to the same
    /// file — even in-process — so the original "each thread opens its own `Ledger`" model
    /// cannot be expressed for an *embedded* store the way SQLite's WAL allowed. The integrity
    /// guarantee being proven (serialized seq allocation + non-forked chain under concurrency)
    /// is preserved by sharing ONE `Database` across the writer threads: redb's `begin_write()`
    /// is a serializable single-writer critical section, so the threads serialize exactly as
    /// the old separate-handle writers did. A *second* `Ledger::open` on the locked file is
    /// also asserted to be excluded (the cross-process contract), so the multi-process safety
    /// the original test guarded is documented and tested below, not deleted.
    #[test]
    fn concurrent_writers_serialize_no_lock_no_fork() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let db = temp_db();
        let path = db.to_string_lossy().into_owned();

        // One shared ledger handle behind a Mutex: each `append` is its own serializable
        // begin_write critical section, so the Mutex only bounds the &mut self borrow — the
        // chaining atomicity is redb's, not the Mutex's.
        let led = Arc::new(Mutex::new(Ledger::open(&path).unwrap()));

        const WRITERS: usize = 8;
        const PER_WRITER: usize = 25;

        let mut handles = vec![];
        for w in 0..WRITERS {
            let led = Arc::clone(&led);
            handles.push(thread::spawn(move || {
                for i in 0..PER_WRITER {
                    let ts = (w as u64) * 1_000_000 + i as u64;
                    led.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .append("checkpoint", &format!("HFTASK-W{w}"), "{}", ts)
                        .expect("append must not fail under concurrency");
                }
            }));
        }
        for h in handles {
            h.join().expect("writer thread panicked");
        }

        // Cross-process contract (C-2): while this process holds the write handle, a second
        // open of the SAME file for write is excluded by redb's OS file lock.
        assert!(
            matches!(
                Ledger::open(&path),
                Err(LedgerError::Database(
                    redb::DatabaseError::DatabaseAlreadyOpen
                ))
            ),
            "a second open of a locked ledger must be excluded"
        );

        let led = Arc::try_unwrap(led).ok().unwrap().into_inner().unwrap();

        // AC1: every event landed (no lost writes, no errors).
        let events = led.all_events().unwrap();
        assert_eq!(
            events.len(),
            WRITERS * PER_WRITER,
            "all concurrent appends must land"
        );

        // seqs are a contiguous 1..=N with no gaps/dupes (serialized allocation).
        for (idx, ev) in events.iter().enumerate() {
            assert_eq!(ev.seq, idx as u64 + 1, "seq must be contiguous (no fork)");
        }

        // AC2: the witness chain verifies over the full count.
        let verified = led.verify_witness_chain().unwrap();
        assert_eq!(verified, WRITERS * PER_WRITER);

        // AC2 (stronger): the stored prev_hash chain is contiguous — each row's prev_hash
        // equals the previous row's action_hash, so no two writers chained off the same prev.
        let bodies: Vec<EventBody> = {
            let tx = led.db.begin_read().unwrap();
            let table = tx.open_table(EVENTS).unwrap();
            table
                .iter()
                .unwrap()
                .map(|r| decode_body(r.unwrap().1.value()).unwrap())
                .collect()
        };
        let mut expected_prev = [0u8; 32];
        for body in &bodies {
            assert_eq!(
                body.prev_hash, expected_prev,
                "prev_hash chain must be contiguous"
            );
            expected_prev = body.action_hash;
        }

        drop(led);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// ADR-0017 C-2 (the cross-process exclusion, isolated): two `Ledger::open` calls on the
    /// same file cannot both hold the write handle — the second is excluded (OS file lock).
    /// This is the direct analogue of the SQLite single-writer guarantee.
    #[test]
    fn second_open_is_excluded_single_writer() {
        let db = temp_db();
        let path = db.to_string_lossy().into_owned();
        let first = Ledger::open(&path).unwrap();
        assert!(
            matches!(
                Ledger::open(&path),
                Err(LedgerError::Database(
                    redb::DatabaseError::DatabaseAlreadyOpen
                ))
            ),
            "expected DatabaseAlreadyOpen"
        );
        drop(first);
        // Once released, a fresh open succeeds.
        let _second = Ledger::open(&path).unwrap();
        drop(_second);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// ADR-0017: a fresh open creates all tables; re-opening is idempotent (the redb analogue
    /// of `migration_is_idempotent` / `fresh_open_creates_provenance_schema`). Native appends
    /// are unconstrained by the origin index (the partial-unique semantics).
    #[test]
    fn fresh_open_creates_schema_and_native_appends_unconstrained() {
        let db = temp_db();
        let path = db.to_string_lossy().into_owned();
        {
            let mut led = Ledger::open(&path).unwrap();
            // Two native appends for the same work order must not collide (no origin index entry).
            led.append("checkpoint", "HFTASK-X", "{}", 10).unwrap();
            led.append("checkpoint", "HFTASK-X", "{}", 11).unwrap();
            assert_eq!(led.all_events().unwrap().len(), 2);
        }
        // Re-open (idempotent table creation) and verify the prior data + chain survive.
        let led = Ledger::open(&path).unwrap();
        assert_eq!(led.all_events().unwrap().len(), 2);
        assert_eq!(led.verify_witness_chain().unwrap(), 2);
        drop(led);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// ADR-0017 (port of `old_schema_db_migrates_and_still_verifies`): a previously-written
    /// redb ledger re-opens with no data loss and still verifies; new appends keep the chain
    /// intact and stay native (NULL-origin). (The SQLite "old schema" case becomes "open an
    /// existing redb DB" — there is no column-level migration in a typed KV store.)
    #[test]
    fn existing_db_reopens_and_still_verifies() {
        let db = temp_db();
        let path = db.to_string_lossy().into_owned();
        // 1. Build a populated ledger and close it.
        {
            let mut led = Ledger::open(&path).unwrap();
            for (et, wo, pl) in [
                ("task_transition", "HFTASK-OLD", "{\"status\":\"Claimed\"}"),
                ("checkpoint", "HFTASK-OLD", "{}"),
                ("task_transition", "HFTASK-OLD", "{\"status\":\"Done\"}"),
            ] {
                led.append(et, wo, pl, 1_000).unwrap();
            }
        }
        // 2. Re-open: no data loss + the witness chain verifies over the full count.
        let mut led = Ledger::open(&path).unwrap();
        assert_eq!(led.all_events().unwrap().len(), 3, "no rows lost");
        assert_eq!(led.verify_witness_chain().unwrap(), 3);
        // 3. All pre-existing rows are native (no origin) → provenance is vacuously faithful.
        assert_eq!(led.verify_rollup_provenance().unwrap().total(), 0);
        // 4. append() still works and stays native.
        led.append("checkpoint", "HFTASK-OLD", "{}", 2_000).unwrap();
        assert_eq!(led.verify_witness_chain().unwrap(), 4);
        assert_eq!(led.verify_rollup_provenance().unwrap().total(), 0);
        drop(led);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// HFTASK-0031 AC4: the sync_cursor get/set helper round-trips (None before set, value
    /// after, upsert overwrites).
    #[test]
    fn sync_cursor_get_set_round_trips() {
        let db = temp_db();
        let mut led = Ledger::open(db.to_str().unwrap()).unwrap();

        assert_eq!(led.sync_cursor_get("repo-a").unwrap(), None, "unset = None");

        led.sync_cursor_set("repo-a", 7, 111).unwrap();
        assert_eq!(led.sync_cursor_get("repo-a").unwrap(), Some(7));

        // Upsert: a later sync advances the high-water mark for the same repo.
        led.sync_cursor_set("repo-a", 12, 222).unwrap();
        assert_eq!(led.sync_cursor_get("repo-a").unwrap(), Some(12));

        // Distinct repos are independent rows.
        led.sync_cursor_set("repo-b", 3, 333).unwrap();
        assert_eq!(led.sync_cursor_get("repo-b").unwrap(), Some(3));
        assert_eq!(led.sync_cursor_get("repo-a").unwrap(), Some(12));

        drop(led);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    // ----- HFTASK-0032: rollup (append-with-provenance re-chaining) ---------

    /// Build a real source ledger with `n` native events and return (its temp dir, path).
    fn source_ledger_with(n: usize, wo_prefix: &str) -> (std::path::PathBuf, String) {
        let db = temp_db();
        let path = db.to_string_lossy().into_owned();
        {
            let mut led = Ledger::open(&path).unwrap();
            for i in 0..n {
                led.append(
                    "checkpoint",
                    &format!("{wo_prefix}-{i}"),
                    "{}",
                    1_000 + i as u64,
                )
                .unwrap();
            }
        }
        (db, path)
    }

    /// HFTASK-0032 AC1+AC4: roll two source repos into a central ledger; provenance is
    /// faithful; central verifies the full combined count; each source verifies alone.
    #[test]
    fn rollup_two_sources_provenance_and_combined_chain() {
        let central_dir = temp_db();
        let central_path = central_dir.to_string_lossy().into_owned();

        let (src_a_dir, src_a) = source_ledger_with(3, "A");
        let (src_b_dir, src_b) = source_ledger_with(2, "B");

        // The source rows (with their own action_hash) the rollup will consume.
        let rows_a = Ledger::open(&src_a).unwrap().events_after(0).unwrap();
        let rows_b = Ledger::open(&src_b).unwrap().events_after(0).unwrap();

        let mut central = Ledger::open(&central_path).unwrap();
        let sa = central.rollup_from("repo-a", &rows_a, 1).unwrap();
        let sb = central.rollup_from("repo-b", &rows_b, 2).unwrap();
        assert_eq!(
            sa,
            RollupStat {
                appended: 3,
                skipped_existing: 0
            }
        );
        assert_eq!(
            sb,
            RollupStat {
                appended: 2,
                skipped_existing: 0
            }
        );

        // Central verifies over the full combined count (3 + 2 = 5).
        assert_eq!(central.verify_witness_chain().unwrap(), 5);
        assert_eq!(central.all_events().unwrap().len(), 5);

        // PROVENANCE faithful (AC4): the verifier confirms every rolled row, and the source
        // action_hash equals the recomputed hash equals the stored provenance hash.
        let prov = central.verify_rollup_provenance().unwrap();
        assert!(prov.is_faithful());
        assert_eq!(prov.verified, 5);
        for (repo, src_rows) in [("repo-a", &rows_a), ("repo-b", &rows_b)] {
            for src in src_rows {
                let recomputed =
                    hash_action(&src.event_type, &src.work_order_id, &src.payload_json);
                assert_eq!(src.action_hash, recomputed, "source ah == recomputed");
            }
            let _ = repo;
        }

        // Each source chain still verifies independently.
        assert_eq!(
            Ledger::open(&src_a)
                .unwrap()
                .verify_witness_chain()
                .unwrap(),
            3
        );
        assert_eq!(
            Ledger::open(&src_b)
                .unwrap()
                .verify_witness_chain()
                .unwrap(),
            2
        );

        // Cursors advanced to each source's max seq.
        assert_eq!(central.sync_cursor_get("repo-a").unwrap(), Some(3));
        assert_eq!(central.sync_cursor_get("repo-b").unwrap(), Some(2));

        drop(central);
        for d in [central_dir, src_a_dir, src_b_dir] {
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    /// HFTASK-0032 AC2: idempotent — re-rolling the same rows appends 0, skips all, leaves
    /// the central count and cursor unchanged.
    #[test]
    fn rollup_is_idempotent() {
        let central_dir = temp_db();
        let central_path = central_dir.to_string_lossy().into_owned();
        let (src_dir, src) = source_ledger_with(4, "I");
        let rows = Ledger::open(&src).unwrap().events_after(0).unwrap();

        let mut central = Ledger::open(&central_path).unwrap();
        let first = central.rollup_from("repo", &rows, 1).unwrap();
        assert_eq!(
            first,
            RollupStat {
                appended: 4,
                skipped_existing: 0
            }
        );
        let count_after_first = central.all_events().unwrap().len();
        let cursor_after_first = central.sync_cursor_get("repo").unwrap();

        // Re-run with the SAME rows (simulates `hf sync` run twice without the cursor gate).
        let second = central.rollup_from("repo", &rows, 2).unwrap();
        assert_eq!(
            second,
            RollupStat {
                appended: 0,
                skipped_existing: 4
            }
        );
        assert_eq!(
            central.all_events().unwrap().len(),
            count_after_first,
            "count unchanged"
        );
        assert_eq!(
            central.sync_cursor_get("repo").unwrap(),
            cursor_after_first,
            "cursor unchanged"
        );
        assert_eq!(central.verify_witness_chain().unwrap(), 4);

        drop(central);
        for d in [central_dir, src_dir] {
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    /// HFTASK-0032 AC3: incremental — append M new source events, the cursor-driven pull
    /// (`events_after(cursor)`) rolls up exactly M.
    #[test]
    fn rollup_is_incremental_via_cursor() {
        let central_dir = temp_db();
        let central_path = central_dir.to_string_lossy().into_owned();
        let src_dir = temp_db();
        let src = src_dir.to_string_lossy().into_owned();

        // 3 initial source events → roll up.
        {
            let mut s = Ledger::open(&src).unwrap();
            for i in 0..3 {
                s.append("checkpoint", "WO", "{}", 100 + i).unwrap();
            }
        }
        let mut central = Ledger::open(&central_path).unwrap();
        let cursor0 = central.sync_cursor_get("repo").unwrap().unwrap_or(0);
        let rows0 = Ledger::open(&src).unwrap().events_after(cursor0).unwrap();
        assert_eq!(central.rollup_from("repo", &rows0, 1).unwrap().appended, 3);

        // 2 MORE source events.
        {
            let mut s = Ledger::open(&src).unwrap();
            for i in 0..2 {
                s.append("checkpoint", "WO", "{}", 200 + i).unwrap();
            }
        }
        // Cursor-driven pull: only the 2 new events come back, and exactly 2 roll up.
        let cursor1 = central.sync_cursor_get("repo").unwrap().unwrap();
        assert_eq!(cursor1, 3, "cursor at first batch max");
        let rows1 = Ledger::open(&src).unwrap().events_after(cursor1).unwrap();
        assert_eq!(
            rows1.len(),
            2,
            "events_after(cursor) returns only the new ones"
        );
        let stat1 = central.rollup_from("repo", &rows1, 2).unwrap();
        assert_eq!(
            stat1,
            RollupStat {
                appended: 2,
                skipped_existing: 0
            }
        );
        assert_eq!(central.all_events().unwrap().len(), 5);
        assert_eq!(central.sync_cursor_get("repo").unwrap(), Some(5));
        assert_eq!(central.verify_witness_chain().unwrap(), 5);

        drop(central);
        for d in [central_dir, src_dir] {
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    /// HFTASK-0032 AC6: native append still works alongside rolled rows — a NULL-origin
    /// event re-chains onto the central tail (incl. rolled rows) and the chain verifies.
    #[test]
    fn native_append_after_rollup_still_verifies() {
        let central_dir = temp_db();
        let central_path = central_dir.to_string_lossy().into_owned();
        let (src_dir, src) = source_ledger_with(2, "N");
        let rows = Ledger::open(&src).unwrap().events_after(0).unwrap();

        let mut central = Ledger::open(&central_path).unwrap();
        central.rollup_from("repo", &rows, 1).unwrap();
        // Native checkpoint on the central ledger after rollup.
        central
            .append("checkpoint", "CENTRAL-NATIVE", "{}", 9_000)
            .unwrap();

        assert_eq!(central.verify_witness_chain().unwrap(), 3);
        // The native event is NULL-origin; the rolled ones are not → provenance total is 2.
        assert_eq!(central.verify_rollup_provenance().unwrap().total(), 2);

        drop(central);
        for d in [central_dir, src_dir] {
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    /// Empty batch is a no-op (no cursor write, no rows).
    #[test]
    fn rollup_empty_is_noop() {
        let central_dir = temp_db();
        let mut central = Ledger::open(central_dir.to_str().unwrap()).unwrap();
        let stat = central.rollup_from("repo", &[], 1).unwrap();
        assert_eq!(stat, RollupStat::default());
        assert_eq!(central.all_events().unwrap().len(), 0);
        assert_eq!(central.sync_cursor_get("repo").unwrap(), None);
        drop(central);
        let _ = std::fs::remove_dir_all(central_dir.parent().unwrap());
    }

    // ----- HFTASK-0033: verify_rollup_provenance (the provenance bridge) ----

    /// HFTASK-0033 AC: after rolling two sources up, `verify_rollup_provenance` confirms
    /// every rolled row's recomputed hash matches its stored `origin_action_hash`; the
    /// per-repo breakdown counts each source; native rows are out of scope.
    #[test]
    fn verify_rollup_provenance_is_faithful_and_per_repo() {
        let central_dir = temp_db();
        let central_path = central_dir.to_string_lossy().into_owned();
        let (src_a_dir, src_a) = source_ledger_with(3, "A");
        let (src_b_dir, src_b) = source_ledger_with(2, "B");
        let rows_a = Ledger::open(&src_a).unwrap().events_after(0).unwrap();
        let rows_b = Ledger::open(&src_b).unwrap().events_after(0).unwrap();

        let mut central = Ledger::open(&central_path).unwrap();
        // A native local event on the central ledger too — must be IGNORED by the verifier.
        central
            .append("checkpoint", "CENTRAL-NATIVE", "{}", 1)
            .unwrap();
        central.rollup_from("repo-a", &rows_a, 1).unwrap();
        central.rollup_from("repo-b", &rows_b, 2).unwrap();

        let prov = central.verify_rollup_provenance().unwrap();
        assert!(prov.is_faithful(), "all rolled rows must verify: {prov:?}");
        assert_eq!(prov.verified, 5, "3 (repo-a) + 2 (repo-b) rolled rows");
        assert_eq!(prov.mismatched, 0);
        assert_eq!(prov.total(), 5, "native CENTRAL-NATIVE row is out of scope");
        assert_eq!(
            prov.per_repo,
            vec![("repo-a".to_string(), 3), ("repo-b".to_string(), 2)],
            "per-repo breakdown sorted by origin_repo"
        );

        drop(central);
        for d in [central_dir, src_a_dir, src_b_dir] {
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    /// HFTASK-0033 AC (the failure direction): if a rolled row's content is tampered so it
    /// no longer reproduces its `origin_action_hash`, the verifier flags the mismatch and
    /// `is_faithful()` is false — the provenance bridge is broken. (We tamper by rewriting the
    /// stored body's payload_json directly in redb, leaving origin_action_hash unchanged.)
    #[test]
    fn verify_rollup_provenance_detects_tampered_row() {
        let central_dir = temp_db();
        let central_path = central_dir.to_string_lossy().into_owned();
        let (src_dir, src) = source_ledger_with(3, "T");
        let rows = Ledger::open(&src).unwrap().events_after(0).unwrap();

        let mut central = Ledger::open(&central_path).unwrap();
        central.rollup_from("repo-t", &rows, 1).unwrap();
        assert!(central.verify_rollup_provenance().unwrap().is_faithful());

        // Tamper ONE rolled row (origin_seq == 2) by rewriting its stored payload_json without
        // touching origin_action_hash: the recomputed hash now diverges from the stored one.
        {
            // Find the central seq of the rolled (repo-t, origin_seq=2) row, then rewrite it.
            let target_seq = {
                let tx = central.db.begin_read().unwrap();
                let idx = tx.open_table(ORIGIN_INDEX).unwrap();
                idx.get(("repo-t", 2u64)).unwrap().unwrap().value()
            };
            let tx = central.db.begin_write().unwrap();
            {
                let mut events = tx.open_table(EVENTS).unwrap();
                let mut body =
                    decode_body(events.get(target_seq).unwrap().unwrap().value()).unwrap();
                body.payload_json = "{\"tampered\":true}".to_string();
                events
                    .insert(target_seq, encode_body(&body).as_slice())
                    .unwrap();
            }
            tx.commit().unwrap();
        }

        let prov = central.verify_rollup_provenance().unwrap();
        assert!(
            !prov.is_faithful(),
            "tampered provenance must NOT be faithful"
        );
        assert_eq!(prov.mismatched, 1, "exactly the tampered row mismatches");
        assert_eq!(prov.verified, 2, "the other two rolled rows still verify");

        drop(central);
        for d in [central_dir, src_dir] {
            let _ = std::fs::remove_dir_all(d.parent().unwrap());
        }
    }

    /// A ledger with only native (NULL-origin) events has no rollup rows, so provenance is
    /// vacuously faithful (`total() == 0`, `is_faithful()` true).
    #[test]
    fn verify_rollup_provenance_vacuous_when_no_rollups() {
        let db = temp_db();
        let mut led = Ledger::open(db.to_str().unwrap()).unwrap();
        led.append("checkpoint", "NATIVE", "{}", 1).unwrap();
        led.append("task_transition", "NATIVE", "{\"status\":\"Done\"}", 2)
            .unwrap();
        let prov = led.verify_rollup_provenance().unwrap();
        assert!(prov.is_faithful());
        assert_eq!(prov.total(), 0, "no origin_repo rows to verify");
        assert!(prov.per_repo.is_empty());
        drop(led);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    // ----- ADR-0017: differential / golden chain test ----------------------

    /// ADR-0017 acceptance: a shared event stream produces a deterministic, hand-verifiable
    /// `seq`/`action_hash`/`prev_hash` chain. We assert the chain is self-consistent (each
    /// prev_hash == the previous action_hash, genesis prev == zero) AND that the action_hashes
    /// equal an INDEPENDENT recomputation via `hash_action` — the golden chain. Because
    /// `hash_action` is the backend-agnostic primitive shared with the (legacy) SQLite path,
    /// an identical stream yields byte-identical hashes on either backend.
    #[test]
    fn golden_chain_is_deterministic_and_self_consistent() {
        let stream = [
            ("task_transition", "WO-1", "{\"status\":\"Claimed\"}"),
            ("checkpoint", "WO-1", "{\"note\":\"a\"}"),
            ("lease_acquired", "WO-1", "{\"holder\":\"x\"}"),
            ("task_transition", "WO-1", "{\"status\":\"Done\"}"),
        ];

        let mut led = Ledger::open(":memory:").unwrap();
        for (i, (et, wo, pl)) in stream.iter().enumerate() {
            let seq = led.append(et, wo, pl, 1_000 + i as u64).unwrap();
            assert_eq!(seq, i as u64 + 1, "seq is dense 1..=N");
        }

        // Pull the stored bodies in seq order and check the golden chain.
        let bodies: Vec<EventBody> = {
            let tx = led.db.begin_read().unwrap();
            let table = tx.open_table(EVENTS).unwrap();
            table
                .iter()
                .unwrap()
                .map(|r| decode_body(r.unwrap().1.value()).unwrap())
                .collect()
        };
        assert_eq!(bodies.len(), stream.len());

        let mut expected_prev = [0u8; 32];
        for (body, (et, wo, pl)) in bodies.iter().zip(stream.iter()) {
            // action_hash is exactly the independent recomputation (golden).
            let golden = hash_action(et, wo, pl);
            assert_eq!(body.action_hash, golden, "action_hash matches hash_action");
            // prev_hash chains the previous action_hash (genesis = zero).
            assert_eq!(body.prev_hash, expected_prev, "prev_hash chains the tail");
            expected_prev = body.action_hash;
        }

        // And the witness chain verifies over the whole stream.
        assert_eq!(led.verify_witness_chain().unwrap(), stream.len());
    }
}
