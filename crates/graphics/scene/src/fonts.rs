//! CSS `font-family` resolution onto the bundled faces.
//!
//! A page names families the way a stylesheet does — `"Helvetica Neue", Arial,
//! sans-serif` — and the renderer has a fixed set of openly licensed faces, some of
//! them metric-compatible stand-ins for families it cannot ship (Arimo for Arial and
//! Helvetica, Tinos for Times New Roman, Cousine for Courier New, Gelasio for
//! Georgia, Carlito for Calibri, Caladea for Cambria). [`resolve_family`] walks the
//! list the way a browser does — first family that is available wins, generic
//! families at the end catch the rest — with the alias table below standing in for
//! the host font set. Nothing here consults a host: the answer is a function of the
//! string alone, so the same page resolves to the same face on every machine.
use crate::metrics::Typeface;

/// The face a CSS `font-family` list resolves to.
///
/// Names are matched case-insensitively, with quotes (`"Open Sans"`, `'Lato'`)
/// stripped and internal whitespace collapsed. For each family in order: a bundled
/// family's own name (see [`Typeface::family_name`]) wins outright; a known alias
/// maps to its stand-in; a generic family (`sans-serif`, `serif`, `monospace`,
/// `system-ui`, `ui-sans-serif`, `ui-serif`, `ui-monospace`, `ui-rounded`,
/// `cursive`, `fantasy`, `math`, `emoji`) maps to the bundled face that plays that
/// part; anything else is skipped. An empty list, or one naming nothing bundled,
/// resolves to the default sans, Arimo.
pub fn resolve_family(list: &str) -> Typeface {
    families(list)
        .into_iter()
        .find_map(|name| resolve_one(&name))
        .unwrap_or(DEFAULT)
}

/// The face a page gets when its list names nothing bundled: the web's default
/// sans-serif, which Arimo stands in for.
pub const DEFAULT: Typeface = Typeface::Arimo;

/// The bundled face for one family name, already normalised (lower case, unquoted,
/// single-spaced), or `None` when the name is unknown.
fn resolve_one(name: &str) -> Option<Typeface> {
    if let Some(face) = Typeface::ALL
        .iter()
        .copied()
        .find(|face| face.family_name().eq_ignore_ascii_case(name))
    {
        return Some(face);
    }
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map(|(_, face)| *face)
}

/// Family names that are not bundled and the face that stands in for each. Bundled
/// families are matched by [`Typeface::family_name`] before this table is consulted,
/// so an entry here never shadows a real face.
pub const ALIASES: &[(&str, Typeface)] = &[
    // Arial and Helvetica, and the platform UI stacks pages spell out in full.
    ("arial", Typeface::Arimo),
    ("helvetica", Typeface::Arimo),
    ("helvetica neue", Typeface::Arimo),
    ("-apple-system", Typeface::Arimo),
    ("blinkmacsystemfont", Typeface::Arimo),
    ("segoe ui", Typeface::Arimo),
    ("liberation sans", Typeface::Arimo),
    ("nimbus sans", Typeface::Arimo),
    // Times.
    ("times", Typeface::Tinos),
    ("times new roman", Typeface::Tinos),
    ("liberation serif", Typeface::Tinos),
    ("nimbus roman", Typeface::Tinos),
    // Courier is metric-compatible with Cousine; the modern editor monospaces are
    // closer to JetBrains Mono.
    ("courier", Typeface::Cousine),
    ("courier new", Typeface::Cousine),
    ("liberation mono", Typeface::Cousine),
    ("menlo", Typeface::JetBrainsMono),
    ("monaco", Typeface::JetBrainsMono),
    ("consolas", Typeface::JetBrainsMono),
    ("sf mono", Typeface::JetBrainsMono),
    ("cascadia code", Typeface::JetBrainsMono),
    ("cascadia mono", Typeface::JetBrainsMono),
    ("fira code", Typeface::JetBrainsMono),
    ("fira mono", Typeface::JetBrainsMono),
    ("source code pro", Typeface::JetBrainsMono),
    ("roboto mono", Typeface::JetBrainsMono),
    ("ubuntu mono", Typeface::JetBrainsMono),
    // The Microsoft Office faces and their metric-compatible twins.
    ("georgia", Typeface::Gelasio),
    ("calibri", Typeface::Carlito),
    ("cambria", Typeface::Caladea),
    // Verdana and Tahoma are not bundled and have no metric-compatible twin; a
    // Linux Chromium substitutes its default sans (Liberation Sans, Arimo's metrics),
    // and the web engine's parity fixtures depend on matching that choice.
    ("verdana", Typeface::Arimo),
    ("tahoma", Typeface::Arimo),
    ("bitstream vera sans", Typeface::DejaVu),
    ("bitstream vera sans mono", Typeface::Mono),
    // Earlier names of bundled families.
    ("source sans pro", Typeface::SourceSans),
    ("source serif pro", Typeface::SourceSerif),
    ("playfair", Typeface::Playfair),
    ("ubuntu sans", Typeface::Ubuntu),
    // Generic families.
    ("sans-serif", Typeface::Arimo),
    ("serif", Typeface::Tinos),
    ("monospace", Typeface::Cousine),
    ("system-ui", Typeface::Arimo),
    ("ui-sans-serif", Typeface::Arimo),
    ("ui-serif", Typeface::Tinos),
    ("ui-monospace", Typeface::JetBrainsMono),
    ("ui-rounded", Typeface::Arimo),
    ("cursive", Typeface::Gelasio),
    ("fantasy", Typeface::Playfair),
    ("math", Typeface::DejaVu),
    ("emoji", Typeface::DejaVu),
];

/// The family names of a CSS list, in order, normalised: unquoted, lower case, with
/// runs of whitespace collapsed to one space. A comma inside quotes does not split.
fn families(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in list.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == ',' => out.push(std::mem::take(&mut current)),
            None => current.push(c),
        }
    }
    out.push(current);
    out.into_iter()
        .map(|name| {
            name.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_bundled_family_resolves_to_itself_by_name() {
        for face in Typeface::ALL {
            assert_eq!(resolve_family(face.family_name()), face, "{face:?}");
            assert_eq!(
                resolve_family(&face.family_name().to_uppercase()),
                face,
                "{face:?}"
            );
            assert_eq!(
                resolve_family(&format!("\"{}\", serif", face.family_name())),
                face,
                "{face:?}"
            );
        }
    }
    #[test]
    fn every_alias_maps_to_its_stand_in() {
        for (alias, face) in ALIASES {
            assert_eq!(resolve_family(alias), *face, "{alias}");
            assert_eq!(
                resolve_family(&format!("'{alias}'")),
                *face,
                "{alias} quoted"
            );
            assert_eq!(
                resolve_family(&format!("Nonexistent Family, {alias}, monospace")),
                *face,
                "{alias} after an unknown family"
            );
        }
        // Every alias this table promises, spelled the way pages spell them.
        let expected = [
            ("Arial", Typeface::Arimo),
            ("Helvetica", Typeface::Arimo),
            ("Helvetica Neue", Typeface::Arimo),
            ("-apple-system", Typeface::Arimo),
            ("BlinkMacSystemFont", Typeface::Arimo),
            ("Segoe UI", Typeface::Arimo),
            ("Times", Typeface::Tinos),
            ("Times New Roman", Typeface::Tinos),
            ("Courier", Typeface::Cousine),
            ("Courier New", Typeface::Cousine),
            ("Menlo", Typeface::JetBrainsMono),
            ("Monaco", Typeface::JetBrainsMono),
            ("Consolas", Typeface::JetBrainsMono),
            ("Georgia", Typeface::Gelasio),
            ("Calibri", Typeface::Carlito),
            ("Cambria", Typeface::Caladea),
            ("Verdana", Typeface::Arimo),
            ("Tahoma", Typeface::Arimo),
            ("sans-serif", Typeface::Arimo),
            ("serif", Typeface::Tinos),
            ("monospace", Typeface::Cousine),
            ("system-ui", Typeface::Arimo),
            ("ui-sans-serif", Typeface::Arimo),
            ("ui-serif", Typeface::Tinos),
            ("ui-monospace", Typeface::JetBrainsMono),
            ("cursive", Typeface::Gelasio),
            ("fantasy", Typeface::Playfair),
        ];
        for (name, face) in expected {
            assert_eq!(resolve_family(name), face, "{name}");
        }
    }
    #[test]
    fn the_first_bundled_family_wins_and_generics_catch_the_rest() {
        assert_eq!(
            resolve_family("-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif"),
            Typeface::Arimo
        );
        assert_eq!(resolve_family("Roboto, sans-serif"), Typeface::Roboto);
        assert_eq!(
            resolve_family("Gill Sans, 'Open Sans', Arial"),
            Typeface::OpenSans
        );
        assert_eq!(
            resolve_family("Garamond, Baskerville, serif"),
            Typeface::Tinos
        );
        assert_eq!(
            resolve_family("Gill Sans, Futura, monospace"),
            Typeface::Cousine
        );
        // A bundled family named exactly beats the alias that would otherwise apply.
        assert_eq!(resolve_family("Inter, Arial"), Typeface::Inter);
        assert_eq!(resolve_family("Arial, Inter"), Typeface::Arimo);
        assert_eq!(resolve_family("Source Sans 3"), Typeface::SourceSans);
        assert_eq!(
            resolve_family("Source Serif 4, Georgia"),
            Typeface::SourceSerif
        );
        assert_eq!(resolve_family("Playfair Display"), Typeface::Playfair);
        assert_eq!(resolve_family("JetBrains Mono"), Typeface::JetBrainsMono);
        assert_eq!(resolve_family("DejaVu Sans Mono"), Typeface::Mono);
    }
    #[test]
    fn unknown_lists_fall_back_to_the_default_sans() {
        assert_eq!(resolve_family(""), DEFAULT);
        assert_eq!(resolve_family("   "), DEFAULT);
        assert_eq!(resolve_family("Gill Sans, Futura"), DEFAULT);
        assert_eq!(resolve_family(",,"), DEFAULT);
        assert_eq!(resolve_family("\"unterminated"), DEFAULT);
    }
    #[test]
    fn names_are_normalised_before_matching() {
        assert_eq!(resolve_family("  TIMES   NEW\tROMAN  "), Typeface::Tinos);
        assert_eq!(resolve_family("'times new roman'"), Typeface::Tinos);
        assert_eq!(resolve_family("\"Open  Sans\""), Typeface::OpenSans);
        // A comma inside quotes is part of the name, not a separator.
        assert_eq!(
            resolve_family("\"Times, New Roman\", Georgia"),
            Typeface::Gelasio
        );
        assert_eq!(families("a, \"b, c\", 'd'  e ,,"), ["a", "b, c", "d e"]);
    }
}
