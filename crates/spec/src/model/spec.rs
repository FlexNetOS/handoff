//! SpecDoc — a parsed base spec: title, optional Purpose, ordered requirements.

use super::{normalize_name, Block, Requirement};

/// A whole base spec file.
///
/// The requirement order is **load-bearing**: RENAMED/MODIFIED keep position,
/// ADDED appends, REMOVED deletes (design §5). Comparing two `SpecDoc`s for
/// equality therefore proves merge semantics independent of whitespace.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SpecDoc {
    /// `# <name> Specification` heading text (the whole H1 line content).
    pub title: Option<String>,
    /// Prose blocks under `## Purpose` (if present).
    pub purpose: Option<Vec<Block>>,
    /// Ordered `### Requirement:` blocks.
    pub requirements: Vec<Requirement>,
}

impl SpecDoc {
    /// Index of the requirement whose normalized name matches `name`, if any.
    pub fn position_of(&self, name: &str) -> Option<usize> {
        let target = normalize_name(name);
        self.requirements
            .iter()
            .position(|r| normalize_name(&r.name) == target)
    }

    /// Does a requirement with this (normalized) name exist?
    pub fn contains(&self, name: &str) -> bool {
        self.position_of(name).is_some()
    }
}
