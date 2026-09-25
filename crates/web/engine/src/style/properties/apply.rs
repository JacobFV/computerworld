//! Specified to computed: each function writes one longhand's computed value into a
//! `ComputedStyle`, resolving relative units, `currentcolor`, relative font sizes
//! and weights, and clamping where the spec clamps at computed-value time. Returns
//! `false` when the value is invalid at computed-value time (the cascade then treats
//! the declaration as `unset`).

use super::*;
use crate::style::fonts;
use cw_scene::Color;

macro_rules! simple {
    ($fn:ident, $var:ident, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
            match v {
                Specified::$var($x) => {
                    let $x = $x.clone();
                    $body;
                    true
                }
                _ => false,
            }
        }
    };
}

fn clamp_non_negative(lp: LengthPercentage) -> LengthPercentage {
    match lp {
        LengthPercentage::Length(l) => LengthPercentage::Length(l.max(Au::ZERO)),
        LengthPercentage::Percent(p) => LengthPercentage::Percent(p.max(0)),
        c => c,
    }
}

/// `Specified::Lp` computed with the element's own lengths.
fn lp_value(v: &Specified, c: &ComputeCtx, non_negative: bool) -> Option<LengthPercentage> {
    match v {
        Specified::Lp(l) => {
            let r = l.compute(&c.lengths)?;
            Some(if non_negative {
                clamp_non_negative(r)
            } else {
                r
            })
        }
        _ => None,
    }
}

macro_rules! lp_prop {
    ($fn:ident, $nn:expr, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
            match lp_value(v, c, $nn) {
                Some($x) => {
                    $body;
                    true
                }
                None => false,
            }
        }
    };
}

fn lpa_value(v: &Specified, c: &ComputeCtx) -> Option<LengthPercentageAuto> {
    match v {
        Specified::Lpa(LpaSpec::Auto) => Some(LengthPercentageAuto::Auto),
        Specified::Lpa(LpaSpec::Lp(l)) => Some(LengthPercentageAuto::Set(l.compute(&c.lengths)?)),
        _ => None,
    }
}

macro_rules! lpa_prop {
    ($fn:ident, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
            match lpa_value(v, c) {
                Some($x) => {
                    $body;
                    true
                }
                None => false,
            }
        }
    };
}

fn sizing_value(v: &Specified, c: &ComputeCtx) -> Option<Sizing> {
    Some(match v {
        Specified::Sizing(SizingSpec::Auto) => Sizing::Auto,
        // `flex-basis: content` sizes the item as max-content (css-flexbox §9.2.3).
        Specified::Sizing(SizingSpec::Content) => Sizing::MaxContent,
        Specified::Sizing(SizingSpec::None) => Sizing::None,
        Specified::Sizing(SizingSpec::MinContent) => Sizing::MinContent,
        Specified::Sizing(SizingSpec::MaxContent) => Sizing::MaxContent,
        Specified::Sizing(SizingSpec::FitContent) => Sizing::FitContent,
        Specified::Sizing(SizingSpec::Lp(l)) => {
            Sizing::Set(clamp_non_negative(l.compute(&c.lengths)?))
        }
        _ => return None,
    })
}

macro_rules! sizing_prop {
    ($fn:ident, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
            match sizing_value(v, c) {
                Some($x) => {
                    $body;
                    true
                }
                None => false,
            }
        }
    };
}

fn border_width_value(v: &Specified, c: &ComputeCtx) -> Option<Au> {
    Some(match v {
        Specified::BorderWidth(BorderWidthSpec::Thin) => Au::from_px_i32(1),
        Specified::BorderWidth(BorderWidthSpec::Medium) => Au::from_px_i32(3),
        Specified::BorderWidth(BorderWidthSpec::Thick) => Au::from_px_i32(5),
        Specified::BorderWidth(BorderWidthSpec::Length(l)) => {
            l.compute_length(&c.lengths)?.max(Au::ZERO)
        }
        _ => return None,
    })
}

macro_rules! border_width_prop {
    ($fn:ident, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
            match border_width_value(v, c) {
                Some($x) => {
                    $body;
                    true
                }
                None => false,
            }
        }
    };
}

/// Colours other than `color` itself resolve `currentcolor` against the element's
/// own `color`, which is applied in an earlier phase.
fn color_value(v: &Specified, s: &ComputedStyle) -> Option<Color> {
    match v {
        Specified::Color(c) => Some(c.resolve(s.color)),
        _ => None,
    }
}

macro_rules! color_prop {
    ($fn:ident, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
            match color_value(v, $s) {
                Some($x) => {
                    $body;
                    true
                }
                None => false,
            }
        }
    };
}

// --- Fonts -----------------------------------------------------------------------

pub fn font_family(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::FontFamily(list) = v else {
        return false;
    };
    let was_mono = is_monospace_family(&s.font.family);
    s.font.family = fonts::serialize_family_list(list);
    s.font.typeface = fonts::resolve_family_with(list, c.fonts, c.web_fonts);
    let now_mono = is_monospace_family(&s.font.family);
    if was_mono != now_mono {
        if let Some(i) = s.font_size_keyword {
            s.font.size = absolute_font_size(i, now_mono);
        }
    }
    true
}

/// The monospace scale quirk applies when the family is exactly the generic.
pub fn is_monospace_family(family: &str) -> bool {
    family.eq_ignore_ascii_case("monospace")
}

pub fn font_size(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::FontSize(spec) = v else {
        return false;
    };
    let mono = is_monospace_family(&s.font.family);
    let parent = c.parent.font.size;
    // The parent's unrounded size, for sizes relative to it.
    let parent_micro = if c.parent.font.size_micro > 0 {
        c.parent.font.size_micro
    } else {
        i64::from(parent.0) * 15_625
    };
    s.font_size_keyword = None;
    s.font.size = match spec {
        FontSizeSpec::Absolute(i) => {
            s.font_size_keyword = Some(*i);
            absolute_font_size(*i, mono)
        }
        FontSizeSpec::Larger => parent.scale(12, 10),
        FontSizeSpec::Smaller => parent.scale(10, 12),
        FontSizeSpec::Lp(l) => match l.compute(&c.parent_lengths) {
            Some(lp) => lp.resolve(parent).max(Au::ZERO),
            None => return false,
        },
    };
    // The unrounded size, where it is simple to have: glyphs are scaled by it
    // (`Font::glyph_size`); anything else falls back to the rounded size.
    s.font.size_micro = match spec {
        FontSizeSpec::Larger => parent_micro * 12 / 10,
        FontSizeSpec::Smaller => parent_micro * 10 / 12,
        FontSizeSpec::Lp(LpSpec::Length(l)) => match l.unit {
            LengthUnit::Em => {
                (i128::from(l.value.micro) * i128::from(parent_micro) / 1_000_000) as i64
            }
            LengthUnit::Rem
            | LengthUnit::Ex
            | LengthUnit::Ch
            | LengthUnit::Lh
            | LengthUnit::Rlh => i64::from(s.font.size.0) * 15_625,
            _ => l.to_micro_px(&c.parent_lengths) as i64,
        },
        FontSizeSpec::Lp(LpSpec::Percent(n)) => {
            (i128::from(n.micro) * i128::from(parent_micro) / 100_000_000) as i64
        }
        _ => i64::from(s.font.size.0) * 15_625,
    }
    .max(0);
    true
}

pub fn font_weight(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::FontWeight(spec) = v else {
        return false;
    };
    let p = c.parent.font.weight;
    s.font.weight = match spec {
        FontWeightSpec::Absolute(w) => *w,
        FontWeightSpec::Bolder => {
            if p < 350 {
                400
            } else if p < 550 {
                700
            } else if p < 900 {
                900
            } else {
                p
            }
        }
        FontWeightSpec::Lighter => {
            if p < 100 {
                p
            } else if p < 550 {
                100
            } else if p < 750 {
                400
            } else {
                700
            }
        }
    };
    true
}

simple!(font_style, FontStyle, |s, x| s.font.style = x);
simple!(font_variant, Bool, |s, x| s.font.small_caps = x);
simple!(font_kerning, Bool, |s, x| s.font.kerning_none = x);
simple!(font_feature_settings, Integer, |s, x| s.font.kern_feature =
    x.clamp(-1, 1) as i8);

pub fn line_height(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::LineHeight(spec) = v else {
        return false;
    };
    s.line_height = match spec {
        LineHeightSpec::Normal => LineHeight::Normal,
        LineHeightSpec::Number(n) => LineHeight::Number(micro_to_milli(n.micro).max(0)),
        LineHeightSpec::Lp(l) => match l.compute(&c.lengths) {
            Some(lp) => LineHeight::Length(lp.resolve(s.font.size).max(Au::ZERO)),
            None => return false,
        },
    };
    true
}

pub fn color(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Color(spec) = v else {
        return false;
    };
    s.color = spec.resolve(c.parent.color);
    true
}

// --- Box generation --------------------------------------------------------------

simple!(display, Display, |s, x| s.display = x);
simple!(position, Position, |s, x| s.position = x);
simple!(float, Float, |s, x| s.float = x);
simple!(clear, Clear, |s, x| s.clear = x);
simple!(visibility, Visibility, |s, x| s.visibility = x);
simple!(box_sizing, BoxSizing, |s, x| s.box_sizing = x);
simple!(overflow_x, Overflow, |s, x| s.overflow_x = x);
simple!(overflow_y, Overflow, |s, x| s.overflow_y = x);
simple!(z_index, ZIndex, |s, x| s.z_index = x);
simple!(direction, Direction, |s, x| s.direction = x);

// --- Box model -------------------------------------------------------------------

sizing_prop!(width, |s, x| s.width = x);
sizing_prop!(height, |s, x| s.height = x);
sizing_prop!(min_width, |s, x| s.min_width = x);
sizing_prop!(min_height, |s, x| s.min_height = x);
sizing_prop!(max_width, |s, x| s.max_width = x);
sizing_prop!(max_height, |s, x| s.max_height = x);
lpa_prop!(margin_top, |s, x| s.margin.top = x);
lpa_prop!(margin_right, |s, x| s.margin.right = x);
lpa_prop!(margin_bottom, |s, x| s.margin.bottom = x);
lpa_prop!(margin_left, |s, x| s.margin.left = x);
lp_prop!(padding_top, true, |s, x| s.padding.top = x);
lp_prop!(padding_right, true, |s, x| s.padding.right = x);
lp_prop!(padding_bottom, true, |s, x| s.padding.bottom = x);
lp_prop!(padding_left, true, |s, x| s.padding.left = x);
border_width_prop!(border_top_width, |s, x| s.border.top.width = x);
border_width_prop!(border_right_width, |s, x| s.border.right.width = x);
border_width_prop!(border_bottom_width, |s, x| s.border.bottom.width = x);
border_width_prop!(border_left_width, |s, x| s.border.left.width = x);
simple!(border_top_style, BorderStyle, |s, x| s.border.top.style = x);
simple!(border_right_style, BorderStyle, |s, x| s
    .border
    .right
    .style = x);
simple!(border_bottom_style, BorderStyle, |s, x| s
    .border
    .bottom
    .style = x);
simple!(border_left_style, BorderStyle, |s, x| s.border.left.style =
    x);
color_prop!(border_top_color, |s, x| s.border.top.color = x);
color_prop!(border_right_color, |s, x| s.border.right.color = x);
color_prop!(border_bottom_color, |s, x| s.border.bottom.color = x);
color_prop!(border_left_color, |s, x| s.border.left.color = x);

fn radius_value(v: &Specified, c: &ComputeCtx) -> Option<(LengthPercentage, LengthPercentage)> {
    match v {
        Specified::LpPair(a, b) => Some((
            clamp_non_negative(a.compute(&c.lengths)?),
            clamp_non_negative(b.compute(&c.lengths)?),
        )),
        _ => None,
    }
}

macro_rules! radius_prop {
    ($fn:ident, |$s:ident, $x:ident| $body:expr) => {
        pub fn $fn($s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
            match radius_value(v, c) {
                Some($x) => {
                    $body;
                    true
                }
                None => false,
            }
        }
    };
}

radius_prop!(border_top_left_radius, |s, x| s.border_radius.top_left = x);
radius_prop!(border_top_right_radius, |s, x| s.border_radius.top_right =
    x);
radius_prop!(border_bottom_right_radius, |s, x| s
    .border_radius
    .bottom_right = x);
radius_prop!(border_bottom_left_radius, |s, x| s
    .border_radius
    .bottom_left = x);
lpa_prop!(top, |s, x| s.inset.top = x);
lpa_prop!(right, |s, x| s.inset.right = x);
lpa_prop!(bottom, |s, x| s.inset.bottom = x);
lpa_prop!(left, |s, x| s.inset.left = x);

// --- Text ------------------------------------------------------------------------

simple!(text_align, TextAlign, |s, x| s.text_align = x);
lp_prop!(text_indent, false, |s, x| s.text_indent = x);
simple!(text_transform, TextTransform, |s, x| s.text_transform = x);

pub fn text_decoration_line(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::TextDecorationLine {
        underline,
        overline,
        line_through,
    } = v
    else {
        return false;
    };
    s.text_decoration.underline = *underline;
    s.text_decoration.overline = *overline;
    s.text_decoration.line_through = *line_through;
    true
}

pub fn text_decoration_color(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Color(c) = v else { return false };
    s.text_decoration.color = if c.is_current() {
        None
    } else {
        Some(c.resolve(s.color))
    };
    true
}

simple!(text_decoration_style, TextDecorationStyle, |s, x| s
    .text_decoration
    .style =
    x);
simple!(text_overflow, TextOverflow, |s, x| s.text_overflow = x);
simple!(white_space, WhiteSpace, |s, x| s.white_space = x);
simple!(word_break, WordBreak, |s, x| s.word_break = x);
simple!(overflow_wrap, OverflowWrap, |s, x| s.overflow_wrap = x);
simple!(scrollbar_width, ScrollbarWidth, |s, x| s.scrollbar_width =
    x);

fn length_or_normal_value(v: &Specified, c: &ComputeCtx) -> Option<Au> {
    match v {
        Specified::LengthOrNormal(None) => Some(Au::ZERO),
        Specified::LengthOrNormal(Some(l)) => l.compute_length(&c.lengths),
        _ => None,
    }
}

pub fn letter_spacing(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    match length_or_normal_value(v, c) {
        Some(x) => {
            s.letter_spacing = x;
            true
        }
        None => false,
    }
}

pub fn word_spacing(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    match length_or_normal_value(v, c) {
        Some(x) => {
            s.word_spacing = x;
            true
        }
        None => false,
    }
}

pub fn vertical_align(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    s.vertical_align = match v {
        Specified::VerticalAlign(VerticalAlignSpec::Keyword(k)) => *k,
        Specified::VerticalAlign(VerticalAlignSpec::Lp(l)) => match l.compute(&c.lengths) {
            Some(lp) => VerticalAlign::Length(lp),
            None => return false,
        },
        _ => return false,
    };
    true
}

pub fn text_shadow(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Shadows(list) = v else {
        return false;
    };
    let mut out = Vec::with_capacity(list.len());
    for sh in list {
        let (Some(x), Some(y), Some(blur)) = (
            sh.x.compute_length(&c.lengths),
            sh.y.compute_length(&c.lengths),
            sh.blur.compute_length(&c.lengths),
        ) else {
            return false;
        };
        let color = sh
            .color
            .as_ref()
            .map(|col| col.resolve(s.color))
            .unwrap_or(s.color);
        out.push(TextShadow {
            offset_x: x,
            offset_y: y,
            blur: blur.max(Au::ZERO),
            color,
        });
    }
    s.text_shadow = out;
    true
}

pub fn tab_size(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Number(n) = v else {
        return false;
    };
    s.tab_size = n.round().clamp(0, 255) as u8;
    true
}

// --- Lists and tables ------------------------------------------------------------

simple!(list_style_type, ListStyleType, |s, x| s.list_style_type = x);
simple!(list_style_position, ListStylePosition, |s, x| s
    .list_style_position =
    x);

pub fn list_style_image(_s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    // Accepted and ignored: markers are drawn from `list-style-type`.
    matches!(v, Specified::Images(_))
}

simple!(table_layout, TableLayout, |s, x| s.table_layout = x);
simple!(border_collapse, BorderCollapse, |s, x| s.border_collapse =
    x);

pub fn border_spacing(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::LpPair(a, b) = v else {
        return false;
    };
    let (Some(a), Some(b)) = (a.compute_length(&c.lengths), b.compute_length(&c.lengths)) else {
        return false;
    };
    s.border_spacing = (a.max(Au::ZERO), b.max(Au::ZERO));
    true
}

simple!(caption_side, CaptionSide, |s, x| s.caption_side = x);
simple!(empty_cells, EmptyCells, |s, x| s.empty_cells = x);

// --- Backgrounds -----------------------------------------------------------------

color_prop!(background_color, |s, x| s.background_color = x);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LayerField {
    Image,
    Repeat,
    Size,
    PositionX,
    PositionY,
    Origin,
    Clip,
    Attachment,
}

pub fn default_layer() -> BackgroundLayer {
    BackgroundLayer {
        image: BackgroundImage::None,
        repeat: BackgroundRepeat::Repeat,
        size: BackgroundSize::Auto,
        position: (LengthPercentage::ZERO, LengthPercentage::ZERO),
        origin: BackgroundBox::PaddingBox,
        clip: BackgroundBox::BorderBox,
        attachment_fixed: false,
    }
}

/// Makes `s.background` have `n` layers when `set_count` (the image list decides
/// the count), or at least one layer when a list longhand applies to an empty set.
fn ensure_layers(s: &mut ComputedStyle, n: usize, set_count: bool) {
    if set_count || s.background.is_empty() {
        s.background.resize(n, default_layer());
    }
}

fn set_layer_field<T: Clone>(
    s: &mut ComputedStyle,
    list: &[T],
    f: impl Fn(&mut BackgroundLayer, T),
) {
    if list.is_empty() {
        return;
    }
    ensure_layers(s, list.len(), false);
    for (i, layer) in s.background.iter_mut().enumerate() {
        f(layer, list[i % list.len()].clone());
    }
}

/// Copies one layer field from `src` to `dst` (for `initial`/`inherit`).
pub fn copy_layers(dst: &mut ComputedStyle, src: &ComputedStyle, field: LayerField) {
    if src.background.is_empty() {
        match field {
            LayerField::Image => dst.background.clear(),
            _ => {
                let d = default_layer();
                for l in dst.background.iter_mut() {
                    copy_field(l, &d, field);
                }
            }
        }
        return;
    }
    if field == LayerField::Image || dst.background.is_empty() {
        dst.background.resize(src.background.len(), default_layer());
    }
    let n = src.background.len();
    for (i, l) in dst.background.iter_mut().enumerate() {
        copy_field(l, &src.background[i % n], field);
    }
}

fn copy_field(dst: &mut BackgroundLayer, src: &BackgroundLayer, field: LayerField) {
    match field {
        LayerField::Image => dst.image = src.image.clone(),
        LayerField::Repeat => dst.repeat = src.repeat,
        LayerField::Size => dst.size = src.size,
        LayerField::PositionX => dst.position.0 = src.position.0,
        LayerField::PositionY => dst.position.1 = src.position.1,
        LayerField::Origin => dst.origin = src.origin,
        LayerField::Clip => dst.clip = src.clip,
        LayerField::Attachment => dst.attachment_fixed = src.attachment_fixed,
    }
}

fn compute_stops(
    stops: &[StopSpec],
    s: &ComputedStyle,
    c: &ComputeCtx,
) -> Option<Vec<GradientStop>> {
    let mut out = Vec::with_capacity(stops.len());
    let mut last_color = Color::TRANSPARENT;
    for st in stops {
        let position = match &st.position {
            Some(p) => Some(p.compute(&c.lengths)?),
            None => None,
        };
        let color = match &st.color {
            Some(col) => {
                last_color = col.resolve(s.color);
                last_color
            }
            // A colour hint: represented as a stop with the previous colour at that position.
            None => last_color,
        };
        out.push(GradientStop { color, position });
    }
    Some(out)
}

pub fn compute_image(
    img: &ImageSpec,
    s: &ComputedStyle,
    c: &ComputeCtx,
) -> Option<BackgroundImage> {
    Some(match img {
        ImageSpec::None => BackgroundImage::None,
        ImageSpec::Url(u) => BackgroundImage::Url(u.clone()),
        ImageSpec::Linear {
            direction, stops, ..
        } => BackgroundImage::LinearGradient {
            angle_centi_deg: direction.to_centi_degrees(),
            stops: compute_stops(stops, s, c)?,
        },
        ImageSpec::Radial { circle, stops, .. } => BackgroundImage::RadialGradient {
            circle: *circle,
            stops: compute_stops(stops, s, c)?,
        },
    })
}

pub fn background_image(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Images(list) = v else {
        return false;
    };
    let mut images = Vec::with_capacity(list.len());
    for i in list {
        match compute_image(i, s, c) {
            Some(img) => images.push(img),
            None => return false,
        }
    }
    ensure_layers(s, images.len(), true);
    for (layer, img) in s.background.iter_mut().zip(images) {
        layer.image = img;
    }
    true
}

pub fn background_repeat(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Repeats(list) = v else {
        return false;
    };
    set_layer_field(s, list, |l, r| l.repeat = r);
    true
}

pub fn background_size(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::BgSizes(list) = v else {
        return false;
    };
    let mut sizes = Vec::with_capacity(list.len());
    for sz in list {
        sizes.push(match sz {
            BgSizeSpec::Auto => BackgroundSize::Auto,
            BgSizeSpec::Cover => BackgroundSize::Cover,
            BgSizeSpec::Contain => BackgroundSize::Contain,
            BgSizeSpec::Explicit(a, b) => {
                let f = |x: &LpaSpec| -> Option<LengthPercentageAuto> {
                    Some(match x {
                        LpaSpec::Auto => LengthPercentageAuto::Auto,
                        LpaSpec::Lp(l) => {
                            LengthPercentageAuto::Set(clamp_non_negative(l.compute(&c.lengths)?))
                        }
                    })
                };
                let (Some(a), Some(b)) = (f(a), f(b)) else {
                    return false;
                };
                BackgroundSize::Explicit(a, b)
            }
        });
    }
    set_layer_field(s, &sizes, |l, v| l.size = v);
    true
}

fn lp_list(v: &Specified, c: &ComputeCtx) -> Option<Vec<LengthPercentage>> {
    let Specified::LpList(list) = v else {
        return None;
    };
    list.iter().map(|l| l.compute(&c.lengths)).collect()
}

pub fn background_position_x(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Some(list) = lp_list(v, c) else {
        return false;
    };
    set_layer_field(s, &list, |l, v| l.position.0 = v);
    true
}

pub fn background_position_y(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Some(list) = lp_list(v, c) else {
        return false;
    };
    set_layer_field(s, &list, |l, v| l.position.1 = v);
    true
}

pub fn background_origin(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::BgBoxes(list) = v else {
        return false;
    };
    set_layer_field(s, list, |l, v| l.origin = v);
    true
}

pub fn background_clip(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::BgBoxes(list) = v else {
        return false;
    };
    set_layer_field(s, list, |l, v| l.clip = v);
    true
}

pub fn background_attachment(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Bools(list) = v else {
        return false;
    };
    set_layer_field(s, list, |l, v| l.attachment_fixed = v);
    true
}

pub fn box_shadow(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Shadows(list) = v else {
        return false;
    };
    let mut out = Vec::with_capacity(list.len());
    for sh in list {
        let l = |x: &LpSpec| x.compute_length(&c.lengths);
        let (Some(x), Some(y), Some(blur), Some(spread)) =
            (l(&sh.x), l(&sh.y), l(&sh.blur), l(&sh.spread))
        else {
            return false;
        };
        let color = sh
            .color
            .as_ref()
            .map(|col| col.resolve(s.color))
            .unwrap_or(s.color);
        out.push(BoxShadow {
            offset_x: x,
            offset_y: y,
            blur: blur.max(Au::ZERO),
            spread,
            color,
            inset: sh.inset,
        });
    }
    s.box_shadow = out;
    true
}

pub fn opacity(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Number(n) = v else {
        return false;
    };
    s.opacity = fraction_to_255(n.micro.clamp(0, 1_000_000));
    true
}

pub fn backdrop_filter(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Lp(l) = v else { return false };
    let Some(x) = l.compute_length(&c.lengths) else {
        return false;
    };
    s.backdrop_blur = x.max(Au::ZERO);
    true
}

pub fn transform(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Transform(ops) = v else {
        return false;
    };
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(match op {
            TransformSpec::Translate(x, y) => {
                let (Some(x), Some(y)) = (x.compute(&c.lengths), y.compute(&c.lengths)) else {
                    return false;
                };
                TransformOp::Translate(x, y)
            }
            TransformSpec::Scale(x, y) => {
                TransformOp::Scale(micro_to_milli(x.micro), micro_to_milli(y.micro))
            }
            TransformSpec::Rotate(a) => TransformOp::Rotate(*a),
            TransformSpec::SkewX(a) => TransformOp::SkewX(*a),
            TransformSpec::SkewY(a) => TransformOp::SkewY(*a),
        });
    }
    s.transform = out;
    true
}

pub fn transform_origin(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::LpPair(a, b) = v else {
        return false;
    };
    let (Some(a), Some(b)) = (a.compute(&c.lengths), b.compute(&c.lengths)) else {
        return false;
    };
    s.transform_origin = (a, b);
    true
}

border_width_prop!(outline_width, |s, x| s.outline.width = x);
simple!(outline_style, BorderStyle, |s, x| s.outline.style = x);
color_prop!(outline_color, |s, x| s.outline.color = x);

pub fn outline_offset(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::Lp(l) = v else { return false };
    let Some(x) = l.compute_length(&c.lengths) else {
        return false;
    };
    s.outline_offset = x;
    true
}

// --- Flex ------------------------------------------------------------------------

simple!(flex_direction, FlexDirection, |s, x| s.flex_direction = x);
simple!(flex_wrap, FlexWrap, |s, x| s.flex_wrap = x);

pub fn flex_grow(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Number(n) = v else {
        return false;
    };
    s.flex_grow = micro_to_milli(n.micro).max(0);
    true
}

pub fn flex_shrink(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Number(n) = v else {
        return false;
    };
    s.flex_shrink = micro_to_milli(n.micro).max(0);
    true
}

sizing_prop!(flex_basis, |s, x| s.flex_basis = x);
simple!(order, Integer, |s, x| s.order = x);
simple!(justify_content, JustifyContent, |s, x| s.justify_content =
    x);
simple!(align_items, AlignItems, |s, x| s.align_items = x);
simple!(align_self, AlignSelf, |s, x| s.align_self = x);
simple!(align_content, AlignContent, |s, x| s.align_content = x);
lp_prop!(row_gap, true, |s, x| s.row_gap = x);
lp_prop!(column_gap, true, |s, x| s.column_gap = x);

// --- Grid ------------------------------------------------------------------------

fn breadth(b: &TrackBreadthSpec, c: &ComputeCtx) -> Option<TrackBreadth> {
    Some(match b {
        TrackBreadthSpec::Lp(l) => TrackBreadth::Fixed(clamp_non_negative(l.compute(&c.lengths)?)),
        TrackBreadthSpec::Flex(n) => TrackBreadth::Flex(micro_to_milli(n.micro).max(0)),
        TrackBreadthSpec::Auto => TrackBreadth::Auto,
        TrackBreadthSpec::MinContent => TrackBreadth::MinContent,
        TrackBreadthSpec::MaxContent => TrackBreadth::MaxContent,
    })
}

pub fn compute_track_size(t: &TrackSizeSpec, c: &ComputeCtx) -> Option<TrackSize> {
    Some(match t {
        TrackSizeSpec::Breadth(b) => match breadth(b, c)? {
            TrackBreadth::Fixed(l) => TrackSize::Fixed(l),
            TrackBreadth::Flex(f) => TrackSize::Flex(f),
            TrackBreadth::Auto => TrackSize::Auto,
            TrackBreadth::MinContent => TrackSize::MinContent,
            TrackBreadth::MaxContent => TrackSize::MaxContent,
        },
        TrackSizeSpec::MinMax(a, b) => TrackSize::MinMax(breadth(a, c)?, breadth(b, c)?),
        TrackSizeSpec::FitContent(l) => {
            TrackSize::FitContent(clamp_non_negative(l.compute(&c.lengths)?))
        }
    })
}

/// Expands a track list: fixed `repeat()`s are unrolled, an `auto-fill`/`auto-fit`
/// repeat is kept aside for layout, and line names are merged at boundaries.
pub fn compute_track_list(spec: &TrackListSpec, c: &ComputeCtx) -> Option<TrackList> {
    fn walk(
        entries: &[TrackEntry],
        c: &ComputeCtx,
        out: &mut TrackList,
        pending: &mut Vec<String>,
    ) -> Option<()> {
        for e in entries {
            match e {
                TrackEntry::LineNames(n) => pending.extend(n.iter().cloned()),
                TrackEntry::Track(t) => {
                    out.line_names.push(std::mem::take(pending));
                    out.tracks.push(compute_track_size(t, c)?);
                }
                TrackEntry::Repeat(RepeatCount::Fixed(n), inner) => {
                    for _ in 0..(*n).min(10_000) {
                        walk(inner, c, out, pending)?;
                    }
                }
                TrackEntry::Repeat(count, inner) => {
                    let mut rep = TrackList::default();
                    let mut rep_pending = std::mem::take(pending);
                    walk(inner, c, &mut rep, &mut rep_pending)?;
                    rep.line_names.push(rep_pending);
                    out.auto_repeat = Some(AutoRepeat {
                        fill: matches!(count, RepeatCount::AutoFill),
                        at: out.tracks.len(),
                        tracks: rep.tracks,
                        line_names: rep.line_names,
                    });
                }
            }
        }
        Some(())
    }
    let mut out = TrackList::default();
    let mut pending = Vec::new();
    walk(&spec.entries, c, &mut out, &mut pending)?;
    out.line_names.push(pending);
    Some(out)
}

pub fn grid_template_rows(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::TrackList(t) = v else {
        return false;
    };
    let Some(t) = compute_track_list(t, c) else {
        return false;
    };
    s.grid_template_rows = t;
    true
}

pub fn grid_template_columns(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    let Specified::TrackList(t) = v else {
        return false;
    };
    let Some(t) = compute_track_list(t, c) else {
        return false;
    };
    s.grid_template_columns = t;
    true
}

simple!(grid_template_areas, GridAreas, |s, x| s
    .grid_template_areas =
    x);

fn auto_tracks(v: &Specified, c: &ComputeCtx) -> Option<Vec<TrackSize>> {
    let Specified::AutoTracks(list) = v else {
        return None;
    };
    list.iter().map(|t| compute_track_size(t, c)).collect()
}

pub fn grid_auto_rows(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    match auto_tracks(v, c) {
        Some(t) => {
            s.grid_auto_rows = std::rc::Rc::new(t);
            true
        }
        None => false,
    }
}

pub fn grid_auto_columns(s: &mut ComputedStyle, v: &Specified, c: &ComputeCtx) -> bool {
    match auto_tracks(v, c) {
        Some(t) => {
            s.grid_auto_columns = std::rc::Rc::new(t);
            true
        }
        None => false,
    }
}

simple!(grid_auto_flow, GridAutoFlow, |s, x| s.grid_auto_flow = x);
simple!(grid_row_start, GridLine, |s, x| s.grid_row_start = x);
simple!(grid_row_end, GridLine, |s, x| s.grid_row_end = x);
simple!(grid_column_start, GridLine, |s, x| s.grid_column_start = x);
simple!(grid_column_end, GridLine, |s, x| s.grid_column_end = x);
simple!(justify_items, AlignItems, |s, x| s.justify_items = x);
simple!(justify_self, AlignSelf, |s, x| s.justify_self = x);

// --- Interaction and misc --------------------------------------------------------

simple!(cursor, Cursor, |s, x| s.cursor = x);
simple!(pointer_events, PointerEvents, |s, x| s.pointer_events = x);

/// A specified SVG paint as the builder uses it: colours resolved, `currentColor`
/// kept as the keyword (each painted element resolves it against its own colour).
fn svg_paint_value(v: &SvgPaintSpec, s: &ComputedStyle) -> SvgPaint {
    match v {
        SvgPaintSpec::None => SvgPaint::None,
        SvgPaintSpec::Color(ColorSpec::CurrentColor) => SvgPaint::Current,
        SvgPaintSpec::Color(c) => SvgPaint::Color(c.resolve(s.color)),
        SvgPaintSpec::Url(u, f) => SvgPaint::Url(
            u.clone(),
            f.as_ref().map(|f| Box::new(svg_paint_value(f, s))),
        ),
    }
}

pub fn fill(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::SvgPaint(p) = v else {
        return false;
    };
    s.fill = Some(svg_paint_value(p, s));
    true
}

pub fn stroke(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::SvgPaint(p) = v else {
        return false;
    };
    s.stroke = Some(svg_paint_value(p, s));
    true
}
simple!(user_select, UserSelect, |s, x| s.user_select = x);
simple!(appearance, Appearance, |s, x| s.appearance = x);
simple!(object_fit, ObjectFit, |s, x| s.object_fit = x);
simple!(aspect_ratio, AspectRatio, |s, x| s.aspect_ratio = x);
simple!(line_clamp, LineClamp, |s, x| s.line_clamp = x);
simple!(box_orient, BoxOrientVertical, |s, x| s
    .box_orient_vertical =
    x);

pub fn content(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Content(spec) = v else {
        return false;
    };
    s.content = match spec {
        ContentSpec::Normal => Content::Normal,
        ContentSpec::None => Content::None,
        ContentSpec::Items(items) => Content::Items(items.clone()),
    };
    true
}

pub fn quotes(s: &mut ComputedStyle, v: &Specified, _c: &ComputeCtx) -> bool {
    let Specified::Quotes(q) = v else {
        return false;
    };
    s.quotes = match q {
        None => ComputedStyle::initial().quotes,
        Some(pairs) => std::rc::Rc::new(pairs.clone()),
    };
    true
}

simple!(counter_reset, Counters, |s, x| s.counter_reset = x);
simple!(counter_increment, Counters, |s, x| s.counter_increment = x);

// --- Transitions and animations --------------------------------------------------

simple!(transition_property, Idents, |s, x| std::rc::Rc::make_mut(
    &mut s.transitions
)
.property = x);
simple!(transition_duration, Times, |s, x| std::rc::Rc::make_mut(
    &mut s.transitions
)
.duration = x);
simple!(transition_timing_function, Timings, |s, x| {
    std::rc::Rc::make_mut(&mut s.transitions).timing = x
});
simple!(transition_delay, Times, |s, x| std::rc::Rc::make_mut(
    &mut s.transitions
)
.delay = x);
simple!(animation_name, Idents, |s, x| std::rc::Rc::make_mut(
    &mut s.animations
)
.name = x);
simple!(animation_duration, Times, |s, x| std::rc::Rc::make_mut(
    &mut s.animations
)
.duration = x);
simple!(animation_timing_function, Timings, |s, x| {
    std::rc::Rc::make_mut(&mut s.animations).timing = x
});
simple!(animation_delay, Times, |s, x| std::rc::Rc::make_mut(
    &mut s.animations
)
.delay = x);
simple!(animation_iteration_count, IterationCounts, |s, x| {
    std::rc::Rc::make_mut(&mut s.animations).iteration_count = x
});
simple!(animation_direction, AnimationDirections, |s, x| {
    std::rc::Rc::make_mut(&mut s.animations).direction = x
});
simple!(animation_fill_mode, AnimationFillModes, |s, x| {
    std::rc::Rc::make_mut(&mut s.animations).fill_mode = x
});
simple!(animation_play_state, Bools, |s, x| std::rc::Rc::make_mut(
    &mut s.animations
)
.play_state = x);
