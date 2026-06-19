//! The PURE domain model.
//!
//! This submodule is intentionally dependency-light: no `comrak`, no `serde`,
//! no I/O. It is the hexagonal core (see `spec-engine-design.md` §1, §3) so the
//! merge logic in [`merge`] is unit-testable without a Markdown parser in the
//! loop. Parsing (comrak), serialization (serde), and filesystem access all
//! live at the crate edges (`parse`, `validate`, `archive`).

mod block;
mod delta;
mod merge;
mod requirement;
mod spec;

pub use block::Block;
pub use delta::{Delta, DeltaOp};
pub use merge::{apply_delta, sync_delta, MergeError};
pub use requirement::{Requirement, Scenario};
pub use spec::SpecDoc;

/// Normalize a requirement/heading name for op matching.
///
/// Matching is whitespace-insensitive on the heading text (`oracle-verified`,
/// per the contract): trim the ends and collapse any internal whitespace runs
/// to a single space. Used by every delta op lookup so the same logic is shared.
pub fn normalize_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::normalize_name;

    #[test]
    fn normalize_collapses_and_trims() {
        assert_eq!(
            normalize_name("  Export   rate  limit "),
            "Export rate limit"
        );
        assert_eq!(normalize_name("CSV export"), "CSV export");
        assert_eq!(normalize_name("a\tb\nc"), "a b c");
    }
}
