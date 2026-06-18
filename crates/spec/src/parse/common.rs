//! Shared comrak helpers for the parsers. `comrak` types do not escape this
//! module's siblings' public surfaces.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_commonmark, Arena, Options};

use crate::model::Block;

/// The comrak options used for both parsing and emit. `width = 0` disables line
/// wrapping; the default bullet (`-`) and emphasis match the fixtures.
pub fn options() -> Options<'static> {
    let mut o = Options::default();
    o.render.width = 0;
    o
}

/// Parse a document into an arena and return the root.
pub fn parse<'a>(arena: &'a Arena<'a>, src: &str) -> &'a AstNode<'a> {
    comrak::parse_document(arena, src, &options())
}

/// Concatenated inline text of a node (used for heading text).
pub fn inline_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut s = String::new();
    for c in node.descendants() {
        match &c.data.borrow().value {
            NodeValue::Text(t) => s.push_str(t),
            NodeValue::Code(code) => s.push_str(&code.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => s.push(' '),
            _ => {}
        }
    }
    s
}

/// If `node` is a heading of `level`, return its inline text.
pub fn heading_text<'a>(node: &'a AstNode<'a>, level: u8) -> Option<String> {
    match &node.data.borrow().value {
        NodeValue::Heading(h) if h.level == level => Some(inline_text(node)),
        _ => None,
    }
}

/// Heading level of `node`, if it is a heading.
pub fn heading_level(node: &AstNode<'_>) -> Option<u8> {
    match &node.data.borrow().value {
        NodeValue::Heading(h) => Some(h.level),
        _ => None,
    }
}

/// Render a single block node to a trimmed CommonMark [`Block`].
pub fn block_of<'a>(node: &'a AstNode<'a>) -> Block {
    let mut out = String::new();
    // format_commonmark is infallible for a String sink in practice; if it
    // ever errored we fall back to empty rather than panicking the parser.
    let _ = format_commonmark(node, &options(), &mut out);
    Block::new(out.trim_end().to_string())
}
