//! The user-agent stylesheet, modelled on the HTML Living Standard's Rendering
//! section. Split into CSS files by area so each can be read as CSS.

/// The sheet applied to every document.
pub const UA_CSS: &str = concat!(
    include_str!("ua/hidden.css"),
    "\n",
    include_str!("ua/flow.css"),
    "\n",
    include_str!("ua/phrasing.css"),
    "\n",
    include_str!("ua/tables.css"),
    "\n",
    include_str!("ua/forms.css"),
    "\n",
    include_str!("ua/svg.css"),
);

/// Extra rules for documents in quirks mode, applied after `UA_CSS`.
pub const QUIRKS_CSS: &str = include_str!("ua/quirks.css");

use crate::css::{parse_stylesheet, Origin, Stylesheet};
use std::sync::OnceLock;

static SHEET: OnceLock<Stylesheet> = OnceLock::new();
static QUIRKS: OnceLock<Stylesheet> = OnceLock::new();

/// The parsed user-agent sheet, parsed once. Lenient: the sheet uses a few values the
/// engine does not draw (system colours, `disclosure-*` markers), which parse to their
/// nearest supported form or are dropped.
pub fn sheet() -> &'static Stylesheet {
    SHEET.get_or_init(|| {
        parse_stylesheet(UA_CSS, Origin::UserAgent, crate::Strictness::Lenient)
            .expect("lenient parse cannot fail")
    })
}

/// The parsed quirks-mode additions.
pub fn quirks_sheet() -> &'static Stylesheet {
    QUIRKS.get_or_init(|| {
        parse_stylesheet(QUIRKS_CSS, Origin::UserAgent, crate::Strictness::Lenient)
            .expect("lenient parse cannot fail")
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn sheets_are_nonempty_and_balanced() {
        for css in [super::UA_CSS, super::QUIRKS_CSS] {
            assert!(!css.is_empty());
            assert_eq!(css.matches('{').count(), css.matches('}').count());
        }
    }
}
