//! `SpecDoc` -> Markdown (rebuild strategy, design §5 emit phase).
//!
//! Emit strategy: **rebuild from the model** (string assembly of the stored
//! CommonMark block spans), not arena splicing. The block spans were produced
//! by comrak's `format_commonmark` at parse time, so they are already normalized
//! CommonMark; re-emitting them makes `parse(emit(x))` stable.
//!
//! **Byte-exact parity with the oracle** (`@fission-ai/openspec@1.4.1`, design
//! §5 "Tune the formatter for byte-stability", §7/§8). The oracle tightens
//! blank lines on re-serialization; reproduced here exactly (verified against
//! `oracle-fixtures/03-archived-result.md` and `08-rename-modify-result.md`):
//!
//! - `# Title` followed by a blank line.
//! - The `## Purpose` section is fully **tight**: heading → body → the
//!   `## Requirements` heading all on consecutive lines (no blank lines).
//! - Each requirement: `### Requirement: <name>` is **tight** to its body; a
//!   blank line precedes each `#### Scenario:`; the scenario heading is tight to
//!   its steps; and a **blank line follows each requirement** (which produces
//!   the file's trailing blank line).
//! - Within a body or a scenario's steps, multiple blocks are separated by a
//!   blank line so the model round-trips (the fixtures have one block each).

use crate::model::Block;
use crate::model::SpecDoc;

/// Render a [`SpecDoc`] to Markdown, byte-matching the oracle's archive output.
pub fn emit_spec(doc: &SpecDoc) -> String {
    let mut out = String::new();

    // H1, then a single blank line.
    if let Some(title) = &doc.title {
        out.push_str("# ");
        out.push_str(title);
        out.push('\n');
    }
    out.push('\n');

    // Purpose section — fully tight (no trailing blank; it flows straight into
    // `## Requirements`).
    if let Some(purpose) = &doc.purpose {
        out.push_str("## Purpose\n");
        push_blocks(&mut out, purpose);
    }

    // Requirements.
    out.push_str("## Requirements\n");
    for req in &doc.requirements {
        out.push_str("### Requirement: ");
        out.push_str(&req.name);
        out.push('\n');
        push_blocks(&mut out, &req.body);
        for scen in &req.scenarios {
            out.push('\n'); // blank line before each scenario
            out.push_str("#### Scenario: ");
            out.push_str(&scen.name);
            out.push('\n');
            push_blocks(&mut out, &scen.steps);
        }
        out.push('\n'); // blank line after each requirement
    }

    out
}

/// Append a run of blocks, each ending in `\n`, separated from each other by a
/// blank line (so two paragraphs / lists re-parse as distinct blocks). A bullet
/// list is a single block whose `text` already holds all its lines.
fn push_blocks(out: &mut String, blocks: &[Block]) {
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push('\n'); // blank line between consecutive blocks
        }
        out.push_str(b.text.trim_end());
        out.push('\n');
    }
}
