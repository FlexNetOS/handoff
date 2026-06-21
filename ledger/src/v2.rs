//! RVF vector-native ledger v2.
//!
//! Hybrid design: the proven rusqlite+v1 store remains the authoritative structured event
//! ledger (append, replay, witness chain, lease state, rollup provenance). An RVF vector
//! store is layered on top for semantic recall over session history via HNSW indexing.
//!
//! Vectors: 384-dim, cosine metric. Embeddings are deterministic hash-based pseudo-embeddings
//! so the crate needs no external model or network access.

use std::collections::HashSet;
use std::path::Path;

use rvf_runtime::{
    options::{DistanceMetric, MetadataEntry, MetadataValue, QueryOptions, RvfOptions},
    RvfStore,
};

use crate::v1;

pub use crate::v1::{
    hash_action, resolve_lease, EventRow, LeaseOutcome, RollupProvenance, RollupStat,
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

fn rvf_err(e: rvf_types::RvfError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
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

/// Acquire the RVF sidecar store, retrying on transient lock contention.
///
/// HFTASK-0060 (sibling of HFTASK-0059): the SQLite path got `with_busy_retry`, but the RVF
/// sidecar open did NOT — so two `hf` processes touching the same ledger back-to-back (a
/// session + a checkpoint hook, or rapid CLI calls) surfaced `0x0300 LockHeld` ("another
/// writer holds the lock") as a hard error, which hf call sites `.unwrap()`-ed into a panic.
/// Retry open/create on LockHeld/LockStale with a short capped linear backoff; a genuinely
/// stuck lock still surfaces after the attempt cap. The RVF store is a best-effort recall
/// sidecar (the v1 SQLite store is authoritative), so a bounded wait never risks the chain.
fn acquire_store(rvf: &Path) -> Result<RvfStore, rvf_types::RvfError> {
    const MAX_ATTEMPTS: u32 = 100;
    let mut attempt: u32 = 0;
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
            Err(e) if is_rvf_lock_contention(&e) && attempt + 1 < MAX_ATTEMPTS => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis((attempt as u64).min(10)));
            }
            other => return other,
        }
    }
}

impl Ledger {
    /// Open or create the ledger.
    ///
    /// The v1 SQLite store lives at `path`. The RVF vector sidecar lives at `{path}.rvf`.
    /// If the RVF sidecar does not exist, it is created. If opening the RVF sidecar fails,
    /// the call returns an error so callers can fall back to the v1 feature if desired.
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let v1 = v1::Ledger::open(path)?;
        let rvf = rvf_path(path);
        // HFTASK-0060: retry the sidecar acquisition on transient RVF lock contention
        // (0x0300 LockHeld) — the RVF analogue of the SQLite busy-retry (HFTASK-0059).
        let store = acquire_store(&rvf).map_err(rvf_err)?;
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
    ) -> rusqlite::Result<u64> {
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
    pub fn query_by_intent(
        &self,
        intent_vector: &[f32],
        k: usize,
    ) -> rusqlite::Result<Vec<EventRow>> {
        if intent_vector.len() != self.dim {
            return Err(rusqlite::Error::InvalidParameterCount(
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

    pub fn all_events(&self) -> rusqlite::Result<Vec<EventRow>> {
        self.v1.all_events()
    }

    pub fn events_after(&self, after_seq: u64) -> rusqlite::Result<Vec<EventRow>> {
        self.v1.events_after(after_seq)
    }

    pub fn verify_witness_chain(&self) -> rusqlite::Result<usize> {
        self.v1.verify_witness_chain()
    }

    pub fn verify_rollup_provenance(&self) -> rusqlite::Result<RollupProvenance> {
        self.v1.verify_rollup_provenance()
    }

    pub fn rollup_from(
        &mut self,
        origin_repo: &str,
        rows: &[EventRow],
        updated_ns: u64,
    ) -> rusqlite::Result<RollupStat> {
        self.v1.rollup_from(origin_repo, rows, updated_ns)
    }

    pub fn sync_cursor_get(&self, origin_repo: &str) -> rusqlite::Result<Option<u64>> {
        self.v1.sync_cursor_get(origin_repo)
    }

    pub fn sync_cursor_set(
        &mut self,
        origin_repo: &str,
        last_seq: u64,
        updated_ns: u64,
    ) -> rusqlite::Result<()> {
        self.v1.sync_cursor_set(origin_repo, last_seq, updated_ns)
    }

    pub fn try_acquire_lease(
        &mut self,
        resource: &str,
        holder: &str,
        ttl_secs: u64,
        now_ns: u64,
    ) -> rusqlite::Result<LeaseOutcome> {
        self.v1
            .try_acquire_lease(resource, holder, ttl_secs, now_ns)
    }

    pub fn release_lease(
        &mut self,
        resource: &str,
        holder: &str,
        now_ns: u64,
    ) -> rusqlite::Result<u64> {
        self.v1.release_lease(resource, holder, now_ns)
    }

    pub fn lease_holder(&self, resource: &str, now_ns: u64) -> rusqlite::Result<Option<String>> {
        self.v1.lease_holder(resource, now_ns)
    }

    pub fn record_transition(
        &mut self,
        wo: &work_order::WorkOrder,
        status: work_order::Status,
        ts_ns: u64,
    ) -> rusqlite::Result<u64> {
        self.v1.record_transition(wo, status, ts_ns)
    }

    pub fn replay_latest_status(&self) -> rusqlite::Result<Vec<(String, work_order::Status)>> {
        self.v1.replay_latest_status()
    }

    /// Close the ledger, flushing the RVF index and releasing locks.
    ///
    /// The underlying v1 SQLite connection is dropped automatically; this call primarily
    /// ensures the RVF store is cleanly closed.
    pub fn close(self) -> rusqlite::Result<()> {
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
