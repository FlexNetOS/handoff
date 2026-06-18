//! EDGE: structural validation of a parsed spec -> a [`ValidationReport`] that
//! serializes to match the SHAPE of the oracle fixtures (`04`/`05`).

mod report;
mod rules;

pub use report::{Counts, Issue, IssueLevel, Item, Report, Summary};
pub use rules::{validate_spec, validate_spec_item};
