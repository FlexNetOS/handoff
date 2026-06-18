//! The delta-merge algorithm (the crux). Pure: `SpecDoc` + `Delta` in, a new
//! `SpecDoc` (or a `MergeError`) out — no comrak, no I/O.
//!
//! Faithful to the `oracle-verified` archive semantics (contract §3, design §5,
//! §7):
//! - **ADDED**  — name must be absent; append to the end.
//! - **MODIFIED**— name must exist; replace the WHOLE requirement in place
//!   (keep index, discard old scenarios). NOT a scenario merge.
//! - **REMOVED** — name must exist; delete the block.
//! - **RENAMED** — `from` must exist, `to` must be absent; change the name in
//!   place (keep index, body, scenarios).
//!
//! **Op-evaluation order (RENAME-first).** When a delta renames `X→Y` and
//! modifies/removes the same requirement, the oracle requires the MODIFIED/
//! REMOVED block to reference the NEW name `Y`; referencing the old `X` aborts.
//! So preconditions are checked against the post-rename namespace and RENAMED
//! ops apply before ADDED/MODIFIED/REMOVED.
//!
//! **Transactional.** Every op's precondition is validated first; on any failure
//! nothing is mutated and the original `SpecDoc` is left untouched (matching the
//! CLI's `"Aborted. No files were changed."`).

use super::{normalize_name, DeltaOp, SpecDoc};

/// A precondition failure during merge. The messages mirror the Node CLI's
/// abort messages so the CLI slice can surface them faithfully.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    /// ADDED of a name that already exists.
    #[error("ADDED failed for header \"### Requirement: {name}\" - already exists")]
    AlreadyExists { name: String },
    /// MODIFIED / REMOVED of a name that is not present.
    #[error("{op} failed for header \"### Requirement: {name}\" - not found")]
    NotFound { op: &'static str, name: String },
    /// RENAMED whose `to` name already exists (would collide).
    #[error("RENAMED failed for header \"### Requirement: {name}\" - already exists")]
    RenameTargetExists { name: String },
    /// A MODIFIED/REMOVED that references the OLD name of a requirement renamed
    /// in the SAME delta. The oracle requires the NEW (post-rename) header
    /// (`oracle-verified`, U5 probe of `@fission-ai/openspec@1.4.1`: archiving
    /// such a delta aborts with "when a rename exists, MODIFIED must reference
    /// the NEW header ...").
    #[error(
        "{op} failed for header \"### Requirement: {old}\" - when a rename exists, {op} must reference the new header \"### Requirement: {new}\""
    )]
    RenamedOldNameReferenced {
        op: &'static str,
        old: String,
        new: String,
    },
}

/// Apply a delta to a base spec, transactionally. All ops are validated first;
/// on any error the function returns `Err` and `base` is observably unchanged
/// (it is taken by reference; only a working clone is mutated).
///
/// **Op-evaluation order (`oracle-verified`, design §7, U5 probe).** RENAMED ops
/// are applied **before** ADDED/MODIFIED/REMOVED, so a delta that renames `X→Y`
/// and modifies the same requirement must reference the NEW name `Y` in its
/// MODIFIED block (referencing the old `X` aborts). This matches
/// `@fission-ai/openspec@1.4.1`.
pub fn apply_delta(base: &SpecDoc, delta: &super::Delta) -> Result<SpecDoc, MergeError> {
    // 1. Validate EVERY precondition first (rename-aware). Transactional: a
    //    mid-merge failure mutates nothing.
    validate_preconditions(base, delta)?;

    // 2. Apply RENAMED first so the other ops resolve against the post-rename
    //    namespace (oracle semantics).
    let mut out = base.clone();
    for op in &delta.ops {
        if let DeltaOp::Renamed { from, to } = op {
            if let Some(i) = out.position_of(from) {
                out.requirements[i].name = to.clone();
            }
        }
    }

    // 3. Then ADDED/MODIFIED/REMOVED, in delta order. Preconditions held, so
    //    these lookups cannot legitimately miss.
    for op in &delta.ops {
        match op {
            DeltaOp::Renamed { .. } => {} // already applied above
            DeltaOp::Added(req) => out.requirements.push(req.clone()),
            DeltaOp::Modified(req) => {
                if let Some(i) = out.position_of(&req.name) {
                    out.requirements[i] = req.clone();
                }
            }
            DeltaOp::Removed { name, .. } => {
                if let Some(i) = out.position_of(name) {
                    out.requirements.remove(i);
                }
            }
        }
    }
    Ok(out)
}

/// Apply a delta to a base spec with "intelligent sync" semantics.
///
/// Unlike [`apply_delta`] (which is a programmatic whole-block replacement
/// faithful to the CLI `archive`), `sync_delta` implements the agent-driven
/// [`sync`] capability:
/// - **MODIFIED** requirements merge their scenarios: new scenarios in the
///   delta are appended to the base; if the delta requirement has a non-empty
///   body, it replaces the base body.
/// - **ADDED**, **REMOVED**, and **RENAMED** behave identically to
///   [`apply_delta`].
///
/// This allows an agent to add a single scenario to a requirement by providing
/// only that scenario in a MODIFIED block, without needing to know or copy
/// the existing scenarios.
pub fn sync_delta(base: &SpecDoc, delta: &super::Delta) -> Result<SpecDoc, MergeError> {
    validate_preconditions(base, delta)?;

    let mut out = base.clone();
    // 1. Apply RENAMED first (oracle namespace parity).
    for op in &delta.ops {
        if let DeltaOp::Renamed { from, to } = op {
            if let Some(i) = out.position_of(from) {
                out.requirements[i].name = to.clone();
            }
        }
    }

    // 2. Apply ADDED/MODIFIED/REMOVED.
    for op in &delta.ops {
        match op {
            DeltaOp::Renamed { .. } => {}
            DeltaOp::Added(req) => out.requirements.push(req.clone()),
            DeltaOp::Modified(delta_req) => {
                if let Some(i) = out.position_of(&delta_req.name) {
                    let base_req = &mut out.requirements[i];
                    // Sync body if delta has one.
                    if !delta_req.body.is_empty() {
                        base_req.body = delta_req.body.clone();
                    }
                    // Sync scenarios: append only NEW scenarios (by name).
                    for delta_sc in &delta_req.scenarios {
                        let exists = base_req.scenarios.iter().any(|s| {
                            super::normalize_name(&s.name) == super::normalize_name(&delta_sc.name)
                        });
                        if !exists {
                            base_req.scenarios.push(delta_sc.clone());
                        } else {
                            // If it exists, replace it (intelligent update).
                            if let Some(sc_idx) = base_req.scenarios.iter().position(|s| {
                                super::normalize_name(&s.name)
                                    == super::normalize_name(&delta_sc.name)
                            }) {
                                base_req.scenarios[sc_idx] = delta_sc.clone();
                            }
                        }
                    }
                }
            }
            DeltaOp::Removed { name, .. } => {
                if let Some(i) = out.position_of(name) {
                    out.requirements.remove(i);
                }
            }
        }
    }
    Ok(out)
}

/// Validate all op preconditions, rename-aware: MODIFIED/REMOVED/ADDED are
/// checked against the **post-rename** namespace (the oracle applies renames
/// first). Referencing the old name of a same-delta rename is a distinct error.
fn validate_preconditions(base: &SpecDoc, delta: &super::Delta) -> Result<(), MergeError> {
    // Index this delta's renames (old -> new).
    let renames: Vec<(&str, &str)> = delta
        .ops
        .iter()
        .filter_map(|op| match op {
            DeltaOp::Renamed { from, to } => Some((from.as_str(), to.as_str())),
            _ => None,
        })
        .collect();

    // If `name` is the OLD side of a rename in this delta, return its NEW name.
    let renamed_from = |name: &str| -> Option<&str> {
        let n = normalize_name(name);
        renames
            .iter()
            .find(|(f, _)| normalize_name(f) == n)
            .map(|(_, t)| *t)
    };
    // Is `name` a rename TARGET (new name) created by this delta?
    let is_rename_target = |name: &str| -> bool {
        let n = normalize_name(name);
        renames.iter().any(|(_, t)| normalize_name(t) == n)
    };
    // Does a requirement with `name` exist once this delta's renames are applied?
    let exists_after_rename = |name: &str| -> bool {
        (base.contains(name) && renamed_from(name).is_none()) || is_rename_target(name)
    };

    for op in &delta.ops {
        match op {
            DeltaOp::Added(req) => {
                if exists_after_rename(&req.name) {
                    return Err(MergeError::AlreadyExists {
                        name: req.name.clone(),
                    });
                }
            }
            DeltaOp::Modified(req) => {
                if let Some(new) = renamed_from(&req.name) {
                    return Err(MergeError::RenamedOldNameReferenced {
                        op: "MODIFIED",
                        old: req.name.clone(),
                        new: new.to_string(),
                    });
                }
                if !exists_after_rename(&req.name) {
                    return Err(MergeError::NotFound {
                        op: "MODIFIED",
                        name: req.name.clone(),
                    });
                }
            }
            DeltaOp::Removed { name, .. } => {
                if let Some(new) = renamed_from(name) {
                    return Err(MergeError::RenamedOldNameReferenced {
                        op: "REMOVED",
                        old: name.clone(),
                        new: new.to_string(),
                    });
                }
                if !exists_after_rename(name) {
                    return Err(MergeError::NotFound {
                        op: "REMOVED",
                        name: name.clone(),
                    });
                }
            }
            DeltaOp::Renamed { from, to } => {
                if !base.contains(from) {
                    return Err(MergeError::NotFound {
                        op: "RENAMED",
                        name: from.clone(),
                    });
                }
                if base.contains(to) {
                    return Err(MergeError::RenameTargetExists { name: to.clone() });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, Delta, Requirement, Scenario};

    fn req(name: &str, body: &str, scenarios: &[&str]) -> Requirement {
        let mut r = Requirement::new(name);
        r.body.push(Block::new(body));
        for s in scenarios {
            r.scenarios.push(Scenario::new(*s));
        }
        r
    }

    fn base() -> SpecDoc {
        SpecDoc {
            title: Some("widget-export Specification".into()),
            purpose: None,
            requirements: vec![
                req("CSV export", "The system SHALL export CSV.", &["ok"]),
                req(
                    "Export rate limit",
                    "The system SHALL limit to 10.",
                    &["within", "over"],
                ),
                req(
                    "Legacy XML export",
                    "The system SHALL export XML.",
                    &["xml"],
                ),
                req(
                    "Export filename",
                    "The system SHALL name files.",
                    &["fname"],
                ),
            ],
        }
    }

    // ---- happy paths ----

    #[test]
    fn added_appends_to_end() {
        let d = Delta::new(vec![DeltaOp::Added(req(
            "JSON export",
            "The system SHALL export JSON.",
            &["json"],
        ))]);
        let out = apply_delta(&base(), &d).unwrap();
        assert_eq!(out.requirements.len(), 5);
        assert_eq!(out.requirements[4].name, "JSON export");
    }

    #[test]
    fn modified_replaces_whole_block_in_place() {
        let newblock = req(
            "Export rate limit",
            "The system SHALL limit to 20.",
            &["within20"], // fewer scenarios than base (2) — proves whole replace, not merge
        );
        let d = Delta::new(vec![DeltaOp::Modified(newblock.clone())]);
        let out = apply_delta(&base(), &d).unwrap();
        // same position
        assert_eq!(out.requirements[1].name, "Export rate limit");
        // whole block replaced: body + scenarios are the delta's, old discarded
        assert_eq!(out.requirements[1], newblock);
        assert_eq!(out.requirements[1].scenarios.len(), 1);
    }

    #[test]
    fn removed_deletes_block() {
        let d = Delta::new(vec![DeltaOp::Removed {
            name: "Legacy XML export".into(),
            reason: Some("deprecated".into()),
            migration: Some("use JSON".into()),
        }]);
        let out = apply_delta(&base(), &d).unwrap();
        assert_eq!(out.requirements.len(), 3);
        assert!(!out.contains("Legacy XML export"));
    }

    #[test]
    fn renamed_changes_name_in_place_keeps_body() {
        let d = Delta::new(vec![DeltaOp::Renamed {
            from: "Export filename".into(),
            to: "Exported file naming".into(),
        }]);
        let out = apply_delta(&base(), &d).unwrap();
        assert_eq!(out.requirements[3].name, "Exported file naming");
        // body + scenarios preserved
        assert_eq!(out.requirements[3].scenarios.len(), 1);
        assert_eq!(
            out.requirements[3].body[0].text,
            "The system SHALL name files."
        );
    }

    #[test]
    fn whitespace_insensitive_match() {
        let d = Delta::new(vec![DeltaOp::Modified(req(
            "Export   rate  limit", // extra internal whitespace
            "The system SHALL limit to 20.",
            &["x"],
        ))]);
        let out = apply_delta(&base(), &d).unwrap();
        assert_eq!(out.requirements[1].name, "Export   rate  limit");
    }

    // ---- the four error modes + transactionality ----

    #[test]
    fn added_exists_errors_and_does_not_mutate() {
        let b = base();
        let d = Delta::new(vec![DeltaOp::Added(req(
            "CSV export",
            "The system SHALL export CSV again.",
            &["dup"],
        ))]);
        let err = apply_delta(&b, &d).unwrap_err();
        assert_eq!(
            err,
            MergeError::AlreadyExists {
                name: "CSV export".into()
            }
        );
        // transactionality: base is unchanged
        assert_eq!(b, base());
    }

    #[test]
    fn modified_not_found_errors_and_does_not_mutate() {
        let b = base();
        let d = Delta::new(vec![DeltaOp::Modified(req(
            "Nonexistent",
            "The system MUST do x.",
            &["s"],
        ))]);
        let err = apply_delta(&b, &d).unwrap_err();
        assert_eq!(
            err,
            MergeError::NotFound {
                op: "MODIFIED",
                name: "Nonexistent".into()
            }
        );
        assert_eq!(b, base());
    }

    #[test]
    fn removed_not_found_errors_and_does_not_mutate() {
        let b = base();
        let d = Delta::new(vec![DeltaOp::Removed {
            name: "Nonexistent".into(),
            reason: None,
            migration: None,
        }]);
        let err = apply_delta(&b, &d).unwrap_err();
        assert_eq!(
            err,
            MergeError::NotFound {
                op: "REMOVED",
                name: "Nonexistent".into()
            }
        );
        assert_eq!(b, base());
    }

    #[test]
    fn renamed_not_found_errors_and_does_not_mutate() {
        let b = base();
        let d = Delta::new(vec![DeltaOp::Renamed {
            from: "Nonexistent".into(),
            to: "Whatever".into(),
        }]);
        let err = apply_delta(&b, &d).unwrap_err();
        assert_eq!(
            err,
            MergeError::NotFound {
                op: "RENAMED",
                name: "Nonexistent".into()
            }
        );
        assert_eq!(b, base());
    }

    #[test]
    fn partial_failure_in_multi_op_delta_aborts_all() {
        // A valid ADDED followed by an invalid MODIFIED: nothing should apply.
        let b = base();
        let d = Delta::new(vec![
            DeltaOp::Added(req("JSON export", "SHALL export JSON.", &["j"])),
            DeltaOp::Modified(req("Nonexistent", "MUST x.", &["s"])),
        ]);
        assert!(apply_delta(&b, &d).is_err());
        assert_eq!(b, base()); // the ADDED did not leak through
    }

    // ---- RENAME + MODIFY of the SAME requirement in one delta ----
    // Pinned against `@fission-ai/openspec@1.4.1` (U5 oracle probe, design §7):
    // RENAMED applies first, so MODIFIED must reference the NEW (post-rename)
    // header; referencing the OLD name aborts.

    #[test]
    fn rename_then_modify_new_name_succeeds_in_place() {
        // Delta order is MODIFIED-before-RENAMED (matching the delta section
        // order ## MODIFIED ... ## RENAMED), yet rename is applied first.
        let d = Delta::new(vec![
            DeltaOp::Modified(req(
                "Exported file naming", // the NEW name
                "The system SHALL name files using the set name AND timestamp.",
                &["fname2"],
            )),
            DeltaOp::Renamed {
                from: "Export filename".into(),
                to: "Exported file naming".into(),
            },
        ]);
        let out = apply_delta(&base(), &d).unwrap();
        // position 3 keeps its slot; name is the rename target; body is modified.
        assert_eq!(out.requirements.len(), 4);
        assert_eq!(out.requirements[3].name, "Exported file naming");
        assert!(out.requirements[3].body[0].text.contains("AND timestamp"));
        assert_eq!(out.requirements[3].scenarios.len(), 1);
        assert_eq!(out.requirements[3].scenarios[0].name, "fname2");
    }

    #[test]
    fn rename_with_modify_referencing_old_name_aborts() {
        // MODIFIED references the OLD name while a rename of it exists: abort,
        // no mutation, with the rename-reference error (matches the oracle).
        let b = base();
        let d = Delta::new(vec![
            DeltaOp::Modified(req(
                "Export filename", // the OLD name — illegal when a rename exists
                "The system SHALL name files differently.",
                &["x"],
            )),
            DeltaOp::Renamed {
                from: "Export filename".into(),
                to: "Exported file naming".into(),
            },
        ]);
        let err = apply_delta(&b, &d).unwrap_err();
        assert_eq!(
            err,
            MergeError::RenamedOldNameReferenced {
                op: "MODIFIED",
                old: "Export filename".into(),
                new: "Exported file naming".into(),
            }
        );
        assert_eq!(b, base()); // transactional: nothing changed
    }

    #[test]
    fn rename_with_remove_referencing_old_name_aborts() {
        // The same rule applies to REMOVED referencing a renamed-away name.
        let b = base();
        let d = Delta::new(vec![
            DeltaOp::Removed {
                name: "Export filename".into(),
                reason: None,
                migration: None,
            },
            DeltaOp::Renamed {
                from: "Export filename".into(),
                to: "Exported file naming".into(),
            },
        ]);
        let err = apply_delta(&b, &d).unwrap_err();
        assert_eq!(
            err,
            MergeError::RenamedOldNameReferenced {
                op: "REMOVED",
                old: "Export filename".into(),
                new: "Exported file naming".into(),
            }
        );
        assert_eq!(b, base());
    }

    #[test]
    fn test_sync_delta_intelligent_merge() {
        use super::super::{Block, Requirement, Scenario};

        let mut b = SpecDoc::default();
        let mut r1 = Requirement::new("Req 1");
        r1.body.push(Block::new("Base body"));
        r1.scenarios.push(Scenario::new("Base Scen"));
        b.requirements.push(r1);

        // Sync a delta that adds a scenario and changes the body
        let mut d_req = Requirement::new("Req 1");
        d_req.body.push(Block::new("New body"));
        d_req.scenarios.push(Scenario::new("New Scen"));
        let d = super::super::Delta::new(vec![DeltaOp::Modified(d_req)]);

        let merged = sync_delta(&b, &d).unwrap();
        assert_eq!(merged.requirements.len(), 1);
        let m_req = &merged.requirements[0];
        assert_eq!(m_req.body[0].text, "New body");
        assert_eq!(m_req.scenarios.len(), 2);
        assert_eq!(m_req.scenarios[0].name, "Base Scen");
        assert_eq!(m_req.scenarios[1].name, "New Scen");

        // Sync another delta that updates an existing scenario
        let mut d_req2 = Requirement::new("Req 1");
        let mut sc_update = Scenario::new("Base Scen");
        sc_update.steps.push(Block::new("Updated step"));
        d_req2.scenarios.push(sc_update);
        let d2 = super::super::Delta::new(vec![DeltaOp::Modified(d_req2)]);

        let merged2 = sync_delta(&merged, &d2).unwrap();
        assert_eq!(merged2.requirements[0].scenarios.len(), 2);
        assert_eq!(merged2.requirements[0].scenarios[0].name, "Base Scen");
        assert_eq!(
            merged2.requirements[0].scenarios[0].steps[0].text,
            "Updated step"
        );
    }
}
