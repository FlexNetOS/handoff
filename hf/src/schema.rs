//! HFTASK-0057 (PRD §7.3/§23): JSON Schema runtime validation + the `hf schema` verb.
//!
//! The fail-closed half of card loading. The schema is *generated* from the live
//! `work_order::WorkOrder` types (schemars, see `work_order::task_schema_json`) so it can never
//! drift from the Rust contract. This module compiles that schema **once** (`OnceLock`) and
//! validates a card's raw `serde_json::Value` before deserialization, so a card that violates
//! the contract — missing `intent_lock`, a bad `id`, the wrong `schema` const — is rejected
//! **loudly** (named violations) instead of being silently dropped (the FAIL-OPEN anti-pattern
//! that cost a whole session when card #95's missing `intent_lock` vanished from `hf status`).

use std::sync::OnceLock;

use jsonschema::Validator;

/// The compiled handoff.task.v1 validator, built once from `work_order::task_schema_json()`.
/// `OnceLock` so the (small) schema is parsed + compiled a single time per process.
fn validator() -> &'static Validator {
    static VALIDATOR: OnceLock<Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema_str = work_order::task_schema_json();
        let schema: serde_json::Value =
            serde_json::from_str(&schema_str).expect("generated task schema must be valid JSON");
        // Fail-closed: a broken *generated* schema is a build-time contract bug, not a card bug.
        jsonschema::validator_for(&schema).expect("generated task schema must compile")
    })
}

/// Validate a card's raw JSON value against the handoff.task.v1 schema. Returns `Ok(())` when
/// the card conforms, or `Err(message)` with a concise, human-readable list of every violation
/// (each naming the offending instance path). Used at the card-load boundary to reject a
/// non-conformant card loudly rather than dropping it silently.
pub fn validate_card(value: &serde_json::Value) -> Result<(), String> {
    let v = validator();
    let mut violations: Vec<String> = v
        .iter_errors(value)
        .map(|e| {
            let path = e.instance_path().to_string();
            if path.is_empty() {
                format!("(root): {e}")
            } else {
                format!("{path}: {e}")
            }
        })
        .collect();
    if violations.is_empty() {
        Ok(())
    } else {
        violations.sort();
        Err(violations.join("; "))
    }
}

/// On-disk location of the committed canonical schema (relative to the repo root).
fn schema_file() -> std::path::PathBuf {
    std::path::Path::new("schemas").join("task.schema.json")
}

/// `hf schema [--check|--write]`:
///   * `--write` writes `schemas/task.schema.json` (pretty) from the generated schema.
///   * `--check` regenerates and diffs against the on-disk file; exit 1 on drift.
///   * (bare)    prints the schema to stdout.
///
/// Returns the process exit code so `main` can dispatch it uniformly.
pub fn cmd_schema(args: &[String]) -> i32 {
    let generated = work_order::task_schema_json();
    match args.first().map(String::as_str) {
        Some("--write") => {
            let path = schema_file();
            if let Some(parent) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    eprintln!("hf: cannot create {}: {e}", parent.display());
                    return 1;
                }
            }
            // Trailing newline so the committed file ends cleanly (git-friendly).
            if let Err(e) = std::fs::write(&path, format!("{generated}\n")) {
                eprintln!("hf: cannot write {}: {e}", path.display());
                return 1;
            }
            println!("hf: wrote {}", path.display());
            0
        }
        Some("--check") => {
            let path = schema_file();
            match std::fs::read_to_string(&path) {
                Ok(on_disk) => {
                    if on_disk.trim_end() == generated.trim_end() {
                        println!("hf: schema up to date ({})", path.display());
                        0
                    } else {
                        eprintln!(
                            "hf: schema DRIFT — {} differs from the generated schema. \
                             Run `hf schema --write` to regenerate.",
                            path.display()
                        );
                        1
                    }
                }
                Err(e) => {
                    eprintln!(
                        "hf: cannot read {} ({e}); run `hf schema --write` to create it.",
                        path.display()
                    );
                    1
                }
            }
        }
        Some(other) => {
            eprintln!("hf: schema: unknown flag `{other}` (use --check | --write)");
            1
        }
        None => {
            println!("{generated}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A minimal but schema-valid handoff.task.v1 card value.
    fn valid_card() -> serde_json::Value {
        json!({
            "schema": "handoff.task.v1",
            "id": "TASK-0001",
            "title": "A valid card",
            "status": "backlog",
            "priority": "P1",
            "objective": "Do a thing that is long enough",
            "path_scope": ["hf/src/"],
            "acceptance_criteria": ["it works"],
            "test_commands": ["cargo test"],
            "correlation_id": "wf-0001",
            "intent_lock": {
                "objective_hash": "blake3:aa",
                "path_scope_hash": "blake3:bb",
                "acceptance_hash": "blake3:cc"
            }
        })
    }

    #[test]
    fn valid_card_validates_ok() {
        assert!(
            validate_card(&valid_card()).is_ok(),
            "a well-formed card must validate: {:?}",
            validate_card(&valid_card())
        );
    }

    #[test]
    fn card_missing_intent_lock_is_rejected_naming_the_field() {
        let mut card = valid_card();
        card.as_object_mut().unwrap().remove("intent_lock");
        let err = validate_card(&card).expect_err("missing intent_lock must be rejected");
        assert!(
            err.contains("intent_lock"),
            "rejection must name the missing field, got: {err}"
        );
    }

    #[test]
    fn card_with_wrong_schema_const_is_rejected() {
        let mut card = valid_card();
        card["schema"] = json!("handoff.task.v0");
        let err = validate_card(&card).expect_err("wrong schema const must be rejected");
        assert!(!err.is_empty(), "rejection message must be non-empty");
    }

    #[test]
    fn card_with_bad_id_is_rejected() {
        let mut card = valid_card();
        // Not matching ^TASK-[0-9]{4,}$ — a free-form id violates the pattern constraint.
        card["id"] = json!("not-a-task-id");
        let err = validate_card(&card).expect_err("a malformed id must be rejected");
        assert!(!err.is_empty(), "rejection message must be non-empty");
    }
}
