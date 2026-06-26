//! RVF vector-native ledger v2.
//!
//! Hybrid design: the pure-Rust redb-backed v1 store remains the authoritative structured
//! event ledger (append, replay, witness chain, lease state, rollup provenance). An RVF vector
//! store is layered on top for semantic recall over session history via HNSW indexing.
//!
//! Vectors: 384-dim, cosine metric. Embeddings are deterministic hash-based pseudo-embeddings
//! so the crate needs no external model or network access.

use std::collections::HashSet;
use std::path::Path;

use rvf_runtime::{
    RvfStore,
    options::{DistanceMetric, MetadataEntry, MetadataValue, QueryOptions, RvfOptions},
};

use crate::v1;

pub use crate::v1::{
    EventRow, LeaseOutcome, LedgerError, Result, RollupProvenance, RollupStat,
    file_is_legacy_sqlite, hash_action, resolve_lease,
};

/// v2 ledger: v1 structured storage + RVF vector overlay for semantic recall.
pub struct Ledger {
    v1: v1::Ledger,
    store: RvfStore,
    dim: usize,
}

const DIM: usize = 384;

/// Field ids for RVF metadata attached to each event vector.
mod meta_fields {
    pub const EVENT_TYPE: u16 = 1;
    pub const WORK_ORDER_ID: u16 = 2;
    pub const PAYLOAD_JSON: u16 = 3;
    pub const TS_NS: u16 = 4;
}

/// Hash-based deterministic pseudo-embedding for event content.
///
/// The result is a 384-dim vector in [-1, 1] derived from a SHA3-256 hash of the event
/// components. Same inputs always produce the same vector, and small input changes produce
/// uncorrelated vectors, which is sufficient for similarity grouping of session events.
pub fn encode_event_to_vector(event_type: &str, work_order_id: &str, payload: &str) -> Vec<f32> {
    use sha3::{Digest, Sha3_256};
    let combined = format!("{}:{}:{}", event_type, work_order_id, payload);
    let hash = Sha3_256::digest(combined.as_bytes());
    hash.iter()
        .map(|b| *b as f32 / 128.0 - 1.0)
        .chain(std::iter::repeat(0.0f32))
        .take(DIM)
        .collect()
}

fn rvf_path(path: &str) -> std::path::PathBuf {
    Path::new(path).with_extension("db.rvf")
}

fn rvf_err(e: rvf_types::RvfError) -> LedgerError {
    LedgerError::Overlay(e.to_string())
}

/// True if an RVF error is transient lock contention (another writer holds/held the sidecar
/// lock) — the RVF analogue of SQLITE_BUSY, safe to retry.
fn is_rvf_lock_contention(e: &rvf_types::RvfError) -> bool {
    matches!(
        e,
        rvf_types::RvfError::Code(rvf_types::ErrorCode::LockHeld)
            | rvf_types::RvfError::Code(rvf_types::ErrorCode::LockStale)
    )
}

// --- HFTASK-0062: provably-dead RVF lock reclaim ------------------------------------------
//
// RVF's own `WriterLock` (rvf-runtime/src/locking.rs, spec 09) only breaks a stale lock when
// the holder PID is dead AND the lock is older than 30s. A *freshly* orphaned lock (holder
// died < 30s ago) therefore returns `LockHeld` and wedges every `hf` call for up to 30s —
// `acquire_store`'s ~1s retry window never outlasts it. That is the liveness bug: a dead
// holder must NEVER wedge the kernel (no-human-in-the-loop). handoff reads the documented
// 104-byte lock format ONLY to reclaim a *provably-dead same-host* holder immediately, and
// fails closed (refuses) on a live, cross-host, or unparseable holder.

/// RVF advisory lock format: magic "RVLF"@0x00 (LE u32), holder PID@0x04 (LE u32),
/// hostname@0x08..0x48 (null-terminated), timestamp_ns@0x48..0x50.
const RVF_LOCK_MAGIC: u32 = 0x5256_4C46; // "RVLF"
const RVF_LOCK_SIZE: usize = 104;

fn rvf_lock_path(rvf: &Path) -> std::path::PathBuf {
    let mut p = rvf.as_os_str().to_os_string();
    p.push(".lock");
    std::path::PathBuf::from(p)
}

/// This host's name, resolved exactly as rvf-runtime does so the same-host comparison agrees.
fn current_hostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| {
        std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "unknown".into())
            .trim()
            .to_string()
    })
}

/// True if `pid` is a live process on THIS host. Unix: `kill(pid, 0)` == 0 (exists) or errno
/// EPERM (exists, different user). Windows: `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`
/// succeeds for live/queryable processes; `ERROR_INVALID_PARAMETER` means the process does not
/// exist. A pid of 0 or one that would cast to a negative `i32` on Unix is never our format →
/// treated as alive so we REFUSE to reclaim (fail-closed). Unknown probe errors also fail closed.
// The only unsafe in the workspace: audited FFI process-liveness probes (no memory-safety
// surface — signal 0 / a query-only handle, with the pid range guarded above). The workspace
// lint policy is `unsafe_code = "deny"`; this is the single justified exception.
#[allow(unsafe_code)]
fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return true; // unverifiable / unsafe to probe → fail-closed (do not reclaim)
    }
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        // SAFETY: signal 0 performs only an existence/permission check; no signal is sent,
        // and `pid` is guaranteed in `1..=i32::MAX` by the guard above (never a group/-1).
        let ret = unsafe { kill(pid as i32, 0) };
        if ret == 0 {
            return true;
        }
        // EPERM (1) => process exists but belongs to another user => still alive.
        std::io::Error::last_os_error().raw_os_error() == Some(1)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, GetLastError,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        // SAFETY: OpenProcess only asks the OS for a query handle. We close a non-null handle
        // immediately and treat ambiguous failures as live so lock reclaim remains fail-closed.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !handle.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return true;
        }
        match unsafe { GetLastError() } {
            ERROR_INVALID_PARAMETER => false,
            ERROR_ACCESS_DENIED => true,
            _ => true,
        }
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        true
    }
}

/// The outcome of inspecting (and possibly reclaiming) the RVF lock for `rvf`.
#[derive(Debug, PartialEq, Eq)]
enum LockReclaim {
    /// No lock file present (nothing to reclaim here).
    NoLock,
    /// A provably-dead same-host holder — the lock file was removed.
    Reclaimed { pid: u32 },
    /// The holder PID is alive — refuse to steal it (fail-closed).
    RefusedLive { pid: u32 },
    /// Cross-host, malformed, or wrong-magic — cannot prove death → refuse (fail-closed).
    RefusedUnverifiable,
}

/// Reclaim the RVF lock IFF its holder is provably dead on this host. Never touches a live,
/// cross-host (can't probe a remote PID), or unrecognized-format lock — the magic is validated
/// before any offset is trusted.
fn inspect_lock(rvf: &Path) -> LockReclaim {
    let lock = rvf_lock_path(rvf);
    let content = match std::fs::read(&lock) {
        Ok(c) => c,
        Err(_) => return LockReclaim::NoLock, // absent/unreadable → nothing to reclaim
    };
    if content.len() < RVF_LOCK_SIZE {
        return LockReclaim::RefusedUnverifiable;
    }
    let magic = u32::from_le_bytes([content[0], content[1], content[2], content[3]]);
    if magic != RVF_LOCK_MAGIC {
        return LockReclaim::RefusedUnverifiable;
    }
    let pid = u32::from_le_bytes([content[4], content[5], content[6], content[7]]);
    let host_field = &content[0x08..0x48];
    let host_end = host_field
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(host_field.len());
    let host = String::from_utf8_lossy(&host_field[..host_end]).into_owned();
    if host != current_hostname() {
        return LockReclaim::RefusedUnverifiable; // can't verify a remote PID → fail-closed
    }
    if pid_is_alive(pid) {
        return LockReclaim::RefusedLive { pid };
    }
    // Provably-dead same-host holder: reclaim. A lost race (file already gone) is fine — the
    // open retry will see whatever state results.
    match std::fs::remove_file(&lock) {
        Ok(()) => LockReclaim::Reclaimed { pid },
        Err(_) => LockReclaim::RefusedUnverifiable,
    }
}

/// Acquire the RVF sidecar store, retrying on transient lock contention and reclaiming a
/// provably-dead holder. Returns the store plus `Some(pid)` when a dead holder's lock was
/// reclaimed (so the caller can witness it).
///
/// HFTASK-0060 (sibling of HFTASK-0059): the SQLite path got `with_busy_retry`, but the RVF
/// sidecar open did NOT — so two `hf` processes touching the same ledger back-to-back (a
/// session + a checkpoint hook, or rapid CLI calls) surfaced `0x0300 LockHeld` ("another
/// writer holds the lock") as a hard error, which hf call sites `.unwrap()`-ed into a panic.
/// Retry open/create on LockHeld/LockStale with a short capped linear backoff.
///
/// HFTASK-0062: before each backoff, reclaim the lock if its holder is provably dead (rvf only
/// breaks dead locks older than 30s, so a freshly orphaned lock would otherwise wedge every
/// call meanwhile). A live/cross-host/unverifiable holder is refused — a genuinely stuck *live*
/// lock still surfaces after the attempt cap. The RVF store is a best-effort recall sidecar
/// (the v1 SQLite store is authoritative), so a bounded wait/reclaim never risks the chain.
fn acquire_store(rvf: &Path) -> std::result::Result<(RvfStore, Option<u32>), rvf_types::RvfError> {
    const MAX_ATTEMPTS: u32 = 100;
    let mut attempt: u32 = 0;
    let mut reclaimed_pid: Option<u32> = None;
    loop {
        let res = if rvf.exists() {
            RvfStore::open(rvf)
        } else {
            RvfStore::create(
                rvf,
                RvfOptions {
                    dimension: DIM as u16,
                    metric: DistanceMetric::Cosine,
                    ..Default::default()
                },
            )
        };
        match res {
            Ok(store) => return Ok((store, reclaimed_pid)),
            Err(e) if is_rvf_lock_contention(&e) && attempt + 1 < MAX_ATTEMPTS => {
                match inspect_lock(rvf) {
                    LockReclaim::Reclaimed { pid } => {
                        reclaimed_pid = Some(pid);
                        // dead lock removed → retry immediately, no backoff.
                    }
                    LockReclaim::NoLock
                    | LockReclaim::RefusedLive { .. }
                    | LockReclaim::RefusedUnverifiable => {
                        attempt += 1;
                        std::thread::sleep(std::time::Duration::from_millis(
                            (attempt as u64).min(10),
                        ));
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
}

impl Ledger {
    /// Open or create the ledger.
    ///
    /// The v1 SQLite store lives at `path`. The RVF vector sidecar lives at `{path}.rvf`.
    /// If the RVF sidecar does not exist, it is created. If opening the RVF sidecar fails,
    /// the call returns an error so callers can fall back to the v1 feature if desired.
    pub fn open(path: &str) -> Result<Self> {
        let mut v1 = v1::Ledger::open(path)?;
        let rvf = rvf_path(path);
        // HFTASK-0060: retry the sidecar acquisition on transient RVF lock contention
        // (0x0300 LockHeld) — the RVF analogue of the SQLite busy-retry (HFTASK-0059).
        // HFTASK-0062: reclaim a provably-dead holder immediately and witness it.
        let (store, reclaimed) = acquire_store(&rvf).map_err(rvf_err)?;
        if let Some(dead_pid) = reclaimed {
            // A reclaim is a real state change — witness it on the authoritative v1 chain.
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let payload = serde_json::json!({
                "rvf": rvf.to_string_lossy(),
                "dead_pid": dead_pid,
                "host": current_hostname(),
            })
            .to_string();
            // Best-effort witness: never fail an open because the reclaim note couldn't write.
            let _ = v1.append("lock_reclaimed", "SYSTEM", &payload, ts);
        }
        Ok(Self {
            v1,
            store,
            dim: DIM,
        })
    }

    /// Append a witnessed event to the structured ledger and ingest its vector into RVF.
    ///
    /// The SQLite append is authoritative. The RVF ingest is best-effort: if it fails the
    /// event is still durably recorded and can be re-embedded on a later open if needed.
    pub fn append(
        &mut self,
        event_type: &str,
        work_order_id: &str,
        payload_json: &str,
        ts_ns: u64,
    ) -> Result<u64> {
        let seq = self
            .v1
            .append(event_type, work_order_id, payload_json, ts_ns)?;
        let embedding = encode_event_to_vector(event_type, work_order_id, payload_json);
        let metadata = vec![
            MetadataEntry {
                field_id: meta_fields::EVENT_TYPE,
                value: MetadataValue::String(event_type.to_string()),
            },
            MetadataEntry {
                field_id: meta_fields::WORK_ORDER_ID,
                value: MetadataValue::String(work_order_id.to_string()),
            },
            MetadataEntry {
                field_id: meta_fields::PAYLOAD_JSON,
                value: MetadataValue::String(payload_json.to_string()),
            },
            MetadataEntry {
                field_id: meta_fields::TS_NS,
                value: MetadataValue::U64(ts_ns),
            },
        ];
        // Per-vector metadata: exactly one MetadataEntry block per vector.
        let _ = self
            .store
            .ingest_batch(&[&embedding], &[seq], Some(&metadata));
        Ok(seq)
    }

    /// Semantic recall: return the `k` events whose embeddings are most similar to the
    /// supplied intent vector, ordered by cosine distance (closest first).
    pub fn query_by_intent(&self, intent_vector: &[f32], k: usize) -> Result<Vec<EventRow>> {
        if intent_vector.len() != self.dim {
            return Err(LedgerError::InvalidParameterCount(
                self.dim,
                intent_vector.len(),
            ));
        }
        let results = self
            .store
            .query(intent_vector, k.max(1), &QueryOptions::default())
            .map_err(rvf_err)?;
        if results.is_empty() {
            return Ok(Vec::new());
        }
        let order: Vec<u64> = results.iter().map(|r| r.id).collect();
        let ids: HashSet<u64> = order.iter().copied().collect();
        let mut rows: Vec<EventRow> = self
            .v1
            .all_events()?
            .into_iter()
            .filter(|r| ids.contains(&r.seq))
            .collect();
        rows.sort_by_key(|r| {
            order
                .iter()
                .position(|id| *id == r.seq)
                .unwrap_or(usize::MAX)
        });
        Ok(rows)
    }

    // ------------------------------------------------------------------
    // Delegated v1 API (authoritative structured storage / witness / lease)
    // ------------------------------------------------------------------

    pub fn all_events(&self) -> Result<Vec<EventRow>> {
        self.v1.all_events()
    }

    pub fn events_after(&self, after_seq: u64) -> Result<Vec<EventRow>> {
        self.v1.events_after(after_seq)
    }

    pub fn verify_witness_chain(&self) -> Result<usize> {
        self.v1.verify_witness_chain()
    }

    pub fn verify_rollup_provenance(&self) -> Result<RollupProvenance> {
        self.v1.verify_rollup_provenance()
    }

    pub fn rollup_from(
        &mut self,
        origin_repo: &str,
        rows: &[EventRow],
        updated_ns: u64,
    ) -> Result<RollupStat> {
        self.v1.rollup_from(origin_repo, rows, updated_ns)
    }

    pub fn sync_cursor_get(&self, origin_repo: &str) -> Result<Option<u64>> {
        self.v1.sync_cursor_get(origin_repo)
    }

    pub fn sync_cursor_set(
        &mut self,
        origin_repo: &str,
        last_seq: u64,
        updated_ns: u64,
    ) -> Result<()> {
        self.v1.sync_cursor_set(origin_repo, last_seq, updated_ns)
    }

    pub fn try_acquire_lease(
        &mut self,
        resource: &str,
        holder: &str,
        ttl_secs: u64,
        now_ns: u64,
    ) -> Result<LeaseOutcome> {
        self.v1
            .try_acquire_lease(resource, holder, ttl_secs, now_ns)
    }

    pub fn release_lease(&mut self, resource: &str, holder: &str, now_ns: u64) -> Result<u64> {
        self.v1.release_lease(resource, holder, now_ns)
    }

    pub fn lease_holder(&self, resource: &str, now_ns: u64) -> Result<Option<String>> {
        self.v1.lease_holder(resource, now_ns)
    }

    pub fn record_transition(
        &mut self,
        wo: &work_order::WorkOrder,
        status: work_order::Status,
        ts_ns: u64,
    ) -> Result<u64> {
        self.v1.record_transition(wo, status, ts_ns)
    }

    pub fn replay_latest_status(&self) -> Result<Vec<(String, work_order::Status)>> {
        self.v1.replay_latest_status()
    }

    /// Close the ledger, flushing the RVF index and releasing locks.
    ///
    /// The underlying v1 SQLite connection is dropped automatically; this call primarily
    /// ensures the RVF store is cleanly closed.
    pub fn close(self) -> Result<()> {
        self.store.close().map_err(rvf_err)?;
        drop(self.v1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // RvfStore::create/open open several files and fsync the manifest. Under full
    // `cargo test --workspace` parallelism (many test binaries doing concurrent /tmp
    // IO) this intermittently surfaced as a transient FsyncFailed (0x0303) — fd/fsync
    // resource pressure, not a logic bug (each test already uses a unique path). The
    // ledger is opened single-threaded in production, so serialize the RVF-touching
    // tests to bound concurrency and make them deterministic.
    static RVF_TEST_GUARD: Mutex<()> = Mutex::new(());

    /// Acquire the global RVF test lock, recovering from poisoning so a single failing
    /// test does not cascade into the rest.
    fn rvf_guard() -> std::sync::MutexGuard<'static, ()> {
        RVF_TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn temp_db() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        // Monotonic counter guarantees a unique path even if two calls land on the same
        // nanosecond, on top of pid + timestamp.
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hf-ledger-v2-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("ledger.db")
    }

    fn cleanup(p: &std::path::Path) {
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// Build a 104-byte RVF lock file (the format `inspect_lock` parses): magic@0, pid@4,
    /// host@0x08 (null-terminated), recent timestamp@0x48.
    fn build_test_lock(pid: u32, host: &str) -> Vec<u8> {
        let mut buf = vec![0u8; RVF_LOCK_SIZE];
        buf[0..4].copy_from_slice(&RVF_LOCK_MAGIC.to_le_bytes());
        buf[4..8].copy_from_slice(&pid.to_le_bytes());
        let hb = host.as_bytes();
        let n = hb.len().min(62);
        buf[0x08..0x08 + n].copy_from_slice(&hb[..n]);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        buf[0x48..0x50].copy_from_slice(&ts.to_le_bytes());
        buf
    }

    // A pid that is positive-as-i32 and almost certainly dead (rvf's own tests use it too).
    const DEAD_PID: u32 = 999_999_999;

    #[test]
    fn inspect_lock_reclaims_provably_dead_same_host() {
        let path = temp_db();
        let rvf = rvf_path(path.to_str().unwrap());
        let lock = rvf_lock_path(&rvf);
        std::fs::write(&lock, build_test_lock(DEAD_PID, &current_hostname())).unwrap();
        assert_eq!(inspect_lock(&rvf), LockReclaim::Reclaimed { pid: DEAD_PID });
        assert!(!lock.exists(), "a provably-dead lock must be removed");
        cleanup(&path);
    }

    #[test]
    fn inspect_lock_refuses_live_holder() {
        let path = temp_db();
        let rvf = rvf_path(path.to_str().unwrap());
        let lock = rvf_lock_path(&rvf);
        let me = std::process::id(); // this test process is alive
        std::fs::write(&lock, build_test_lock(me, &current_hostname())).unwrap();
        assert_eq!(inspect_lock(&rvf), LockReclaim::RefusedLive { pid: me });
        assert!(lock.exists(), "a LIVE holder's lock must NOT be stolen");
        cleanup(&path);
    }

    #[test]
    fn inspect_lock_refuses_foreign_host_and_bad_magic() {
        let path = temp_db();
        let rvf = rvf_path(path.to_str().unwrap());
        let lock = rvf_lock_path(&rvf);
        // Foreign host: a remote PID can't be probed → fail-closed.
        std::fs::write(&lock, build_test_lock(DEAD_PID, "some-other-host-xyz")).unwrap();
        assert_eq!(inspect_lock(&rvf), LockReclaim::RefusedUnverifiable);
        assert!(lock.exists());
        // Wrong magic: never touch a file we can't positively identify as an RVF lock.
        let mut bad = build_test_lock(DEAD_PID, &current_hostname());
        bad[0] ^= 0xFF;
        std::fs::write(&lock, bad).unwrap();
        assert_eq!(inspect_lock(&rvf), LockReclaim::RefusedUnverifiable);
        assert!(lock.exists());
        cleanup(&path);
    }

    #[test]
    fn inspect_lock_nolock_when_absent() {
        let path = temp_db();
        let rvf = rvf_path(path.to_str().unwrap());
        assert_eq!(inspect_lock(&rvf), LockReclaim::NoLock);
        cleanup(&path);
    }

    #[test]
    fn open_reclaims_dead_lock_and_witnesses() {
        let _g = rvf_guard();
        let path = temp_db();
        let ps = path.to_str().unwrap();
        // First open creates the sidecar; close releases OUR writer lock.
        {
            let led = Ledger::open(ps).unwrap();
            led.close().unwrap();
        }
        // Simulate a crashed writer: ensure the only lock present is a dead-holder one.
        let rvf = rvf_path(ps);
        let lock = rvf_lock_path(&rvf);
        let _ = std::fs::remove_file(&lock);
        std::fs::write(&lock, build_test_lock(DEAD_PID, &current_hostname())).unwrap();
        // Opening must RECLAIM the dead lock (not wedge) AND witness it on the v1 chain.
        let led = Ledger::open(ps).unwrap();
        let evs = led.all_events().unwrap();
        assert!(
            evs.iter().any(|e| e.event_type == "lock_reclaimed"),
            "the reclaim of a dead lock must be witnessed"
        );
        led.close().unwrap();
        cleanup(&path);
    }

    #[test]
    fn encode_is_deterministic_and_384d() {
        let v1 = encode_event_to_vector("checkpoint", "WO-1", "{}");
        let v2 = encode_event_to_vector("checkpoint", "WO-1", "{}");
        assert_eq!(v1.len(), DIM);
        assert_eq!(v1, v2);

        let v3 = encode_event_to_vector("checkpoint", "WO-2", "{}");
        assert_ne!(v1, v3);
    }

    #[test]
    fn append_roundtrips_through_all_events() {
        let _g = rvf_guard();
        let path = temp_db();
        {
            let mut led = Ledger::open(path.to_str().unwrap()).unwrap();
            led.append("checkpoint", "WO-1", "{}", 1_000).unwrap();
            led.append("checkpoint", "WO-2", "{}", 2_000).unwrap();
            let evs = led.all_events().unwrap();
            assert_eq!(evs.len(), 2);
            assert_eq!(evs[0].seq, 1);
            assert_eq!(evs[1].seq, 2);
            assert_eq!(evs[1].work_order_id, "WO-2");
            led.close().unwrap();
        }
        cleanup(&path);
    }

    #[test]
    fn witness_chain_verifies() {
        let _g = rvf_guard();
        let path = temp_db();
        {
            let mut led = Ledger::open(path.to_str().unwrap()).unwrap();
            for i in 0..3 {
                led.append("checkpoint", &format!("WO-{i}"), "{}", 1_000 + i)
                    .unwrap();
            }
            assert_eq!(led.verify_witness_chain().unwrap(), 3);
            led.close().unwrap();
        }
        cleanup(&path);
    }

    #[test]
    fn semantic_recall_finds_similar_event() {
        let _g = rvf_guard();
        let path = temp_db();
        {
            let mut led = Ledger::open(path.to_str().unwrap()).unwrap();
            led.append("checkpoint", "WO-1", "{\"msg\":\"hello world\"}", 1_000)
                .unwrap();
            led.append(
                "checkpoint",
                "WO-2",
                "{\"msg\":\"completely different\"}",
                2_000,
            )
            .unwrap();

            let query = encode_event_to_vector("checkpoint", "WO-1", "{\"msg\":\"hello world\"}");
            let hits = led.query_by_intent(&query, 2).unwrap();
            assert!(!hits.is_empty());
            assert_eq!(hits[0].work_order_id, "WO-1");

            led.close().unwrap();
        }
        cleanup(&path);
    }

    #[test]
    fn events_after_and_rollup_still_work() {
        let _g = rvf_guard();
        let central_path = temp_db();
        let src_path = temp_db();
        {
            let mut src = Ledger::open(src_path.to_str().unwrap()).unwrap();
            src.append("checkpoint", "WO-A", "{}", 100).unwrap();
            src.append("checkpoint", "WO-B", "{}", 200).unwrap();
            let rows = src.events_after(0).unwrap();
            src.close().unwrap();

            let mut central = Ledger::open(central_path.to_str().unwrap()).unwrap();
            let stat = central.rollup_from("repo-x", &rows, 1).unwrap();
            assert_eq!(stat.appended, 2);
            assert_eq!(central.all_events().unwrap().len(), 2);
            assert!(central.verify_rollup_provenance().unwrap().is_faithful());
            central.close().unwrap();
        }
        cleanup(&central_path);
        cleanup(&src_path);
    }

    #[test]
    fn atomic_lease_works() {
        let _g = rvf_guard();
        let path = temp_db();
        {
            let mut led = Ledger::open(path.to_str().unwrap()).unwrap();
            let now = 1_000_000_000;
            assert!(
                matches!(
                    led.try_acquire_lease("res", "alice", 60, now).unwrap(),
                    LeaseOutcome::Acquired { .. }
                ),
                "alice should acquire"
            );
            assert!(
                matches!(
                    led.try_acquire_lease("res", "bob", 60, now + 1).unwrap(),
                    LeaseOutcome::Conflict { holder } if holder == "alice"
                ),
                "bob should conflict with alice"
            );
            assert!(
                matches!(
                    led.try_acquire_lease("res", "alice", 60, now + 2).unwrap(),
                    LeaseOutcome::Heartbeat { .. }
                ),
                "alice heartbeat"
            );
            led.close().unwrap();
        }
        cleanup(&path);
    }
}
