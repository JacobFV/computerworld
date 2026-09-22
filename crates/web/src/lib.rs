//! A deterministic web engine for the simulated browser: HTML to a DOM, CSS through
//! the cascade to computed styles, fixed-point layout, and paint to a `cw_scene::Scene`.
//! See `DESIGN.md` for the contracts between the modules and
//! `docs/contracts/web-engine-plan.md` for the milestones.

pub mod css;
pub mod dom;
pub mod geom;
pub mod html;
pub mod layout;
pub mod page;
pub mod paint;
pub mod script;
pub mod style;

/// How unknown or unsupported input is treated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Strictness {
    /// Ignore what is not understood and record it; what browsers do.
    #[default]
    Lenient,
    /// Fail on the first unsupported construct, naming it; for sites authored here.
    Strict,
}

/// Something the engine does not implement, recorded in lenient mode or returned as the
/// error in strict mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    pub kind: UnsupportedKind,
    pub name: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedKind {
    Element,
    Attribute,
    Property,
    Value,
    Selector,
    AtRule,
    Feature,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unsupported {:?} `{}`: {}",
            self.kind, self.name, self.detail
        )
    }
}

/// The viewport a document is laid out and painted in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    /// Device pixel ratio numerator over 1; phones use 2 or 3.
    pub scale: u8,
    /// Zoom in percent (100 = none), applied to CSS px.
    pub zoom: u16,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            scale: 1,
            zoom: 100,
        }
    }
}
