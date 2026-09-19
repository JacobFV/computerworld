//! Native structured-page presentation. Every control is derived from a received
//! page element; this layer never reads services, users or privileged world state.
use super::ImageAsset;
use cw_protocol::{Page, PageElement, Style};
use cw_scene::{
    metrics, Color, Lang, Node, Primitive, Rect, RoundedClip, Scene, Semantic, Style as TextStyle,
    Typeface,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const INK: Color = Color::rgb(37, 43, 54);
const MUTED: Color = Color::rgb(111, 120, 133);
const BORDER: Color = Color::rgb(225, 229, 235);
/// Vertical air below a block element; every measured height carries its own.
const GAP: u32 = 12;
/// Line breaking uses the widest bundled family so the same breaks fit whichever
/// platform typeface the shell selects.
const FACE: Typeface = Typeface::DejaVu;
fn parse_color(value: &str) -> Option<Color> {
    if !cw_protocol::valid_color(value) {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&value[i..i + 2], 16).ok();
    Some(Color(
        b(1)?,
        b(3)?,
        b(5)?,
        if value.len() == 9 { b(7)? } else { 255 },
    ))
}
fn hash(s: &str) -> u64 {
    let mut id = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        id = (id ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    id
}
/// Stand-in artwork is a flat tint chosen from the label, so it is stable per asset
/// and obviously synthetic rather than pretending to be photography.
fn tint(label: &str) -> Color {
    const PALETTE: [Color; 8] = [
        Color::rgb(203, 213, 228),
        Color::rgb(210, 222, 212),
        Color::rgb(226, 214, 206),
        Color::rgb(214, 209, 227),
        Color::rgb(205, 220, 228),
        Color::rgb(228, 216, 206),
        Color::rgb(212, 218, 206),
        Color::rgb(219, 209, 213),
    ];
    PALETTE[(hash(label) % 8) as usize]
}
/// Blends `b` into `a` by `pct`, so themed rules track the theme's own contrast.
fn mix(a: Color, b: Color, pct: u32) -> Color {
    let c = |a: u8, b: u8| ((u32::from(a) * (100 - pct) + u32::from(b) * pct) / 100) as u8;
    Color(c(a.0, b.0), c(a.1, b.1), c(a.2, b.2), a.3)
}
fn ui_text(text: &str, size: u16, color: Color, style: TextStyle) -> Primitive {
    Primitive::ui_text(text, color, size, style)
}
/// Whether a block holds reading matter — a sentence, a field, a picture — rather than
/// a stack of small controls like a vote arrow over a score.
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
/// What a form is for, read off its id the way a person reads it off the page: the
/// card title where sites show one, and the words on its submit button. A generic
/// `Submit` is all a service's form helper knows to write.
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
/// The words a button shows: its own, unless it is a helper's generic `Submit`.
fn submit_label<'a>(id: &str, text: &'a str) -> &'a str {
    if text == "Submit" {
        form_purpose(id.strip_suffix("-submit").unwrap_or(id)).1
    } else {
        text
    }
}
fn line_height(size: u16) -> u32 {
    (u32::from(size) * 13).div_ceil(10)
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
/// The face a styled element's text is measured with: the monospace family when the
/// style asks for it, otherwise the page face.
fn face_of(style: &Style) -> Typeface {
    if style.mono == Some(true) {
        Typeface::Mono
    } else {
        FACE
    }
}
/// Text in the face `style` asks for: the fixed-pitch `Text` primitive for monospace
/// (one grid cell per character, which is what `Typeface::Mono` measures), otherwise
/// proportional UI text.
fn text_primitive(style: &Style, text: &str, size: u16, color: Color, ts: TextStyle) -> Primitive {
    if style.mono == Some(true) {
        Primitive::Text {
            text: text.to_owned(),
            color,
            size,
        }
    } else {
        ui_text(text, size, color, ts)
    }
}
/// Inner horizontal padding of a link or button: a link is bare text unless it is boxed
/// (filled or bordered), a button is always a padded pill.
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
/// Whether a row child is a one-line control (a link, a button, a chip) that sits at
/// its own width in a row of chips rather than filling a cell.
fn explicit_flex(e: &PageElement) -> bool {
    style_of(e).is_some_and(|s| s.flex.is_some())
}
/// A width the author fixed: a style's, or a picture's declared size, which a row keeps
/// rather than stretching the picture across a flex share.
fn fixed_width(e: &PageElement) -> Option<u32> {
    match e {
        PageElement::Image { width, .. } if *width > 0 => Some(*width),
        _ => style_of(e).and_then(|s| s.width),
    }
}
/// Accessible name for a card: the first text its subtree offers.
fn label_of(children: &[PageElement]) -> String {
    for c in children {
        let found = match c {
            PageElement::Heading { text, .. }
            | PageElement::Text { text, .. }
            | PageElement::Styled { text, .. }
            | PageElement::Badge { text, .. }
            | PageElement::Link { text, .. }
            | PageElement::Button { text, .. } => text.clone(),
            PageElement::Thumbnail { label, .. } | PageElement::Icon { label, .. } => label.clone(),
            PageElement::Row { children, .. }
            | PageElement::Grid { children, .. }
            | PageElement::Card { children, .. }
            | PageElement::Group { children, .. }
            | PageElement::Form { children, .. } => label_of(children),
            _ => String::new(),
        };
        if !found.is_empty() {
            return found;
        }
    }
    String::new()
}
struct Layout<'a> {
    scene: Scene,
    fields: &'a BTreeMap<String, String>,
    images: &'a BTreeMap<String, Arc<ImageAsset>>,
    /// How far each sideways-scrolling row (`Style::scroll_x`) is scrolled, by row id.
    hscroll: &'a BTreeMap<String, i32>,
    used: BTreeSet<u64>,
    decoration: u64,
    accent: Color,
    ink: Color,
    muted: Color,
    border: Color,
    surface: Color,
    /// Measurement runs the real placement with output suppressed, so the measure
    /// pass can never disagree with the place pass.
    dry: bool,
    /// The page's language (`Page::lang`), for text that does not name its own.
    lang: Lang,
}
impl Layout<'_> {
    fn id(&mut self, s: &str) -> u64 {
        if self.dry {
            return 0;
        }
        let mut id = hash(s) & (((1u64 << 51) - 1) & !15);
        while self.used.contains(&id) {
            id = (id + 16) & (((1u64 << 51) - 1) & !15);
        }
        self.used.insert(id);
        id
    }
    fn node(
        &mut self,
        id: u64,
        r: Rect,
        p: Primitive,
        semantic: Option<Semantic>,
        action: Option<&str>,
    ) {
        if self.dry {
            return;
        }
        let mut n = Node::new(id, r, p);
        n.z = self.scene.nodes.len() as i32;
        n.semantic = semantic;
        n.interaction = action.map(str::to_owned);
        n.clip = Some(Rect::new(0, 0, self.scene.width, self.scene.height));
        self.scene.nodes.push(n);
    }
    /// A picture node, its corners rounded by `radius` (an avatar), as one click
    /// target when `action` names one.
    fn picture(
        &mut self,
        id: u64,
        r: Rect,
        p: Primitive,
        radius: u32,
        semantic: Semantic,
        action: Option<&str>,
    ) {
        if self.dry {
            return;
        }
        let mut n = Node::new(id, r, p);
        n.z = self.scene.nodes.len() as i32;
        n.semantic = Some(semantic);
        n.interaction = action.map(str::to_owned);
        n.clip = Some(Rect::new(0, 0, self.scene.width, self.scene.height));
        if radius > 0 {
            n.rounded_clip = Some(RoundedClip { rect: r, radius });
        }
        self.scene.nodes.push(n);
    }
    fn decor(&mut self, r: Rect, fill: Color, radius: u32, border: Option<Color>) {
        let id = self.decoration;
        self.decoration += 1;
        self.node(
            id,
            r,
            Primitive::RoundedBox {
                fill,
                border,
                border_width: u32::from(border.is_some()),
                radius,
            },
            None,
            None,
        );
    }
    fn text(
        &mut self,
        id: u64,
        r: Rect,
        text: &str,
        size: u16,
        color: Color,
        style: impl Into<TextStyle>,
    ) {
        let mut style = style.into();
        if style.lang.is_auto() {
            style.lang = self.lang;
        }
        self.node(id, r, ui_text(text, size, color, style), None, None);
    }
    /// `ts` with the page's language filled in where the element named none.
    fn with_lang(&self, mut ts: TextStyle) -> TextStyle {
        if ts.lang.is_auto() {
            ts.lang = self.lang;
        }
        ts
    }
    fn caption(&mut self, r: Rect, s: &str, size: u16, color: Color, bold: impl Into<TextStyle>) {
        let id = self.decoration;
        self.decoration += 1;
        self.text(id, r, s, size, color, bold);
    }
    /// Height an element occupies at `w`, including its trailing air.
    /// The narrowest `e` can be drawn without breaking a word or clipping a control,
    /// the way CSS's min-content width is. Rows wrap and grids drop columns to keep
    /// every child at least this wide.
    fn min_width(&self, e: &PageElement) -> u32 {
        let longest = |text: &str, bold: TextStyle, size: u16| {
            text.split_whitespace()
                .map(|word| metrics::text_width(FACE, bold, word, size))
                .max()
                .unwrap_or(0)
        };
        let widest = |children: &[PageElement]| {
            children
                .iter()
                .map(|c| self.min_width(c))
                .max()
                .unwrap_or(0)
        };
        let natural = match e {
            PageElement::Heading { text, level, .. } => {
                longest(text, true.into(), if *level <= 1 { 18 } else { 15 })
            }
            PageElement::Text { text, .. } => longest(text, false.into(), 13),
            PageElement::Link { text, style, .. } => {
                let plain = Style::default();
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(13).clamp(6, 96);
                let ts = self.text_style(style);
                let pad = control_pad(style, false) * 2;
                // One-line text is ellipsized to whatever width it gets, so it never
                // needs more than its longest word to draw; asking for the whole line
                // would wrap its row instead.
                pad + text
                    .split_whitespace()
                    .map(|word| metrics::text_width(face_of(style), ts, word, size))
                    .max()
                    .unwrap_or(0)
            }
            PageElement::Button {
                id, text, style, ..
            } => {
                let plain = Style::default();
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(12).clamp(6, 96);
                let ts = self.button_text_style(style);
                metrics::text_width(face_of(style), ts, submit_label(id, text), size)
                    + control_pad(style, true) * 2
            }
            // The label sits above the field on one line, so it sets the floor too.
            PageElement::Input { label, .. } => {
                (metrics::text_width(FACE, false, label, 12) + 20).max(120)
            }
            PageElement::Image { width, .. } => (*width).min(96),
            PageElement::Styled { text, style, .. } => {
                let size = style.size.unwrap_or(13).clamp(6, 96);
                let bold = self.text_style(style);
                let pad = style.padding.unwrap_or(0).min(64) * 2;
                let face = face_of(style);
                pad + text
                    .split_whitespace()
                    .map(|word| metrics::text_width(face, bold, word, size))
                    .max()
                    .unwrap_or(0)
            }
            PageElement::Badge { text, style, .. } => {
                let size = style.size.unwrap_or(10).clamp(6, 96);
                let pad = style.padding.unwrap_or(0).min(64);
                metrics::text_width(FACE, true, text, size) + 2 * pad.max(9)
            }
            // A hairline (a progress bar's segment) carries no caption, so it has no floor.
            PageElement::Thumbnail { style, .. } if style.height.is_some_and(|h| h < 12) => 1,
            PageElement::Thumbnail { .. } => 48,
            PageElement::Icon { style, .. } => {
                u32::from(style.size.unwrap_or(20).clamp(6, 96))
                    + 2 * style.padding.unwrap_or(0).min(64)
            }
            PageElement::Card {
                children, style, ..
            } => style.padding.unwrap_or(14).min(64) * 2 + widest(children),
            PageElement::Row {
                children, style, ..
            }
            | PageElement::Grid {
                children, style, ..
            } => style.padding.unwrap_or(0).min(64) * 2 + widest(children),
            PageElement::Group { children, .. } | PageElement::Form { children, .. } => {
                widest(children)
            }
            PageElement::Divider { .. } | PageElement::Spacer { .. } => 0,
        };
        // On a phone-sized viewport a block of stacked content is a column, and a
        // column wants the screen: side-by-side columns stack, as a site's mobile
        // breakpoint would make them. Single-item chips and pills still flow.
        let viewport = self.scene.width;
        let column = match e {
            PageElement::Row { children, .. }
            | PageElement::Grid { children, .. }
            | PageElement::Card { children, .. }
            | PageElement::Group { children, .. }
            | PageElement::Form { children, .. } => children.len() > 1 && prose(children),
            _ => false,
        };
        let natural = if column && viewport < 600 {
            natural.max(viewport * 3 / 5)
        } else {
            natural
        };
        // A fixed width is a promise the author made; it is honoured as-is.
        style_of(e).and_then(|s| s.width).unwrap_or(natural)
    }
    /// The width `e` takes when nothing constrains it, the way CSS's max-content width
    /// is: text on one line, a picture at its size, a row as the sum of its children.
    /// A row child sits at this width when a sibling flexes or the row justifies.
    fn content_width(&self, e: &PageElement) -> u32 {
        let widest = |children: &[PageElement]| {
            children
                .iter()
                .map(|c| self.content_width(c))
                .max()
                .unwrap_or(0)
        };
        let natural = match e {
            PageElement::Heading { text, level, .. } => {
                metrics::text_width(FACE, true, text, if *level <= 1 { 18 } else { 15 })
            }
            PageElement::Text { text, .. } => metrics::text_width(FACE, false, text, 13),
            PageElement::Link { text, style, .. } => {
                let plain = Style::default();
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(13).clamp(6, 96);
                metrics::text_width(face_of(style), self.text_style(style), text, size)
                    + control_pad(style, false) * 2
            }
            PageElement::Styled { text, style, .. } => {
                let size = style.size.unwrap_or(13).clamp(6, 96);
                metrics::text_width(face_of(style), self.text_style(style), text, size)
                    + style.padding.unwrap_or(0).min(64) * 2
            }
            PageElement::Image { width, .. } => *width,
            PageElement::Thumbnail { .. } => 120,
            PageElement::Input { .. } => 240,
            PageElement::Row {
                children,
                gap,
                style,
                ..
            } => {
                style.padding.unwrap_or(0).min(64) * 2
                    + children.iter().map(|c| self.content_width(c)).sum::<u32>()
                    + (*gap).min(128) * children.len().saturating_sub(1) as u32
            }
            PageElement::Card {
                children, style, ..
            } => style.padding.unwrap_or(14).min(64) * 2 + widest(children),
            PageElement::Grid {
                children, style, ..
            } => style.padding.unwrap_or(0).min(64) * 2 + widest(children),
            PageElement::Group { children, .. } | PageElement::Form { children, .. } => {
                widest(children)
            }
            _ => return self.min_width(e),
        };
        style_of(e)
            .and_then(|s| s.width)
            .unwrap_or(natural.max(self.min_width(e)))
    }
    fn measure(&mut self, e: &PageElement, w: u32, forced: Option<u32>) -> u32 {
        let was = std::mem::replace(&mut self.dry, true);
        let h = self.place(e, 0, 0, w, forced);
        self.dry = was;
        h
    }
    fn element(&mut self, e: &PageElement, x: i32, y: &mut i32, w: u32) {
        *y += self.place(e, x, *y, w, None) as i32;
    }
    fn ink_of(&self, style: &Style) -> Color {
        style
            .color
            .as_deref()
            .and_then(parse_color)
            .unwrap_or(self.ink)
    }
    fn bold_of(&self, style: &Style) -> bool {
        matches!(style.weight.as_deref(), Some("bold") | Some("medium"))
    }
    /// A button's text is bold unless its style says "regular".
    fn button_text_style(&self, style: &Style) -> TextStyle {
        TextStyle::new(
            style.weight.as_deref() != Some("regular"),
            style.italic.unwrap_or(false),
            style
                .lang
                .as_deref()
                .map_or(self.lang, cw_scene::Lang::from_tag),
        )
    }
    /// Weight, slant and language of a styled element's text.
    fn text_style(&self, style: &Style) -> TextStyle {
        TextStyle::new(
            self.bold_of(style),
            style.italic.unwrap_or(false),
            style
                .lang
                .as_deref()
                .map_or(self.lang, cw_scene::Lang::from_tag),
        )
    }
    /// Horizontal offset of `text_width` inside `w` for the style's alignment.
    fn offset(style: &Style, w: u32, text_width: u32) -> i32 {
        match style.align.as_deref() {
            Some("center") => (w.saturating_sub(text_width) / 2) as i32,
            Some("right") | Some("end") => w.saturating_sub(text_width) as i32,
            _ => 0,
        }
    }
    /// Draws `e` at `x`,`y` in `w` pixels and returns the height it consumed.
    /// `forced` is a total height imposed by a row or grid cell.
    fn place(&mut self, e: &PageElement, x: i32, y: i32, w: u32, forced: Option<u32>) -> u32 {
        let id = self.id(e.id());
        match e {
            PageElement::Form {
                id: form, children, ..
            } => {
                // A form with nothing to fill in is a button that posts: it sits inline,
                // with no card around it, like a "Message bob" entry in a sidebar.
                if !children
                    .iter()
                    .any(|c| matches!(c, PageElement::Input { .. }))
                {
                    let mut cy = y;
                    for child in children {
                        self.element(child, x, &mut cy, w);
                    }
                    return (cy - y) as u32;
                }
                let inner = w.saturating_sub(32);
                let body: u32 = children
                    .iter()
                    .map(|c| self.measure(c, inner, None))
                    .sum::<u32>();
                let title = form_purpose(form).0;
                let head = if title.is_some() { 48 } else { 16 };
                let total = head + body + GAP;
                self.decor(
                    Rect::new(x, y, w, total),
                    self.surface,
                    10,
                    Some(self.border),
                );
                if let Some(title) = title {
                    self.text(
                        id,
                        Rect::new(x + 16, y + 16, inner, 24),
                        title,
                        14,
                        self.ink,
                        true,
                    );
                }
                let mut cy = y + head as i32;
                for child in children {
                    self.element(child, x + 16, &mut cy, inner);
                }
                total
            }
            PageElement::Group { children, .. } => {
                let mut cy = y;
                for child in children {
                    self.element(child, x, &mut cy, w);
                }
                (cy - y) as u32
            }
            PageElement::Input {
                id: action,
                label,
                value,
                placeholder,
            } => {
                let value = self.fields.get(action).unwrap_or(value);
                let multiline =
                    action.ends_with("-body") || label == "Content" || label == "Message";
                let h = if multiline { 82 } else { 36 };
                self.text(id, Rect::new(x, y, w, 18), label, 11, self.muted, false);
                let r = Rect::new(x, y + 21, w, h);
                let semantic = Semantic {
                    role: "textbox".into(),
                    label: label.clone(),
                    value: Some(value.clone()),
                    focusable: true,
                    ..Semantic::default()
                };
                self.node(
                    id + 1,
                    r,
                    Primitive::RoundedBox {
                        fill: Color::rgb(253, 254, 255),
                        border: Some(Color::rgb(207, 215, 226)),
                        border_width: 1,
                        radius: 5,
                    },
                    Some(semantic),
                    Some(action),
                );
                let (shown, colour) = if value.is_empty() {
                    (placeholder, self.muted)
                } else {
                    (value, self.ink)
                };
                self.text(
                    id + 2,
                    Rect::new(x + 10, y + 30, w.saturating_sub(20), h.saturating_sub(12)),
                    shown,
                    13,
                    colour,
                    false,
                );
                h + 34
            }
            PageElement::Button {
                id: action,
                text,
                style,
                ..
            } => {
                let text = submit_label(action, text);
                let plain = Style::default();
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(12).clamp(6, 96);
                let face = face_of(style);
                let ts = self.button_text_style(style);
                let pad = control_pad(style, true);
                // The pill's vertical padding follows its horizontal one at the ratio
                // the default 34 px button has.
                let vpad = style.padding.map_or(9, |p| p.min(64) * 2 / 3);
                let lh = line_height(size);
                let shown = metrics::ellipsize(face, ts, text, size, w.saturating_sub(pad * 2));
                let tw = metrics::text_width(face, ts, &shown, size);
                let bw = style.width.unwrap_or(tw + pad * 2).min(w);
                let bh = style.height.unwrap_or(lh + vpad * 2);
                let bx = x + Self::offset(style, w, bw);
                let edge = style.border.as_deref().and_then(parse_color);
                self.node(
                    id,
                    Rect::new(bx, y, bw, bh),
                    Primitive::RoundedBox {
                        fill: style
                            .background
                            .as_deref()
                            .and_then(parse_color)
                            .unwrap_or(self.accent),
                        border: edge,
                        border_width: u32::from(edge.is_some()),
                        radius: style.radius.unwrap_or(6).min(64),
                    },
                    Some(Semantic {
                        role: "button".into(),
                        label: text.into(),
                        focusable: true,
                        ..Semantic::default()
                    }),
                    Some(action),
                );
                let colour = style
                    .color
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or(Color::WHITE);
                let tw = tw.min(bw.saturating_sub(pad * 2).max(1));
                let ts = self.with_lang(ts);
                self.node(
                    id + 1,
                    Rect::new(
                        bx + (bw.saturating_sub(tw) / 2) as i32,
                        y + (bh.saturating_sub(lh) / 2) as i32,
                        tw + 4,
                        lh + 3,
                    ),
                    text_primitive(style, &shown, size, colour, ts),
                    None,
                    None,
                );
                bh + GAP
            }
            PageElement::Link {
                id: action,
                text,
                style,
                ..
            } => {
                // Bare, a link is accent-coloured text at its own width, as a link in a
                // paragraph or a sidebar is; a style makes it a nav item, a tab or a
                // bordered button. A long one wraps, and its box grows with it.
                let plain = Style::default();
                let styled = style.is_some();
                let style = style.as_ref().unwrap_or(&plain);
                let size = style.size.unwrap_or(13).clamp(6, 96);
                let face = face_of(style);
                let ts = self.text_style(style);
                let boxed = style.background.is_some() || style.border.is_some();
                let pad = control_pad(style, false);
                let lh = line_height(size);
                let inner = style.width.map_or(w, |v| v.min(w)).saturating_sub(pad * 2);
                let lines: Vec<String> = if style.one_line == Some(true) {
                    vec![metrics::ellipsize(face, ts, text, size, inner)]
                } else {
                    metrics::wrap(face, ts, text, size, inner)
                        .iter()
                        .map(|l| l.trim_end().to_owned())
                        .collect()
                };
                let tw = lines
                    .iter()
                    .map(|l| metrics::text_width(face, ts, l, size))
                    .max()
                    .unwrap_or(0)
                    .min(inner.max(1));
                let body = lines.len() as u32 * lh;
                let bw = style.width.unwrap_or(tw + pad * 2).min(w);
                let bh = style.height.unwrap_or(body + pad * 2);
                let bx = x + Self::offset(style, w, bw);
                let colour = style
                    .color
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or(self.accent);
                let semantic = Semantic {
                    role: "link".into(),
                    label: text.clone(),
                    focusable: true,
                    ..Semantic::default()
                };
                let text_rect = Rect::new(
                    bx + pad as i32,
                    y + pad as i32,
                    bw.saturating_sub(pad * 2).max(1),
                    body.max(lh),
                );
                let ts = self.with_lang(ts);
                let primitive = text_primitive(style, &lines.join("\n"), size, colour, ts);
                if boxed {
                    let edge = style.border.as_deref().and_then(parse_color);
                    self.node(
                        id,
                        Rect::new(bx, y, bw, bh),
                        Primitive::RoundedBox {
                            fill: style
                                .background
                                .as_deref()
                                .and_then(parse_color)
                                .unwrap_or(Color::TRANSPARENT),
                            border: edge,
                            border_width: u32::from(edge.is_some()),
                            radius: style.radius.unwrap_or(6).min(64),
                        },
                        Some(semantic),
                        Some(action),
                    );
                    self.node(id + 1, text_rect, primitive, None, None);
                } else {
                    self.node(id, text_rect, primitive, Some(semantic), Some(action));
                }
                bh + if styled { 6 } else { 9 }
            }
            PageElement::Heading { text, level, .. } => {
                let size = if *level <= 1 { 18 } else { 15 };
                self.node(
                    id,
                    Rect::new(x, y, w, 30),
                    ui_text(text, size, self.ink, TextStyle::new(true, false, self.lang)),
                    Some(Semantic {
                        role: "heading".into(),
                        label: text.clone(),
                        ..Semantic::default()
                    }),
                    None,
                );
                38
            }
            PageElement::Text { text, .. } => {
                let lines = metrics::wrap(FACE, false, text, 13, w);
                let lines: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();
                let h = lines.len() as u32 * 17 + 9;
                self.node(
                    id,
                    Rect::new(x, y, w, h),
                    ui_text(
                        &lines.join("\n"),
                        13,
                        self.ink,
                        TextStyle::new(false, false, self.lang),
                    ),
                    Some(Semantic {
                        role: "text".into(),
                        label: text.clone(),
                        ..Semantic::default()
                    }),
                    None,
                );
                h + GAP
            }
            PageElement::Image {
                id: asset_id,
                alt,
                width,
                height,
                style,
                action,
                ..
            } => {
                let plain = Style::default();
                let style = style.as_ref().unwrap_or(&plain);
                if let Some(a) = self.images.get(asset_id) {
                    let natural_w =
                        style
                            .width
                            .unwrap_or(if *width == 0 { a.width } else { *width });
                    let natural_h = if *height == 0 { a.height } else { *height };
                    let dw = natural_w.min(w).max(1);
                    // A picture narrowed to its column keeps its proportions.
                    let dh = style.height.unwrap_or_else(|| {
                        (u64::from(natural_h) * u64::from(dw) / u64::from(natural_w.max(1)))
                            .min(4096) as u32
                    });
                    let r = Rect::new(x + Self::offset(style, w, dw), y, dw, dh);
                    let (semantic, target) = match action {
                        Some(action) => (
                            Semantic {
                                role: if action.method.eq_ignore_ascii_case("GET") {
                                    "link".into()
                                } else {
                                    "button".into()
                                },
                                label: alt.clone(),
                                focusable: true,
                                ..Semantic::default()
                            },
                            Some(asset_id.as_str()),
                        ),
                        None => (
                            Semantic {
                                role: "img".into(),
                                label: alt.clone(),
                                ..Semantic::default()
                            },
                            None,
                        ),
                    };
                    let radius = style.radius.unwrap_or(0).min(dw.min(dh) / 2);
                    self.picture(
                        id,
                        r,
                        Primitive::Image {
                            width: a.width,
                            height: a.height,
                            rgba: a.rgba.clone(),
                        },
                        radius,
                        semantic,
                        target,
                    );
                    if let Some(edge) = style.border.as_deref().and_then(parse_color) {
                        self.decor(r, Color::TRANSPARENT, radius, Some(edge));
                    }
                    dh + GAP
                } else {
                    self.text(id, Rect::new(x, y, w, 24), alt, 13, self.muted, false);
                    32
                }
            }
            PageElement::Row {
                id: row,
                children,
                gap,
                style,
                ..
            } if style.scroll_x == Some(true) => {
                self.scroll_row(row, children, *gap, x, y, w, style, forced)
            }
            PageElement::Row {
                children,
                gap,
                align,
                style,
                ..
            } => self.row(children, *gap, align, x, y, w, style, forced),
            PageElement::Grid {
                columns,
                children,
                gap,
                style,
                ..
            } => self.grid(children, *columns, *gap, x, y, w, style, forced),
            PageElement::Card {
                id: target,
                children,
                style,
                action,
            } => {
                let w = style.width.map_or(w, |v| v.min(w));
                let pad = style.padding.unwrap_or(14).min(64);
                let radius = style.radius.unwrap_or(10).min(64);
                let inner = w.saturating_sub(pad * 2);
                let content: u32 = children
                    .iter()
                    .map(|c| self.measure(c, inner, None))
                    .sum::<u32>();
                let natural = (content + pad * 2).saturating_sub(GAP).max(pad * 2);
                let box_h = forced
                    .map(|f| f.saturating_sub(GAP))
                    .or(style.height)
                    .unwrap_or(natural);
                let fill = style
                    .background
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or(self.surface);
                let edge = style.border.as_deref().and_then(parse_color);
                let r = Rect::new(x, y, w, box_h);
                if let Some(action) = action {
                    let role = if action.method.eq_ignore_ascii_case("GET") {
                        "link"
                    } else {
                        "button"
                    };
                    self.node(
                        id,
                        r,
                        Primitive::RoundedBox {
                            fill,
                            border: edge,
                            border_width: u32::from(edge.is_some()),
                            radius,
                        },
                        Some(Semantic {
                            role: role.into(),
                            label: label_of(children),
                            focusable: true,
                            ..Semantic::default()
                        }),
                        Some(target),
                    );
                } else {
                    self.decor(r, fill, radius, edge);
                }
                let mut cy = y + pad as i32;
                for child in children {
                    self.element(child, x + pad as i32, &mut cy, inner);
                }
                box_h + GAP
            }
            PageElement::Styled { text, style, .. } => {
                let size = style.size.unwrap_or(13).clamp(6, 96);
                let bold = self.text_style(style);
                let face = face_of(style);
                let pad = style.padding.unwrap_or(0).min(64);
                let colour = self.ink_of(style);
                let w = style.width.map_or(w, |v| v.min(w));
                let inner = w.saturating_sub(pad * 2);
                let lh = line_height(size);
                let lines: Vec<String> = if style.one_line.unwrap_or(false) {
                    vec![metrics::ellipsize(face, bold, text, size, inner)]
                } else {
                    metrics::wrap(face, bold, text, size, inner)
                        .iter()
                        .map(|l| l.trim_end().to_owned())
                        .collect()
                };
                let body = lines.len() as u32 * lh;
                let box_h = style.height.unwrap_or(body + pad * 2);
                if let Some(fill) = style.background.as_deref().and_then(parse_color) {
                    self.decor(
                        Rect::new(x, y, w, box_h),
                        fill,
                        style.radius.unwrap_or(0).min(64),
                        style.border.as_deref().and_then(parse_color),
                    );
                }
                let semantic = Semantic {
                    role: "text".into(),
                    label: text.clone(),
                    ..Semantic::default()
                };
                let bold = self.with_lang(bold);
                if style.align.is_none() || style.align.as_deref() == Some("left") {
                    self.node(
                        id,
                        Rect::new(x + pad as i32, y + pad as i32, inner, body.max(lh)),
                        text_primitive(style, &lines.join("\n"), size, colour, bold),
                        Some(semantic),
                        None,
                    );
                } else {
                    // Per-line nodes are the only way to align wrapped text; the first
                    // carries the accessible name for the whole block, the rest are decor.
                    for (i, line) in lines.iter().enumerate() {
                        let tw = metrics::text_width(face, bold, line, size);
                        let r = Rect::new(
                            x + pad as i32 + Self::offset(style, inner, tw),
                            y + pad as i32 + (i as u32 * lh) as i32,
                            tw.max(1),
                            lh,
                        );
                        let primitive = text_primitive(style, line, size, colour, bold);
                        if i == 0 {
                            self.node(id, r, primitive, Some(semantic.clone()), None);
                        } else {
                            let n = self.decoration;
                            self.decoration += 1;
                            self.node(n, r, primitive, None, None);
                        }
                    }
                }
                box_h + 6
            }
            PageElement::Thumbnail {
                id: target,
                label,
                style,
                action,
            } => {
                let w = style.width.map_or(w, |v| v.min(w));
                let box_h = forced
                    .map(|f| f.saturating_sub(GAP))
                    .or(style.height)
                    .unwrap_or(120);
                let fill = style
                    .background
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or_else(|| tint(label));
                let radius = style.radius.unwrap_or(8).min(64);
                let r = Rect::new(x, y, w, box_h);
                let edge = style.border.as_deref().and_then(parse_color);
                let primitive = Primitive::RoundedBox {
                    fill,
                    border: edge,
                    border_width: u32::from(edge.is_some()),
                    radius,
                };
                if let Some(action) = action {
                    let role = if action.method.eq_ignore_ascii_case("GET") {
                        "link"
                    } else {
                        "button"
                    };
                    self.node(
                        id,
                        r,
                        primitive,
                        Some(Semantic {
                            role: role.into(),
                            label: label.clone(),
                            focusable: true,
                            ..Semantic::default()
                        }),
                        Some(target),
                    );
                } else {
                    self.node(
                        id,
                        r,
                        primitive,
                        Some(Semantic {
                            role: "img".into(),
                            label: label.clone(),
                            ..Semantic::default()
                        }),
                        None,
                    );
                }
                // A caption taller than its box (a progress bar's segment) stays the
                // accessible name only; painting it would spill over its neighbours.
                let caption = line_height(style.size.unwrap_or(12).clamp(6, 96));
                if !label.is_empty() && caption <= box_h {
                    let size = style.size.unwrap_or(12).clamp(6, 96);
                    let colour = self.ink_of(style);
                    // Small tiles (avatars) keep a 2 px margin so initials fit.
                    let room = if w >= 64 { w - 16 } else { w.saturating_sub(4) };
                    let shown = metrics::ellipsize(FACE, true, label, size, room);
                    let tw = metrics::text_width(FACE, true, &shown, size);
                    let lh = line_height(size);
                    self.caption(
                        Rect::new(
                            x + (w.saturating_sub(tw) / 2) as i32,
                            y + (box_h.saturating_sub(lh) / 2) as i32,
                            tw.max(1),
                            lh,
                        ),
                        &shown,
                        size,
                        colour,
                        true,
                    );
                }
                box_h + GAP
            }
            PageElement::Badge { text, style, .. } => {
                let size = style.size.unwrap_or(10).clamp(6, 96);
                let tw = metrics::text_width(FACE, true, text, size);
                // Padding grows the pill around its text; without any it keeps the
                // compact 20 px count-badge shape.
                let pad = style.padding.unwrap_or(0).min(64);
                let bw = style.width.unwrap_or(tw + 2 * pad.max(9)).min(w);
                let bh = style
                    .height
                    .unwrap_or((line_height(size) + 2 * pad).max(20));
                // Only a badge that names neither a fill, a border nor a text colour is the
                // default accent pill. An outlined badge is see-through, and one that only
                // colours its text is a plain label (a count, a star), not a pill painted in
                // the colour of its own text.
                let fill = style.background.as_deref().and_then(parse_color).unwrap_or(
                    if style.border.is_some() || style.color.is_some() {
                        Color::TRANSPARENT
                    } else {
                        self.accent
                    },
                );
                // An empty badge holds its place in the layout and draws nothing: a count
                // of zero is no count, not a blank pill.
                if text.is_empty() {
                    return bh + 6;
                }
                let colour = style
                    .color
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or(Color::WHITE);
                let bx = x + Self::offset(style, w, bw);
                self.node(
                    id,
                    Rect::new(bx, y, bw, bh),
                    Primitive::RoundedBox {
                        fill,
                        border: style.border.as_deref().and_then(parse_color),
                        border_width: u32::from(style.border.is_some()),
                        radius: style.radius.unwrap_or(bh / 2).min(64),
                    },
                    Some(Semantic {
                        role: "text".into(),
                        label: text.clone(),
                        ..Semantic::default()
                    }),
                    None,
                );
                self.caption(
                    Rect::new(
                        bx + (bw.saturating_sub(tw) / 2) as i32,
                        y + (bh.saturating_sub(line_height(size)) / 2) as i32,
                        tw.max(1),
                        line_height(size),
                    ),
                    text,
                    size,
                    colour,
                    true,
                );
                bh + 6
            }
            PageElement::Icon {
                id: target,
                name,
                label,
                style,
                action,
            } => {
                let size = u32::from(style.size.unwrap_or(20).clamp(6, 96));
                let pad = style.padding.unwrap_or(0).min(64);
                let bw = style.width.unwrap_or(size + 2 * pad).min(w);
                let bh = style.height.unwrap_or(size + 2 * pad);
                let bx = x + Self::offset(style, w, bw);
                let r = Rect::new(bx, y, bw, bh);
                let edge = style.border.as_deref().and_then(parse_color);
                let primitive = Primitive::RoundedBox {
                    fill: style
                        .background
                        .as_deref()
                        .and_then(parse_color)
                        .unwrap_or(Color::TRANSPARENT),
                    border: edge,
                    border_width: u32::from(edge.is_some()),
                    radius: style.radius.unwrap_or(bw.min(bh) / 2).min(64),
                };
                match action {
                    Some(action) => {
                        let role = if action.method.eq_ignore_ascii_case("GET") {
                            "link"
                        } else {
                            "button"
                        };
                        self.node(
                            id,
                            r,
                            primitive,
                            Some(Semantic {
                                role: role.into(),
                                label: label.clone(),
                                focusable: true,
                                ..Semantic::default()
                            }),
                            Some(target),
                        );
                    }
                    None => self.node(
                        id,
                        r,
                        primitive,
                        Some(Semantic {
                            role: "img".into(),
                            label: label.clone(),
                            ..Semantic::default()
                        }),
                        None,
                    ),
                }
                let glyph = self.decoration;
                self.decoration += 1;
                self.node(
                    glyph,
                    Rect::new(
                        bx + (bw.saturating_sub(size) / 2) as i32,
                        y + (bh.saturating_sub(size) / 2) as i32,
                        size,
                        size,
                    ),
                    Primitive::Symbol {
                        asset: format!("symbol/{name}"),
                        color: self.ink_of(style),
                    },
                    None,
                    None,
                );
                bh + 6
            }
            PageElement::Divider { style, .. } => {
                let colour = style
                    .color
                    .as_deref()
                    .and_then(parse_color)
                    .unwrap_or(self.border);
                let h = style.height.unwrap_or(1).clamp(1, 64);
                self.decor(Rect::new(x, y + 8, w, h), colour, 0, None);
                h + 16
            }
            PageElement::Spacer { height, .. } => (*height).min(8192),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        children: &[PageElement],
        gap: u32,
        align: &str,
        x: i32,
        y: i32,
        w: u32,
        style: &Style,
        forced: Option<u32>,
    ) -> u32 {
        let w = style.width.map_or(w, |v| v.min(w));
        let pad = style.padding.unwrap_or(0).min(64);
        let gap = gap.min(128);
        if children.is_empty() {
            return style.height.unwrap_or(0);
        }
        let inner = w.saturating_sub(pad * 2);
        // Too narrow for every child side by side: flow onto as many lines as it takes,
        // like `flex-wrap: wrap`. Each line then lays out as a row of its own.
        let mins: Vec<u32> = children.iter().map(|c| self.min_width(c)).collect();
        let needed = mins.iter().sum::<u32>() + gap * (children.len() as u32 - 1);
        if children.len() > 1 && needed > inner {
            let mut lines = Vec::new();
            let (mut start, mut used) = (0, 0);
            for (i, min) in mins.iter().enumerate() {
                let add = if i > start { gap + min } else { *min };
                if i > start && used + add > inner {
                    lines.push(start..i);
                    (start, used) = (i, *min);
                } else {
                    used += add;
                }
            }
            lines.push(start..children.len());
            let plain = Style::default();
            let was = std::mem::replace(&mut self.dry, true);
            let heights: Vec<u32> = lines
                .iter()
                .map(|r| self.row(&children[r.clone()], gap, align, 0, 0, inner, &plain, None))
                .collect();
            self.dry = was;
            let content = heights.iter().sum::<u32>() + gap * (lines.len() as u32 - 1);
            let box_h = forced.or(style.height).unwrap_or(content + pad * 2);
            self.row_decor(x, y, w, box_h, style);
            let mut cy = y + pad as i32;
            for (r, h) in lines.into_iter().zip(heights) {
                self.row(
                    &children[r],
                    gap,
                    align,
                    x + pad as i32,
                    cy,
                    inner,
                    &plain,
                    None,
                );
                cy += (h + gap) as i32;
            }
            return box_h;
        }
        let avail = inner.saturating_sub(gap * (children.len() as u32 - 1));
        // Once any child flexes (or the row justifies its children), the others are
        // chips at their own width: a "+ Create" pill next to a spacer, a nav bar's
        // items. Without either, every child fills an equal share as before.
        let chips = style.justify.is_some() || children.iter().any(explicit_flex);
        let fixed: Vec<Option<u32>> = children
            .iter()
            .map(|c| {
                fixed_width(c)
                    .or_else(|| (chips && !explicit_flex(c)).then(|| self.content_width(c)))
                    .map(|v| v.min(avail))
            })
            .collect();
        // A flex child never shrinks below its content: those that would are held at
        // their minimum, and the rest share what is left, as `min-width: auto` does.
        let mut fixed = fixed;
        loop {
            let rest = avail.saturating_sub(fixed.iter().flatten().sum::<u32>());
            let share: u32 = children
                .iter()
                .zip(&fixed)
                .filter(|(_, f)| f.is_none())
                .map(|(c, _)| style_of(c).and_then(|s| s.flex).unwrap_or(1).max(1))
                .sum();
            let mut held = false;
            for ((c, f), min) in children.iter().zip(fixed.iter_mut()).zip(&mins) {
                let flex = style_of(c).and_then(|s| s.flex).unwrap_or(1).max(1);
                if f.is_none() && (rest * flex).checked_div(share).unwrap_or(0) < *min {
                    *f = Some((*min).min(avail));
                    held = true;
                }
            }
            if !held {
                break;
            }
        }
        let mut rest = avail.saturating_sub(fixed.iter().flatten().sum::<u32>());
        let mut share: u32 = children
            .iter()
            .zip(&fixed)
            .filter(|(_, f)| f.is_none())
            .map(|(c, _)| style_of(c).and_then(|s| s.flex).unwrap_or(1).max(1))
            .sum();
        let widths: Vec<u32> = children
            .iter()
            .zip(&fixed)
            .map(|(c, f)| match f {
                Some(v) => *v,
                None => {
                    let flex = style_of(c).and_then(|s| s.flex).unwrap_or(1).max(1);
                    let take = (rest * flex).checked_div(share).unwrap_or(0);
                    rest -= take;
                    share -= flex;
                    take
                }
            })
            .collect();
        let heights: Vec<u32> = children
            .iter()
            .zip(&widths)
            .map(|(c, w)| self.measure(c, *w, None))
            .collect();
        let content = heights.iter().copied().max().unwrap_or(0);
        let box_h = forced.or(style.height).unwrap_or(content + pad * 2);
        self.row_decor(x, y, w, box_h, style);
        let band = box_h.saturating_sub(pad * 2);
        // Room the children leave, placed as `justify` says: `flex: 1` children have
        // already taken it all, so this only moves a row of chips.
        let spare = avail.saturating_sub(widths.iter().sum::<u32>());
        let slots = children.len() as u32 - 1;
        // The leading offset, and what each gap grows by: `space-between` shares the
        // spare width over the gaps, the odd pixels going to the first ones.
        let (lead, extra, odd) = match style.justify.as_deref() {
            Some("center") => (spare / 2, 0, 0),
            Some("end") => (spare, 0, 0),
            Some("space-between") if slots > 0 => (0, spare / slots, spare % slots),
            _ => (0, 0, 0),
        };
        let mut cx = x + pad as i32 + lead as i32;
        for (i, ((child, cw), ch)) in children.iter().zip(&widths).zip(&heights).enumerate() {
            let (cy, forced) = match align {
                "center" => (y + pad as i32 + (band.saturating_sub(*ch) / 2) as i32, None),
                "end" => (y + pad as i32 + band.saturating_sub(*ch) as i32, None),
                "stretch" => (y + pad as i32, Some(band)),
                _ => (y + pad as i32, None),
            };
            self.place(child, cx, cy, *cw, forced);
            cx += (*cw + gap + extra + u32::from((i as u32) < odd)) as i32;
        }
        box_h
    }
    /// A row that scrolls sideways: every child at its own width on one line, shifted by
    /// the row's scroll offset and clipped to the row, which is published as a
    /// horizontal scroll area so a wheel or a swipe over it moves it.
    #[allow(clippy::too_many_arguments)]
    fn scroll_row(
        &mut self,
        row: &str,
        children: &[PageElement],
        gap: u32,
        x: i32,
        y: i32,
        w: u32,
        style: &Style,
        forced: Option<u32>,
    ) -> u32 {
        let w = style.width.map_or(w, |v| v.min(w));
        let pad = style.padding.unwrap_or(0).min(64);
        let gap = gap.min(128);
        let widths: Vec<u32> = children
            .iter()
            .map(|c| fixed_width(c).unwrap_or_else(|| self.min_width(c)).max(1))
            .collect();
        let heights: Vec<u32> = children
            .iter()
            .zip(&widths)
            .map(|(c, cw)| self.measure(c, *cw, None))
            .collect();
        let content =
            widths.iter().sum::<u32>() + gap * children.len().saturating_sub(1) as u32 + pad * 2;
        let band = heights.iter().copied().max().unwrap_or(0);
        let box_h = forced.or(style.height).unwrap_or(band + pad * 2);
        self.row_decor(x, y, w, box_h, style);
        let max = content.saturating_sub(w) as i32;
        let offset = self.hscroll.get(row).copied().unwrap_or(0).clamp(0, max);
        let view = Rect::new(x, y, w, box_h);
        let mark = self.scene.nodes.len();
        let mut cx = x + pad as i32 - offset;
        for (child, cw) in children.iter().zip(&widths) {
            // Children wholly outside the row are not drawn at all.
            if cx + (*cw as i32) > x && cx < x + w as i32 {
                self.place(child, cx, y + pad as i32, *cw, Some(band));
            }
            cx += (*cw + gap) as i32;
        }
        if !self.dry {
            for n in &mut self.scene.nodes[mark..] {
                n.clip = Some(
                    n.clip
                        .unwrap_or(view)
                        .intersection(view)
                        .unwrap_or(Rect::new(x, y, 0, 0)),
                );
            }
            self.scene.scrolls.push(cw_scene::ScrollArea {
                target: format!("pane:row:{row}"),
                window: None,
                bounds: view,
                offset,
                extent: content.max(w),
                title: None,
                title_height: 0,
                horizontal: true,
            });
        }
        box_h
    }
    fn row_decor(&mut self, x: i32, y: i32, w: u32, h: u32, style: &Style) {
        let radius = style.radius.unwrap_or(0).min(64);
        let edge = style.border.as_deref().and_then(parse_color);
        if let Some(fill) = style.background.as_deref().and_then(parse_color) {
            self.decor(Rect::new(x, y, w, h), fill, radius, edge);
        } else if edge.is_some() {
            self.decor(Rect::new(x, y, w, h), Color::TRANSPARENT, radius, edge);
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn grid(
        &mut self,
        children: &[PageElement],
        columns: u32,
        gap: u32,
        x: i32,
        y: i32,
        w: u32,
        style: &Style,
        forced: Option<u32>,
    ) -> u32 {
        let w = style.width.map_or(w, |v| v.min(w));
        let columns = columns.clamp(1, 12) as usize;
        let gap = gap.min(128);
        let pad = style.padding.unwrap_or(0).min(64);
        let inner = w.saturating_sub(pad * 2);
        // Fewer columns when the cells would be narrower than their content, or than a
        // phone-sized column: what a responsive grid's breakpoints do.
        // Only cells that are blocks of content (cards, tiles) reflow; a calendar's
        // day cells or a keypad keep their columns at any width.
        let blocks = children.iter().any(|c| match c {
            PageElement::Card { children, .. }
            | PageElement::Group { children, .. }
            | PageElement::Row { children, .. }
            | PageElement::Grid { children, .. } => children.len() > 1 && prose(children),
            PageElement::Thumbnail { style, .. } => style.height.unwrap_or(0) >= 60,
            _ => false,
        });
        let floor = if blocks {
            children
                .iter()
                .map(|c| self.min_width(c))
                .max()
                .unwrap_or(0)
                .max(if self.scene.width < 600 { 150 } else { 0 })
        } else {
            0
        };
        let fits = |n: usize| inner.saturating_sub(gap * (n as u32 - 1)) / n as u32 >= floor;
        let columns = (1..=columns).rev().find(|n| fits(*n)).unwrap_or(1);
        let cell = inner.saturating_sub(gap * (columns as u32 - 1)) / columns as u32;
        let rows: Vec<&[PageElement]> = children.chunks(columns).collect();
        let heights: Vec<u32> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|c| self.measure(c, cell, None))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let content = heights.iter().sum::<u32>() + gap * rows.len().saturating_sub(1) as u32;
        let box_h = forced.or(style.height).unwrap_or(content + pad * 2);
        if let Some(fill) = style.background.as_deref().and_then(parse_color) {
            self.decor(
                Rect::new(x, y, w, box_h),
                fill,
                style.radius.unwrap_or(0).min(64),
                style.border.as_deref().and_then(parse_color),
            );
        }
        let mut cy = y + pad as i32;
        for (row, h) in rows.iter().zip(&heights) {
            for (i, child) in row.iter().enumerate() {
                let cx = x + pad as i32 + (i as u32 * (cell + gap)) as i32;
                self.place(child, cx, cy, cell, Some(*h));
            }
            cy += (*h + gap) as i32;
        }
        box_h
    }
}
/// Draw a laid-out page `percent`% of its size into a `width` x `height` viewport.
/// Text is re-rasterised at the scaled size rather than resampled, so zoomed text is
/// as sharp as any other; each text box gets a little slack because glyph advances
/// do not scale exactly linearly and a line that just fitted must still fit.
pub(super) fn scale(scene: &mut Scene, percent: u32, width: u32, height: u32) {
    let p = |v: i32| (i64::from(v) * i64::from(percent) / 100) as i32;
    let q = |v: u32| (u64::from(v) * u64::from(percent)).div_ceil(100) as u32;
    let rect = |r: Rect| Rect::new(p(r.x), p(r.y), q(r.width), q(r.height));
    let size = |s: u16| ((u32::from(s) * percent + 50) / 100).clamp(6, 400) as u16;
    for node in &mut scene.nodes {
        node.bounds = rect(node.bounds);
        node.clip = node.clip.map(rect);
        if let Some(clip) = &mut node.rounded_clip {
            clip.rect = rect(clip.rect);
            clip.radius = q(clip.radius);
        }
        node.transform.tx = p(node.transform.tx);
        node.transform.ty = p(node.transform.ty);
        match &mut node.primitive {
            Primitive::Box { border_width, .. } => *border_width = q(*border_width),
            Primitive::RoundedBox {
                border_width,
                radius,
                ..
            } => {
                *border_width = q(*border_width);
                *radius = q(*radius);
            }
            Primitive::UiText { size: s, .. }
            | Primitive::UiTextBold { size: s, .. }
            | Primitive::Text { size: s, .. } => {
                *s = size(*s);
                node.bounds.width += node.bounds.width / 24 + 2;
            }
            Primitive::Shadow { radius, blur, .. } => {
                *radius = q(*radius);
                *blur = q(*blur);
            }
            Primitive::Backdrop { radius, .. } => *radius = q(*radius),
            Primitive::Path {
                points,
                stroke_width,
                ..
            } => {
                for (x, y) in points.iter_mut() {
                    (*x, *y) = (p(*x), p(*y));
                }
                *stroke_width =
                    ((u32::from(*stroke_width) * percent + 50) / 100).clamp(1, 64) as u16;
            }
            _ => {}
        }
    }
    // The page still fills the viewport; its offset and extent stay in CSS pixels,
    // the units `browser.v1 scroll` takes. A row that scrolls sideways is drawn larger
    // or smaller with everything else.
    for area in &mut scene.scrolls {
        area.bounds = if area.horizontal {
            rect(area.bounds)
        } else {
            Rect::new(0, 0, width, height)
        };
    }
    scene.width = width;
    scene.height = height;
}
pub(super) fn layout(
    page: &Page,
    fields: &BTreeMap<String, String>,
    images: &BTreeMap<String, Arc<ImageAsset>>,
    width: u32,
    height: u32,
    scroll: i32,
) -> Scene {
    layout_scrolled(
        page,
        fields,
        images,
        width,
        height,
        scroll,
        &BTreeMap::new(),
    )
}
/// `layout`, with sideways-scrolling rows at the offsets `hscroll` names.
pub(super) fn layout_scrolled(
    page: &Page,
    fields: &BTreeMap<String, String>,
    images: &BTreeMap<String, Arc<ImageAsset>>,
    width: u32,
    height: u32,
    scroll: i32,
    hscroll: &BTreeMap<String, i32>,
) -> Scene {
    let theme = page.theme.as_ref();
    let themed = theme.is_some();
    let colour = |pick: fn(&cw_protocol::PageTheme) -> Option<&String>, fallback: Color| {
        theme
            .and_then(pick)
            .map(String::as_str)
            .and_then(parse_color)
            .unwrap_or(fallback)
    };
    let accent = colour(
        |t| t.accent.as_ref(),
        match page.title.as_str() {
            "Chat" => Color::rgb(89, 47, 112),
            "Calendar" => Color::rgb(31, 112, 201),
            "Documents" => Color::rgb(33, 132, 100),
            _ => Color::rgb(34, 112, 205),
        },
    );
    let ink = colour(|t| t.ink.as_ref(), INK);
    let surface = colour(|t| t.surface.as_ref(), Color::WHITE);
    let mut p = Layout {
        scene: Scene::new(width, height),
        fields,
        images,
        hscroll,
        used: BTreeSet::new(),
        decoration: 1 << 52,
        accent,
        ink,
        muted: colour(|t| t.muted.as_ref(), MUTED),
        border: if themed {
            mix(surface, ink, 14)
        } else {
            BORDER
        },
        surface,
        dry: false,
        lang: page.lang.as_deref().map_or(Lang::Auto, Lang::from_tag),
    };
    p.scene.background = colour(|t| t.background.as_ref(), Color::rgb(248, 250, 253));
    let special = !themed
        && matches!(
            page.title.as_str(),
            "Mail" | "Chat" | "Calendar" | "Documents"
        );
    let sidebar = if special && width >= 720 { 176 } else { 0 };
    let mut y = if special { 88 } else { 16 };
    y -= scroll;
    if special {
        if sidebar > 0 {
            p.decor(
                Rect::new(0, 64, sidebar, height.saturating_sub(64)),
                if page.title == "Chat" {
                    Color::rgb(246, 242, 248)
                } else {
                    Color::rgb(241, 245, 250)
                },
                0,
                None,
            );
            p.caption(
                Rect::new(18, 84, 145, 20),
                match page.title.as_str() {
                    "Mail" => "MESSAGES",
                    "Chat" => "CHANNELS",
                    "Documents" => "DOCUMENTS",
                    _ => "AGENDA",
                },
                10,
                MUTED,
                true,
            );
        }
    } else if !themed {
        p.text(
            1,
            Rect::new(16, y, width.saturating_sub(32), 28),
            &page.title,
            20,
            ink,
            true,
        );
        y += 40;
    }
    let mut x = sidebar as i32 + if special { 24 } else { 16 };
    let mut total = width.saturating_sub(sidebar + if special { 48 } else { 32 });
    // A themed content column centres itself once the viewport is wider than it asks for.
    if let Some(cw) = theme.and_then(|t| t.content_width).filter(|v| *v > 0) {
        if cw < total {
            x += (total - cw) as i32 / 2;
            total = cw;
        }
    }
    let two_column = !themed && matches!(page.title.as_str(), "Mail" | "Calendar") && total >= 600;
    let mainw = if two_column {
        total.saturating_sub(286)
    } else {
        total
    };
    let mut side_y = 116 - scroll;
    let mut form_y = 88 - scroll;
    let mut deferred = Vec::new();
    fn pin_of(e: &PageElement) -> Option<&str> {
        style_of(e).and_then(|s| s.pin.as_deref())
    }
    let pinned = |e: &PageElement| matches!(pin_of(e), Some("top" | "bottom"));
    // A sticky header is measured first: the page flows below it, never under it.
    let tops: Vec<&PageElement> = page
        .elements
        .iter()
        .filter(|e| pin_of(e) == Some("top"))
        .collect();
    let top_heights: Vec<u32> = tops.iter().map(|e| p.measure(e, width, None)).collect();
    let header = top_heights.iter().sum::<u32>().min(height);
    y += header as i32;
    side_y += header as i32;
    form_y += header as i32;
    for e in &page.elements {
        if pinned(e) {
            continue;
        }
        if !themed && matches!(e,PageElement::Heading{text,..}if text==&page.title) {
            continue;
        }
        if sidebar > 0
            && matches!(page.title.as_str(), "Chat" | "Documents")
            && matches!(e, PageElement::Link { .. })
        {
            p.element(e, 12, &mut side_y, sidebar - 24);
            continue;
        }
        if !themed
            && matches!(page.title.as_str(), "Mail" | "Calendar")
            && matches!(e,PageElement::Form{id,..}if id=="compose"||id=="create")
        {
            if two_column {
                p.element(e, x + mainw as i32 + 24, &mut form_y, 262);
            } else {
                deferred.push(e);
            }
            continue;
        }
        if page.title == "Mail" && special && matches!(e, PageElement::Heading { .. }) {
            p.decor(
                Rect::new(x - 10, y - 9, mainw + 20, 39),
                Color::WHITE,
                6,
                Some(BORDER),
            );
        }
        if page.title == "Calendar" && special && matches!(e, PageElement::Heading { .. }) {
            p.decor(Rect::new(x - 9, y - 5, 3, 30), accent, 1, None);
        }
        p.element(e, x, &mut y, mainw);
    }
    for e in deferred {
        y += 18;
        p.element(e, x, &mut y, mainw);
    }
    // Pinned bars span the viewport on its top or bottom edge, stacked in page order,
    // and the page under them is clipped away so a click on a bar never reaches what
    // it covers.
    let bars: Vec<&PageElement> = page
        .elements
        .iter()
        .filter(|e| pin_of(e) == Some("bottom"))
        .collect();
    // Everything the page holds, unscrolled: the flowed columns (which already start
    // below the header) plus the bars pinned over its bottom edge, which the last row
    // must be able to scroll clear of.
    let mut extent = (y.max(side_y).max(form_y) + scroll).max(0) as u32 + 16;
    if !bars.is_empty() || !tops.is_empty() {
        let heights: Vec<u32> = bars.iter().map(|e| p.measure(e, width, None)).collect();
        let total = heights
            .iter()
            .sum::<u32>()
            .min(height.saturating_sub(header));
        extent += total;
        let edge = height.saturating_sub(total);
        // Content wholly behind the bars keeps a one-row clip strip between them rather
        // than none, so it stays in the page (reachable by scrolling) yet paints nothing
        // there.
        let visible = Rect::new(0, header as i32, width, edge.saturating_sub(header).max(1));
        for node in &mut p.scene.nodes {
            node.clip = Some(
                node.clip
                    .unwrap_or(visible)
                    .intersection(visible)
                    .unwrap_or(Rect::new(0, header as i32, 1, 1)),
            );
        }
        let mut ty = 0;
        for (e, h) in tops.into_iter().zip(top_heights) {
            p.place(e, 0, ty, width, None);
            ty += h as i32;
        }
        let mut by = edge as i32;
        for (e, h) in bars.into_iter().zip(heights) {
            p.place(e, 0, by, width, None);
            by += h as i32;
        }
    }
    if special {
        for node in &mut p.scene.nodes {
            node.clip = Some(Rect::new(0, 64, width, height.saturating_sub(64)));
        }
        p.decor(Rect::new(0, 0, width, 64), Color::WHITE, 0, None);
        p.decor(Rect::new(0, 63, width, 1), BORDER, 0, None);
        let icon = match page.title.as_str() {
            "Mail" => "mail",
            "Chat" => "messages",
            "Calendar" => "calendar",
            _ => "editor",
        };
        p.node(
            2,
            Rect::new(20, 16, 30, 30),
            Primitive::AssetImage {
                asset: format!("icon/common/{icon}"),
            },
            None,
            None,
        );
        p.caption(
            Rect::new(62, 22, width.saturating_sub(80), 26),
            &page.title,
            20,
            INK,
            true,
        );
    }
    p.scene.scrolls.push(cw_scene::ScrollArea {
        target: "pane:page".into(),
        window: None,
        bounds: Rect::new(0, 0, width, height),
        offset: scroll,
        extent,
        title: None,
        title_height: 0,
        horizontal: false,
    });
    p.scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use cw_protocol::{PageAction, PageTheme};
    fn scene(page: &Page, width: u32) -> Scene {
        layout(page, &BTreeMap::new(), &BTreeMap::new(), width, 700, 0)
    }
    fn styled(id: &str, style: Style) -> PageElement {
        PageElement::Styled {
            id: id.into(),
            text: "cell".into(),
            style,
        }
    }
    #[test]
    fn an_icon_is_a_labelled_button_over_a_tinted_symbol() {
        let mut page = Page::new("Player");
        page.elements = vec![PageElement::Icon {
            id: "like".into(),
            name: "thumb-up".into(),
            label: "Like".into(),
            style: Style::default().size(24).padding(8).color("#ff0000"),
            action: Some(PageAction {
                method: "POST".into(),
                url: "/items/x/like".into(),
                fields: BTreeMap::new(),
            }),
        }];
        page.validate().unwrap();
        let s = scene(&page, 400);
        let button = find(&s, "like");
        assert_eq!((button.bounds.width, button.bounds.height), (40, 40));
        let semantic = button.semantic.as_ref().unwrap();
        assert_eq!(
            (semantic.role.as_str(), semantic.label.as_str()),
            ("button", "Like")
        );
        let glyph = s
            .nodes
            .iter()
            .find(|n| matches!(&n.primitive, Primitive::Symbol { asset, .. } if asset == "symbol/thumb-up"))
            .expect("the glyph is drawn");
        assert_eq!(
            glyph.bounds,
            Rect::new(button.bounds.x + 8, button.bounds.y + 8, 24, 24)
        );
        assert!(
            matches!(glyph.primitive, Primitive::Symbol { color, .. } if color == Color::rgb(255, 0, 0))
        );
        // Without an action it is a picture with the same name, and no click target.
        if let PageElement::Icon { action, .. } = &mut page.elements[0] {
            *action = None;
        }
        let s = scene(&page, 400);
        assert!(s.nodes.iter().all(|n| n.interaction.is_none()));
        assert!(s.nodes.iter().any(|n| n
            .semantic
            .as_ref()
            .is_some_and(|m| m.role == "img" && m.label == "Like")));
    }
    fn find<'a>(scene: &'a Scene, action: &str) -> &'a Node {
        scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some(action))
            .unwrap()
    }
    #[test]
    fn page_language_and_italic_reach_the_text_primitives() {
        let mut page = Page::new("記事");
        page.lang = Some("ja-JP".into());
        page.elements = vec![
            PageElement::Heading {
                id: "h".into(),
                text: "骨の話".into(),
                level: 1,
            },
            PageElement::Text {
                id: "t".into(),
                text: "直次".into(),
            },
            PageElement::Styled {
                id: "quote".into(),
                text: "an italic pull quote that is long enough to wrap".into(),
                style: Style::default().italic(),
            },
            PageElement::Styled {
                id: "tc".into(),
                text: "骨".into(),
                style: Style::default().lang("zh-TW").bold(),
            },
        ];
        let scene_of = |page: &Page| scene(page, 220);
        let scene = scene_of(&page);
        let style_of = |text: &str| {
            scene
                .nodes
                .iter()
                .find(|n| n.painted_text().is_some_and(|t| t.contains(text)))
                .and_then(|n| n.primitive.text_style())
                .unwrap()
        };
        assert_eq!(style_of("骨の話"), TextStyle::new(true, false, Lang::Ja));
        assert_eq!(style_of("直次"), TextStyle::new(false, false, Lang::Ja));
        assert_eq!(style_of("italic"), TextStyle::new(false, true, Lang::Ja));
        assert_eq!(style_of("骨"), TextStyle::new(true, false, Lang::Ja));
        let tc = scene
            .nodes
            .iter()
            .find(|n| n.painted_text() == Some("骨"))
            .unwrap();
        assert_eq!(
            tc.primitive.text_style(),
            Some(TextStyle::new(true, false, Lang::ZhHant))
        );
        // The italic block wraps with italic metrics.
        let quote = scene
            .nodes
            .iter()
            .find(|n| n.painted_text().is_some_and(|t| t.contains("italic")))
            .unwrap();
        for line in quote.painted_text().unwrap().lines() {
            assert!(
                metrics::text_width(FACE, TextStyle::new(false, true, Lang::Ja), line, 13)
                    <= quote.bounds.width
            );
        }
        // Scene JSON only carries the new fields where they are set.
        let json = serde_json::to_string(&scene).unwrap();
        assert!(json.contains(r#""lang":"ja""#) && json.contains(r#""italic":true"#));
        let plain = scene_of(&Page::new("p"));
        assert!(!serde_json::to_string(&plain).unwrap().contains("lang"));
    }
    #[test]
    fn mail_uses_one_title_and_fields_remain_hit_testable() {
        let mut page = Page::new("Mail");
        page.elements = vec![
            PageElement::Heading {
                id: "title".into(),
                text: "Mail".into(),
                level: 1,
            },
            PageElement::Form {
                id: "compose".into(),
                action: PageAction {
                    method: "POST".into(),
                    url: "/send".into(),
                    fields: BTreeMap::new(),
                },
                children: vec![PageElement::Input {
                    id: "compose-to".into(),
                    label: "Recipients".into(),
                    value: "alice".into(),
                    placeholder: String::new(),
                }],
            },
            PageElement::Heading {
                id: "message".into(),
                text: "Actual subject".into(),
                level: 1,
            },
        ];
        let scene = scene(&page, 1100);
        scene.validate().unwrap();
        assert_eq!(
            scene
                .nodes
                .iter()
                .filter(|n| matches!(&n.primitive,Primitive::UiTextBold{text,..}if text=="Mail"))
                .count(),
            1
        );
        let input = find(&scene, "compose-to");
        assert!(input.bounds.x > 700);
        assert_eq!(
            scene
                .hit_test(input.bounds.x + 3, input.bounds.y + 3)
                .unwrap()
                .interaction
                .as_deref(),
            Some("compose-to")
        );
        assert_eq!(
            input.semantic.as_ref().unwrap().value.as_deref(),
            Some("alice")
        );
    }
    #[test]
    fn document_links_are_real_sidebar_controls_and_header_masks_scroll() {
        let mut page = Page::new("Documents");
        page.elements.push(PageElement::Link {
            id: "doc".into(),
            text: "Project plan".into(),
            url: "/documents/doc".into(),
            style: None,
        });
        let scene = layout(&page, &BTreeMap::new(), &BTreeMap::new(), 900, 500, 100);
        assert!(scene
            .nodes
            .iter()
            .any(|n| n.interaction.as_deref() == Some("doc")));
        assert!(scene.hit_test(20, 30).is_none());
        assert_eq!(
            scene,
            layout(&page, &BTreeMap::new(), &BTreeMap::new(), 900, 500, 100)
        );
    }
    #[test]
    fn row_gives_fixed_children_their_width_and_splits_the_rest_by_flex() {
        let mut page = Page::new("Feed");
        page.theme = Some(PageTheme {
            content_width: Some(640),
            ..PageTheme::default()
        });
        page.elements = vec![PageElement::Row {
            id: "bar".into(),
            children: vec![
                PageElement::Thumbnail {
                    id: "avatar".into(),
                    label: "A".into(),
                    style: Style::default().width(48).height(48),
                    action: None,
                },
                PageElement::Thumbnail {
                    id: "one".into(),
                    label: "one".into(),
                    style: Style::default().height(40).flex(1),
                    action: None,
                },
                PageElement::Thumbnail {
                    id: "three".into(),
                    label: "three".into(),
                    style: Style::default().height(40).flex(3),
                    action: None,
                },
            ],
            gap: 10,
            align: "center".into(),
            style: Style::default(),
        }];
        page.validate().unwrap();
        let scene = scene(&page, 1000);
        let bounds = |label: &str| {
            scene
                .nodes
                .iter()
                .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == label))
                .unwrap()
                .bounds
        };
        let (avatar, one, three) = (bounds("A"), bounds("one"), bounds("three"));
        // 640 column - 48 fixed - 2 gaps of 10 = 572, split 1:3.
        assert_eq!(avatar.width, 48);
        assert_eq!(one.width, 143);
        assert_eq!(three.width, 429);
        assert_eq!(one.x, avatar.x + 58);
        assert_eq!(three.x, one.x + 153);
        // The centred column starts halfway into the surplus viewport width.
        assert_eq!(avatar.x, 16 + (968 - 640) / 2);
        // "center" puts the shorter children on the row's mid-line.
        assert_eq!(avatar.y, three.y - 4);
    }
    #[test]
    fn grid_wraps_row_major_into_equal_columns() {
        let mut page = Page::new("Grid");
        page.theme = Some(PageTheme::default());
        page.elements = vec![PageElement::Grid {
            id: "tiles".into(),
            columns: 3,
            children: (0..5)
                .map(|i| styled(&format!("c{i}"), Style::default().height(30)))
                .collect(),
            gap: 12,
            style: Style::default(),
        }];
        page.validate().unwrap();
        let scene = scene(&page, 632);
        let cells: Vec<Rect> = scene
            .nodes
            .iter()
            .filter(|n| n.semantic.as_ref().is_some_and(|s| s.role == "text"))
            .map(|n| n.bounds)
            .collect();
        assert_eq!(cells.len(), 5);
        // 600 usable - 2 gaps of 12 = 576, three 192px columns.
        assert_eq!(cells[0].width, 192);
        assert_eq!(cells[1].x - cells[0].x, 204);
        assert_eq!(cells[2].x - cells[0].x, 408);
        assert_eq!(cells[3].x, cells[0].x);
        assert_eq!(cells[3].y - cells[0].y, 30 + 6 + 12);
        assert_eq!(cells[4].x, cells[1].x);
    }
    #[test]
    fn only_cards_and_thumbnails_with_actions_are_controls() {
        let mut page = Page::new("Results");
        page.theme = Some(PageTheme::default());
        page.elements = vec![
            PageElement::Card {
                id: "hit".into(),
                children: vec![styled("hit-title", Style::default().size(16).bold())],
                style: Style::default().background("#ffffff").border("#e1e5eb"),
                action: Some(PageAction {
                    method: "GET".into(),
                    url: "/r/1".into(),
                    fields: BTreeMap::new(),
                }),
            },
            PageElement::Card {
                id: "inert".into(),
                children: vec![styled("inert-title", Style::default())],
                style: Style::default(),
                action: None,
            },
            PageElement::Thumbnail {
                id: "still".into(),
                label: "Still".into(),
                style: Style::default(),
                action: None,
            },
        ];
        page.validate().unwrap();
        let scene = scene(&page, 900);
        scene.validate().unwrap();
        let card = find(&scene, "hit");
        let semantic = card.semantic.as_ref().unwrap();
        assert_eq!(semantic.role, "link");
        assert_eq!(semantic.label, "cell");
        assert!(semantic.focusable);
        assert_eq!(
            scene
                .hit_test(card.bounds.x + 4, card.bounds.y + 4)
                .unwrap()
                .interaction
                .as_deref(),
            Some("hit")
        );
        assert!(!scene
            .nodes
            .iter()
            .any(|n| matches!(n.interaction.as_deref(), Some("inert") | Some("still"))));
        let still = scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == "Still"))
            .unwrap();
        assert_eq!(still.semantic.as_ref().unwrap().role, "img");
        assert!(!still.semantic.as_ref().unwrap().focusable);
    }
    #[test]
    fn theme_colours_the_surface_and_layout_stays_reproducible() {
        let mut page = Page::new("Dark");
        page.theme = Some(PageTheme {
            background: Some("#101318".into()),
            accent: Some("#4f8cff".into()),
            ink: Some("#f2f4f8".into()),
            ..PageTheme::default()
        });
        page.elements = vec![
            PageElement::Badge {
                id: "live".into(),
                text: "LIVE".into(),
                style: Style::default(),
            },
            PageElement::Styled {
                id: "blurb".into(),
                text: "centred copy ".repeat(40),
                style: Style::default().align("center"),
            },
        ];
        page.validate().unwrap();
        let scene = scene(&page, 800);
        assert_eq!(scene.background, Color::rgb(16, 19, 24));
        let badge = scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == "LIVE"))
            .unwrap();
        assert!(badge.interaction.is_none());
        assert!(matches!(
            badge.primitive,
            Primitive::RoundedBox {
                fill: Color(79, 140, 255, 255),
                ..
            }
        ));
        // Aligned text emits a node per line; only the first is a semantic block.
        scene.validate().unwrap();
        assert_eq!(
            scene
                .nodes
                .iter()
                .filter(|n| n
                    .semantic
                    .as_ref()
                    .is_some_and(|s| s.label.starts_with("centred")))
                .count(),
            1
        );
        assert_eq!(scene, self::scene(&page, 800));
    }
    fn node_by_label<'a>(scene: &'a Scene, label: &str) -> &'a Node {
        scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == label))
            .unwrap_or_else(|| panic!("no node labelled {label}"))
    }
    fn link(id: &str, text: &str) -> PageElement {
        PageElement::Link {
            id: id.into(),
            text: text.into(),
            url: format!("/{id}"),
            style: None,
        }
    }
    #[test]
    fn a_row_too_narrow_for_its_children_wraps_instead_of_crushing_them() {
        let mut page = Page::new("Nav");
        page.elements = vec![PageElement::Row {
            id: "nav".into(),
            children: ["Product", "Pricing", "Careers", "Contact", "Documentation"]
                .iter()
                .map(|t| link(&t.to_lowercase(), t))
                .collect(),
            gap: 12,
            align: "center".into(),
            style: Style::default(),
        }];
        page.validate().unwrap();
        // Wide: one line, every link on the same baseline.
        let wide = scene(&page, 1000);
        let tops: BTreeSet<i32> = ["Product", "Documentation"]
            .iter()
            .map(|l| node_by_label(&wide, l).bounds.y)
            .collect();
        assert_eq!(tops.len(), 1);
        // Phone: more than one line, and no link narrower than its own text. (Bare links
        // are only as wide as their words, so a 390 px phone still fits this nav.)
        let narrow = scene(&page, 300);
        let rows: BTreeSet<i32> = ["Product", "Pricing", "Careers", "Contact", "Documentation"]
            .iter()
            .map(|l| node_by_label(&narrow, l).bounds.y)
            .collect();
        assert!(rows.len() > 1, "the row did not wrap");
        for label in ["Product", "Documentation"] {
            let width = node_by_label(&narrow, label).bounds.width;
            assert!(
                width >= metrics::text_width(FACE, false, label, 13),
                "{label} was crushed to {width}"
            );
        }
    }
    #[test]
    fn a_grid_drops_columns_on_a_phone() {
        let mut page = Page::new("Tiles");
        page.elements = vec![PageElement::Grid {
            id: "tiles".into(),
            columns: 4,
            children: (0..4)
                .map(|i| PageElement::Thumbnail {
                    id: format!("t{i}"),
                    label: format!("tile {i}"),
                    style: Style::default().height(60),
                    action: None,
                })
                .collect(),
            gap: 10,
            style: Style::default(),
        }];
        page.validate().unwrap();
        let tops = |width| {
            let scene = scene(&page, width);
            (0..4)
                .map(|i| node_by_label(&scene, &format!("tile {i}")).bounds.y)
                .collect::<BTreeSet<i32>>()
                .len()
        };
        assert_eq!(tops(1000), 1, "a desktop keeps its four columns");
        assert!(
            tops(390) > 1,
            "a phone still squeezes four tiles into a line"
        );
    }
    #[test]
    fn a_badge_that_only_colours_its_text_is_a_label_and_an_empty_one_draws_nothing() {
        let badge = |id: &str, text: &str, style: Style| PageElement::Badge {
            id: id.into(),
            text: text.into(),
            style,
        };
        let mut page = Page::new("Badges");
        page.elements = vec![
            badge("pill", "LIVE", Style::default()),
            badge("count", "4", Style::default().color("#5f6368")),
            badge(
                "outline",
                "Two-day",
                Style::default().border("#e47911").color("#e47911"),
            ),
            badge("none", "", Style::default().color("#5f6368")),
        ];
        page.validate().unwrap();
        let scene = scene(&page, 800);
        let fill = |label: &str| match &node_by_label(&scene, label).primitive {
            Primitive::RoundedBox { fill, .. } => *fill,
            other => panic!("{other:?}"),
        };
        assert_ne!(
            fill("LIVE"),
            Color::TRANSPARENT,
            "the default badge is a pill"
        );
        assert_eq!(fill("4"), Color::TRANSPARENT);
        assert_eq!(fill("Two-day"), Color::TRANSPARENT);
        assert!(!scene.nodes.iter().any(|n| n
            .semantic
            .as_ref()
            .is_some_and(|s| s.role == "text" && s.label.is_empty())));
    }
    #[test]
    fn a_long_link_wraps_and_a_short_one_is_bare_text_at_its_own_width() {
        let mut page = Page::new("Links");
        let title = "Show HN: Atlas, a simulated machine you can replay byte for byte";
        page.elements = vec![link("short", "Home"), link("long", title)];
        page.validate().unwrap();
        let scene = scene(&page, 260);
        let home = node_by_label(&scene, "Home");
        assert_eq!(home.bounds.height, 17);
        assert_eq!(
            home.bounds.width,
            metrics::text_width(FACE, false, "Home", 13)
        );
        assert!(
            matches!(&home.primitive, Primitive::UiText { color, .. } if *color == Color::rgb(34, 112, 205))
        );
        assert!(!scene
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive, Primitive::RoundedBox { fill, .. } if *fill == Color::rgb(242, 246, 252))));
        assert!(node_by_label(&scene, title).bounds.height > 17);
    }
    fn action(url: &str) -> PageAction {
        PageAction {
            method: "GET".into(),
            url: url.into(),
            fields: BTreeMap::new(),
        }
    }
    #[test]
    fn styled_links_and_buttons_take_their_style_and_their_own_width() {
        let mut page = Page::new("Nav");
        page.elements = vec![
            PageElement::Link {
                id: "tab".into(),
                text: "Pull requests".into(),
                url: "/pulls".into(),
                style: Some(
                    Style::default()
                        .size(14)
                        .bold()
                        .color("#ffffff")
                        .background("#24292f")
                        .border("#57606a")
                        .radius(4)
                        .padding(6),
                ),
            },
            PageElement::Button {
                id: "merge".into(),
                text: "Merge".into(),
                action: action("/merge"),
                style: Some(
                    Style::default()
                        .background("#2da44e")
                        .color("#ffffff")
                        .width(200)
                        .radius(3),
                ),
            },
            PageElement::Button {
                id: "plain".into(),
                text: "Save".into(),
                action: action("/save"),
                style: None,
            },
            PageElement::Link {
                id: "sha".into(),
                text: "a1b2c3d".into(),
                url: "/commit/a1b2c3d".into(),
                style: Some(Style::default().size(12).mono().color("#0969da")),
            },
        ];
        page.validate().unwrap();
        let scene = scene(&page, 600);
        let tab = find(&scene, "tab");
        let tw = metrics::text_width(FACE, true, "Pull requests", 14);
        assert_eq!(tab.bounds.width, tw + 12);
        assert_eq!(tab.bounds.height, line_height(14) + 12);
        assert!(
            matches!(&tab.primitive, Primitive::RoundedBox { fill, border: Some(_), radius: 4, .. } if *fill == Color::rgb(0x24, 0x29, 0x2f))
        );
        assert_eq!(tab.semantic.as_ref().unwrap().role, "link");
        let merge = find(&scene, "merge");
        assert_eq!(merge.bounds.width, 200);
        assert!(
            matches!(&merge.primitive, Primitive::RoundedBox { fill, radius: 3, .. } if *fill == Color::rgb(0x2d, 0xa4, 0x4e))
        );
        let plain = find(&scene, "plain");
        assert_eq!(plain.bounds.height, 34);
        assert_eq!(
            plain.bounds.width,
            metrics::text_width(FACE, true, "Save", 12) + 28
        );
        assert!(
            matches!(&plain.primitive, Primitive::RoundedBox { fill, .. } if *fill == Color::rgb(34, 112, 205))
        );
        let sha = find(&scene, "sha");
        assert!(matches!(&sha.primitive, Primitive::Text { size: 12, .. }));
        assert_eq!(sha.bounds.width, cw_scene::text_cell(12).0 * 7);
        // Deterministic, and the scaled layout keeps the monospace text.
        assert_eq!(scene, self::scene(&page, 600));
    }
    #[test]
    fn rows_justify_chips_and_give_flexing_siblings_the_rest() {
        let chip = |id: &str, text: &str| PageElement::Badge {
            id: id.into(),
            text: text.into(),
            style: Style::default().padding(8),
        };
        let mut page = Page::new("Chips");
        page.elements = vec![
            PageElement::Row {
                id: "between".into(),
                children: vec![chip("a", "Code"), chip("b", "Issues"), chip("c", "Pulls")],
                gap: 8,
                align: "center".into(),
                style: Style::default().justify("space-between"),
            },
            PageElement::Row {
                id: "end".into(),
                children: vec![chip("d", "Code"), chip("e", "Issues")],
                gap: 8,
                align: "center".into(),
                style: Style::default().justify("end"),
            },
            PageElement::Row {
                id: "flex".into(),
                children: vec![
                    chip("f", "Filter"),
                    styled("search", Style::default().flex(1)),
                    chip("g", "New"),
                ],
                gap: 8,
                align: "center".into(),
                style: Style::default(),
            },
        ];
        page.validate().unwrap();
        let scene = scene(&page, 600);
        let at = |label: &str| node_by_label(&scene, label).bounds;
        let width = |text: &str| metrics::text_width(FACE, true, text, 10) + 18;
        assert_eq!(at("Code").width, width("Code"));
        assert_eq!(at("Code").x, 16);
        assert_eq!(at("Pulls").x + at("Pulls").width as i32, 16 + 568);
        let mid = at("Issues");
        assert!(mid.x > at("Code").x + at("Code").width as i32 + 8);
        let end: Vec<&Node> = scene
            .nodes
            .iter()
            .filter(|n| n.semantic.as_ref().is_some_and(|s| s.label == "Issues"))
            .collect();
        assert_eq!(end[1].bounds.x + end[1].bounds.width as i32, 16 + 568);
        let f = at("Filter");
        let g = at("New");
        let search = find_text(&scene, "cell");
        assert_eq!(f.width, width("Filter"));
        assert_eq!(g.width, width("New"));
        assert_eq!(search.bounds.width, 568 - f.width - g.width - 16);
    }
    fn find_text<'a>(scene: &'a Scene, label: &str) -> &'a Node {
        scene
            .nodes
            .iter()
            .find(|n| n.semantic.as_ref().is_some_and(|s| s.label == label))
            .unwrap()
    }
    #[test]
    fn a_top_pinned_header_stays_put_and_the_page_flows_below_it() {
        let mut page = Page::new("App");
        page.elements = vec![
            PageElement::Row {
                id: "header".into(),
                children: vec![styled("brand", Style::default().bold())],
                gap: 0,
                align: "center".into(),
                style: Style::default()
                    .pin("top")
                    .background("#1f2328")
                    .padding(12),
            },
            PageElement::Text {
                id: "body".into(),
                text: "first paragraph".into(),
            },
        ];
        page.validate().unwrap();
        let unscrolled = layout(&page, &BTreeMap::new(), &BTreeMap::new(), 400, 300, 0);
        let scrolled = layout(&page, &BTreeMap::new(), &BTreeMap::new(), 400, 300, 40);
        let header = |s: &Scene| {
            s.nodes
                .iter()
                .find(|n| matches!(&n.primitive, Primitive::RoundedBox { fill, .. } if *fill == Color::rgb(0x1f, 0x23, 0x28)))
                .unwrap()
                .bounds
        };
        assert_eq!(header(&unscrolled).y, 0);
        assert_eq!(header(&scrolled).y, 0, "the header scrolls with nothing");
        let body = node_by_label(&unscrolled, "first paragraph").bounds;
        assert!(body.y >= header(&unscrolled).height as i32);
        let moved = node_by_label(&scrolled, "first paragraph").bounds;
        assert_eq!(moved.y, body.y - 40);
        // What scrolls under the header is clipped away, never painted over it.
        let clip = node_by_label(&scrolled, "first paragraph").clip.unwrap();
        assert_eq!(clip.y, header(&scrolled).height as i32);
    }
    #[test]
    fn a_styled_image_is_rounded_and_can_be_a_link() {
        let mut page = Page::new("Profile");
        page.elements = vec![PageElement::Image {
            id: "avatar".into(),
            source: "/avatar.rgba".into(),
            alt: "Ada Lovelace".into(),
            width: 40,
            height: 40,
            style: Some(Style::default().radius(20)),
            action: Some(action("/users/ada")),
        }];
        page.validate().unwrap();
        let images = BTreeMap::from([(
            "avatar".to_string(),
            Arc::new(ImageAsset {
                width: 40,
                height: 40,
                rgba: vec![255; 40 * 40 * 4],
            }),
        )]);
        let scene = layout(&page, &BTreeMap::new(), &images, 400, 300, 0);
        let picture = find(&scene, "avatar");
        assert_eq!(picture.rounded_clip.as_ref().map(|c| c.radius), Some(20));
        let semantic = picture.semantic.as_ref().unwrap();
        assert_eq!(
            (semantic.role.as_str(), semantic.label.as_str()),
            ("link", "Ada Lovelace")
        );
        assert!(semantic.focusable);
    }
    #[test]
    fn a_search_form_is_a_field_and_a_search_button() {
        let action = PageAction {
            method: "POST".into(),
            url: "/results".into(),
            fields: Default::default(),
        };
        let mut page = Page::new("Search");
        page.elements = vec![PageElement::Form {
            id: "search".into(),
            action: action.clone(),
            children: vec![
                PageElement::Input {
                    id: "search-q".into(),
                    label: "Search".into(),
                    value: String::new(),
                    placeholder: String::new(),
                },
                PageElement::Button {
                    id: "search-submit".into(),
                    text: "Submit".into(),
                    action,
                    style: None,
                },
            ],
        }];
        page.validate().unwrap();
        let scene = scene(&page, 800);
        let texts: Vec<String> = scene
            .nodes
            .iter()
            .filter_map(|n| match &n.primitive {
                Primitive::UiText { text, .. } | Primitive::UiTextBold { text, .. } => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        assert!(!texts.iter().any(|t| t == "Update"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Search"), "{texts:?}");
        assert!(!texts.iter().any(|t| t == "Save changes"), "{texts:?}");
    }
}
