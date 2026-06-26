//! One-time legacy C-SQLite → redb importer (ADR-0017 §"Migration / cutover plan", step 3).
//!
//! Compiled ONLY under the non-default `legacy-sqlite` feature (it pulls in bundled C-SQLite,
//! which the default build must never link). It streams an existing `ledger.db` (the old
//! rusqlite events table) into a fresh redb ledger **in seq order**, re-appending each event
//! through the authoritative [`crate::v1::Ledger::append`] path so the redb chain re-derives
//! identically. After import, the witness chain is re-verified — **fail-closed** on any
//! mismatch (a count/verification discrepancy aborts with an error rather than silently
//! producing a divergent ledger).
//!
//! Provenance/rollup rows are migrated faithfully too: a row with a non-NULL `origin_repo` is
//! re-applied via [`crate::v1::Ledger::rollup_from`] so its `origin_*` columns and the partial
//! idempotency index are reconstructed.

use rusqlite::Connection;

use crate::v1::{EventRow, Ledger, LedgerError, Result, hash_action};

/// Migrate the legacy SQLite ledger at `sqlite_path` into a fresh redb ledger at `redb_path`.
///
/// Returns the number of events imported. Fails closed if the redb chain does not verify to
/// the same event count as the source (no-downgrade guarantee).
pub fn migrate_sqlite_to_redb(sqlite_path: &str, redb_path: &str) -> Result<usize> {
    let conn = Connection::open(sqlite_path)
        .map_err(|e| LedgerError::Decode(format!("open legacy sqlite: {e}")))?;

    // Pull every legacy row in seq order, including provenance columns (which may be absent on
    // a very old pre-HFTASK-0031 schema — tolerate their absence).
    let has_origin = sqlite_has_origin_columns(&conn);
    let rows = read_legacy_events(&conn, has_origin)?;
    let source_count = rows.len();

    let mut led = Ledger::open(redb_path)?;
    for ev in &rows {
        match &ev.origin {
            None => {
                // Native event: re-append (re-chains onto the redb tail, recomputes the hash).
                led.append(
                    &ev.event_type,
                    &ev.work_order_id,
                    &ev.payload_json,
                    ev.ts_ns,
                )?;
            }
            Some((origin_repo, origin_seq)) => {
                // Rolled-up event: re-apply via rollup so origin_* + the idempotency index are
                // reconstructed. One-row batch preserves ordering.
                let row = EventRow {
                    seq: *origin_seq,
                    ts_ns: ev.ts_ns,
                    event_type: ev.event_type.clone(),
                    work_order_id: ev.work_order_id.clone(),
                    payload_json: ev.payload_json.clone(),
                    action_hash: hash_action(&ev.event_type, &ev.work_order_id, &ev.payload_json),
                };
                led.rollup_from(origin_repo, std::slice::from_ref(&row), ev.ts_ns)?;
            }
        }
    }

    // Fail-closed: the migrated chain must verify to the same count.
    let verified = led.verify_witness_chain()?;
    if verified != source_count {
        return Err(LedgerError::Decode(format!(
            "migration chain mismatch: source had {source_count} events, redb verified {verified}"
        )));
    }
    Ok(source_count)
}

/// One legacy row, with its optional rollup provenance `(origin_repo, origin_seq)`.
struct LegacyEvent {
    ts_ns: u64,
    event_type: String,
    work_order_id: String,
    payload_json: String,
    origin: Option<(String, u64)>,
}

fn sqlite_has_origin_columns(conn: &Connection) -> bool {
    let Ok(mut stmt) = conn.prepare("PRAGMA table_info(events)") else {
        return false;
    };
    let Ok(cols) = stmt.query_map([], |r| r.get::<_, String>(1)) else {
        return false;
    };
    let names: Vec<String> = cols.flatten().collect();
    names.iter().any(|c| c == "origin_repo")
}

fn read_legacy_events(conn: &Connection, has_origin: bool) -> Result<Vec<LegacyEvent>> {
    let sql = if has_origin {
        "SELECT ts_ns, event_type, work_order_id, payload_json, origin_repo, origin_seq
         FROM events ORDER BY seq"
    } else {
        "SELECT ts_ns, event_type, work_order_id, payload_json, NULL, NULL
         FROM events ORDER BY seq"
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| LedgerError::Decode(format!("prepare legacy read: {e}")))?;
    let rows = stmt
        .query_map([], |r| {
            let origin_repo: Option<String> = r.get(4)?;
            let origin_seq: Option<i64> = r.get(5)?;
            Ok(LegacyEvent {
                ts_ns: r.get::<_, i64>(0)? as u64,
                event_type: r.get(1)?,
                work_order_id: r.get(2)?,
                payload_json: r.get(3)?,
                origin: match (origin_repo, origin_seq) {
                    (Some(repo), Some(seq)) => Some((repo, seq as u64)),
                    _ => None,
                },
            })
        })
        .map_err(|e| LedgerError::Decode(format!("query legacy read: {e}")))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| LedgerError::Decode(format!("collect legacy rows: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "hf-migrate-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Build a legacy SQLite ledger (the old schema + a real chain), migrate it to redb, and
    /// assert the redb chain verifies to the same count with byte-identical action hashes.
    #[test]
    fn migrates_legacy_sqlite_chain_into_redb() {
        let dir = temp_dir();
        let sqlite_path = dir.join("legacy.db");
        let redb_path = dir.join("new.redb");

        // Hand-build the legacy schema + append a witnessed chain the same way the old code did.
        let stream = [
            ("task_transition", "HFTASK-OLD", "{\"status\":\"Claimed\"}"),
            ("checkpoint", "HFTASK-OLD", "{}"),
            ("task_transition", "HFTASK-OLD", "{\"status\":\"Done\"}"),
        ];
        {
            let conn = Connection::open(&sqlite_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE events (
                    seq INTEGER PRIMARY KEY, ts_ns INTEGER NOT NULL, event_type TEXT NOT NULL,
                    work_order_id TEXT NOT NULL, payload_json TEXT NOT NULL,
                    action_hash BLOB NOT NULL, prev_hash BLOB NOT NULL,
                    origin_repo TEXT, origin_seq INTEGER, origin_action_hash BLOB
                );",
            )
            .unwrap();
            let mut prev = [0u8; 32];
            for (i, (et, wo, pl)) in stream.iter().enumerate() {
                let ah = hash_action(et, wo, pl);
                conn.execute(
                    "INSERT INTO events (seq, ts_ns, event_type, work_order_id, payload_json, action_hash, prev_hash)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![(i as i64) + 1, 1_000i64 + i as i64, et, wo, pl, ah.to_vec(), prev.to_vec()],
                )
                .unwrap();
                prev = ah;
            }
        }

        let imported =
            migrate_sqlite_to_redb(sqlite_path.to_str().unwrap(), redb_path.to_str().unwrap())
                .unwrap();
        assert_eq!(imported, 3);

        // The redb ledger verifies and the action hashes are byte-identical to the source.
        let led = Ledger::open(redb_path.to_str().unwrap()).unwrap();
        assert_eq!(led.verify_witness_chain().unwrap(), 3);
        let rows = led.all_events().unwrap();
        for (row, (et, wo, pl)) in rows.iter().zip(stream.iter()) {
            assert_eq!(row.action_hash, hash_action(et, wo, pl));
        }
        drop(led);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
