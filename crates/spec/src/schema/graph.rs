//! DAG algorithms over a [`Schema`]'s artifacts (design §4): topological order,
//! `next_ready`, and `is_archivable`. Pure — they operate on a caller-supplied
//! `done` set (the FS-detected "this artifact's files exist" state).

use std::collections::BTreeSet;

use super::{Artifact, Schema, SchemaError};

impl Schema {
    /// Look up an artifact by id.
    pub fn artifact(&self, id: &str) -> Option<&Artifact> {
        self.artifacts.iter().find(|a| a.id == id)
    }

    /// Artifacts in a topological order (every artifact's `requires` precede it).
    /// Errors on an unknown `requires` id or a cycle.
    pub fn topo_order(&self) -> Result<Vec<&Artifact>, SchemaError> {
        // Validate requires reference known ids first.
        for a in &self.artifacts {
            for r in &a.requires {
                if self.artifact(r).is_none() {
                    return Err(SchemaError::UnknownRequire {
                        artifact: a.id.clone(),
                        missing: r.clone(),
                    });
                }
            }
        }
        let mut ordered: Vec<&Artifact> = Vec::with_capacity(self.artifacts.len());
        let mut placed: BTreeSet<String> = BTreeSet::new();
        // Repeatedly place any artifact whose requires are all already placed.
        // O(n^2) but n is tiny (5).
        while ordered.len() < self.artifacts.len() {
            let before = ordered.len();
            for a in &self.artifacts {
                if placed.contains(&a.id) {
                    continue;
                }
                if a.requires.iter().all(|r| placed.contains(r)) {
                    ordered.push(a);
                    placed.insert(a.id.clone());
                }
            }
            if ordered.len() == before {
                // No progress → a cycle among the remaining artifacts.
                let unresolved: Vec<&str> = self
                    .artifacts
                    .iter()
                    .filter(|a| !placed.contains(&a.id))
                    .map(|a| a.id.as_str())
                    .collect();
                return Err(SchemaError::Cycle {
                    unresolved: unresolved.join(", "),
                });
            }
        }
        Ok(ordered)
    }

    /// The next artifact to produce: the first (in topological order) that is
    /// **not** yet `done` but whose every `requires` **is** `done`. `None` when
    /// all artifacts are done (or the next is blocked by an undone prerequisite).
    pub fn next_ready(&self, done: &BTreeSet<String>) -> Option<&Artifact> {
        let order = self.topo_order().ok()?;
        order
            .into_iter()
            .find(|a| !done.contains(&a.id) && a.requires.iter().all(|r| done.contains(r)))
    }

    /// Is the change archivable — i.e. every artifact done? (The `apply` phase is
    /// separate; archive readiness is over the artifacts, per design §4.)
    pub fn is_archivable(&self, done: &BTreeSet<String>) -> bool {
        self.artifacts.iter().all(|a| done.contains(&a.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::load_schema;

    const SCHEMA_YAML: &str = include_str!(
        "../../../../intent-driven-template/openspec/schemas/intent-driven/schema.yaml"
    );

    fn schema() -> Schema {
        load_schema(SCHEMA_YAML).unwrap()
    }

    fn done(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn topo_order_respects_requires() {
        let s = schema();
        let order: Vec<&str> = s
            .topo_order()
            .unwrap()
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        // proposal first; tasks last; each requirement precedes its dependent.
        let pos = |id: &str| order.iter().position(|x| *x == id).unwrap();
        assert!(pos("proposal") < pos("specs"));
        assert!(pos("proposal") < pos("design"));
        assert!(pos("design") < pos("adr"));
        assert!(pos("specs") < pos("tasks"));
        assert!(pos("adr") < pos("tasks"));
    }

    #[test]
    fn next_ready_walks_the_dag() {
        let s = schema();
        // Nothing done → proposal is next.
        assert_eq!(s.next_ready(&done(&[])).unwrap().id, "proposal");
        // proposal done → specs or design ready; topo order yields specs first.
        assert_eq!(s.next_ready(&done(&["proposal"])).unwrap().id, "specs");
        // proposal+specs done → design next (its only require, proposal, is done).
        assert_eq!(
            s.next_ready(&done(&["proposal", "specs"])).unwrap().id,
            "design"
        );
        // proposal+specs+design done → adr next.
        assert_eq!(
            s.next_ready(&done(&["proposal", "specs", "design"]))
                .unwrap()
                .id,
            "adr"
        );
        // everything but tasks → tasks next.
        assert_eq!(
            s.next_ready(&done(&["proposal", "specs", "design", "adr"]))
                .unwrap()
                .id,
            "tasks"
        );
        // all done → none.
        assert!(s
            .next_ready(&done(&["proposal", "specs", "design", "adr", "tasks"]))
            .is_none());
    }

    #[test]
    fn is_archivable_requires_all_artifacts() {
        let s = schema();
        assert!(!s.is_archivable(&done(&["proposal", "specs", "design", "adr"])));
        assert!(s.is_archivable(&done(&["proposal", "specs", "design", "adr", "tasks"])));
    }
}
