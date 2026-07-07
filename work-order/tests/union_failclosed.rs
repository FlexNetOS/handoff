// HFTASK-0080 (ADR-0019 D5 #3): this whole crate is a test; unwrap/expect are idiomatic here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Additive RED suite — plan/handoff-union-cycle2 (test-coverage dimension, union seam).
//!
//! Union acceptance criterion: the intent plane (rusty-idd) loads `handoff.task.v1` cards via a
//! **mirrored copy** of this crate (`rusty-idd/crates/work-order/src/lib.rs:54` — "mirrors
//! `…/handoff/schemas/task.schema.json`") and TODAY parses them with a bare
//! `serde_json::from_str::<WorkOrder>` — the FAIL-OPEN card-load found in cycle 1.
//!
//! `WorkOrder` derives `Deserialize`, but the `#[schemars(regex(...))]` constraints on `schema`
//! and `id` (`work-order/src/lib.rs:60,66`) are SCHEMA-only — serde does NOT enforce them at
//! deserialize time. And nothing on the load path re-checks the `intent_lock` against the card's
//! content. So the only load surface this crate exposes silently accepts a foreign-schema card,
//! a malformed-id card, and a card whose intent_lock is a lie.
//!
//! These tests encode the fail-closed LOADER contract the union needs work-order to provide
//! (e.g. `WorkOrder::from_card_json(&str) -> Result<WorkOrder, LoadError>` that binds
//! deserialize + `handoff_schema::validate_card` + `intent_unchanged`). They COMPILE and RUN
//! against the EXISTING public surface and FAIL on assertion because that fail-closed loader is
//! unbuilt — RED for the right reason (capability missing), not a compile error.

use serde_json::{Value, json};
use work_order::{SwarmBundle, WorkOrder, work_orders_from_bundle};

/// A genuinely-valid handoff.task.v1 card, as a JSON `Value`, built from the crate's own
/// deterministic intake seam so every required field + a correct intent_lock are present.
fn valid_card_value() -> Value {
    let bundle = SwarmBundle {
        workflow_id: "wf-union-0001".to_string(),
        role_prompts: vec![(
            "coder".to_string(),
            "Implement the checkout flow in rust".to_string(),
        )],
        handoff_template: String::new(),
        consistency_report: vec![],
        evolution_suggestions: vec![],
    };
    let order = work_orders_from_bundle(&bundle).remove(0);
    serde_json::from_str(&order.to_json()).expect("a valid WorkOrder serializes to a JSON object")
}

/// Sanity: the fixture itself is a real, load-clean card (so the RED below is about the loader,
/// not a malformed fixture). This test is expected to PASS.
#[test]
fn fixture_is_a_clean_valid_card() {
    let card = valid_card_value();
    let loaded: WorkOrder =
        serde_json::from_value(card).expect("the fixture is a structurally valid card");
    assert_eq!(loaded.schema, "handoff.task.v1");
    assert!(
        loaded.intent_unchanged(),
        "fixture's intent_lock must match its content"
    );
}

/// RED — foreign-schema card. A card whose `schema` is not `handoff.task.v1` is not a
/// handoff.task.v1 envelope and MUST be rejected at the load boundary. Today serde accepts it
/// (the `#[schemars(regex)]` is schema-only), so `is_err()` is false → assertion fails.
#[test]
fn workorder_load_rejects_foreign_schema_card() {
    let mut card = valid_card_value();
    card["schema"] = json!("foreign.schema.v9");
    let loaded: Result<WorkOrder, _> = serde_json::from_value(card);
    assert!(
        loaded.is_err(),
        "FAIL-OPEN: the only work-order load path (serde) accepted a foreign-schema card. \
         A fail-closed loader (e.g. WorkOrder::from_card_json) must reject schema != handoff.task.v1."
    );
}

/// RED — malformed-id card. An id that violates `^[A-Z]*TASK-[A-Z0-9][A-Z0-9-]*$` MUST be
/// rejected at load. serde fills the `String` field regardless → `is_err()` is false.
#[test]
fn workorder_load_rejects_malformed_id_card() {
    let mut card = valid_card_value();
    card["id"] = json!("not-a-task-id");
    let loaded: Result<WorkOrder, _> = serde_json::from_value(card);
    assert!(
        loaded.is_err(),
        "FAIL-OPEN: serde accepted a card whose id violates the schema id pattern. \
         A fail-closed loader must reject a malformed id at the load boundary."
    );
}

/// RED — drifted/tampered intent_lock. A structurally-valid card whose stored `intent_lock` no
/// longer matches its content (objective hand-edited, lock left stale) MUST be rejected (or
/// re-verified) at load. The only load path accepts it AND runs no integrity check, so a
/// downstream consumer trusts a lock that lies — `intent_unchanged()` on the loaded card is
/// false → assertion fails.
#[test]
fn workorder_load_rejects_card_with_drifted_intent_lock() {
    let mut card = valid_card_value();
    card["objective"] = json!("Redesign the entire architecture from scratch (drifted intent)");
    let loaded: WorkOrder =
        serde_json::from_value(card).expect("a structurally-valid card still deserializes");
    assert!(
        loaded.intent_unchanged(),
        "FAIL-OPEN: loaded a card whose intent_lock does not match its content. \
         A fail-closed loader must reject (or re-verify) a card with a drifted intent_lock."
    );
}
