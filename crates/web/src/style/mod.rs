//! The cascade: from stylesheets, presentational hints and inline style to one
//! `ComputedStyle` per element. See DESIGN.md. `computed.rs` is the shared contract
//! that layout and paint read; the rest of this module is owned by the style track.

pub mod computed;
pub use computed::*;
