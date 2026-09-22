//! `font-family` resolution to a bundled `cw_scene::Typeface`, through
//! `cw_scene::fonts::resolve_family` (the alias table of metric-compatible
//! stand-ins), and the serialisation of family lists for `getComputedStyle`.

pub use crate::css::FontEnvironment;
use cw_scene::Typeface;

/// Resolves a family list to the bundled face the scene's alias table picks.
pub fn resolve_family(families: &[String]) -> Typeface {
    cw_scene::fonts::resolve_family(&serialize_family_list(families))
}

/// Resolves a family list on a device with the given installed fonts.
pub fn resolve_family_in(families: &[String], env: FontEnvironment) -> Typeface {
    match env {
        FontEnvironment::Bundled => resolve_family(families),
        FontEnvironment::LinuxBaseline => families
            .iter()
            .find_map(|f| linux_baseline_face(f))
            .unwrap_or(Typeface::Arimo),
    }
}

/// The face a stock Linux desktop has for one family name, or `None` when the name
/// is not installed there and the list moves on.
fn linux_baseline_face(name: &str) -> Option<Typeface> {
    let name = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    Some(match name.as_str() {
        "sans-serif" | "system-ui" | "ui-sans-serif" | "arial" | "helvetica"
        | "liberation sans" | "arimo" | "nimbus sans" => Typeface::Arimo,
        "serif" | "times" | "times new roman" | "liberation serif" | "tinos" | "nimbus roman" => {
            Typeface::Tinos
        }
        // Chromium's fixed-width default there is Liberation Mono (hn-front's
        // `monospace` input measures as it), and fontconfig binds Consolas to DejaVu
        // Sans Mono (slack-shell's code font).
        "monospace" | "courier" | "courier new" | "liberation mono" | "cousine" => {
            Typeface::Cousine
        }
        "consolas" | "dejavu sans mono" | "bitstream vera sans mono" => Typeface::Mono,
        "dejavu sans" | "bitstream vera sans" => Typeface::DejaVu,
        _ => return None,
    })
}

/// Serialises a family list the way `getComputedStyle` does (Chromium): a name that
/// is not a single identifier is quoted; generic families are not.
pub fn serialize_family_list(families: &[String]) -> String {
    families
        .iter()
        .map(|f| serialize_family(f))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn serialize_family(f: &str) -> String {
    let generic = matches!(
        f.to_ascii_lowercase().as_str(),
        "serif"
            | "sans-serif"
            | "monospace"
            | "cursive"
            | "fantasy"
            | "system-ui"
            | "ui-serif"
            | "ui-sans-serif"
            | "ui-monospace"
            | "ui-rounded"
            | "emoji"
            | "math"
            | "fangsong"
    );
    if generic || is_identifier_sequence(f) {
        f.to_owned()
    } else {
        let mut s = String::from("\"");
        for c in f.chars() {
            if c == '"' || c == '\\' {
                s.push('\\');
            }
            s.push(c);
        }
        s.push('"');
        s
    }
}

/// Whether `f` is one plain CSS identifier.
fn is_identifier_sequence(f: &str) -> bool {
    if f.is_empty() || f.contains(' ') {
        return false;
    }
    std::iter::once(f).all(|w| {
        let mut chars = w.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        let start_ok = first.is_ascii_alphabetic()
            || first == '_'
            || first as u32 > 0x7F
            || (first == '-' && w.len() > 1 && !w.chars().nth(1).unwrap().is_ascii_digit());
        start_ok
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c as u32 > 0x7F)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_go_through_the_scene_table() {
        assert_eq!(
            resolve_family(&["Helvetica Neue".into(), "Arial".into(), "sans-serif".into()]),
            Typeface::Arimo
        );
        assert_eq!(
            resolve_family(&["Nope".into(), "Roboto".into()]),
            Typeface::Roboto
        );
        assert_eq!(resolve_family(&["Courier New".into()]), Typeface::Cousine);
        assert_eq!(resolve_family(&["Inter".into()]), Typeface::Inter);
        assert_eq!(resolve_family(&[]), cw_scene::fonts::DEFAULT);
    }

    #[test]
    fn a_linux_baseline_device_has_only_liberation_and_dejavu() {
        let env = FontEnvironment::LinuxBaseline;
        let list = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        // What Chromium shaped the parity fixtures' lists with (their fonts.json).
        assert_eq!(
            resolve_family_in(
                &list(&["Inter", "-apple-system", "Segoe UI", "Roboto", "sans-serif"]),
                env
            ),
            Typeface::Arimo
        );
        assert_eq!(
            resolve_family_in(
                &list(&["Poppins", "Montserrat", "Inter", "sans-serif"]),
                env
            ),
            Typeface::Arimo
        );
        assert_eq!(
            resolve_family_in(&list(&["Georgia", "Times New Roman", "serif"]), env),
            Typeface::Tinos
        );
        assert_eq!(
            resolve_family_in(&list(&["JetBrains Mono", "Courier New", "monospace"]), env),
            Typeface::Cousine
        );
        assert_eq!(
            resolve_family_in(
                &list(&[
                    "ui-monospace",
                    "JetBrains Mono",
                    "Courier  NEW",
                    "monospace"
                ]),
                env
            ),
            Typeface::Cousine
        );
        assert_eq!(
            resolve_family_in(
                &list(&[
                    "JetBrains Mono",
                    "Fira Code",
                    "Menlo",
                    "Consolas",
                    "monospace"
                ]),
                env
            ),
            Typeface::Mono
        );
        assert_eq!(
            resolve_family_in(&list(&["Roboto", "Open Sans", "Arial", "sans-serif"]), env),
            Typeface::Arimo
        );
        assert_eq!(
            resolve_family_in(&list(&["monospace"]), env),
            Typeface::Cousine
        );
        assert_eq!(resolve_family_in(&list(&["Nope"]), env), Typeface::Arimo);
        // The world's browser keeps its bundled faces.
        assert_eq!(
            resolve_family_in(&list(&["Inter", "sans-serif"]), FontEnvironment::Bundled),
            Typeface::Inter
        );
    }

    #[test]
    fn family_serialization() {
        assert_eq!(
            serialize_family_list(&["Helvetica Neue".into(), "Arial".into(), "sans-serif".into()]),
            "\"Helvetica Neue\", Arial, sans-serif"
        );
        assert_eq!(
            serialize_family_list(&["Source Sans 3".into()]),
            "\"Source Sans 3\""
        );
        assert_eq!(serialize_family_list(&["a\"b".into()]), "\"a\\\"b\"");
        assert_eq!(
            serialize_family_list(&["Open  Sans".into()]),
            "\"Open  Sans\""
        );
    }
}
