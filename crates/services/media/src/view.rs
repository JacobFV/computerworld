//! What every page of every skin is built from: the document shell with the seed's
//! palette on `<html>`, controls that post, artwork, and CSS-drawn icons.
//!
//! One service backs eight sites. The `mode` picks the routes (`video`, `audio`,
//! `music`); the *skin* picks the look, and is the seed's `skin` key or, when the seed
//! has none, its brand: `youtube`, `netflix`, `twitch`, `vimeo` and `tiktok` for a
//! video site, `spotify` and `soundcloud` for an audio one, `ytmusic` for music. Each
//! skin is a stylesheet next to this file over a base sheet its mode shares, and a
//! `skin-<name>` class on `<body>`.
use super::*;
pub use web::html::{button, div, el, form, hidden, href, link, span, text_input, Document, Html};

const ICONS_CSS: &str = include_str!("icons.css");
const VIDEO_CSS: &str = include_str!("video.css");
const MUSIC_CSS: &str = include_str!("music.css");
const VIDEO_SKINS: &[(&str, &str)] = &[
    ("youtube", include_str!("youtube.css")),
    ("netflix", include_str!("netflix.css")),
    ("twitch", include_str!("twitch.css")),
    ("vimeo", include_str!("vimeo.css")),
    ("tiktok", include_str!("tiktok.css")),
];
const AUDIO_SKINS: &[(&str, &str)] = &[
    ("spotify", include_str!("spotify.css")),
    ("soundcloud", include_str!("soundcloud.css")),
];
const MUSIC_SKINS: &[(&str, &str)] = &[("ytmusic", include_str!("ytmusic.css"))];

fn skins(mode: &str) -> &'static [(&'static str, &'static str)] {
    match mode {
        "audio" => AUDIO_SKINS,
        "music" => MUSIC_SKINS,
        _ => VIDEO_SKINS,
    }
}
/// The look of this site: the seed's `skin` if it names one its mode has, else the
/// skin its brand names, else the mode's first.
pub fn skin(state: &Value, mode: &str) -> &'static str {
    let all = skins(mode);
    let named = web::text(state, "skin");
    let brand: String = web::text(state, "brand")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    all.iter()
        .find(|(name, _)| named == *name)
        .or_else(|| all.iter().find(|(name, _)| brand.contains(name)))
        .unwrap_or(&all[0])
        .0
}
/// The page shell: title, language, the mode's base sheet then the skin's, the seed's
/// palette as custom properties, and `skin-<name> <page class>` on `<body>`.
pub fn document(
    state: &Value,
    mode: &str,
    title: &str,
    page_class: &str,
    body: Vec<Html>,
) -> Document {
    let skin = skin(state, mode);
    let sheet = skins(mode)
        .iter()
        .find(|(name, _)| *name == skin)
        .map_or("", |(_, css)| css);
    let theme = state.get("theme").cloned().unwrap_or(Value::Null);
    let mut root = String::new();
    for (key, property) in [
        ("accent", "--accent"),
        ("background", "--paper"),
        ("surface", "--surface"),
        ("ink", "--ink"),
        ("muted", "--muted"),
    ] {
        let value = web::text(&theme, key);
        // Only a plain hex colour goes on the element: the value is seed text.
        if value.starts_with('#') && value[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            if !root.is_empty() {
                root.push_str("; ");
            }
            root.push_str(&format!("{property}: {value}"));
        }
    }
    let mut doc = Document::new(title)
        .lang("en")
        .stylesheet(ICONS_CSS)
        .stylesheet(if mode == "video" {
            VIDEO_CSS
        } else {
            MUSIC_CSS
        })
        .stylesheet(sheet)
        .body_class(&format!("skin-{skin} {page_class}"))
        .body(body);
    if !root.is_empty() {
        doc = doc.root_style(&root);
    }
    doc
}
/// A control that posts: a one-button form carrying `fields`, the button holding the
/// id the control always had (`<id>-form` is the form around it).
pub fn act(id: &str, url: &str, fields: &[(&str, &str)], control: Html) -> Html {
    form(&format!("{id}-form"), url, "post")
        .class("act")
        .each(fields.iter(), |(k, v)| hidden(k, v))
        .child(control.id(id).attr("type", "submit"))
}
/// The button of an [`act`]: a class and, for one that shows only a glyph, its name.
pub fn press(class: &str, label: &str) -> Html {
    el("button").class(class).when(!label.is_empty(), |b| {
        b.attr("aria-label", label).attr("title", label)
    })
}
/// A form with one visible text field and its submit button: the Page model's
/// `form(id, url, [(field, label, value)])`, with the same `<id>-<field>` and
/// `<id>-submit` ids.
pub fn field_form(
    id: &str,
    url: &str,
    method: &str,
    field: &str,
    label: &str,
    value: &str,
    submit: &str,
) -> Html {
    form(id, url, method)
        .class("field")
        .child(
            text_input(&format!("{id}-{field}"), field, value)
                .attr("aria-label", label)
                .attr("placeholder", label)
                .attr("autocomplete", "off"),
        )
        .child(button(&format!("{id}-submit"), submit))
}
/// A link that says when it leads to the page it sits on, so a reader and an agent are told
/// they are already there rather than offered a trip that lands where they stand.
pub fn here_aware(node: Html, url: &str, here: &str) -> Html {
    if url != here {
        return node;
    }
    node.class("on").attr("aria-current", "page")
}
/// A CSS-drawn glyph: `<span class="ic ic-play">`. The stylesheet draws it.
pub fn icon(name: &str) -> Html {
    span(&format!("ic ic-{name}"))
        .attr("aria-hidden", "true")
        .child(el("i"))
}
/// Artwork: `cw_artwork`'s composition for `of`, served rasterised by this site at
/// `/art/<of>` at the size it is shown. `radius` rounds the raster's own corners; a
/// radius near half the side is a round avatar and is made exactly round.
pub fn cover(id: &str, of: &str, label: &str, side: u32, radius: u32) -> Html {
    let radius = if radius * 2 + 8 >= side {
        side / 2
    } else {
        radius
    };
    el("img")
        .id(id)
        .class("cover")
        .attr(
            "src",
            format!("/art/{}?size={side}&radius={radius}", encode(of)),
        )
        .attr("alt", label)
        .attr("width", side.to_string())
        .attr("height", side.to_string())
}
/// A picture that fills a fixed-aspect box (`object-fit: cover`): a video still, a
/// poster, a banner. `size` is the square raster asked for, not the box.
pub fn still(id: &str, of: &str, label: &str, size: u32) -> Html {
    el("img")
        .id(id)
        .class("still")
        .attr("src", format!("/art/{}?size={size}", encode(of)))
        .attr("alt", label)
}
/// The first letter of a name, upper-cased: what an avatar without a picture shows.
pub fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}
/// `http://` links in a text become real links, `<prefix>-link-<word index>`.
pub fn links(prefix: &str, text: &str) -> Vec<Html> {
    text.split_whitespace()
        .enumerate()
        .filter_map(|(i, word)| {
            let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
            (url.starts_with("http://") || url.starts_with("https://"))
                .then(|| link(&format!("{prefix}-link-{i}"), url, url))
        })
        .collect()
}
/// "184K", "1.2M": how every one of these sites prints a large count.
pub fn short(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        _ => {
            let tenths = n / 100_000;
            if tenths.is_multiple_of(10) {
                format!("{}M", tenths / 10)
            } else {
                format!("{}.{}M", tenths / 10, tenths % 10)
            }
        }
    }
}
