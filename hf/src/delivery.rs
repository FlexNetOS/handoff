//! Delivery / output endpoint (HFTASK-0021).
//!
//! ADR-0001 §13/R14: the pipeline is prompt_hub(input) -> process -> delivery(output).
//! The front door already stamps `correlation_id` (= SwarmBundle.workflow_id) on every
//! `WorkOrder`. This module round-trips the merged cycle's result back to that workflow
//! by emitting a witnessed `delivery` ledger event on `pr_merged`, and exposes `hf delivery`
//! queries so RuVocal / prompt_hub can surface it.
//!
//! The delivery artifact is written to `.handoff/deliveries/<correlation_id>.delivery.json`
//! for easy front-door polling, with the ledger remaining the authoritative source of truth.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use work_order::WorkOrder;

use crate::ledger_path;
use ledger::{EventRow, Ledger};

const HF: &str = ".handoff";
const DELIVERY_SCHEMA: &str = "handoff.delivery.v1";

/// Typed delivery envelope — the round-trip payload keyed by `correlation_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Delivery {
    pub schema: String,
    pub correlation_id: String,
    pub task_id: String,
    pub pr: String,
    pub status: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub timestamp_ns: u64,
}

impl Delivery {
    /// Build a delivery record from a completed work order + merged PR.
    pub fn from_merged(wo: &WorkOrder, pr: &str, timestamp_ns: u64) -> Self {
        let summary = format!("{} completed via PR {} (status: merged)", wo.id, pr);
        Self {
            schema: DELIVERY_SCHEMA.to_string(),
            correlation_id: wo.correlation_id.clone(),
            task_id: wo.id.clone(),
            pr: pr.to_string(),
            status: "merged".to_string(),
            summary,
            url: pr_url(pr),
            timestamp_ns,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// Best-effort PR URL resolver. If `pr` is already a URL, pass it through; otherwise
/// assume `FlexNetOS/handoff#N` and build a github.com URL. Unknown shapes return None.
fn pr_url(pr: &str) -> Option<String> {
    if pr.starts_with("http://") || pr.starts_with("https://") {
        return Some(pr.to_string());
    }
    // Numeric PR number -> default repo URL.
    if pr.chars().all(|c| c.is_ascii_digit()) {
        return Some(format!("https://github.com/FlexNetOS/handoff/pull/{pr}"));
    }
    // owner/repo#N
    if let Some((owner_repo, num)) = pr.split_once('#')
        && num.chars().all(|c| c.is_ascii_digit())
        && !owner_repo.is_empty()
    {
        return Some(format!("https://github.com/{owner_repo}/pull/{num}"));
    }
    None
}

fn deliveries_dir() -> PathBuf {
    Path::new(HF).join("deliveries")
}

fn delivery_path(correlation_id: &str) -> PathBuf {
    deliveries_dir().join(format!("{correlation_id}.delivery.json"))
}

/// Emit a delivery record on `pr_merged`. Witnesses a `delivery` ledger event and writes
/// the artifact file for easy front-door lookup. Best-effort: a ledger append failure is
/// logged but does not block the merge-done flow.
pub fn emit_delivery(led: &mut Ledger, wo: &WorkOrder, pr: &str, timestamp_ns: u64) {
    let delivery = Delivery::from_merged(wo, pr, timestamp_ns);
    let payload = delivery.to_json();
    if let Err(e) = led.append("delivery", &wo.id, &payload, timestamp_ns) {
        eprintln!("hf delivery: ledger witness failed: {e}");
    }
    if let Err(e) = write_delivery_artifact(&delivery) {
        eprintln!("hf delivery: artifact write failed: {e}");
    }
}

fn write_delivery_artifact(delivery: &Delivery) -> std::io::Result<()> {
    let path = delivery_path(&delivery.correlation_id);
    std::fs::create_dir_all(deliveries_dir())?;
    std::fs::write(&path, delivery.to_json())
}

/// Read all `delivery` events from the ledger, newest-first.
fn load_deliveries_from_ledger() -> Vec<Delivery> {
    let Ok(led) = Ledger::open(&ledger_path()) else {
        return vec![];
    };
    let Ok(rows) = led.all_events() else {
        return vec![];
    };
    parse_delivery_rows(&rows)
}

fn parse_delivery_rows(rows: &[EventRow]) -> Vec<Delivery> {
    let mut out: Vec<Delivery> = rows
        .iter()
        .filter(|r| r.event_type == "delivery")
        .filter_map(|r| serde_json::from_str::<Delivery>(&r.payload_json).ok())
        .collect();
    // Newest first.
    out.sort_by_key(|d| std::cmp::Reverse(d.timestamp_ns));
    out
}

/// `hf delivery get <correlation_id> [--json]` — return the newest delivery for a workflow.
pub fn cmd_delivery_get(correlation_id: &str, json: bool) {
    if correlation_id.is_empty() {
        eprintln!("usage: hf delivery get <correlation_id> [--json]");
        std::process::exit(2);
    }
    // Prefer the artifact (fast, front-door pollable); fall back to ledger scan.
    let delivery = if let Ok(s) = std::fs::read_to_string(delivery_path(correlation_id)) {
        serde_json::from_str::<Delivery>(&s).ok()
    } else {
        load_deliveries_from_ledger()
            .into_iter()
            .find(|d| d.correlation_id == correlation_id)
    };
    match delivery {
        Some(d) => {
            if json {
                println!("{}", d.to_json());
            } else {
                println!("hf delivery get: {} -> {}", d.correlation_id, d.summary);
                if let Some(url) = &d.url {
                    println!("  url: {url}");
                }
            }
        }
        None => {
            if json {
                println!("{{}}");
            } else {
                eprintln!("hf delivery get: no delivery for correlation_id '{correlation_id}'");
            }
            std::process::exit(1);
        }
    }
}

/// `hf delivery list [--json]` — list all delivered workflows, newest first.
pub fn cmd_delivery_list(json: bool) {
    let deliveries = load_deliveries_from_ledger();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.delivery_list.v1",
            "deliveries": deliveries,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return;
    }
    if deliveries.is_empty() {
        println!("hf delivery list: no deliveries");
        return;
    }
    println!(
        "hf delivery list: {} delivered workflow(s)",
        deliveries.len()
    );
    for d in &deliveries {
        println!("  {} -> {} ({})", d.correlation_id, d.task_id, d.status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handoff_test_support::cwd_lock;

    fn sample_wo() -> WorkOrder {
        WorkOrder {
            schema: "handoff.task.v1".to_string(),
            id: "TASK-0021".to_string(),
            title: "delivery endpoint".to_string(),
            status: work_order::Status::Done,
            priority: work_order::Priority::P2,
            objective: "round-trip".to_string(),
            path_scope: vec!["spike/**".to_string()],
            acceptance_criteria: vec!["cargo test green".to_string()],
            test_commands: vec!["cargo test".to_string()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "wf-frontend-0021".to_string(),
            role: None,
            intent_lock: WorkOrder::compute_intent_lock(
                "round-trip",
                &["spike/**".to_string()],
                &["cargo test green".to_string()],
            ),
        }
    }

    #[test]
    fn delivery_from_merged_round_trips_correlation_id() {
        let wo = sample_wo();
        let d = Delivery::from_merged(&wo, "73", 1_000_000);
        assert_eq!(d.schema, "handoff.delivery.v1");
        assert_eq!(d.correlation_id, "wf-frontend-0021");
        assert_eq!(d.task_id, "TASK-0021");
        assert_eq!(d.pr, "73");
        assert_eq!(d.status, "merged");
        assert_eq!(d.timestamp_ns, 1_000_000);
        assert!(d.summary.contains("TASK-0021") && d.summary.contains("73"));
        assert_eq!(
            d.url,
            Some("https://github.com/FlexNetOS/handoff/pull/73".to_string())
        );
    }

    #[test]
    fn pr_url_handles_various_shapes() {
        assert_eq!(
            pr_url("42"),
            Some("https://github.com/FlexNetOS/handoff/pull/42".to_string())
        );
        assert_eq!(
            pr_url("https://github.com/FlexNetOS/handoff/pull/42"),
            Some("https://github.com/FlexNetOS/handoff/pull/42".to_string())
        );
        assert_eq!(
            pr_url("owner/repo#99"),
            Some("https://github.com/owner/repo/pull/99".to_string())
        );
        assert_eq!(pr_url("not-a-pr"), None);
    }

    #[test]
    fn delivery_json_roundtrip() {
        let wo = sample_wo();
        let d = Delivery::from_merged(&wo, "73", 1_000_000);
        let s = d.to_json();
        let parsed: Delivery = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, d);
    }

    #[test]
    fn parse_delivery_rows_filters_and_sorts() {
        let rows = vec![
            EventRow {
                seq: 1,
                ts_ns: 100,
                event_type: "delivery".to_string(),
                work_order_id: "TASK-A".to_string(),
                payload_json: serde_json::json!({
                    "schema": "handoff.delivery.v1",
                    "correlation_id": "wf-a",
                    "task_id": "TASK-A",
                    "pr": "1",
                    "status": "merged",
                    "summary": "a",
                    "timestamp_ns": 100
                })
                .to_string(),
                action_hash: [0u8; 32],
            },
            EventRow {
                seq: 2,
                ts_ns: 200,
                event_type: "delivery".to_string(),
                work_order_id: "TASK-B".to_string(),
                payload_json: serde_json::json!({
                    "schema": "handoff.delivery.v1",
                    "correlation_id": "wf-b",
                    "task_id": "TASK-B",
                    "pr": "2",
                    "status": "merged",
                    "summary": "b",
                    "timestamp_ns": 200
                })
                .to_string(),
                action_hash: [0u8; 32],
            },
            EventRow {
                seq: 3,
                ts_ns: 50,
                event_type: "pr_merged".to_string(),
                work_order_id: "TASK-A".to_string(),
                payload_json: "{}".to_string(),
                action_hash: [0u8; 32],
            },
        ];
        let parsed = parse_delivery_rows(&rows);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].correlation_id, "wf-b"); // newest first
        assert_eq!(parsed[1].correlation_id, "wf-a");
    }

    /// HFTASK-0021: `emit_delivery` witnesses a `delivery` ledger event AND writes the
    /// artifact file, so the front door can poll by correlation_id.
    #[test]
    fn emit_delivery_witnesses_and_writes_artifact() {
        let _g = cwd_lock();
        let mut tmp = std::env::temp_dir();
        tmp.push(format!(
            "hf-delivery-emit-{}-{}",
            std::process::id(),
            crate::now_ns()
        ));
        std::fs::create_dir_all(tmp.join(".handoff")).unwrap();
        let tmp = tmp.canonicalize().unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let wo = sample_wo();
        let ledger_path = tmp.join(".handoff").join("ledger.db");
        let mut led = Ledger::open(&ledger_path.to_string_lossy()).unwrap();
        emit_delivery(&mut led, &wo, "73", 1_000_000);

        // Ledger contains a delivery event keyed by task id.
        let rows = led.all_events().unwrap();
        let delivery_rows: Vec<_> = rows.iter().filter(|r| r.event_type == "delivery").collect();
        assert_eq!(delivery_rows.len(), 1);
        let parsed: Delivery = serde_json::from_str(&delivery_rows[0].payload_json).unwrap();
        assert_eq!(parsed.correlation_id, "wf-frontend-0021");
        assert_eq!(parsed.task_id, "TASK-0021");
        assert_eq!(parsed.pr, "73");

        // Artifact file is written for front-door polling.
        let artifact = delivery_path("wf-frontend-0021");
        assert!(
            artifact.exists(),
            "artifact should exist at {}",
            artifact.display()
        );
        let from_file: Delivery =
            serde_json::from_str(&std::fs::read_to_string(&artifact).unwrap()).unwrap();
        assert_eq!(from_file, parsed);

        std::env::set_current_dir(&prev).unwrap();
        let _ = std::fs::remove_dir_all(tmp);
    }
}
