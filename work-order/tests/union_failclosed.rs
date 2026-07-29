// HFTASK-0080 (ADR-0019 D5 #3): this whole crate is a test; unwrap/expect are idiomatic here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Union acceptance suite — plan/handoff-union-cycle2 (test-coverage dimension, union seam).
//!
//! Union acceptance criterion: the intent plane (rusty-idd) loads `handoff.task.v1` cards via a
//! **mirrored copy** of this crate (`rusty-idd/crates/work-order/src/lib.rs:54` — "mirrors
//! `…/handoff/schemas/task.schema.json`") and parsed them with a bare
//! `serde_json::from_str::<WorkOrder>` — the FAIL-OPEN card-load found in cycle 1.
//!
//! `WorkOrder` derives `Deserialize`, but the `#[schemars(regex(...))]` constraints on `schema`
//! and `id` are SCHEMA-only — serde does NOT enforce them at deserialize time. And nothing on
//! the bare load path re-checks the `intent_lock` against the card's content. So that surface
//! silently accepts a foreign-schema card, a malformed-id card, and a card whose intent_lock is
//! a lie.
//!
//! These tests pin the fail-closed LOADER contract the union needs work-order to provide, which
//! the RED suite named directly: `WorkOrder::from_card_json(&str) -> Result<WorkOrder,
//! LoadError>`. They drive that loader rather than the bare serde path.
//!
//! Why they drive the loader instead of asserting on bare `serde_json::from_value`, as the RED
//! placeholders did: the third criterion below is the reason. As authored it required the card
//! to deserialize AND `intent_unchanged()` to then report true after the objective had been
//! hand-edited — i.e. it required the load path to *recompute* the tampered lock. Silently
//! re-deriving a lock so it always matches is the opposite of fail-closed; it turns the drift
//! sentinel into a rubber stamp. The loader rejects instead, and the test asserts the rejection.

use serde_json::{Value, json};
use work_order::{LoadError, SwarmBundle, WorkOrder, work_orders_from_bundle};

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

/// Sanity: the fixture itself is a real, load-clean card (so the rejections below are about the
/// loader, not a malformed fixture).
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

/// The fail-closed loader accepts a genuinely valid card — the gate refuses bad cards without
/// refusing good ones.
#[test]
fn workorder_load_accepts_a_valid_card() {
    let card = valid_card_value();
    let loaded = WorkOrder::from_card_value(card).expect("a valid card must load");
    assert_eq!(loaded.schema, "handoff.task.v1");
    assert!(loaded.intent_unchanged());
}

/// Foreign-schema card. A card whose `schema` is not `handoff.task.v1` is not a
/// handoff.task.v1 envelope and MUST be rejected at the load boundary.
#[test]
fn workorder_load_rejects_foreign_schema_card() {
    let mut card = valid_card_value();
    card["schema"] = json!("foreign.schema.v9");

    // The bare serde path is fail-open — this is the defect the loader exists to close.
    assert!(
        serde_json::from_value::<WorkOrder>(card.clone()).is_ok(),
        "precondition: bare serde is fail-open on a foreign schema"
    );

    let loaded = WorkOrder::from_card_value(card);
    assert!(
        matches!(loaded, Err(LoadError::ForeignSchema(ref s)) if s == "foreign.schema.v9"),
        "the fail-closed loader must reject schema != handoff.task.v1; got {loaded:?}"
    );
}

/// Malformed-id card. An id that violates `^[A-Z]*TASK-[A-Z0-9][A-Z0-9-]*$` MUST be rejected at
/// load.
#[test]
fn workorder_load_rejects_malformed_id_card() {
    let mut card = valid_card_value();
    card["id"] = json!("not-a-task-id");

    assert!(
        serde_json::from_value::<WorkOrder>(card.clone()).is_ok(),
        "precondition: bare serde is fail-open on a malformed id"
    );

    let loaded = WorkOrder::from_card_value(card);
    assert!(
        matches!(loaded, Err(LoadError::MalformedId(ref s)) if s == "not-a-task-id"),
        "the fail-closed loader must reject a malformed id; got {loaded:?}"
    );
}

/// Drifted/tampered intent_lock. A structurally-valid card whose stored `intent_lock` no longer
/// matches its content (objective hand-edited, lock left stale) MUST be rejected — not silently
/// re-locked.
#[test]
fn workorder_load_rejects_card_with_drifted_intent_lock() {
    let mut card = valid_card_value();
    card["objective"] = json!("Redesign the entire architecture from scratch (drifted intent)");

    let bare: WorkOrder = serde_json::from_value(card.clone())
        .expect("precondition: a structurally-valid card still deserializes");
    assert!(
        !bare.intent_unchanged(),
        "precondition: bare serde loads a card whose lock is a lie"
    );

    let loaded = WorkOrder::from_card_value(card);
    assert!(
        matches!(loaded, Err(LoadError::DriftedIntentLock(_))),
        "the fail-closed loader must reject a drifted intent_lock; got {loaded:?}"
    );
}

/// The canonical id form accepts every shape on disk today (numeric kernel/intake ids and
/// slug-style kb-minted ids) and refuses free-form or empty slugs.
#[test]
fn canonical_task_ids_are_accepted_and_free_form_ids_are_not() {
    let accept = [
        "HFTASK-0058",
        "PHTASK-0025",
        "TASK-0001",
        "KBTASK-FLEET-HANDOFF-ROLLOUT",
        "KBTASK-HFTASK-0058",
    ];
    let reject = [
        "not-a-task-id",
        "TASK-",
        "TASK--X",
        "hftask-0058",
        "MyTASK-0001",
        "",
    ];
    for id in accept {
        let mut card = valid_card_value();
        card["id"] = json!(id);
        // Re-lock is unnecessary: id is not part of the intent lock surface.
        assert!(
            !matches!(
                WorkOrder::from_card_value(card),
                Err(LoadError::MalformedId(_))
            ),
            "{id} must be accepted as a canonical task id"
        );
    }
    for id in reject {
        let mut card = valid_card_value();
        card["id"] = json!(id);
        assert!(
            matches!(
                WorkOrder::from_card_value(card),
                Err(LoadError::MalformedId(_))
            ),
            "{id:?} must be refused as a malformed task id"
        );
    }
}

/// `from_card_json` is the string-input twin of `from_card_value` and refuses the same cards.
#[test]
fn from_card_json_matches_from_card_value() {
    let card = valid_card_value();
    let text = serde_json::to_string(&card).unwrap();
    assert!(WorkOrder::from_card_json(&text).is_ok());

    let mut foreign = valid_card_value();
    foreign["schema"] = json!("foreign.schema.v9");
    let text = serde_json::to_string(&foreign).unwrap();
    assert!(matches!(
        WorkOrder::from_card_json(&text),
        Err(LoadError::ForeignSchema(_))
    ));

    assert!(matches!(
        WorkOrder::from_card_json("{ not json"),
        Err(LoadError::Malformed(_))
    ));
}
