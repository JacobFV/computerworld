//! Value grammars for every longhand. Each function parses one property's value
//! (without the CSS-wide keywords, which `parse_longhand` handles) and returns the
//! `Specified` variant the property's applier expects.

use super::*;
use crate::css::token::Token;

fn keyword<T: Clone>(p: &mut Parser, table: &[(&str, T)]) -> Option<T> {
    p.try_parse(|p| {
        let s = p.expect_ident_lower()?;
        table.iter().find(|(k, _)| *k == s).map(|(_, v)| v.clone())
    })
}

pub fn display(p: &mut Parser) -> Option<Specified> {
    // Two-value syntax (`block flow`, `inline flow-root`) collapses to the legacy keyword.
    let table: &[(&str, Display)] = &[
        ("inline", Display::Inline),
        ("block", Display::Block),
        ("inline-block", Display::InlineBlock),
        ("list-item", Display::ListItem),
        ("flex", Display::Flex),
        ("inline-flex", Display::InlineFlex),
        ("-webkit-flex", Display::Flex),
        ("-webkit-inline-flex", Display::InlineFlex),
        ("-webkit-box", Display::Flex),
        ("-webkit-inline-box", Display::InlineFlex),
        ("grid", Display::Grid),
        ("inline-grid", Display::InlineGrid),
        ("table", Display::Table),
        ("inline-table", Display::InlineTable),
        ("table-row-group", Display::TableRowGroup),
        ("table-header-group", Display::TableHeaderGroup),
        ("table-footer-group", Display::TableFooterGroup),
        ("table-row", Display::TableRow),
        ("table-cell", Display::TableCell),
        ("table-column-group", Display::TableColumnGroup),
        ("table-column", Display::TableColumn),
        ("table-caption", Display::TableCaption),
        ("flow-root", Display::FlowRoot),
        ("contents", Display::Contents),
        ("none", Display::None),
    ];
    if let Some(d) = keyword(p, table) {
        if p.is_done() {
            return Some(Specified::Display(d));
        }
        // `block flow` etc.
        let second = p.expect_ident_lower()?;
        let inner = match second.as_str() {
            "flow" => d,
            "flow-root" => match d {
                Display::Block => Display::FlowRoot,
                Display::Inline => Display::InlineBlock,
                _ => return None,
            },
            "flex" => match d {
                Display::Block => Display::Flex,
                Display::Inline => Display::InlineFlex,
                _ => return None,
            },
            "grid" => match d {
                Display::Block => Display::Grid,
                Display::Inline => Display::InlineGrid,
                _ => return None,
            },
            "table" => match d {
                Display::Block => Display::Table,
                Display::Inline => Display::InlineTable,
                _ => return None,
            },
            "list-item" => Display::ListItem,
            _ => return None,
        };
        return Some(Specified::Display(inner));
    }
    None
}

pub fn position(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("static", Position::Static),
            ("relative", Position::Relative),
            ("absolute", Position::Absolute),
            ("fixed", Position::Fixed),
            ("sticky", Position::Sticky),
            ("-webkit-sticky", Position::Sticky),
        ],
    )
    .map(Specified::Position)
}

pub fn float(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("none", Float::None),
            ("left", Float::Left),
            ("right", Float::Right),
            ("inline-start", Float::Left),
            ("inline-end", Float::Right),
        ],
    )
    .map(Specified::Float)
}

pub fn clear(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("none", Clear::None),
            ("left", Clear::Left),
            ("right", Clear::Right),
            ("both", Clear::Both),
            ("inline-start", Clear::Left),
            ("inline-end", Clear::Right),
        ],
    )
    .map(Specified::Clear)
}

pub fn visibility(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("visible", Visibility::Visible),
            ("hidden", Visibility::Hidden),
            ("collapse", Visibility::Collapse),
        ],
    )
    .map(Specified::Visibility)
}

pub fn box_sizing(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("content-box", BoxSizing::ContentBox),
            ("border-box", BoxSizing::BorderBox),
        ],
    )
    .map(Specified::BoxSizing)
}

pub fn overflow_keyword(p: &mut Parser) -> Option<Overflow> {
    keyword(
        p,
        &[
            ("visible", Overflow::Visible),
            ("hidden", Overflow::Hidden),
            ("clip", Overflow::Clip),
            ("scroll", Overflow::Scroll),
            ("auto", Overflow::Auto),
            ("overlay", Overflow::Auto),
        ],
    )
}

pub fn overflow(p: &mut Parser) -> Option<Specified> {
    overflow_keyword(p).map(Specified::Overflow)
}

pub fn z_index(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("auto").is_some() {
        return Some(Specified::ZIndex(ZIndex::Auto));
    }
    parse_integer_spec(p).map(|i| Specified::ZIndex(ZIndex::Int(i)))
}

pub fn direction(p: &mut Parser) -> Option<Specified> {
    keyword(p, &[("ltr", Direction::Ltr), ("rtl", Direction::Rtl)]).map(Specified::Direction)
}

pub fn integer(p: &mut Parser) -> Option<Specified> {
    parse_integer_spec(p).map(Specified::Integer)
}

pub fn non_negative_number(p: &mut Parser) -> Option<Specified> {
    let n = parse_number_spec(p)?;
    if n.is_negative() {
        return None;
    }
    Some(Specified::Number(n))
}

pub fn sizing_spec(p: &mut Parser, allow_none: bool) -> Option<SizingSpec> {
    p.try_parse(|p| {
        if let Some(kw) = p.peek_ident_lower() {
            let v = match kw.as_str() {
                "auto" => Some(SizingSpec::Auto),
                "none" if allow_none => Some(SizingSpec::None),
                "min-content" | "-webkit-min-content" => Some(SizingSpec::MinContent),
                "max-content" | "-webkit-max-content" => Some(SizingSpec::MaxContent),
                "fit-content" | "-webkit-fit-content" => Some(SizingSpec::FitContent),
                "-webkit-fill-available" | "-moz-available" | "stretch" => Some(SizingSpec::Auto),
                _ => None,
            };
            if let Some(v) = v {
                p.next();
                return Some(v);
            }
        }
        if let Some(args) = p.expect_function_named("fit-content") {
            Parser::new(args).parse_entirely(|p| parse_lp(p, Allow::NON_NEGATIVE))?;
            return Some(SizingSpec::FitContent);
        }
        parse_lp(p, Allow::NON_NEGATIVE).map(SizingSpec::Lp)
    })
}

pub fn sizing(p: &mut Parser) -> Option<Specified> {
    sizing_spec(p, false).map(Specified::Sizing)
}

pub fn max_sizing(p: &mut Parser) -> Option<Specified> {
    sizing_spec(p, true).map(Specified::Sizing)
}

pub fn flex_basis(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("content").is_some() {
        return Some(Specified::Sizing(SizingSpec::Content));
    }
    sizing(p)
}

pub fn lpa_spec(p: &mut Parser) -> Option<LpaSpec> {
    if p.expect_ident_matching("auto").is_some() {
        return Some(LpaSpec::Auto);
    }
    parse_lp(p, Allow::ALL).map(LpaSpec::Lp)
}

pub fn lpa(p: &mut Parser) -> Option<Specified> {
    lpa_spec(p).map(Specified::Lpa)
}

pub fn lp(p: &mut Parser) -> Option<Specified> {
    parse_lp(p, Allow::ALL).map(Specified::Lp)
}

pub fn lp_non_negative(p: &mut Parser) -> Option<Specified> {
    parse_lp(p, Allow::NON_NEGATIVE).map(Specified::Lp)
}

pub fn length(p: &mut Parser) -> Option<Specified> {
    parse_length_spec(p, false).map(Specified::Lp)
}

pub fn gap(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("normal").is_some() {
        return Some(Specified::Lp(LpSpec::ZERO));
    }
    lp_non_negative(p)
}

pub fn border_width_spec(p: &mut Parser) -> Option<BorderWidthSpec> {
    if let Some(k) = keyword(
        p,
        &[
            ("thin", BorderWidthSpec::Thin),
            ("medium", BorderWidthSpec::Medium),
            ("thick", BorderWidthSpec::Thick),
        ],
    ) {
        return Some(k);
    }
    parse_length_spec(p, true).map(BorderWidthSpec::Length)
}

pub fn border_width(p: &mut Parser) -> Option<Specified> {
    border_width_spec(p).map(Specified::BorderWidth)
}

pub fn border_style_keyword(p: &mut Parser) -> Option<BorderStyle> {
    keyword(
        p,
        &[
            ("none", BorderStyle::None),
            ("hidden", BorderStyle::Hidden),
            ("solid", BorderStyle::Solid),
            ("dashed", BorderStyle::Dashed),
            ("dotted", BorderStyle::Dotted),
            ("double", BorderStyle::Double),
            ("groove", BorderStyle::Groove),
            ("ridge", BorderStyle::Ridge),
            ("inset", BorderStyle::Inset),
            ("outset", BorderStyle::Outset),
        ],
    )
}

pub fn border_style(p: &mut Parser) -> Option<Specified> {
    border_style_keyword(p).map(Specified::BorderStyle)
}

pub fn outline_style(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("auto").is_some() {
        return Some(Specified::BorderStyle(BorderStyle::Solid));
    }
    border_style(p)
}

pub fn color(p: &mut Parser) -> Option<Specified> {
    parse_color(p).map(Specified::Color)
}

pub fn outline_color(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("invert").is_some() {
        return Some(Specified::Color(ColorSpec::CurrentColor));
    }
    color(p)
}

pub fn radius(p: &mut Parser) -> Option<Specified> {
    let a = parse_lp(p, Allow::NON_NEGATIVE)?;
    let b = parse_lp(p, Allow::NON_NEGATIVE).unwrap_or_else(|| a.clone());
    Some(Specified::LpPair(a, b))
}

// --- Fonts -----------------------------------------------------------------------

/// One family name: a string or a run of identifiers joined by single spaces.
pub fn family_name(p: &mut Parser) -> Option<String> {
    if let Some(s) = p.expect_string() {
        return Some(s.to_owned());
    }
    let mut parts = Vec::new();
    while let Some(id) = p.try_parse(|p| {
        let s = p.expect_ident()?;
        match s.to_ascii_lowercase().as_str() {
            "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" => None,
            _ => Some(s.to_owned()),
        }
    }) {
        parts.push(id);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

pub fn font_family_list(p: &mut Parser) -> Option<Vec<String>> {
    p.comma_list(family_name)
}

pub fn font_family(p: &mut Parser) -> Option<Specified> {
    font_family_list(p).map(Specified::FontFamily)
}

pub fn font_size_spec(p: &mut Parser) -> Option<FontSizeSpec> {
    let table: &[(&str, FontSizeSpec)] = &[
        ("xx-small", FontSizeSpec::Absolute(0)),
        ("x-small", FontSizeSpec::Absolute(1)),
        ("small", FontSizeSpec::Absolute(2)),
        ("medium", FontSizeSpec::Absolute(3)),
        ("large", FontSizeSpec::Absolute(4)),
        ("x-large", FontSizeSpec::Absolute(5)),
        ("xx-large", FontSizeSpec::Absolute(6)),
        ("xxx-large", FontSizeSpec::Absolute(7)),
        ("-webkit-xxx-large", FontSizeSpec::Absolute(7)),
        ("larger", FontSizeSpec::Larger),
        ("smaller", FontSizeSpec::Smaller),
    ];
    if let Some(k) = p.try_parse(|p| {
        let s = p.expect_ident_lower()?;
        table.iter().find(|(k, _)| *k == s).map(|(_, v)| v.clone())
    }) {
        return Some(k);
    }
    parse_lp(p, Allow::NON_NEGATIVE).map(FontSizeSpec::Lp)
}

pub fn font_size(p: &mut Parser) -> Option<Specified> {
    font_size_spec(p).map(Specified::FontSize)
}

pub fn font_weight_spec(p: &mut Parser) -> Option<FontWeightSpec> {
    if let Some(k) = keyword(
        p,
        &[
            ("normal", FontWeightSpec::Absolute(400)),
            ("bold", FontWeightSpec::Absolute(700)),
            ("bolder", FontWeightSpec::Bolder),
            ("lighter", FontWeightSpec::Lighter),
        ],
    ) {
        return Some(k);
    }
    let n = parse_number_spec(p)?;
    if n.micro < 1_000_000 || n.micro > 1_000_000_000 {
        return None;
    }
    Some(FontWeightSpec::Absolute(n.round().clamp(1, 1000) as u16))
}

pub fn font_weight(p: &mut Parser) -> Option<Specified> {
    font_weight_spec(p).map(Specified::FontWeight)
}

pub fn font_style_keyword(p: &mut Parser) -> Option<FontStyle> {
    let k = keyword(
        p,
        &[
            ("normal", FontStyle::Normal),
            ("italic", FontStyle::Italic),
            ("oblique", FontStyle::Oblique),
        ],
    )?;
    if k == FontStyle::Oblique {
        // Optional angle.
        let _ = parse_angle(p, false);
    }
    Some(k)
}

pub fn font_style(p: &mut Parser) -> Option<Specified> {
    font_style_keyword(p).map(Specified::FontStyle)
}

pub fn font_variant(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[("normal", false), ("small-caps", true), ("none", false)],
    )
    .map(Specified::Bool)
}

pub fn line_height_spec(p: &mut Parser) -> Option<LineHeightSpec> {
    if p.expect_ident_matching("normal").is_some() {
        return Some(LineHeightSpec::Normal);
    }
    if let Some(n) = p.try_parse(|p| {
        let n = p.expect_number()?;
        if n.is_negative() {
            None
        } else {
            Some(n)
        }
    }) {
        return Some(LineHeightSpec::Number(n));
    }
    if let Some(l) = parse_lp(p, Allow::NON_NEGATIVE) {
        return Some(LineHeightSpec::Lp(l));
    }
    // A numeric calc() is a number.
    let n = parse_number_spec(p)?;
    if n.is_negative() {
        return None;
    }
    Some(LineHeightSpec::Number(n))
}

pub fn line_height(p: &mut Parser) -> Option<Specified> {
    line_height_spec(p).map(Specified::LineHeight)
}

// --- Text ------------------------------------------------------------------------

pub fn text_align(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("start", TextAlign::Start),
            ("end", TextAlign::End),
            ("left", TextAlign::Left),
            ("right", TextAlign::Right),
            ("center", TextAlign::Center),
            ("justify", TextAlign::Justify),
            ("-webkit-center", TextAlign::WebkitCenter),
            ("-webkit-left", TextAlign::Left),
            ("-webkit-right", TextAlign::Right),
            ("match-parent", TextAlign::Start),
            ("justify-all", TextAlign::Justify),
        ],
    )
    .map(Specified::TextAlign)
}

pub fn text_transform(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("none", TextTransform::None),
            ("uppercase", TextTransform::Uppercase),
            ("lowercase", TextTransform::Lowercase),
            ("capitalize", TextTransform::Capitalize),
            ("full-width", TextTransform::None),
        ],
    )
    .map(Specified::TextTransform)
}

pub fn text_decoration_line(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::TextDecorationLine {
            underline: false,
            overline: false,
            line_through: false,
        });
    }
    let (mut u, mut o, mut l) = (false, false, false);
    let mut any = false;
    while let Some(kw) = p.peek_ident_lower() {
        match kw.as_str() {
            "underline" if !u => u = true,
            "overline" if !o => o = true,
            "line-through" if !l => l = true,
            "blink" => {}
            _ => break,
        }
        p.next();
        any = true;
    }
    if !any {
        return None;
    }
    Some(Specified::TextDecorationLine {
        underline: u,
        overline: o,
        line_through: l,
    })
}

pub fn text_decoration_style(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("solid", TextDecorationStyle::Solid),
            ("double", TextDecorationStyle::Double),
            ("dotted", TextDecorationStyle::Dotted),
            ("dashed", TextDecorationStyle::Dashed),
            ("wavy", TextDecorationStyle::Wavy),
        ],
    )
    .map(Specified::TextDecorationStyle)
}

pub fn text_overflow(p: &mut Parser) -> Option<Specified> {
    let k = keyword(
        p,
        &[
            ("clip", TextOverflow::Clip),
            ("ellipsis", TextOverflow::Ellipsis),
        ],
    )?;
    // The two-value form applies to both ends; take the end value.
    let second = keyword(
        p,
        &[
            ("clip", TextOverflow::Clip),
            ("ellipsis", TextOverflow::Ellipsis),
        ],
    );
    Some(Specified::TextOverflow(second.unwrap_or(k)))
}

pub fn white_space(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("normal", WhiteSpace::Normal),
            ("nowrap", WhiteSpace::NoWrap),
            ("pre", WhiteSpace::Pre),
            ("pre-wrap", WhiteSpace::PreWrap),
            ("pre-line", WhiteSpace::PreLine),
            ("break-spaces", WhiteSpace::BreakSpaces),
            ("-moz-pre-wrap", WhiteSpace::PreWrap),
        ],
    )
    .map(Specified::WhiteSpace)
}

pub fn word_break(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("normal", WordBreak::Normal),
            ("break-all", WordBreak::BreakAll),
            ("keep-all", WordBreak::KeepAll),
            ("break-word", WordBreak::BreakWord),
        ],
    )
    .map(Specified::WordBreak)
}

pub fn overflow_wrap(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("normal", OverflowWrap::Normal),
            ("anywhere", OverflowWrap::Anywhere),
            ("break-word", OverflowWrap::BreakWord),
        ],
    )
    .map(Specified::OverflowWrap)
}

pub fn scrollbar_width(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("auto", ScrollbarWidth::Auto),
            ("thin", ScrollbarWidth::Thin),
            ("none", ScrollbarWidth::None),
        ],
    )
    .map(Specified::ScrollbarWidth)
}

pub fn length_or_normal(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("normal").is_some() {
        return Some(Specified::LengthOrNormal(None));
    }
    parse_length_spec(p, false).map(|l| Specified::LengthOrNormal(Some(l)))
}

pub fn vertical_align(p: &mut Parser) -> Option<Specified> {
    if let Some(k) = keyword(
        p,
        &[
            ("baseline", VerticalAlign::Baseline),
            ("sub", VerticalAlign::Sub),
            ("super", VerticalAlign::Super),
            ("text-top", VerticalAlign::TextTop),
            ("text-bottom", VerticalAlign::TextBottom),
            ("middle", VerticalAlign::Middle),
            ("top", VerticalAlign::Top),
            ("bottom", VerticalAlign::Bottom),
        ],
    ) {
        return Some(Specified::VerticalAlign(VerticalAlignSpec::Keyword(k)));
    }
    parse_lp(p, Allow::ALL).map(|l| Specified::VerticalAlign(VerticalAlignSpec::Lp(l)))
}

fn shadow(p: &mut Parser, allow_inset_spread: bool) -> Option<ShadowSpec> {
    p.try_parse(|p| {
        let mut color = None;
        let mut inset = false;
        let mut lengths: Option<Vec<LpSpec>> = None;
        loop {
            if allow_inset_spread && !inset && p.expect_ident_matching("inset").is_some() {
                inset = true;
                continue;
            }
            if color.is_none() {
                if let Some(c) = parse_color(p) {
                    color = Some(c);
                    continue;
                }
            }
            if lengths.is_none() {
                let mut v = Vec::new();
                while let Some(l) = parse_length_spec(p, false) {
                    v.push(l);
                    if v.len() == 4 {
                        break;
                    }
                }
                if !v.is_empty() {
                    lengths = Some(v);
                    continue;
                }
            }
            break;
        }
        let l = lengths?;
        let max = if allow_inset_spread { 4 } else { 3 };
        if l.len() < 2 || l.len() > max {
            return None;
        }
        if l.len() >= 3 && l[2].is_negative() {
            return None;
        }
        Some(ShadowSpec {
            x: l[0].clone(),
            y: l[1].clone(),
            blur: l.get(2).cloned().unwrap_or(LpSpec::ZERO),
            spread: l.get(3).cloned().unwrap_or(LpSpec::ZERO),
            color,
            inset,
        })
    })
}

pub fn text_shadow(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Shadows(Vec::new()));
    }
    p.comma_list(|p| shadow(p, false)).map(Specified::Shadows)
}

pub fn box_shadow(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Shadows(Vec::new()));
    }
    p.comma_list(|p| shadow(p, true)).map(Specified::Shadows)
}

pub fn tab_size(p: &mut Parser) -> Option<Specified> {
    let n = parse_number_spec(p)?;
    if n.is_negative() {
        return None;
    }
    Some(Specified::Number(n))
}

// --- Lists and tables ------------------------------------------------------------

pub fn list_style_type_keyword(p: &mut Parser) -> Option<ListStyleType> {
    if let Some(k) = keyword(
        p,
        &[
            ("disc", ListStyleType::Disc),
            ("circle", ListStyleType::Circle),
            ("square", ListStyleType::Square),
            ("decimal", ListStyleType::Decimal),
            ("decimal-leading-zero", ListStyleType::DecimalLeadingZero),
            ("lower-alpha", ListStyleType::LowerAlpha),
            ("lower-latin", ListStyleType::LowerAlpha),
            ("upper-alpha", ListStyleType::UpperAlpha),
            ("upper-latin", ListStyleType::UpperAlpha),
            ("lower-roman", ListStyleType::LowerRoman),
            ("upper-roman", ListStyleType::UpperRoman),
            ("none", ListStyleType::None),
        ],
    ) {
        return Some(k);
    }
    // Other counter styles render as decimal; a string marker is treated as disc.
    if p.expect_string().is_some() {
        return Some(ListStyleType::Disc);
    }
    let s = p.expect_ident_lower()?;
    match s.as_str() {
        "lower-greek" | "armenian" | "georgian" | "cjk-decimal" | "hebrew" | "hiragana"
        | "katakana" | "arabic-indic" | "persian" | "bengali" | "devanagari"
        | "disclosure-open" | "disclosure-closed" => Some(ListStyleType::Decimal),
        _ => None,
    }
}

pub fn list_style_type(p: &mut Parser) -> Option<Specified> {
    list_style_type_keyword(p).map(Specified::ListStyleType)
}

pub fn list_style_position_keyword(p: &mut Parser) -> Option<ListStylePosition> {
    keyword(
        p,
        &[
            ("outside", ListStylePosition::Outside),
            ("inside", ListStylePosition::Inside),
        ],
    )
}

pub fn list_style_position(p: &mut Parser) -> Option<Specified> {
    list_style_position_keyword(p).map(Specified::ListStylePosition)
}

pub fn list_style_image(p: &mut Parser) -> Option<Specified> {
    parse_image(p).map(|i| Specified::Images(vec![i]))
}

pub fn table_layout(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[("auto", TableLayout::Auto), ("fixed", TableLayout::Fixed)],
    )
    .map(Specified::TableLayout)
}

pub fn border_collapse(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("separate", BorderCollapse::Separate),
            ("collapse", BorderCollapse::Collapse),
        ],
    )
    .map(Specified::BorderCollapse)
}

pub fn border_spacing(p: &mut Parser) -> Option<Specified> {
    let a = parse_length_spec(p, true)?;
    let b = parse_length_spec(p, true).unwrap_or_else(|| a.clone());
    Some(Specified::LpPair(a, b))
}

pub fn caption_side(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[("top", CaptionSide::Top), ("bottom", CaptionSide::Bottom)],
    )
    .map(Specified::CaptionSide)
}

pub fn empty_cells(p: &mut Parser) -> Option<Specified> {
    keyword(p, &[("show", EmptyCells::Show), ("hide", EmptyCells::Hide)]).map(Specified::EmptyCells)
}

// --- Backgrounds -----------------------------------------------------------------

pub fn background_image(p: &mut Parser) -> Option<Specified> {
    p.comma_list(parse_image).map(Specified::Images)
}

pub fn repeat_style(p: &mut Parser) -> Option<BackgroundRepeat> {
    if let Some(k) = keyword(
        p,
        &[
            ("repeat-x", BackgroundRepeat::RepeatX),
            ("repeat-y", BackgroundRepeat::RepeatY),
        ],
    ) {
        return Some(k);
    }
    let table: &[(&str, BackgroundRepeat)] = &[
        ("repeat", BackgroundRepeat::Repeat),
        ("no-repeat", BackgroundRepeat::NoRepeat),
        ("space", BackgroundRepeat::Space),
        ("round", BackgroundRepeat::Round),
    ];
    let a = keyword(p, table)?;
    let b = keyword(p, table);
    Some(match (a, b) {
        (a, None) => a,
        (BackgroundRepeat::Repeat, Some(BackgroundRepeat::NoRepeat)) => BackgroundRepeat::RepeatX,
        (BackgroundRepeat::NoRepeat, Some(BackgroundRepeat::Repeat)) => BackgroundRepeat::RepeatY,
        (a, Some(b)) if a == b => a,
        // Mixed space/round pairs are not representable; take the horizontal one.
        (a, Some(_)) => a,
    })
}

pub fn background_repeat(p: &mut Parser) -> Option<Specified> {
    p.comma_list(repeat_style).map(Specified::Repeats)
}

pub fn bg_size(p: &mut Parser) -> Option<BgSizeSpec> {
    if let Some(k) = keyword(
        p,
        &[
            ("cover", BgSizeSpec::Cover),
            ("contain", BgSizeSpec::Contain),
        ],
    ) {
        return Some(k);
    }
    let a = p.try_parse(|p| {
        if p.expect_ident_matching("auto").is_some() {
            return Some(LpaSpec::Auto);
        }
        parse_lp(p, Allow::NON_NEGATIVE).map(LpaSpec::Lp)
    })?;
    let b = p
        .try_parse(|p| {
            if p.expect_ident_matching("auto").is_some() {
                return Some(LpaSpec::Auto);
            }
            parse_lp(p, Allow::NON_NEGATIVE).map(LpaSpec::Lp)
        })
        .unwrap_or(LpaSpec::Auto);
    if a == LpaSpec::Auto && b == LpaSpec::Auto {
        return Some(BgSizeSpec::Auto);
    }
    Some(BgSizeSpec::Explicit(a, b))
}

pub fn background_size(p: &mut Parser) -> Option<Specified> {
    p.comma_list(bg_size).map(Specified::BgSizes)
}

pub fn background_position_x(p: &mut Parser) -> Option<Specified> {
    p.comma_list(|p| {
        p.try_parse(|p| {
            let pct = |v: i64| LpSpec::Percent(Number::from_i64(v));
            if let Some(kw) = keyword(p, &[("left", 0i64), ("center", 50), ("right", 100)]) {
                if kw != 50 {
                    if let Some(off) = parse_lp(p, Allow::ALL) {
                        return Some(offset_from(kw, off));
                    }
                }
                return Some(pct(kw));
            }
            parse_lp(p, Allow::ALL)
        })
    })
    .map(Specified::LpList)
}

pub fn background_position_y(p: &mut Parser) -> Option<Specified> {
    p.comma_list(|p| {
        p.try_parse(|p| {
            let pct = |v: i64| LpSpec::Percent(Number::from_i64(v));
            if let Some(kw) = keyword(p, &[("top", 0i64), ("center", 50), ("bottom", 100)]) {
                if kw != 50 {
                    if let Some(off) = parse_lp(p, Allow::ALL) {
                        return Some(offset_from(kw, off));
                    }
                }
                return Some(pct(kw));
            }
            parse_lp(p, Allow::ALL)
        })
    })
    .map(Specified::LpList)
}

/// `right 10px` is `calc(100% - 10px)`; `left 10px` is `10px`.
fn offset_from(edge_pct: i64, off: LpSpec) -> LpSpec {
    if edge_pct == 0 {
        return off;
    }
    let node = match off {
        LpSpec::Length(l) => CalcNode::Length(l),
        LpSpec::Percent(n) => CalcNode::Percent(n),
        LpSpec::Calc(c) => *c,
    };
    LpSpec::Calc(Box::new(CalcNode::Sum(vec![
        CalcNode::Percent(Number::from_i64(100)),
        CalcNode::Neg(Box::new(node)),
    ])))
}

pub fn background_box_keyword(p: &mut Parser) -> Option<BackgroundBox> {
    keyword(
        p,
        &[
            ("border-box", BackgroundBox::BorderBox),
            ("padding-box", BackgroundBox::PaddingBox),
            ("content-box", BackgroundBox::ContentBox),
        ],
    )
}

pub fn background_box(p: &mut Parser) -> Option<Specified> {
    p.comma_list(background_box_keyword).map(Specified::BgBoxes)
}

pub fn background_clip(p: &mut Parser) -> Option<Specified> {
    p.comma_list(|p| {
        if p.expect_ident_matching("text").is_some() {
            return Some(BackgroundBox::BorderBox);
        }
        background_box_keyword(p)
    })
    .map(Specified::BgBoxes)
}

pub fn attachment_keyword(p: &mut Parser) -> Option<bool> {
    keyword(p, &[("scroll", false), ("fixed", true), ("local", false)])
}

pub fn background_attachment(p: &mut Parser) -> Option<Specified> {
    p.comma_list(attachment_keyword).map(Specified::Bools)
}

pub fn opacity(p: &mut Parser) -> Option<Specified> {
    let f = parse_number_or_percent_fraction(p)?;
    Some(Specified::Number(Number {
        micro: f.clamp(0, 1_000_000),
        int: false,
    }))
}

/// `backdrop-filter`: the blur radius of its `blur()`, `none` as zero. The other
/// filter functions (brightness, saturate, ...) parse and are not applied.
pub fn backdrop_filter(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Lp(LpSpec::ZERO));
    }
    let mut blur = None;
    let mut any = false;
    while let Some((name, args)) = p.expect_function() {
        any = true;
        if name.eq_ignore_ascii_case("blur") {
            let mut a = Parser::new(args);
            blur = if a.is_done() {
                Some(LpSpec::ZERO)
            } else {
                Some(parse_length_spec(&mut a, false)?)
            };
        }
    }
    (any && p.is_done()).then(|| Specified::Lp(blur.unwrap_or(LpSpec::ZERO)))
}

// --- Transforms ------------------------------------------------------------------

pub fn transform(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Transform(Vec::new()));
    }
    let mut ops = Vec::new();
    while let Some((name, args)) = p.expect_function() {
        let mut a = Parser::new(args);
        let lp_list = |a: &mut Parser| a.comma_list(|p| parse_lp(p, Allow::ALL));
        let num_list = |a: &mut Parser| a.comma_list(parse_number_spec);
        let one = Number::from_i64(1);
        let zero = LpSpec::ZERO;
        match name.to_ascii_lowercase().as_str() {
            "translate" => {
                let v = a.parse_entirely(lp_list)?;
                match v.len() {
                    1 => ops.push(TransformSpec::Translate(v[0].clone(), zero)),
                    2 => ops.push(TransformSpec::Translate(v[0].clone(), v[1].clone())),
                    _ => return None,
                }
            }
            "translatex" => ops.push(TransformSpec::Translate(
                a.parse_entirely(|p| parse_lp(p, Allow::ALL))?,
                zero,
            )),
            "translatey" => ops.push(TransformSpec::Translate(
                zero,
                a.parse_entirely(|p| parse_lp(p, Allow::ALL))?,
            )),
            "translatez" => {
                a.parse_entirely(|p| parse_length_spec(p, false))?;
            }
            "translate3d" => {
                let v = a.parse_entirely(lp_list)?;
                if v.len() != 3 {
                    return None;
                }
                ops.push(TransformSpec::Translate(v[0].clone(), v[1].clone()));
            }
            "scale" => {
                let v = a.parse_entirely(|a| a.comma_list(parse_number_or_percent_fraction))?;
                let n = |m: i64| Number {
                    micro: m,
                    int: false,
                };
                match v.len() {
                    1 => ops.push(TransformSpec::Scale(n(v[0]), n(v[0]))),
                    2 => ops.push(TransformSpec::Scale(n(v[0]), n(v[1]))),
                    _ => return None,
                }
            }
            "scalex" => ops.push(TransformSpec::Scale(
                a.parse_entirely(parse_number_spec)?,
                one,
            )),
            "scaley" => ops.push(TransformSpec::Scale(
                one,
                a.parse_entirely(parse_number_spec)?,
            )),
            "scalez" => {
                a.parse_entirely(parse_number_spec)?;
            }
            "scale3d" => {
                let v = a.parse_entirely(num_list)?;
                if v.len() != 3 {
                    return None;
                }
                ops.push(TransformSpec::Scale(v[0], v[1]));
            }
            "rotate" | "rotatez" => ops.push(TransformSpec::Rotate(
                a.parse_entirely(|p| parse_angle(p, true))?,
            )),
            "skewx" => ops.push(TransformSpec::SkewX(
                a.parse_entirely(|p| parse_angle(p, true))?,
            )),
            "skewy" => ops.push(TransformSpec::SkewY(
                a.parse_entirely(|p| parse_angle(p, true))?,
            )),
            "skew" => {
                let v = a.parse_entirely(|a| a.comma_list(|p| parse_angle(p, true)))?;
                match v.len() {
                    1 => ops.push(TransformSpec::SkewX(v[0])),
                    2 => {
                        ops.push(TransformSpec::SkewX(v[0]));
                        ops.push(TransformSpec::SkewY(v[1]));
                    }
                    _ => return None,
                }
            }
            "matrix" => {
                let v = a.parse_entirely(num_list)?;
                if v.len() != 6 {
                    return None;
                }
                // [a b c d e f]: a pure translate/scale matrix has b = c = 0.
                if !v[1].is_zero() || !v[2].is_zero() {
                    return None;
                }
                let px = |n: Number| {
                    LpSpec::Length(Length {
                        value: n,
                        unit: LengthUnit::Px,
                    })
                };
                ops.push(TransformSpec::Translate(px(v[4]), px(v[5])));
                if v[0] != one || v[3] != one {
                    ops.push(TransformSpec::Scale(v[0], v[3]));
                }
            }
            _ => return None,
        }
    }
    if ops.is_empty() && !p.is_done() {
        return None;
    }
    if ops.is_empty() {
        // Only no-op 3-D functions such as `translateZ(0)`.
        return Some(Specified::Transform(Vec::new()));
    }
    Some(Specified::Transform(ops))
}

pub fn transform_origin(p: &mut Parser) -> Option<Specified> {
    let (x, y) = parse_position(p)?;
    // Optional z offset.
    let _ = parse_length_spec(p, false);
    Some(Specified::LpPair(x, y))
}

// --- Flex and alignment ----------------------------------------------------------

pub fn flex_direction(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("row", FlexDirection::Row),
            ("row-reverse", FlexDirection::RowReverse),
            ("column", FlexDirection::Column),
            ("column-reverse", FlexDirection::ColumnReverse),
        ],
    )
    .map(Specified::FlexDirection)
}

pub fn flex_wrap(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("nowrap", FlexWrap::NoWrap),
            ("wrap", FlexWrap::Wrap),
            ("wrap-reverse", FlexWrap::WrapReverse),
        ],
    )
    .map(Specified::FlexWrap)
}

pub fn justify_content(p: &mut Parser) -> Option<Specified> {
    // `safe`/`unsafe` prefixes are accepted and ignored.
    let _ = keyword(p, &[("safe", ()), ("unsafe", ())]);
    keyword(
        p,
        &[
            ("flex-start", JustifyContent::FlexStart),
            ("flex-end", JustifyContent::FlexEnd),
            ("center", JustifyContent::Center),
            ("space-between", JustifyContent::SpaceBetween),
            ("space-around", JustifyContent::SpaceAround),
            ("space-evenly", JustifyContent::SpaceEvenly),
            ("start", JustifyContent::Start),
            ("end", JustifyContent::End),
            ("left", JustifyContent::Left),
            ("right", JustifyContent::Right),
            ("stretch", JustifyContent::Stretch),
            ("normal", JustifyContent::FlexStart),
        ],
    )
    .map(Specified::JustifyContent)
}

const ALIGN_ITEMS: &[(&str, AlignItems)] = &[
    ("stretch", AlignItems::Stretch),
    ("normal", AlignItems::Stretch),
    ("flex-start", AlignItems::FlexStart),
    ("flex-end", AlignItems::FlexEnd),
    ("center", AlignItems::Center),
    ("baseline", AlignItems::Baseline),
    ("start", AlignItems::Start),
    ("end", AlignItems::End),
    ("self-start", AlignItems::SelfStart),
    ("self-end", AlignItems::SelfEnd),
    ("left", AlignItems::Start),
    ("right", AlignItems::End),
];

pub fn align_items(p: &mut Parser) -> Option<Specified> {
    let _ = keyword(
        p,
        &[("safe", ()), ("unsafe", ()), ("first", ()), ("last", ())],
    );
    keyword(p, ALIGN_ITEMS).map(Specified::AlignItems)
}

pub fn justify_items(p: &mut Parser) -> Option<Specified> {
    let _ = keyword(p, &[("safe", ()), ("unsafe", ()), ("legacy", ())]);
    keyword(p, ALIGN_ITEMS).map(Specified::AlignItems)
}

pub fn align_self(p: &mut Parser) -> Option<Specified> {
    let _ = keyword(
        p,
        &[("safe", ()), ("unsafe", ()), ("first", ()), ("last", ())],
    );
    keyword(
        p,
        &[
            ("auto", AlignSelf::Auto),
            ("normal", AlignSelf::Auto),
            ("stretch", AlignSelf::Stretch),
            ("flex-start", AlignSelf::FlexStart),
            ("flex-end", AlignSelf::FlexEnd),
            ("center", AlignSelf::Center),
            ("baseline", AlignSelf::Baseline),
            ("start", AlignSelf::Start),
            ("end", AlignSelf::End),
            ("self-start", AlignSelf::Start),
            ("self-end", AlignSelf::End),
            ("left", AlignSelf::Start),
            ("right", AlignSelf::End),
        ],
    )
    .map(Specified::AlignSelf)
}

pub fn align_content(p: &mut Parser) -> Option<Specified> {
    let _ = keyword(p, &[("safe", ()), ("unsafe", ())]);
    keyword(
        p,
        &[
            ("normal", AlignContent::Normal),
            ("flex-start", AlignContent::FlexStart),
            ("flex-end", AlignContent::FlexEnd),
            ("center", AlignContent::Center),
            ("space-between", AlignContent::SpaceBetween),
            ("space-around", AlignContent::SpaceAround),
            ("space-evenly", AlignContent::SpaceEvenly),
            ("stretch", AlignContent::Stretch),
            ("start", AlignContent::Start),
            ("end", AlignContent::End),
            ("baseline", AlignContent::Start),
        ],
    )
    .map(Specified::AlignContent)
}

// --- Grid ------------------------------------------------------------------------

fn track_breadth(p: &mut Parser) -> Option<TrackBreadthSpec> {
    p.try_parse(|p| {
        if let Some(k) = keyword(
            p,
            &[
                ("auto", TrackBreadthSpec::Auto),
                ("min-content", TrackBreadthSpec::MinContent),
                ("max-content", TrackBreadthSpec::MaxContent),
            ],
        ) {
            return Some(k);
        }
        if let Some(n) = p.try_parse(|p| {
            let (n, unit) = p.expect_dimension()?;
            if unit.eq_ignore_ascii_case("fr") && !n.is_negative() {
                Some(n)
            } else {
                None
            }
        }) {
            return Some(TrackBreadthSpec::Flex(n));
        }
        parse_lp(p, Allow::NON_NEGATIVE).map(TrackBreadthSpec::Lp)
    })
}

pub fn track_size(p: &mut Parser) -> Option<TrackSizeSpec> {
    p.try_parse(|p| {
        if let Some(args) = p.expect_function_named("minmax") {
            let mut a = Parser::new(args);
            let lo = track_breadth(&mut a)?;
            a.expect_comma()?;
            let hi = track_breadth(&mut a)?;
            if !a.is_done() {
                return None;
            }
            if matches!(lo, TrackBreadthSpec::Flex(_)) {
                return None;
            }
            return Some(TrackSizeSpec::MinMax(lo, hi));
        }
        if let Some(args) = p.expect_function_named("fit-content") {
            let v = Parser::new(args).parse_entirely(|p| parse_lp(p, Allow::NON_NEGATIVE))?;
            return Some(TrackSizeSpec::FitContent(v));
        }
        track_breadth(p).map(TrackSizeSpec::Breadth)
    })
}

fn line_names(p: &mut Parser) -> Option<Vec<String>> {
    let contents = p.expect_square_block()?;
    let mut inner = Parser::new(contents);
    let mut names = Vec::new();
    while let Some(n) = parse_custom_ident(&mut inner) {
        names.push(n);
    }
    if !inner.is_done() {
        return None;
    }
    Some(names)
}

fn track_entries(p: &mut Parser, allow_repeat: bool) -> Option<Vec<TrackEntry>> {
    let mut entries = Vec::new();
    loop {
        if let Some(names) = p.try_parse(line_names) {
            entries.push(TrackEntry::LineNames(names));
            continue;
        }
        if allow_repeat {
            if let Some(args) = p.expect_function_named("repeat") {
                let mut a = Parser::new(args);
                let count = a.try_parse(|a| {
                    if a.expect_ident_matching("auto-fill").is_some() {
                        return Some(RepeatCount::AutoFill);
                    }
                    if a.expect_ident_matching("auto-fit").is_some() {
                        return Some(RepeatCount::AutoFit);
                    }
                    let n = a.expect_integer()?;
                    if n < 1 {
                        return None;
                    }
                    Some(RepeatCount::Fixed(n as u32))
                })?;
                a.expect_comma()?;
                let inner = track_entries(&mut a, false)?;
                if !a.is_done() || !inner.iter().any(|e| matches!(e, TrackEntry::Track(_))) {
                    return None;
                }
                entries.push(TrackEntry::Repeat(count, inner));
                continue;
            }
        }
        if let Some(t) = track_size(p) {
            entries.push(TrackEntry::Track(t));
            continue;
        }
        break;
    }
    if entries.is_empty() {
        return None;
    }
    Some(entries)
}

pub fn track_list_spec(p: &mut Parser) -> Option<TrackListSpec> {
    if p.expect_ident_matching("none").is_some() {
        return Some(TrackListSpec::default());
    }
    let entries = track_entries(p, true)?;
    if !entries
        .iter()
        .any(|e| matches!(e, TrackEntry::Track(_) | TrackEntry::Repeat(..)))
    {
        return None;
    }
    let auto_repeats = entries
        .iter()
        .filter(|e| {
            matches!(
                e,
                TrackEntry::Repeat(RepeatCount::AutoFill | RepeatCount::AutoFit, _)
            )
        })
        .count();
    if auto_repeats > 1 {
        return None;
    }
    Some(TrackListSpec { entries })
}

pub fn track_list(p: &mut Parser) -> Option<Specified> {
    // `subgrid` and `masonry` are excluded by the plan.
    track_list_spec(p).map(Specified::TrackList)
}

pub fn grid_template_areas(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::GridAreas(Vec::new()));
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    while let Some(s) = p.expect_string() {
        let row = parse_area_row(s)?;
        if let Some(first) = rows.first() {
            if first.len() != row.len() {
                return None;
            }
        }
        rows.push(row);
    }
    if rows.is_empty() || !p.is_done() {
        return None;
    }
    Some(Specified::GridAreas(rows))
}

/// Splits an area string into cell tokens; runs of `.` are one null cell.
fn parse_area_row(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    for c in s.chars() {
        if c.is_whitespace() {
            flush(&mut cur, &mut out);
        } else if c == '.' {
            if !cur.is_empty() && !cur.starts_with('.') {
                flush(&mut cur, &mut out);
            }
            cur.push('.');
        } else if c.is_alphanumeric() || c == '-' || c == '_' || c as u32 > 0x7F {
            if cur.starts_with('.') {
                flush(&mut cur, &mut out);
            }
            cur.push(c);
        } else {
            return None;
        }
    }
    flush(&mut cur, &mut out);
    if out.is_empty() {
        return None;
    }
    Some(
        out.into_iter()
            .map(|t| {
                if t.starts_with('.') {
                    ".".to_owned()
                } else {
                    t
                }
            })
            .collect(),
    )
}

pub fn auto_tracks(p: &mut Parser) -> Option<Specified> {
    let mut v = Vec::new();
    while let Some(t) = track_size(p) {
        v.push(t);
    }
    if v.is_empty() {
        return None;
    }
    Some(Specified::AutoTracks(v))
}

pub fn grid_auto_flow(p: &mut Parser) -> Option<Specified> {
    let (mut row, mut column, mut dense) = (false, false, false);
    for _ in 0..2 {
        match p.peek_ident_lower().as_deref() {
            Some("row") if !row && !column => row = true,
            Some("column") if !row && !column => column = true,
            Some("dense") if !dense => dense = true,
            _ => break,
        }
        p.next();
    }
    if !row && !column && !dense {
        return None;
    }
    Some(Specified::GridAutoFlow(match (column, dense) {
        (false, false) => GridAutoFlow::Row,
        (false, true) => GridAutoFlow::RowDense,
        (true, false) => GridAutoFlow::Column,
        (true, true) => GridAutoFlow::ColumnDense,
    }))
}

pub fn grid_line_spec(p: &mut Parser) -> Option<GridLine> {
    p.try_parse(|p| {
        if p.expect_ident_matching("auto").is_some() {
            return Some(GridLine::Auto);
        }
        let mut span = false;
        let mut number: Option<i32> = None;
        let mut name: Option<String> = None;
        for _ in 0..3 {
            if !span && p.expect_ident_matching("span").is_some() {
                span = true;
                continue;
            }
            if number.is_none() {
                if let Some(n) = p.expect_integer() {
                    if n == 0 {
                        return None;
                    }
                    number = Some(n);
                    continue;
                }
            }
            if name.is_none() {
                if let Some(n) = parse_custom_ident(p) {
                    if n.eq_ignore_ascii_case("span") || n.eq_ignore_ascii_case("auto") {
                        return None;
                    }
                    name = Some(n);
                    continue;
                }
            }
            break;
        }
        Some(match (span, number, name) {
            (false, None, None) => return None,
            (false, Some(n), name) => GridLine::Line(n, name),
            (false, None, Some(name)) => GridLine::Name(name),
            (true, None, None) => GridLine::Span(1, None),
            (true, Some(n), name) => {
                if n < 0 {
                    return None;
                }
                GridLine::Span(n as u32, name)
            }
            (true, None, Some(name)) => GridLine::Span(1, Some(name)),
        })
    })
}

pub fn grid_line(p: &mut Parser) -> Option<Specified> {
    grid_line_spec(p).map(Specified::GridLine)
}

// --- Interaction and misc --------------------------------------------------------

pub fn cursor(p: &mut Parser) -> Option<Specified> {
    // `url(...) [x y]?,` prefixes are skipped; the keyword at the end applies.
    while p.expect_url().is_some() {
        let _ = parse_number_spec(p);
        let _ = parse_number_spec(p);
        p.expect_comma()?;
    }
    keyword(
        p,
        &[
            ("auto", Cursor::Auto),
            ("default", Cursor::Default),
            ("pointer", Cursor::Pointer),
            ("hand", Cursor::Pointer),
            ("text", Cursor::Text),
            ("vertical-text", Cursor::Text),
            ("move", Cursor::Move),
            ("not-allowed", Cursor::NotAllowed),
            ("no-drop", Cursor::NotAllowed),
            ("grab", Cursor::Grab),
            ("-webkit-grab", Cursor::Grab),
            ("grabbing", Cursor::Grabbing),
            ("-webkit-grabbing", Cursor::Grabbing),
            ("crosshair", Cursor::Crosshair),
            ("wait", Cursor::Wait),
            ("progress", Cursor::Progress),
            ("help", Cursor::Help),
            ("col-resize", Cursor::ColResize),
            ("row-resize", Cursor::RowResize),
            ("ns-resize", Cursor::NsResize),
            ("n-resize", Cursor::NsResize),
            ("s-resize", Cursor::NsResize),
            ("ew-resize", Cursor::EwResize),
            ("e-resize", Cursor::EwResize),
            ("w-resize", Cursor::EwResize),
            ("nesw-resize", Cursor::NeswResize),
            ("ne-resize", Cursor::NeswResize),
            ("sw-resize", Cursor::NeswResize),
            ("nwse-resize", Cursor::NwseResize),
            ("nw-resize", Cursor::NwseResize),
            ("se-resize", Cursor::NwseResize),
            ("none", Cursor::None),
            ("context-menu", Cursor::Default),
            ("cell", Cursor::Crosshair),
            ("alias", Cursor::Pointer),
            ("copy", Cursor::Pointer),
            ("all-scroll", Cursor::Move),
            ("zoom-in", Cursor::Default),
            ("zoom-out", Cursor::Default),
        ],
    )
    .map(Specified::Cursor)
}

pub fn pointer_events(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("auto", PointerEvents::Auto),
            ("none", PointerEvents::None),
            ("visiblepainted", PointerEvents::Auto),
            ("visiblefill", PointerEvents::Auto),
            ("visiblestroke", PointerEvents::Auto),
            ("visible", PointerEvents::Auto),
            ("painted", PointerEvents::Auto),
            ("fill", PointerEvents::Auto),
            ("stroke", PointerEvents::Auto),
            ("all", PointerEvents::Auto),
        ],
    )
    .map(Specified::PointerEvents)
}

pub fn user_select(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("auto", UserSelect::Auto),
            ("none", UserSelect::None),
            ("text", UserSelect::Text),
            ("all", UserSelect::All),
            ("contain", UserSelect::Text),
        ],
    )
    .map(Specified::UserSelect)
}

pub fn appearance(p: &mut Parser) -> Option<Specified> {
    let s = p.expect_ident_lower()?;
    Some(Specified::Appearance(match s.as_str() {
        "none" => Appearance::None,
        "auto" | "textfield" | "menulist" | "menulist-button" | "button" | "checkbox" | "radio"
        | "searchfield" | "textarea" | "listbox" | "meter" | "progress-bar"
        | "slider-horizontal" | "square-button" | "push-button" => Appearance::Auto,
        _ => return None,
    }))
}

pub fn object_fit(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("fill", ObjectFit::Fill),
            ("contain", ObjectFit::Contain),
            ("cover", ObjectFit::Cover),
            ("none", ObjectFit::None),
            ("scale-down", ObjectFit::ScaleDown),
        ],
    )
    .map(Specified::ObjectFit)
}

/// `-webkit-line-clamp: none | <integer [1,∞]>`.
pub fn line_clamp(p: &mut Parser) -> Option<Specified> {
    if p.try_parse(|p| p.expect_ident_matching("none")).is_some() {
        return Some(Specified::LineClamp(None));
    }
    let n = parse_integer_spec(p)?;
    (n >= 1).then_some(Specified::LineClamp(Some(n as u32)))
}

/// `-webkit-box-orient`: only whether the legacy box is vertical matters here.
pub fn box_orient(p: &mut Parser) -> Option<Specified> {
    keyword(
        p,
        &[
            ("horizontal", false),
            ("inline-axis", false),
            ("vertical", true),
            ("block-axis", true),
        ],
    )
    .map(Specified::BoxOrientVertical)
}

/// `aspect-ratio: auto || <ratio>`, with `<ratio> = <number [0,∞]> [ / <number [0,∞]> ]?`.
pub fn aspect_ratio(p: &mut Parser) -> Option<Specified> {
    let mut auto = false;
    let mut ratio: Option<(i64, i64)> = None;
    let mut seen_ratio = false;
    for _ in 0..2 {
        if !auto && p.try_parse(|p| p.expect_ident_matching("auto")).is_some() {
            auto = true;
        } else if !seen_ratio {
            let Some(w) = parse_number_spec(p) else { break };
            let h = match p.try_parse(|p| p.expect_delim('/')) {
                Some(()) => parse_number_spec(p)?.micro,
                None => 1_000_000,
            };
            if w.micro < 0 || h < 0 {
                return None;
            }
            seen_ratio = true;
            // A degenerate ratio behaves as `auto`.
            ratio = (w.micro > 0 && h > 0).then_some((w.micro, h));
        }
    }
    (auto || seen_ratio).then_some(Specified::AspectRatio(AspectRatio { auto, ratio }))
}

pub fn content(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("normal").is_some() {
        return Some(Specified::Content(ContentSpec::Normal));
    }
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Content(ContentSpec::None));
    }
    let mut items = Vec::new();
    loop {
        if let Some(s) = p.expect_string() {
            items.push(ContentItem::Text(s.to_owned()));
            continue;
        }
        if let Some(u) = p.expect_url() {
            items.push(ContentItem::Url(u));
            continue;
        }
        if let Some(kw) = p.try_parse(|p| {
            let s = p.expect_ident_lower()?;
            match s.as_str() {
                "open-quote" => Some(Some(ContentItem::OpenQuote)),
                "close-quote" => Some(Some(ContentItem::CloseQuote)),
                "no-open-quote" | "no-close-quote" => Some(None),
                _ => None,
            }
        }) {
            if let Some(item) = kw {
                items.push(item);
            }
            continue;
        }
        if let Some((name, args)) = p.expect_function() {
            let mut a = Parser::new(args);
            match name.to_ascii_lowercase().as_str() {
                "attr" => {
                    let n = a.expect_ident()?.to_owned();
                    // Optional `<type>` and fallback are accepted and ignored.
                    items.push(ContentItem::Attr(n));
                }
                "counter" => {
                    let n = parse_custom_ident(&mut a)?;
                    let style = if a.expect_comma().is_some() {
                        list_style_type_keyword(&mut a)?
                    } else {
                        ListStyleType::Decimal
                    };
                    if !a.is_done() {
                        return None;
                    }
                    items.push(ContentItem::Counter(n, style));
                }
                "counters" => {
                    let n = parse_custom_ident(&mut a)?;
                    a.expect_comma()?;
                    a.expect_string()?;
                    let style = if a.expect_comma().is_some() {
                        list_style_type_keyword(&mut a)?
                    } else {
                        ListStyleType::Decimal
                    };
                    if !a.is_done() {
                        return None;
                    }
                    items.push(ContentItem::Counter(n, style));
                }
                _ => return None,
            }
            continue;
        }
        break;
    }
    if items.is_empty() && !p.is_done() {
        return None;
    }
    if items.is_empty() {
        return None;
    }
    Some(Specified::Content(ContentSpec::Items(items)))
}

pub fn quotes(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Quotes(Some(Vec::new())));
    }
    if p.expect_ident_matching("auto").is_some() {
        return Some(Specified::Quotes(None));
    }
    let mut pairs = Vec::new();
    while let Some(a) = p.expect_string() {
        let b = p.expect_string()?;
        pairs.push((a.to_owned(), b.to_owned()));
    }
    if pairs.is_empty() {
        return None;
    }
    Some(Specified::Quotes(Some(pairs)))
}

fn counters(p: &mut Parser, default: i32) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() {
        return Some(Specified::Counters(Vec::new()));
    }
    let mut out = Vec::new();
    while let Some(name) = parse_custom_ident(p) {
        let n = p.expect_integer().unwrap_or(default);
        out.push((name, n));
    }
    if out.is_empty() {
        return None;
    }
    Some(Specified::Counters(out))
}

pub fn counter_reset(p: &mut Parser) -> Option<Specified> {
    counters(p, 0)
}

pub fn counter_increment(p: &mut Parser) -> Option<Specified> {
    counters(p, 1)
}

// --- Transitions and animations --------------------------------------------------

pub fn transition_property(p: &mut Parser) -> Option<Specified> {
    if p.expect_ident_matching("none").is_some() && p.is_done() {
        return Some(Specified::Idents(vec!["none".into()]));
    }
    p.comma_list(|p| p.expect_ident().map(|s| s.to_ascii_lowercase()))
        .map(Specified::Idents)
}

pub fn animation_name(p: &mut Parser) -> Option<Specified> {
    p.comma_list(|p| {
        if let Some(s) = p.expect_string() {
            return Some(s.to_owned());
        }
        if p.expect_ident_matching("none").is_some() {
            return Some("none".into());
        }
        parse_custom_ident(p)
    })
    .map(Specified::Idents)
}

pub fn times(p: &mut Parser) -> Option<Specified> {
    p.comma_list(parse_time).map(Specified::Times)
}

pub fn timing_function(p: &mut Parser) -> Option<TimingFunction> {
    if let Some(k) = keyword(
        p,
        &[
            ("ease", TimingFunction::Ease),
            ("linear", TimingFunction::Linear),
            ("ease-in", TimingFunction::EaseIn),
            ("ease-out", TimingFunction::EaseOut),
            ("ease-in-out", TimingFunction::EaseInOut),
            ("step-start", TimingFunction::StepStart),
            ("step-end", TimingFunction::StepEnd),
        ],
    ) {
        return Some(k);
    }
    let (name, args) = p.expect_function()?;
    let mut a = Parser::new(args);
    match name.to_ascii_lowercase().as_str() {
        "cubic-bezier" => {
            let v = a.parse_entirely(|a| a.comma_list(parse_number_spec))?;
            if v.len() != 4 {
                return None;
            }
            let x1 = micro_to_milli(v[0].micro);
            let x2 = micro_to_milli(v[2].micro);
            if !(0..=1000).contains(&x1) || !(0..=1000).contains(&x2) {
                return None;
            }
            Some(TimingFunction::CubicBezier(
                x1,
                micro_to_milli(v[1].micro),
                x2,
                micro_to_milli(v[3].micro),
            ))
        }
        "steps" => {
            let n = a.expect_integer()?;
            if n < 1 {
                return None;
            }
            let start = if a.expect_comma().is_some() {
                let s = a.expect_ident_lower()?;
                match s.as_str() {
                    "start" | "jump-start" | "jump-both" => true,
                    "end" | "jump-end" | "jump-none" => false,
                    _ => return None,
                }
            } else {
                false
            };
            if !a.is_done() {
                return None;
            }
            Some(TimingFunction::Steps(n as u32, start))
        }
        _ => None,
    }
}

pub fn timing_functions(p: &mut Parser) -> Option<Specified> {
    p.comma_list(timing_function).map(Specified::Timings)
}

pub fn iteration_count(p: &mut Parser) -> Option<Option<i32>> {
    if p.expect_ident_matching("infinite").is_some() {
        return Some(None);
    }
    let n = parse_number_spec(p)?;
    if n.is_negative() {
        return None;
    }
    Some(Some(micro_to_milli(n.micro)))
}

pub fn iteration_counts(p: &mut Parser) -> Option<Specified> {
    p.comma_list(iteration_count)
        .map(Specified::IterationCounts)
}

pub fn animation_direction_keyword(p: &mut Parser) -> Option<AnimationDirection> {
    keyword(
        p,
        &[
            ("normal", AnimationDirection::Normal),
            ("reverse", AnimationDirection::Reverse),
            ("alternate", AnimationDirection::Alternate),
            ("alternate-reverse", AnimationDirection::AlternateReverse),
        ],
    )
}

pub fn animation_directions(p: &mut Parser) -> Option<Specified> {
    p.comma_list(animation_direction_keyword)
        .map(Specified::AnimationDirections)
}

pub fn animation_fill_mode_keyword(p: &mut Parser) -> Option<AnimationFillMode> {
    keyword(
        p,
        &[
            ("none", AnimationFillMode::None),
            ("forwards", AnimationFillMode::Forwards),
            ("backwards", AnimationFillMode::Backwards),
            ("both", AnimationFillMode::Both),
        ],
    )
}

pub fn animation_fill_modes(p: &mut Parser) -> Option<Specified> {
    p.comma_list(animation_fill_mode_keyword)
        .map(Specified::AnimationFillModes)
}

pub fn animation_play_state_keyword(p: &mut Parser) -> Option<bool> {
    keyword(p, &[("running", true), ("paused", false)])
}

pub fn animation_play_states(p: &mut Parser) -> Option<Specified> {
    p.comma_list(animation_play_state_keyword)
        .map(Specified::Bools)
}

/// Whether the next token is a plain identifier equal to `kw` (for shorthands).
pub fn peek_is(p: &mut Parser, kw: &str) -> bool {
    matches!(p.peek(), Some(ComponentValue::Token(Token::Ident(s))) if s.eq_ignore_ascii_case(kw))
}
