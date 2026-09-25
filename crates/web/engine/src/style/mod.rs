//! The cascade: from stylesheets, presentational hints and inline style to one
//! `ComputedStyle` per element. See DESIGN.md. `computed.rs` is the shared contract
//! that layout and paint read; the rest of this module is owned by the style track.
//!
//! - `values`: value parsing (lengths, calc, colours, images, ...).
//! - `properties`: the longhand table (name, syntax, initial, inherited, computed form).
//! - `shorthands`: shorthand expansion into longhands.
//! - `fonts`: `font-family` to a bundled typeface.
//! - `ua`: the user-agent stylesheet; `hints`: presentational attributes.
//! - `cascade`: matching, ordering, inheritance, custom properties, restyle.
//! - `serialize`: `ComputedStyle::serialize`, the `getComputedStyle` string forms.

pub mod cascade;
pub mod computed;
pub mod fonts;
pub mod hints;
pub(crate) mod invalidation;
pub mod profile;
pub mod properties;
pub mod serialize;
pub mod shorthands;
pub mod ua;
pub mod values;
pub use cascade::{cascade, restyle, restyle_state, FontFace, Restyled, StyleEngine};
pub use computed::*;
