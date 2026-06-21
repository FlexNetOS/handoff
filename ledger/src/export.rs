//! Deterministic JSONL export/import of the witnessed ledger (ADR-0018 D1).
//!
//! The committed continuity truth is a **text** export — one JSON object per event, in seq order —
//! so a fresh clone carries the full ledger and the file diffs/merges in git. The binary redb store
//! stays a LOCAL rebuild cache (gitignored): `rebuild_from_jsonl` re-derives it from the committed
//! text on a fresh checkout, re-appending each event through the authoritative [`Ledger::append`]
//! path and **failing closed** if the rebuilt witness chain does not verify to the same count.
//!
//! Native (non-rolled) events round-trip exactly. Rolled-up provenance (`origin_*`, FLEET ledger)
//! is not carried by this per-repo export — the FLEET ledger is itself re-derivable from member
//! ledgers — so a rebuilt chain is witness-faithful even though origin attribution is re-stamped on
//! the next rollup. The `action_hash` is exported as lowercase hex for audit/diff only; import
//! recomputes and re-verifies it, never trusts it.

use serde::{Deserialize, Serialize};

use crate::v1::{EventRow, Ledger, LedgerError, Result};

#[derive(Serialize, Deserialize)]
struct ExportedEvent {
    seq: u64,
    ts_ns: u64,
    event_type: String,
    work_order_id: String,
    payload_json: String,
    /// Lowercase hex of the source event's `action_hash` — an audit/diff aid only. Import
    /// recomputes the hash through `append` and re-verifies the chain; it never trusts this value.
    action_hash: String,
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Export witnessed events as deterministic, seq-ordered JSONL (one object per line). This is the
/// committed continuity truth (ADR-0018 D1); the binary redb store stays a local cache. Takes the
/// events directly (via `Ledger::all_events`) so it is independent of which store overlay (v1/v2)
/// the caller holds.
pub fn export_jsonl(events: &[EventRow]) -> Result<String> {
    let mut out = String::new();
    for ev in events {
        let rec = ExportedEvent {
            seq: ev.seq,
            ts_ns: ev.ts_ns,
            event_type: ev.event_type.clone(),
            work_order_id: ev.work_order_id.clone(),
            payload_json: ev.payload_json.clone(),
            action_hash: hex_lower(&ev.action_hash),
        };
        let line = serde_json::to_string(&rec)
            .map_err(|e| LedgerError::Decode(format!("export encode: {e}")))?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Rebuild a fresh redb ledger at `redb_path` from a JSONL export (ADR-0018 D1 — a fresh clone
/// re-derives its binary ledger from the committed text). Fail-closed: the rebuilt witness chain
/// must verify to the same event count as the export, else abort with no half-built ledger trusted.
/// Returns the number of events imported.
pub fn rebuild_from_jsonl(jsonl: &str, redb_path: &str) -> Result<usize> {
    let mut led = Ledger::open(redb_path)?;
    let mut n = 0usize;
    for (i, line) in jsonl.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let rec: ExportedEvent = serde_json::from_str(line)
            .map_err(|e| LedgerError::Decode(format!("import line {}: {e}", i + 1)))?;
        led.append(
            &rec.event_type,
            &rec.work_order_id,
            &rec.payload_json,
            rec.ts_ns,
        )?;
        n += 1;
    }
    let verified = led.verify_witness_chain()?;
    if verified != n {
        return Err(LedgerError::Decode(format!(
            "import chain mismatch: {n} events imported, redb verified {verified}"
        )));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ledger(tag: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("hf-export-test-{}-{tag}.redb", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn jsonl_round_trips_witness_faithfully() {
        let src = tmp_ledger("src");
        let dst = tmp_ledger("dst");
        {
            let mut led = Ledger::open(&src).unwrap();
            led.append(
                "task_transition",
                "HFTASK-0001",
                "{\"status\":\"Active\"}",
                1_000,
            )
            .unwrap();
            led.append("checkpoint", "HFTASK-0001", "{\"note\":\"x\"}", 2_000)
                .unwrap();
            led.append(
                "task_transition",
                "HFTASK-0001",
                "{\"status\":\"Done\"}",
                3_000,
            )
            .unwrap();
        }
        let jsonl = {
            let led = Ledger::open(&src).unwrap();
            export_jsonl(&led.all_events().unwrap()).unwrap()
        };
        // One line per event, seq-ordered, valid JSON.
        assert_eq!(jsonl.lines().count(), 3);
        assert!(jsonl.lines().next().unwrap().contains("\"seq\":1"));

        // Rebuild a fresh ledger from the text and confirm the chain verifies to the same count.
        let imported = rebuild_from_jsonl(&jsonl, &dst).unwrap();
        assert_eq!(imported, 3);
        let re = Ledger::open(&dst).unwrap();
        assert_eq!(re.verify_witness_chain().unwrap(), 3);
        // Re-export is byte-identical (deterministic).
        assert_eq!(export_jsonl(&re.all_events().unwrap()).unwrap(), jsonl);

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn corrupt_line_fails_closed() {
        let dst = tmp_ledger("corrupt");
        let err = rebuild_from_jsonl("{not valid json}\n", &dst);
        assert!(err.is_err());
        let _ = std::fs::remove_file(&dst);
    }
}
