//! Media Queries Level 4 (the features the engine can answer) and `@supports`
//! conditions. Evaluation is three-valued as the specification requires: an unknown
//! feature makes its query false, but `not (unknown)` is not true.

use super::selector::SelectorList;
use super::token::{ComponentValue, Number, Token};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MediaType {
    #[default]
    Screen,
    Print,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HoverCapability {
    None,
    #[default]
    Hover,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerCapability {
    None,
    Coarse,
    #[default]
    Fine,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DisplayMode {
    #[default]
    Browser,
    MinimalUi,
    Standalone,
    Fullscreen,
}

/// The environment a media query is evaluated against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Media {
    pub width_px: i32,
    pub height_px: i32,
    /// Device pixel ratio; `1dppx` is `Number::from_i64(1)`.
    pub dppx: Number,
    pub color_scheme: ColorScheme,
    pub hover: HoverCapability,
    pub pointer: PointerCapability,
    pub reduced_motion: bool,
    pub media_type: MediaType,
    pub display_mode: DisplayMode,
    /// Which font families count as installed on the device.
    pub fonts: FontEnvironment,
}

/// The fonts installed on the device a page renders on, which decides where a
/// `font-family` list lands when it names a face the device does not have.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontEnvironment {
    /// Every bundled face is installed (the world's browser).
    #[default]
    Bundled,
    /// A stock Linux desktop, the machine the Chromium parity dumps were made on:
    /// only the Liberation and DejaVu families and fontconfig's metric aliases for
    /// them (Arial, Helvetica, Times New Roman, Courier New); every other family
    /// falls through to the list's generic.
    LinuxBaseline,
}

impl Default for Media {
    fn default() -> Self {
        Media { width_px: 1280, height_px: 800, dppx: Number::from_i64(1), color_scheme: ColorScheme::Light, hover: HoverCapability::Hover, pointer: PointerCapability::Fine, reduced_motion: false, media_type: MediaType::Screen, display_mode: DisplayMode::Browser, fonts: FontEnvironment::Bundled }
    }
}

impl Media {
    pub fn with_size(width_px: i32, height_px: i32) -> Media {
        Media { width_px, height_px, ..Media::default() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareOp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl CompareOp {
    fn holds(self, ord: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::*;
        match self {
            CompareOp::Lt => ord == Less,
            CompareOp::Le => ord != Greater,
            CompareOp::Eq => ord == Equal,
            CompareOp::Ge => ord != Less,
            CompareOp::Gt => ord == Greater,
        }
    }
    fn flip(self) -> CompareOp {
        match self {
            CompareOp::Lt => CompareOp::Gt,
            CompareOp::Le => CompareOp::Ge,
            CompareOp::Eq => CompareOp::Eq,
            CompareOp::Ge => CompareOp::Le,
            CompareOp::Gt => CompareOp::Lt,
        }
    }
    fn text(self) -> &'static str {
        match self {
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Eq => "=",
            CompareOp::Ge => ">=",
            CompareOp::Gt => ">",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FeatureValue {
    /// A length in micro-pixels (px * 1_000_000).
    Length(i64),
    /// `<number>` or `<integer>`.
    Number(Number),
    /// `<ratio>`: numerator / denominator in micro units.
    Ratio(i64, i64),
    /// A resolution in micro-dppx.
    Resolution(i64),
    Ident(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MediaFeature {
    /// `(hover)`, `(width)`: true when the feature's value is not zero/none.
    Boolean(String),
    /// `(width: 600px)`, `(min-width: 600px)` (as `Ge`), `(width >= 600px)`.
    Compare { name: String, op: CompareOp, value: FeatureValue },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MediaCondition {
    Not(Box<MediaCondition>),
    And(Vec<MediaCondition>),
    Or(Vec<MediaCondition>),
    Feature(MediaFeature),
    /// `<general-enclosed>`: syntactically fine, semantically unknown.
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MediaQuery {
    pub negated: bool,
    pub only: bool,
    /// `None` is `all`. Unknown media types are stored by name and never match.
    pub media_type: Option<String>,
    pub condition: Option<MediaCondition>,
    /// A query that failed to parse: serializes and evaluates as `not all`.
    pub invalid: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MediaQueryList {
    pub queries: Vec<MediaQuery>,
}

impl MediaQueryList {
    pub fn parse(src: &str) -> MediaQueryList {
        MediaQueryList::from_values(&super::parser::parse_component_value_list(src))
    }
    /// Parses a list from a rule prelude; an empty list matches everything.
    pub fn from_values(values: &[ComponentValue]) -> MediaQueryList {
        let trimmed = trim_ws(values);
        if trimmed.is_empty() {
            return MediaQueryList { queries: Vec::new() };
        }
        let queries = split_commas(trimmed).into_iter().map(|q| MediaQuery::from_values(q).unwrap_or(MediaQuery { negated: true, only: false, media_type: None, condition: None, invalid: true })).collect();
        MediaQueryList { queries }
    }
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }
    pub fn evaluate(&self, media: &Media) -> bool {
        self.queries.is_empty() || self.queries.iter().any(|q| q.evaluate(media))
    }
}

impl MediaQuery {
    pub fn evaluate(&self, media: &Media) -> bool {
        if self.invalid {
            return false;
        }
        let type_ok = match self.media_type.as_deref() {
            None => true,
            Some("all") => true,
            Some("screen") => media.media_type == MediaType::Screen,
            Some("print") => media.media_type == MediaType::Print,
            Some(_) => false,
        };
        let cond = match &self.condition {
            None => Some(true),
            Some(c) => c.evaluate(media),
        };
        let result = if type_ok { cond } else { Some(false) };
        match (self.negated, result) {
            (false, Some(v)) => v,
            (true, Some(v)) => !v,
            (_, None) => false,
        }
    }

    fn from_values(values: &[ComponentValue]) -> Option<MediaQuery> {
        let mut p = P { values: trim_ws(values), pos: 0 };
        if p.at_end() {
            return None;
        }
        // <media-condition> alone (starts with `(` or `not (`).
        let starts_paren = |p: &P, n: usize| matches!(p.peek_at(n), Some(ComponentValue::Block { open: Token::OpenParen, .. }) | Some(ComponentValue::Function { .. }));
        if starts_paren(&p, 0) || (p.ident_is(0, "not") && p.peek_at(1).is_some_and(|v| v.is_whitespace()) && starts_paren(&p, 2)) {
            let cond = parse_condition(&mut p, true)?;
            p.skip_ws();
            if !p.at_end() {
                return None;
            }
            return Some(MediaQuery { negated: false, only: false, media_type: None, condition: Some(cond), invalid: false });
        }
        let mut negated = false;
        let mut only = false;
        if p.ident_is(0, "not") {
            negated = true;
            p.pos += 1;
        } else if p.ident_is(0, "only") {
            only = true;
            p.pos += 1;
        }
        p.skip_ws();
        let ty = match p.next() {
            Some(ComponentValue::Token(Token::Ident(t))) => t.to_ascii_lowercase(),
            _ => return None,
        };
        if matches!(ty.as_str(), "not" | "only" | "and" | "or" | "layer") {
            return None;
        }
        let media_type = if ty == "all" { None } else { Some(ty) };
        p.skip_ws();
        let condition = if p.at_end() {
            None
        } else {
            if !p.ident_is(0, "and") {
                return None;
            }
            p.pos += 1;
            p.skip_ws();
            let c = parse_condition(&mut p, false)?;
            p.skip_ws();
            if !p.at_end() {
                return None;
            }
            Some(c)
        };
        Some(MediaQuery { negated, only, media_type, condition, invalid: false })
    }
}

struct P<'a> {
    values: &'a [ComponentValue],
    pos: usize,
}

impl<'a> P<'a> {
    fn peek_at(&self, n: usize) -> Option<&'a ComponentValue> {
        self.values.get(self.pos + n)
    }
    fn next(&mut self) -> Option<&'a ComponentValue> {
        let v = self.values.get(self.pos);
        if v.is_some() {
            self.pos += 1;
        }
        v
    }
    fn at_end(&self) -> bool {
        self.pos >= self.values.len()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek_at(0), Some(v) if v.is_whitespace()) {
            self.pos += 1;
        }
    }
    fn ident_is(&self, n: usize, name: &str) -> bool {
        matches!(self.peek_at(n), Some(ComponentValue::Token(Token::Ident(i))) if i.eq_ignore_ascii_case(name))
    }
}

fn trim_ws(values: &[ComponentValue]) -> &[ComponentValue] {
    let start = values.iter().position(|v| !v.is_whitespace()).unwrap_or(values.len());
    let end = values.iter().rposition(|v| !v.is_whitespace()).map_or(start, |e| e + 1);
    &values[start..end.max(start)]
}

fn split_commas(values: &[ComponentValue]) -> Vec<&[ComponentValue]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, v) in values.iter().enumerate() {
        if matches!(v, ComponentValue::Token(Token::Comma)) {
            out.push(&values[start..i]);
            start = i + 1;
        }
    }
    out.push(&values[start..]);
    out
}

/// `<media-condition>` (or without `or` when `allow_or` is false).
fn parse_condition(p: &mut P, allow_or: bool) -> Option<MediaCondition> {
    p.skip_ws();
    if p.ident_is(0, "not") {
        p.pos += 1;
        p.skip_ws();
        let inner = parse_in_parens(p)?;
        return Some(MediaCondition::Not(Box::new(inner)));
    }
    let first = parse_in_parens(p)?;
    p.skip_ws();
    let joiner = if p.ident_is(0, "and") {
        "and"
    } else if p.ident_is(0, "or") && allow_or {
        "or"
    } else {
        return Some(first);
    };
    let mut items = vec![first];
    while p.ident_is(0, joiner) {
        p.pos += 1;
        p.skip_ws();
        items.push(parse_in_parens(p)?);
        p.skip_ws();
    }
    if p.ident_is(0, "and") || p.ident_is(0, "or") {
        return None;
    }
    Some(if joiner == "and" { MediaCondition::And(items) } else { MediaCondition::Or(items) })
}

fn parse_in_parens(p: &mut P) -> Option<MediaCondition> {
    match p.next()? {
        ComponentValue::Block { open: Token::OpenParen, contents } => {
            let inner = trim_ws(contents);
            // A nested condition.
            let mut q = P { values: inner, pos: 0 };
            let is_cond = matches!(q.peek_at(0), Some(ComponentValue::Block { open: Token::OpenParen, .. })) || (q.ident_is(0, "not") && q.peek_at(1).is_some_and(|v| v.is_whitespace()) && matches!(q.peek_at(2), Some(ComponentValue::Block { open: Token::OpenParen, .. })));
            if is_cond {
                let c = parse_condition(&mut q, true);
                q.skip_ws();
                return match c {
                    Some(c) if q.at_end() => Some(c),
                    _ => Some(MediaCondition::Unknown(serialize(contents))),
                };
            }
            Some(parse_feature_or_range(inner).unwrap_or_else(|| MediaCondition::Unknown(serialize(contents))))
        }
        ComponentValue::Function { name, args } => Some(MediaCondition::Unknown(format!("{name}({})", serialize(args)))),
        _ => None,
    }
}

fn serialize(values: &[ComponentValue]) -> String {
    let mut s = String::new();
    for v in values {
        super::parser::serialize_component_value(v, &mut s);
    }
    s
}

fn parse_feature(values: &[ComponentValue]) -> Option<MediaFeature> {
    let items: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    let ident = |v: &ComponentValue| match v {
        ComponentValue::Token(Token::Ident(i)) => Some(i.to_ascii_lowercase()),
        _ => None,
    };
    let is_delim = |v: &ComponentValue, c: char| matches!(v, ComponentValue::Token(Token::Delim(d)) if *d == c);
    // Boolean context: `(name)`.
    if let [v] = items.as_slice() {
        return ident(v).map(MediaFeature::Boolean);
    }
    // Plain: `name : value`.
    if items.len() >= 3 && matches!(items[1], ComponentValue::Token(Token::Colon)) {
        let name = ident(items[0])?;
        let value = parse_value(&items[2..])?;
        let (name, op) = if let Some(n) = name.strip_prefix("min-") {
            (n.to_owned(), CompareOp::Ge)
        } else if let Some(n) = name.strip_prefix("max-") {
            (n.to_owned(), CompareOp::Le)
        } else {
            (name, CompareOp::Eq)
        };
        return Some(MediaFeature::Compare { name, op, value });
    }
    // Range syntax. Find the comparison operators.
    let mut ops: Vec<(usize, usize, CompareOp)> = Vec::new(); // (start, end, op)
    let mut i = 0;
    while i < items.len() {
        let op = if is_delim(items[i], '<') || is_delim(items[i], '>') {
            let c = if is_delim(items[i], '<') { CompareOp::Lt } else { CompareOp::Gt };
            if i + 1 < items.len() && is_delim(items[i + 1], '=') {
                ops.push((i, i + 2, if c == CompareOp::Lt { CompareOp::Le } else { CompareOp::Ge }));
                i += 2;
                continue;
            }
            c
        } else if is_delim(items[i], '=') {
            CompareOp::Eq
        } else {
            i += 1;
            continue;
        };
        ops.push((i, i + 1, op));
        i += 1;
    }
    match ops.as_slice() {
        [(s, e, op)] => {
            let left = &items[..*s];
            let right = &items[*e..];
            if let [l] = left {
                if let Some(name) = ident(l) {
                    if !matches!(op, CompareOp::Eq) || right.len() == 1 {
                        return Some(MediaFeature::Compare { name, op: *op, value: parse_value(right)? });
                    }
                }
            }
            if let [r] = right {
                if let Some(name) = ident(r) {
                    return Some(MediaFeature::Compare { name, op: op.flip(), value: parse_value(left)? });
                }
            }
            None
        }
        _ => None,
    }
}

/// Range-form features with two comparisons expand to an `and`; handled here so that
/// `parse_feature` stays a single feature.
fn parse_feature_or_range(values: &[ComponentValue]) -> Option<MediaCondition> {
    if let Some(f) = parse_feature(values) {
        return Some(MediaCondition::Feature(f));
    }
    let items: Vec<&ComponentValue> = values.iter().filter(|v| !v.is_whitespace()).collect();
    let is_delim = |v: &ComponentValue, c: char| matches!(v, ComponentValue::Token(Token::Delim(d)) if *d == c);
    let mut ops: Vec<(usize, usize, CompareOp)> = Vec::new();
    let mut i = 0;
    while i < items.len() {
        if is_delim(items[i], '<') || is_delim(items[i], '>') {
            let lt = is_delim(items[i], '<');
            if i + 1 < items.len() && is_delim(items[i + 1], '=') {
                ops.push((i, i + 2, if lt { CompareOp::Le } else { CompareOp::Ge }));
                i += 2;
            } else {
                ops.push((i, i + 1, if lt { CompareOp::Lt } else { CompareOp::Gt }));
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    if let [(s1, e1, op1), (s2, e2, op2)] = ops.as_slice() {
        if *s2 != *e1 + 1 {
            return None;
        }
        let name = match items[*e1] {
            ComponentValue::Token(Token::Ident(i)) => i.to_ascii_lowercase(),
            _ => return None,
        };
        let same_dir = matches!((op1, op2), (CompareOp::Lt | CompareOp::Le, CompareOp::Lt | CompareOp::Le) | (CompareOp::Gt | CompareOp::Ge, CompareOp::Gt | CompareOp::Ge));
        if !same_dir {
            return None;
        }
        let lo = parse_value(&items[..*s1])?;
        let hi = parse_value(&items[*e2..])?;
        return Some(MediaCondition::And(vec![MediaCondition::Feature(MediaFeature::Compare { name: name.clone(), op: op1.flip(), value: lo }), MediaCondition::Feature(MediaFeature::Compare { name, op: *op2, value: hi })]));
    }
    None
}

fn length_micro(value: Number, unit: &str) -> Option<i64> {
    let m = value.micro;
    Some(match unit.to_ascii_lowercase().as_str() {
        "px" => m,
        "em" | "rem" => m.saturating_mul(16),
        "in" => m.saturating_mul(96),
        "cm" => m.saturating_mul(9600) / 254,
        "mm" => m.saturating_mul(960) / 254,
        "q" => m.saturating_mul(240) / 254,
        "pt" => m.saturating_mul(4) / 3,
        "pc" => m.saturating_mul(16),
        _ => return None,
    })
}

fn parse_value(items: &[&ComponentValue]) -> Option<FeatureValue> {
    match items {
        [ComponentValue::Token(Token::Dimension { value, unit, .. })] => {
            let u = unit.to_ascii_lowercase();
            if u == "dppx" || u == "x" {
                Some(FeatureValue::Resolution(value.micro))
            } else if u == "dpi" {
                Some(FeatureValue::Resolution(value.micro / 96))
            } else if u == "dpcm" {
                Some(FeatureValue::Resolution(value.micro.saturating_mul(254) / 9600))
            } else {
                length_micro(*value, unit).map(FeatureValue::Length)
            }
        }
        [ComponentValue::Token(Token::Number { value, .. })] => Some(FeatureValue::Number(*value)),
        [ComponentValue::Token(Token::Number { value: n, .. }), ComponentValue::Token(Token::Delim('/')), ComponentValue::Token(Token::Number { value: d, .. })] => {
            if n.is_negative() || d.is_negative() {
                None
            } else {
                Some(FeatureValue::Ratio(n.micro, d.micro))
            }
        }
        [ComponentValue::Token(Token::Ident(i))] => Some(FeatureValue::Ident(i.to_ascii_lowercase())),
        _ => None,
    }
}

impl MediaCondition {
    /// `None` is "unknown".
    pub fn evaluate(&self, media: &Media) -> Option<bool> {
        match self {
            MediaCondition::Not(c) => c.evaluate(media).map(|v| !v),
            MediaCondition::And(items) => {
                let mut unknown = false;
                for i in items {
                    match i.evaluate(media) {
                        Some(false) => return Some(false),
                        None => unknown = true,
                        Some(true) => {}
                    }
                }
                if unknown {
                    None
                } else {
                    Some(true)
                }
            }
            MediaCondition::Or(items) => {
                let mut unknown = false;
                for i in items {
                    match i.evaluate(media) {
                        Some(true) => return Some(true),
                        None => unknown = true,
                        Some(false) => {}
                    }
                }
                if unknown {
                    None
                } else {
                    Some(false)
                }
            }
            MediaCondition::Feature(f) => f.evaluate(media),
            MediaCondition::Unknown(_) => None,
        }
    }
}

impl MediaFeature {
    pub fn evaluate(&self, media: &Media) -> Option<bool> {
        let px = |v: i32| (v as i64).saturating_mul(1_000_000);
        match self {
            MediaFeature::Boolean(name) => Some(match name.as_str() {
                "width" | "device-width" => media.width_px != 0,
                "height" | "device-height" => media.height_px != 0,
                "aspect-ratio" | "device-aspect-ratio" => media.width_px != 0 && media.height_px != 0,
                "orientation" | "resolution" | "prefers-color-scheme" | "display-mode" | "color" | "color-gamut" => true,
                "hover" => media.hover == HoverCapability::Hover,
                "any-hover" => media.hover == HoverCapability::Hover,
                "pointer" | "any-pointer" => media.pointer != PointerCapability::None,
                "prefers-reduced-motion" => media.reduced_motion,
                "monochrome" | "grid" | "prefers-contrast" | "forced-colors" | "inverted-colors" => false,
                _ => return None,
            }),
            MediaFeature::Compare { name, op, value } => {
                let ord = |actual: i64, wanted: i64| Some(op.holds(actual.cmp(&wanted)));
                match (name.as_str(), value) {
                    ("width" | "device-width", FeatureValue::Length(l)) => ord(px(media.width_px), *l),
                    ("height" | "device-height", FeatureValue::Length(l)) => ord(px(media.height_px), *l),
                    ("aspect-ratio" | "device-aspect-ratio", v) => {
                        let (n, d) = match v {
                            FeatureValue::Ratio(n, d) => (*n, *d),
                            FeatureValue::Number(n) => (n.micro, 1_000_000),
                            _ => return None,
                        };
                        if d == 0 {
                            return None;
                        }
                        // width/height vs n/d  <=>  width*d vs height*n
                        let lhs = (media.width_px as i128) * (d as i128);
                        let rhs = (media.height_px as i128) * (n as i128);
                        Some(op.holds(lhs.cmp(&rhs)))
                    }
                    ("orientation", FeatureValue::Ident(i)) if *op == CompareOp::Eq => Some(match i.as_str() {
                        "portrait" => media.height_px >= media.width_px,
                        "landscape" => media.width_px > media.height_px,
                        _ => return None,
                    }),
                    ("resolution", FeatureValue::Resolution(r)) => ord(media.dppx.micro, *r),
                    ("prefers-color-scheme", FeatureValue::Ident(i)) if *op == CompareOp::Eq => Some(match i.as_str() {
                        "light" => media.color_scheme == ColorScheme::Light,
                        "dark" => media.color_scheme == ColorScheme::Dark,
                        _ => return None,
                    }),
                    ("prefers-reduced-motion", FeatureValue::Ident(i)) if *op == CompareOp::Eq => Some(match i.as_str() {
                        "reduce" => media.reduced_motion,
                        "no-preference" => !media.reduced_motion,
                        _ => return None,
                    }),
                    ("hover" | "any-hover", FeatureValue::Ident(i)) if *op == CompareOp::Eq => Some(match i.as_str() {
                        "hover" => media.hover == HoverCapability::Hover,
                        "none" => media.hover == HoverCapability::None,
                        _ => return None,
                    }),
                    ("pointer" | "any-pointer", FeatureValue::Ident(i)) if *op == CompareOp::Eq => Some(match i.as_str() {
                        "fine" => media.pointer == PointerCapability::Fine,
                        "coarse" => media.pointer == PointerCapability::Coarse,
                        "none" => media.pointer == PointerCapability::None,
                        _ => return None,
                    }),
                    ("display-mode", FeatureValue::Ident(i)) if *op == CompareOp::Eq => Some(match i.as_str() {
                        "browser" => media.display_mode == DisplayMode::Browser,
                        "minimal-ui" => media.display_mode == DisplayMode::MinimalUi,
                        "standalone" => media.display_mode == DisplayMode::Standalone,
                        "fullscreen" => media.display_mode == DisplayMode::Fullscreen,
                        _ => return None,
                    }),
                    ("color", FeatureValue::Number(n)) => ord(8_000_000, n.micro),
                    ("monochrome", FeatureValue::Number(n)) => ord(0, n.micro),
                    _ => None,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Serialization

impl fmt::Display for MediaQueryList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, q) in self.queries.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{q}")?;
        }
        Ok(())
    }
}

impl fmt::Display for MediaQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.invalid {
            return f.write_str("not all");
        }
        if self.negated {
            f.write_str("not ")?;
        } else if self.only {
            f.write_str("only ")?;
        }
        match (&self.media_type, &self.condition) {
            (None, Some(c)) if !self.negated && !self.only => write!(f, "{c}"),
            (None, Some(c)) => write!(f, "all and {c}"),
            (None, None) => f.write_str("all"),
            (Some(t), None) => f.write_str(t),
            (Some(t), Some(c)) => write!(f, "{t} and {c}"),
        }
    }
}

impl fmt::Display for MediaCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaCondition::Not(c) => write!(f, "not {}", Paren(c)),
            MediaCondition::And(items) => {
                for (i, c) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" and ")?;
                    }
                    write!(f, "{}", Paren(c))?;
                }
                Ok(())
            }
            MediaCondition::Or(items) => {
                for (i, c) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" or ")?;
                    }
                    write!(f, "{}", Paren(c))?;
                }
                Ok(())
            }
            MediaCondition::Feature(feat) => write!(f, "({feat})"),
            MediaCondition::Unknown(s) => write!(f, "({s})"),
        }
    }
}

struct Paren<'a>(&'a MediaCondition);
impl fmt::Display for Paren<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            MediaCondition::And(_) | MediaCondition::Or(_) | MediaCondition::Not(_) => write!(f, "({})", self.0),
            c => write!(f, "{c}"),
        }
    }
}

impl fmt::Display for FeatureValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeatureValue::Length(l) => write!(f, "{}px", Number { micro: *l, int: l % 1_000_000 == 0 }.to_f64()),
            FeatureValue::Number(n) => write!(f, "{}", n.to_f64()),
            FeatureValue::Ratio(n, d) => write!(f, "{} / {}", Number { micro: *n, int: true }.to_f64(), Number { micro: *d, int: true }.to_f64()),
            FeatureValue::Resolution(r) => write!(f, "{}dppx", Number { micro: *r, int: true }.to_f64()),
            FeatureValue::Ident(i) => f.write_str(i),
        }
    }
}

impl fmt::Display for MediaFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaFeature::Boolean(n) => f.write_str(n),
            MediaFeature::Compare { name, op, value } => match op {
                CompareOp::Eq => write!(f, "{name}: {value}"),
                CompareOp::Ge => write!(f, "min-{name}: {value}"),
                CompareOp::Le => write!(f, "max-{name}: {value}"),
                op => write!(f, "{name} {} {value}", op.text()),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// @supports

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SupportsCondition {
    Not(Box<SupportsCondition>),
    And(Vec<SupportsCondition>),
    Or(Vec<SupportsCondition>),
    /// `(property: value)`; `value` is trimmed of surrounding whitespace.
    Declaration { property: String, value: Vec<ComponentValue> },
    /// `selector(<selector-list>)`: `Ok` when the selector parsed and every
    /// pseudo-element in it is supported.
    Selector { text: String, supported: bool },
    /// `<general-enclosed>`: never supported.
    Unknown(String),
}

/// Parses an `@supports` prelude; `None` means the rule is invalid and dropped.
pub fn parse_supports_condition(values: &[ComponentValue]) -> Option<SupportsCondition> {
    let mut p = P { values: trim_ws(values), pos: 0 };
    let c = parse_supports(&mut p)?;
    p.skip_ws();
    if p.at_end() {
        Some(c)
    } else {
        None
    }
}

fn parse_supports(p: &mut P) -> Option<SupportsCondition> {
    p.skip_ws();
    if p.ident_is(0, "not") {
        p.pos += 1;
        p.skip_ws();
        return Some(SupportsCondition::Not(Box::new(parse_supports_in_parens(p)?)));
    }
    let first = parse_supports_in_parens(p)?;
    p.skip_ws();
    let joiner = if p.ident_is(0, "and") {
        "and"
    } else if p.ident_is(0, "or") {
        "or"
    } else {
        return Some(first);
    };
    let mut items = vec![first];
    while p.ident_is(0, joiner) {
        p.pos += 1;
        p.skip_ws();
        items.push(parse_supports_in_parens(p)?);
        p.skip_ws();
    }
    if p.ident_is(0, "and") || p.ident_is(0, "or") {
        return None;
    }
    Some(if joiner == "and" { SupportsCondition::And(items) } else { SupportsCondition::Or(items) })
}

fn parse_supports_in_parens(p: &mut P) -> Option<SupportsCondition> {
    match p.next()? {
        ComponentValue::Block { open: Token::OpenParen, contents } => {
            let inner = trim_ws(contents);
            let mut q = P { values: inner, pos: 0 };
            let is_cond = matches!(q.peek_at(0), Some(ComponentValue::Block { open: Token::OpenParen, .. }) | Some(ComponentValue::Function { .. })) || q.ident_is(0, "not");
            if is_cond {
                if let Some(c) = parse_supports(&mut q) {
                    q.skip_ws();
                    if q.at_end() {
                        return Some(c);
                    }
                }
            }
            // A declaration: ident ':' value.
            let mut q = P { values: inner, pos: 0 };
            if let Some(ComponentValue::Token(Token::Ident(name))) = q.peek_at(0) {
                q.pos += 1;
                q.skip_ws();
                if matches!(q.peek_at(0), Some(ComponentValue::Token(Token::Colon))) {
                    q.pos += 1;
                    let value = trim_ws(&inner[q.pos..]).to_vec();
                    let property = if name.starts_with("--") { name.clone() } else { name.to_ascii_lowercase() };
                    return Some(SupportsCondition::Declaration { property, value });
                }
            }
            Some(SupportsCondition::Unknown(serialize(contents)))
        }
        ComponentValue::Function { name, args } if name.eq_ignore_ascii_case("selector") => {
            let text = serialize(args);
            let supported = match SelectorList::parse(args) {
                Ok(list) => list.0.iter().all(|s| s.pseudo_element.is_none_or(|pe| pe.supported())),
                Err(_) => false,
            };
            Some(SupportsCondition::Selector { text, supported })
        }
        ComponentValue::Function { name, args } => Some(SupportsCondition::Unknown(format!("{name}({})", serialize(args)))),
        _ => None,
    }
}

impl SupportsCondition {
    /// Evaluates with the style track's knowledge of which declarations it accepts.
    pub fn evaluate(&self, is_supported: &dyn Fn(&str, &[ComponentValue]) -> bool) -> bool {
        match self {
            SupportsCondition::Not(c) => !c.evaluate(is_supported),
            SupportsCondition::And(items) => items.iter().all(|c| c.evaluate(is_supported)),
            SupportsCondition::Or(items) => items.iter().any(|c| c.evaluate(is_supported)),
            SupportsCondition::Declaration { property, value } => is_supported(property, value),
            SupportsCondition::Selector { supported, .. } => *supported,
            SupportsCondition::Unknown(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(q: &str, m: &Media) -> bool {
        MediaQueryList::parse(q).evaluate(m)
    }

    #[test]
    fn media_types_and_modifiers() {
        let m = Media::default();
        assert!(eval("", &m));
        assert!(eval("all", &m));
        assert!(eval("screen", &m));
        assert!(!eval("print", &m));
        assert!(eval("not print", &m));
        assert!(eval("only screen", &m));
        assert!(!eval("only print", &m));
        assert!(eval("print, screen", &m));
        assert!(!eval("tv", &m));
        assert!(eval("not tv", &m));
        let p = Media { media_type: MediaType::Print, ..Media::default() };
        assert!(eval("print", &p));
        assert!(!eval("screen and (width)", &p));
        // Grammar errors become `not all`.
        assert!(!eval("screen and", &m));
        assert!(!eval("and (width)", &m));
        assert!(!eval("screen (width)", &m));
        assert!(!eval("only", &m));
        assert!(!eval("(width) or (height) and (hover)", &m));
        assert!(!eval("screen and (width) or (height)", &m));
        assert_eq!(MediaQueryList::parse("screen and, print").to_string(), "not all, print");
    }

    #[test]
    fn width_height_and_ranges() {
        let m = Media::with_size(800, 600);
        assert!(eval("(width: 800px)", &m));
        assert!(!eval("(width: 801px)", &m));
        assert!(eval("(min-width: 800px)", &m));
        assert!(eval("(min-width: 600px) and (max-width: 900px)", &m));
        assert!(!eval("(min-width: 801px)", &m));
        assert!(eval("(max-height: 600px)", &m));
        assert!(eval("(width >= 600px)", &m));
        assert!(eval("(width > 600px)", &m));
        assert!(!eval("(width > 800px)", &m));
        assert!(eval("(600px <= width <= 900px)", &m));
        assert!(eval("(900px > width > 600px)", &m));
        assert!(!eval("(600px <= width < 800px)", &m));
        assert!(eval("(800px = width)", &m));
        assert!(eval("(width = 800px)", &m));
        assert!(!eval("(600px <= width >= 900px)", &m));
        assert!(eval("(min-width: 50em)", &m));
        assert!(eval("(max-width: 600pt)", &m));
        assert!(eval("(width < 9in)", &m));
        assert!(!eval("(width: 800)", &m));
        assert!(eval("(width)", &m));
        assert!(!eval("(width)", &Media::with_size(0, 0)));
        assert!(eval("(orientation: landscape)", &m));
        assert!(!eval("(orientation: portrait)", &m));
        assert!(eval("(orientation: portrait)", &Media::with_size(600, 600)));
        assert!(eval("(aspect-ratio: 4/3)", &m));
        assert!(eval("(aspect-ratio: 4 / 3)", &m));
        assert!(eval("(min-aspect-ratio: 1/1)", &m));
        assert!(!eval("(min-aspect-ratio: 16/9)", &m));
        assert!(eval("(aspect-ratio > 1)", &m));
        assert!(eval("(max-aspect-ratio: 16/9)", &m));
    }

    #[test]
    fn resolution_and_preferences() {
        let m = Media::default();
        assert!(eval("(resolution: 1dppx)", &m));
        assert!(eval("(min-resolution: 96dpi)", &m));
        assert!(!eval("(min-resolution: 1.5dppx)", &m));
        assert!(eval("(resolution >= 1x)", &m));
        let hi = Media { dppx: Number::parse("2").unwrap(), ..Media::default() };
        assert!(eval("(min-resolution: 1.5dppx)", &hi));
        assert!(eval("(min-resolution: 192dpi)", &hi));
        assert!(eval("(prefers-color-scheme: light)", &m));
        assert!(!eval("(prefers-color-scheme: dark)", &m));
        assert!(eval("(prefers-color-scheme: dark)", &Media { color_scheme: ColorScheme::Dark, ..m }));
        assert!(!eval("(prefers-reduced-motion: reduce)", &m));
        assert!(eval("(prefers-reduced-motion: no-preference)", &m));
        assert!(eval("(prefers-reduced-motion)", &Media { reduced_motion: true, ..m }));
        assert!(eval("(hover: hover)", &m));
        assert!(eval("(hover)", &m));
        assert!(eval("(any-hover: hover)", &m));
        assert!(!eval("(hover: none)", &m));
        assert!(eval("(hover: none)", &Media { hover: HoverCapability::None, ..m }));
        assert!(eval("(pointer: fine)", &m));
        assert!(eval("(any-pointer: fine)", &m));
        assert!(!eval("(pointer: coarse)", &m));
        assert!(eval("(pointer: coarse)", &Media { pointer: PointerCapability::Coarse, ..m }));
        assert!(!eval("(pointer)", &Media { pointer: PointerCapability::None, ..m }));
        assert!(eval("(display-mode: browser)", &m));
        assert!(!eval("(display-mode: standalone)", &m));
        assert!(eval("(display-mode: standalone)", &Media { display_mode: DisplayMode::Standalone, ..m }));
    }

    #[test]
    fn logic_and_unknown() {
        let m = Media::with_size(800, 600);
        assert!(eval("(width: 800px) and (height: 600px)", &m));
        assert!(eval("(width: 1px) or (height: 600px)", &m));
        assert!(!eval("not (width: 800px)", &m));
        assert!(eval("not (width: 1px)", &m));
        assert!(eval("((width: 1px) or (height: 600px)) and (hover)", &m));
        assert!(eval("screen and (width: 800px) and (hover)", &m));
        assert!(eval("not screen and (width: 1px)", &m));
        assert!(!eval("(foo: bar)", &m));
        assert!(!eval("not (foo: bar)", &m));
        assert!(eval("(foo: bar) or (width)", &m));
        assert!(!eval("(foo: bar) and (width)", &m));
        assert!(!eval("not ((foo: bar) and (width))", &m));
        assert!(!eval("(width: 800px) and (color-index)", &m));
        assert!(!eval("general(enclosed)", &m));
        assert!(!eval("screen and general(enclosed)", &m));
        assert!(eval("(prefers-color-scheme)", &m));
    }

    #[test]
    fn serialization() {
        assert_eq!(MediaQueryList::parse("SCREEN AND (MIN-WIDTH: 600px), print").to_string(), "screen and (min-width: 600px), print");
        assert_eq!(MediaQueryList::parse("(width >= 600px) and (hover)").to_string(), "(min-width: 600px) and (hover)");
        assert_eq!(MediaQueryList::parse("(width > 600px) and (600px < height)").to_string(), "(width > 600px) and (height > 600px)");
        assert_eq!(MediaQueryList::parse("not print").to_string(), "not print");
        assert_eq!(MediaQueryList::parse("not (hover)").to_string(), "not (hover)");
        assert_eq!(MediaQueryList::parse("(600px <= width <= 900px)").to_string(), "(min-width: 600px) and (max-width: 900px)");
        assert_eq!(MediaQueryList::parse("(aspect-ratio: 16/9)").to_string(), "(aspect-ratio: 16 / 9)");
        assert_eq!(MediaQueryList::parse("(prefers-color-scheme: dark)").to_string(), "(prefers-color-scheme: dark)");
        let q = MediaQueryList::parse("only screen and (max-width: 20em)");
        assert_eq!(MediaQueryList::parse(&q.to_string()), q);
    }

    #[test]
    fn supports_conditions() {
        let ok = |p: &str, v: &[ComponentValue]| p == "display" && v.iter().any(|c| c.as_ident() == Some("grid"));
        let ev = |s: &str| parse_supports_condition(&super::super::parser::parse_component_value_list(s)).map(|c| c.evaluate(&ok));
        assert_eq!(ev("(display: grid)"), Some(true));
        assert_eq!(ev("(display: flex)"), Some(false));
        assert_eq!(ev("not (display: flex)"), Some(true));
        assert_eq!(ev("(display: grid) and (display: flex)"), Some(false));
        assert_eq!(ev("(display: grid) or (display: flex)"), Some(true));
        assert_eq!(ev("((display: grid) or (display: flex)) and (not (display: flex))"), Some(true));
        assert_eq!(ev("(display: grid) and (display: grid) or (display: grid)"), None);
        assert_eq!(ev("display: grid"), None);
        assert_eq!(ev("selector(a:hover)"), Some(true));
        assert_eq!(ev("selector(a::first-line)"), Some(false));
        // `:host` is a valid selector (it just never matches a document tree).
        assert_eq!(ev("selector(a:host)"), Some(true));
        assert_eq!(ev("selector(a:host-context(x))"), Some(false));
        assert_eq!(ev("selector(:has(> a))"), Some(true));
        assert_eq!(ev("not selector(a || b)"), Some(true));
        assert_eq!(ev("font-tech(color-COLRv1)"), Some(false));
        assert_eq!(ev("(--x: 1)"), Some(false));
        assert_eq!(ev("(display : grid )"), Some(true));
        assert_eq!(ev("( ( display: grid ) )"), Some(true));
        match parse_supports_condition(&super::super::parser::parse_component_value_list("(DISPLAY: grid )")) {
            Some(SupportsCondition::Declaration { property, value }) => {
                assert_eq!(property, "display");
                assert_eq!(value.len(), 1);
            }
            o => panic!("{o:?}"),
        }
    }
}
