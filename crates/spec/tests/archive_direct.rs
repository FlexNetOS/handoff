use rusty_idd_spec::archive::{archive_specs, OpCounts, SpecMerge};
use rusty_idd_spec::model::MergeError;

#[test]
fn test_archive_specs_success() {
    let spec1 = SpecMerge {
        capability: "cap1",
        base_src: "# Cap 1\n\n## Requirements\n",
        delta_src: "## ADDED Requirements\n### Requirement: Req A\n",
    };
    let spec2 = SpecMerge {
        capability: "cap2",
        base_src: "# Cap 2\n\n## Requirements\n",
        delta_src: "## ADDED Requirements\n### Requirement: Req B\n",
    };

    let results = archive_specs(&[spec1, spec2]).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].capability, "cap1");
    assert_eq!(
        results[0].counts,
        OpCounts {
            added: 1,
            ..Default::default()
        }
    );
    assert!(results[0].markdown.contains("### Requirement: Req A"));

    assert_eq!(results[1].capability, "cap2");
    assert_eq!(
        results[1].counts,
        OpCounts {
            added: 1,
            ..Default::default()
        }
    );
    assert!(results[1].markdown.contains("### Requirement: Req B"));
}

#[test]
fn test_archive_specs_abort_on_failure() {
    let spec_ok = SpecMerge {
        capability: "ok",
        base_src: "# OK\n\n## Requirements\n",
        delta_src: "## ADDED Requirements\n### Requirement: Valid\n",
    };
    let spec_fail = SpecMerge {
        capability: "fail",
        base_src: "# Fail\n\n## Requirements\n### Requirement: Existing\n",
        delta_src: "## ADDED Requirements\n### Requirement: Existing\n", // Conflict: ADDED existing
    };

    let result = archive_specs(&[spec_ok, spec_fail]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.capability, "fail");
    // MergeError::Conflict is expected because we added a requirement that already exists.
    assert!(matches!(err.source, MergeError::AlreadyExists { .. }));
}

#[test]
fn test_archive_specs_empty() {
    let result = archive_specs(&[]).unwrap();
    assert!(result.is_empty());
}
