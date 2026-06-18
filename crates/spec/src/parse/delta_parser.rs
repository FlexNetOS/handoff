//! comrak AST -> `Delta`. Scans `## ADDED|MODIFIED|REMOVED|RENAMED
//! Requirements` sections and lowers each into [`DeltaOp`]s (design §5).

use comrak::nodes::AstNode;
use comrak::Arena;

use super::common;
use crate::model::{Delta, DeltaOp, Requirement, Scenario};

const REQ_PREFIX: &str = "Requirement:";
const SCEN_PREFIX: &str = "Scenario:";

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Added,
    Modified,
    Removed,
    Renamed,
    Other,
}

/// Parse a change's delta Markdown into a [`Delta`].
pub fn parse_delta(src: &str) -> Delta {
    let arena = Arena::new();
    let root = common::parse(&arena, src);

    let mut ops: Vec<DeltaOp> = Vec::new();
    let mut section = Op::Other;

    // For ADDED/MODIFIED we accumulate the requirement currently being built.
    let mut current_req: Option<Requirement> = None;
    // For REMOVED we accumulate name + reason/migration.
    let mut removed: Option<RemovedAccum> = None;

    macro_rules! flush_req {
        () => {
            if let Some(req) = current_req.take() {
                match section {
                    Op::Added => ops.push(DeltaOp::Added(req)),
                    Op::Modified => ops.push(DeltaOp::Modified(req)),
                    _ => {}
                }
            }
        };
    }
    macro_rules! flush_removed {
        () => {
            if let Some(r) = removed.take() {
                ops.push(DeltaOp::Removed {
                    name: r.name,
                    reason: r.reason,
                    migration: r.migration,
                });
            }
        };
    }

    for node in root.children() {
        // H2 op-section switch — flush any in-progress accumulators first.
        if let Some(h2) = common::heading_text(node, 2) {
            flush_req!();
            flush_removed!();
            section = classify_op(&h2);
            continue;
        }

        match section {
            Op::Added | Op::Modified => {
                if let Some(h3) = common::heading_text(node, 3) {
                    if let Some(name) = strip_prefix(&h3, REQ_PREFIX) {
                        flush_req!();
                        current_req = Some(Requirement::new(name));
                        continue;
                    }
                }
                if let Some(h4) = common::heading_text(node, 4) {
                    if let Some(name) = strip_prefix(&h4, SCEN_PREFIX) {
                        if let Some(req) = current_req.as_mut() {
                            req.scenarios.push(Scenario::new(name));
                        }
                        continue;
                    }
                }
                if common::heading_level(node).is_some() {
                    continue;
                }
                if let Some(req) = current_req.as_mut() {
                    let block = common::block_of(node);
                    if let Some(scen) = req.scenarios.last_mut() {
                        scen.steps.push(block);
                    } else {
                        req.body.push(block);
                    }
                }
            }
            Op::Removed => {
                if let Some(h3) = common::heading_text(node, 3) {
                    if let Some(name) = strip_prefix(&h3, REQ_PREFIX) {
                        flush_removed!();
                        removed = Some(RemovedAccum::new(name));
                        continue;
                    }
                }
                if common::heading_level(node).is_some() {
                    continue;
                }
                if let Some(acc) = removed.as_mut() {
                    acc.absorb(node);
                }
            }
            Op::Renamed => {
                // FROM/TO live in a bullet list of `- FROM: `...`` items.
                collect_renames(node, &mut ops);
            }
            Op::Other => {}
        }
    }

    flush_req!();
    flush_removed!();

    Delta::new(ops)
}

struct RemovedAccum {
    name: String,
    reason: Option<String>,
    migration: Option<String>,
}

impl RemovedAccum {
    fn new(name: &str) -> Self {
        RemovedAccum {
            name: name.to_string(),
            reason: None,
            migration: None,
        }
    }

    /// Absorb a `**Reason**: ...` / `**Migration**: ...` paragraph.
    fn absorb<'a>(&mut self, node: &'a AstNode<'a>) {
        let text = common::inline_text(node);
        if let Some(rest) = field_value(&text, "Reason") {
            self.reason = Some(rest);
        } else if let Some(rest) = field_value(&text, "Migration") {
            self.migration = Some(rest);
        }
    }
}

/// Parse a `- FROM: \`### Requirement: X\`` / `- TO: \`### Requirement: Y\``
/// bullet list into `Renamed` ops. Pairs FROM with the next TO.
fn collect_renames<'a>(list_node: &'a AstNode<'a>, ops: &mut Vec<DeltaOp>) {
    use comrak::nodes::NodeValue;
    if !matches!(list_node.data.borrow().value, NodeValue::List(_)) {
        return;
    }
    let mut pending_from: Option<String> = None;
    for item in list_node.children() {
        let text = common::inline_text(item);
        if let Some(v) = field_value(&text, "FROM") {
            pending_from = Some(strip_req_heading(&v));
        } else if let Some(v) = field_value(&text, "TO") {
            if let Some(from) = pending_from.take() {
                ops.push(DeltaOp::Renamed {
                    from,
                    to: strip_req_heading(&v),
                });
            }
        }
    }
}

fn classify_op(h2: &str) -> Op {
    match h2.split_whitespace().next() {
        Some("ADDED") => Op::Added,
        Some("MODIFIED") => Op::Modified,
        Some("REMOVED") => Op::Removed,
        Some("RENAMED") => Op::Renamed,
        _ => Op::Other,
    }
}

/// Extract the value after `<Field>:` (with optional `**` bold markers already
/// stripped by `inline_text`), trimmed.
fn field_value(text: &str, field: &str) -> Option<String> {
    let t = text.trim_start();
    let after = t.strip_prefix(field)?;
    let after = after.trim_start();
    let after = after.strip_prefix(':')?;
    Some(after.trim().to_string())
}

/// Strip a leading `### Requirement: ` (and surrounding backticks) from a
/// FROM/TO value, leaving just the requirement name.
fn strip_req_heading(value: &str) -> String {
    let v = value.trim().trim_matches('`').trim();
    v.trim_start()
        .strip_prefix("###")
        .map(|s| s.trim_start())
        .and_then(|s| s.strip_prefix(REQ_PREFIX))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| v.to_string())
}

fn strip_prefix<'a>(heading: &'a str, prefix: &str) -> Option<&'a str> {
    heading
        .trim_start()
        .strip_prefix(prefix)
        .map(|rest| rest.trim())
}
