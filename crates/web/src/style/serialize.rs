//! `ComputedStyle::serialize(property)`: the string `getComputedStyle` would return
//! for every longhand, in Chromium's forms (`16px`, `rgb(0, 0, 0)`,
//! `rgba(0, 0, 0, 0.5)`, keywords lower-cased, lists comma-separated). The parity
//! harness compares these strings.

use super::computed::*;
use super::properties::LonghandId;
use super::values::serialize_component_values;
use crate::geom::Au;
use cw_scene::Color;

/// `Au` as decimal px, exact (multiples of 1/64 need at most six digits), trimmed.
pub fn au_to_string(a: Au) -> String {
    let neg = a.0 < 0;
    let abs = a.0.unsigned_abs() as u64;
    let int = abs / 64;
    let frac = abs % 64;
    let mut s = String::new();
    if neg && abs != 0 {
        s.push('-');
    }
    s.push_str(&int.to_string());
    if frac != 0 {
        let mut f = format!("{:06}", frac * 1_000_000 / 64);
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
    }
    s
}

pub fn px(a: Au) -> String {
    format!("{}px", au_to_string(a))
}

/// A myriad percentage (5000 is `50%`).
pub fn percent(p: i32) -> String {
    format!("{}%", milli_to_string(p as i64 * 10, 1000))
}

/// A fixed-point value with `den` units per one, trimmed.
fn milli_to_string(v: i64, den: i64) -> String {
    let neg = v < 0;
    let abs = v.unsigned_abs();
    let int = abs / den as u64;
    let frac = abs % den as u64;
    let mut s = String::new();
    if neg && abs != 0 {
        s.push('-');
    }
    s.push_str(&int.to_string());
    if frac != 0 {
        let digits = den.to_string().len() - 1;
        let mut f = format!("{:0width$}", frac, width = digits);
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
    }
    s
}

pub fn lp(v: LengthPercentage) -> String {
    match v {
        LengthPercentage::Length(l) => px(l),
        LengthPercentage::Percent(p) => percent(p),
        LengthPercentage::Calc(l, p) => {
            if l.0 < 0 {
                format!("calc({} - {})", percent(p), px(-l))
            } else {
                format!("calc({} + {})", percent(p), px(l))
            }
        }
        LengthPercentage::Clamp { lo, v, hi } => {
            let part = |(l, p): (Au, i32)| lp(if p == 0 { LengthPercentage::Length(l) } else { LengthPercentage::Calc(l, p) });
            match (lo, hi) {
                (Some(lo), Some(hi)) => format!("clamp({}, {}, {})", part(lo), part(v), part(hi)),
                (Some(lo), None) => format!("max({}, {})", part(lo), part(v)),
                (None, Some(hi)) => format!("min({}, {})", part(v), part(hi)),
                (None, None) => part(v),
            }
        }
    }
}

pub fn lpa(v: LengthPercentageAuto) -> String {
    match v {
        LengthPercentageAuto::Auto => "auto".into(),
        LengthPercentageAuto::Set(l) => lp(l),
    }
}

pub fn sizing(v: Sizing) -> String {
    match v {
        Sizing::Auto => "auto".into(),
        Sizing::None => "none".into(),
        Sizing::Set(l) => lp(l),
        Sizing::MinContent => "min-content".into(),
        Sizing::MaxContent => "max-content".into(),
        Sizing::FitContent => "fit-content".into(),
    }
}

/// Chromium's colour form: `rgb(r, g, b)` when opaque, else `rgba(r, g, b, a)`.
pub fn color(c: Color) -> String {
    if c.3 == 255 {
        format!("rgb({}, {}, {})", c.0, c.1, c.2)
    } else if c.3 == 0 {
        "rgba(0, 0, 0, 0)".into()
    } else {
        format!("rgba({}, {}, {}, {})", c.0, c.1, c.2, alpha(c.3))
    }
}

/// An 8-bit alpha as CSSOM serialises it: the shortest decimal that rounds back to
/// the same byte. `rgb(0 0 0 / 0.1)` stores 26, and 26/255 is 0.10196…, but the
/// shortest string that still stores 26 is `0.1`, which is what a browser prints.
fn alpha(a: u8) -> String {
    for den in [10i64, 100, 1000] {
        let v = (a as i64 * den + 127) / 255;
        if (v * 255 + den / 2) / den == a as i64 {
            return milli_to_string(v, den);
        }
    }
    milli_to_string((a as i64 * 1000 + 127) / 255, 1000)
}

fn border_style(s: BorderStyle) -> &'static str {
    match s {
        BorderStyle::None => "none",
        BorderStyle::Hidden => "hidden",
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Double => "double",
        BorderStyle::Groove => "groove",
        BorderStyle::Ridge => "ridge",
        BorderStyle::Inset => "inset",
        BorderStyle::Outset => "outset",
    }
}

pub fn display(d: Display) -> &'static str {
    match d {
        Display::Inline => "inline",
        Display::Block => "block",
        Display::InlineBlock => "inline-block",
        Display::ListItem => "list-item",
        Display::Flex => "flex",
        Display::InlineFlex => "inline-flex",
        Display::Grid => "grid",
        Display::InlineGrid => "inline-grid",
        Display::Table => "table",
        Display::InlineTable => "inline-table",
        Display::TableRowGroup => "table-row-group",
        Display::TableHeaderGroup => "table-header-group",
        Display::TableFooterGroup => "table-footer-group",
        Display::TableRow => "table-row",
        Display::TableCell => "table-cell",
        Display::TableColumnGroup => "table-column-group",
        Display::TableColumn => "table-column",
        Display::TableCaption => "table-caption",
        Display::FlowRoot => "flow-root",
        Display::Contents => "contents",
        Display::None => "none",
    }
}

fn overflow(o: Overflow) -> &'static str {
    match o {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
        Overflow::Clip => "clip",
        Overflow::Scroll => "scroll",
        Overflow::Auto => "auto",
    }
}

pub fn list_style_type(t: ListStyleType) -> &'static str {
    match t {
        ListStyleType::Disc => "disc",
        ListStyleType::Circle => "circle",
        ListStyleType::Square => "square",
        ListStyleType::Decimal => "decimal",
        ListStyleType::DecimalLeadingZero => "decimal-leading-zero",
        ListStyleType::LowerAlpha => "lower-alpha",
        ListStyleType::UpperAlpha => "upper-alpha",
        ListStyleType::LowerRoman => "lower-roman",
        ListStyleType::UpperRoman => "upper-roman",
        ListStyleType::None => "none",
    }
}

fn gradient_stops(stops: &[GradientStop]) -> String {
    stops
        .iter()
        .map(|s| match s.position {
            Some(p) => format!("{} {}", color(s.color), lp(p)),
            None => color(s.color),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn image(i: &BackgroundImage) -> String {
    match i {
        BackgroundImage::None => "none".into(),
        BackgroundImage::Url(u) => format!("url(\"{u}\")"),
        BackgroundImage::LinearGradient { angle_centi_deg, stops } => format!("linear-gradient({}deg, {})", milli_to_string(*angle_centi_deg as i64, 100), gradient_stops(stops)),
        BackgroundImage::RadialGradient { circle, stops } => format!("radial-gradient({}{})", if *circle { "circle, " } else { "" }, gradient_stops(stops)),
    }
}

fn bg_box(b: BackgroundBox) -> &'static str {
    match b {
        BackgroundBox::PaddingBox => "padding-box",
        BackgroundBox::BorderBox => "border-box",
        BackgroundBox::ContentBox => "content-box",
    }
}

fn track_breadth(b: TrackBreadth) -> String {
    match b {
        TrackBreadth::Fixed(l) => lp(l),
        TrackBreadth::Flex(f) => format!("{}fr", milli_to_string(f as i64, 1000)),
        TrackBreadth::Auto => "auto".into(),
        TrackBreadth::MinContent => "min-content".into(),
        TrackBreadth::MaxContent => "max-content".into(),
    }
}

fn track_size(t: &TrackSize) -> String {
    match t {
        TrackSize::Fixed(l) => lp(*l),
        TrackSize::Flex(f) => format!("{}fr", milli_to_string(*f as i64, 1000)),
        TrackSize::Auto => "auto".into(),
        TrackSize::MinContent => "min-content".into(),
        TrackSize::MaxContent => "max-content".into(),
        TrackSize::MinMax(a, b) => format!("minmax({}, {})", track_breadth(*a), track_breadth(*b)),
        TrackSize::FitContent(l) => format!("fit-content({})", lp(*l)),
    }
}

fn line_names(n: &[String]) -> Option<String> {
    if n.is_empty() {
        None
    } else {
        Some(format!("[{}]", n.join(" ")))
    }
}

fn track_list(t: &TrackList) -> String {
    if t.tracks.is_empty() && t.auto_repeat.is_none() {
        return "none".into();
    }
    let mut parts: Vec<String> = Vec::new();
    for (i, tr) in t.tracks.iter().enumerate() {
        if let Some(n) = t.line_names.get(i).and_then(|n| line_names(n)) {
            parts.push(n);
        }
        if let Some(r) = &t.auto_repeat {
            if r.at == i {
                parts.push(auto_repeat(r));
            }
        }
        parts.push(track_size(tr));
    }
    if let Some(r) = &t.auto_repeat {
        if r.at >= t.tracks.len() {
            parts.push(auto_repeat(r));
        }
    }
    if let Some(n) = t.line_names.get(t.tracks.len()).and_then(|n| line_names(n)) {
        parts.push(n);
    }
    parts.join(" ")
}

fn auto_repeat(r: &AutoRepeat) -> String {
    let mut inner: Vec<String> = Vec::new();
    for (i, tr) in r.tracks.iter().enumerate() {
        if let Some(n) = r.line_names.get(i).and_then(|n| line_names(n)) {
            inner.push(n);
        }
        inner.push(track_size(tr));
    }
    if let Some(n) = r.line_names.get(r.tracks.len()).and_then(|n| line_names(n)) {
        inner.push(n);
    }
    format!("repeat({}, {})", if r.fill { "auto-fill" } else { "auto-fit" }, inner.join(" "))
}

fn grid_line(l: &GridLine) -> String {
    match l {
        GridLine::Auto => "auto".into(),
        GridLine::Line(n, None) => n.to_string(),
        GridLine::Line(n, Some(name)) => format!("{n} {name}"),
        GridLine::Span(n, None) => format!("span {n}"),
        GridLine::Span(n, Some(name)) => format!("span {n} {name}"),
        GridLine::Name(n) => n.clone(),
    }
}

fn align_items(a: AlignItems) -> &'static str {
    match a {
        AlignItems::Stretch => "stretch",
        AlignItems::FlexStart => "flex-start",
        AlignItems::FlexEnd => "flex-end",
        AlignItems::Center => "center",
        AlignItems::Baseline => "baseline",
        AlignItems::Start => "start",
        AlignItems::End => "end",
        AlignItems::SelfStart => "self-start",
        AlignItems::SelfEnd => "self-end",
    }
}

fn align_self(a: AlignSelf) -> &'static str {
    match a {
        AlignSelf::Auto => "auto",
        AlignSelf::Stretch => "stretch",
        AlignSelf::FlexStart => "flex-start",
        AlignSelf::FlexEnd => "flex-end",
        AlignSelf::Center => "center",
        AlignSelf::Baseline => "baseline",
        AlignSelf::Start => "start",
        AlignSelf::End => "end",
    }
}

fn timing(t: TimingFunction) -> String {
    match t {
        TimingFunction::Ease => "ease".into(),
        TimingFunction::Linear => "linear".into(),
        TimingFunction::EaseIn => "ease-in".into(),
        TimingFunction::EaseOut => "ease-out".into(),
        TimingFunction::EaseInOut => "ease-in-out".into(),
        TimingFunction::StepStart => "step-start".into(),
        TimingFunction::StepEnd => "step-end".into(),
        TimingFunction::CubicBezier(a, b, c, d) => format!("cubic-bezier({}, {}, {}, {})", milli_to_string(a as i64, 1000), milli_to_string(b as i64, 1000), milli_to_string(c as i64, 1000), milli_to_string(d as i64, 1000)),
        TimingFunction::Steps(n, start) => format!("steps({n}, {})", if start { "start" } else { "end" }),
    }
}

fn ms(t: i32) -> String {
    format!("{}s", milli_to_string(t as i64, 1000))
}

fn list<T>(items: &[T], f: impl Fn(&T) -> String) -> String {
    items.iter().map(f).collect::<Vec<_>>().join(", ")
}

fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\a "),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn content(c: &Content) -> String {
    match c {
        Content::Normal => "normal".into(),
        Content::None => "none".into(),
        Content::Items(items) => items
            .iter()
            .map(|i| match i {
                ContentItem::Text(t) => quote(t),
                ContentItem::Attr(a) => format!("attr({a})"),
                ContentItem::Counter(n, s) => {
                    if *s == ListStyleType::Decimal {
                        format!("counter({n})")
                    } else {
                        format!("counter({n}, {})", list_style_type(*s))
                    }
                }
                ContentItem::OpenQuote => "open-quote".into(),
                ContentItem::CloseQuote => "close-quote".into(),
                ContentItem::Url(u) => format!("url(\"{u}\")"),
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn counters(c: &[(String, i32)]) -> String {
    if c.is_empty() {
        "none".into()
    } else {
        c.iter().map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>().join(" ")
    }
}

fn shadow_common(x: Au, y: Au, blur: Au, spread: Option<Au>, c: Color, inset: bool) -> String {
    let mut s = format!("{} {} {} {}", color(c), px(x), px(y), px(blur));
    if let Some(sp) = spread {
        s.push(' ');
        s.push_str(&px(sp));
    }
    if inset {
        s.push_str(" inset");
    }
    s
}

/// A number in a `matrix()`, the way a browser prints one: at most six decimals,
/// trailing zeros trimmed, no `-0`.
fn matrix_number(v: f64) -> String {
    let r = (v * 1e6).round() / 1e6;
    let r = if r == 0.0 { 0.0 } else { r };
    let mut s = format!("{r:.6}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

fn transform(ops: &[TransformOp]) -> String {
    if ops.is_empty() {
        return "none".into();
    }
    // CSSOM §"resolved values": `transform` resolves to the composed matrix. A
    // percentage in `translate()` resolves against the border box, which the style
    // layer has not laid out, so such a list keeps its function form.
    let px_of = |v: LengthPercentage| match v {
        LengthPercentage::Length(l) => Some(l.to_f64_px()),
        _ => None,
    };
    // (a c e / b d f / 0 0 1), multiplied left to right as the functions apply.
    let mut m = [1.0f64, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut composable = true;
    for op in ops {
        let n = match *op {
            TransformOp::Translate(x, y) => match (px_of(x), px_of(y)) {
                (Some(x), Some(y)) => [1.0, 0.0, 0.0, 1.0, x, y],
                _ => {
                    composable = false;
                    break;
                }
            },
            TransformOp::Scale(x, y) => [x as f64 / 1000.0, 0.0, 0.0, y as f64 / 1000.0, 0.0, 0.0],
            TransformOp::Rotate(a) => {
                let r = (a as f64 / 100.0).to_radians();
                [r.cos(), r.sin(), -r.sin(), r.cos(), 0.0, 0.0]
            }
            TransformOp::SkewX(a) => [1.0, 0.0, (a as f64 / 100.0).to_radians().tan(), 1.0, 0.0, 0.0],
            TransformOp::SkewY(a) => [1.0, (a as f64 / 100.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
        };
        m = [
            m[0] * n[0] + m[2] * n[1],
            m[1] * n[0] + m[3] * n[1],
            m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3],
            m[0] * n[4] + m[2] * n[5] + m[4],
            m[1] * n[4] + m[3] * n[5] + m[5],
        ];
    }
    if composable {
        return format!("matrix({})", m.iter().map(|v| matrix_number(*v)).collect::<Vec<_>>().join(", "));
    }
    ops.iter()
        .map(|op| match op {
            TransformOp::Translate(x, y) => format!("translate({}, {})", lp(*x), lp(*y)),
            TransformOp::Scale(x, y) => format!("scale({}, {})", milli_to_string(*x as i64, 1000), milli_to_string(*y as i64, 1000)),
            TransformOp::Rotate(a) => format!("rotate({}deg)", milli_to_string(*a as i64, 100)),
            TransformOp::SkewX(a) => format!("skewX({}deg)", milli_to_string(*a as i64, 100)),
            TransformOp::SkewY(a) => format!("skewY({}deg)", milli_to_string(*a as i64, 100)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl ComputedStyle {
    /// The `getComputedStyle` string of a longhand (or a custom property), or
    /// `None` for a name that is not a longhand.
    pub fn serialize(&self, property: &str) -> Option<String> {
        if property.starts_with("--") {
            return Some(self.custom.get(property).map(|v| serialize_component_values(v)).unwrap_or_default());
        }
        let id = LonghandId::by_name(property)?;
        Some(self.serialize_longhand(id))
    }

    pub fn serialize_longhand(&self, id: LonghandId) -> String {
        use LonghandId as L;
        let s = self;
        let layers = |f: &dyn Fn(&BackgroundLayer) -> String, default: &str| -> String {
            if s.background.is_empty() {
                default.to_owned()
            } else {
                list(&s.background, f)
            }
        };
        match id {
            L::FontFamily => {
                if s.font.family.is_empty() {
                    s.font.typeface.family_name().to_owned()
                } else {
                    s.font.family.clone()
                }
            }
            L::FontSize => px(s.font.size),
            L::FontWeight => s.font.weight.to_string(),
            L::FontStyle => match s.font.style {
                FontStyle::Normal => "normal",
                FontStyle::Italic => "italic",
                FontStyle::Oblique => "oblique",
            }
            .into(),
            L::FontVariant => if s.font.small_caps { "small-caps" } else { "normal" }.into(),
            L::LineHeight => match s.line_height {
                LineHeight::Normal => "normal".into(),
                LineHeight::Number(n) => px(s.font.size.scale(n, 1000)),
                LineHeight::Length(l) => px(l),
            },
            L::Color => color(s.color),
            L::Display => display(s.display).into(),
            L::Position => match s.position {
                Position::Static => "static",
                Position::Relative => "relative",
                Position::Absolute => "absolute",
                Position::Fixed => "fixed",
                Position::Sticky => "sticky",
            }
            .into(),
            L::Float => match s.float {
                Float::None => "none",
                Float::Left => "left",
                Float::Right => "right",
            }
            .into(),
            L::Clear => match s.clear {
                Clear::None => "none",
                Clear::Left => "left",
                Clear::Right => "right",
                Clear::Both => "both",
            }
            .into(),
            L::Visibility => match s.visibility {
                Visibility::Visible => "visible",
                Visibility::Hidden => "hidden",
                Visibility::Collapse => "collapse",
            }
            .into(),
            L::BoxSizing => match s.box_sizing {
                BoxSizing::ContentBox => "content-box",
                BoxSizing::BorderBox => "border-box",
            }
            .into(),
            L::OverflowX => overflow(s.overflow_x).into(),
            L::OverflowY => overflow(s.overflow_y).into(),
            L::ZIndex => match s.z_index {
                ZIndex::Auto => "auto".into(),
                ZIndex::Int(i) => i.to_string(),
            },
            L::Direction => match s.direction {
                Direction::Ltr => "ltr",
                Direction::Rtl => "rtl",
            }
            .into(),
            L::Width => sizing(s.width),
            L::Height => sizing(s.height),
            L::MinWidth => sizing(s.min_width),
            L::MinHeight => sizing(s.min_height),
            L::MaxWidth => sizing(s.max_width),
            L::MaxHeight => sizing(s.max_height),
            L::MarginTop => lpa(s.margin.top),
            L::MarginRight => lpa(s.margin.right),
            L::MarginBottom => lpa(s.margin.bottom),
            L::MarginLeft => lpa(s.margin.left),
            L::PaddingTop => lp(s.padding.top),
            L::PaddingRight => lp(s.padding.right),
            L::PaddingBottom => lp(s.padding.bottom),
            L::PaddingLeft => lp(s.padding.left),
            L::BorderTopWidth => px(s.border.top.used_width()),
            L::BorderRightWidth => px(s.border.right.used_width()),
            L::BorderBottomWidth => px(s.border.bottom.used_width()),
            L::BorderLeftWidth => px(s.border.left.used_width()),
            L::BorderTopStyle => border_style(s.border.top.style).into(),
            L::BorderRightStyle => border_style(s.border.right.style).into(),
            L::BorderBottomStyle => border_style(s.border.bottom.style).into(),
            L::BorderLeftStyle => border_style(s.border.left.style).into(),
            L::BorderTopColor => color(s.border.top.color),
            L::BorderRightColor => color(s.border.right.color),
            L::BorderBottomColor => color(s.border.bottom.color),
            L::BorderLeftColor => color(s.border.left.color),
            L::BorderTopLeftRadius => radius(s.border_radius.top_left),
            L::BorderTopRightRadius => radius(s.border_radius.top_right),
            L::BorderBottomRightRadius => radius(s.border_radius.bottom_right),
            L::BorderBottomLeftRadius => radius(s.border_radius.bottom_left),
            L::Top => lpa(s.inset.top),
            L::Right => lpa(s.inset.right),
            L::Bottom => lpa(s.inset.bottom),
            L::Left => lpa(s.inset.left),
            L::TextAlign => match s.text_align {
                TextAlign::Start => "start",
                TextAlign::End => "end",
                TextAlign::Left => "left",
                TextAlign::Right => "right",
                TextAlign::Center => "center",
                TextAlign::Justify => "justify",
                TextAlign::WebkitCenter => "-webkit-center",
            }
            .into(),
            L::TextIndent => lp(s.text_indent),
            L::TextTransform => match s.text_transform {
                TextTransform::None => "none",
                TextTransform::Uppercase => "uppercase",
                TextTransform::Lowercase => "lowercase",
                TextTransform::Capitalize => "capitalize",
            }
            .into(),
            L::TextDecorationLine => {
                let d = s.text_decoration;
                let mut v = Vec::new();
                if d.underline {
                    v.push("underline");
                }
                if d.overline {
                    v.push("overline");
                }
                if d.line_through {
                    v.push("line-through");
                }
                if v.is_empty() {
                    "none".into()
                } else {
                    v.join(" ")
                }
            }
            L::TextDecorationColor => color(s.text_decoration.color.unwrap_or(s.color)),
            L::TextDecorationStyle => match s.text_decoration.style {
                TextDecorationStyle::Solid => "solid",
                TextDecorationStyle::Double => "double",
                TextDecorationStyle::Dotted => "dotted",
                TextDecorationStyle::Dashed => "dashed",
                TextDecorationStyle::Wavy => "wavy",
            }
            .into(),
            L::TextOverflow => match s.text_overflow {
                TextOverflow::Clip => "clip",
                TextOverflow::Ellipsis => "ellipsis",
            }
            .into(),
            L::WhiteSpace => match s.white_space {
                WhiteSpace::Normal => "normal",
                WhiteSpace::NoWrap => "nowrap",
                WhiteSpace::Pre => "pre",
                WhiteSpace::PreWrap => "pre-wrap",
                WhiteSpace::PreLine => "pre-line",
                WhiteSpace::BreakSpaces => "break-spaces",
            }
            .into(),
            L::WordBreak => match s.word_break {
                WordBreak::Normal => "normal",
                WordBreak::BreakAll => "break-all",
                WordBreak::KeepAll => "keep-all",
                WordBreak::BreakWord => "break-word",
            }
            .into(),
            L::ScrollbarWidth => match s.scrollbar_width {
                ScrollbarWidth::Auto => "auto",
                ScrollbarWidth::Thin => "thin",
                ScrollbarWidth::None => "none",
            }
            .into(),
            L::OverflowWrap => match s.overflow_wrap {
                OverflowWrap::Normal => "normal",
                OverflowWrap::Anywhere => "anywhere",
                OverflowWrap::BreakWord => "break-word",
            }
            .into(),
            L::LetterSpacing => {
                if s.letter_spacing.is_zero() {
                    "normal".into()
                } else {
                    px(s.letter_spacing)
                }
            }
            L::WordSpacing => px(s.word_spacing),
            L::VerticalAlign => match s.vertical_align {
                VerticalAlign::Baseline => "baseline".into(),
                VerticalAlign::Sub => "sub".into(),
                VerticalAlign::Super => "super".into(),
                VerticalAlign::TextTop => "text-top".into(),
                VerticalAlign::TextBottom => "text-bottom".into(),
                VerticalAlign::Middle => "middle".into(),
                VerticalAlign::Top => "top".into(),
                VerticalAlign::Bottom => "bottom".into(),
                VerticalAlign::Length(l) => lp(l),
            },
            L::TextShadow => {
                if s.text_shadow.is_empty() {
                    "none".into()
                } else {
                    list(&s.text_shadow, |t| shadow_common(t.offset_x, t.offset_y, t.blur, None, t.color, false))
                }
            }
            L::TabSize => s.tab_size.to_string(),
            L::ListStyleType => list_style_type(s.list_style_type).into(),
            L::ListStylePosition => match s.list_style_position {
                ListStylePosition::Outside => "outside",
                ListStylePosition::Inside => "inside",
            }
            .into(),
            L::ListStyleImage => "none".into(),
            L::TableLayout => match s.table_layout {
                TableLayout::Auto => "auto",
                TableLayout::Fixed => "fixed",
            }
            .into(),
            L::BorderCollapse => match s.border_collapse {
                BorderCollapse::Separate => "separate",
                BorderCollapse::Collapse => "collapse",
            }
            .into(),
            L::BorderSpacing => format!("{} {}", px(s.border_spacing.0), px(s.border_spacing.1)),
            L::CaptionSide => match s.caption_side {
                CaptionSide::Top => "top",
                CaptionSide::Bottom => "bottom",
            }
            .into(),
            L::EmptyCells => match s.empty_cells {
                EmptyCells::Show => "show",
                EmptyCells::Hide => "hide",
            }
            .into(),
            L::BackgroundColor => color(s.background_color),
            L::BackgroundImage => layers(&|l| image(&l.image), "none"),
            L::BackgroundRepeat => layers(
                &|l| match l.repeat {
                    BackgroundRepeat::Repeat => "repeat",
                    BackgroundRepeat::RepeatX => "repeat-x",
                    BackgroundRepeat::RepeatY => "repeat-y",
                    BackgroundRepeat::NoRepeat => "no-repeat",
                    BackgroundRepeat::Space => "space",
                    BackgroundRepeat::Round => "round",
                }
                .into(),
                "repeat",
            ),
            L::BackgroundSize => layers(
                &|l| match l.size {
                    BackgroundSize::Auto => "auto".into(),
                    BackgroundSize::Cover => "cover".into(),
                    BackgroundSize::Contain => "contain".into(),
                    BackgroundSize::Explicit(a, b) => format!("{} {}", lpa(a), lpa(b)),
                },
                "auto",
            ),
            L::BackgroundPositionX => layers(&|l| lp(l.position.0), "0%"),
            L::BackgroundPositionY => layers(&|l| lp(l.position.1), "0%"),
            L::BackgroundOrigin => layers(&|l| bg_box(l.origin).into(), "padding-box"),
            L::BackgroundClip => layers(&|l| bg_box(l.clip).into(), "border-box"),
            L::BackgroundAttachment => layers(&|l| if l.attachment_fixed { "fixed" } else { "scroll" }.into(), "scroll"),
            L::BoxShadow => {
                if s.box_shadow.is_empty() {
                    "none".into()
                } else {
                    list(&s.box_shadow, |b| shadow_common(b.offset_x, b.offset_y, b.blur, Some(b.spread), b.color, b.inset))
                }
            }
            // Opacity is stored in 1/255; two decimals reproduce the common authored values.
            L::Opacity => milli_to_string((s.opacity as i64 * 100 + 127) / 255, 100),
            L::Transform => transform(&s.transform),
            L::TransformOrigin => format!("{} {}", lp(s.transform_origin.0), lp(s.transform_origin.1)),
            L::OutlineWidth => px(s.outline.used_width()),
            L::OutlineStyle => border_style(s.outline.style).into(),
            L::OutlineColor => color(s.outline.color),
            L::OutlineOffset => px(s.outline_offset),
            L::FlexDirection => match s.flex_direction {
                FlexDirection::Row => "row",
                FlexDirection::RowReverse => "row-reverse",
                FlexDirection::Column => "column",
                FlexDirection::ColumnReverse => "column-reverse",
            }
            .into(),
            L::FlexWrap => match s.flex_wrap {
                FlexWrap::NoWrap => "nowrap",
                FlexWrap::Wrap => "wrap",
                FlexWrap::WrapReverse => "wrap-reverse",
            }
            .into(),
            L::FlexGrow => milli_to_string(s.flex_grow as i64, 1000),
            L::FlexShrink => milli_to_string(s.flex_shrink as i64, 1000),
            L::FlexBasis => sizing(s.flex_basis),
            L::Order => s.order.to_string(),
            L::JustifyContent => match s.justify_content {
                JustifyContent::FlexStart => "flex-start",
                JustifyContent::FlexEnd => "flex-end",
                JustifyContent::Center => "center",
                JustifyContent::SpaceBetween => "space-between",
                JustifyContent::SpaceAround => "space-around",
                JustifyContent::SpaceEvenly => "space-evenly",
                JustifyContent::Start => "start",
                JustifyContent::End => "end",
                JustifyContent::Left => "left",
                JustifyContent::Right => "right",
                JustifyContent::Stretch => "stretch",
            }
            .into(),
            L::AlignItems => align_items(s.align_items).into(),
            L::AlignSelf => align_self(s.align_self).into(),
            L::AlignContent => match s.align_content {
                AlignContent::Normal => "normal",
                AlignContent::FlexStart => "flex-start",
                AlignContent::FlexEnd => "flex-end",
                AlignContent::Center => "center",
                AlignContent::SpaceBetween => "space-between",
                AlignContent::SpaceAround => "space-around",
                AlignContent::SpaceEvenly => "space-evenly",
                AlignContent::Stretch => "stretch",
                AlignContent::Start => "start",
                AlignContent::End => "end",
            }
            .into(),
            L::RowGap => lp(s.row_gap),
            L::ColumnGap => lp(s.column_gap),
            L::GridTemplateRows => track_list(&s.grid_template_rows),
            L::GridTemplateColumns => track_list(&s.grid_template_columns),
            L::GridTemplateAreas => {
                if s.grid_template_areas.is_empty() {
                    "none".into()
                } else {
                    s.grid_template_areas.iter().map(|r| quote(&r.join(" "))).collect::<Vec<_>>().join(" ")
                }
            }
            L::GridAutoRows => s.grid_auto_rows.iter().map(track_size).collect::<Vec<_>>().join(" "),
            L::GridAutoColumns => s.grid_auto_columns.iter().map(track_size).collect::<Vec<_>>().join(" "),
            L::GridAutoFlow => match s.grid_auto_flow {
                GridAutoFlow::Row => "row",
                GridAutoFlow::Column => "column",
                GridAutoFlow::RowDense => "row dense",
                GridAutoFlow::ColumnDense => "column dense",
            }
            .into(),
            L::GridRowStart => grid_line(&s.grid_row_start),
            L::GridRowEnd => grid_line(&s.grid_row_end),
            L::GridColumnStart => grid_line(&s.grid_column_start),
            L::GridColumnEnd => grid_line(&s.grid_column_end),
            L::JustifyItems => match s.justify_items {
                AlignItems::Stretch => "normal".into(),
                a => align_items(a).into(),
            },
            L::JustifySelf => align_self(s.justify_self).into(),
            L::Cursor => match s.cursor {
                Cursor::Auto => "auto",
                Cursor::Default => "default",
                Cursor::Pointer => "pointer",
                Cursor::Text => "text",
                Cursor::Move => "move",
                Cursor::NotAllowed => "not-allowed",
                Cursor::Grab => "grab",
                Cursor::Grabbing => "grabbing",
                Cursor::Crosshair => "crosshair",
                Cursor::Wait => "wait",
                Cursor::Progress => "progress",
                Cursor::Help => "help",
                Cursor::ColResize => "col-resize",
                Cursor::RowResize => "row-resize",
                Cursor::NsResize => "ns-resize",
                Cursor::EwResize => "ew-resize",
                Cursor::NeswResize => "nesw-resize",
                Cursor::NwseResize => "nwse-resize",
                Cursor::None => "none",
            }
            .into(),
            L::PointerEvents => match s.pointer_events {
                PointerEvents::Auto => "auto",
                PointerEvents::None => "none",
            }
            .into(),
            L::UserSelect => match s.user_select {
                UserSelect::Auto => "auto",
                UserSelect::None => "none",
                UserSelect::Text => "text",
                UserSelect::All => "all",
            }
            .into(),
            L::Appearance => match s.appearance {
                Appearance::Auto => "auto",
                Appearance::None => "none",
            }
            .into(),
            L::LineClamp => s.line_clamp.map(|n| n.to_string()).unwrap_or_else(|| "none".into()),
            L::BoxOrient => if s.box_orient_vertical { "vertical" } else { "horizontal" }.into(),
            L::AspectRatio => {
                let num = |micro: i64| {
                    let t = format!("{}.{:06}", micro / 1_000_000, micro % 1_000_000);
                    t.trim_end_matches('0').trim_end_matches('.').to_owned()
                };
                match (s.aspect_ratio.auto, s.aspect_ratio.ratio) {
                    (_, None) => "auto".into(),
                    (false, Some((w, h))) => format!("{} / {}", num(w), num(h)),
                    (true, Some((w, h))) => format!("auto {} / {}", num(w), num(h)),
                }
            }
            L::ObjectFit => match s.object_fit {
                ObjectFit::Fill => "fill",
                ObjectFit::Contain => "contain",
                ObjectFit::Cover => "cover",
                ObjectFit::None => "none",
                ObjectFit::ScaleDown => "scale-down",
            }
            .into(),
            L::Content => content(&s.content),
            L::Quotes => {
                if s.quotes.is_empty() {
                    "none".into()
                } else {
                    s.quotes.iter().map(|(a, b)| format!("{} {}", quote(a), quote(b))).collect::<Vec<_>>().join(" ")
                }
            }
            L::CounterReset => counters(&s.counter_reset),
            L::CounterIncrement => counters(&s.counter_increment),
            L::TransitionProperty => s.transitions.property.join(", "),
            L::TransitionDuration => list(&s.transitions.duration, |t| ms(*t)),
            L::TransitionTimingFunction => list(&s.transitions.timing, |t| timing(*t)),
            L::TransitionDelay => list(&s.transitions.delay, |t| ms(*t)),
            L::AnimationName => s.animations.name.join(", "),
            L::AnimationDuration => list(&s.animations.duration, |t| ms(*t)),
            L::AnimationTimingFunction => list(&s.animations.timing, |t| timing(*t)),
            L::AnimationDelay => list(&s.animations.delay, |t| ms(*t)),
            L::AnimationIterationCount => list(&s.animations.iteration_count, |c| match c {
                None => "infinite".into(),
                Some(n) => milli_to_string(*n as i64, 1000),
            }),
            L::AnimationDirection => list(&s.animations.direction, |d| {
                match d {
                    AnimationDirection::Normal => "normal",
                    AnimationDirection::Reverse => "reverse",
                    AnimationDirection::Alternate => "alternate",
                    AnimationDirection::AlternateReverse => "alternate-reverse",
                }
                .into()
            }),
            L::AnimationFillMode => list(&s.animations.fill_mode, |f| {
                match f {
                    AnimationFillMode::None => "none",
                    AnimationFillMode::Forwards => "forwards",
                    AnimationFillMode::Backwards => "backwards",
                    AnimationFillMode::Both => "both",
                }
                .into()
            }),
            L::AnimationPlayState => list(&s.animations.play_state, |p| if *p { "running" } else { "paused" }.into()),
        }
    }
}

fn radius(r: (LengthPercentage, LengthPercentage)) -> String {
    if r.0 == r.1 {
        lp(r.0)
    } else {
        format!("{} {}", lp(r.0), lp(r.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_colours() {
        assert_eq!(au_to_string(Au::from_px_i32(16)), "16");
        assert_eq!(au_to_string(Au(32)), "0.5");
        assert_eq!(au_to_string(Au(1)), "0.015625");
        assert_eq!(au_to_string(Au(-96)), "-1.5");
        assert_eq!(percent(5000), "50%");
        assert_eq!(percent(1250), "12.5%");
        assert_eq!(color(Color(1, 2, 3, 255)), "rgb(1, 2, 3)");
        // The shortest decimal that stores the same byte, as CSSOM serialises alpha.
        assert_eq!(color(Color(1, 2, 3, 128)), "rgba(1, 2, 3, 0.5)");
        assert_eq!(color(Color(1, 2, 3, 26)), "rgba(1, 2, 3, 0.1)");
        assert_eq!(color(Color(1, 2, 3, 102)), "rgba(1, 2, 3, 0.4)");
        assert_eq!(color(Color(1, 2, 3, 64)), "rgba(1, 2, 3, 0.25)");
        assert_eq!(color(Color(1, 2, 3, 1)), "rgba(1, 2, 3, 0.004)");
        assert_eq!(matrix_number(0.949999999), "0.95");
        assert_eq!(matrix_number(-0.0), "0");
        assert_eq!(color(Color(1, 2, 3, 0)), "rgba(0, 0, 0, 0)");
        assert_eq!(lp(LengthPercentage::Calc(Au::from_px_i32(-20), 10000)), "calc(100% - 20px)");
    }

    #[test]
    fn every_longhand_serializes_from_initial() {
        let s = ComputedStyle::initial();
        for id in LonghandId::all() {
            let v = s.serialize(id.name()).unwrap();
            assert!(!v.is_empty(), "{}", id.name());
        }
        assert_eq!(s.serialize("display").unwrap(), "inline");
        assert_eq!(s.serialize("font-size").unwrap(), "16px");
        assert_eq!(s.serialize("margin-top").unwrap(), "0px");
        assert_eq!(s.serialize("border-top-width").unwrap(), "0px");
        assert_eq!(s.serialize("background-color").unwrap(), "rgba(0, 0, 0, 0)");
        assert_eq!(s.serialize("background-image").unwrap(), "none");
        assert_eq!(s.serialize("opacity").unwrap(), "1");
        assert_eq!(s.serialize("flex-shrink").unwrap(), "1");
        assert_eq!(s.serialize("quotes").unwrap(), "\"\u{201C}\" \"\u{201D}\" \"\u{2018}\" \"\u{2019}\"");
        assert_eq!(s.serialize("transition-duration").unwrap(), "0s");
        assert_eq!(s.serialize("animation-iteration-count").unwrap(), "1");
        assert_eq!(s.serialize("grid-template-columns").unwrap(), "none");
        assert_eq!(s.serialize("transform-origin").unwrap(), "50% 50%");
        assert_eq!(s.serialize("nonsense"), None);
        assert_eq!(s.serialize("--x").unwrap(), "");
    }
}
