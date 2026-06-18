//! EDGE: load the intent-driven `schema.yaml` (via `serde_norway`) into an
//! artifact graph, and answer lifecycle-state questions over it (design §2/§4).
//!
//! The artifact DAG is: `proposal → {specs, design}`, `design → adr`,
//! `{specs, adr} → tasks`, then the `apply` phase is gated on `tasks`. An
//! artifact is **ready** when every artifact in its `requires` set is **done**;
//! the change is **archivable** when all artifacts are done.
//!
//! This module is the serde edge: the deserialized structs live here. Graph
//! algorithms are in [`graph`]; filesystem "is this artifact produced?" lives in
//! the CLI (the FS edge), which feeds a `done` set into these queries.

mod graph;

use serde::Deserialize;

/// A parsed workflow schema (`schema.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Schema {
    pub name: String,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub description: String,
    /// The ordered artifacts (proposal/specs/design/adr/tasks).
    pub artifacts: Vec<Artifact>,
    /// The terminal `apply` phase (gated on `tasks`, tracks `tasks.md`).
    pub apply: ApplyPhase,
}

/// One artifact node in the DAG.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub id: String,
    /// Glob of files this artifact produces (relative to the change dir; `adr`
    /// uses `../../../adr/*.md` to reach the repo-level `adr/`).
    pub generates: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub template: String,
    #[serde(default)]
    pub instruction: String,
    /// Ids of artifacts that must be `done` before this one is ready.
    #[serde(default)]
    pub requires: Vec<String>,
}

/// The `apply` phase: not a generated artifact, gated on `tasks`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ApplyPhase {
    #[serde(default)]
    pub requires: Vec<String>,
    /// The file whose checkboxes drive apply progress (`tasks.md`).
    pub tracks: String,
    #[serde(default)]
    pub instruction: String,
}

/// Errors loading or walking a schema.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// `schema.yaml` failed to deserialize.
    #[error("failed to parse schema: {0}")]
    Parse(String),
    /// The artifact graph has a cycle (no topological order).
    #[error("schema artifact graph has a cycle (unresolved: {unresolved})")]
    Cycle { unresolved: String },
    /// An artifact's `requires` names an unknown artifact id.
    #[error("artifact {artifact:?} requires unknown artifact {missing:?}")]
    UnknownRequire { artifact: String, missing: String },
}

/// Parse `schema.yaml` source into a [`Schema`].
pub fn load_schema(yaml: &str) -> Result<Schema, SchemaError> {
    serde_norway::from_str(yaml).map_err(|e| SchemaError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical intent-driven schema, embedded for tests.
    const SCHEMA_YAML: &str = include_str!(
        "../../../../intent-driven-template/openspec/schemas/intent-driven/schema.yaml"
    );

    fn schema() -> Schema {
        load_schema(SCHEMA_YAML).expect("intent-driven schema.yaml must parse")
    }

    #[test]
    fn parses_intent_driven_schema() {
        let s = schema();
        assert_eq!(s.name, "intent-driven");
        assert_eq!(s.version, 1);
        let ids: Vec<&str> = s.artifacts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["proposal", "specs", "design", "adr", "tasks"]);
        assert_eq!(s.apply.tracks, "tasks.md");
    }

    #[test]
    fn load_schema_fails_on_corrupt_yaml() {
        let yaml = "name: corrupt\nartifacts:\n  - id: foo\n    generates: missing_colon";
        let result = load_schema(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        eprintln!("Actual error: {}", err);
        match err {
            SchemaError::Parse(_) => (),
            _ => panic!("Expected Parse error"),
        }
    }

    #[test]
    fn load_schema_fails_on_malformed_yaml() {
        let yaml = "name: malformed\n  - !invalid_syntax";
        let result = load_schema(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err();
        eprintln!("Actual malformed error: {}", err);
        match err {
            SchemaError::Parse(_) => (),
            _ => panic!("Expected Parse error"),
        }
    }

    #[test]
    fn artifact_requires_edges() {
        let s = schema();
        assert_eq!(
            s.artifact("proposal").unwrap().requires,
            Vec::<String>::new()
        );
        assert_eq!(s.artifact("specs").unwrap().requires, vec!["proposal"]);
        assert_eq!(s.artifact("design").unwrap().requires, vec!["proposal"]);
        assert_eq!(s.artifact("adr").unwrap().requires, vec!["design"]);
        let mut tasks_req = s.artifact("tasks").unwrap().requires.clone();
        tasks_req.sort();
        assert_eq!(tasks_req, vec!["adr", "specs"]);
    }
}
