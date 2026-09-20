//! Layout: from the styled DOM to a tree of positioned fragments in app units.
//! `fragment.rs` is the shared contract between `layout` and `paint`; see DESIGN.md.

pub mod fragment;
pub use fragment::*;
