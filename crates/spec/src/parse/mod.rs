//! EDGE: comrak AST <-> model. `comrak` is confined to this module (and never
//! reaches `model/`).

mod common;
mod delta_parser;
mod emit;
mod spec_parser;

pub use delta_parser::parse_delta;
pub use emit::emit_spec;
pub use spec_parser::parse_spec;
