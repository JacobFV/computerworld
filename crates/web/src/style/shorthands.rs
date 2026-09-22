//! Shorthand properties, expanded into the longhands of the property table. Every
//! shorthand accepts the CSS-wide keywords (applied to each of its longhands) and a
//! value with `var()` (kept pending for each longhand until substitution).

use super::computed::{self, *};
use super::properties::parse as p;
use super::properties::*;
use super::values::*;
use crate::css::token::{ComponentValue, Number};

/// Why a shorthand did not expand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShorthandError {
    /// The name is not a shorthand (nor a longhand alias handled here).
    NotShorthand,
    /// The value does not match the shorthand's grammar.
    Invalid,
    /// The shorthand is recognised but its feature is not implemented (`columns`).
    Unsupported,
}

pub const SHORTHANDS: &[&str] = &[
    "all",
    "margin",
    "padding",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-width",
    "border-style",
    "border-color",
    "border-radius",
    "inset",
    "background",
    "background-position",
    "font",
    "flex",
    "flex-flow",
    "gap",
    "grid-template",
    "grid",
    "grid-area",
    "grid-row",
    "grid-column",
    "place-items",
    "place-content",
    "place-self",
    "list-style",
    "outline",
    "text-decoration",
    "overflow",
    "transition",
    "animation",
    "columns",
    "margin-inline",
    "margin-block",
    "padding-inline",
    "padding-block",
    "inset-inline",
    "inset-block",
    "border-inline",
    "border-block",
    "border-inline-start",
    "border-inline-end",
    "border-block-start",
    "border-block-end",
];

/// Logical longhands, mapped to physical ones for horizontal writing with `dir`.
fn logical_longhand(name: &str, dir: Direction) -> Option<LonghandId> {
    let (start, end) = match dir {
        Direction::Ltr => ("left", "right"),
        Direction::Rtl => ("right", "left"),
    };
    let physical = match name {
        "inline-size" => "width".to_owned(),
        "block-size" => "height".to_owned(),
        "min-inline-size" => "min-width".to_owned(),
        "min-block-size" => "min-height".to_owned(),
        "max-inline-size" => "max-width".to_owned(),
        "max-block-size" => "max-height".to_owned(),
        "inset-inline-start" => start.to_owned(),
        "inset-inline-end" => end.to_owned(),
        "inset-block-start" => "top".to_owned(),
        "inset-block-end" => "bottom".to_owned(),
        n => {
            let (prefix, rest) = n.split_once('-')?;
            if !matches!(prefix, "margin" | "padding" | "border") {
                return None;
            }
            let (side, suffix) = match rest {
                "inline-start" => (start, ""),
                "inline-end" => (end, ""),
                "block-start" => ("top", ""),
                "block-end" => ("bottom", ""),
                r => {
                    let (s, suf) = r.rsplit_once('-')?;
                    let side = match s {
                        "inline-start" => start,
                        "inline-end" => end,
                        "block-start" => "top",
                        "block-end" => "bottom",
                        _ => return None,
                    };
                    (side, suf)
                }
            };
            if suffix.is_empty() {
                format!("{prefix}-{side}")
            } else {
                format!("{prefix}-{side}-{suffix}")
            }
        }
    };
    LonghandId::by_name(&physical)
}

/// Resolves a declared property name to the longhand it sets (after aliases and
/// logical mapping), if it is a longhand.
pub fn resolve_longhand(name: &str, dir: Direction) -> Option<LonghandId> {
    let n = normalize_property_name(name);
    LonghandId::by_name(&n).or_else(|| logical_longhand(&n, dir))
}

pub fn is_shorthand(name: &str) -> bool {
    let n = normalize_property_name(name);
    SHORTHANDS.contains(&n.as_str())
}

/// Expands a shorthand declaration into `(longhand, specified)` pairs.
pub fn expand(
    name: &str,
    value: &[ComponentValue],
    dir: Direction,
) -> Result<Vec<(LonghandId, Specified)>, ShorthandError> {
    let n = normalize_property_name(name);
    let longhands = shorthand_longhands(&n, dir).ok_or(ShorthandError::NotShorthand)?;
    if n == "columns" {
        return Err(ShorthandError::Unsupported);
    }
    let mut parser = Parser::new(value);
    if let Some(k) = parse_css_wide(&mut parser) {
        return Ok(longhands
            .iter()
            .map(|id| (*id, Specified::CssWide(k)))
            .collect());
    }
    if n == "all" {
        return Err(ShorthandError::Invalid);
    }
    if contains_var(value) {
        return Ok(longhands
            .iter()
            .map(|id| {
                (
                    *id,
                    Specified::Pending {
                        property: n.clone(),
                        value: value.to_vec(),
                    },
                )
            })
            .collect());
    }
    let out = parser
        .parse_entirely(|p| expand_value(&n, p, dir))
        .ok_or(ShorthandError::Invalid)?;
    Ok(out)
}

/// The longhands a shorthand resets.
pub fn shorthand_longhands(name: &str, dir: Direction) -> Option<Vec<LonghandId>> {
    use LonghandId::*;
    let (start, end) = match dir {
        computed::Direction::Ltr => ("left", "right"),
        computed::Direction::Rtl => ("right", "left"),
    };
    let by = |names: &[String]| -> Vec<LonghandId> {
        names
            .iter()
            .filter_map(|n| LonghandId::by_name(n))
            .collect()
    };
    let side_border = |side: &str| {
        vec![
            format!("border-{side}-width"),
            format!("border-{side}-style"),
            format!("border-{side}-color"),
        ]
    };
    Some(match name {
        "all" => LonghandId::all().collect(),
        "margin" => vec![MarginTop, MarginRight, MarginBottom, MarginLeft],
        "padding" => vec![PaddingTop, PaddingRight, PaddingBottom, PaddingLeft],
        "border" => vec![
            BorderTopWidth,
            BorderRightWidth,
            BorderBottomWidth,
            BorderLeftWidth,
            BorderTopStyle,
            BorderRightStyle,
            BorderBottomStyle,
            BorderLeftStyle,
            BorderTopColor,
            BorderRightColor,
            BorderBottomColor,
            BorderLeftColor,
        ],
        "border-top" => vec![BorderTopWidth, BorderTopStyle, BorderTopColor],
        "border-right" => vec![BorderRightWidth, BorderRightStyle, BorderRightColor],
        "border-bottom" => vec![BorderBottomWidth, BorderBottomStyle, BorderBottomColor],
        "border-left" => vec![BorderLeftWidth, BorderLeftStyle, BorderLeftColor],
        "border-width" => vec![
            BorderTopWidth,
            BorderRightWidth,
            BorderBottomWidth,
            BorderLeftWidth,
        ],
        "border-style" => vec![
            BorderTopStyle,
            BorderRightStyle,
            BorderBottomStyle,
            BorderLeftStyle,
        ],
        "border-color" => vec![
            BorderTopColor,
            BorderRightColor,
            BorderBottomColor,
            BorderLeftColor,
        ],
        "border-radius" => vec![
            BorderTopLeftRadius,
            BorderTopRightRadius,
            BorderBottomRightRadius,
            BorderBottomLeftRadius,
        ],
        "inset" => vec![Top, Right, Bottom, Left],
        "background" => vec![
            BackgroundColor,
            BackgroundImage,
            BackgroundPositionX,
            BackgroundPositionY,
            BackgroundSize,
            BackgroundRepeat,
            BackgroundOrigin,
            BackgroundClip,
            BackgroundAttachment,
        ],
        "background-position" => vec![BackgroundPositionX, BackgroundPositionY],
        "font" => vec![
            FontStyle,
            FontVariant,
            FontWeight,
            FontSize,
            LineHeight,
            FontFamily,
        ],
        "flex" => vec![FlexGrow, FlexShrink, FlexBasis],
        "flex-flow" => vec![FlexDirection, FlexWrap],
        "gap" => vec![RowGap, ColumnGap],
        "grid-template" => vec![GridTemplateRows, GridTemplateColumns, GridTemplateAreas],
        "grid" => vec![
            GridTemplateRows,
            GridTemplateColumns,
            GridTemplateAreas,
            GridAutoRows,
            GridAutoColumns,
            GridAutoFlow,
        ],
        "grid-area" => vec![GridRowStart, GridColumnStart, GridRowEnd, GridColumnEnd],
        "grid-row" => vec![GridRowStart, GridRowEnd],
        "grid-column" => vec![GridColumnStart, GridColumnEnd],
        "place-items" => vec![AlignItems, JustifyItems],
        "place-content" => vec![AlignContent, JustifyContent],
        "place-self" => vec![AlignSelf, JustifySelf],
        "list-style" => vec![ListStyleType, ListStylePosition, ListStyleImage],
        "outline" => vec![OutlineWidth, OutlineStyle, OutlineColor],
        "text-decoration" => vec![TextDecorationLine, TextDecorationStyle, TextDecorationColor],
        "overflow" => vec![OverflowX, OverflowY],
        "transition" => vec![
            TransitionProperty,
            TransitionDuration,
            TransitionTimingFunction,
            TransitionDelay,
        ],
        "animation" => vec![
            AnimationName,
            AnimationDuration,
            AnimationTimingFunction,
            AnimationDelay,
            AnimationIterationCount,
            AnimationDirection,
            AnimationFillMode,
            AnimationPlayState,
        ],
        "columns" => Vec::new(),
        "margin-inline" => by(&[format!("margin-{start}"), format!("margin-{end}")]),
        "margin-block" => vec![MarginTop, MarginBottom],
        "padding-inline" => by(&[format!("padding-{start}"), format!("padding-{end}")]),
        "padding-block" => vec![PaddingTop, PaddingBottom],
        "inset-inline" => by(&[start.to_owned(), end.to_owned()]),
        "inset-block" => vec![Top, Bottom],
        "border-inline" => by(&[side_border(start), side_border(end)].concat()),
        "border-block" => by(&[side_border("top"), side_border("bottom")].concat()),
        "border-inline-start" => by(&side_border(start)),
        "border-inline-end" => by(&side_border(end)),
        "border-block-start" => by(&side_border("top")),
        "border-block-end" => by(&side_border("bottom")),
        _ => return None,
    })
}

type Out = Vec<(LonghandId, Specified)>;

/// Up to four values in the top/right/bottom/left order.
fn four<T: Clone>(p: &mut Parser, mut f: impl FnMut(&mut Parser) -> Option<T>) -> Option<[T; 4]> {
    let a = f(p)?;
    let b = f(p);
    let c = if b.is_some() { f(p) } else { None };
    let d = if c.is_some() { f(p) } else { None };
    let b = b.unwrap_or_else(|| a.clone());
    let c = c.unwrap_or_else(|| a.clone());
    let d = d.unwrap_or_else(|| b.clone());
    Some([a, b, c, d])
}

fn expand_value(name: &str, p: &mut Parser, dir: Direction) -> Option<Out> {
    use LonghandId::*;
    let ids = shorthand_longhands(name, dir)?;
    Some(match name {
        "margin" | "margin-inline" | "margin-block" | "inset" | "inset-inline" | "inset-block" => {
            let f = |p: &mut Parser| p::lpa_spec(p).map(Specified::Lpa);
            if ids.len() == 4 {
                let v = four(p, f)?;
                ids.into_iter().zip(v).collect()
            } else {
                let a = f(p)?;
                let b = f(p).unwrap_or_else(|| a.clone());
                vec![(ids[0], a), (ids[1], b)]
            }
        }
        "padding" | "padding-inline" | "padding-block" => {
            let f = |p: &mut Parser| parse_lp(p, Allow::NON_NEGATIVE).map(Specified::Lp);
            if ids.len() == 4 {
                let v = four(p, f)?;
                ids.into_iter().zip(v).collect()
            } else {
                let a = f(p)?;
                let b = f(p).unwrap_or_else(|| a.clone());
                vec![(ids[0], a), (ids[1], b)]
            }
        }
        "border-width" => {
            let v = four(p, |p| p::border_width_spec(p).map(Specified::BorderWidth))?;
            ids.into_iter().zip(v).collect()
        }
        "border-style" => {
            let v = four(p, |p| {
                p::border_style_keyword(p).map(Specified::BorderStyle)
            })?;
            ids.into_iter().zip(v).collect()
        }
        "border-color" => {
            let v = four(p, |p| parse_color(p).map(Specified::Color))?;
            ids.into_iter().zip(v).collect()
        }
        "border"
        | "border-top"
        | "border-right"
        | "border-bottom"
        | "border-left"
        | "border-inline"
        | "border-block"
        | "border-inline-start"
        | "border-inline-end"
        | "border-block-start"
        | "border-block-end"
        | "outline" => {
            let (w, s, c) = border_triple(p, name == "outline")?;
            let mut out = Vec::new();
            for id in ids {
                let n = id.name();
                let v = if n.ends_with("-width") {
                    Specified::BorderWidth(w.clone())
                } else if n.ends_with("-style") {
                    Specified::BorderStyle(s)
                } else {
                    Specified::Color(c.clone())
                };
                out.push((id, v));
            }
            out
        }
        "border-radius" => {
            let h = four(p, |p| parse_lp(p, Allow::NON_NEGATIVE))?;
            let v = if p.expect_delim('/').is_some() {
                four(p, |p| parse_lp(p, Allow::NON_NEGATIVE))?
            } else {
                h.clone()
            };
            ids.into_iter()
                .zip(h.into_iter().zip(v))
                .map(|(id, (a, b))| (id, Specified::LpPair(a, b)))
                .collect()
        }
        "background" => background(p)?,
        "background-position" => {
            let list = p.comma_list(parse_position)?;
            let (xs, ys): (Vec<_>, Vec<_>) = list.into_iter().unzip();
            vec![
                (BackgroundPositionX, Specified::LpList(xs)),
                (BackgroundPositionY, Specified::LpList(ys)),
            ]
        }
        "font" => font(p)?,
        "flex" => flex(p)?,
        "flex-flow" => {
            let mut d = None;
            let mut w = None;
            for _ in 0..2 {
                if d.is_none() {
                    if let Some(v) = p::flex_direction(p) {
                        d = Some(v);
                        continue;
                    }
                }
                if w.is_none() {
                    if let Some(v) = p::flex_wrap(p) {
                        w = Some(v);
                        continue;
                    }
                }
                break;
            }
            if d.is_none() && w.is_none() {
                return None;
            }
            vec![
                (
                    FlexDirection,
                    d.unwrap_or(Specified::FlexDirection(computed::FlexDirection::Row)),
                ),
                (
                    FlexWrap,
                    w.unwrap_or(Specified::FlexWrap(computed::FlexWrap::NoWrap)),
                ),
            ]
        }
        "gap" => {
            let a = p::gap(p)?;
            let b = p::gap(p).unwrap_or_else(|| a.clone());
            vec![(RowGap, a), (ColumnGap, b)]
        }
        "grid-template" => grid_template(p)?,
        "grid" => grid(p)?,
        "grid-area" => {
            let a = p::grid_line_spec(p)?;
            let mut lines = vec![a];
            while p.expect_delim('/').is_some() {
                lines.push(p::grid_line_spec(p)?);
            }
            if lines.len() > 4 {
                return None;
            }
            let implied = |l: &GridLine| -> GridLine {
                match l {
                    GridLine::Name(n) => GridLine::Name(n.clone()),
                    _ => GridLine::Auto,
                }
            };
            let row_start = lines[0].clone();
            let col_start = lines.get(1).cloned().unwrap_or_else(|| implied(&row_start));
            let row_end = lines.get(2).cloned().unwrap_or_else(|| implied(&row_start));
            let col_end = lines.get(3).cloned().unwrap_or_else(|| implied(&col_start));
            vec![
                (GridRowStart, Specified::GridLine(row_start)),
                (GridColumnStart, Specified::GridLine(col_start)),
                (GridRowEnd, Specified::GridLine(row_end)),
                (GridColumnEnd, Specified::GridLine(col_end)),
            ]
        }
        "grid-row" | "grid-column" => {
            let a = p::grid_line_spec(p)?;
            let b = if p.expect_delim('/').is_some() {
                p::grid_line_spec(p)?
            } else {
                match &a {
                    GridLine::Name(n) => GridLine::Name(n.clone()),
                    _ => GridLine::Auto,
                }
            };
            vec![
                (ids[0], Specified::GridLine(a)),
                (ids[1], Specified::GridLine(b)),
            ]
        }
        "place-items" => {
            let a = p::align_items(p)?;
            let b = p::justify_items(p).unwrap_or_else(|| a.clone());
            vec![(AlignItems, a), (JustifyItems, b)]
        }
        "place-content" => {
            let a = p::align_content(p)?;
            let b = p::justify_content(p).unwrap_or_else(|| match &a {
                Specified::AlignContent(ac) => Specified::JustifyContent(match ac {
                    computed::AlignContent::Normal => computed::JustifyContent::FlexStart,
                    computed::AlignContent::FlexStart => computed::JustifyContent::FlexStart,
                    computed::AlignContent::FlexEnd => computed::JustifyContent::FlexEnd,
                    computed::AlignContent::Center => computed::JustifyContent::Center,
                    computed::AlignContent::SpaceBetween => computed::JustifyContent::SpaceBetween,
                    computed::AlignContent::SpaceAround => computed::JustifyContent::SpaceAround,
                    computed::AlignContent::SpaceEvenly => computed::JustifyContent::SpaceEvenly,
                    computed::AlignContent::Stretch => computed::JustifyContent::Stretch,
                    computed::AlignContent::Start => computed::JustifyContent::Start,
                    computed::AlignContent::End => computed::JustifyContent::End,
                }),
                _ => unreachable!(),
            });
            vec![(AlignContent, a), (JustifyContent, b)]
        }
        "place-self" => {
            let a = p::align_self(p)?;
            let b = p::align_self(p).unwrap_or_else(|| a.clone());
            vec![(AlignSelf, a), (JustifySelf, b)]
        }
        "list-style" => list_style(p)?,
        "text-decoration" => {
            let mut line = None;
            let mut style = None;
            let mut color = None;
            for _ in 0..3 {
                if line.is_none() {
                    if let Some(v) = p::text_decoration_line(p) {
                        line = Some(v);
                        continue;
                    }
                }
                if style.is_none() {
                    if let Some(v) = p::text_decoration_style(p) {
                        style = Some(v);
                        continue;
                    }
                }
                if color.is_none() {
                    if let Some(v) = p::color(p) {
                        color = Some(v);
                        continue;
                    }
                }
                break;
            }
            if line.is_none() && style.is_none() && color.is_none() {
                return None;
            }
            vec![
                (
                    TextDecorationLine,
                    line.unwrap_or(Specified::TextDecorationLine {
                        underline: false,
                        overline: false,
                        line_through: false,
                    }),
                ),
                (
                    TextDecorationStyle,
                    style.unwrap_or(Specified::TextDecorationStyle(
                        computed::TextDecorationStyle::Solid,
                    )),
                ),
                (
                    TextDecorationColor,
                    color.unwrap_or(Specified::Color(ColorSpec::CurrentColor)),
                ),
            ]
        }
        "overflow" => {
            let a = p::overflow_keyword(p)?;
            let b = p::overflow_keyword(p).unwrap_or(a);
            vec![
                (OverflowX, Specified::Overflow(a)),
                (OverflowY, Specified::Overflow(b)),
            ]
        }
        "transition" => transition(p)?,
        "animation" => animation(p)?,
        _ => return None,
    })
}

fn border_triple(
    p: &mut Parser,
    outline: bool,
) -> Option<(BorderWidthSpec, BorderStyle, ColorSpec)> {
    let mut w = None;
    let mut s = None;
    let mut c = None;
    for _ in 0..3 {
        if w.is_none() {
            if let Some(v) = p::border_width_spec(p) {
                w = Some(v);
                continue;
            }
        }
        if s.is_none() {
            let v = if outline {
                p.try_parse(|p| {
                    if p.expect_ident_matching("auto").is_some() {
                        Some(BorderStyle::Solid)
                    } else {
                        p::border_style_keyword(p)
                    }
                })
            } else {
                p::border_style_keyword(p)
            };
            if let Some(v) = v {
                s = Some(v);
                continue;
            }
        }
        if c.is_none() {
            let v = if outline && p.expect_ident_matching("invert").is_some() {
                Some(ColorSpec::CurrentColor)
            } else {
                parse_color(p)
            };
            if let Some(v) = v {
                c = Some(v);
                continue;
            }
        }
        break;
    }
    if w.is_none() && s.is_none() && c.is_none() {
        return None;
    }
    Some((
        w.unwrap_or(BorderWidthSpec::Medium),
        s.unwrap_or(BorderStyle::None),
        c.unwrap_or(ColorSpec::CurrentColor),
    ))
}

fn background(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    struct Layer {
        image: Option<ImageSpec>,
        position: Option<(LpSpec, LpSpec)>,
        size: Option<BgSizeSpec>,
        repeat: Option<computed::BackgroundRepeat>,
        attachment: Option<bool>,
        boxes: Vec<BackgroundBox>,
    }
    let mut layers: Vec<Layer> = Vec::new();
    let mut color: Option<ColorSpec> = None;
    loop {
        let mut layer = Layer {
            image: None,
            position: None,
            size: None,
            repeat: None,
            attachment: None,
            boxes: Vec::new(),
        };
        let mut any = false;
        loop {
            if layer.image.is_none() {
                if let Some(i) = parse_image(p) {
                    layer.image = Some(i);
                    any = true;
                    continue;
                }
            }
            if layer.position.is_none() {
                if let Some(pos) = parse_position(p) {
                    layer.position = Some(pos);
                    any = true;
                    if p.expect_delim('/').is_some() {
                        layer.size = Some(p::bg_size(p)?);
                    }
                    continue;
                }
            }
            if layer.repeat.is_none() {
                if let Some(r) = p::repeat_style(p) {
                    layer.repeat = Some(r);
                    any = true;
                    continue;
                }
            }
            if layer.attachment.is_none() {
                if let Some(a) = p::attachment_keyword(p) {
                    layer.attachment = Some(a);
                    any = true;
                    continue;
                }
            }
            if layer.boxes.len() < 2 {
                if let Some(b) = p::background_box_keyword(p) {
                    layer.boxes.push(b);
                    any = true;
                    continue;
                }
            }
            if color.is_none() {
                if let Some(c) = parse_color(p) {
                    color = Some(c);
                    any = true;
                    continue;
                }
            }
            break;
        }
        if !any {
            return None;
        }
        layers.push(layer);
        if p.expect_comma().is_some() {
            // Colour is only allowed in the last layer.
            if color.is_some() {
                return None;
            }
            continue;
        }
        break;
    }
    let pct = |v: i64| LpSpec::Percent(Number::from_i64(v));
    let mut images = Vec::new();
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    let mut sizes = Vec::new();
    let mut repeats = Vec::new();
    let mut origins = Vec::new();
    let mut clips = Vec::new();
    let mut attachments = Vec::new();
    for l in layers {
        images.push(l.image.unwrap_or(ImageSpec::None));
        let (x, y) = l.position.unwrap_or((pct(0), pct(0)));
        xs.push(x);
        ys.push(y);
        sizes.push(l.size.unwrap_or(BgSizeSpec::Auto));
        repeats.push(l.repeat.unwrap_or(computed::BackgroundRepeat::Repeat));
        attachments.push(l.attachment.unwrap_or(false));
        let origin = l
            .boxes
            .first()
            .copied()
            .unwrap_or(BackgroundBox::PaddingBox);
        let clip = l.boxes.get(1).copied().unwrap_or(if l.boxes.is_empty() {
            BackgroundBox::BorderBox
        } else {
            origin
        });
        origins.push(origin);
        clips.push(clip);
    }
    Some(vec![
        (
            BackgroundColor,
            Specified::Color(color.unwrap_or(ColorSpec::Rgba(cw_scene::Color::TRANSPARENT))),
        ),
        (BackgroundImage, Specified::Images(images)),
        (BackgroundPositionX, Specified::LpList(xs)),
        (BackgroundPositionY, Specified::LpList(ys)),
        (BackgroundSize, Specified::BgSizes(sizes)),
        (BackgroundRepeat, Specified::Repeats(repeats)),
        (BackgroundOrigin, Specified::BgBoxes(origins)),
        (BackgroundClip, Specified::BgBoxes(clips)),
        (BackgroundAttachment, Specified::Bools(attachments)),
    ])
}

fn font(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    // System fonts: accepted, mapped to the UI font at 13.333px (what Chromium reports).
    if let Some(()) = p.try_parse(|p| {
        let s = p.expect_ident_lower()?;
        if matches!(
            s.as_str(),
            "caption" | "icon" | "menu" | "message-box" | "small-caption" | "status-bar"
        ) && p.is_done()
        {
            Some(())
        } else {
            None
        }
    }) {
        let size = LpSpec::Length(Length {
            value: Number {
                micro: 13_333_333,
                int: false,
            },
            unit: LengthUnit::Px,
        });
        return Some(vec![
            (FontStyle, Specified::FontStyle(computed::FontStyle::Normal)),
            (FontVariant, Specified::Bool(false)),
            (
                FontWeight,
                Specified::FontWeight(FontWeightSpec::Absolute(400)),
            ),
            (FontSize, Specified::FontSize(FontSizeSpec::Lp(size))),
            (LineHeight, Specified::LineHeight(LineHeightSpec::Normal)),
            (FontFamily, Specified::FontFamily(vec!["system-ui".into()])),
        ]);
    }
    let mut style = None;
    let mut variant = None;
    let mut weight = None;
    // Up to three `normal`-able prefix values in any order (`font-stretch` accepted).
    for _ in 0..4 {
        if p.expect_ident_matching("normal").is_some() {
            continue;
        }
        if style.is_none() {
            if let Some(v) = p::font_style_keyword(p) {
                style = Some(v);
                continue;
            }
        }
        if variant.is_none() && p.expect_ident_matching("small-caps").is_some() {
            variant = Some(true);
            continue;
        }
        if weight.is_none() {
            if let Some(v) = p.try_parse(|p| {
                // A bare number here must be a weight, not a size.
                if matches!(
                    p.peek(),
                    Some(ComponentValue::Token(
                        crate::css::token::Token::Number { .. }
                    ))
                ) || p.peek_ident_lower().is_some()
                {
                    p::font_weight_spec(p)
                } else {
                    None
                }
            }) {
                weight = Some(v);
                continue;
            }
        }
        if p.try_parse(|p| {
            let s = p.expect_ident_lower()?;
            if matches!(
                s.as_str(),
                "ultra-condensed"
                    | "extra-condensed"
                    | "condensed"
                    | "semi-condensed"
                    | "semi-expanded"
                    | "expanded"
                    | "extra-expanded"
                    | "ultra-expanded"
            ) {
                Some(())
            } else {
                None
            }
        })
        .is_some()
        {
            continue;
        }
        break;
    }
    let size = p::font_size_spec(p)?;
    let line_height = if p.expect_delim('/').is_some() {
        p::line_height_spec(p)?
    } else {
        LineHeightSpec::Normal
    };
    let family = p::font_family_list(p)?;
    Some(vec![
        (
            FontStyle,
            Specified::FontStyle(style.unwrap_or(computed::FontStyle::Normal)),
        ),
        (FontVariant, Specified::Bool(variant.unwrap_or(false))),
        (
            FontWeight,
            Specified::FontWeight(weight.unwrap_or(FontWeightSpec::Absolute(400))),
        ),
        (FontSize, Specified::FontSize(size)),
        (LineHeight, Specified::LineHeight(line_height)),
        (FontFamily, Specified::FontFamily(family)),
    ])
}

fn flex(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    let one = Number::from_i64(1);
    let zero = Number::ZERO;
    let out = |g: Number, s: Number, b: SizingSpec| {
        vec![
            (FlexGrow, Specified::Number(g)),
            (FlexShrink, Specified::Number(s)),
            (FlexBasis, Specified::Sizing(b)),
        ]
    };
    if p.expect_ident_matching("none").is_some() {
        return Some(out(zero, zero, SizingSpec::Auto));
    }
    if p.expect_ident_matching("auto").is_some() {
        return Some(out(one, one, SizingSpec::Auto));
    }
    let mut grow = None;
    let mut shrink = None;
    let mut basis = None;
    for _ in 0..3 {
        if grow.is_none() {
            if let Some(n) = p.try_parse(|p| {
                let n = p.expect_number()?;
                if n.is_negative() {
                    None
                } else {
                    Some(n)
                }
            }) {
                grow = Some(n);
                shrink = p.try_parse(|p| {
                    let n = p.expect_number()?;
                    if n.is_negative() {
                        None
                    } else {
                        Some(n)
                    }
                });
                continue;
            }
        }
        if basis.is_none() {
            if let Some(Specified::Sizing(b)) = p::flex_basis(p) {
                basis = Some(b);
                continue;
            }
        }
        break;
    }
    if grow.is_none() && basis.is_none() {
        return None;
    }
    let basis = basis.unwrap_or(SizingSpec::Lp(LpSpec::Percent(Number::ZERO)));
    Some(out(grow.unwrap_or(one), shrink.unwrap_or(one), basis))
}

/// `grid-template`'s areas form: `[ <line-names>? <string> <track-size>? <line-names>? ]+ [ / <explicit-track-list> ]?`.
fn grid_template_areas_form(
    p: &mut Parser,
) -> Option<(TrackListSpec, TrackListSpec, Vec<Vec<String>>)> {
    p.try_parse(|p| {
        let mut rows = Vec::new();
        let mut areas: Vec<Vec<String>> = Vec::new();
        let mut pending_names: Vec<String> = Vec::new();
        loop {
            if let Some(names) = p.try_parse(|p| {
                let b = p.expect_square_block()?;
                let mut inner = Parser::new(b);
                let mut names = Vec::new();
                while let Some(n) = parse_custom_ident(&mut inner) {
                    names.push(n);
                }
                if inner.is_done() {
                    Some(names)
                } else {
                    None
                }
            }) {
                pending_names.extend(names);
            }
            let Some(s) = p.expect_string() else { break };
            let row = Parser::new(&[tok_string(s)]).parse_entirely(p::grid_template_areas)?;
            let Specified::GridAreas(mut r) = row else {
                return None;
            };
            let r = r.pop()?;
            if let Some(first) = areas.first() {
                if first.len() != r.len() {
                    return None;
                }
            }
            areas.push(r);
            if !pending_names.is_empty() {
                rows.push(TrackEntry::LineNames(std::mem::take(&mut pending_names)));
            }
            let size = p::track_size(p).unwrap_or(TrackSizeSpec::Breadth(TrackBreadthSpec::Auto));
            rows.push(TrackEntry::Track(size));
        }
        if areas.is_empty() {
            return None;
        }
        if !pending_names.is_empty() {
            rows.push(TrackEntry::LineNames(pending_names));
        }
        let cols = if p.expect_delim('/').is_some() {
            p::track_list_spec(p)?
        } else {
            TrackListSpec::default()
        };
        Some((TrackListSpec { entries: rows }, cols, areas))
    })
}

fn grid_template(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    let (rows, cols, areas) = grid_template_parts(p)?;
    Some(vec![
        (GridTemplateRows, Specified::TrackList(rows)),
        (GridTemplateColumns, Specified::TrackList(cols)),
        (GridTemplateAreas, Specified::GridAreas(areas)),
    ])
}

fn grid_template_parts(p: &mut Parser) -> Option<(TrackListSpec, TrackListSpec, Vec<Vec<String>>)> {
    if p.expect_ident_matching("none").is_some() {
        return Some((
            TrackListSpec::default(),
            TrackListSpec::default(),
            Vec::new(),
        ));
    }
    if let Some(v) = grid_template_areas_form(p) {
        return Some(v);
    }
    let rows = p::track_list_spec(p)?;
    p.expect_delim('/')?;
    let cols = p::track_list_spec(p)?;
    Some((rows, cols, Vec::new()))
}

fn grid(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    let auto_flow = |p: &mut Parser| -> Option<bool> {
        // `auto-flow && dense?` in either order; returns `dense`.
        p.try_parse(|p| {
            let mut flow = false;
            let mut dense = false;
            for _ in 0..2 {
                if !flow && p.expect_ident_matching("auto-flow").is_some() {
                    flow = true;
                    continue;
                }
                if !dense && p.expect_ident_matching("dense").is_some() {
                    dense = true;
                    continue;
                }
                break;
            }
            if flow {
                Some(dense)
            } else {
                None
            }
        })
    };
    let auto_tracks = |p: &mut Parser| -> Vec<TrackSizeSpec> {
        let mut v = Vec::new();
        while let Some(t) = p::track_size(p) {
            v.push(t);
        }
        v
    };
    let base = |rows, cols, areas, auto_rows, auto_cols, flow| {
        vec![
            (GridTemplateRows, Specified::TrackList(rows)),
            (GridTemplateColumns, Specified::TrackList(cols)),
            (GridTemplateAreas, Specified::GridAreas(areas)),
            (GridAutoRows, Specified::AutoTracks(auto_rows)),
            (GridAutoColumns, Specified::AutoTracks(auto_cols)),
            (GridAutoFlow, Specified::GridAutoFlow(flow)),
        ]
    };
    let auto = vec![TrackSizeSpec::Breadth(TrackBreadthSpec::Auto)];
    // `[auto-flow && dense?] <auto-rows>? / <columns>`
    if let Some(dense) = auto_flow(p) {
        let mut rows = auto_tracks(p);
        if rows.is_empty() {
            rows = auto.clone();
        }
        p.expect_delim('/')?;
        let cols = p::track_list_spec(p)?;
        return Some(base(
            TrackListSpec::default(),
            cols,
            Vec::new(),
            rows,
            auto,
            if dense {
                computed::GridAutoFlow::RowDense
            } else {
                computed::GridAutoFlow::Row
            },
        ));
    }
    // `<rows> / [auto-flow && dense?] <auto-columns>?`
    if let Some(v) = p.try_parse(|p| {
        let rows = p::track_list_spec(p)?;
        p.expect_delim('/')?;
        let dense = auto_flow(p)?;
        let mut cols = auto_tracks(p);
        if cols.is_empty() {
            cols = auto.clone();
        }
        Some(base(
            rows,
            TrackListSpec::default(),
            Vec::new(),
            auto.clone(),
            cols,
            if dense {
                computed::GridAutoFlow::ColumnDense
            } else {
                computed::GridAutoFlow::Column
            },
        ))
    }) {
        return Some(v);
    }
    let (rows, cols, areas) = grid_template_parts(p)?;
    Some(base(
        rows,
        cols,
        areas,
        auto.clone(),
        auto,
        computed::GridAutoFlow::Row,
    ))
}

fn list_style(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    let mut ty = None;
    let mut pos = None;
    let mut image = None;
    let mut nones = 0;
    for _ in 0..3 {
        if p.expect_ident_matching("none").is_some() {
            nones += 1;
            continue;
        }
        if pos.is_none() {
            if let Some(v) = p::list_style_position_keyword(p) {
                pos = Some(v);
                continue;
            }
        }
        if image.is_none() {
            if let Some(v) = p.try_parse(|p| {
                let i = parse_image(p)?;
                if i == ImageSpec::None {
                    None
                } else {
                    Some(i)
                }
            }) {
                image = Some(v);
                continue;
            }
        }
        if ty.is_none() {
            if let Some(v) = p::list_style_type_keyword(p) {
                ty = Some(v);
                continue;
            }
        }
        break;
    }
    if nones > 1 || (ty.is_none() && pos.is_none() && image.is_none() && nones == 0) {
        return None;
    }
    if nones == 1 {
        if ty.is_none() {
            ty = Some(computed::ListStyleType::None);
        } else if image.is_some() {
            return None;
        }
    }
    Some(vec![
        (
            ListStyleType,
            Specified::ListStyleType(ty.unwrap_or(computed::ListStyleType::Disc)),
        ),
        (
            ListStylePosition,
            Specified::ListStylePosition(pos.unwrap_or(computed::ListStylePosition::Outside)),
        ),
        (
            ListStyleImage,
            Specified::Images(vec![image.unwrap_or(ImageSpec::None)]),
        ),
    ])
}

fn transition(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    struct Item {
        property: Option<String>,
        duration: Option<i32>,
        timing: Option<TimingFunction>,
        delay: Option<i32>,
    }
    let items = p.comma_list(|p| {
        let mut it = Item {
            property: None,
            duration: None,
            timing: None,
            delay: None,
        };
        let mut any = false;
        for _ in 0..4 {
            if let Some(t) = parse_time(p) {
                if it.duration.is_none() {
                    it.duration = Some(t);
                } else if it.delay.is_none() {
                    it.delay = Some(t);
                } else {
                    return None;
                }
                any = true;
                continue;
            }
            if it.timing.is_none() {
                if let Some(t) = p::timing_function(p) {
                    it.timing = Some(t);
                    any = true;
                    continue;
                }
            }
            if it.property.is_none() {
                if let Some(s) = p.expect_ident() {
                    it.property = Some(s.to_ascii_lowercase());
                    any = true;
                    continue;
                }
            }
            break;
        }
        if any {
            Some(it)
        } else {
            None
        }
    })?;
    if items.len() > 1 && items.iter().any(|i| i.property.as_deref() == Some("none")) {
        return None;
    }
    Some(vec![
        (
            TransitionProperty,
            Specified::Idents(
                items
                    .iter()
                    .map(|i| i.property.clone().unwrap_or_else(|| "all".into()))
                    .collect(),
            ),
        ),
        (
            TransitionDuration,
            Specified::Times(items.iter().map(|i| i.duration.unwrap_or(0)).collect()),
        ),
        (
            TransitionTimingFunction,
            Specified::Timings(
                items
                    .iter()
                    .map(|i| i.timing.unwrap_or(TimingFunction::Ease))
                    .collect(),
            ),
        ),
        (
            TransitionDelay,
            Specified::Times(items.iter().map(|i| i.delay.unwrap_or(0)).collect()),
        ),
    ])
}

fn animation(p: &mut Parser) -> Option<Out> {
    use LonghandId::*;
    #[derive(Default)]
    struct Item {
        name: Option<String>,
        duration: Option<i32>,
        timing: Option<TimingFunction>,
        delay: Option<i32>,
        count: Option<Option<i32>>,
        direction: Option<computed::AnimationDirection>,
        fill: Option<computed::AnimationFillMode>,
        play: Option<bool>,
    }
    let items = p.comma_list(|p| {
        let mut it = Item::default();
        let mut any = false;
        for _ in 0..8 {
            if let Some(t) = parse_time(p) {
                if it.duration.is_none() {
                    it.duration = Some(t);
                } else if it.delay.is_none() {
                    it.delay = Some(t);
                } else {
                    return None;
                }
                any = true;
                continue;
            }
            if it.timing.is_none() {
                if let Some(t) = p::timing_function(p) {
                    it.timing = Some(t);
                    any = true;
                    continue;
                }
            }
            if it.count.is_none() {
                if let Some(c) = p::iteration_count(p) {
                    it.count = Some(c);
                    any = true;
                    continue;
                }
            }
            if it.direction.is_none() {
                if let Some(d) = p::animation_direction_keyword(p) {
                    it.direction = Some(d);
                    any = true;
                    continue;
                }
            }
            if it.fill.is_none() {
                if let Some(f) = p::animation_fill_mode_keyword(p) {
                    it.fill = Some(f);
                    any = true;
                    continue;
                }
            }
            if it.play.is_none() {
                if let Some(s) = p::animation_play_state_keyword(p) {
                    it.play = Some(s);
                    any = true;
                    continue;
                }
            }
            if it.name.is_none() {
                if let Some(s) = p.expect_string() {
                    it.name = Some(s.to_owned());
                    any = true;
                    continue;
                }
                if p.expect_ident_matching("none").is_some() {
                    it.name = Some("none".into());
                    any = true;
                    continue;
                }
                if let Some(s) = parse_custom_ident(p) {
                    it.name = Some(s);
                    any = true;
                    continue;
                }
            }
            break;
        }
        if any {
            Some(it)
        } else {
            None
        }
    })?;
    Some(vec![
        (
            AnimationName,
            Specified::Idents(
                items
                    .iter()
                    .map(|i| i.name.clone().unwrap_or_else(|| "none".into()))
                    .collect(),
            ),
        ),
        (
            AnimationDuration,
            Specified::Times(items.iter().map(|i| i.duration.unwrap_or(0)).collect()),
        ),
        (
            AnimationTimingFunction,
            Specified::Timings(
                items
                    .iter()
                    .map(|i| i.timing.unwrap_or(TimingFunction::Ease))
                    .collect(),
            ),
        ),
        (
            AnimationDelay,
            Specified::Times(items.iter().map(|i| i.delay.unwrap_or(0)).collect()),
        ),
        (
            AnimationIterationCount,
            Specified::IterationCounts(
                items
                    .iter()
                    .map(|i| i.count.unwrap_or(Some(1000)))
                    .collect(),
            ),
        ),
        (
            AnimationDirection,
            Specified::AnimationDirections(
                items
                    .iter()
                    .map(|i| i.direction.unwrap_or_default())
                    .collect(),
            ),
        ),
        (
            AnimationFillMode,
            Specified::AnimationFillModes(
                items.iter().map(|i| i.fill.unwrap_or_default()).collect(),
            ),
        ),
        (
            AnimationPlayState,
            Specified::Bools(items.iter().map(|i| i.play.unwrap_or(true)).collect()),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::values::test_util::tokenize;
    use LonghandId::*;

    fn ex(name: &str, v: &str) -> Result<Vec<(LonghandId, Specified)>, ShorthandError> {
        expand(name, &tokenize(v), computed::Direction::Ltr)
    }
    fn get(out: &[(LonghandId, Specified)], id: LonghandId) -> &Specified {
        &out.iter().find(|(i, _)| *i == id).unwrap().1
    }
    fn px(v: i64) -> LpSpec {
        LpSpec::px(v)
    }

    #[test]
    fn margin_and_padding() {
        let m = ex("margin", "1px 2px 3px 4px").unwrap();
        assert_eq!(get(&m, MarginTop), &Specified::Lpa(LpaSpec::Lp(px(1))));
        assert_eq!(get(&m, MarginRight), &Specified::Lpa(LpaSpec::Lp(px(2))));
        assert_eq!(get(&m, MarginBottom), &Specified::Lpa(LpaSpec::Lp(px(3))));
        assert_eq!(get(&m, MarginLeft), &Specified::Lpa(LpaSpec::Lp(px(4))));
        let m = ex("margin", "1px 2px").unwrap();
        assert_eq!(get(&m, MarginBottom), &Specified::Lpa(LpaSpec::Lp(px(1))));
        assert_eq!(get(&m, MarginLeft), &Specified::Lpa(LpaSpec::Lp(px(2))));
        let m = ex("margin", "0 auto").unwrap();
        assert_eq!(get(&m, MarginLeft), &Specified::Lpa(LpaSpec::Auto));
        assert_eq!(
            ex("margin", "1px 2px 3px 4px 5px"),
            Err(ShorthandError::Invalid)
        );
        assert_eq!(ex("padding", "auto"), Err(ShorthandError::Invalid));
        assert_eq!(ex("padding", "-1px"), Err(ShorthandError::Invalid));
        let pd = ex("padding", "1em").unwrap();
        assert_eq!(pd.len(), 4);
        assert_eq!(
            ex("margin", "inherit").unwrap()[0].1,
            Specified::CssWide(CssWide::Inherit)
        );
        assert_eq!(ex("nope", "1px"), Err(ShorthandError::NotShorthand));
        assert_eq!(ex("columns", "2"), Err(ShorthandError::Unsupported));
    }

    #[test]
    fn borders() {
        let b = ex("border", "1px solid red").unwrap();
        assert_eq!(b.len(), 12);
        assert_eq!(
            get(&b, BorderLeftStyle),
            &Specified::BorderStyle(computed::BorderStyle::Solid)
        );
        assert_eq!(
            get(&b, BorderTopWidth),
            &Specified::BorderWidth(BorderWidthSpec::Length(px(1)))
        );
        let b = ex("border", "red").unwrap();
        assert_eq!(
            get(&b, BorderTopWidth),
            &Specified::BorderWidth(BorderWidthSpec::Medium)
        );
        assert_eq!(
            get(&b, BorderTopStyle),
            &Specified::BorderStyle(computed::BorderStyle::None)
        );
        let b = ex("border-top", "dotted").unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!(
            get(&b, BorderTopColor),
            &Specified::Color(ColorSpec::CurrentColor)
        );
        let b = ex("border-width", "1px 2px").unwrap();
        assert_eq!(
            get(&b, BorderBottomWidth),
            &Specified::BorderWidth(BorderWidthSpec::Length(px(1)))
        );
        let b = ex("border-color", "red green blue").unwrap();
        assert_eq!(
            get(&b, BorderLeftColor),
            &Specified::Color(ColorSpec::Rgba(cw_scene::Color(0, 128, 0, 255)))
        );
        assert_eq!(ex("border", "1px 2px"), Err(ShorthandError::Invalid));
        assert_eq!(ex("border", "solid dashed"), Err(ShorthandError::Invalid));
        let o = ex("outline", "2px auto blue").unwrap();
        assert_eq!(
            get(&o, OutlineStyle),
            &Specified::BorderStyle(computed::BorderStyle::Solid)
        );
        let l = ex("border-inline-start", "1px solid").unwrap();
        assert_eq!(l[0].0, BorderLeftWidth);
        let r = expand(
            "border-inline-start",
            &tokenize("1px solid"),
            computed::Direction::Rtl,
        )
        .unwrap();
        assert_eq!(r[0].0, BorderRightWidth);
    }

    #[test]
    fn border_radius_with_slash() {
        let r = ex("border-radius", "1px 2px / 3px").unwrap();
        assert_eq!(
            get(&r, BorderTopLeftRadius),
            &Specified::LpPair(px(1), px(3))
        );
        assert_eq!(
            get(&r, BorderTopRightRadius),
            &Specified::LpPair(px(2), px(3))
        );
        assert_eq!(
            get(&r, BorderBottomLeftRadius),
            &Specified::LpPair(px(2), px(3))
        );
        let r = ex("border-radius", "50%").unwrap();
        assert_eq!(
            get(&r, BorderBottomRightRadius),
            &Specified::LpPair(
                LpSpec::Percent(Number::from_i64(50)),
                LpSpec::Percent(Number::from_i64(50))
            )
        );
        assert_eq!(ex("border-radius", "1px /"), Err(ShorthandError::Invalid));
        assert_eq!(ex("border-radius", "-1px"), Err(ShorthandError::Invalid));
    }

    #[test]
    fn inset_and_logical() {
        let i = ex("inset", "0").unwrap();
        assert_eq!(i.len(), 4);
        let i = ex("inset-inline", "1px 2px").unwrap();
        assert_eq!(i[0].0, Left);
        assert_eq!(i[1].0, Right);
        assert_eq!(
            resolve_longhand("inset-inline-end", computed::Direction::Rtl),
            Some(Left)
        );
        assert_eq!(
            resolve_longhand("margin-block-start", computed::Direction::Ltr),
            Some(MarginTop)
        );
        assert_eq!(
            resolve_longhand("border-inline-end-width", computed::Direction::Ltr),
            Some(BorderRightWidth)
        );
        assert_eq!(
            resolve_longhand("inline-size", computed::Direction::Ltr),
            Some(Width)
        );
        assert_eq!(
            resolve_longhand("-webkit-border-radius", computed::Direction::Ltr),
            None
        );
        assert!(is_shorthand("-webkit-border-radius"));
    }

    #[test]
    fn background_layers() {
        let b = ex("background", "url(a.png) no-repeat center / cover, linear-gradient(red, blue) padding-box content-box, #fff").unwrap();
        assert_eq!(
            get(&b, BackgroundColor),
            &Specified::Color(ColorSpec::Rgba(cw_scene::Color(255, 255, 255, 255)))
        );
        match get(&b, BackgroundImage) {
            Specified::Images(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], ImageSpec::Url("a.png".into()));
                assert_eq!(v[2], ImageSpec::None);
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(
            get(&b, BackgroundRepeat),
            &Specified::Repeats(vec![
                computed::BackgroundRepeat::NoRepeat,
                computed::BackgroundRepeat::Repeat,
                computed::BackgroundRepeat::Repeat
            ])
        );
        assert_eq!(
            get(&b, BackgroundSize),
            &Specified::BgSizes(vec![BgSizeSpec::Cover, BgSizeSpec::Auto, BgSizeSpec::Auto])
        );
        assert_eq!(
            get(&b, BackgroundOrigin),
            &Specified::BgBoxes(vec![
                computed::BackgroundBox::PaddingBox,
                computed::BackgroundBox::PaddingBox,
                computed::BackgroundBox::PaddingBox
            ])
        );
        assert_eq!(
            get(&b, BackgroundClip),
            &Specified::BgBoxes(vec![
                computed::BackgroundBox::BorderBox,
                computed::BackgroundBox::ContentBox,
                computed::BackgroundBox::BorderBox
            ])
        );
        let b = ex("background", "red").unwrap();
        assert_eq!(
            get(&b, BackgroundImage),
            &Specified::Images(vec![ImageSpec::None])
        );
        let b = ex("background", "none").unwrap();
        assert_eq!(
            get(&b, BackgroundColor),
            &Specified::Color(ColorSpec::Rgba(cw_scene::Color::TRANSPARENT))
        );
        let b = ex("background", "url(x) fixed border-box").unwrap();
        assert_eq!(get(&b, BackgroundAttachment), &Specified::Bools(vec![true]));
        assert_eq!(
            get(&b, BackgroundClip),
            &Specified::BgBoxes(vec![computed::BackgroundBox::BorderBox])
        );
        // Colour only in the last layer.
        assert_eq!(
            ex("background", "red, url(x)"),
            Err(ShorthandError::Invalid)
        );
        assert_eq!(ex("background", "red red"), Err(ShorthandError::Invalid));
        assert_eq!(
            ex("background", "url(x) / cover"),
            Err(ShorthandError::Invalid)
        );
        let bp = ex("background-position", "right 10px top, center").unwrap();
        match get(&bp, BackgroundPositionX) {
            Specified::LpList(v) => assert_eq!(v.len(), 2),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn font_shorthand() {
        let f = ex(
            "font",
            "italic small-caps bold 12px/1.5 \"Helvetica Neue\", Arial, sans-serif",
        )
        .unwrap();
        assert_eq!(
            get(&f, FontStyle),
            &Specified::FontStyle(computed::FontStyle::Italic)
        );
        assert_eq!(get(&f, FontVariant), &Specified::Bool(true));
        assert_eq!(
            get(&f, FontWeight),
            &Specified::FontWeight(FontWeightSpec::Absolute(700))
        );
        assert_eq!(
            get(&f, FontSize),
            &Specified::FontSize(FontSizeSpec::Lp(px(12)))
        );
        assert_eq!(
            get(&f, LineHeight),
            &Specified::LineHeight(LineHeightSpec::Number(Number {
                micro: 1_500_000,
                int: false
            }))
        );
        assert_eq!(
            get(&f, FontFamily),
            &Specified::FontFamily(vec![
                "Helvetica Neue".into(),
                "Arial".into(),
                "sans-serif".into()
            ])
        );
        let f = ex("font", "16px serif").unwrap();
        assert_eq!(
            get(&f, FontWeight),
            &Specified::FontWeight(FontWeightSpec::Absolute(400))
        );
        assert_eq!(
            get(&f, LineHeight),
            &Specified::LineHeight(LineHeightSpec::Normal)
        );
        let f = ex("font", "700 medium Georgia").unwrap();
        assert_eq!(
            get(&f, FontSize),
            &Specified::FontSize(FontSizeSpec::Absolute(3))
        );
        let f = ex("font", "menu").unwrap();
        assert_eq!(
            get(&f, FontFamily),
            &Specified::FontFamily(vec!["system-ui".into()])
        );
        assert_eq!(ex("font", "12px"), Err(ShorthandError::Invalid));
        assert_eq!(ex("font", "serif"), Err(ShorthandError::Invalid));
        assert_eq!(ex("font", "bold 12px"), Err(ShorthandError::Invalid));
        assert_eq!(ex("font", "12px/ serif"), Err(ShorthandError::Invalid));
    }

    #[test]
    fn flex_forms() {
        let one = Number::from_i64(1);
        let f = ex("flex", "1").unwrap();
        assert_eq!(get(&f, FlexGrow), &Specified::Number(one));
        assert_eq!(get(&f, FlexShrink), &Specified::Number(one));
        assert_eq!(
            get(&f, FlexBasis),
            &Specified::Sizing(SizingSpec::Lp(LpSpec::Percent(Number::ZERO)))
        );
        let f = ex("flex", "none").unwrap();
        assert_eq!(get(&f, FlexGrow), &Specified::Number(Number::ZERO));
        assert_eq!(get(&f, FlexBasis), &Specified::Sizing(SizingSpec::Auto));
        let f = ex("flex", "auto").unwrap();
        assert_eq!(get(&f, FlexGrow), &Specified::Number(one));
        let f = ex("flex", "2 3 10px").unwrap();
        assert_eq!(get(&f, FlexShrink), &Specified::Number(Number::from_i64(3)));
        assert_eq!(
            get(&f, FlexBasis),
            &Specified::Sizing(SizingSpec::Lp(px(10)))
        );
        let f = ex("flex", "10px 2").unwrap();
        assert_eq!(get(&f, FlexGrow), &Specified::Number(Number::from_i64(2)));
        assert_eq!(ex("flex", "-1"), Err(ShorthandError::Invalid));
        assert_eq!(ex("flex", "1 2 3 4"), Err(ShorthandError::Invalid));
        let ff = ex("flex-flow", "column wrap").unwrap();
        assert_eq!(
            get(&ff, FlexDirection),
            &Specified::FlexDirection(computed::FlexDirection::Column)
        );
        let ff = ex("flex-flow", "wrap-reverse").unwrap();
        assert_eq!(
            get(&ff, FlexDirection),
            &Specified::FlexDirection(computed::FlexDirection::Row)
        );
        let g = ex("gap", "10px 20px").unwrap();
        assert_eq!(get(&g, ColumnGap), &Specified::Lp(px(20)));
        let g = ex("gap", "normal").unwrap();
        assert_eq!(get(&g, RowGap), &Specified::Lp(LpSpec::ZERO));
    }

    #[test]
    fn grid_forms() {
        let g = ex("grid-template", "100px 1fr / repeat(2, 50px)").unwrap();
        match get(&g, GridTemplateRows) {
            Specified::TrackList(t) => assert_eq!(t.entries.len(), 2),
            o => panic!("{o:?}"),
        }
        let g = ex("grid-template", "[a] \"x y\" 1fr [b] \"z z\" / auto 1fr").unwrap();
        assert_eq!(
            get(&g, GridTemplateAreas),
            &Specified::GridAreas(vec![
                vec!["x".into(), "y".into()],
                vec!["z".into(), "z".into()]
            ])
        );
        match get(&g, GridTemplateRows) {
            Specified::TrackList(t) => assert_eq!(t.entries.len(), 4),
            o => panic!("{o:?}"),
        }
        let g = ex("grid", "auto-flow dense 40px / 1fr 1fr").unwrap();
        assert_eq!(
            get(&g, GridAutoFlow),
            &Specified::GridAutoFlow(computed::GridAutoFlow::RowDense)
        );
        assert_eq!(
            get(&g, GridAutoRows),
            &Specified::AutoTracks(vec![TrackSizeSpec::Breadth(TrackBreadthSpec::Lp(px(40)))])
        );
        let g = ex("grid", "1fr / auto-flow").unwrap();
        assert_eq!(
            get(&g, GridAutoFlow),
            &Specified::GridAutoFlow(computed::GridAutoFlow::Column)
        );
        let g = ex("grid", "none").unwrap();
        assert_eq!(
            get(&g, GridTemplateRows),
            &Specified::TrackList(TrackListSpec::default())
        );
        let a = ex("grid-area", "1 / 2 / 3 / 4").unwrap();
        assert_eq!(
            get(&a, GridColumnEnd),
            &Specified::GridLine(GridLine::Line(4, None))
        );
        let a = ex("grid-area", "header").unwrap();
        assert_eq!(
            get(&a, GridColumnEnd),
            &Specified::GridLine(GridLine::Name("header".into()))
        );
        let a = ex("grid-area", "2").unwrap();
        assert_eq!(
            get(&a, GridColumnStart),
            &Specified::GridLine(GridLine::Auto)
        );
        let r = ex("grid-row", "span 2 / 5").unwrap();
        assert_eq!(
            get(&r, GridRowStart),
            &Specified::GridLine(GridLine::Span(2, None))
        );
        let c = ex("grid-column", "1 / -1").unwrap();
        assert_eq!(
            get(&c, GridColumnEnd),
            &Specified::GridLine(GridLine::Line(-1, None))
        );
        assert_eq!(
            ex("grid-area", "1 / 2 / 3 / 4 / 5"),
            Err(ShorthandError::Invalid)
        );
        assert_eq!(ex("grid-template", "100px"), Err(ShorthandError::Invalid));
        let g = ex("grid-template", "none").unwrap();
        assert_eq!(get(&g, GridTemplateAreas), &Specified::GridAreas(vec![]));
    }

    #[test]
    fn place_list_outline_text_decoration_overflow() {
        let pl = ex("place-items", "center").unwrap();
        assert_eq!(
            get(&pl, JustifyItems),
            &Specified::AlignItems(computed::AlignItems::Center)
        );
        let pl = ex("place-content", "center space-between").unwrap();
        assert_eq!(
            get(&pl, JustifyContent),
            &Specified::JustifyContent(computed::JustifyContent::SpaceBetween)
        );
        let pl = ex("place-self", "end").unwrap();
        assert_eq!(
            get(&pl, JustifySelf),
            &Specified::AlignSelf(computed::AlignSelf::End)
        );
        let ls = ex("list-style", "none").unwrap();
        assert_eq!(
            get(&ls, ListStyleType),
            &Specified::ListStyleType(computed::ListStyleType::None)
        );
        let ls = ex("list-style", "inside square").unwrap();
        assert_eq!(
            get(&ls, ListStyleType),
            &Specified::ListStyleType(computed::ListStyleType::Square)
        );
        assert_eq!(
            get(&ls, ListStylePosition),
            &Specified::ListStylePosition(computed::ListStylePosition::Inside)
        );
        let ls = ex("list-style", "url(x.png) none").unwrap();
        assert_eq!(
            get(&ls, ListStyleType),
            &Specified::ListStyleType(computed::ListStyleType::None)
        );
        assert_eq!(
            ex("list-style", "none none none"),
            Err(ShorthandError::Invalid)
        );
        let td = ex("text-decoration", "underline dotted red").unwrap();
        assert_eq!(
            get(&td, TextDecorationLine),
            &Specified::TextDecorationLine {
                underline: true,
                overline: false,
                line_through: false
            }
        );
        assert_eq!(
            get(&td, TextDecorationStyle),
            &Specified::TextDecorationStyle(computed::TextDecorationStyle::Dotted)
        );
        let td = ex("text-decoration", "none").unwrap();
        assert_eq!(
            get(&td, TextDecorationColor),
            &Specified::Color(ColorSpec::CurrentColor)
        );
        let td = ex("text-decoration", "underline line-through").unwrap();
        assert_eq!(
            get(&td, TextDecorationLine),
            &Specified::TextDecorationLine {
                underline: true,
                overline: false,
                line_through: true
            }
        );
        assert_eq!(
            ex("text-decoration", "underline underline"),
            Err(ShorthandError::Invalid)
        );
        let ov = ex("overflow", "hidden auto").unwrap();
        assert_eq!(
            get(&ov, OverflowY),
            &Specified::Overflow(computed::Overflow::Auto)
        );
        let ov = ex("overflow", "scroll").unwrap();
        assert_eq!(
            get(&ov, OverflowX),
            &Specified::Overflow(computed::Overflow::Scroll)
        );
        assert_eq!(
            get(&ov, OverflowY),
            &Specified::Overflow(computed::Overflow::Scroll)
        );
    }

    #[test]
    fn transition_and_animation() {
        let t = ex("transition", "opacity 0.3s ease-in 100ms, transform 1s").unwrap();
        assert_eq!(
            get(&t, TransitionProperty),
            &Specified::Idents(vec!["opacity".into(), "transform".into()])
        );
        assert_eq!(
            get(&t, TransitionDuration),
            &Specified::Times(vec![300, 1000])
        );
        assert_eq!(
            get(&t, TransitionTimingFunction),
            &Specified::Timings(vec![
                computed::TimingFunction::EaseIn,
                computed::TimingFunction::Ease
            ])
        );
        assert_eq!(get(&t, TransitionDelay), &Specified::Times(vec![100, 0]));
        let t = ex("transition", "all 0.2s cubic-bezier(0.4, 0, 0.2, 1)").unwrap();
        assert_eq!(
            get(&t, TransitionTimingFunction),
            &Specified::Timings(vec![computed::TimingFunction::CubicBezier(
                400, 0, 200, 1000
            )])
        );
        let t = ex("transition", "none").unwrap();
        assert_eq!(
            get(&t, TransitionProperty),
            &Specified::Idents(vec!["none".into()])
        );
        assert_eq!(ex("transition", "1s 2s 3s"), Err(ShorthandError::Invalid));
        let a = ex("animation", "spin 2s linear infinite").unwrap();
        assert_eq!(
            get(&a, AnimationName),
            &Specified::Idents(vec!["spin".into()])
        );
        assert_eq!(
            get(&a, AnimationIterationCount),
            &Specified::IterationCounts(vec![None])
        );
        assert_eq!(
            get(&a, AnimationTimingFunction),
            &Specified::Timings(vec![computed::TimingFunction::Linear])
        );
        let a = ex("animation", "1s steps(4, start) alternate both paused fade").unwrap();
        assert_eq!(
            get(&a, AnimationName),
            &Specified::Idents(vec!["fade".into()])
        );
        assert_eq!(
            get(&a, AnimationDirection),
            &Specified::AnimationDirections(vec![computed::AnimationDirection::Alternate])
        );
        assert_eq!(
            get(&a, AnimationFillMode),
            &Specified::AnimationFillModes(vec![computed::AnimationFillMode::Both])
        );
        assert_eq!(get(&a, AnimationPlayState), &Specified::Bools(vec![false]));
        assert_eq!(
            get(&a, AnimationTimingFunction),
            &Specified::Timings(vec![computed::TimingFunction::Steps(4, true)])
        );
        let a = ex("animation", "none").unwrap();
        assert_eq!(
            get(&a, AnimationName),
            &Specified::Idents(vec!["none".into()])
        );
    }

    #[test]
    fn css_wide_and_var_on_shorthands() {
        let b = ex("border", "unset").unwrap();
        assert!(b
            .iter()
            .all(|(_, v)| *v == Specified::CssWide(CssWide::Unset)));
        let a = ex("all", "revert").unwrap();
        assert_eq!(a.len(), LonghandId::COUNT);
        assert_eq!(ex("all", "1px"), Err(ShorthandError::Invalid));
        let m = ex("margin", "var(--x) 2px").unwrap();
        assert!(matches!(&m[0].1, Specified::Pending { property, .. } if property == "margin"));
    }
}
