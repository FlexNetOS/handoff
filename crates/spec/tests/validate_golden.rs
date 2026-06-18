//! Validate golden tests (the GATE item 3).
//!
//! Match the JSON SHAPE of the oracle fixtures `04`/`05` (level/message/valid/
//! summary). `durationMs` is nulled out for comparison (the oracle's timing is
//! not reproducible).
//!
//! Note: fixture `05` carries a duplicate WARNING item the Node oracle emitted
//! in addition to the ERROR. The contract treats GIVEN/WHEN/THEN as content-lint
//! and the gate is "match the level/message intent" — we assert the structural
//! ERROR (valid:false, path, message) and the summary shape, which is the
//! load-bearing part.

use rusty_idd_spec::validate::{validate_spec, IssueLevel};

const NO_SCENARIO_SPEC: &str = "\
# no-scenario Specification

## Purpose
This spec exists to exercise the no-scenario error path in the validator here.

## Requirements

### Requirement: Lonely requirement
The system SHALL do something but has no scenario at all.
";

const BRIEF_PURPOSE_SPEC: &str = "\
# widget-export Specification

## Purpose
Defines how users export widget data.

## Requirements

### Requirement: CSV export
The system SHALL allow users to export widget data as CSV.

#### Scenario: Successful CSV export
- **GIVEN** a user has saved widgets
- **WHEN** the user exports their widgets as CSV
- **THEN** the system provides a CSV file containing the widgets
";

#[test]
fn no_scenario_is_invalid_with_error() {
    let report = validate_spec("no-scenario", NO_SCENARIO_SPEC);
    let item = &report.items[0];

    assert!(!item.valid, "no-scenario spec must be invalid");
    let err = item
        .issues
        .iter()
        .find(|i| i.level == IssueLevel::Error)
        .expect("must have an ERROR");
    assert_eq!(err.path, "requirements.0.scenarios");
    assert_eq!(err.message, "Requirement must have at least one scenario");

    // Summary shape matches fixture 05.
    assert_eq!(report.summary.totals.items, 1);
    assert_eq!(report.summary.totals.passed, 0);
    assert_eq!(report.summary.totals.failed, 1);
    assert_eq!(report.version, "1.0");

    // JSON shape sanity: keys present, durationMs serializes as null.
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["items"][0]["valid"], serde_json::json!(false));
    assert_eq!(json["items"][0]["type"], serde_json::json!("spec"));
    assert!(json["items"][0]["durationMs"].is_null());
    assert_eq!(
        json["summary"]["byType"]["spec"]["failed"],
        serde_json::json!(1)
    );
}

#[test]
fn brief_purpose_is_valid_with_warning() {
    let report = validate_spec("widget-export", BRIEF_PURPOSE_SPEC);
    let item = &report.items[0];

    assert!(
        item.valid,
        "brief-purpose spec must still be valid (non-strict)"
    );
    let warn = item
        .issues
        .iter()
        .find(|i| i.level == IssueLevel::Warning)
        .expect("must have a WARNING");
    assert_eq!(warn.path, "overview");
    assert_eq!(
        warn.message,
        "Purpose section is too brief (less than 50 characters)"
    );
    assert!(
        !item.issues.iter().any(|i| i.level == IssueLevel::Error),
        "no ERRORs expected"
    );

    // Summary shape matches fixture 04.
    assert_eq!(report.summary.totals.passed, 1);
    assert_eq!(report.summary.totals.failed, 0);
}
