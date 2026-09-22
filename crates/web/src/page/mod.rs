//! `cw_protocol::Page` to HTML plus a stylesheet: the migration bridge. Every
//! `PageElement` becomes semantic HTML whose `id` is the element's id (so the browser's
//! click, fill and submit by id keep working), and the stylesheet reproduces what
//! `crates/browser/src/page_scene.rs` draws today as closely as CSS can: the same
//! sizes, paddings, radii and colours, the 16 px page gutter, the centred
//! `content_width` column, link pills, buttons, rows as wrapping flex lines, grids as
//! equal columns with the column-drop rule approximated by media queries, `pin: top`
//! as a sticky bar, `scroll_x` as a sideways scroll container, icons as a span holding
//! the symbol name inside the symbol's box.
//!
//! What is not reproduced: the app chrome `page_scene` adds for the four legacy titles
//! (Mail, Chat, Calendar, Documents: the 64 px white bar is emitted, the 176 px sidebar
//! and the two-column mail layout are not), a form's `Submit` label rewrite is
//! reproduced from the same table, and a missing image asset's alt-text fallback is
//! the browser's business.
//!
//! The emitted CSS uses only the properties in [`SUPPORTED_PROPERTIES`]; a test checks
//! every declaration against that list so the strict validator never sees a surprise.

use cw_protocol::{Page, PageAction, PageElement, Style};
use std::fmt::Write as _;

/// Every CSS property the converter is allowed to emit. Kept in step with the style
/// module's property table; a test walks all emitted declarations against this list.
pub const SUPPORTED_PROPERTIES: &[&str] = &[
    "display",
    "position",
    "top",
    "bottom",
    "left",
    "right",
    "width",
    "max-width",
    "min-width",
    "height",
    "box-sizing",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border",
    "border-top",
    "border-bottom",
    "border-radius",
    "background-color",
    "color",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "text-align",
    "text-decoration-line",
    "text-overflow",
    "white-space",
    "vertical-align",
    "overflow-x",
    "overflow-y",
    "visibility",
    "cursor",
    "flex",
    "flex-wrap",
    "align-items",
    "justify-content",
    "gap",
    "grid-template-columns",
    "z-index",
    "appearance",
    "outline-style",
];

/// The default page face: what `page_scene` measures with (`Typeface::DejaVu`).
const FACE: &str = "'DejaVu Sans', sans-serif";
const MONO: &str = "'DejaVu Sans Mono', monospace";
/// Vertical air below a block element (`page_scene::GAP`).
const GAP: u32 = 12;
/// The page gutter.
const GUTTER: u32 = 16;

fn line_height(size: u16) -> u32 {
    (u32::from(size) * 13).div_ceil(10)
}

fn esc_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn esc_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn colour(value: Option<&String>, fallback: &str) -> String {
    match value {
        Some(v) if cw_protocol::valid_color(v) => v.clone(),
        _ => fallback.to_owned(),
    }
}

/// A colour as `#rrggbb` bytes, for mixing.
fn rgb(value: &str) -> Option<(u8, u8, u8)> {
    if !cw_protocol::valid_color(value) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).ok();
    Some((b(1)?, b(3)?, b(5)?))
}

/// Blends `b` into `a` by `pct` (`page_scene::mix`).
fn mix(a: &str, b: &str, pct: u32) -> String {
    match (rgb(a), rgb(b)) {
        (Some(a), Some(b)) => {
            let c = |a: u8, b: u8| ((u32::from(a) * (100 - pct) + u32::from(b) * pct) / 100) as u8;
            format!("#{:02x}{:02x}{:02x}", c(a.0, b.0), c(a.1, b.1), c(a.2, b.2))
        }
        _ => a.to_owned(),
    }
}

fn hash(s: &str) -> u64 {
    let mut id = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        id = (id ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    id
}

/// Stand-in artwork tint, chosen from the label (`page_scene::tint`).
fn tint(label: &str) -> &'static str {
    const PALETTE: [&str; 8] = [
        "#cbd5e4", "#d2ded4", "#e2d6ce", "#d6d1e3", "#cddce4", "#e4d8ce", "#d4dace", "#dbd1d5",
    ];
    PALETTE[(hash(label) % 8) as usize]
}

/// What a form is for, read off its id (`page_scene::form_purpose`).
fn form_purpose(form: &str) -> (Option<&'static str>, &'static str) {
    let parts: Vec<&str> = form.split(['-', '_']).collect();
    type Purpose = (&'static [&'static str], Option<&'static str>, &'static str);
    const PURPOSES: [Purpose; 18] = [
        (&["search", "find", "q"], None, "Search"),
        (&["compose"], Some("New message"), "Send"),
        (
            &["send", "dm", "composer", "message", "prompt"],
            None,
            "Send",
        ),
        (&["reply"], None, "Reply"),
        (&["comment"], Some("Add a comment"), "Post comment"),
        (&["rsvp"], None, "RSVP"),
        (&["react"], None, "React"),
        (&["move"], None, "Move"),
        (&["share"], Some("Share"), "Share"),
        (&["upload"], Some("Upload a file"), "Upload"),
        (&["folder"], Some("New folder"), "Create"),
        (&["playlist"], Some("New playlist"), "Create"),
        (&["event"], Some("New event"), "Create"),
        (&["subscribe", "email"], None, "Subscribe"),
        (&["create", "new"], Some("Create"), "Create"),
        (&["label"], None, "Apply"),
        (&["edit"], Some("Edit"), "Save"),
        (&["metadata", "cell", "slide"], None, "Save"),
    ];
    PURPOSES
        .iter()
        .find(|(keys, _, _)| keys.iter().any(|k| parts.contains(k)))
        .map_or((None, "Submit"), |(_, title, submit)| (*title, *submit))
}

fn submit_label<'a>(id: &str, text: &'a str) -> &'a str {
    if text == "Submit" {
        form_purpose(id.strip_suffix("-submit").unwrap_or(id)).1
    } else {
        text
    }
}

fn style_of(e: &PageElement) -> Option<&Style> {
    match e {
        PageElement::Row { style, .. }
        | PageElement::Grid { style, .. }
        | PageElement::Card { style, .. }
        | PageElement::Styled { style, .. }
        | PageElement::Thumbnail { style, .. }
        | PageElement::Badge { style, .. }
        | PageElement::Icon { style, .. }
        | PageElement::Divider { style, .. } => Some(style),
        PageElement::Link { style, .. }
        | PageElement::Button { style, .. }
        | PageElement::Image { style, .. } => style.as_ref(),
        _ => None,
    }
}

fn fixed_width(e: &PageElement) -> Option<u32> {
    match e {
        PageElement::Image { width, .. } if *width > 0 => Some(*width),
        _ => style_of(e).and_then(|s| s.width),
    }
}

fn explicit_flex(e: &PageElement) -> bool {
    style_of(e).is_some_and(|s| s.flex.is_some())
}

/// Whether a block holds reading matter (`page_scene::prose`).
fn prose(children: &[PageElement]) -> bool {
    children.iter().any(|c| match c {
        PageElement::Heading { text, .. }
        | PageElement::Text { text, .. }
        | PageElement::Styled { text, .. } => text.chars().count() >= 40,
        PageElement::Input { .. } | PageElement::Image { .. } => true,
        PageElement::Thumbnail { style, .. } => style.height.unwrap_or(0) >= 120,
        PageElement::Row { children, .. }
        | PageElement::Grid { children, .. }
        | PageElement::Card { children, .. }
        | PageElement::Group { children, .. }
        | PageElement::Form { children, .. } => prose(children),
        _ => false,
    })
}

fn control_pad(style: &Style, button: bool) -> u32 {
    let boxed = style.background.is_some() || style.border.is_some();
    style
        .padding
        .unwrap_or(if button {
            14
        } else if boxed {
            8
        } else {
            0
        })
        .min(64)
}

/// A declaration list builder that keeps `property: value` pairs in order.
#[derive(Default)]
struct Decls(Vec<String>);

impl Decls {
    fn push(&mut self, property: &str, value: impl std::fmt::Display) -> &mut Self {
        debug_assert!(
            SUPPORTED_PROPERTIES.contains(&property),
            "unsupported property {property}"
        );
        self.0.push(format!("{property}: {value}"));
        self
    }
    fn px(&mut self, property: &str, value: u32) -> &mut Self {
        self.push(property, format!("{value}px"))
    }
    fn attr(&self) -> String {
        if self.0.is_empty() {
            String::new()
        } else {
            format!(" style=\"{}\"", esc_attr(&self.0.join("; ")))
        }
    }
}

struct Theme {
    /// The page face: the theme's `font` list ahead of the default, or the default.
    face: String,
    accent: String,
    ink: String,
    muted: String,
    border: String,
    surface: String,
    background: String,
    content_width: Option<u32>,
    themed: bool,
}

struct Emitter {
    theme: Theme,
    html: String,
    css: String,
    /// Media queries for grids, appended after the base sheet.
    extra_css: String,
    /// Whether the element being emitted is a direct child of a row (its wrapper cell
    /// carries the flex).
    depth: usize,
}

impl Emitter {
    fn attrs(&self, id: &str, decls: &Decls, lang: Option<&str>) -> String {
        let mut s = format!(" id=\"{}\"", esc_attr(id));
        if let Some(l) = lang {
            let _ = write!(s, " lang=\"{}\"", esc_attr(l));
        }
        s.push_str(&decls.attr());
        s
    }

    fn ink_of(&self, style: &Style) -> String {
        colour(style.color.as_ref(), &self.theme.ink)
    }

    fn box_decor(&self, style: &Style, decls: &mut Decls, default_radius: u32) {
        if let Some(bg) = style
            .background
            .as_ref()
            .filter(|c| cw_protocol::valid_color(c))
        {
            decls.push("background-color", bg);
        }
        if let Some(edge) = style
            .border
            .as_ref()
            .filter(|c| cw_protocol::valid_color(c))
        {
            decls.push("border", format!("1px solid {edge}"));
        }
        let radius = style.radius.unwrap_or(default_radius).min(64);
        if radius > 0 {
            decls.px("border-radius", radius);
        }
    }

    fn hidden_fields(&mut self, action: &PageAction) {
        for (name, value) in &action.fields {
            let _ = write!(
                self.html,
                "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
                esc_attr(name),
                esc_attr(value)
            );
        }
    }

    /// Opens a click target around an element: a link for a GET action, a form with a
    /// submit button for anything else. Returns what closes it.
    fn open_action(&mut self, action: Option<&PageAction>, block: bool) -> &'static str {
        let Some(action) = action else { return "" };
        let class = if block {
            "cw-target cw-target-block"
        } else {
            "cw-target"
        };
        if action.method.eq_ignore_ascii_case("GET") && action.fields.is_empty() {
            let _ = write!(
                self.html,
                "<a class=\"{class}\" href=\"{}\">",
                esc_attr(&action.url)
            );
            "</a>"
        } else {
            let _ = write!(
                self.html,
                "<form class=\"cw-action\" action=\"{}\" method=\"{}\">",
                esc_attr(&action.url),
                esc_attr(&action.method.to_ascii_lowercase())
            );
            self.hidden_fields(action);
            let _ = write!(self.html, "<button type=\"submit\" class=\"{class}\">");
            "</button></form>"
        }
    }

    fn text_decls(&self, style: &Style, size: u16, decls: &mut Decls) {
        decls.px("font-size", u32::from(size));
        decls.px("line-height", line_height(size));
        if matches!(style.weight.as_deref(), Some("bold") | Some("medium")) {
            decls.push("font-weight", "bold");
        }
        if style.italic == Some(true) {
            decls.push("font-style", "italic");
        }
        if style.mono == Some(true) {
            decls.push("font-family", MONO);
        }
        if style.one_line == Some(true) {
            decls
                .push("white-space", "nowrap")
                .push("overflow-x", "hidden")
                .push("overflow-y", "hidden")
                .push("text-overflow", "ellipsis");
        }
    }

    fn line_wrapper(&mut self, style: &Style, margin_bottom: u32) {
        let mut d = Decls::default();
        match style.align.as_deref() {
            Some("center") => {
                d.push("text-align", "center");
            }
            Some("right") | Some("end") => {
                d.push("text-align", "right");
            }
            _ => {}
        }
        d.px("margin-bottom", margin_bottom);
        let _ = write!(self.html, "<div class=\"cw-line\"{}>", d.attr());
    }

    fn children(&mut self, children: &[PageElement]) {
        self.depth += 1;
        for c in children {
            self.element(c);
        }
        self.depth -= 1;
    }

    fn element(&mut self, e: &PageElement) {
        let plain = Style::default();
        match e {
            PageElement::Heading { id, text, level } => {
                let level = (*level).clamp(1, 6);
                let mut d = Decls::default();
                d.px("font-size", if level <= 1 { 18 } else { 15 });
                let _ = writeln!(
                    self.html,
                    "<h{level} class=\"cw-heading\"{}>{}</h{level}>",
                    self.attrs(id, &d, None),
                    esc_text(text)
                );
            }
            PageElement::Text { id, text } => {
                let _ = writeln!(
                    self.html,
                    "<p class=\"cw-text\"{}>{}</p>",
                    self.attrs(id, &Decls::default(), None),
                    esc_text(text)
                );
            }
            PageElement::Link {
                id,
                text,
                url,
                style,
            } => {
                let styled = style.is_some();
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(13).clamp(6, 96);
                let boxed = style.background.is_some() || style.border.is_some();
                let pad = control_pad(style, false);
                self.line_wrapper(style, if styled { 6 } else { 9 });
                let mut d = Decls::default();
                self.text_decls(style, size, &mut d);
                d.push("color", colour(style.color.as_ref(), &self.theme.accent));
                if pad > 0 {
                    d.px("padding", pad);
                }
                if boxed {
                    self.box_decor(style, &mut d, 6);
                }
                if let Some(w) = style.width {
                    d.px("width", w.min(8192));
                }
                if let Some(h) = style.height {
                    d.px("height", h.min(8192));
                }
                let class = if boxed { "cw-link cw-boxed" } else { "cw-link" };
                let _ = writeln!(
                    self.html,
                    "<a class=\"{class}\" href=\"{}\"{}>{}</a></div>",
                    esc_attr(url),
                    self.attrs(id, &d, style.lang.as_deref()),
                    esc_text(text)
                );
            }
            PageElement::Button {
                id,
                text,
                action,
                style,
            } => {
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(12).clamp(6, 96);
                let pad = control_pad(style, true);
                let vpad = style.padding.map_or(9, |p| p.min(64) * 2 / 3);
                let label = submit_label(id, text);
                let _ = write!(
                    self.html,
                    "<form class=\"cw-line cw-action\" action=\"{}\" method=\"{}\"{}>",
                    esc_attr(&action.url),
                    esc_attr(&action.method.to_ascii_lowercase()),
                    {
                        let mut d = Decls::default();
                        match style.align.as_deref() {
                            Some("center") => {
                                d.push("text-align", "center");
                            }
                            Some("right") | Some("end") => {
                                d.push("text-align", "right");
                            }
                            _ => {}
                        }
                        d.px("margin-bottom", GAP);
                        d.attr()
                    }
                );
                self.hidden_fields(action);
                let mut d = Decls::default();
                d.px("font-size", u32::from(size))
                    .px("line-height", line_height(size));
                if style.weight.as_deref() == Some("regular") {
                    d.push("font-weight", "normal");
                }
                if style.italic == Some(true) {
                    d.push("font-style", "italic");
                }
                if style.mono == Some(true) {
                    d.push("font-family", MONO);
                }
                d.push("padding", format!("{vpad}px {pad}px"));
                d.push("color", colour(style.color.as_ref(), "#ffffff"));
                d.push(
                    "background-color",
                    colour(style.background.as_ref(), &self.theme.accent),
                );
                if let Some(edge) = style
                    .border
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("border", format!("1px solid {edge}"));
                }
                d.px("border-radius", style.radius.unwrap_or(6).min(64));
                if let Some(w) = style.width {
                    d.px("width", w.min(8192));
                }
                if let Some(h) = style.height {
                    d.px("height", h.min(8192));
                }
                let _ = writeln!(
                    self.html,
                    "<button type=\"submit\" class=\"cw-button\"{}>{}</button></form>",
                    self.attrs(id, &d, style.lang.as_deref()),
                    esc_text(label)
                );
            }
            PageElement::Input {
                id,
                label,
                value,
                placeholder,
            } => {
                let multiline = id.ends_with("-body") || label == "Content" || label == "Message";
                let _ = write!(
                    self.html,
                    "<div class=\"cw-field\"><label class=\"cw-label\" for=\"{}\">{}</label>",
                    esc_attr(id),
                    esc_text(label)
                );
                if multiline {
                    let _ = write!(
                        self.html,
                        "<textarea class=\"cw-input cw-textarea\" id=\"{}\" name=\"{}\" placeholder=\"{}\">{}</textarea>",
                        esc_attr(id),
                        esc_attr(id),
                        esc_attr(placeholder),
                        esc_text(value)
                    );
                } else {
                    let _ = write!(
                        self.html,
                        "<input class=\"cw-input\" type=\"text\" id=\"{}\" name=\"{}\" value=\"{}\" placeholder=\"{}\">",
                        esc_attr(id),
                        esc_attr(id),
                        esc_attr(value),
                        esc_attr(placeholder)
                    );
                }
                self.html.push_str("</div>\n");
            }
            PageElement::Form {
                id,
                action,
                children,
            } => {
                let has_inputs = children
                    .iter()
                    .any(|c| matches!(c, PageElement::Input { .. }));
                let open = format!(
                    "action=\"{}\" method=\"{}\"",
                    esc_attr(&action.url),
                    esc_attr(&action.method.to_ascii_lowercase())
                );
                if !has_inputs {
                    let _ = write!(
                        self.html,
                        "<form class=\"cw-form-bare\" id=\"{}\" {open}>",
                        esc_attr(id)
                    );
                    self.hidden_fields(action);
                    self.children(children);
                    self.html.push_str("</form>\n");
                } else {
                    let title = form_purpose(id).0;
                    let _ = write!(
                        self.html,
                        "<form class=\"cw-form\" id=\"{}\" {open}>",
                        esc_attr(id)
                    );
                    self.hidden_fields(action);
                    if let Some(t) = title {
                        let _ = write!(
                            self.html,
                            "<div class=\"cw-form-title\">{}</div>",
                            esc_text(t)
                        );
                    }
                    self.children(children);
                    self.html.push_str("</form>\n");
                }
            }
            PageElement::Group { id, children } => {
                let _ = writeln!(
                    self.html,
                    "<div class=\"cw-group\" id=\"{}\">",
                    esc_attr(id)
                );
                self.children(children);
                self.html.push_str("</div>\n");
            }
            PageElement::Image {
                id,
                source,
                alt,
                width,
                height,
                style,
                action,
            } => {
                let style = style.as_ref().unwrap_or(&plain);
                self.line_wrapper(style, GAP);
                let close = self.open_action(action.as_ref(), false);
                let mut d = Decls::default();
                let dw = style.width.or(Some(*width)).filter(|w| *w > 0);
                if let Some(w) = dw {
                    d.px("width", w.min(8192));
                }
                if let Some(h) = style.height {
                    d.px("height", h.min(8192));
                }
                if let Some(edge) = style
                    .border
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("border", format!("1px solid {edge}"));
                }
                let radius = style.radius.unwrap_or(0).min(64);
                if radius > 0 {
                    d.px("border-radius", radius);
                }
                let _ = write!(
                    self.html,
                    "<img class=\"cw-image\" src=\"{}\" alt=\"{}\"{}{}{}>",
                    esc_attr(source),
                    esc_attr(alt),
                    if *width > 0 {
                        format!(" width=\"{width}\"")
                    } else {
                        String::new()
                    },
                    if *height > 0 {
                        format!(" height=\"{height}\"")
                    } else {
                        String::new()
                    },
                    self.attrs(id, &d, None)
                );
                let _ = writeln!(self.html, "{close}</div>");
            }
            PageElement::Row {
                id,
                children,
                gap,
                align,
                style,
            } => {
                let gap = (*gap).min(128);
                let pad = style.padding.unwrap_or(0).min(64);
                let scroll = style.scroll_x == Some(true);
                let mut d = Decls::default();
                d.push("display", "flex");
                d.push("flex-wrap", if scroll { "nowrap" } else { "wrap" });
                if scroll {
                    d.push("overflow-x", "auto");
                }
                d.px("gap", gap);
                d.push(
                    "align-items",
                    match align.as_str() {
                        "center" => "center",
                        "end" => "flex-end",
                        "stretch" => "stretch",
                        _ => "flex-start",
                    },
                );
                match style.justify.as_deref() {
                    Some("center") => {
                        d.push("justify-content", "center");
                    }
                    Some("end") => {
                        d.push("justify-content", "flex-end");
                    }
                    Some("space-between") => {
                        d.push("justify-content", "space-between");
                    }
                    _ => {}
                }
                if pad > 0 {
                    d.px("padding", pad);
                }
                self.box_decor(style, &mut d, 0);
                if let Some(w) = style.width {
                    d.push("width", format!("{}px", w.min(8192)))
                        .push("max-width", "100%");
                }
                if let Some(h) = style.height {
                    d.px("height", h.min(8192));
                }
                let _ = writeln!(
                    self.html,
                    "<div class=\"cw-row\"{}>",
                    self.attrs(id, &d, None)
                );
                // Once any child flexes explicitly, or the row justifies, the others are
                // chips at their own width; without either every child shares the width.
                let chips = style.justify.is_some() || children.iter().any(explicit_flex);
                for c in children {
                    let mut cell = Decls::default();
                    if let Some(w) = fixed_width(c) {
                        cell.push("flex", format!("0 0 {}px", w.min(8192)));
                        cell.push("max-width", "100%");
                    } else if scroll || (chips && !explicit_flex(c)) {
                        cell.push("flex", "0 0 auto");
                    } else {
                        let f = style_of(c).and_then(|s| s.flex).unwrap_or(1).max(1);
                        cell.push("flex", format!("{f} 1 0%"));
                    }
                    let _ = write!(self.html, "<div class=\"cw-cell\"{}>", cell.attr());
                    self.depth += 1;
                    self.element(c);
                    self.depth -= 1;
                    self.html.push_str("</div>\n");
                }
                self.html.push_str("</div>\n");
            }
            PageElement::Grid {
                id,
                columns,
                children,
                gap,
                style,
            } => {
                let columns = (*columns).clamp(1, 12);
                let gap = (*gap).min(128);
                let pad = style.padding.unwrap_or(0).min(64);
                let mut d = Decls::default();
                d.push("display", "grid");
                d.push(
                    "grid-template-columns",
                    format!("repeat({columns}, minmax(0, 1fr))"),
                );
                d.px("gap", gap);
                if pad > 0 {
                    d.px("padding", pad);
                }
                self.box_decor(style, &mut d, 0);
                if let Some(w) = style.width {
                    d.push("width", format!("{}px", w.min(8192)))
                        .push("max-width", "100%");
                }
                if let Some(h) = style.height {
                    d.px("height", h.min(8192));
                }
                let _ = writeln!(
                    self.html,
                    "<div class=\"cw-grid\"{}>",
                    self.attrs(id, &d, None)
                );
                // The renderer drops columns when cells of content blocks would be
                // narrower than a phone column (150 px below a 600 px viewport). As a
                // media-query approximation: n columns need 150n + gap(n-1) + 2*gutter
                // px of viewport; below that, one fewer.
                let blocks = children.iter().any(|c| match c {
                    PageElement::Card { children, .. }
                    | PageElement::Group { children, .. }
                    | PageElement::Row { children, .. }
                    | PageElement::Grid { children, .. } => children.len() > 1 && prose(children),
                    PageElement::Thumbnail { style, .. } => style.height.unwrap_or(0) >= 60,
                    _ => false,
                });
                if blocks && columns > 1 {
                    for n in (1..columns).rev() {
                        let needs = 150 * (n + 1) + gap * n + 2 * GUTTER;
                        if needs > 600 {
                            continue;
                        }
                        let _ = writeln!(
                            self.extra_css,
                            "@media (max-width: {}px) {{ #{} {{ grid-template-columns: repeat({n}, minmax(0, 1fr)); }} }}",
                            needs - 1,
                            css_ident(id)
                        );
                    }
                }
                self.children(children);
                self.html.push_str("</div>\n");
            }
            PageElement::Card {
                id,
                children,
                style,
                action,
            } => {
                let pad = style.padding.unwrap_or(14).min(64);
                let mut d = Decls::default();
                d.px("padding", pad);
                d.push(
                    "background-color",
                    colour(style.background.as_ref(), &self.theme.surface),
                );
                if let Some(edge) = style
                    .border
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("border", format!("1px solid {edge}"));
                }
                d.px("border-radius", style.radius.unwrap_or(10).min(64));
                if let Some(w) = style.width {
                    d.push("width", format!("{}px", w.min(8192)))
                        .push("max-width", "100%");
                }
                if let Some(h) = style.height {
                    d.push("height", format!("{}px", h.min(8192)))
                        .push("box-sizing", "border-box");
                }
                match action {
                    Some(a) if a.method.eq_ignore_ascii_case("GET") && a.fields.is_empty() => {
                        let _ = writeln!(
                            self.html,
                            "<a class=\"cw-card cw-target-block\" href=\"{}\"{}>",
                            esc_attr(&a.url),
                            self.attrs(id, &d, None)
                        );
                        self.children(children);
                        self.html.push_str("</a>\n");
                    }
                    Some(a) => {
                        let _ = write!(self.html, "<form class=\"cw-action cw-action-block\" action=\"{}\" method=\"{}\">", esc_attr(&a.url), esc_attr(&a.method.to_ascii_lowercase()));
                        self.hidden_fields(a);
                        let _ = writeln!(
                            self.html,
                            "<button type=\"submit\" class=\"cw-card cw-target-block\"{}>",
                            self.attrs(id, &d, None)
                        );
                        self.children(children);
                        self.html.push_str("</button></form>\n");
                    }
                    None => {
                        let _ = writeln!(
                            self.html,
                            "<div class=\"cw-card\"{}>",
                            self.attrs(id, &d, None)
                        );
                        self.children(children);
                        self.html.push_str("</div>\n");
                    }
                }
            }
            PageElement::Styled { id, text, style } => {
                let size = style.size.unwrap_or(13).clamp(6, 96);
                let pad = style.padding.unwrap_or(0).min(64);
                let mut d = Decls::default();
                self.text_decls(style, size, &mut d);
                d.push("color", self.ink_of(style));
                if pad > 0 {
                    d.px("padding", pad);
                }
                self.box_decor(style, &mut d, 0);
                if let Some(w) = style.width {
                    d.push("width", format!("{}px", w.min(8192)))
                        .push("max-width", "100%")
                        .push("box-sizing", "border-box");
                }
                if let Some(h) = style.height {
                    d.push("height", format!("{}px", h.min(8192)))
                        .push("box-sizing", "border-box");
                }
                match style.align.as_deref() {
                    Some("center") => {
                        d.push("text-align", "center");
                    }
                    Some("right") | Some("end") => {
                        d.push("text-align", "right");
                    }
                    _ => {}
                }
                let _ = writeln!(
                    self.html,
                    "<div class=\"cw-styled\"{}>{}</div>",
                    self.attrs(id, &d, style.lang.as_deref()),
                    esc_text(text)
                );
            }
            PageElement::Thumbnail {
                id,
                label,
                style,
                action,
            } => {
                let box_h = style.height.unwrap_or(120).min(8192);
                let size = style.size.unwrap_or(12).clamp(6, 96);
                self.line_wrapper(&plain, GAP);
                let close = self.open_action(action.as_ref(), true);
                let mut d = Decls::default();
                d.px("height", box_h).px("line-height", box_h);
                if let Some(w) = style.width {
                    d.push("width", format!("{}px", w.min(8192)))
                        .push("max-width", "100%");
                }
                d.push(
                    "background-color",
                    colour(style.background.as_ref(), tint(label)),
                );
                if let Some(edge) = style
                    .border
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("border", format!("1px solid {edge}"));
                }
                d.px("border-radius", style.radius.unwrap_or(8).min(64));
                d.px("font-size", u32::from(size))
                    .push("color", self.ink_of(style));
                let shown = if line_height(size) <= box_h {
                    esc_text(label)
                } else {
                    String::new()
                };
                let _ = writeln!(
                    self.html,
                    "<div class=\"cw-thumb\" role=\"img\" aria-label=\"{}\"{}>{}</div>{close}</div>",
                    esc_attr(label),
                    self.attrs(id, &d, style.lang.as_deref()),
                    shown
                );
            }
            PageElement::Badge { id, text, style } => {
                let size = style.size.unwrap_or(10).clamp(6, 96);
                let pad = style.padding.unwrap_or(0).min(64);
                let bh = style
                    .height
                    .unwrap_or((line_height(size) + 2 * pad).max(20))
                    .min(8192);
                self.line_wrapper(style, 6);
                let mut d = Decls::default();
                d.px("font-size", u32::from(size))
                    .px("height", bh)
                    .px("line-height", bh);
                d.push("padding", format!("0 {}px", pad.max(9)));
                if let Some(w) = style.width {
                    d.push("width", format!("{}px", w.min(8192)))
                        .push("box-sizing", "border-box");
                }
                let fill = match &style.background {
                    Some(c) if cw_protocol::valid_color(c) => c.clone(),
                    _ if style.border.is_some() || style.color.is_some() => "transparent".into(),
                    _ => self.theme.accent.clone(),
                };
                d.push("background-color", fill);
                d.push("color", colour(style.color.as_ref(), "#ffffff"));
                if let Some(edge) = style
                    .border
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("border", format!("1px solid {edge}"));
                }
                d.px("border-radius", style.radius.unwrap_or(bh / 2).min(64));
                if text.is_empty() {
                    d.push("visibility", "hidden");
                }
                let _ = writeln!(
                    self.html,
                    "<span class=\"cw-badge\"{}>{}</span></div>",
                    self.attrs(id, &d, style.lang.as_deref()),
                    esc_text(text)
                );
            }
            PageElement::Divider { id, style } => {
                let mut d = Decls::default();
                d.px("height", style.height.unwrap_or(1).clamp(1, 64));
                d.push(
                    "background-color",
                    colour(style.color.as_ref(), &self.theme.border),
                );
                let _ = writeln!(
                    self.html,
                    "<hr class=\"cw-divider\"{}>",
                    self.attrs(id, &d, None)
                );
            }
            PageElement::Icon {
                id,
                name,
                label,
                style,
                action,
            } => {
                let size = u32::from(style.size.unwrap_or(20).clamp(6, 96));
                let pad = style.padding.unwrap_or(0).min(64);
                let bw = style.width.unwrap_or(size + 2 * pad).min(8192);
                let bh = style.height.unwrap_or(size + 2 * pad).min(8192);
                self.line_wrapper(style, 6);
                let close = self.open_action(action.as_ref(), false);
                let mut d = Decls::default();
                d.px("width", bw).px("height", bh).px("line-height", bh);
                d.push("color", self.ink_of(style));
                if let Some(bg) = style
                    .background
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("background-color", bg);
                }
                if let Some(edge) = style
                    .border
                    .as_ref()
                    .filter(|c| cw_protocol::valid_color(c))
                {
                    d.push("border", format!("1px solid {edge}"));
                }
                d.px(
                    "border-radius",
                    style.radius.unwrap_or(bw.min(bh) / 2).min(64),
                );
                let mut g = Decls::default();
                g.px("width", size)
                    .px("height", size)
                    .px("line-height", size);
                let _ = writeln!(
                    self.html,
                    "<span class=\"cw-icon\" role=\"img\" aria-label=\"{}\" title=\"{}\"{}><span class=\"cw-glyph\" data-symbol=\"{}\"{}>{}</span></span>{close}</div>",
                    esc_attr(label),
                    esc_attr(label),
                    self.attrs(id, &d, None),
                    esc_attr(name),
                    g.attr(),
                    esc_text(name)
                );
            }
            PageElement::Spacer { id, height } => {
                let mut d = Decls::default();
                d.px("height", (*height).min(8192));
                let _ = writeln!(
                    self.html,
                    "<div class=\"cw-spacer\"{}></div>",
                    self.attrs(id, &d, None)
                );
            }
        }
    }
}

/// An id as a CSS identifier for a selector: characters outside `[A-Za-z0-9_-]` are
/// escaped the way `CSS.escape` does.
fn css_ident(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for (i, c) in id.chars().enumerate() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            if i == 0 && c.is_ascii_digit() {
                let _ = write!(out, "\\{:x} ", c as u32);
            } else {
                out.push(c);
            }
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

fn theme_of(page: &Page) -> Theme {
    let t = page.theme.as_ref();
    let pick = |f: fn(&cw_protocol::PageTheme) -> Option<&String>, fallback: &str| {
        colour(t.and_then(f), fallback)
    };
    let accent_fallback = match page.title.as_str() {
        "Chat" => "#592f70",
        "Calendar" => "#1f70c9",
        "Documents" => "#218464",
        _ => "#2270cd",
    };
    let ink = pick(|t| t.ink.as_ref(), "#252b36");
    let surface = pick(|t| t.surface.as_ref(), "#ffffff");
    let themed = t.is_some();
    Theme {
        face: match t
            .and_then(|t| t.font.as_deref())
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            Some(f) => format!("{f}, {FACE}"),
            None => FACE.to_owned(),
        },
        accent: pick(|t| t.accent.as_ref(), accent_fallback),
        muted: pick(|t| t.muted.as_ref(), "#6f7885"),
        border: if themed {
            mix(&surface, &ink, 14)
        } else {
            "#e1e5eb".into()
        },
        background: pick(|t| t.background.as_ref(), "#f8fafd"),
        content_width: t.and_then(|t| t.content_width).filter(|v| *v > 0),
        ink,
        surface,
        themed,
    }
}

/// The base stylesheet with the theme's colours filled in.
fn base_css(theme: &Theme) -> String {
    let t = theme;
    format!(
        "\
html {{ background-color: {bg}; }}
body {{ margin: 0; font-family: {face}; font-size: 13px; line-height: 17px; color: {ink}; }}
.cw-app-bar {{ height: 63px; line-height: 63px; padding-left: 62px; background-color: #ffffff; border-bottom: 1px solid #e1e5eb; font-size: 20px; font-weight: bold; color: #252b36; overflow-x: hidden; overflow-y: hidden; white-space: nowrap; }}
.cw-page {{ padding: {gutter}px; }}
.cw-page-app {{ padding: 24px; }}
.cw-content {{ margin: 0 auto; }}
.cw-pin-top {{ position: sticky; top: 0; z-index: 10; }}
.cw-pin-bottom {{ position: fixed; bottom: 0; left: 0; right: 0; z-index: 10; }}
.cw-title {{ font-size: 20px; line-height: 28px; height: 28px; overflow-x: hidden; overflow-y: hidden; font-weight: bold; margin: 0 0 12px 0; }}
.cw-heading {{ font-weight: bold; line-height: 30px; height: 30px; overflow-x: hidden; overflow-y: hidden; margin: 0 0 8px 0; }}
.cw-text {{ margin: 0 0 21px 0; }}
.cw-line {{ margin: 0; }}
.cw-link {{ display: inline-block; max-width: 100%; box-sizing: border-box; text-decoration-line: none; vertical-align: top; cursor: pointer; }}
.cw-button {{ display: inline-block; max-width: 100%; box-sizing: border-box; font-family: {face}; font-weight: bold; border: 0; white-space: nowrap; overflow-x: hidden; overflow-y: hidden; text-overflow: ellipsis; vertical-align: top; text-align: center; cursor: pointer; appearance: none; }}
.cw-action {{ display: block; margin: 0; }}
.cw-field {{ margin: 0 0 13px 0; }}
.cw-label {{ display: block; font-size: 11px; line-height: 18px; height: 21px; color: {muted}; }}
.cw-input {{ display: block; width: 100%; box-sizing: border-box; height: 36px; padding: 0 10px; font-family: {face}; font-size: 13px; line-height: 34px; color: {ink}; background-color: #fdfeff; border: 1px solid #cfd7e2; border-radius: 5px; appearance: none; outline-style: none; }}
.cw-textarea {{ height: 82px; padding: 9px 10px; line-height: 17px; }}
.cw-form {{ display: block; margin: 0; padding: 16px 16px 12px 16px; background-color: {surface}; border: 1px solid {border}; border-radius: 10px; }}
.cw-form-title {{ font-size: 14px; font-weight: bold; line-height: 24px; height: 24px; margin: 0 0 8px 0; overflow-x: hidden; overflow-y: hidden; }}
.cw-form-bare {{ display: block; margin: 0; }}
.cw-image {{ display: inline-block; max-width: 100%; vertical-align: top; }}
.cw-target {{ display: inline-block; padding: 0; margin: 0; border: 0; background-color: transparent; color: inherit; font-family: {face}; text-decoration-line: none; cursor: pointer; }}
.cw-target-block {{ display: block; width: 100%; text-align: left; }}
.cw-action-block {{ display: block; margin: 0; }}
.cw-row {{ box-sizing: border-box; }}
.cw-cell {{ min-width: 0; }}
.cw-grid {{ box-sizing: border-box; }}
.cw-card {{ display: block; box-sizing: border-box; margin: 0 0 {gap}px 0; color: inherit; font-family: {face}; font-size: 13px; line-height: 17px; text-decoration-line: none; text-align: left; }}
.cw-card > :last-child {{ margin-bottom: 0; }}
.cw-styled {{ margin: 0 0 6px 0; }}
.cw-thumb {{ box-sizing: border-box; padding: 0 8px; font-weight: bold; text-align: center; white-space: nowrap; overflow-x: hidden; overflow-y: hidden; text-overflow: ellipsis; }}
.cw-badge {{ display: inline-block; box-sizing: border-box; font-weight: bold; white-space: nowrap; vertical-align: top; text-align: center; }}
.cw-divider {{ display: block; border: 0; margin: 8px 0 8px 0; }}
.cw-icon {{ display: inline-block; box-sizing: border-box; text-align: center; vertical-align: top; overflow-x: hidden; overflow-y: hidden; }}
.cw-glyph {{ display: inline-block; font-size: 8px; overflow-x: hidden; overflow-y: hidden; white-space: nowrap; vertical-align: top; }}
.cw-spacer {{ display: block; }}
",
        bg = t.background,
        face = t.face,
        ink = t.ink,
        muted = t.muted,
        surface = t.surface,
        border = t.border,
        gutter = GUTTER,
        gap = GAP,
    )
}

/// Converts a page to an HTML document (without its stylesheet) and the stylesheet.
/// Element ids, form actions and methods, image sources and the page's language are
/// preserved; see the module documentation for what the CSS reproduces.
pub fn to_html(page: &Page) -> (String, String) {
    let theme = theme_of(page);
    let special = !theme.themed
        && matches!(
            page.title.as_str(),
            "Mail" | "Chat" | "Calendar" | "Documents"
        );
    let mut em = Emitter {
        css: base_css(&theme),
        extra_css: String::new(),
        html: String::new(),
        theme,
        depth: 0,
    };
    let pin_of = |e: &PageElement| style_of(e).and_then(|s| s.pin.clone());
    let lang = page
        .lang
        .as_deref()
        .map(|l| format!(" lang=\"{}\"", esc_attr(l)))
        .unwrap_or_default();
    let _ = writeln!(em.html, "<!DOCTYPE html>\n<html{lang}>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n</head>\n<body>", esc_text(&page.title));
    if special {
        let _ = writeln!(
            em.html,
            "<div class=\"cw-app-bar\">{}</div>",
            esc_text(&page.title)
        );
    }
    let tops: Vec<&PageElement> = page
        .elements
        .iter()
        .filter(|e| pin_of(e).as_deref() == Some("top"))
        .collect();
    if !tops.is_empty() {
        em.html.push_str("<div class=\"cw-pin-top\">\n");
        for e in tops {
            em.element(e);
        }
        em.html.push_str("</div>\n");
    }
    let _ = writeln!(
        em.html,
        "<div class=\"{}\">",
        if special {
            "cw-page cw-page-app"
        } else {
            "cw-page"
        }
    );
    let mut content = Decls::default();
    if let Some(cw) = em.theme.content_width {
        content.px("max-width", cw);
    }
    let _ = writeln!(em.html, "<div class=\"cw-content\"{}>", content.attr());
    // An untitled page draws its title itself; a heading that repeats it is not drawn
    // twice, but its id stays on the title so the heading is still addressable.
    let title_heading = page.elements.iter().find_map(|e| match e {
        PageElement::Heading { id, text, .. } if !em.theme.themed && text == &page.title => {
            Some(id.as_str())
        }
        _ => None,
    });
    if !em.theme.themed && !special {
        let id = title_heading
            .map(|id| format!(" id=\"{}\"", esc_attr(id)))
            .unwrap_or_default();
        let _ = writeln!(
            em.html,
            "<h1 class=\"cw-title\"{id}>{}</h1>",
            esc_text(&page.title)
        );
    }
    for e in &page.elements {
        if pin_of(e).is_some() {
            continue;
        }
        if matches!(e, PageElement::Heading { id, .. } if Some(id.as_str()) == title_heading) {
            continue;
        }
        em.element(e);
    }
    em.html.push_str("</div>\n</div>\n");
    let bottoms: Vec<&PageElement> = page
        .elements
        .iter()
        .filter(|e| pin_of(e).as_deref() == Some("bottom"))
        .collect();
    if !bottoms.is_empty() {
        em.html.push_str("<div class=\"cw-pin-bottom\">\n");
        for e in bottoms {
            em.element(e);
        }
        em.html.push_str("</div>\n");
    }
    em.html.push_str("</body>\n</html>\n");
    let mut css = em.css;
    css.push_str(&em.extra_css);
    (em.html, css)
}

/// The HTML with its stylesheet inlined in `<head>`: what the engine parses.
pub fn to_document(page: &Page) -> String {
    let (html, css) = to_html(page);
    html.replacen("</head>", &format!("<style>\n{css}</style>\n</head>"), 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_protocol::PageTheme;
    use std::collections::{BTreeMap, BTreeSet};

    const VOID: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source",
        "track", "wbr",
    ];

    /// A small well-formedness check: tags balance, attribute values are quoted and
    /// hold no raw `<` or `"`, text holds no raw `<`.
    fn check_well_formed(html: &str) -> Result<Vec<(String, String)>, String> {
        let mut stack: Vec<String> = Vec::new();
        let mut attrs = Vec::new();
        let mut rest = html;
        while let Some(i) = rest.find('<') {
            let text = &rest[..i];
            if text.contains('>') && !text.trim().is_empty() && text.contains("<") {
                return Err(format!("raw < in text: {text:?}"));
            }
            let tag_end = rest[i..].find('>').ok_or_else(|| {
                format!(
                    "unterminated tag at {:?}",
                    &rest[i..(i + 40).min(rest.len())]
                )
            })?;
            let tag = &rest[i + 1..i + tag_end];
            rest = &rest[i + tag_end + 1..];
            if tag.starts_with('!') {
                continue;
            }
            if let Some(name) = tag.strip_prefix('/') {
                let open = stack
                    .pop()
                    .ok_or_else(|| format!("close </{name}> with nothing open"))?;
                if open != name.trim() {
                    return Err(format!("close </{name}> but <{open}> is open"));
                }
                continue;
            }
            let name: String = tag
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            let mut a = &tag[name.len()..];
            while let Some(eq) = a.find('=') {
                let key = a[..eq].trim().to_owned();
                let after = &a[eq + 1..];
                if !after.starts_with('"') {
                    return Err(format!("unquoted attribute {key} in <{tag}>"));
                }
                let close = after[1..]
                    .find('"')
                    .ok_or_else(|| format!("unterminated attribute {key}"))?;
                let value = &after[1..1 + close];
                if value.contains('<') {
                    return Err(format!("raw < in attribute {key}"));
                }
                attrs.push((key, value.to_owned()));
                a = &after[1 + close + 1..];
            }
            if !VOID.contains(&name.as_str()) {
                stack.push(name);
            }
        }
        if !stack.is_empty() {
            return Err(format!("unclosed: {stack:?}"));
        }
        Ok(attrs)
    }

    fn element_ids(elements: &[PageElement], out: &mut Vec<String>) {
        for e in elements {
            out.push(e.id().to_owned());
            match e {
                PageElement::Form { children, .. }
                | PageElement::Group { children, .. }
                | PageElement::Row { children, .. }
                | PageElement::Grid { children, .. }
                | PageElement::Card { children, .. } => element_ids(children, out),
                _ => {}
            }
        }
    }

    fn declarations(css: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut rest = css;
        while let Some(open) = rest.find('{') {
            let close = rest[open..]
                .find('}')
                .map(|c| open + c)
                .unwrap_or(rest.len());
            let body = &rest[open + 1..close];
            if body.contains('{') {
                // A media query: descend into its block.
                rest = &rest[open + 1..];
                continue;
            }
            for decl in body.split(';') {
                if let Some((k, v)) = decl.split_once(':') {
                    out.push((k.trim().to_owned(), v.trim().to_owned()));
                }
            }
            rest = &rest[close.min(rest.len() - 1) + 1..];
        }
        out
    }

    fn inline_declarations(attrs: &[(String, String)]) -> Vec<(String, String)> {
        attrs
            .iter()
            .filter(|(k, _)| k == "style")
            .flat_map(|(_, v)| {
                v.split(';')
                    .filter_map(|d| {
                        d.split_once(':')
                            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn check_page(page: &Page) -> (String, String) {
        let (html, css) = to_html(page);
        let attrs =
            check_well_formed(&html).unwrap_or_else(|e| panic!("{}: {e}\n{html}", page.title));
        let mut wanted = Vec::new();
        element_ids(&page.elements, &mut wanted);
        let ids: Vec<&str> = attrs
            .iter()
            .filter(|(k, _)| k == "id")
            .map(|(_, v)| v.as_str())
            .collect();
        let unique: BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "{}: duplicate ids in output",
            page.title
        );
        for id in &wanted {
            assert!(
                unique.contains(esc_attr(id).as_str()),
                "{}: element id {id:?} lost",
                page.title
            );
        }
        for (k, v) in declarations(&css)
            .into_iter()
            .chain(inline_declarations(&attrs))
        {
            assert!(
                SUPPORTED_PROPERTIES.contains(&k.as_str()),
                "{}: unsupported property {k}: {v}",
                page.title
            );
            assert!(!v.is_empty(), "{}: empty value for {k}", page.title);
        }
        (html, css)
    }

    fn fixture_pages() -> Vec<Page> {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../worlds/company-2026");
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(root.join("sites"))
            .expect("worlds/company-2026/sites")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.push(root.join("world.json"));
        files.sort();
        fn walk(v: &serde_json::Value, out: &mut Vec<Page>) {
            match v {
                serde_json::Value::Object(m) => {
                    if m.contains_key("elements") && m.contains_key("title") {
                        if let Ok(p) = serde_json::from_value::<Page>(v.clone()) {
                            out.push(p);
                            return;
                        }
                    }
                    for x in m.values() {
                        walk(x, out);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                _ => {}
            }
        }
        let mut pages = Vec::new();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            let v: serde_json::Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            walk(&v, &mut pages);
        }
        pages
    }

    #[test]
    fn every_fixture_page_converts_to_well_formed_html_with_supported_css() {
        let pages = fixture_pages();
        assert!(
            pages.len() >= 300,
            "expected the site fixtures to hold hundreds of pages, found {}",
            pages.len()
        );
        let mut kinds = BTreeSet::new();
        for page in &pages {
            let (html, _) = check_page(page);
            assert!(html.starts_with("<!DOCTYPE html>"));
            fn note(elements: &[PageElement], kinds: &mut BTreeSet<&'static str>) {
                for e in elements {
                    let name = match e {
                        PageElement::Heading { .. } => "heading",
                        PageElement::Text { .. } => "text",
                        PageElement::Link { .. } => "link",
                        PageElement::Button { .. } => "button",
                        PageElement::Input { .. } => "input",
                        PageElement::Form { .. } => "form",
                        PageElement::Group { .. } => "group",
                        PageElement::Image { .. } => "image",
                        PageElement::Row { .. } => "row",
                        PageElement::Grid { .. } => "grid",
                        PageElement::Card { .. } => "card",
                        PageElement::Styled { .. } => "styled",
                        PageElement::Thumbnail { .. } => "thumbnail",
                        PageElement::Badge { .. } => "badge",
                        PageElement::Divider { .. } => "divider",
                        PageElement::Icon { .. } => "icon",
                        PageElement::Spacer { .. } => "spacer",
                    };
                    kinds.insert(name);
                    match e {
                        PageElement::Form { children, .. }
                        | PageElement::Group { children, .. }
                        | PageElement::Row { children, .. }
                        | PageElement::Grid { children, .. }
                        | PageElement::Card { children, .. } => note(children, kinds),
                        _ => {}
                    }
                }
            }
            note(&page.elements, &mut kinds);
        }
        // The fixtures exercise most of the vocabulary; the synthetic page below covers
        // the rest.
        assert!(kinds.len() >= 10, "fixtures cover only {kinds:?}");
    }

    #[test]
    fn every_fixture_page_parses_back_with_its_ids() {
        for page in fixture_pages() {
            let doc = crate::html::parse(&to_document(&page));
            let mut wanted = Vec::new();
            element_ids(&page.elements, &mut wanted);
            for id in wanted {
                assert!(
                    !doc.by_id(&id).is_empty(),
                    "{}: id {id:?} not found after parsing",
                    page.title
                );
            }
            assert!(doc.body().is_some());
        }
    }

    fn sample_page() -> Page {
        let get = |url: &str| PageAction {
            method: "GET".into(),
            url: url.into(),
            fields: BTreeMap::new(),
        };
        let post = |url: &str| PageAction {
            method: "POST".into(),
            url: url.into(),
            fields: BTreeMap::from([("kind".to_owned(), "like".to_owned())]),
        };
        Page {
            version: 1,
            title: "Sample".into(),
            lang: Some("en".into()),
            theme: Some(PageTheme { accent: Some("#ff6600".into()), background: Some("#f6f6ef".into()), surface: None, ink: Some("#1f1f1f".into()), muted: None, content_width: Some(980), font: Some("Verdana, Geneva, sans-serif".into()) }),
            elements: vec![
                PageElement::Row {
                    id: "nav".into(),
                    gap: 8,
                    align: "center".into(),
                    style: Style::default().pin("top").background("#ffffff").justify("space-between"),
                    children: vec![
                        PageElement::Link { id: "home".into(), text: "Home".into(), url: "/".into(), style: Some(Style::default().background("#eeeeee").radius(4)) },
                        PageElement::Spacer { id: "push".into(), height: 0 },
                        PageElement::Icon { id: "bell".into(), name: "bell".into(), label: "Notifications".into(), style: Style::default(), action: Some(get("/notifications")) },
                    ],
                },
                PageElement::Heading { id: "h".into(), text: "Sample".into(), level: 1 },
                PageElement::Text { id: "t".into(), text: "A paragraph of text that says <nothing> & wraps.".into() },
                PageElement::Grid {
                    id: "cards".into(),
                    columns: 3,
                    gap: 12,
                    style: Style::default(),
                    children: vec![
                        PageElement::Card {
                            id: "c1".into(),
                            style: Style::default(),
                            action: Some(get("/c1")),
                            children: vec![
                                PageElement::Thumbnail { id: "th1".into(), label: "Cover".into(), style: Style::default().height(120), action: None },
                                PageElement::Styled { id: "s1".into(), text: "A styled line that is long enough to count as prose for the grid".into(), style: Style::default().size(15).bold().align("center") },
                                PageElement::Badge { id: "b1".into(), text: "New".into(), style: Style::default() },
                            ],
                        },
                        PageElement::Card { id: "c2".into(), style: Style::default().padding(20), action: Some(post("/like")), children: vec![PageElement::Text { id: "t2".into(), text: "Card two has a paragraph long enough to be prose too.".into() }] },
                        PageElement::Card { id: "c3".into(), style: Style::default(), action: None, children: vec![PageElement::Image { id: "img".into(), source: "/img/a.rgba".into(), alt: "A picture".into(), width: 200, height: 100, style: None, action: Some(get("/img")) }] },
                    ],
                },
                PageElement::Row {
                    id: "shelf".into(),
                    gap: 8,
                    align: String::new(),
                    style: Style::default().scroll_x(),
                    children: vec![PageElement::Thumbnail { id: "a1".into(), label: "One".into(), style: Style::default().width(120), action: None }, PageElement::Thumbnail { id: "a2".into(), label: "Two".into(), style: Style::default().width(120), action: None }],
                },
                PageElement::Form {
                    id: "comment-form".into(),
                    action: post("/comment"),
                    children: vec![
                        PageElement::Input { id: "comment-body".into(), label: "Message".into(), value: String::new(), placeholder: "Say something".into() },
                        PageElement::Input { id: "name".into(), label: "Name".into(), value: "alice".into(), placeholder: String::new() },
                        PageElement::Button { id: "comment-submit".into(), text: "Submit".into(), action: post("/comment"), style: None },
                    ],
                },
                PageElement::Divider { id: "d".into(), style: Style::default() },
                PageElement::Group { id: "g".into(), children: vec![PageElement::Button { id: "buy".into(), text: "Buy now".into(), action: post("/buy"), style: Some(Style::default().align("center").color("#000000").background("#ffcc00")) }] },
                PageElement::Spacer { id: "sp".into(), height: 24 },
            ],
        }
    }

    #[test]
    fn sample_page_reproduces_the_page_scene_metrics() {
        let page = sample_page();
        page.validate().unwrap();
        let (html, css) = check_page(&page);
        // The gutter and the centred column.
        assert!(css.contains(".cw-page { padding: 16px; }"));
        assert!(html.contains("<div class=\"cw-content\" style=\"max-width: 980px\">"));
        assert!(css.contains(".cw-content { margin: 0 auto; }"));
        // Theme colours.
        assert!(css.contains("html { background-color: #f6f6ef; }"));
        assert!(css.contains("color: #1f1f1f;"));
        assert!(css.contains("body { margin: 0; font-family: Verdana, Geneva, sans-serif, 'DejaVu Sans', sans-serif; font-size: 13px; line-height: 17px; color: #1f1f1f; }"));
        // A themed page draws no title, and the heading equal to the title stays.
        assert!(!html.contains("cw-title"));
        assert!(html
            .contains("<h1 class=\"cw-heading\" id=\"h\" style=\"font-size: 18px\">Sample</h1>"));
        assert!(css.contains(".cw-heading { font-weight: bold; line-height: 30px; height: 30px;"));
        // Text escapes and keeps its paragraph metrics.
        assert!(html.contains("<p class=\"cw-text\" id=\"t\">A paragraph of text that says &lt;nothing&gt; &amp; wraps.</p>"));
        assert!(css.contains(".cw-text { margin: 0 0 21px 0; }"));
        // The pinned row is sticky, flex, space-between, its link a pill in the accent.
        assert!(html.contains("<div class=\"cw-pin-top\">"));
        assert!(css.contains(".cw-pin-top { position: sticky; top: 0;"));
        assert!(html.contains("display: flex; flex-wrap: wrap; gap: 8px; align-items: center; justify-content: space-between; background-color: #ffffff"));
        assert!(html.contains("<a class=\"cw-link cw-boxed\" href=\"/\" id=\"home\" style=\"font-size: 13px; line-height: 17px; color: #ff6600; padding: 8px; background-color: #eeeeee; border-radius: 4px\">Home</a>"));
        assert!(html.contains("<div class=\"cw-cell\" style=\"flex: 0 0 auto\">"));
        // The icon is a labelled box holding the symbol name, inside a GET link.
        assert!(html.contains("<a class=\"cw-target\" href=\"/notifications\"><span class=\"cw-icon\" role=\"img\" aria-label=\"Notifications\" title=\"Notifications\" id=\"bell\" style=\"width: 20px; height: 20px; line-height: 20px; color: #1f1f1f; border-radius: 10px\"><span class=\"cw-glyph\" data-symbol=\"bell\" style=\"width: 20px; height: 20px; line-height: 20px\">bell</span></span></a>"));
        // The grid: three equal columns, and the drop rule as media queries.
        assert!(html.contains(
            "display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px"
        ));
        assert!(css.contains("@media (max-width: 343px) { #cards { grid-template-columns: repeat(1, minmax(0, 1fr)); } }"));
        assert!(css.contains("@media (max-width: 505px) { #cards { grid-template-columns: repeat(2, minmax(0, 1fr)); } }"));
        // Cards: default padding 14 and radius 10; a GET card is a link, a POST card a button.
        assert!(html.contains("<a class=\"cw-card cw-target-block\" href=\"/c1\" id=\"c1\" style=\"padding: 14px; background-color: #ffffff; border-radius: 10px\">"));
        assert!(html.contains("<form class=\"cw-action cw-action-block\" action=\"/like\" method=\"post\"><input type=\"hidden\" name=\"kind\" value=\"like\"><button type=\"submit\" class=\"cw-card cw-target-block\" id=\"c2\" style=\"padding: 20px;"));
        // Thumbnail, styled and badge.
        assert!(html.contains("<div class=\"cw-thumb\" role=\"img\" aria-label=\"Cover\" id=\"th1\" style=\"height: 120px; line-height: 120px; background-color: #"));
        assert!(html.contains("id=\"s1\" style=\"font-size: 15px; line-height: 20px; font-weight: bold; color: #1f1f1f; text-align: center\""));
        assert!(html.contains("<span class=\"cw-badge\" id=\"b1\" style=\"font-size: 10px; height: 20px; line-height: 20px; padding: 0 9px; background-color: #ff6600; color: #ffffff; border-radius: 10px\">New</span>"));
        // The image keeps its source and sits in its own link.
        assert!(html.contains("<a class=\"cw-target\" href=\"/img\"><img class=\"cw-image\" src=\"/img/a.rgba\" alt=\"A picture\" width=\"200\" height=\"100\" id=\"img\" style=\"width: 200px\"></a>"));
        // A sideways shelf.
        assert!(html.contains("id=\"shelf\" style=\"display: flex; flex-wrap: nowrap; overflow-x: auto; gap: 8px; align-items: flex-start\""));
        assert!(html.contains("<div class=\"cw-cell\" style=\"flex: 0 0 120px; max-width: 100%\">"));
        // The form keeps its action and method, gets its purpose title, a textarea for
        // the body, and its Submit button reads as the purpose says.
        assert!(html.contains("<form class=\"cw-form\" id=\"comment-form\" action=\"/comment\" method=\"post\"><input type=\"hidden\" name=\"kind\" value=\"like\"><div class=\"cw-form-title\">Add a comment</div>"));
        assert!(html.contains("<textarea class=\"cw-input cw-textarea\" id=\"comment-body\" name=\"comment-body\" placeholder=\"Say something\"></textarea>"));
        assert!(html.contains("<input class=\"cw-input\" type=\"text\" id=\"name\" name=\"name\" value=\"alice\" placeholder=\"\">"));
        assert!(html.contains("<button type=\"submit\" class=\"cw-button\" id=\"comment-submit\" style=\"font-size: 12px; line-height: 16px; padding: 9px 14px; color: #ffffff; background-color: #ff6600; border-radius: 6px\">Post comment</button>"));
        assert!(css.contains(
            ".cw-input { display: block; width: 100%; box-sizing: border-box; height: 36px;"
        ));
        assert!(css.contains(".cw-textarea { height: 82px;"));
        // Divider, centred button, spacer.
        assert!(html.contains(
            "<hr class=\"cw-divider\" id=\"d\" style=\"height: 1px; background-color: #"
        ));
        assert!(html.contains("<form class=\"cw-line cw-action\" action=\"/buy\" method=\"post\" style=\"text-align: center; margin-bottom: 12px\">"));
        assert!(html.contains("<div class=\"cw-spacer\" id=\"sp\" style=\"height: 24px\"></div>"));
        assert!(html.contains("<html lang=\"en\">"));
        // The document form inlines the sheet.
        let doc = to_document(&page);
        assert!(doc.contains("<style>\nhtml { background-color: #f6f6ef; }"));
    }

    #[test]
    fn untitled_plain_pages_draw_the_title_and_legacy_apps_get_the_bar() {
        let mut page = Page::new("Plain");
        page.elements.push(PageElement::Heading {
            id: "h".into(),
            text: "Plain".into(),
            level: 1,
        });
        page.elements.push(PageElement::Text {
            id: "t".into(),
            text: "body".into(),
        });
        let (html, css) = check_page(&page);
        assert!(html.contains("<h1 class=\"cw-title\" id=\"h\">Plain</h1>"));
        assert!(
            !html.contains("cw-heading"),
            "a heading equal to the title is not drawn twice"
        );
        assert!(css.contains("html { background-color: #f8fafd; }"));
        assert!(css.contains(".cw-title { font-size: 20px; line-height: 28px; height: 28px;"));
        let mut mail = Page::new("Mail");
        mail.elements.push(PageElement::Text {
            id: "t".into(),
            text: "inbox".into(),
        });
        let (html, _) = check_page(&mail);
        assert!(html.contains("<div class=\"cw-app-bar\">Mail</div>"));
        assert!(html.contains("<div class=\"cw-page cw-page-app\">"));
        assert!(!html.contains("cw-title"));
    }

    #[test]
    fn ids_and_text_are_escaped() {
        let mut page = Page::new("Esc");
        page.elements.push(PageElement::Link {
            id: "a\"b<c".into(),
            text: "<x> & y".into(),
            url: "/?a=1&b=\"2\"".into(),
            style: None,
        });
        let (html, _) = check_page(&page);
        assert!(html.contains("id=\"a&quot;b&lt;c\""));
        assert!(html.contains("href=\"/?a=1&amp;b=&quot;2&quot;\""));
        assert!(html.contains(">&lt;x&gt; &amp; y</a>"));
        assert_eq!(css_ident("1a b.c"), "\\31 a\\ b\\.c");
    }
}
