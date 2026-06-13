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
    /// `UNIQUE(origin_repo, origin_seq)` index fired, or the cursor already covered them).
    pub skipped_existing: usize,
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
        // HFTASK-0031 (ADR-0004 §3.3 rev): additive, backward-compatible migration for
        // rollup provenance. A NULL origin_repo marks a *native* (local) event — this
        // ledger's own. When the CENTRAL fleet ledger re-appends an event rolled up from a
        // per-repo ledger (HFTASK-0032 `hf sync`), it stamps the origin_* columns so both the
        // per-repo chain and the central chain verify independently (CT/RFC6962 model).
        // This task is schema-only: it does NOT write the rollup or a verifier.
        Self::migrate_provenance(&conn)?;
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

    /// HFTASK-0031 (ADR-0004 §3.3 rev): idempotent, additive migration that bolts rollup
    /// provenance onto an existing (or fresh) `events` table without touching any existing
    /// row or breaking the witness chain.
    ///
    /// SQLite has no `ADD COLUMN IF NOT EXISTS`, so we probe `PRAGMA table_info(events)` and
    /// only `ALTER TABLE ... ADD COLUMN` the columns that are absent — safe to run on every
    /// `open()`, on both pre-migration and post-migration DBs.
    ///
    /// - `origin_repo TEXT` / `origin_seq INTEGER` / `origin_action_hash BLOB`: all NULL for
    ///   native (local) events. `append()` never writes them, so old + new local events stay
    ///   NULL. `verify_witness_chain()` reads only `ts_ns` + `action_hash` ordered by `seq`,
    ///   so these columns are invisible to it — old ledgers verify unchanged.
    /// - `idx_events_origin`: a PARTIAL unique index — the idempotency guard for rollup
    ///   (`UNIQUE(origin_repo, origin_seq) WHERE origin_repo IS NOT NULL`), so native NULL
    ///   events are unconstrained while a given source event can be rolled up at most once.
    /// - `sync_cursor`: per-source-repo high-water mark, lives in the CENTRAL ledger so a
    ///   re-run of `hf sync` only re-appends events past the last rolled-up seq.
    fn migrate_provenance(conn: &Connection) -> rusqlite::Result<()> {
        // Which provenance columns already exist? (idempotency: ALTER only the missing ones.)
        let mut have_origin_repo = false;
        let mut have_origin_seq = false;
        let mut have_origin_action_hash = false;
        {
            let mut stmt = conn.prepare("PRAGMA table_info(events)")?;
            let cols = stmt.query_map([], |r| r.get::<_, String>(1))?; // column 1 = name
            for col in cols {
                match col?.as_str() {
                    "origin_repo" => have_origin_repo = true,
                    "origin_seq" => have_origin_seq = true,
                    "origin_action_hash" => have_origin_action_hash = true,
                    _ => {}
                }
            }
        }
        if !have_origin_repo {
            conn.execute_batch("ALTER TABLE events ADD COLUMN origin_repo TEXT")?;
        }
        if !have_origin_seq {
            conn.execute_batch("ALTER TABLE events ADD COLUMN origin_seq INTEGER")?;
        }
        if !have_origin_action_hash {
            conn.execute_batch("ALTER TABLE events ADD COLUMN origin_action_hash BLOB")?;
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_events_origin
                 ON events(origin_repo, origin_seq) WHERE origin_repo IS NOT NULL;
             CREATE TABLE IF NOT EXISTS sync_cursor (
                 origin_repo  TEXT PRIMARY KEY,
                 last_seq     INTEGER NOT NULL,
                 updated_ns   INTEGER NOT NULL
             );",
        )?;
        Ok(())
    }

    /// HFTASK-0031: read a source repo's rollup high-water mark from the central ledger's
    /// `sync_cursor` (the last per-repo `seq` already rolled up). `None` = never synced.
    /// (The rollup itself is HFTASK-0032; this is the cursor accessor it will use.)
    pub fn sync_cursor_get(&self, origin_repo: &str) -> rusqlite::Result<Option<u64>> {
        self.conn
            .query_row(
                "SELECT last_seq FROM sync_cursor WHERE origin_repo = ?1",
                rusqlite::params![origin_repo],
                |r| Ok(r.get::<_, i64>(0)? as u64),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
    }

    /// HFTASK-0031: upsert a source repo's rollup high-water mark (last rolled-up per-repo
    /// `seq`) into the central ledger's `sync_cursor`.
    pub fn sync_cursor_set(
        &mut self,
        origin_repo: &str,
        last_seq: u64,
        updated_ns: u64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO sync_cursor (origin_repo, last_seq, updated_ns)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(origin_repo) DO UPDATE SET
                 last_seq   = excluded.last_seq,
                 updated_ns = excluded.updated_ns",
            rusqlite::params![origin_repo, last_seq as i64, updated_ns as i64],
        )?;
        Ok(())
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
            "SELECT seq, ts_ns, event_type, work_order_id, payload_json, action_hash
             FROM events ORDER BY seq",
        )?;
        let rows = stmt
            .query_map([], Self::map_event_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// HFTASK-0032: read the source ledger's events whose `seq > after_seq`, ordered by
    /// `seq` — the rollup pull. `after_seq` is the central ledger's `sync_cursor` value for
    /// this source repo (0 = never synced → all events). Self-contained events (the full
    /// row incl. `action_hash`) so the central ledger can re-append with provenance without
    /// re-opening the source mid-transaction.
    pub fn events_after(&self, after_seq: u64) -> rusqlite::Result<Vec<EventRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, ts_ns, event_type, work_order_id, payload_json, action_hash
             FROM events WHERE seq > ?1 ORDER BY seq",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![after_seq as i64], Self::map_event_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Shared row mapper for the `events` SELECT shape
    /// `(seq, ts_ns, event_type, work_order_id, payload_json, action_hash)`.
    fn map_event_row(r: &rusqlite::Row) -> rusqlite::Result<EventRow> {
        let ah: Vec<u8> = r.get(5)?;
        let mut action_hash = [0u8; 32];
        if ah.len() == 32 {
            action_hash.copy_from_slice(&ah);
        }
        Ok(EventRow {
            seq: r.get::<_, i64>(0)? as u64,
            ts_ns: r.get::<_, i64>(1)? as u64,
            event_type: r.get(2)?,
            work_order_id: r.get(3)?,
            payload_json: r.get(4)?,
            action_hash,
        })
    }

    /// HFTASK-0032 (ADR-0004 §3.3 rev): roll one source repo's events into THIS (central)
    /// ledger via **append-with-provenance re-chaining** (CT/RFC6962 model) — the whole
    /// batch + the cursor advance commit in ONE transaction (crash-safe: both or neither).
    ///
    /// For each row (assumed ordered by source `seq`):
    /// - Re-append into `events` re-chaining `prev_hash` onto the CURRENT central tail
    ///   (read inside the tx, like `append()` — HFTASK-0028), allocating a fresh central
    ///   `seq`. The central `action_hash` is recomputed from `(event_type, work_order_id,
    ///   payload_json)` — byte-identical to the source's `action_hash` (same inputs) — and
    ///   stored in BOTH `action_hash` and `origin_action_hash` (the provenance bridge).
    /// - Stamp `origin_repo` = the member dir name, `origin_seq` = the source `seq`.
    /// - On the partial `UNIQUE(origin_repo, origin_seq)` conflict (already rolled up),
    ///   SKIP and count it — idempotency backstop independent of the cursor.
    ///
    /// After the batch, advance `sync_cursor[origin_repo]` to the max source `seq` seen
    /// (incl. skipped rows — they're still covered), in the SAME transaction. Chains are
    /// NEVER merged; self-contained events are re-appended onto the central tail.
    ///
    /// Native `append()` / `verify_witness_chain()` are unaffected — this only adds rows.
    pub fn rollup_from(
        &mut self,
        origin_repo: &str,
        rows: &[EventRow],
        updated_ns: u64,
    ) -> rusqlite::Result<RollupStat> {
        let mut stat = RollupStat::default();
        if rows.is_empty() {
            return Ok(stat);
        }
        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        // Authoritative central tail, read INSIDE the write tx (HFTASK-0028): re-chain onto it.
        let (mut tail_seq, tail_prev): (u64, Vec<u8>) = tx
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

        let mut max_origin_seq = 0u64;
        for row in rows {
            max_origin_seq = max_origin_seq.max(row.seq);
            // Recompute the central action_hash from the SAME inputs → identical to source.
            let action_hash = hash_action(&row.event_type, &row.work_order_id, &row.payload_json);
            let next_seq = tail_seq + 1;
            let res = tx.execute(
                "INSERT INTO events
                    (seq, ts_ns, event_type, work_order_id, payload_json, action_hash, prev_hash,
                     origin_repo, origin_seq, origin_action_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    next_seq as i64,
                    row.ts_ns as i64,
                    row.event_type,
                    row.work_order_id,
                    row.payload_json,
                    action_hash.to_vec(),
                    prev_hash.to_vec(),
                    origin_repo,
                    row.seq as i64,
                    action_hash.to_vec(),
                ],
            );
            match res {
                Ok(_) => {
                    // Only a successfully appended row advances the central tail/chain.
                    tail_seq = next_seq;
                    prev_hash = action_hash;
                    stat.appended += 1;
                }
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if e.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    // Already rolled up (idx_events_origin) — skip, don't touch the tail.
                    stat.skipped_existing += 1;
                }
                Err(other) => return Err(other),
            }
        }

        // Advance the cursor to the max source seq covered by this batch (incl. skips), in
        // the SAME transaction as the appends — crash-safe.
        tx.execute(
            "INSERT INTO sync_cursor (origin_repo, last_seq, updated_ns)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(origin_repo) DO UPDATE SET
                 last_seq   = MAX(sync_cursor.last_seq, excluded.last_seq),
                 updated_ns = excluded.updated_ns",
            rusqlite::params![origin_repo, max_origin_seq as i64, updated_ns as i64],
        )?;
        tx.commit()?;

        // Keep the in-memory cache consistent with the committed central tail.
        self.seq = tail_seq;
        self.prev_witness_hash = prev_hash;
        Ok(stat)
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

    /// Helpers shared by the HFTASK-0031 provenance migration tests.
    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    fn object_exists(conn: &Connection, kind: &str, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2",
            rusqlite::params![kind, name],
            |_| Ok(()),
        )
        .is_ok()
    }

    /// HFTASK-0031 AC1: a fresh DB gets the 3 origin columns, the partial unique index, and
    /// the sync_cursor table.
    #[test]
    fn fresh_open_creates_provenance_schema() {
        let db = temp_db();
        let led = Ledger::open(db.to_str().unwrap()).unwrap();

        let cols = column_names(&led.conn, "events");
        for expected in ["origin_repo", "origin_seq", "origin_action_hash"] {
            assert!(
                cols.iter().any(|c| c == expected),
                "events must have column {expected}; got {cols:?}"
            );
        }
        assert!(
            object_exists(&led.conn, "index", "idx_events_origin"),
            "idx_events_origin must exist"
        );
        assert!(
            object_exists(&led.conn, "table", "sync_cursor"),
            "sync_cursor table must exist"
        );

        // The index is partial — native (NULL origin_repo) events are unconstrained, so two
        // appends with NULL origin_* must not collide on the unique index.
        let mut led = led;
        led.append("checkpoint", "HFTASK-X", "{}", 10).unwrap();
        led.append("checkpoint", "HFTASK-X", "{}", 11).unwrap();
        assert_eq!(led.all_events().unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// HFTASK-0031 AC2: migration is idempotent — open() twice on the same path does not
    /// error and does not duplicate the origin columns.
    #[test]
    fn migration_is_idempotent() {
        let db = temp_db();
        {
            let _ = Ledger::open(db.to_str().unwrap()).unwrap();
        }
        // Second open re-runs migrate_provenance over an already-migrated DB.
        let led = Ledger::open(db.to_str().unwrap()).unwrap();
        let cols = column_names(&led.conn, "events");
        let count = |name: &str| cols.iter().filter(|c| c.as_str() == name).count();
        assert_eq!(count("origin_repo"), 1, "no duplicate origin_repo column");
        assert_eq!(count("origin_seq"), 1, "no duplicate origin_seq column");
        assert_eq!(
            count("origin_action_hash"),
            1,
            "no duplicate origin_action_hash column"
        );
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    /// HFTASK-0031 AC3 (THE backward-compat proof): a pre-migration ledger (events table
    /// WITHOUT the origin columns) with real appended events still verifies after open()
    /// migrates it in place — no data loss, full chain, old rows have NULL origin_*.
    #[test]
    fn old_schema_db_migrates_and_still_verifies() {
        let db = temp_db();
        let path = db.to_str().unwrap();

        // 1. Hand-build the OLD schema (exactly the pre-HFTASK-0031 events table) and append
        //    a few witnessed events the SAME way append() does, so we have a real chain.
        {
            let conn = Connection::open(path).unwrap();
            conn.execute_batch(
                "CREATE TABLE events (
                    seq            INTEGER PRIMARY KEY,
                    ts_ns          INTEGER NOT NULL,
                    event_type     TEXT NOT NULL,
                    work_order_id  TEXT NOT NULL,
                    payload_json   TEXT NOT NULL,
                    action_hash    BLOB NOT NULL,
                    prev_hash      BLOB NOT NULL
                );",
            )
            .unwrap();
            // Confirm the pre-condition: no origin columns yet.
            let cols = column_names(&conn, "events");
            assert!(
                !cols.iter().any(|c| c == "origin_repo"),
                "precondition: old schema has no origin_repo"
            );

            let mut prev = [0u8; 32];
            for (i, (et, wo, pl)) in [
                ("task_transition", "HFTASK-OLD", "{\"status\":\"Claimed\"}"),
                ("checkpoint", "HFTASK-OLD", "{}"),
                ("task_transition", "HFTASK-OLD", "{\"status\":\"Done\"}"),
            ]
            .iter()
            .enumerate()
            {
                let ah = hash_action(et, wo, pl);
                conn.execute(
                    "INSERT INTO events (seq, ts_ns, event_type, work_order_id, payload_json, action_hash, prev_hash)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        (i as i64) + 1,
                        1_000i64 + i as i64,
                        et,
                        wo,
                        pl,
                        ah.to_vec(),
                        prev.to_vec(),
                    ],
                )
                .unwrap();
                prev = ah;
            }
        }

        // 2. open() triggers migrate_provenance over the existing (populated) old DB.
        let mut led = Ledger::open(path).unwrap();

        // 3. Schema was migrated in place — origin columns + sync_cursor now exist.
        let cols = column_names(&led.conn, "events");
        for expected in ["origin_repo", "origin_seq", "origin_action_hash"] {
            assert!(
                cols.iter().any(|c| c == expected),
                "migration must add {expected} to the old table"
            );
        }
        assert!(object_exists(&led.conn, "table", "sync_cursor"));

        // 4. No data loss + the witness chain verifies over the full original count.
        assert_eq!(led.all_events().unwrap().len(), 3, "no rows lost");
        assert_eq!(
            led.verify_witness_chain().unwrap(),
            3,
            "old chain must still verify after migration"
        );

        // 5. Old rows have NULL origin_* (they are native local events).
        let null_origins: i64 = led
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE origin_repo IS NULL
                   AND origin_seq IS NULL AND origin_action_hash IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(null_origins, 3, "all pre-existing rows stay native (NULL)");

        // 6. append() still works on the migrated ledger and still leaves origin_* NULL.
        led.append("checkpoint", "HFTASK-OLD", "{}", 2_000).unwrap();
        assert_eq!(led.verify_witness_chain().unwrap(), 4);
        let native_after_append: i64 = led
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE origin_repo IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            native_after_append, 4,
            "append() must keep origin_* NULL (native events)"
        );

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

        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }

    // ----- HFTASK-0032: rollup (append-with-provenance re-chaining) ---------

    /// Build a real source ledger with `n` native events and return (its temp dir, path).
    fn source_ledger_with(n: usize, wo_prefix: &str) -> (std::path::PathBuf, String) {
        let db = temp_db();
        let path = db.to_string_lossy().into_owned();
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

        // PROVENANCE faithful (AC4): for each (origin_repo, origin_seq) the central row's
        // origin_action_hash == central action_hash == recomputed SHA3(event_type||wo||
        // payload) == the source row's action_hash. Look each source row up by provenance.
        let provenance = |repo: &str, origin_seq: u64| -> (Vec<u8>, Vec<u8>) {
            central
                .conn
                .query_row(
                    "SELECT origin_action_hash, action_hash
                     FROM events WHERE origin_repo = ?1 AND origin_seq = ?2",
                    rusqlite::params![repo, origin_seq as i64],
                    |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?)),
                )
                .expect("central row exists for this provenance")
        };
        for (repo, src_rows) in [("repo-a", &rows_a), ("repo-b", &rows_b)] {
            for src in src_rows {
                let (origin_ah, central_ah) = provenance(repo, src.seq);
                let recomputed =
                    hash_action(&src.event_type, &src.work_order_id, &src.payload_json).to_vec();
                assert_eq!(origin_ah, recomputed, "origin_action_hash == recomputed");
                assert_eq!(central_ah, recomputed, "central action_hash == recomputed");
                assert_eq!(
                    origin_ah,
                    src.action_hash.to_vec(),
                    "origin_action_hash == source action_hash"
                );
            }
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
        // The native event is NULL-origin; the rolled ones are not.
        let native: i64 = central
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE origin_repo IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(native, 1, "exactly the native checkpoint is NULL-origin");

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
        let _ = std::fs::remove_dir_all(central_dir.parent().unwrap());
    }
}
