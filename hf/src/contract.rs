//! AgentContract proof at `hf handoff` — HFTASK-0004 (ADR-0011).
//!
//! The kernel's blake3 **intent-lock** (`work_order::IntentLock`) IS the agent contract.
//! At handoff we discharge that contract through the **`ruvector-verified`** formal layer —
//! the real RuVector proof-carrying crate (`ProofEnvironment` over the lean-agentic
//! dependent-type kernel, `Eq.refl` reflexive-equality proofs, and a tamper-evident
//! [`ProofAttestation`] that serializes into an RVF `WITNESS_SEG`) — and **fail closed**: an
//! unprovable contract blocks the handoff.
//!
//! Pattern mirrors `ruvector_verified::prove_dim_eq`: the obligation's equality is decided
//! in Rust (here, a collision-free **full-string** comparison of the recorded vs re-derived
//! contract hash — strictly sounder than reducing to a `u32` dimension), and only when it
//! holds is an `Eq.refl` proof term minted into the proof environment. Obligations:
//!
//! 1. `objective` integrity — `recorded.objective_hash == rederive(objective)`
//! 2. `path_scope` integrity — `recorded.path_scope_hash == rederive(path_scope)`
//! 3. `acceptance` integrity — `recorded.acceptance_hash == rederive(acceptance)`
//! 4. `completion` *(only when handed off as complete — status `Review`/`Done`)* —
//!    completion evidence exists (≥1 witnessed checkpoint).
//!
//! Re-derivation reuses [`WorkOrder::compute_intent_lock`] **exactly**, so the proof is
//! faithful to the live contract rather than a parallel hash. The resulting
//! `ProofAttestation` (proof-term hash, environment hash, lean-agentic verifier version) is
//! rendered into the packet — a real verification receipt, witnessed.

use std::hash::{Hash, Hasher};
use work_order::{Status, WorkOrder};

use ruvector_verified::invariants::symbols::EQ_REFL;
use ruvector_verified::{ProofAttestation, ProofEnvironment};

/// A discharged proof obligation: its name and the `Eq.refl` proof-term id minted for it.
#[derive(Debug, Clone)]
pub struct Obligation {
    pub name: &'static str,
    pub proof_term: u32,
}

/// Why a contract could not be proven — each variant fails the handoff closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofError {
    /// An intent-lock hash no longer matches the re-derived contract surface (drift).
    IntentDrift { task: String, field: &'static str },
    /// The task is handed off as complete but its completion cannot be proven.
    UnprovenCompletion { task: String, reason: String },
    /// The proof environment is malformed (the `Eq.refl` rule is absent) — fail closed.
    EnvironmentBroken { task: String },
}

impl std::fmt::Display for ProofError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProofError::IntentDrift { task, field } => write!(
                f,
                "intent drift: {task} — recorded {field}_hash ≠ re-derived (contract surface mutated without re-lock)"
            ),
            ProofError::UnprovenCompletion { task, reason } => {
                write!(f, "unproven completion: {task} — {reason}")
            }
            ProofError::EnvironmentBroken { task } => write!(
                f,
                "proof environment broken: {task} — the ruvector-verified Eq.refl rule is absent"
            ),
        }
    }
}

/// The completion evidence the kernel can witness for the active task (read from the ledger
/// by the caller — this module stays pure over the contract + evidence).
#[derive(Debug, Clone)]
pub struct CompletionEvidence {
    /// Current replayed status of the task.
    pub status: Status,
    /// Number of witnessed checkpoint transitions for the task in the ledger.
    pub checkpoints: usize,
    /// Current capsule North-Star doctrine hash (HFTASK-0047). Used to discharge the
    /// `intent:northstar` obligation when the recorded lock carries that surface. Empty is
    /// acceptable for a legacy partial lock (the obligation is simply not raised).
    #[allow(dead_code)]
    pub northstar_revision: String,
}

/// A machine-checked AgentContract proof, carrying the real `ruvector-verified` receipt.
#[derive(Debug, Clone)]
pub struct ContractProof {
    pub task: String,
    pub obligations: Vec<Obligation>,
    /// Total `Eq.refl` proof terms minted.
    pub proof_terms: u32,
    /// The tamper-evident `ruvector-verified` attestation (proof-term/environment hashes +
    /// lean-agentic verifier version; serializes into an RVF `WITNESS_SEG`).
    pub attestation: ProofAttestation,
    /// Binds the attestation to THIS contract's hash surface (the attestation's own hashes
    /// cover proof/env state, not the specific recorded hashes — this closes that gap).
    pub content_hash: u64,
}

/// Discharge one equality obligation through the `ruvector-verified` proof environment.
/// The decision is the exact full-string comparison (sound, collision-free); on success an
/// `Eq.refl` proof term is minted — exactly `prove_dim_eq`'s discipline, generalized off the
/// `u32`-dimension surface. Returns the proof-term id, or `None` if the values differ.
fn discharge(
    env: &mut ProofEnvironment,
    recorded: &str,
    rederived: &str,
) -> Result<Option<u32>, ()> {
    if recorded != rederived {
        return Ok(None);
    }
    // The RuVector proof environment must carry the reflexive-equality rule.
    env.require_symbol(EQ_REFL).map_err(|_| ())?;
    let proof_id = env.alloc_term();
    env.stats.proofs_verified += 1;
    Ok(Some(proof_id))
}

/// Prove the AgentContract for one active task. `Ok` carries the attestation; `Err` is a
/// fail-closed signal the caller turns into a blocked handoff.
pub fn prove_contract(
    task: &WorkOrder,
    evidence: &CompletionEvidence,
) -> Result<ContractProof, ProofError> {
    let mut env = ProofEnvironment::new();
    let mut obligations: Vec<Obligation> = Vec::new();
    let mut last_proof_id: u32 = 0;

    // Re-derive the intent-lock from the LIVE card fields, exactly as the kernel mints it.
    let rederived = WorkOrder::compute_intent_lock(
        &task.objective,
        &task.path_scope,
        &task.acceptance_criteria,
    );
    let recorded = &task.intent_lock;

    let checks: [(&'static str, &'static str, &String, &String); 3] = [
        (
            "intent:objective",
            "objective",
            &recorded.objective_hash,
            &rederived.objective_hash,
        ),
        (
            "intent:path_scope",
            "path_scope",
            &recorded.path_scope_hash,
            &rederived.path_scope_hash,
        ),
        (
            "intent:acceptance",
            "acceptance",
            &recorded.acceptance_hash,
            &rederived.acceptance_hash,
        ),
    ];
    for (name, field, rec, red) in checks {
        match discharge(&mut env, rec, red) {
            Ok(Some(proof_term)) => {
                last_proof_id = proof_term;
                obligations.push(Obligation { name, proof_term });
            }
            Ok(None) => {
                return Err(ProofError::IntentDrift {
                    task: task.id.clone(),
                    field,
                })
            }
            Err(()) => {
                return Err(ProofError::EnvironmentBroken {
                    task: task.id.clone(),
                })
            }
        }
    }

    // The two HFTASK-0047 surfaces (§12.1 constraint + capsule North-Star). Each obligation is
    // raised ONLY when the recorded lock actually carries that surface — a legacy partial lock
    // (empty field) raises nothing, so existing 3-obligation proofs are unchanged (no-downgrade).
    let extra: [(&'static str, &'static str, &str, String); 2] = [
        (
            "intent:constraint",
            "constraint",
            recorded.constraint_hash.as_str(),
            if recorded.constraint_hash.is_empty() {
                String::new()
            } else {
                task.constraint_hash()
            },
        ),
        (
            "intent:northstar",
            "northstar",
            recorded.northstar_revision.as_str(),
            evidence.northstar_revision.clone(),
        ),
    ];
    for (name, field, rec, red) in extra {
        if rec.is_empty() {
            continue; // legacy partial lock — surface not under contract
        }
        match discharge(&mut env, rec, &red) {
            Ok(Some(proof_term)) => {
                last_proof_id = proof_term;
                obligations.push(Obligation { name, proof_term });
            }
            Ok(None) => {
                return Err(ProofError::IntentDrift {
                    task: task.id.clone(),
                    field,
                })
            }
            Err(()) => {
                return Err(ProofError::EnvironmentBroken {
                    task: task.id.clone(),
                })
            }
        }
    }

    // Completion obligation — only when the task is being handed off AS COMPLETE.
    if matches!(evidence.status, Status::Review | Status::Done) {
        // Decide on the flag, then witness it: completion holds iff ≥1 witnessed checkpoint.
        let flag = if evidence.checkpoints > 0 { "1" } else { "0" };
        match discharge(&mut env, flag, "1") {
            Ok(Some(proof_term)) => {
                last_proof_id = proof_term;
                obligations.push(Obligation {
                    name: "completion",
                    proof_term,
                });
            }
            Ok(None) => {
                return Err(ProofError::UnprovenCompletion {
                    task: task.id.clone(),
                    reason: format!(
                        "status {:?} with no witnessed checkpoint — run `hf checkpoint {}` before handoff",
                        evidence.status, task.id
                    ),
                })
            }
            Err(()) => return Err(ProofError::EnvironmentBroken { task: task.id.clone() }),
        }
    }

    let attestation = ruvector_verified::proof_store::create_attestation(&env, last_proof_id);
    let content_hash = content_hash(&task.id, &obligations, recorded);
    Ok(ContractProof {
        task: task.id.clone(),
        obligations,
        proof_terms: env.terms_allocated(),
        attestation,
        content_hash,
    })
}

/// Bind the attestation to THIS contract's recorded hash surface (task id + obligation names
/// + the recorded intent-lock hashes), so the receipt is tied to the specific contract.
fn content_hash(task: &str, obligations: &[Obligation], lock: &work_order::IntentLock) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    task.hash(&mut h);
    for o in obligations {
        o.name.hash(&mut h);
        o.proof_term.hash(&mut h);
    }
    lock.objective_hash.hash(&mut h);
    lock.path_scope_hash.hash(&mut h);
    lock.acceptance_hash.hash(&mut h);
    // HFTASK-0047 surfaces — empty on a legacy partial lock, so the binding is byte-stable for
    // pre-0047 contracts (hashing "" is a no-op for those).
    lock.constraint_hash.hash(&mut h);
    lock.northstar_revision.hash(&mut h);
    h.finish()
}

/// Render the attestation as a packet section (ADR-0011: the proof is witnessed in the packet).
pub fn render_proof_section(p: &ContractProof) -> String {
    let mut s = String::new();
    s.push_str("\n## Contract Proof (ADR-0011 — ruvector-verified/Lean)\n");
    s.push_str(&format!(
        "Active task **{}** — AgentContract PROVEN via ruvector-verified ({} obligation(s)).\n",
        p.task,
        p.obligations.len()
    ));
    for o in &p.obligations {
        s.push_str(&format!(
            "- ✓ `{}` (Eq.refl proof-term #{})\n",
            o.name, o.proof_term
        ));
    }
    s.push_str(&format!(
        "{} proof-term(s) · proof-hash `{}` · binding `{:#018x}` · verifier `{:#010x}` (lean-agentic 0.1.0).\n",
        p.proof_terms,
        short_hex(&p.attestation.proof_term_hash),
        p.content_hash,
        p.attestation.verifier_version
    ));
    s
}

/// First 8 bytes of a 32-byte hash, hex — enough to identify the receipt in a packet line.
fn short_hex(h: &[u8; 32]) -> String {
    h[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_task(status: Status) -> WorkOrder {
        let objective = "prove the contract".to_string();
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec!["implemented + cargo test green".to_string()];
        let intent_lock = WorkOrder::compute_intent_lock(&objective, &path_scope, &acceptance);
        WorkOrder {
            schema: "handoff.task.v1".to_string(),
            id: "HFTASK-TEST".to_string(),
            title: "test contract".to_string(),
            status,
            priority: work_order::Priority::P1,
            objective,
            path_scope,
            acceptance_criteria: acceptance,
            test_commands: vec![],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: String::new(),
            role: None,
            intent_lock,
        }
    }

    fn ev(status: Status, checkpoints: usize) -> CompletionEvidence {
        CompletionEvidence {
            status,
            checkpoints,
            northstar_revision: String::new(),
        }
    }

    #[test]
    fn intact_contract_proves_intent_obligations() {
        let task = mk_task(Status::Checkpointed);
        let proof = prove_contract(&task, &ev(Status::Checkpointed, 1))
            .expect("intact contract should prove");
        // Mid-work (not complete): exactly the 3 intent-integrity obligations, no completion.
        assert_eq!(proof.obligations.len(), 3);
        assert_eq!(proof.proof_terms, 3);
        // Real ruvector-verified receipt: lean-agentic 0.1.0 = 0x0001_0000.
        assert_eq!(proof.attestation.verifier_version, 0x0001_0000);
        assert!(proof
            .obligations
            .iter()
            .all(|o| o.name.starts_with("intent:")));
    }

    #[test]
    fn drifted_intent_blocks_handoff() {
        let mut task = mk_task(Status::Checkpointed);
        // Mutate the objective WITHOUT re-locking: the recorded hash no longer matches.
        task.objective = "a different objective entirely".to_string();
        let err = prove_contract(&task, &ev(Status::Checkpointed, 1))
            .expect_err("drift must fail closed");
        assert_eq!(
            err,
            ProofError::IntentDrift {
                task: "HFTASK-TEST".to_string(),
                field: "objective",
            }
        );
    }

    #[test]
    fn complete_task_with_checkpoint_proves_completion() {
        let task = mk_task(Status::Done);
        let proof =
            prove_contract(&task, &ev(Status::Done, 2)).expect("done + checkpoint should prove");
        // 3 intent + 1 completion.
        assert_eq!(proof.obligations.len(), 4);
        assert!(proof.obligations.iter().any(|o| o.name == "completion"));
    }

    #[test]
    fn complete_task_without_checkpoint_is_unproven() {
        let task = mk_task(Status::Done);
        let err =
            prove_contract(&task, &ev(Status::Done, 0)).expect_err("no checkpoint must block");
        match err {
            ProofError::UnprovenCompletion { task, .. } => assert_eq!(task, "HFTASK-TEST"),
            other => panic!("expected UnprovenCompletion, got {other:?}"),
        }
    }

    #[test]
    fn attestation_is_deterministic() {
        let task = mk_task(Status::Checkpointed);
        let a = prove_contract(&task, &ev(Status::Checkpointed, 1)).unwrap();
        let b = prove_contract(&task, &ev(Status::Checkpointed, 1)).unwrap();
        // The content binding + the proof/environment hashes are deterministic for the same
        // contract (the attestation's wall-clock timestamp is the only varying field).
        assert_eq!(a.content_hash, b.content_hash);
        assert_eq!(a.attestation.proof_term_hash, b.attestation.proof_term_hash);
        assert_eq!(
            a.attestation.environment_hash,
            b.attestation.environment_hash
        );
    }

    #[test]
    fn full_lock_proves_five_intent_obligations() {
        // HFTASK-0047: a 5-field lock raises the two extra obligations and they discharge.
        let mut task = mk_task(Status::Checkpointed);
        task.intent_lock = task.full_intent_lock("blake3:northstar-rev-1");
        let mut evidence = ev(Status::Checkpointed, 1);
        evidence.northstar_revision = "blake3:northstar-rev-1".to_string();
        let proof = prove_contract(&task, &evidence).expect("full contract should prove");
        // 3 base + constraint + northstar = 5 intent obligations (mid-work, no completion).
        assert_eq!(proof.obligations.len(), 5);
        assert!(proof
            .obligations
            .iter()
            .any(|o| o.name == "intent:constraint"));
        assert!(proof
            .obligations
            .iter()
            .any(|o| o.name == "intent:northstar"));
    }

    #[test]
    fn constraint_drift_on_full_lock_blocks_handoff() {
        let mut task = mk_task(Status::Checkpointed);
        task.intent_lock = task.full_intent_lock("blake3:rev-1");
        // Mutate the permission surface WITHOUT re-locking.
        task.allows_network = !task.allows_network;
        let mut evidence = ev(Status::Checkpointed, 1);
        evidence.northstar_revision = "blake3:rev-1".to_string();
        let err = prove_contract(&task, &evidence).expect_err("constraint drift must fail closed");
        assert_eq!(
            err,
            ProofError::IntentDrift {
                task: "HFTASK-TEST".to_string(),
                field: "constraint",
            }
        );
    }

    #[test]
    fn northstar_revision_drift_blocks_handoff() {
        let mut task = mk_task(Status::Checkpointed);
        task.intent_lock = task.full_intent_lock("blake3:rev-1");
        let mut evidence = ev(Status::Checkpointed, 1);
        evidence.northstar_revision = "blake3:rev-2".to_string(); // doctrine moved under it
        let err = prove_contract(&task, &evidence).expect_err("northstar drift must fail closed");
        assert_eq!(
            err,
            ProofError::IntentDrift {
                task: "HFTASK-TEST".to_string(),
                field: "northstar",
            }
        );
    }

    #[test]
    fn legacy_partial_lock_raises_no_extra_obligations() {
        // No-downgrade: a pre-0047 lock still proves exactly 3 intent obligations even when the
        // evidence carries a northstar revision (the surface is not under that contract).
        let task = mk_task(Status::Checkpointed);
        let mut evidence = ev(Status::Checkpointed, 1);
        evidence.northstar_revision = "blake3:rev-1".to_string();
        let proof = prove_contract(&task, &evidence).expect("legacy contract still proves");
        assert_eq!(proof.obligations.len(), 3);
        assert!(!proof
            .obligations
            .iter()
            .any(|o| o.name == "intent:constraint"));
        assert!(!proof
            .obligations
            .iter()
            .any(|o| o.name == "intent:northstar"));
    }
}
