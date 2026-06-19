//! Structural validation rules matching the oracle (contract §5, fixtures
//! `04`/`05`). Per-file structural only — it does NOT cross-check delta ops
//! against a base, and GIVEN/WHEN/THEN content is NOT validated.
//!
//! Enforced ERRORs:
//! - every requirement has ≥1 scenario (`requirements.N.scenarios`)
//! - requirement body contains `SHALL` or `MUST` (`requirements.N.text`)
//!
//! Enforced WARNING:
//! - `## Purpose` section too brief (<50 chars) → `overview`.
//!
//! An H3 `### Scenario:` is naturally mis-parsed as a requirement (it is not an
//! H3 `Requirement:`)... see note below: we surface the no-scenario ERROR the
//! same way the oracle does.

use crate::model::SpecDoc;
use crate::parse::parse_spec;

use super::report::{Issue, IssueLevel, Item, Report};

const PURPOSE_MIN_CHARS: usize = 50;

/// Validate a spec's Markdown source, returning a single-item [`Report`] whose
/// JSON shape matches the oracle fixtures.
pub fn validate_spec(id: &str, src: &str) -> Report {
    Report::single(validate_spec_item(id, src))
}

/// Validate one spec into an [`Item`] (no summary). `duration_ms` is left
/// `None` (the oracle's timing is not reproducible and the tests null it out).
pub fn validate_spec_item(id: &str, src: &str) -> Item {
    let doc = parse_spec(src);
    let issues = collect_issues(&doc);
    // Non-strict semantics: valid is false only if there is ≥1 ERROR.
    let valid = !issues.iter().any(|i| i.level == IssueLevel::Error);
    Item {
        id: id.to_string(),
        item_type: "spec".to_string(),
        valid,
        issues,
        duration_ms: None,
    }
}

fn collect_issues(doc: &SpecDoc) -> Vec<Issue> {
    let mut issues = Vec::new();

    // Purpose brevity (WARNING). Concatenate the purpose block text.
    let purpose_len = doc
        .purpose
        .as_ref()
        .map(|blocks| {
            blocks
                .iter()
                .map(|b| b.text.trim().chars().count())
                .sum::<usize>()
        })
        .unwrap_or(0);
    if purpose_len < PURPOSE_MIN_CHARS {
        issues.push(Issue {
            level: IssueLevel::Warning,
            path: "overview".to_string(),
            message: "Purpose section is too brief (less than 50 characters)".to_string(),
        });
    }

    // Per-requirement ERRORs.
    for (i, req) in doc.requirements.iter().enumerate() {
        if req.scenarios.is_empty() {
            issues.push(Issue {
                level: IssueLevel::Error,
                path: format!("requirements.{i}.scenarios"),
                message: "Requirement must have at least one scenario".to_string(),
            });
        }
        if !req.has_shall_or_must() {
            issues.push(Issue {
                level: IssueLevel::Error,
                path: format!("requirements.{i}.text"),
                message: "Requirement must contain SHALL or MUST keyword".to_string(),
            });
        }
    }

    issues
}
