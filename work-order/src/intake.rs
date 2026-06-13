//! Deterministic vibe-Intent → verifiable spec synthesis (HFTASK-0003).
//!
//! A prompt_hub `SwarmBundle` carries `role_prompts` (prompt strings, empty in prod) and a
//! Handlebars `handoff_template` — it has **no** `path_scope`, `acceptance_criteria`, or
//! `test_commands`. Without synthesis every dispatched `WorkOrder` is unverifiable by the
//! drift gate (PRD §12.3 #7 "did tests map to acceptance criteria?", §12.5 "completion
//! without tests"). The S1 spike papered over this with `path_scope: ["."]` (defeats the
//! out-of-scope-write check) and `test_commands: []` (a guaranteed completion hard-fail).
//!
//! This module closes the gap **deterministically** — never via an LLM. The upstream
//! `IntentClassifier` (`prompt_hub/.../vibe.rs:127`) is itself deterministic keyword
//! heuristics, so the coherent downstream is a pure, table-driven mapping that yields
//! byte-identical cards (and identical blake3 intent_locks) on re-run. LLM-authored
//! acceptance criteria are disqualified for a *verification* gate because they reflect the
//! current (possibly buggy) implementation rather than the intended behaviour
//! (Konstantinou et al., ICST 2025) — they would rubber-stamp drift instead of catching it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Mirror of prompt_hub's `vibe` `Intent` (`prompt_hub/prompt-hub/src/models.rs:547`),
/// reduced to the fields the synthesis actually consumes. `domain` / `task_type` are kept
/// as lowercase strings (the upstream enum variants serialize to these) so the contract is
/// resilient to upstream enum churn and trivially constructible from `--intent <json>` or
/// the local classifier. `extracted_entities` mirrors `vibe.rs:233` (notably `"language"`).
///
/// A `BTreeMap` (not `HashMap`) is used for `extracted_entities` so iteration order — and
/// therefore any derived string — is deterministic, the core HFTASK-0003 invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub raw_text: String,
    /// Domain: coding|devops|security|analysis|design|datascience|testing|documentation|writing|general
    #[serde(default = "default_general")]
    pub domain: String,
    /// TaskType: create|fix|improve|explain|convert|test|deploy|review
    #[serde(default = "default_create")]
    pub task_type: String,
    /// Mirrors vibe.rs extract_entities: {"language": "rust"|"python"|...}, {"framework": ...}.
    #[serde(default)]
    pub extracted_entities: BTreeMap<String, String>,
}

fn default_general() -> String {
    "general".to_string()
}
fn default_create() -> String {
    "create".to_string()
}

impl Intent {
    /// Construct an Intent from raw vibe text via deterministic heuristics that mirror
    /// prompt_hub's `IntentClassifier` (`vibe.rs:153,184,233`). Pure: same text in → same
    /// Intent out, no network, no LLM. Lower-cased keyword matching only.
    pub fn classify(raw_text: &str) -> Self {
        let lc = raw_text.to_lowercase();
        let domain = detect_domain(&lc);
        let task_type = detect_task_type(&lc);
        let mut extracted_entities = BTreeMap::new();
        if let Some(lang) = detect_language(&lc) {
            extracted_entities.insert("language".to_string(), lang);
        }
        Intent {
            raw_text: raw_text.to_string(),
            domain,
            task_type,
            extracted_entities,
        }
    }

    fn language(&self) -> Option<&str> {
        self.extracted_entities.get("language").map(String::as_str)
    }
}

/// `vibe.rs:153` detect_domain — keyword → domain. First match wins (deterministic order).
fn detect_domain(lc: &str) -> String {
    const TABLE: &[(&[&str], &str)] = &[
        (
            &[
                "deploy",
                "release",
                "ci/cd",
                "pipeline",
                "docker",
                "kubernetes",
            ],
            "devops",
        ),
        (
            &["security", "auth", "vulnerability", "exploit", "cve"],
            "security",
        ),
        (&["test", "coverage", "regression", "assert"], "testing"),
        (&["document", "docs", "readme", "comment"], "documentation"),
        (&["design", "ui", "ux", "mockup", "layout"], "design"),
        (&["data", "dataset", "model", "train", "ml"], "datascience"),
        (&["analy", "investigate", "audit", "report"], "analysis"),
        (&["write", "draft", "prose"], "writing"),
        (
            &[
                "code",
                "implement",
                "function",
                "bug",
                "fix",
                "refactor",
                "api",
                "build",
            ],
            "coding",
        ),
    ];
    for (kws, domain) in TABLE {
        if kws.iter().any(|k| lc.contains(k)) {
            return domain.to_string();
        }
    }
    "general".to_string()
}

/// `vibe.rs:184` detect_task_type — prefix/keyword → task type. First match wins.
fn detect_task_type(lc: &str) -> String {
    const TABLE: &[(&[&str], &str)] = &[
        (&["fix", "bug", "repair", "patch"], "fix"),
        (&["improve", "optimize", "refactor", "enhance"], "improve"),
        (&["convert", "migrate", "port", "translate"], "convert"),
        (&["explain", "describe", "document", "clarify"], "explain"),
        (&["review", "audit", "inspect"], "review"),
        (&["test", "verify", "validate"], "test"),
        (&["deploy", "release", "ship"], "deploy"),
        (
            &["create", "add", "implement", "build", "write", "design"],
            "create",
        ),
    ];
    for (kws, tt) in TABLE {
        if kws.iter().any(|k| lc.contains(k)) {
            return tt.to_string();
        }
    }
    "create".to_string()
}

/// `vibe.rs:251` extract_entities (language slice) — keyword → canonical language token.
fn detect_language(lc: &str) -> Option<String> {
    const TABLE: &[(&[&str], &str)] = &[
        (&["rust", "cargo", "clippy", ".rs"], "rust"),
        (&["python", "pytest", ".py", "pip"], "python"),
        (&["golang", "go ", "go.mod", " go", "goroutine"], "go"),
        (&["typescript", "ts ", ".ts", "tsx"], "typescript"),
        (&["javascript", "node", ".js", "npm"], "javascript"),
    ];
    for (kws, lang) in TABLE {
        if kws.iter().any(|k| lc.contains(k)) {
            return Some(lang.to_string());
        }
    }
    None
}

/// The synthesized, verifiable spec triple — the three gate-bearing fields a `SwarmBundle`
/// does not carry. Returned by [`synthesize_spec`] so both `hf intake` and (as a follow-up)
/// `hf/src/kb.rs::work_order_from_kb_doc` can share one well-tested synthesis path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthSpec {
    pub path_scope: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub test_commands: Vec<String>,
}

/// Deterministically synthesize the verifiable spec from a structured `Intent`.
///
/// Invariants (HFTASK-0003 acceptance #1):
/// - `path_scope` is **strictly narrower than the repo root** — never `["."]`. When the
///   caller supplies a `scope_override` (the `--scope` flag) it is used verbatim (after
///   dropping a bare `.`); otherwise a scoped default is derived from the intent and falls
///   back to the kernel's authorized `["spike/**", "handoff/**"]`.
/// - `test_commands` is **never empty** — keyed on `extracted_entities["language"]`, with a
///   Rust-kernel default when the language is unknown.
/// - each `acceptance_criteria` entry is phrased so a test maps to it (PRD §12.3 #7), plus a
///   provenance criterion ("drift audit passes / no out-of-scope write").
///
/// Pure: identical `(intent, role, scope_override)` → identical `SynthSpec`.
pub fn synthesize_spec(
    intent: &Intent,
    role: Option<&str>,
    scope_override: Option<&[String]>,
) -> SynthSpec {
    let path_scope = synth_path_scope(intent, scope_override);
    let test_commands = synth_test_commands(intent);
    let acceptance_criteria = synth_acceptance(intent, role, &test_commands);
    SynthSpec {
        path_scope,
        acceptance_criteria,
        test_commands,
    }
}

/// Scoped path_scope, never the repo root. Drops a bare `"."`/`"./"` and de-dupes.
fn synth_path_scope(intent: &Intent, scope_override: Option<&[String]>) -> Vec<String> {
    let mut scope: Vec<String> = match scope_override {
        Some(s) if !s.is_empty() => s
            .iter()
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty() && g != "." && g != "./" && g != "/**" && g != "**")
            .collect(),
        _ => Vec::new(),
    };
    if scope.is_empty() {
        // Derive a scoped default from the intent's domain; never the repo root.
        let derived = match intent.domain.as_str() {
            "testing" => vec!["tests/**", "spike/**", "handoff/**"],
            "documentation" => vec!["docs/**", "handoff/**"],
            "devops" => vec!["scripts/**", "handoff/**"],
            // The kernel front door's own authorized scope (card path_scope).
            _ => vec!["spike/**", "handoff/**"],
        };
        scope = derived.into_iter().map(String::from).collect();
    }
    // Final safety net: a non-empty, root-narrower scope is the invariant.
    scope.dedup();
    if scope.is_empty() {
        scope = vec!["spike/**".to_string(), "handoff/**".to_string()];
    }
    scope
}

/// Non-empty test_commands keyed on language (+ domain). Rust-kernel default when unknown.
fn synth_test_commands(intent: &Intent) -> Vec<String> {
    let mut cmds: Vec<String> = match intent.language() {
        Some("rust") => vec![
            "cargo test".to_string(),
            "cargo clippy --all-targets -- -D warnings".to_string(),
        ],
        Some("python") => vec!["pytest".to_string()],
        Some("go") => vec!["go test ./...".to_string()],
        Some("typescript") | Some("javascript") => vec!["npm test".to_string()],
        // Unknown language → the kernel's own toolchain default (this is a Rust workspace).
        _ => vec!["cargo test".to_string()],
    };
    // Testing-domain work additionally asserts the suite is exercised, not just present.
    if intent.domain == "testing" && !cmds.iter().any(|c| c.contains("test")) {
        cmds.push("cargo test".to_string());
    }
    cmds
}

/// Acceptance criteria templated on task_type, each phrased so a test maps to it, plus a
/// provenance/drift criterion. The first test command is referenced so the gate's "did
/// tests map to acceptance criteria?" check is satisfiable.
fn synth_acceptance(intent: &Intent, role: Option<&str>, test_commands: &[String]) -> Vec<String> {
    let primary = test_commands
        .first()
        .map(String::as_str)
        .unwrap_or("the test suite");
    let role_tag = role.map(|r| format!("[{r}] ")).unwrap_or_default();
    let behavior = match intent.task_type.as_str() {
        "fix" => format!(
            "{role_tag}A regression test reproduces the defect, then asserts the fixed behavior; `{primary}` is green."
        ),
        "improve" => format!(
            "{role_tag}The improvement is covered by a test that asserts the new behavior/metric; `{primary}` is green."
        ),
        "convert" => format!(
            "{role_tag}The converted code is covered by a test asserting equivalent behavior; `{primary}` is green."
        ),
        "explain" | "review" => format!(
            "{role_tag}The deliverable is captured as a reviewable artifact and the existing suite stays green under `{primary}`."
        ),
        "test" => format!(
            "{role_tag}New tests assert the target behavior and fail before / pass after the change; `{primary}` is green."
        ),
        "deploy" => format!(
            "{role_tag}The deploy path is validated by a test/smoke check; `{primary}` is green."
        ),
        // "create" and anything else
        _ => format!(
            "{role_tag}The feature compiles and is covered by a new test asserting the intended behavior; `{primary}` is green."
        ),
    };
    vec![
        behavior,
        "Drift audit passes: no out-of-scope write (edits stay within path_scope) and the intent_lock is unchanged."
            .to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_fix_intent() -> Intent {
        let mut e = BTreeMap::new();
        e.insert("language".to_string(), "rust".to_string());
        Intent {
            raw_text: "fix the panic in the ledger replay".to_string(),
            domain: "coding".to_string(),
            task_type: "fix".to_string(),
            extracted_entities: e,
        }
    }

    #[test]
    fn synth_never_emits_repo_root_scope() {
        let spec = synthesize_spec(&rust_fix_intent(), Some("coder"), None);
        assert!(!spec.path_scope.is_empty());
        assert!(
            !spec.path_scope.iter().any(|s| s == "." || s == "./"),
            "path_scope must be strictly narrower than repo root, got {:?}",
            spec.path_scope
        );
    }

    #[test]
    fn synth_never_emits_empty_test_commands() {
        let spec = synthesize_spec(&rust_fix_intent(), Some("coder"), None);
        assert!(
            !spec.test_commands.is_empty(),
            "test_commands must be non-empty"
        );
    }

    #[test]
    fn rust_language_maps_to_cargo_test() {
        let spec = synthesize_spec(&rust_fix_intent(), None, None);
        assert!(
            spec.test_commands.iter().any(|c| c == "cargo test"),
            "rust language must map to `cargo test`, got {:?}",
            spec.test_commands
        );
    }

    #[test]
    fn unknown_language_still_non_empty() {
        let intent = Intent {
            raw_text: "do a thing".to_string(),
            domain: "general".to_string(),
            task_type: "create".to_string(),
            extracted_entities: BTreeMap::new(),
        };
        let spec = synthesize_spec(&intent, None, None);
        assert!(!spec.test_commands.is_empty());
        assert!(spec.test_commands.iter().any(|c| c == "cargo test"));
    }

    #[test]
    fn language_specific_commands() {
        for (lang, expect) in [
            ("python", "pytest"),
            ("go", "go test ./..."),
            ("typescript", "npm test"),
            ("javascript", "npm test"),
        ] {
            let mut e = BTreeMap::new();
            e.insert("language".to_string(), lang.to_string());
            let intent = Intent {
                raw_text: "build it".to_string(),
                domain: "coding".to_string(),
                task_type: "create".to_string(),
                extracted_entities: e,
            };
            let spec = synthesize_spec(&intent, None, None);
            assert!(
                spec.test_commands.iter().any(|c| c == expect),
                "{lang} should map to {expect}, got {:?}",
                spec.test_commands
            );
        }
    }

    #[test]
    fn acceptance_is_test_mappable_and_has_provenance() {
        let spec = synthesize_spec(&rust_fix_intent(), Some("coder"), None);
        assert!(spec.acceptance_criteria.len() >= 2);
        // first criterion references the primary test command (the §12.3 #7 mapping)
        assert!(spec.acceptance_criteria[0].contains("cargo test"));
        // provenance/drift criterion is always present
        assert!(spec
            .acceptance_criteria
            .iter()
            .any(|c| c.contains("Drift audit") && c.contains("out-of-scope")));
    }

    #[test]
    fn scope_override_is_used_and_root_dropped() {
        let scope = vec![".".to_string(), "hf/**".to_string()];
        let spec = synthesize_spec(&rust_fix_intent(), None, Some(&scope));
        assert_eq!(spec.path_scope, vec!["hf/**".to_string()]);
    }

    #[test]
    fn scope_override_all_root_falls_back_to_default() {
        let scope = vec![".".to_string()];
        let spec = synthesize_spec(&rust_fix_intent(), None, Some(&scope));
        assert!(!spec.path_scope.is_empty());
        assert!(!spec.path_scope.iter().any(|s| s == "."));
    }

    #[test]
    fn synthesis_is_deterministic() {
        let a = synthesize_spec(&rust_fix_intent(), Some("coder"), None);
        let b = synthesize_spec(&rust_fix_intent(), Some("coder"), None);
        assert_eq!(a, b);
    }

    #[test]
    fn classify_is_deterministic_and_detects_rust_fix() {
        let i1 = Intent::classify("Fix the bug in the rust cargo build");
        let i2 = Intent::classify("Fix the bug in the rust cargo build");
        assert_eq!(i1, i2);
        assert_eq!(i1.task_type, "fix");
        assert_eq!(i1.language(), Some("rust"));
    }

    #[test]
    fn classify_detects_devops_deploy() {
        let i = Intent::classify("deploy the service via the docker pipeline");
        assert_eq!(i.domain, "devops");
        assert_eq!(i.task_type, "deploy");
    }
}
