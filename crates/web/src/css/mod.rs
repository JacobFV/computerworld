//! CSS syntax: tokenizer, parser, selectors, stylesheet model and selector matching.
//! `token.rs` is the shared contract between `css` (which produces component values)
//! and `style` (which parses property values from them); see DESIGN.md.

pub mod token;
pub use token::*;
