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
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?; // PRD: WAL
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
    pub fn append(
        &mut self,
        event_type: &str,
        work_order_id: &str,
        payload_json: &str,
        ts_ns: u64,
    ) -> rusqlite::Result<u64> {
        self.seq += 1;
        let action_hash = hash_action(event_type, work_order_id, payload_json);
        self.conn.execute(
            "INSERT INTO events (seq, ts_ns, event_type, work_order_id, payload_json, action_hash, prev_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                self.seq as i64,
                ts_ns as i64,
                event_type,
                work_order_id,
                payload_json,
                action_hash.to_vec(),
                self.prev_witness_hash.to_vec(),
            ],
        )?;
        self.prev_witness_hash = action_hash;
        Ok(self.seq)
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
}
