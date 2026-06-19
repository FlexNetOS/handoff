//! comrak AST -> `SpecDoc`. Scans H1 title, `## Purpose`, and the ordered
//! `### Requirement:` / `#### Scenario:` blocks (design §5 parse phase).

use comrak::Arena;

use super::common;
use crate::model::{Requirement, Scenario, SpecDoc};

const REQ_PREFIX: &str = "Requirement:";
const SCEN_PREFIX: &str = "Scenario:";

/// Which `## ` section the walk cursor is currently in.
#[derive(Clone, Copy, PartialEq)]
enum Section {
    None,
    Purpose,
    Requirements,
}

/// Parse a base spec's Markdown into a [`SpecDoc`].
pub fn parse_spec(src: &str) -> SpecDoc {
    let arena = Arena::new();
    let root = common::parse(&arena, src);

    let mut doc = SpecDoc::default();
    let mut section = Section::None;

    for node in root.children() {
        // H1 title.
        if let Some(t) = common::heading_text(node, 1) {
            doc.title = Some(t);
            continue;
        }
        // H2 section switch.
        if let Some(h2) = common::heading_text(node, 2) {
            section = match h2.trim() {
                "Purpose" => Section::Purpose,
                "Requirements" => Section::Requirements,
                _ => Section::None,
            };
            continue;
        }
        // H3 requirement.
        if let Some(h3) = common::heading_text(node, 3) {
            if let Some(name) = strip_prefix(&h3, REQ_PREFIX) {
                doc.requirements.push(Requirement::new(name));
                continue;
            }
        }
        // H4 scenario (attaches to the current requirement).
        if let Some(h4) = common::heading_text(node, 4) {
            if let Some(name) = strip_prefix(&h4, SCEN_PREFIX) {
                if let Some(req) = doc.requirements.last_mut() {
                    req.scenarios.push(Scenario::new(name));
                }
                continue;
            }
        }
        // Any other heading: ignore for structure.
        if common::heading_level(node).is_some() {
            continue;
        }
        // Content block: route to Purpose, the current scenario, or the current
        // requirement body.
        let block = common::block_of(node);
        match section {
            Section::Purpose => doc.purpose.get_or_insert_with(Vec::new).push(block),
            _ => {
                if let Some(req) = doc.requirements.last_mut() {
                    if let Some(scen) = req.scenarios.last_mut() {
                        scen.steps.push(block);
                    } else {
                        req.body.push(block);
                    }
                }
                // Content before any requirement and outside Purpose has no
                // structural home; the fixtures never exercise this.
            }
        }
    }

    doc
}

/// Strip `Requirement:` / `Scenario:` prefix from a heading's inline text,
/// returning the trimmed remainder.
fn strip_prefix<'a>(heading: &'a str, prefix: &str) -> Option<&'a str> {
    heading
        .trim_start()
        .strip_prefix(prefix)
        .map(|rest| rest.trim())
}
