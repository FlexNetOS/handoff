// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! HFTASK-0057 (PRD §7.3/§23): JSON Schema runtime validation + the `hf schema` verb.
//!
//! HFTASK-0081 (ADR-0019 D5 #4): peeled out of the `hf` monolith into the `handoff-schema` crate.
//! `hf` aliases it as `schema` so existing `schema::validate_card` / `schema::cmd_schema` paths
//! stay valid (behavior-preserving). Depends only on `work-order` (the generated schema source).
//!
//! The fail-closed half of card loading. The schema is *generated* from the live
//! `work_order::WorkOrder` types (schemars, see `work_order::task_schema_json`) so it can never
//! drift from the Rust contract. This module compiles that schema **once** (`OnceLock`) and
//! validates a card's raw `serde_json::Value` before deserialization, so a card that violates
//! the contract — missing `intent_lock`, a bad `id`, the wrong `schema` const — is rejected
//! **loudly** (named violations) instead of being silently dropped (the FAIL-OPEN anti-pattern
//! that cost a whole session when card #95's missing `intent_lock` vanished from `hf status`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use jsonschema::Validator;

/// The compiled handoff.task.v1 validator, built once from `work_order::task_schema_json()`.
/// `OnceLock` so the (small) schema is parsed + compiled a single time per process.
///
/// HFTASK-0080: the two `expect`s here validate the *generated* schema, not user input — a broken
/// generated schema is a build-time contract bug (schemars produced invalid JSON / an
/// uncompilable schema), so aborting loudly IS the fail-closed behavior. Justified at the fn.
#[allow(clippy::expect_used)]
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

const TASK_SCHEMA_REL: &str = "schemas/task.schema.json";

#[derive(Debug, Clone, PartialEq, Eq)]
enum SchemaSource {
    File(PathBuf),
    EmbeddedGenerated,
}

#[derive(Debug, Clone)]
struct SchemaResolution {
    source: SchemaSource,
    content: String,
    attempts: Vec<String>,
}

fn task_schema_path(root: &Path) -> PathBuf {
    root.join(TASK_SCHEMA_REL)
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|p| p == &path) {
        paths.push(path);
    }
}

/// Candidate locations for the committed canonical schema, ordered from local to fleet-kernel.
///
/// A member repo such as `meta/Weave` usually has no local `schemas/` directory, but the fleet
/// root has `.meta.yaml` and a sibling `handoff/` kernel checkout that carries the canonical
/// schema. `HANDOFF_KERNEL_HOME` is an explicit override for packaged/non-standard layouts.
fn schema_file_candidates(cwd: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    push_unique(&mut out, task_schema_path(cwd));
    match std::env::var("HANDOFF_KERNEL_HOME") {
        Ok(home) if !home.trim().is_empty() => {
            push_unique(&mut out, task_schema_path(Path::new(&home)));
        }
        _ => {}
    }
    for ancestor in cwd.ancestors().skip(1) {
        push_unique(&mut out, task_schema_path(ancestor));
        if ancestor.join(".meta.yaml").exists() {
            push_unique(&mut out, task_schema_path(&ancestor.join("handoff")));
        }
    }
    out
}

fn format_attempts(attempts: &[String]) -> String {
    attempts
        .iter()
        .map(|a| format!("  - {a}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn resolution_error(attempts: &[String]) -> String {
    format!(
        "hf: cannot resolve {TASK_SCHEMA_REL}; attempted:\n{}\nrepair: from the kernel home run \
         `hf schema --write`, or set HANDOFF_KERNEL_HOME=<meta>/handoff and retry.",
        format_attempts(attempts)
    )
}

fn resolve_task_schema_from(
    cwd: &Path,
    generated: &str,
    allow_embedded: bool,
) -> Result<SchemaResolution, String> {
    let mut attempts = Vec::new();
    for path in schema_file_candidates(cwd) {
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    attempts.push(format!("{}: found", path.display()));
                    return Ok(SchemaResolution {
                        source: SchemaSource::File(path),
                        content,
                        attempts,
                    });
                }
                Err(e) => {
                    attempts.push(format!("{}: read failed ({e})", path.display()));
                    return Err(resolution_error(&attempts));
                }
            }
        } else {
            attempts.push(format!("{}: missing", path.display()));
        }
    }
    if allow_embedded {
        attempts.push("embedded generated work_order::task_schema_json(): available".to_string());
        Ok(SchemaResolution {
            source: SchemaSource::EmbeddedGenerated,
            content: generated.to_string(),
            attempts,
        })
    } else {
        Err(resolution_error(&attempts))
    }
}

/// On-disk location written by `hf schema --write` (relative to the current repo).
fn schema_file() -> PathBuf {
    Path::new(TASK_SCHEMA_REL).to_path_buf()
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
                match std::fs::create_dir_all(parent) {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("hf: cannot create {}: {e}", parent.display());
                        return 1;
                    }
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
            match resolve_task_schema_from(
                &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                &generated,
                true,
            ) {
                Ok(resolution) => {
                    if resolution.content.trim_end() == generated.trim_end() {
                        match &resolution.source {
                            SchemaSource::File(path) => {
                                println!("hf: schema up to date ({})", path.display());
                            }
                            SchemaSource::EmbeddedGenerated => {
                                println!(
                                    "hf: schema up to date (embedded generated {TASK_SCHEMA_REL}; \
                                     no local schema file required)"
                                );
                            }
                        }
                        0
                    } else {
                        let where_from = match &resolution.source {
                            SchemaSource::File(path) => path.display().to_string(),
                            SchemaSource::EmbeddedGenerated => {
                                format!("embedded generated {TASK_SCHEMA_REL}")
                            }
                        };
                        eprintln!(
                            "hf: schema DRIFT — {where_from} differs from the generated schema. \
                             Run `hf schema --write` from the kernel home to regenerate.\n\
                             resolution attempts:\n{}",
                            format_attempts(&resolution.attempts)
                        );
                        1
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "handoff-schema-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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
        // Not matching ^[A-Z]*TASK-[A-Z0-9][A-Z0-9-]*$ — a lowercase free-form id violates it.
        card["id"] = json!("not-a-task-id");
        let err = validate_card(&card).expect_err("a malformed id must be rejected");
        assert!(!err.is_empty(), "rejection message must be non-empty");
    }

    #[test]
    fn schema_check_resolves_kernel_cwd_schema_file() {
        let root = temp_dir("kernel");
        let schema_path = task_schema_path(&root);
        std::fs::create_dir_all(schema_path.parent().unwrap()).unwrap();
        let generated = work_order::task_schema_json();
        std::fs::write(&schema_path, format!("{generated}\n")).unwrap();

        let resolved = resolve_task_schema_from(&root, &generated, true).unwrap();
        assert_eq!(resolved.source, SchemaSource::File(schema_path));
        assert_eq!(resolved.content.trim_end(), generated.trim_end());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn schema_check_from_member_resolves_sibling_kernel_schema() {
        let meta = temp_dir("member");
        std::fs::write(meta.join(".meta.yaml"), "repos: []\n").unwrap();
        let member = meta.join("Weave");
        std::fs::create_dir_all(&member).unwrap();
        let kernel_schema = task_schema_path(&meta.join("handoff"));
        std::fs::create_dir_all(kernel_schema.parent().unwrap()).unwrap();
        let generated = work_order::task_schema_json();
        std::fs::write(&kernel_schema, format!("{generated}\n")).unwrap();

        let resolved = resolve_task_schema_from(&member, &generated, true).unwrap();
        assert_eq!(resolved.source, SchemaSource::File(kernel_schema));
        assert!(
            resolved.attempts.iter().any(|a| {
                a.replace('\\', "/")
                    .contains("Weave/schemas/task.schema.json")
                    && a.contains("missing")
            }),
            "member-local miss should be recorded: {:?}",
            resolved.attempts
        );

        std::fs::remove_dir_all(&meta).ok();
    }

    #[test]
    fn schema_check_member_without_files_uses_embedded_generated_schema() {
        let member = temp_dir("embedded-member");
        let generated = work_order::task_schema_json();

        let resolved = resolve_task_schema_from(&member, &generated, true).unwrap();
        assert_eq!(resolved.source, SchemaSource::EmbeddedGenerated);
        assert_eq!(resolved.content.trim_end(), generated.trim_end());
        assert!(
            resolved
                .attempts
                .iter()
                .any(|a| a.contains("embedded generated") && a.contains("available")),
            "embedded fallback should be explicit: {:?}",
            resolved.attempts
        );

        std::fs::remove_dir_all(&member).ok();
    }

    #[test]
    fn schema_resolution_failure_names_attempts_and_repair() {
        let member = temp_dir("missing");
        let generated = work_order::task_schema_json();

        let err = resolve_task_schema_from(&member, &generated, false)
            .expect_err("without embedded fallback, missing files must fail closed");
        assert!(err.contains("cannot resolve schemas/task.schema.json"));
        assert!(err.contains("attempted:"));
        assert!(err.contains("repair:"));
        assert!(err.contains("hf schema --write"));

        std::fs::remove_dir_all(&member).ok();
    }
}
