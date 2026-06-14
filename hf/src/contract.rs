//! AgentContract proof at `hf handoff` — HFTASK-0004 (ADR-0011).
//!
//! The kernel's blake3 **intent-lock** (`work_order::IntentLock`) IS the agent contract.
//! At handoff we machine-check that contract with the [`lean-agentic`] dependent-type kernel
//! — the same Lean engine `ruvector-verified` is built on — and **fail closed**: an
//! unprovable contract blocks the handoff.
//!
//! The proof discipline mirrors `ruvector-verified::prove_dim_eq`: a refl proof term is
//! constructible **iff** the proposition holds, verified through the kernel's definitional
//! -equality checker ([`Converter::is_def_eq`]). The obligations for the active claimed task:
//!
//! 1. `objective` integrity — `recorded.objective_hash ≡ rederive(objective)`
//! 2. `path_scope` integrity — `recorded.path_scope_hash ≡ rederive(path_scope)`
//! 3. `acceptance` integrity — `recorded.acceptance_hash ≡ rederive(acceptance)`
//! 4. `completion` *(only when handed off as complete — status `Review`/`Done`)* —
//!    completion evidence exists (≥1 witnessed checkpoint).
//!
//! Re-derivation reuses [`WorkOrder::compute_intent_lock`] **exactly**, so the proof is
//! faithful to the live contract rather than a parallel hash.

use lean_agentic::conversion::Converter;
use lean_agentic::{Arena, Context, Environment, TermId};
use std::hash::{Hash, Hasher};
use work_order::{Status, WorkOrder};

/// Lean verifier version stamped into attestations (lean-agentic 0.1.0 = `0x0001_0000`),
/// matching `ruvector-verified::ProofAttestation`'s `verifier_version` encoding.
pub const VERIFIER_VERSION: u32 = 0x0001_0000;

/// A discharged proof obligation: its name and the lean proof-term id constructed for it.
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
}

/// A machine-checked AgentContract proof (mirrors `ruvector-verified::ProofAttestation`).
#[derive(Debug, Clone)]
pub struct ContractProof {
    pub task: String,
    pub obligations: Vec<Obligation>,
    /// Total lean proof terms constructed.
    pub proof_terms: u32,
    /// Content hash over the proof + environment state (tamper-evident attestation).
    pub attestation: u64,
    pub verifier_version: u32,
}

/// One-shot lean-agentic proof environment for a single contract proof.
struct ProofEnv {
    arena: Arena,
    env: Environment,
    ctx: Context,
    conv: Converter,
    /// Monotonic proof-term counter (the constructed-proof id space).
    terms: u32,
}

impl ProofEnv {
    fn new() -> Self {
        Self {
            arena: Arena::new(),
            env: Environment::new(),
            ctx: Context::new(),
            conv: Converter::new(),
            terms: 0,
        }
    }

    /// Encode a byte string as a lean term: a `len`-headed application spine of one `Nat`
    /// literal per byte. Equal strings intern to def-eq terms; differing strings do not.
    fn encode(&mut self, s: &str) -> TermId {
        let head = self.arena.mk_nat(s.len() as u64);
        let args: Vec<TermId> = s.bytes().map(|b| self.arena.mk_nat(b as u64)).collect();
        self.arena.mk_app_spine(head, &args)
    }

    /// Prove `a ≡ b` through the kernel's definitional-equality checker. Returns the
    /// constructed proof-term id iff the terms are def-eq (the refl proof exists only then).
    fn prove_eq(&mut self, a: &str, b: &str) -> Option<u32> {
        let t1 = self.encode(a);
        let t2 = self.encode(b);
        let eq = self
            .conv
            .is_def_eq(&mut self.arena, &self.env, &self.ctx, t1, t2)
            .unwrap_or(false);
        if eq {
            self.terms += 1;
            Some(self.terms)
        } else {
            None
        }
    }
}

/// Prove the AgentContract for one active task. `Ok` carries the attestation; `Err` is a
/// fail-closed signal the caller turns into a blocked handoff.
pub fn prove_contract(
    task: &WorkOrder,
    evidence: &CompletionEvidence,
) -> Result<ContractProof, ProofError> {
    let mut pe = ProofEnv::new();
    let mut obligations: Vec<Obligation> = Vec::new();

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
        match pe.prove_eq(rec, red) {
            Some(proof_term) => obligations.push(Obligation { name, proof_term }),
            None => {
                return Err(ProofError::IntentDrift {
                    task: task.id.clone(),
                    field,
                })
            }
        }
    }

    // Completion obligation — only when the task is being handed off AS COMPLETE.
    if matches!(evidence.status, Status::Review | Status::Done) {
        // Proof: `completion_flag ≡ TRUE`, where the flag holds iff ≥1 witnessed checkpoint.
        let flag = if evidence.checkpoints > 0 { "1" } else { "0" };
        match pe.prove_eq(flag, "1") {
            Some(proof_term) => obligations.push(Obligation {
                name: "completion",
                proof_term,
            }),
            None => {
                return Err(ProofError::UnprovenCompletion {
                    task: task.id.clone(),
                    reason: format!(
                        "status {:?} with no witnessed checkpoint — run `hf checkpoint {}` before handoff",
                        evidence.status, task.id
                    ),
                })
            }
        }
    }

    let attestation = content_hash(&task.id, &obligations, pe.terms);
    Ok(ContractProof {
        task: task.id.clone(),
        obligations,
        proof_terms: pe.terms,
        attestation,
        verifier_version: VERIFIER_VERSION,
    })
}

/// Tamper-evident content hash over the proof + environment state (mirrors
/// `ruvector-verified::ProofAttestation::content_hash`).
fn content_hash(task: &str, obligations: &[Obligation], terms: u32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    task.hash(&mut h);
    terms.hash(&mut h);
    VERIFIER_VERSION.hash(&mut h);
    for o in obligations {
        o.name.hash(&mut h);
        o.proof_term.hash(&mut h);
    }
    h.finish()
}

/// Render the attestation as a packet section (ADR-0011: the proof is witnessed in the packet).
pub fn render_proof_section(p: &ContractProof) -> String {
    let mut s = String::new();
    s.push_str("\n## Contract Proof (ADR-0011 — ruvector-verified/Lean)\n");
    s.push_str(&format!(
        "Active task **{}** — AgentContract PROVEN via lean-agentic ({} obligation(s)).\n",
        p.task,
        p.obligations.len()
    ));
    for o in &p.obligations {
        s.push_str(&format!(
            "- ✓ `{}` (proof-term #{})\n",
            o.name, o.proof_term
        ));
    }
    s.push_str(&format!(
        "{} lean proof-term(s) · attestation `{:#018x}` · verifier `{:#010x}` (lean-agentic 0.1.0).\n",
        p.proof_terms, p.attestation, p.verifier_version
    ));
    s
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

    #[test]
    fn intact_contract_proves_intent_obligations() {
        let task = mk_task(Status::Checkpointed);
        let ev = CompletionEvidence {
            status: Status::Checkpointed,
            checkpoints: 1,
        };
        let proof = prove_contract(&task, &ev).expect("intact contract should prove");
        // Mid-work (not complete): exactly the 3 intent-integrity obligations, no completion.
        assert_eq!(proof.obligations.len(), 3);
        assert_eq!(proof.proof_terms, 3);
        assert_eq!(proof.verifier_version, VERIFIER_VERSION);
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
        let ev = CompletionEvidence {
            status: Status::Checkpointed,
            checkpoints: 1,
        };
        let err = prove_contract(&task, &ev).expect_err("drift must fail closed");
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
        let ev = CompletionEvidence {
            status: Status::Done,
            checkpoints: 2,
        };
        let proof = prove_contract(&task, &ev).expect("done + checkpoint should prove");
        // 3 intent + 1 completion.
        assert_eq!(proof.obligations.len(), 4);
        assert!(proof.obligations.iter().any(|o| o.name == "completion"));
    }

    #[test]
    fn complete_task_without_checkpoint_is_unproven() {
        let task = mk_task(Status::Done);
        let ev = CompletionEvidence {
            status: Status::Done,
            checkpoints: 0, // never checkpointed → unproven completion
        };
        let err = prove_contract(&task, &ev).expect_err("no checkpoint must block");
        match err {
            ProofError::UnprovenCompletion { task, .. } => assert_eq!(task, "HFTASK-TEST"),
            other => panic!("expected UnprovenCompletion, got {other:?}"),
        }
    }

    #[test]
    fn attestation_is_deterministic() {
        let task = mk_task(Status::Checkpointed);
        let ev = CompletionEvidence {
            status: Status::Checkpointed,
            checkpoints: 1,
        };
        let a = prove_contract(&task, &ev).unwrap();
        let b = prove_contract(&task, &ev).unwrap();
        assert_eq!(a.attestation, b.attestation);
    }
}
