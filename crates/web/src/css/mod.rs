//! CSS syntax: tokenizer, parser, selectors, stylesheet model and selector matching.
//! `token.rs` is the shared contract between `css` (which produces component values)
//! and `style` (which parses property values from them); see DESIGN.md.
//!
//! Entry points for the style track:
//! - `parse_stylesheet(src, Origin, Strictness) -> Result<Stylesheet, Unsupported>`
//! - `parse_declaration_block(src) -> Vec<Declaration>` for `style=""`
//! - `Stylesheet::effective_style_rules(&Media, is_supported)` for the rules in force
//! - `matches(&Document, NodeId, &ComplexSelector, &MatchContext) -> bool`
//! - `SelectorIndex<T>` to test only candidate rules per element
//! - `ComplexSelector::dependencies() -> SelectorDeps` for invalidation

pub mod matching;
pub mod media;
pub mod parser;
pub mod selector;
pub mod token;
pub mod tokenizer;

pub use matching::{
    matches, matches_compound, matches_list, AncestorKeys, AttributeFormState, FormState,
    IndexEntry, MatchContext, SelectorDeps, SelectorIndex, VisitedPolicy,
};
pub use media::{
    ColorScheme, DisplayMode, FontEnvironment, HoverCapability, Media, MediaCondition, MediaQuery,
    MediaQueryList, MediaType, PointerCapability, SupportsCondition,
};
pub use parser::{
    parse_declaration_block, parse_stylesheet, KeyframeSelector, Origin, Rule, StyleRuleRef,
    Stylesheet,
};
pub use selector::{
    parse_selector_list, AttrCase, AttrOp, Combinator, ComplexSelector, CompoundSelector,
    PseudoClass, PseudoElement, RelativeSelector, SelectorError, SelectorList, SimpleSelector,
    Specificity,
};
pub use token::*;
pub use tokenizer::tokenize;

/// `parse_declaration_block` under the name DESIGN.md uses.
pub fn parse_declarations(src: &str) -> Vec<Declaration> {
    parse_declaration_block(src)
}
