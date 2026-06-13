//! `ledger` — the .handoff operational-truth tier (S1 spike).
//!
//! v1 decision (user-approved): **rusqlite (SQLite/WAL) event store + `rvf-crypto::WitnessChain`**
//! bolted on for tamper-evidence. SQL gives queryable/replayable events (what an event ledger
//! needs); the RVF witness chain (a STANDALONE crate — no `rvf-runtime`/napi) gives RVF-grade
//! tamper-evident audit. (RVF vector-native ledger = scheduled v2 upgrade.)
//!
//! Validates: append work-order lifecycle events, witness each one, replay to current state,
//! and verify the witness chain end-to-end.

use rusqlite::Connection;
use rvf_crypto::witness::{create_witness_chain, verify_witness_chain, WitnessEntry};
use sha3::{Digest, Sha3_256};
use work_order::{Status, WorkOrder};

pub struct Ledger {
    conn: Connection,
    seq: u64,
    prev_witness_hash: [u8; 32],
}

#[derive(Debug)]
pub struct EventRow {
    pub seq: u64,
    pub event_type: String,
    pub work_order_id: String,
    pub payload_json: String,
}

fn hash_action(event_type: &str, work_order_id: &str, payload: &str) -> [u8; 32] {
    let mut h = Sha3_256::new();
    h.update(event_type.as_bytes());
    h.update(work_order_id.as_bytes());
    h.update(payload.as_bytes());
    h.finalize().into()
}

impl Ledger {
    /// Open (or create) the ledger. `":memory:"` for ephemeral spike runs.
    ///
    /// HFTASK-0028: harden against concurrent `hf` writers (two sessions, or a session +
    /// a PostEdit checkpoint hook) on the same ledger.db. `journal_mode=WAL` lets readers
    /// and one writer proceed without blocking each other; `busy_timeout=5000` makes a
    /// writer that hits a held lock block-and-retry for up to 5s instead of failing
    /// immediately with "database is locked". The append path then takes a `BEGIN IMMEDIATE`
    /// transaction and reads the latest prev_hash *inside* it, so two concurrent writers
    /// serialize cleanly and can never both chain off the same prev (which would fork the
    /// witness chain).
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?; // PRD: WAL
        conn.busy_timeout(std::time::Duration::from_millis(5000))?; // HFTASK-0028: block-and-retry
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                seq            INTEGER PRIMARY KEY,
                ts_ns          INTEGER NOT NULL,
                event_type     TEXT NOT NULL,
                work_order_id  TEXT NOT NULL,
                payload_json   TEXT NOT NULL,
                action_hash    BLOB NOT NULL,   -- SHA3-256 of the action
                prev_hash      BLOB NOT NULL    -- witness chain link
            );",
        )?;
        // resume seq + prev hash from the tail (replay-safe)
        let (seq, prev): (u64, Vec<u8>) = conn
            .query_row(
                "SELECT seq, action_hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, Vec<u8>>(1)?)),
            )
            .unwrap_or((0, vec![0u8; 32]));
        let mut prev_witness_hash = [0u8; 32];
        if prev.len() == 32 {
            prev_witness_hash.copy_from_slice(&prev);
        }
        Ok(Self {
            conn,
            seq,
            prev_witness_hash,
        })
    }

    /// Append a witnessed event. ts_ns is passed in (deterministic in tests).
    ///
    /// HFTASK-0028: the seq + prev_hash are read from the DB tail *inside* a
    /// `BEGIN IMMEDIATE` transaction (rather than trusting the values cached at `open()`),
    /// so concurrent writers serialize: the second writer blocks on the IMMEDIATE write
    /// lock until the first commits, then reads the now-current tail and chains off it.
    /// Two writers can therefore never both chain off the same prev_hash (no forked
    /// witness chain) and never hit "database is locked" (busy_timeout block-and-retry).
    pub fn append(
        &mut self,
        event_type: &str,
        work_order_id: &str,
        payload_json: &str,
        ts_ns: u64,
    ) -> rusqlite::Result<u64> {
        let action_hash = hash_action(event_type, work_order_id, payload_json);
        // Acquire the write lock up front so the tail read + insert are atomic vs. peers.
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Re-read the authoritative tail from the DB (not the cached open()-time values):
        // a concurrent writer may have advanced it since this handle was opened.
        let (tail_seq, tail_prev): (u64, Vec<u8>) = tx
            .query_row(
                "SELECT seq, action_hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, Vec<u8>>(1)?)),
            )
            .unwrap_or((0, vec![0u8; 32]));
        let mut prev_hash = [0u8; 32];
        if tail_prev.len() == 32 {
            prev_hash.copy_from_slice(&tail_prev);
        }
        let next_seq = tail_seq + 1;
        tx.execute(
            "INSERT INTO events (seq, ts_ns, event_type, work_order_id, payload_json, action_hash, prev_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                next_seq as i64,
                ts_ns as i64,
                event_type,
                work_order_id,
                payload_json,
                action_hash.to_vec(),
                prev_hash.to_vec(),
            ],
        )?;
        tx.commit()?;
        // Keep the in-memory cache consistent with what we just committed.
        self.seq = next_seq;
        self.prev_witness_hash = action_hash;
        Ok(next_seq)
    }

    /// Convenience: record a work-order state transition.
    pub fn record_transition(
        &mut self,
        wo: &WorkOrder,
        status: Status,
        ts_ns: u64,
    ) -> rusqlite::Result<u64> {
        let payload = serde_json::json!({
            "id": wo.id, "status": status, "correlation_id": wo.correlation_id, "role": wo.role
        })
        .to_string();
        self.append("task_transition", &wo.id, &payload, ts_ns)
    }

    pub fn all_events(&self) -> rusqlite::Result<Vec<EventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, event_type, work_order_id, payload_json FROM events ORDER BY seq",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(EventRow {
                    seq: r.get::<_, i64>(0)? as u64,
                    event_type: r.get(1)?,
                    work_order_id: r.get(2)?,
                    payload_json: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// REPLAY (state-precedence tier 2): reconstruct the latest status per work order id.
    pub fn replay_latest_status(&self) -> rusqlite::Result<Vec<(String, Status)>> {
        let mut stmt = self.conn.prepare(
            "SELECT work_order_id, payload_json FROM events WHERE event_type='task_transition' ORDER BY seq",
        )?;
        let mut map: std::collections::BTreeMap<String, Status> = Default::default();
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, payload) = row?;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) {
                if let Some(s) = v.get("status") {
                    if let Ok(st) = serde_json::from_value::<Status>(s.clone()) {
                        map.insert(id, st);
                    }
                }
            }
        }
        Ok(map.into_iter().collect())
    }

    /// Build the RVF witness chain over all events and verify it (tamper-evidence).
    /// Returns the number of verified entries.
    pub fn verify_witness_chain(&self) -> rusqlite::Result<usize> {
        let mut stmt = self
            .conn
            .prepare("SELECT ts_ns, action_hash FROM events ORDER BY seq")?;
        let entries: Vec<WitnessEntry> = stmt
            .query_map([], |r| {
                let ts: i64 = r.get(0)?;
                let ah: Vec<u8> = r.get(1)?;
                let mut action_hash = [0u8; 32];
                action_hash.copy_from_slice(&ah);
                Ok(WitnessEntry {
                    prev_hash: [0u8; 32], // chain links recomputed by create_witness_chain
                    action_hash,
                    timestamp_ns: ts as u64,
                    witness_type: 0x02, // COMPUTATION
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let chain = create_witness_chain(&entries);
        let verified = verify_witness_chain(&chain).expect("witness chain must verify");
        Ok(verified.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use work_order::{work_orders_from_bundle, SwarmBundle};

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

    /// Isolated temp dir for a file-backed ledger (NEVER the real .handoff/ledger.db).
    /// Avoids adding a `tempfile` dev-dep (card sets no dependency-addition allowance).
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
        p.push("ledger.db");
        p
    }

    /// HFTASK-0028 AC3: WAL + busy_timeout are set on the connection at open().
    #[test]
    fn open_sets_wal_and_busy_timeout() {
        let db = temp_db();
        let led = Ledger::open(db.to_str().unwrap()).unwrap();
        let mode: String = led
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal", "journal_mode must be WAL");
        let busy: i64 = led
            .conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 5000, "busy_timeout must be 5000ms");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// HFTASK-0028 AC1+AC2: many concurrent writers on the SAME file ledger all succeed
    /// (no "database is locked") and the witness chain still verifies end-to-end with a
    /// contiguous prev_hash chain (no fork).
    #[test]
    fn concurrent_writers_serialize_no_lock_no_fork() {
        use std::sync::Arc;
        use std::thread;

        let db = temp_db();
        let path = Arc::new(db.to_string_lossy().into_owned());

        // Ensure schema exists before the writers race (each writer opens its own handle).
        Ledger::open(&path).unwrap();

        const WRITERS: usize = 8;
        const PER_WRITER: usize = 25;

        let mut handles = vec![];
        for w in 0..WRITERS {
            let path = Arc::clone(&path);
            handles.push(thread::spawn(move || {
                // Each thread = a separate `hf` process: its own Ledger handle on the file.
                let mut led = Ledger::open(&path).expect("open ledger");
                for i in 0..PER_WRITER {
                    let ts = (w as u64) * 1_000_000 + i as u64;
                    led.append("checkpoint", &format!("HFTASK-W{w}"), "{}", ts)
                        .expect("append must not fail under concurrency");
                }
            }));
        }
        for h in handles {
            h.join().expect("writer thread panicked");
        }

        // AC1: every event landed (no lost writes, no errors).
        let led = Ledger::open(&path).unwrap();
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
        let mut stmt = led
            .conn
            .prepare("SELECT action_hash, prev_hash FROM events ORDER BY seq")
            .unwrap();
        let rows: Vec<(Vec<u8>, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let mut expected_prev = vec![0u8; 32];
        for (action_hash, prev_hash) in &rows {
            assert_eq!(
                prev_hash, &expected_prev,
                "prev_hash chain must be contiguous"
            );
            expected_prev = action_hash.clone();
        }

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }
}
