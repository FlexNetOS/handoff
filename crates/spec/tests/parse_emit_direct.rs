use rusty_idd_spec::model::{Block, Requirement, Scenario};
use rusty_idd_spec::{emit_spec, parse_delta, parse_spec, SpecDoc};

#[test]
fn test_parse_spec_empty_string() {
    let doc = parse_spec("");
    assert_eq!(doc.title, None);
    assert!(doc.requirements.is_empty());
}

#[test]
fn test_parse_spec_unicode() {
    let src = "# 🦀 Rust-native\n\n## Purpose\nBâtir un système robuste. 🚀\n\n## Requirements\n### Requirement: Sécurité\n#### Scenario: Validation UTF-8\nDonnées valides.\n";
    let doc = parse_spec(src);
    assert_eq!(doc.title.unwrap(), "🦀 Rust-native");
    assert_eq!(doc.purpose.unwrap()[0].text, "Bâtir un système robuste. 🚀");
    assert_eq!(doc.requirements[0].name, "Sécurité");
    assert_eq!(doc.requirements[0].scenarios[0].name, "Validation UTF-8");
}

#[test]
fn test_parse_spec_malformed_prefixes() {
    // Missing space after colon is currently handled by strip_prefix which calls trim() on rest.
    // Wait, strip_prefix(heading, "Requirement:") -> if heading is "Requirement:X", rest is "X".
    let src =
        "# Title\n\n## Requirements\n### Requirement:NoSpace\n#### Scenario:NoSpaceScen\nBody\n";
    let doc = parse_spec(src);
    assert_eq!(doc.requirements[0].name, "NoSpace");
    assert_eq!(doc.requirements[0].scenarios[0].name, "NoSpaceScen");
}

#[test]
fn test_parse_delta_empty() {
    let delta = parse_delta("");
    assert!(delta.ops.is_empty());
}

#[test]
fn test_parse_delta_malformed_sections() {
    let src = "## UNKNOWN SECTION\n### Requirement: Foo\n";
    let delta = parse_delta(src);
    assert!(delta.ops.is_empty());
}

#[test]
fn test_emit_spec_minimal() {
    let doc = SpecDoc {
        title: Some("Minimal".to_string()),
        ..Default::default()
    };
    let emitted = emit_spec(&doc);
    // Should have H1, blank line, and ## Requirements header
    assert!(emitted.contains("# Minimal\n\n## Requirements\n"));
}

#[test]
fn test_emit_spec_unicode_roundtrip() {
    let mut doc = SpecDoc {
        title: Some("🦀".to_string()),
        ..Default::default()
    };
    let mut req = Requirement::new("Élan");
    req.scenarios.push(Scenario::new("Déjà vu"));
    doc.requirements.push(req);

    let emitted = emit_spec(&doc);
    let parsed = parse_spec(&emitted);
    assert_eq!(parsed.title.unwrap(), "🦀");
    assert_eq!(parsed.requirements[0].name, "Élan");
    assert_eq!(parsed.requirements[0].scenarios[0].name, "Déjà vu");
}

#[test]
fn test_emit_spec_blank_line_edges() {
    let mut doc = SpecDoc {
        title: Some("Title".to_string()),
        purpose: Some(vec![Block::new("Line 1\n\nLine 2")]),
        ..Default::default()
    };
    doc.requirements.push(Requirement::new("Req"));

    let emitted = emit_spec(&doc);
    // Line 1 and Line 2 should be separated by a blank line within the block
    assert!(emitted.contains("Line 1\n\nLine 2\n"));
    // Oracle format: tight Purpose -> Requirements
    assert!(emitted.contains("Line 2\n## Requirements\n"));
}

#[test]
fn test_emit_spec_multiple_requirements_spacing() {
    let mut doc = SpecDoc {
        title: Some("Title".to_string()),
        ..Default::default()
    };
    doc.requirements.push(Requirement::new("Req 1"));
    doc.requirements.push(Requirement::new("Req 2"));

    let emitted = emit_spec(&doc);
    // There should be a blank line after Req 1 and before Req 2 starts.
    // Spec says: "### Requirement: Req 1\n\n### Requirement: Req 2\n\n"
    // Wait, let's check emit_spec logic:
    // for req in &doc.requirements {
    //     out.push_str("### Requirement: "); ... out.push('\n');
    //     push_blocks(&mut out, &req.body);
    //     ...
    //     out.push('\n'); // blank line after each requirement
    // }
    assert!(emitted.contains("### Requirement: Req 1\n\n### Requirement: Req 2\n"));
}
