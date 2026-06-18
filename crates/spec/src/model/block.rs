//! A model-owned representation of a CommonMark block.
//!
//! The model never depends on `comrak::nodes`; instead the parser lowers each
//! top-level block (paragraph, list, ...) into a [`Block`] carrying its raw
//! CommonMark text span. The emitter renders the span back verbatim. This keeps
//! `merge.rs` parser-free (design §3).

/// An opaque block of CommonMark prose belonging to a requirement body or a
/// scenario's steps. The `text` is the rendered CommonMark for exactly this
/// block (already trimmed of a trailing newline); equality is value equality on
/// that text, which is what the semantic golden test compares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub text: String,
}

impl Block {
    pub fn new(text: impl Into<String>) -> Self {
        Block { text: text.into() }
    }
}
