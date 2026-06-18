//! HFTASK-0017: cognitum-gate action governor behind `hf policy gate`.
//!
//! Adopts RuVector's `cognitum-gate-tilezero` as the in-loop action gate: an agent asks for a
//! permit to perform an action; the gate returns `permit`, `defer`, or `deny`, and the verdict is
//! witnessed in the ledger as a `cognitum_decision` event. This is distinct from the envctl
//! broker (HFTASK-0013) which governs credential/egress decisions; the two gates compose.

use std::path::PathBuf;

use crate::{ledger_path, now_ns, route::route_for_task, Ledger};

/// A witnessed cognitum-gate verdict for an action.
#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub action_id: String,
    pub decision: String,
    pub sequence: u64,
    /// Base64-encoded signed permit token (when the `cognitum` feature is compiled in).
    pub token_b64: Option<String>,
}

impl DecisionRecord {
    /// Serialize the verdict as a ledger payload.
    pub fn to_payload(&self, task_id: Option<&str>) -> String {
        serde_json::json!({
            "schema": "handoff.cognitum_decision.v1",
            "action_id": self.action_id,
            "decision": self.decision,
            "sequence": self.sequence,
            "token_b64": self.token_b64,
            "task_id": task_id,
        })
        .to_string()
    }
}

#[cfg(feature = "cognitum")]
fn evaluate_action_impl(
    action_id: &str,
    action_type: &str,
    agent_id: &str,
    path: Option<&str>,
) -> DecisionRecord {
    use cognitum_gate_tilezero::{
        ActionContext, ActionMetadata, ActionTarget, GateThresholds, TileZero,
    };
    use std::collections::HashMap;

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime for cognitum gate");
    let tilezero = TileZero::new(GateThresholds::default());
    let ctx = ActionContext {
        action_id: action_id.to_string(),
        action_type: action_type.to_string(),
        target: ActionTarget {
            device: None,
            path: path.map(|s| s.to_string()),
            extra: HashMap::new(),
        },
        context: ActionMetadata {
            agent_id: agent_id.to_string(),
            session_id: None,
            prior_actions: vec![],
            urgency: "normal".to_string(),
        },
    };
    let token = rt.block_on(tilezero.decide(&ctx));
    let token_b64 = Some(token.encode_base64());
    DecisionRecord {
        action_id: token.action_id.clone(),
        decision: token.decision.to_string(),
        sequence: token.sequence,
        token_b64,
    }
}

#[cfg(not(feature = "cognitum"))]
fn evaluate_action_impl(
    _action_id: &str,
    _action_type: &str,
    _agent_id: &str,
    _path: Option<&str>,
) -> DecisionRecord {
    DecisionRecord {
        action_id: String::new(),
        decision: "unavailable".to_string(),
        sequence: 0,
        token_b64: None,
    }
}

/// Evaluate `action_id` through the cognitum gate.
pub fn evaluate_action(
    action_id: &str,
    action_type: &str,
    agent_id: &str,
    path: Option<&str>,
) -> DecisionRecord {
    evaluate_action_impl(action_id, action_type, agent_id, path)
}

/// `hf policy gate <action> [--task <id>]` — ask the cognitum gate for a permit and witness it.
pub fn cmd_policy_gate(action: &str, task_id: Option<&str>) {
    if action.is_empty() {
        eprintln!("usage: hf policy gate <action> [--task <id>]");
        std::process::exit(2);
    }

    #[cfg(not(feature = "cognitum"))]
    {
        eprintln!(
            "hf policy gate: cognitum feature not compiled in; build with --features cognitum"
        );
        std::process::exit(2);
    }

    #[cfg(feature = "cognitum")]
    {
        let record = evaluate_action(action, "hf_policy_gate", "hf", None);

        let ledger = match task_id {
            Some(id) => match route_for_task(id) {
                Ok((ledger, _tasks)) => ledger,
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            },
            None => PathBuf::from(ledger_path()),
        };
        let work_order_id = task_id.unwrap_or("policy");

        let mut led = Ledger::open(&ledger.to_string_lossy()).unwrap_or_else(|e| {
            eprintln!("hf policy gate: cannot open ledger: {e}");
            std::process::exit(1);
        });
        let payload = record.to_payload(task_id);
        led.append("cognitum_decision", work_order_id, &payload, now_ns())
            .unwrap_or_else(|e| {
                eprintln!("hf policy gate: cannot witness decision: {e}");
                std::process::exit(1);
            });

        match record.decision.as_str() {
            "permit" => {
                println!("hf policy gate: permit {action} (seq {})", record.sequence);
            }
            "defer" => {
                eprintln!(
                    "hf policy gate: defer {action} (seq {}) — human review required",
                    record.sequence
                );
                std::process::exit(1);
            }
            "deny" => {
                eprintln!("hf policy gate: deny {action} (seq {})", record.sequence);
                std::process::exit(1);
            }
            other => {
                eprintln!("hf policy gate: unexpected decision '{other}' for {action}");
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_record_payload_roundtrips() {
        let record = DecisionRecord {
            action_id: "edit-src-main".into(),
            decision: "permit".into(),
            sequence: 7,
            token_b64: Some("dGVzdA==".into()),
        };
        let payload = record.to_payload(Some("HFTASK-0017"));
        let v: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(v["schema"], "handoff.cognitum_decision.v1");
        assert_eq!(v["action_id"], "edit-src-main");
        assert_eq!(v["decision"], "permit");
        assert_eq!(v["sequence"], 7);
        assert_eq!(v["task_id"], "HFTASK-0017");
        assert!(v["token_b64"].as_str().is_some());
    }

    #[cfg(feature = "cognitum")]
    #[test]
    fn cognitum_default_thresholds_permit_benign_action() {
        let record = evaluate_action("test-browse", "read", "hf", None);
        assert_eq!(record.decision, "permit");
        assert!(record.sequence < 100);
        assert!(record.token_b64.is_some());
    }
}
