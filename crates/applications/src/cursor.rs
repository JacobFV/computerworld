//! The mouse pointer: which shape a target under it asks for, and the glyph a desktop
//! shell draws for that shape. The action reply's `cursor` hint and the pointer painted
//! into the frame both come from here, so they never disagree.
use crate::desktop_scene::DesktopTheme;
use cw_scene::Color;

/// A pointer shape, named after the CSS cursor it corresponds to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CursorKind {
    Default,
    Text,
    Pointer,
    NsResize,
    EwResize,
    NeswResize,
    NwseResize,
    Grab,
    Grabbing,
    Crosshair,
}
impl CursorKind {
    /// The CSS name a host page sets on its canvas.
    pub fn css_name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Text => "text",
            Self::Pointer => "pointer",
            Self::NsResize => "ns-resize",
            Self::EwResize => "ew-resize",
            Self::NeswResize => "nesw-resize",
            Self::NwseResize => "nwse-resize",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::Crosshair => "crosshair",
        }
    }
    /// The shape a CSS cursor name asks for; names this shell has no glyph for
    /// (`wait`, `help`, `move`) fall back to the arrow.
    pub fn from_css(name: &str) -> Self {
        match name {
            "text" | "vertical-text" => Self::Text,
            "pointer" => Self::Pointer,
            "ns-resize" | "row-resize" | "n-resize" | "s-resize" => Self::NsResize,
            "ew-resize" | "col-resize" | "e-resize" | "w-resize" => Self::EwResize,
            "nesw-resize" | "ne-resize" | "sw-resize" => Self::NeswResize,
            "nwse-resize" | "nw-resize" | "se-resize" => Self::NwseResize,
            "grab" => Self::Grab,
            "grabbing" => Self::Grabbing,
            "crosshair" => Self::Crosshair,
            _ => Self::Default,
        }
    }
    /// The shape an interaction target asks for; `captured` while a drag of it is under
    /// way. A resize handle names its edge, a title bar is grabbed, a canvas takes aim,
    /// text is edited, and everything else that reacts to a click is a pointer.
    pub fn for_target(target: &str, captured: bool) -> Self {
        if let Some(edge) = target.rsplit_once("resize:").map(|(_, edge)| edge) {
            return match edge {
                "n" | "s" => Self::NsResize,
                "e" | "w" => Self::EwResize,
                "ne" | "sw" => Self::NeswResize,
                "nw" | "se" => Self::NwseResize,
                _ => Self::Default,
            };
        }
        if target == "drag" || target.ends_with(":drag") {
            return if captured { Self::Grabbing } else { Self::Grab };
        }
        // An image editor's canvas takes aim, not a click.
        if target.contains(":canvas:") {
            return Self::Crosshair;
        }
        if target.ends_with("editor-text")
            || target.contains(":content:editor-text:")
            || target.ends_with("terminal-input")
            || target.ends_with("shell:address")
        {
            return Self::Text;
        }
        Self::Pointer
    }
    /// The glyph's outline as a closed polygon around the hot spot at `(0, 0)`.
    pub fn glyph(self) -> Vec<(i32, i32)> {
        match self {
            Self::Default => vec![
                (0, 0),
                (0, 17),
                (4, 13),
                (7, 20),
                (10, 19),
                (7, 12),
                (12, 12),
            ],
            Self::Text => vec![
                (-3, -8),
                (3, -8),
                (3, -6),
                (1, -6),
                (1, 6),
                (3, 6),
                (3, 8),
                (-3, 8),
                (-3, 6),
                (-1, 6),
                (-1, -6),
                (-3, -6),
            ],
            Self::Pointer => vec![
                (-1, 0),
                (2, 0),
                (2, 7),
                (5, 7),
                (5, 8),
                (8, 8),
                (8, 9),
                (11, 9),
                (11, 16),
                (9, 19),
                (1, 19),
                (-4, 14),
                (-4, 12),
                (-2, 11),
                (-1, 11),
            ],
            Self::NsResize => vec![
                (0, -9),
                (4, -5),
                (1, -5),
                (1, 5),
                (4, 5),
                (0, 9),
                (-4, 5),
                (-1, 5),
                (-1, -5),
                (-4, -5),
            ],
            Self::EwResize => Self::NsResize
                .glyph()
                .into_iter()
                .map(|(x, y)| (y, x))
                .collect(),
            Self::NwseResize => vec![
                (-7, -7),
                (-1, -7),
                (-3, -5),
                (5, 3),
                (7, 1),
                (7, 7),
                (1, 7),
                (3, 5),
                (-5, -3),
                (-7, -1),
            ],
            Self::NeswResize => Self::NwseResize
                .glyph()
                .into_iter()
                .map(|(x, y)| (-x, y))
                .collect(),
            Self::Grab => vec![
                (-5, -1),
                (-3, -3),
                (-1, -2),
                (0, -6),
                (2, -7),
                (3, -3),
                (5, -6),
                (7, -5),
                (7, -1),
                (9, -2),
                (11, 0),
                (11, 8),
                (8, 12),
                (0, 12),
                (-5, 6),
            ],
            Self::Grabbing => vec![
                (-4, 2),
                (-1, 0),
                (2, -1),
                (5, -1),
                (8, 0),
                (11, 2),
                (11, 8),
                (8, 12),
                (0, 12),
                (-4, 8),
            ],
            Self::Crosshair => vec![
                (-1, -8),
                (1, -8),
                (1, -1),
                (8, -1),
                (8, 1),
                (1, 1),
                (1, 8),
                (-1, 8),
                (-1, 1),
                (-8, 1),
                (-8, -1),
                (-1, -1),
            ],
        }
    }
    /// The glyph's fill and outline in a platform's idiom: macOS draws a black arrow with a
    /// white edge, Windows and Ubuntu a white arrow with a black edge. Phones draw none.
    pub fn colors(theme: DesktopTheme) -> Option<(Color, Color)> {
        match theme {
            DesktopTheme::Macos => Some((Color::BLACK, Color::WHITE)),
            DesktopTheme::Windows | DesktopTheme::Ubuntu => Some((Color::WHITE, Color::BLACK)),
            DesktopTheme::Ios | DesktopTheme::Android => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_pick_their_shape() {
        assert_eq!(
            CursorKind::for_target("window:3:resize:se", false),
            CursorKind::NwseResize
        );
        assert_eq!(
            CursorKind::for_target("window:3:resize:n", false),
            CursorKind::NsResize
        );
        assert_eq!(
            CursorKind::for_target("window:3:drag", false),
            CursorKind::Grab
        );
        assert_eq!(
            CursorKind::for_target("window:3:drag", true),
            CursorKind::Grabbing
        );
        assert_eq!(
            CursorKind::for_target("window:3:content:editor-text", false),
            CursorKind::Text
        );
        assert_eq!(
            CursorKind::for_target("shell:launch:terminal", false),
            CursorKind::Pointer
        );
        assert_eq!(
            CursorKind::for_target("window:1:content:canvas:1", false),
            CursorKind::Crosshair
        );
        assert_eq!(CursorKind::Grabbing.css_name(), "grabbing");
    }
    #[test]
    fn every_glyph_is_a_polygon_around_its_hot_spot() {
        for kind in [
            CursorKind::Default,
            CursorKind::Text,
            CursorKind::Pointer,
            CursorKind::NsResize,
            CursorKind::EwResize,
            CursorKind::NeswResize,
            CursorKind::NwseResize,
            CursorKind::Grab,
            CursorKind::Grabbing,
            CursorKind::Crosshair,
        ] {
            let glyph = kind.glyph();
            assert!(glyph.len() >= 3, "{kind:?}");
            assert!(
                glyph.iter().all(|(x, y)| x.abs() <= 24 && y.abs() <= 24),
                "{kind:?}"
            );
        }
        assert!(CursorKind::colors(DesktopTheme::Ios).is_none());
        assert!(CursorKind::colors(DesktopTheme::Macos).is_some());
    }
}
