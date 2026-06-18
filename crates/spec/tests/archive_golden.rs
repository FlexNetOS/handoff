//! Archive golden tests.
//!
//! Two gates over `(01-base + 02-delta) → 03-archived` (and the U5 rename+modify
//! fixtures `06/07/08`):
//! 1. **Semantic** (primary): parse both the emitted output and the oracle file
//!    into `SpecDoc` and assert the MODELS are equal — proves merge semantics
//!    independent of whitespace.
//! 2. **Byte-exact** (U6): `emit_spec` of the merged model equals the oracle
//!    file byte-for-byte (the oracle's blank-line tightening + trailing blank
//!    line, reproduced in `parse/emit.rs`).

use rusty_idd_spec::{apply_delta, emit_spec, parse_delta, parse_spec};

const BASE: &str = include_str!("../../../docs/rusty-idd/oracle-fixtures/01-base-spec.md");
const DELTA: &str = include_str!("../../../docs/rusty-idd/oracle-fixtures/02-delta-spec.md");
const EXPECTED: &str =
    include_str!("../../../docs/rusty-idd/oracle-fixtures/03-archived-result.md");

#[test]
fn archive_produces_semantically_equal_model() {
    let base = parse_spec(BASE);
    let delta = parse_delta(DELTA);
    let merged = apply_delta(&base, &delta).expect("merge must succeed");

    let emitted = emit_spec(&merged);
    let from_emitted = parse_spec(&emitted);
    let from_oracle = parse_spec(EXPECTED);

    assert_eq!(
        from_emitted, from_oracle,
        "merged model must equal the oracle's archived model"
    );
}

/// U6: byte-exact parity. `emit_spec` of the merged model must equal the oracle's
/// archived file **byte-for-byte** (blank-line tightening + trailing newline).
/// The semantic test above remains the primary gate; this locks the formatter.
#[test]
fn archive_emit_is_byte_identical_to_oracle() {
    let base = parse_spec(BASE);
    let delta = parse_delta(DELTA);
    let merged = apply_delta(&base, &delta).expect("merge must succeed");

    let emitted = emit_spec(&merged);
    assert_eq!(
        emitted, EXPECTED,
        "emit must byte-match oracle 03-archived-result.md"
    );
}

/// U6: the same byte-exact gate for the RENAME+MODIFY fixture (08).
#[test]
fn archive_emit_rename_modify_is_byte_identical_to_oracle() {
    let base = parse_spec(RM_BASE);
    let delta = parse_delta(RM_DELTA);
    let merged = apply_delta(&base, &delta).expect("merge must succeed");

    assert_eq!(
        emit_spec(&merged),
        RM_RESULT,
        "emit must byte-match oracle 08-rename-modify-result.md"
    );
}

#[test]
fn archive_requirement_order_and_ops() {
    let base = parse_spec(BASE);
    let delta = parse_delta(DELTA);
    let merged = apply_delta(&base, &delta).unwrap();

    let names: Vec<&str> = merged
        .requirements
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "CSV export",           // untouched, position 0
            "Export rate limit",    // MODIFIED in place
            "Exported file naming", // RENAMED in place (was "Export filename")
            "JSON export",          // ADDED last
        ],
        "Legacy XML export REMOVED; RENAMED in place; ADDED appended last"
    );

    // MODIFIED = whole-block replace: rate limit body now says 20, scenarios replaced.
    let rate = &merged.requirements[1];
    assert!(rate.body[0].text.contains("20 per hour"));
    assert_eq!(rate.scenarios.len(), 2);
    assert!(rate.scenarios[0].steps[0].text.contains("19 times"));
}

#[test]
fn emit_is_well_formed_and_idempotent() {
    let base = parse_spec(BASE);
    let delta = parse_delta(DELTA);
    let merged = apply_delta(&base, &delta).unwrap();

    let once = emit_spec(&merged);
    // parse(emit(x)) == x  (model round-trips through emit)
    assert_eq!(parse_spec(&once), merged, "parse(emit(x)) must equal x");

    // emit(parse(emit(x))) == emit(parse(emit(x)))  -> idempotent
    let twice = emit_spec(&parse_spec(&once));
    assert_eq!(once, twice, "emit must be idempotent");

    // Well-formed: ends in the oracle's trailing blank line (`\n\n`), and never
    // more than that.
    assert!(
        once.ends_with("\n\n"),
        "oracle ends each file with a blank line"
    );
    assert!(
        !once.ends_with("\n\n\n"),
        "no more than one trailing blank line"
    );
}

#[test]
fn round_trip_base_spec_stability() {
    // parse(emit(parse(x))) == parse(x) for the base spec.
    let doc = parse_spec(BASE);
    let re = parse_spec(&emit_spec(&doc));
    assert_eq!(doc, re);
}

// ---- RENAME + MODIFY of the same requirement (U5, oracle-verified) ----

const RM_BASE: &str =
    include_str!("../../../docs/rusty-idd/oracle-fixtures/06-rename-modify-base.md");
const RM_DELTA: &str =
    include_str!("../../../docs/rusty-idd/oracle-fixtures/07-rename-modify-delta.md");
const RM_RESULT: &str =
    include_str!("../../../docs/rusty-idd/oracle-fixtures/08-rename-modify-result.md");

/// A delta that RENAMES `Export filename` → `Exported file naming` and MODIFIES
/// the SAME requirement (referencing the NEW name) must merge to the oracle's
/// archived result: rename applied in place, body+scenario replaced, position
/// kept. Captured from `@fission-ai/openspec@1.4.1` (design §7, now verified).
#[test]
fn rename_and_modify_same_requirement_matches_oracle() {
    let base = parse_spec(RM_BASE);
    let delta = parse_delta(RM_DELTA);
    let merged = apply_delta(&base, &delta).expect("rename+modify must merge");

    let from_emitted = parse_spec(&emit_spec(&merged));
    let from_oracle = parse_spec(RM_RESULT);
    assert_eq!(
        from_emitted, from_oracle,
        "rename+modify-same-requirement model must equal the oracle's result"
    );

    // Spell out the load-bearing facts: position kept, renamed, body modified.
    let names: Vec<&str> = merged
        .requirements
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, vec!["CSV export", "Exported file naming"]);
    assert!(merged.requirements[1].body[0]
        .text
        .contains("timestamp suffix"));
}
