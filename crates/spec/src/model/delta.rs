//! Delta and DeltaOp — the parsed `## <OP> Requirements` sections of a change.

use super::Requirement;

/// One delta operation. ADDED/MODIFIED carry the WHOLE replacement requirement
/// block; REMOVED carries the name (+ optional Reason/Migration, content-lint);
/// RENAMED carries the from/to names. (Design §3, contract §3.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeltaOp {
    /// `## ADDED Requirements` — append a new requirement (must not exist).
    Added(Requirement),
    /// `## MODIFIED Requirements` — whole-block replace in place (must exist).
    Modified(Requirement),
    /// `## REMOVED Requirements` — delete the block (must exist).
    Removed {
        name: String,
        reason: Option<String>,
        migration: Option<String>,
    },
    /// `## RENAMED Requirements` — change heading text only, keep position.
    Renamed { from: String, to: String },
}

/// An ordered list of delta ops grouped from the delta spec's sections.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Delta {
    pub ops: Vec<DeltaOp>,
}

impl Delta {
    pub fn new(ops: Vec<DeltaOp>) -> Self {
        Delta { ops }
    }
}
