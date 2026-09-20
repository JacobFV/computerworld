//! CSS value parsing: a cursor over component values, and the value types shared by
//! every property: lengths in every unit, percentages, `calc()`/`min()`/`max()`/
//! `clamp()`, angles, times, colours, `url()`, strings, identifiers and images.
//! Everything is integer fixed point: numbers in millionths, lengths in `Au`,
//! percentages in myriads (1/100 of a percent), angles in centi-degrees, times in ms.

use crate::css::token::{ComponentValue, Number, Token};
use crate::geom::Au;
use cw_scene::Color;

/// A cursor over a slice of component values that skips whitespace and backtracks.
pub struct Parser<'a> {
    items: &'a [ComponentValue],
    pos: usize,
}

impl<'a> Parser<'a> {
    pub fn new(items: &'a [ComponentValue]) -> Parser<'a> {
        Parser { items, pos: 0 }
    }
    fn skip_ws(&mut self) {
        while self.pos < self.items.len() && self.items[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }
    pub fn peek(&mut self) -> Option<&'a ComponentValue> {
        self.skip_ws();
        self.items.get(self.pos)
    }
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&'a ComponentValue> {
        self.skip_ws();
        let v = self.items.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }
    pub fn is_done(&mut self) -> bool {
        self.peek().is_none()
    }
    pub fn position(&self) -> usize {
        self.pos
    }
    pub fn reset(&mut self, pos: usize) {
        self.pos = pos;
    }
    /// Runs `f`; on `None` restores the position.
    pub fn try_parse<T>(&mut self, f: impl FnOnce(&mut Parser<'a>) -> Option<T>) -> Option<T> {
        let start = self.pos;
        let r = f(self);
        if r.is_none() {
            self.pos = start;
        }
        r
    }
    /// Parses with `f` and requires that nothing follows.
    pub fn parse_entirely<T>(&mut self, f: impl FnOnce(&mut Parser<'a>) -> Option<T>) -> Option<T> {
        let start = self.pos;
        let r = f(self);
        if r.is_some() && self.is_done() {
            r
        } else {
            self.pos = start;
            None
        }
    }
    pub fn expect_ident(&mut self) -> Option<&'a str> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Ident(s)) => Some(s.as_str()),
            _ => None,
        })
    }
    /// The next identifier, lower-cased.
    pub fn expect_ident_lower(&mut self) -> Option<String> {
        self.expect_ident().map(|s| s.to_ascii_lowercase())
    }
    pub fn expect_ident_matching(&mut self, kw: &str) -> Option<()> {
        self.try_parse(|p| {
            let s = p.expect_ident()?;
            if s.eq_ignore_ascii_case(kw) {
                Some(())
            } else {
                None
            }
        })
    }
    /// Peeks the next identifier lower-cased, without consuming.
    pub fn peek_ident_lower(&mut self) -> Option<String> {
        match self.peek()? {
            ComponentValue::Token(Token::Ident(s)) => Some(s.to_ascii_lowercase()),
            _ => None,
        }
    }
    pub fn expect_comma(&mut self) -> Option<()> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Comma) => Some(()),
            _ => None,
        })
    }
    pub fn expect_delim(&mut self, c: char) -> Option<()> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Delim(d)) if *d == c => Some(()),
            _ => None,
        })
    }
    pub fn expect_number(&mut self) -> Option<Number> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Number { value, .. }) => Some(*value),
            _ => None,
        })
    }
    pub fn expect_integer(&mut self) -> Option<i32> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Number { value, .. }) if value.int => Some(value.round()),
            _ => None,
        })
    }
    pub fn expect_percentage(&mut self) -> Option<Number> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Percentage { value, .. }) => Some(*value),
            _ => None,
        })
    }
    pub fn expect_dimension(&mut self) -> Option<(Number, &'a str)> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Dimension { value, unit, .. }) => Some((*value, unit.as_str())),
            _ => None,
        })
    }
    pub fn expect_string(&mut self) -> Option<&'a str> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::String(s)) => Some(s.as_str()),
            _ => None,
        })
    }
    pub fn expect_function(&mut self) -> Option<(&'a str, &'a [ComponentValue])> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Function { name, args } => Some((name.as_str(), args.as_slice())),
            _ => None,
        })
    }
    pub fn expect_function_named(&mut self, name: &str) -> Option<&'a [ComponentValue]> {
        self.try_parse(|p| {
            let (n, args) = p.expect_function()?;
            if n.eq_ignore_ascii_case(name) {
                Some(args)
            } else {
                None
            }
        })
    }
    /// `url(...)` in either form, or `src("...")`.
    pub fn expect_url(&mut self) -> Option<String> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Token(Token::Url(u)) => Some(u.clone()),
            ComponentValue::Function { name, args } if name.eq_ignore_ascii_case("url") || name.eq_ignore_ascii_case("src") => {
                let mut inner = Parser::new(args);
                let s = inner.expect_string()?.to_owned();
                // Ignore url modifiers.
                Some(s)
            }
            _ => None,
        })
    }
    /// A `[ ... ]` block's contents.
    pub fn expect_square_block(&mut self) -> Option<&'a [ComponentValue]> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Block { open: Token::OpenSquare, contents } => Some(contents.as_slice()),
            _ => None,
        })
    }
    pub fn expect_paren_block(&mut self) -> Option<&'a [ComponentValue]> {
        self.try_parse(|p| match p.next()? {
            ComponentValue::Block { open: Token::OpenParen, contents } => Some(contents.as_slice()),
            _ => None,
        })
    }
    /// The remaining items (whitespace-trimmed), consumed.
    pub fn rest(&mut self) -> &'a [ComponentValue] {
        self.skip_ws();
        let r = &self.items[self.pos..];
        self.pos = self.items.len();
        r
    }
    /// Parses a comma-separated list of `f`, at least one item.
    pub fn comma_list<T>(&mut self, mut f: impl FnMut(&mut Parser<'a>) -> Option<T>) -> Option<Vec<T>> {
        let mut out = vec![f(self)?];
        while self.expect_comma().is_some() {
            out.push(f(self)?);
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------------
// Fixed-point helpers

/// `a / b` rounded half away from zero; `b != 0`.
pub fn round_div(a: i128, b: i128) -> i128 {
    let h = b.abs() / 2;
    if (a >= 0) == (b > 0) {
        (a + h) / b
    } else {
        (a - h) / b
    }
}

fn clamp_i32(v: i128) -> i32 {
    v.clamp(i32::MIN as i128, i32::MAX as i128) as i32
}

/// Millionths to a myriad percentage (50% is 5000).
pub fn micro_to_myriad(micro: i64) -> i32 {
    clamp_i32(round_div(micro as i128, 10_000))
}

/// Millionths of a pixel to `Au`, rounded half away from zero.
pub fn micro_px_to_au(micro_px: i128) -> Au {
    let v = round_div(micro_px * Au::PER_PX as i128, 1_000_000);
    Au(v.clamp(Au::MIN.0 as i128, Au::MAX.0 as i128) as i32)
}

/// A number in thousandths, rounded.
pub fn micro_to_milli(micro: i64) -> i32 {
    clamp_i32(round_div(micro as i128, 1000))
}

// ---------------------------------------------------------------------------------
// CSS-wide keywords

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CssWide {
    Initial,
    Inherit,
    Unset,
    Revert,
    RevertLayer,
}

pub fn parse_css_wide(p: &mut Parser) -> Option<CssWide> {
    p.try_parse(|p| {
        let s = p.expect_ident_lower()?;
        let k = match s.as_str() {
            "initial" => CssWide::Initial,
            "inherit" => CssWide::Inherit,
            "unset" => CssWide::Unset,
            "revert" => CssWide::Revert,
            "revert-layer" => CssWide::RevertLayer,
            _ => return None,
        };
        if p.is_done() {
            Some(k)
        } else {
            None
        }
    })
}

// ---------------------------------------------------------------------------------
// Lengths

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthUnit {
    Px,
    Em,
    Rem,
    Ex,
    Ch,
    Vw,
    Vh,
    Vmin,
    Vmax,
    Cm,
    Mm,
    Q,
    In,
    Pt,
    Pc,
    Lh,
    Rlh,
}

impl LengthUnit {
    pub fn parse(unit: &str) -> Option<LengthUnit> {
        Some(match unit.to_ascii_lowercase().as_str() {
            "px" => LengthUnit::Px,
            "em" => LengthUnit::Em,
            "rem" => LengthUnit::Rem,
            "ex" => LengthUnit::Ex,
            "ch" => LengthUnit::Ch,
            "vw" => LengthUnit::Vw,
            "vh" => LengthUnit::Vh,
            "vmin" => LengthUnit::Vmin,
            "vmax" => LengthUnit::Vmax,
            "cm" => LengthUnit::Cm,
            "mm" => LengthUnit::Mm,
            "q" => LengthUnit::Q,
            "in" => LengthUnit::In,
            "pt" => LengthUnit::Pt,
            "pc" => LengthUnit::Pc,
            "lh" => LengthUnit::Lh,
            "rlh" => LengthUnit::Rlh,
            _ => return None,
        })
    }
    pub fn is_absolute(self) -> bool {
        matches!(self, LengthUnit::Px | LengthUnit::Cm | LengthUnit::Mm | LengthUnit::Q | LengthUnit::In | LengthUnit::Pt | LengthUnit::Pc)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            LengthUnit::Px => "px",
            LengthUnit::Em => "em",
            LengthUnit::Rem => "rem",
            LengthUnit::Ex => "ex",
            LengthUnit::Ch => "ch",
            LengthUnit::Vw => "vw",
            LengthUnit::Vh => "vh",
            LengthUnit::Vmin => "vmin",
            LengthUnit::Vmax => "vmax",
            LengthUnit::Cm => "cm",
            LengthUnit::Mm => "mm",
            LengthUnit::Q => "Q",
            LengthUnit::In => "in",
            LengthUnit::Pt => "pt",
            LengthUnit::Pc => "pc",
            LengthUnit::Lh => "lh",
            LengthUnit::Rlh => "rlh",
        }
    }
}

/// A specified length: a number with a unit, resolved to `Au` against a context.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Length {
    pub value: Number,
    pub unit: LengthUnit,
}

impl Length {
    pub const ZERO: Length = Length { value: Number::ZERO, unit: LengthUnit::Px };
    pub fn px(v: i64) -> Length {
        Length { value: Number::from_i64(v), unit: LengthUnit::Px }
    }
    pub fn is_zero(&self) -> bool {
        self.value.is_zero()
    }
    pub fn is_negative(&self) -> bool {
        self.value.is_negative()
    }
    /// In millionths of a CSS pixel.
    pub fn to_micro_px(&self, ctx: &LengthContext) -> i128 {
        let v = self.value.micro as i128;
        let of_au = |au: Au| round_div(v * au.0 as i128, Au::PER_PX as i128);
        match self.unit {
            LengthUnit::Px => v,
            LengthUnit::In => v * 96,
            LengthUnit::Cm => round_div(v * 9600, 254),
            LengthUnit::Mm => round_div(v * 9600, 2540),
            LengthUnit::Q => round_div(v * 9600, 10160),
            LengthUnit::Pt => round_div(v * 4, 3),
            LengthUnit::Pc => v * 16,
            LengthUnit::Em => of_au(ctx.font_size),
            LengthUnit::Rem => of_au(ctx.root_font_size),
            LengthUnit::Ex => of_au(ctx.ex),
            LengthUnit::Ch => of_au(ctx.ch),
            LengthUnit::Lh => of_au(ctx.line_height),
            LengthUnit::Rlh => of_au(ctx.root_line_height),
            LengthUnit::Vw => round_div(of_au(ctx.viewport_width), 100),
            LengthUnit::Vh => round_div(of_au(ctx.viewport_height), 100),
            LengthUnit::Vmin => round_div(of_au(ctx.viewport_width.min(ctx.viewport_height)), 100),
            LengthUnit::Vmax => round_div(of_au(ctx.viewport_width.max(ctx.viewport_height)), 100),
        }
    }
    pub fn to_au(&self, ctx: &LengthContext) -> Au {
        micro_px_to_au(self.to_micro_px(ctx))
    }
}

/// What relative units resolve against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthContext {
    /// For `em`: the element's own font size, or the parent's when computing `font-size`.
    pub font_size: Au,
    pub root_font_size: Au,
    pub ex: Au,
    pub ch: Au,
    pub line_height: Au,
    pub root_line_height: Au,
    pub viewport_width: Au,
    pub viewport_height: Au,
}

impl LengthContext {
    /// A context for a given font size; `ex` and `ch` are half an em, the usual
    /// approximation when the face has no metrics, and `lh` is 1.2em.
    pub fn for_font_size(font_size: Au, root_font_size: Au, viewport: (Au, Au)) -> LengthContext {
        LengthContext {
            font_size,
            root_font_size,
            ex: font_size.scale(1, 2),
            ch: font_size.scale(1, 2),
            line_height: font_size.scale(12, 10),
            root_line_height: root_font_size.scale(12, 10),
            viewport_width: viewport.0,
            viewport_height: viewport.1,
        }
    }
}

impl Default for LengthContext {
    fn default() -> Self {
        LengthContext::for_font_size(Au::from_px_i32(16), Au::from_px_i32(16), (Au::from_px_i32(1280), Au::from_px_i32(800)))
    }
}

/// Parses `<length>`; unitless zero is accepted.
pub fn parse_length(p: &mut Parser) -> Option<Length> {
    p.try_parse(|p| match p.next()? {
        ComponentValue::Token(Token::Dimension { value, unit, .. }) => Some(Length { value: *value, unit: LengthUnit::parse(unit)? }),
        ComponentValue::Token(Token::Number { value, .. }) if value.is_zero() => Some(Length::ZERO),
        _ => None,
    })
}

// ---------------------------------------------------------------------------------
// calc()

/// A `calc()` expression tree, kept until computed-value time because relative units
/// resolve there.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CalcNode {
    Length(Length),
    Percent(Number),
    Number(Number),
    Sum(Vec<CalcNode>),
    Neg(Box<CalcNode>),
    Product(Vec<CalcNode>),
    /// Numerator over a number denominator.
    Div(Box<CalcNode>, Box<CalcNode>),
    Min(Vec<CalcNode>),
    Max(Vec<CalcNode>),
    Clamp(Box<CalcNode>, Box<CalcNode>, Box<CalcNode>),
}

/// The value of a calc expression: a length part, a percentage part, or a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalcValue {
    pub micro_px: i128,
    pub micro_pct: i128,
    pub micro_num: i128,
    pub has_len: bool,
    pub has_pct: bool,
    pub has_num: bool,
    /// Bounds from a `min()`, `max()` or `clamp()` that mixed lengths and
    /// percentages: (length, percentage) pairs the value is clamped below and above
    /// by once the percentage base is known.
    pub lo: Option<(i128, i128)>,
    pub hi: Option<(i128, i128)>,
}

impl CalcValue {
    fn zero() -> CalcValue {
        CalcValue { micro_px: 0, micro_pct: 0, micro_num: 0, has_len: false, has_pct: false, has_num: false, lo: None, hi: None }
    }
    fn bounded(&self) -> bool {
        self.lo.is_some() || self.hi.is_some()
    }
    pub fn is_number(&self) -> bool {
        self.has_num && !self.has_len && !self.has_pct
    }
    pub fn is_length_percentage(&self) -> bool {
        !self.has_num
    }
    fn add(a: CalcValue, b: CalcValue) -> Option<CalcValue> {
        if (a.has_num || b.has_num) && (a.has_len || a.has_pct || b.has_len || b.has_pct) {
            return None;
        }
        // A bounded value shifts its bounds by an unbounded addend; two bounded values
        // cannot be summed into one clamp.
        let (bounded, other) = match (a.bounded(), b.bounded()) {
            (true, true) => return None,
            (true, false) => (a, b),
            _ => (b, a),
        };
        let shift = |bound: Option<(i128, i128)>| bound.map(|(l, p)| (l + other.micro_px, p + other.micro_pct));
        Some(CalcValue {
            micro_px: a.micro_px + b.micro_px,
            micro_pct: a.micro_pct + b.micro_pct,
            micro_num: a.micro_num + b.micro_num,
            has_len: a.has_len || b.has_len,
            has_pct: a.has_pct || b.has_pct,
            has_num: a.has_num || b.has_num,
            lo: shift(bounded.lo),
            hi: shift(bounded.hi),
        })
    }
    fn neg(a: CalcValue) -> CalcValue {
        let flip = |b: Option<(i128, i128)>| b.map(|(l, p)| (-l, -p));
        CalcValue { micro_px: -a.micro_px, micro_pct: -a.micro_pct, micro_num: -a.micro_num, lo: flip(a.hi), hi: flip(a.lo), ..a }
    }
    fn mul(a: CalcValue, b: CalcValue) -> Option<CalcValue> {
        if a.is_number() {
            let scale = |bound: Option<(i128, i128)>| bound.map(|(l, p)| (round_div(l * a.micro_num, 1_000_000), round_div(p * a.micro_num, 1_000_000)));
            let (lo, hi) = if a.micro_num < 0 { (scale(b.hi), scale(b.lo)) } else { (scale(b.lo), scale(b.hi)) };
            Some(CalcValue {
                micro_px: round_div(b.micro_px * a.micro_num, 1_000_000),
                micro_pct: round_div(b.micro_pct * a.micro_num, 1_000_000),
                micro_num: round_div(b.micro_num * a.micro_num, 1_000_000),
                lo,
                hi,
                ..b
            })
        } else if b.is_number() {
            Self::mul(b, a)
        } else {
            None
        }
    }
    fn div(a: CalcValue, b: CalcValue) -> Option<CalcValue> {
        if !b.is_number() || b.micro_num == 0 {
            return None;
        }
        let scale = |bound: Option<(i128, i128)>| bound.map(|(l, p)| (round_div(l * 1_000_000, b.micro_num), round_div(p * 1_000_000, b.micro_num)));
        let (lo, hi) = if b.micro_num < 0 { (scale(a.hi), scale(a.lo)) } else { (scale(a.lo), scale(a.hi)) };
        Some(CalcValue {
            micro_px: round_div(a.micro_px * 1_000_000, b.micro_num),
            micro_pct: round_div(a.micro_pct * 1_000_000, b.micro_num),
            micro_num: round_div(a.micro_num * 1_000_000, b.micro_num),
            lo,
            hi,
            ..a
        })
    }
    /// Folds `min()`/`max()` operands that mix lengths and percentages: pure lengths
    /// and pure percentages collapse among themselves, and what remains (at most
    /// two operands) becomes a bounded value that compares once the base is known.
    fn fold_min_max(items: &[CalcValue], is_min: bool) -> Option<CalcValue> {
        if items.iter().any(|v| v.has_num || v.bounded()) {
            return None;
        }
        let mut rest: Vec<CalcValue> = Vec::new();
        for &v in items {
            match rest.iter_mut().find(|r| CalcValue::same_kind(r, &v) && r.key().is_some()) {
                Some(r) => {
                    let (kr, kv) = (r.key().unwrap(), v.key().unwrap());
                    if (is_min && kv < kr) || (!is_min && kv > kr) {
                        *r = v;
                    }
                }
                None => rest.push(v),
            }
        }
        match rest.as_slice() {
            [one] => Some(*one),
            [a, b] => {
                let bound = Some((b.micro_px, b.micro_pct));
                Some(CalcValue {
                    has_len: a.has_len || b.has_len,
                    has_pct: a.has_pct || b.has_pct,
                    lo: if is_min { None } else { bound },
                    hi: if is_min { bound } else { None },
                    ..*a
                })
            }
            _ => None,
        }
    }
    /// Comparable only when both are pure numbers, pure lengths or pure percentages.
    fn key(&self) -> Option<i128> {
        match (self.has_len, self.has_pct, self.has_num) {
            (true, false, false) => Some(self.micro_px),
            (false, true, false) => Some(self.micro_pct),
            (false, false, true) => Some(self.micro_num),
            (false, false, false) => Some(0),
            _ => None,
        }
    }
    /// Comparable operands: the same single kind (a pure length against a pure
    /// percentage cannot be ordered until the percentage base is known).
    fn same_kind(a: &CalcValue, b: &CalcValue) -> bool {
        (a.has_len, a.has_pct, a.has_num) == (b.has_len, b.has_pct, b.has_num)
    }
    pub fn to_length_percentage(&self) -> Option<crate::style::computed::LengthPercentage> {
        use crate::style::computed::LengthPercentage;
        if self.has_num {
            return None;
        }
        let pct = clamp_i32(round_div(self.micro_pct, 10_000));
        let len = micro_px_to_au(self.micro_px);
        if self.bounded() {
            let part = |(l, p): (i128, i128)| (micro_px_to_au(l), clamp_i32(round_div(p, 10_000)));
            return Some(LengthPercentage::Clamp { lo: self.lo.map(part), v: (len, pct), hi: self.hi.map(part) });
        }
        Some(match (self.has_len, self.has_pct) {
            (_, false) => LengthPercentage::Length(len),
            (false, true) => LengthPercentage::Percent(pct),
            (true, true) => LengthPercentage::Calc(len, pct),
        })
    }
}

impl CalcNode {
    pub fn eval(&self, ctx: &LengthContext) -> Option<CalcValue> {
        Some(match self {
            CalcNode::Length(l) => CalcValue { micro_px: l.to_micro_px(ctx), has_len: true, ..CalcValue::zero() },
            CalcNode::Percent(n) => CalcValue { micro_pct: n.micro as i128, has_pct: true, ..CalcValue::zero() },
            CalcNode::Number(n) => CalcValue { micro_num: n.micro as i128, has_num: true, ..CalcValue::zero() },
            CalcNode::Sum(items) => {
                let mut acc = items[0].eval(ctx)?;
                for i in &items[1..] {
                    acc = CalcValue::add(acc, i.eval(ctx)?)?;
                }
                acc
            }
            CalcNode::Neg(a) => CalcValue::neg(a.eval(ctx)?),
            CalcNode::Product(items) => {
                let mut acc = items[0].eval(ctx)?;
                for i in &items[1..] {
                    acc = CalcValue::mul(acc, i.eval(ctx)?)?;
                }
                acc
            }
            CalcNode::Div(a, b) => CalcValue::div(a.eval(ctx)?, b.eval(ctx)?)?,
            CalcNode::Min(items) | CalcNode::Max(items) => {
                let is_min = matches!(self, CalcNode::Min(_));
                let values = items.iter().map(|i| i.eval(ctx)).collect::<Option<Vec<_>>>()?;
                let same = values.iter().all(|v| CalcValue::same_kind(&values[0], v) && !v.bounded());
                if !same || values[0].key().is_none() {
                    return CalcValue::fold_min_max(&values, is_min);
                }
                let mut best = values[0];
                let mut best_key = best.key()?;
                for v in &values[1..] {
                    let k = v.key()?;
                    if (is_min && k < best_key) || (!is_min && k > best_key) {
                        best = *v;
                        best_key = k;
                    }
                }
                best
            }
            CalcNode::Clamp(lo, mid, hi) => {
                let (lo, mid, hi) = (lo.eval(ctx)?, mid.eval(ctx)?, hi.eval(ctx)?);
                let same = CalcValue::same_kind(&lo, &mid) && CalcValue::same_kind(&mid, &hi) && !lo.bounded() && !mid.bounded() && !hi.bounded();
                if !same || mid.key().is_none() {
                    // max(lo, min(mid, hi)) with the comparison deferred to the base.
                    if [lo, mid, hi].iter().any(|v| v.has_num || v.bounded()) {
                        return None;
                    }
                    return Some(CalcValue {
                        has_len: lo.has_len || mid.has_len || hi.has_len,
                        has_pct: lo.has_pct || mid.has_pct || hi.has_pct,
                        lo: Some((lo.micro_px, lo.micro_pct)),
                        hi: Some((hi.micro_px, hi.micro_pct)),
                        ..mid
                    });
                }
                let (kl, km, kh) = (lo.key()?, mid.key()?, hi.key()?);
                // max(lo, min(mid, hi))
                if km < kl {
                    lo
                } else if km > kh {
                    if kh < kl {
                        lo
                    } else {
                        hi
                    }
                } else {
                    mid
                }
            }
        })
    }
    /// True when no relative unit appears, so the value can be resolved at parse time.
    pub fn is_absolute(&self) -> bool {
        match self {
            CalcNode::Length(l) => l.unit.is_absolute(),
            CalcNode::Percent(_) | CalcNode::Number(_) => true,
            CalcNode::Sum(v) | CalcNode::Product(v) | CalcNode::Min(v) | CalcNode::Max(v) => v.iter().all(|n| n.is_absolute()),
            CalcNode::Neg(a) => a.is_absolute(),
            CalcNode::Div(a, b) => a.is_absolute() && b.is_absolute(),
            CalcNode::Clamp(a, b, c) => a.is_absolute() && b.is_absolute() && c.is_absolute(),
        }
    }
}

/// Parses a math function (`calc`, `min`, `max`, `clamp`) if the next item is one.
pub fn parse_math_function(p: &mut Parser) -> Option<CalcNode> {
    p.try_parse(|p| {
        let (name, args) = p.expect_function()?;
        let mut inner = Parser::new(args);
        match name.to_ascii_lowercase().as_str() {
            "calc" | "-webkit-calc" | "-moz-calc" => inner.parse_entirely(parse_calc_sum),
            "min" => Some(CalcNode::Min(inner.parse_entirely(|p| p.comma_list(parse_calc_sum))?)),
            "max" => Some(CalcNode::Max(inner.parse_entirely(|p| p.comma_list(parse_calc_sum))?)),
            "clamp" => {
                let items = inner.parse_entirely(|p| p.comma_list(parse_calc_sum))?;
                if items.len() != 3 {
                    return None;
                }
                let mut it = items.into_iter();
                let (a, b, c) = (it.next()?, it.next()?, it.next()?);
                Some(CalcNode::Clamp(Box::new(a), Box::new(b), Box::new(c)))
            }
            _ => None,
        }
    })
}

fn parse_calc_sum(p: &mut Parser) -> Option<CalcNode> {
    let mut items = vec![parse_calc_product(p)?];
    loop {
        let start = p.position();
        match p.next() {
            Some(ComponentValue::Token(Token::Delim('+'))) => items.push(parse_calc_product(p)?),
            Some(ComponentValue::Token(Token::Delim('-'))) => items.push(CalcNode::Neg(Box::new(parse_calc_product(p)?))),
            Some(_) => {
                p.reset(start);
                break;
            }
            None => break,
        }
    }
    Some(if items.len() == 1 { items.pop().unwrap() } else { CalcNode::Sum(items) })
}

fn parse_calc_product(p: &mut Parser) -> Option<CalcNode> {
    let mut items = vec![parse_calc_value(p)?];
    loop {
        let start = p.position();
        match p.next() {
            Some(ComponentValue::Token(Token::Delim('*'))) => items.push(parse_calc_value(p)?),
            Some(ComponentValue::Token(Token::Delim('/'))) => {
                let d = parse_calc_value(p)?;
                let n = if items.len() == 1 { items.pop().unwrap() } else { CalcNode::Product(std::mem::take(&mut items)) };
                items.push(CalcNode::Div(Box::new(n), Box::new(d)));
            }
            _ => {
                p.reset(start);
                break;
            }
        }
    }
    Some(if items.len() == 1 { items.pop().unwrap() } else { CalcNode::Product(items) })
}

fn parse_calc_value(p: &mut Parser) -> Option<CalcNode> {
    if let Some(n) = parse_math_function(p) {
        return Some(n);
    }
    let start = p.position();
    match p.next()? {
        ComponentValue::Token(Token::Number { value, .. }) => Some(CalcNode::Number(*value)),
        ComponentValue::Token(Token::Percentage { value, .. }) => Some(CalcNode::Percent(*value)),
        ComponentValue::Token(Token::Dimension { value, unit, .. }) => {
            Some(CalcNode::Length(Length { value: *value, unit: LengthUnit::parse(unit)? }))
        }
        ComponentValue::Token(Token::Ident(s)) if s.eq_ignore_ascii_case("e") => Some(CalcNode::Number(Number { micro: 2_718_282, int: false })),
        ComponentValue::Token(Token::Ident(s)) if s.eq_ignore_ascii_case("pi") => Some(CalcNode::Number(Number { micro: 3_141_593, int: false })),
        ComponentValue::Block { open: Token::OpenParen, contents } => Parser::new(contents).parse_entirely(parse_calc_sum),
        _ => {
            p.reset(start);
            None
        }
    }
}

// ---------------------------------------------------------------------------------
// Length-percentage in specified form

/// `<length-percentage>` before computation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LpSpec {
    Length(Length),
    /// Millionths of a percent unit (50% is 50_000_000).
    Percent(Number),
    Calc(Box<CalcNode>),
}

impl LpSpec {
    pub const ZERO: LpSpec = LpSpec::Length(Length::ZERO);
    pub fn px(v: i64) -> LpSpec {
        LpSpec::Length(Length::px(v))
    }
    pub fn compute(&self, ctx: &LengthContext) -> Option<crate::style::computed::LengthPercentage> {
        use crate::style::computed::LengthPercentage;
        match self {
            LpSpec::Length(l) => Some(LengthPercentage::Length(l.to_au(ctx))),
            LpSpec::Percent(n) => Some(LengthPercentage::Percent(micro_to_myriad(n.micro))),
            LpSpec::Calc(c) => c.eval(ctx)?.to_length_percentage(),
        }
    }
    /// For properties that take only a length: percentages are rejected.
    pub fn compute_length(&self, ctx: &LengthContext) -> Option<Au> {
        match self.compute(ctx)? {
            crate::style::computed::LengthPercentage::Length(l) => Some(l),
            _ => None,
        }
    }
    pub fn is_negative(&self) -> bool {
        match self {
            LpSpec::Length(l) => l.is_negative(),
            LpSpec::Percent(n) => n.is_negative(),
            LpSpec::Calc(_) => false,
        }
    }
}

/// Which of number / percentage / length a parser accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allow {
    pub negative: bool,
    pub percent: bool,
}

impl Allow {
    pub const ALL: Allow = Allow { negative: true, percent: true };
    pub const NON_NEGATIVE: Allow = Allow { negative: false, percent: true };
    pub const LENGTH: Allow = Allow { negative: true, percent: false };
    pub const LENGTH_NON_NEGATIVE: Allow = Allow { negative: false, percent: false };
}

/// `<length-percentage>` including math functions.
pub fn parse_lp(p: &mut Parser, allow: Allow) -> Option<LpSpec> {
    p.try_parse(|p| {
        if let Some(c) = parse_math_function(p) {
            // Type check with a default context; relative units only change magnitude.
            let v = c.eval(&LengthContext::default())?;
            if v.has_num || (!allow.percent && v.has_pct) {
                return None;
            }
            // A calc() may be negative even for non-negative properties (clamped later).
            return Some(LpSpec::Calc(Box::new(c)));
        }
        let v = match p.next()? {
            ComponentValue::Token(Token::Dimension { value, unit, .. }) => LpSpec::Length(Length { value: *value, unit: LengthUnit::parse(unit)? }),
            ComponentValue::Token(Token::Number { value, .. }) if value.is_zero() => LpSpec::ZERO,
            ComponentValue::Token(Token::Percentage { value, .. }) if allow.percent => LpSpec::Percent(*value),
            _ => return None,
        };
        if !allow.negative && v.is_negative() {
            return None;
        }
        Some(v)
    })
}

/// `<length>` including math functions (no percentages).
pub fn parse_length_spec(p: &mut Parser, non_negative: bool) -> Option<LpSpec> {
    parse_lp(p, if non_negative { Allow::LENGTH_NON_NEGATIVE } else { Allow::LENGTH })
}

/// A `<number>` or a numeric math function, in millionths.
pub fn parse_number_spec(p: &mut Parser) -> Option<Number> {
    p.try_parse(|p| {
        if let Some(c) = parse_math_function(p) {
            let v = c.eval(&LengthContext::default())?;
            if !v.is_number() {
                return None;
            }
            return Some(Number { micro: v.micro_num.clamp(i64::MIN as i128, i64::MAX as i128) as i64, int: false });
        }
        p.expect_number()
    })
}

/// `<integer>`, allowing `calc()` that yields an integer-valued number.
pub fn parse_integer_spec(p: &mut Parser) -> Option<i32> {
    p.try_parse(|p| {
        if let Some(n) = p.expect_integer() {
            return Some(n);
        }
        let n = parse_number_spec(p)?;
        Some(n.round())
    })
}

/// `<number> | <percentage>` as a fraction in millionths (100% is 1_000_000).
pub fn parse_number_or_percent_fraction(p: &mut Parser) -> Option<i64> {
    p.try_parse(|p| {
        if let Some(n) = p.expect_percentage() {
            return Some(n.micro / 100);
        }
        parse_number_spec(p).map(|n| n.micro)
    })
}

// ---------------------------------------------------------------------------------
// Angles and times

/// Parses `<angle>` to centi-degrees; unitless zero accepted when `zero_ok`.
pub fn parse_angle(p: &mut Parser, zero_ok: bool) -> Option<i32> {
    p.try_parse(|p| match p.next()? {
        ComponentValue::Token(Token::Dimension { value, unit, .. }) => angle_to_centi(*value, unit),
        ComponentValue::Token(Token::Number { value, .. }) if zero_ok && value.is_zero() => Some(0),
        _ => None,
    })
}

pub fn angle_to_centi(value: Number, unit: &str) -> Option<i32> {
    let v = value.micro as i128;
    let centi = match unit.to_ascii_lowercase().as_str() {
        "deg" => round_div(v, 10_000),
        "grad" => round_div(v * 9, 100_000),
        "rad" => round_div(v * 18_000 * 1_000_000_000_000_000, 3_141_592_653_589_793 * 1_000_000),
        "turn" => round_div(v * 36_000, 1_000_000),
        _ => return None,
    };
    Some(clamp_i32(centi))
}

/// Parses `<time>` to milliseconds.
pub fn parse_time(p: &mut Parser) -> Option<i32> {
    p.try_parse(|p| match p.next()? {
        ComponentValue::Token(Token::Dimension { value, unit, .. }) => {
            let v = value.micro as i128;
            let ms = match unit.to_ascii_lowercase().as_str() {
                "s" => round_div(v, 1000),
                "ms" => round_div(v, 1_000_000),
                _ => return None,
            };
            Some(clamp_i32(ms))
        }
        _ => None,
    })
}

// ---------------------------------------------------------------------------------
// Colours

/// A specified colour: resolved except for `currentcolor`, which computes to the
/// element's `color`, and mixes that involve it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColorSpec {
    Rgba(Color),
    CurrentColor,
    /// `color-mix(in srgb, a pa%, b pb%)` with the percentages already normalised to
    /// myriads summing to 10_000 (and the alpha multiplier from a sum below 100%).
    Mix { a: Box<ColorSpec>, b: Box<ColorSpec>, pa: i32, pb: i32, alpha_mul: i32 },
}

impl ColorSpec {
    pub fn resolve(&self, current: Color) -> Color {
        match self {
            ColorSpec::Rgba(c) => *c,
            ColorSpec::CurrentColor => current,
            ColorSpec::Mix { a, b, pa, pb, alpha_mul } => mix_srgb(a.resolve(current), b.resolve(current), *pa, *pb, *alpha_mul),
        }
    }
    pub fn is_current(&self) -> bool {
        matches!(self, ColorSpec::CurrentColor)
    }
}

/// Mixes two colours in sRGB with premultiplied alpha, per CSS Color 5.
pub fn mix_srgb(a: Color, b: Color, pa: i32, pb: i32, alpha_mul: i32) -> Color {
    let (pa, pb) = (pa as i128, pb as i128);
    let aa = a.3 as i128;
    let ab = b.3 as i128;
    let alpha = round_div(aa * pa + ab * pb, 10_000);
    let ch = |ca: u8, cb: u8| -> u8 {
        if alpha == 0 {
            return 0;
        }
        let pre = ca as i128 * aa * pa + cb as i128 * ab * pb;
        round_div(pre, alpha * 10_000).clamp(0, 255) as u8
    };
    let out_alpha = round_div(alpha * alpha_mul as i128, 10_000).clamp(0, 255) as u8;
    Color(ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2), out_alpha)
}

/// Parses `<color>`.
pub fn parse_color(p: &mut Parser) -> Option<ColorSpec> {
    p.try_parse(|p| match p.next()? {
        ComponentValue::Token(Token::Hash { value, .. }) => parse_hex_color(value).map(ColorSpec::Rgba),
        ComponentValue::Token(Token::Ident(s)) => {
            let l = s.to_ascii_lowercase();
            match l.as_str() {
                "currentcolor" => Some(ColorSpec::CurrentColor),
                "transparent" => Some(ColorSpec::Rgba(Color::TRANSPARENT)),
                _ => named_color(&l).map(ColorSpec::Rgba),
            }
        }
        ComponentValue::Function { name, args } => parse_color_function(&name.to_ascii_lowercase(), args),
        _ => None,
    })
}

pub fn parse_hex_color(s: &str) -> Option<Color> {
    let digits: Vec<u8> = s.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    Some(match digits.len() {
        3 => Color(digits[0] * 17, digits[1] * 17, digits[2] * 17, 255),
        4 => Color(digits[0] * 17, digits[1] * 17, digits[2] * 17, digits[3] * 17),
        6 => Color(digits[0] * 16 + digits[1], digits[2] * 16 + digits[3], digits[4] * 16 + digits[5], 255),
        8 => Color(digits[0] * 16 + digits[1], digits[2] * 16 + digits[3], digits[4] * 16 + digits[5], digits[6] * 16 + digits[7]),
        _ => return None,
    })
}

fn parse_color_function(name: &str, args: &[ComponentValue]) -> Option<ColorSpec> {
    let mut p = Parser::new(args);
    match name {
        "rgb" | "rgba" => p.parse_entirely(parse_rgb_args).map(ColorSpec::Rgba),
        "hsl" | "hsla" => p.parse_entirely(parse_hsl_args).map(ColorSpec::Rgba),
        "hwb" => p.parse_entirely(parse_hwb_args).map(ColorSpec::Rgba),
        "color-mix" => p.parse_entirely(parse_color_mix_args),
        _ => None,
    }
}

/// A channel given as a number (0..255) or percentage, to 0..=255.
fn channel_255(p: &mut Parser) -> Option<u8> {
    p.try_parse(|p| {
        if p.expect_ident_matching("none").is_some() {
            return Some(0);
        }
        if let Some(n) = p.expect_percentage() {
            return Some(clamp_i32(round_div(n.micro as i128 * 255, 100_000_000)).clamp(0, 255) as u8);
        }
        let n = parse_number_spec(p)?;
        Some(clamp_i32(round_div(n.micro as i128, 1_000_000)).clamp(0, 255) as u8)
    })
}

/// Alpha as a number 0..1 or percentage, to 0..=255.
fn alpha_255(p: &mut Parser) -> Option<u8> {
    p.try_parse(|p| {
        if p.expect_ident_matching("none").is_some() {
            return Some(0);
        }
        let f = parse_number_or_percent_fraction(p)?;
        Some(fraction_to_255(f))
    })
}

pub fn fraction_to_255(micro: i64) -> u8 {
    clamp_i32(round_div(micro as i128 * 255, 1_000_000)).clamp(0, 255) as u8
}

fn parse_rgb_args(p: &mut Parser) -> Option<Color> {
    let r = channel_255(p)?;
    if p.expect_comma().is_some() {
        // Legacy syntax: all commas, alpha after a comma.
        let g = channel_255(p)?;
        p.expect_comma()?;
        let b = channel_255(p)?;
        let a = if p.expect_comma().is_some() { alpha_255(p)? } else { 255 };
        return Some(Color(r, g, b, a));
    }
    let g = channel_255(p)?;
    let b = channel_255(p)?;
    let a = if p.expect_delim('/').is_some() { alpha_255(p)? } else { 255 };
    Some(Color(r, g, b, a))
}

/// Hue in micro-degrees, normalised to [0, 360e6).
fn parse_hue(p: &mut Parser) -> Option<i128> {
    p.try_parse(|p| {
        if p.expect_ident_matching("none").is_some() {
            return Some(0);
        }
        let micro_deg: i128 = match p.next()? {
            ComponentValue::Token(Token::Number { value, .. }) => value.micro as i128,
            ComponentValue::Token(Token::Dimension { value, unit, .. }) => angle_to_centi(*value, unit)? as i128 * 10_000,
            _ => return None,
        };
        Some(micro_deg.rem_euclid(360_000_000))
    })
}

/// A percentage or (modern syntax) number in 0..100, as a fraction in millionths.
fn percent_fraction(p: &mut Parser) -> Option<i128> {
    p.try_parse(|p| {
        if p.expect_ident_matching("none").is_some() {
            return Some(0);
        }
        if let Some(n) = p.expect_percentage() {
            return Some((n.micro as i128 / 100).clamp(0, 1_000_000));
        }
        let n = parse_number_spec(p)?;
        Some((n.micro as i128 / 100).clamp(0, 1_000_000))
    })
}

fn parse_hsl_args(p: &mut Parser) -> Option<Color> {
    let h = parse_hue(p)?;
    let legacy = p.expect_comma().is_some();
    let s = percent_fraction(p)?;
    if legacy {
        p.expect_comma()?;
    }
    let l = percent_fraction(p)?;
    let a = if legacy {
        if p.expect_comma().is_some() {
            alpha_255(p)?
        } else {
            255
        }
    } else if p.expect_delim('/').is_some() {
        alpha_255(p)?
    } else {
        255
    };
    let (r, g, b) = hsl_to_rgb(h, s, l);
    Some(Color(r, g, b, a))
}

/// `h` in micro-degrees, `s` and `l` fractions in millionths.
pub fn hsl_to_rgb(h: i128, s: i128, l: i128) -> (u8, u8, u8) {
    let a = round_div(s * l.min(1_000_000 - l), 1_000_000);
    let f = |n: i128| -> u8 {
        let k12 = (n * 30_000_000 + h).rem_euclid(360_000_000) / 30; // k * 1e6
        let m = (k12 - 3_000_000).min(9_000_000 - k12).clamp(-1_000_000, 1_000_000);
        let v = l - round_div(a * m, 1_000_000);
        round_div(v * 255, 1_000_000).clamp(0, 255) as u8
    };
    (f(0), f(8), f(4))
}

fn parse_hwb_args(p: &mut Parser) -> Option<Color> {
    let h = parse_hue(p)?;
    let w = percent_fraction(p)?;
    let b = percent_fraction(p)?;
    let a = if p.expect_delim('/').is_some() { alpha_255(p)? } else { 255 };
    let (r, g, bl) = hwb_to_rgb(h, w, b);
    Some(Color(r, g, bl, a))
}

pub fn hwb_to_rgb(h: i128, w: i128, b: i128) -> (u8, u8, u8) {
    if w + b >= 1_000_000 {
        let gray = round_div(w * 255, w + b).clamp(0, 255) as u8;
        return (gray, gray, gray);
    }
    let (r, g, bl) = hsl_to_rgb(h, 1_000_000, 500_000);
    let f = |c: u8| -> u8 {
        let v = round_div(c as i128 * (1_000_000 - w - b), 1_000_000) + round_div(w * 255, 1_000_000);
        v.clamp(0, 255) as u8
    };
    (f(r), f(g), f(bl))
}

fn parse_color_mix_args(p: &mut Parser) -> Option<ColorSpec> {
    p.expect_ident_matching("in")?;
    let space = p.expect_ident_lower()?;
    if space != "srgb" {
        return None;
    }
    p.expect_comma()?;
    let component = |p: &mut Parser| -> Option<(ColorSpec, Option<i128>)> {
        let mut pct = p.expect_percentage().map(|n| n.micro as i128);
        let c = parse_color(p)?;
        if pct.is_none() {
            pct = p.expect_percentage().map(|n| n.micro as i128);
        }
        Some((c, pct))
    };
    let (a, pa) = component(p)?;
    p.expect_comma()?;
    let (b, pb) = component(p)?;
    // Percentages are in millionths of a percent unit: 50% is 50_000_000.
    let (pa, pb) = match (pa, pb) {
        (None, None) => (50_000_000, 50_000_000),
        (Some(x), None) => (x, 100_000_000 - x),
        (None, Some(y)) => (100_000_000 - y, y),
        (Some(x), Some(y)) => (x, y),
    };
    if pa < 0 || pb < 0 || pa + pb == 0 {
        return None;
    }
    let sum = pa + pb;
    let alpha_mul = if sum < 100_000_000 { round_div(sum, 10_000) } else { 10_000 };
    let pa_m = round_div(pa * 10_000, sum);
    let pb_m = 10_000 - pa_m;
    Some(ColorSpec::Mix { a: Box::new(a), b: Box::new(b), pa: pa_m as i32, pb: pb_m as i32, alpha_mul: alpha_mul as i32 })
}

/// The CSS named colours (Color Level 4 §6.1), lower-case names.
pub const NAMED_COLORS: &[(&str, [u8; 3])] = &[
    ("aliceblue", [240, 248, 255]),
    ("antiquewhite", [250, 235, 215]),
    ("aqua", [0, 255, 255]),
    ("aquamarine", [127, 255, 212]),
    ("azure", [240, 255, 255]),
    ("beige", [245, 245, 220]),
    ("bisque", [255, 228, 196]),
    ("black", [0, 0, 0]),
    ("blanchedalmond", [255, 235, 205]),
    ("blue", [0, 0, 255]),
    ("blueviolet", [138, 43, 226]),
    ("brown", [165, 42, 42]),
    ("burlywood", [222, 184, 135]),
    ("cadetblue", [95, 158, 160]),
    ("chartreuse", [127, 255, 0]),
    ("chocolate", [210, 105, 30]),
    ("coral", [255, 127, 80]),
    ("cornflowerblue", [100, 149, 237]),
    ("cornsilk", [255, 248, 220]),
    ("crimson", [220, 20, 60]),
    ("cyan", [0, 255, 255]),
    ("darkblue", [0, 0, 139]),
    ("darkcyan", [0, 139, 139]),
    ("darkgoldenrod", [184, 134, 11]),
    ("darkgray", [169, 169, 169]),
    ("darkgreen", [0, 100, 0]),
    ("darkgrey", [169, 169, 169]),
    ("darkkhaki", [189, 183, 107]),
    ("darkmagenta", [139, 0, 139]),
    ("darkolivegreen", [85, 107, 47]),
    ("darkorange", [255, 140, 0]),
    ("darkorchid", [153, 50, 204]),
    ("darkred", [139, 0, 0]),
    ("darksalmon", [233, 150, 122]),
    ("darkseagreen", [143, 188, 143]),
    ("darkslateblue", [72, 61, 139]),
    ("darkslategray", [47, 79, 79]),
    ("darkslategrey", [47, 79, 79]),
    ("darkturquoise", [0, 206, 209]),
    ("darkviolet", [148, 0, 211]),
    ("deeppink", [255, 20, 147]),
    ("deepskyblue", [0, 191, 255]),
    ("dimgray", [105, 105, 105]),
    ("dimgrey", [105, 105, 105]),
    ("dodgerblue", [30, 144, 255]),
    ("firebrick", [178, 34, 34]),
    ("floralwhite", [255, 250, 240]),
    ("forestgreen", [34, 139, 34]),
    ("fuchsia", [255, 0, 255]),
    ("gainsboro", [220, 220, 220]),
    ("ghostwhite", [248, 248, 255]),
    ("gold", [255, 215, 0]),
    ("goldenrod", [218, 165, 32]),
    ("gray", [128, 128, 128]),
    ("green", [0, 128, 0]),
    ("greenyellow", [173, 255, 47]),
    ("grey", [128, 128, 128]),
    ("honeydew", [240, 255, 240]),
    ("hotpink", [255, 105, 180]),
    ("indianred", [205, 92, 92]),
    ("indigo", [75, 0, 130]),
    ("ivory", [255, 255, 240]),
    ("khaki", [240, 230, 140]),
    ("lavender", [230, 230, 250]),
    ("lavenderblush", [255, 240, 245]),
    ("lawngreen", [124, 252, 0]),
    ("lemonchiffon", [255, 250, 205]),
    ("lightblue", [173, 216, 230]),
    ("lightcoral", [240, 128, 128]),
    ("lightcyan", [224, 255, 255]),
    ("lightgoldenrodyellow", [250, 250, 210]),
    ("lightgray", [211, 211, 211]),
    ("lightgreen", [144, 238, 144]),
    ("lightgrey", [211, 211, 211]),
    ("lightpink", [255, 182, 193]),
    ("lightsalmon", [255, 160, 122]),
    ("lightseagreen", [32, 178, 170]),
    ("lightskyblue", [135, 206, 250]),
    ("lightslategray", [119, 136, 153]),
    ("lightslategrey", [119, 136, 153]),
    ("lightsteelblue", [176, 196, 222]),
    ("lightyellow", [255, 255, 224]),
    ("lime", [0, 255, 0]),
    ("limegreen", [50, 205, 50]),
    ("linen", [250, 240, 230]),
    ("magenta", [255, 0, 255]),
    ("maroon", [128, 0, 0]),
    ("mediumaquamarine", [102, 205, 170]),
    ("mediumblue", [0, 0, 205]),
    ("mediumorchid", [186, 85, 211]),
    ("mediumpurple", [147, 112, 219]),
    ("mediumseagreen", [60, 179, 113]),
    ("mediumslateblue", [123, 104, 238]),
    ("mediumspringgreen", [0, 250, 154]),
    ("mediumturquoise", [72, 209, 204]),
    ("mediumvioletred", [199, 21, 133]),
    ("midnightblue", [25, 25, 112]),
    ("mintcream", [245, 255, 250]),
    ("mistyrose", [255, 228, 225]),
    ("moccasin", [255, 228, 181]),
    ("navajowhite", [255, 222, 173]),
    ("navy", [0, 0, 128]),
    ("oldlace", [253, 245, 230]),
    ("olive", [128, 128, 0]),
    ("olivedrab", [107, 142, 35]),
    ("orange", [255, 165, 0]),
    ("orangered", [255, 69, 0]),
    ("orchid", [218, 112, 214]),
    ("palegoldenrod", [238, 232, 170]),
    ("palegreen", [152, 251, 152]),
    ("paleturquoise", [175, 238, 238]),
    ("palevioletred", [219, 112, 147]),
    ("papayawhip", [255, 239, 213]),
    ("peachpuff", [255, 218, 185]),
    ("peru", [205, 133, 63]),
    ("pink", [255, 192, 203]),
    ("plum", [221, 160, 221]),
    ("powderblue", [176, 224, 230]),
    ("purple", [128, 0, 128]),
    ("rebeccapurple", [102, 51, 153]),
    ("red", [255, 0, 0]),
    ("rosybrown", [188, 143, 143]),
    ("royalblue", [65, 105, 225]),
    ("saddlebrown", [139, 69, 19]),
    ("salmon", [250, 128, 114]),
    ("sandybrown", [244, 164, 96]),
    ("seagreen", [46, 139, 87]),
    ("seashell", [255, 245, 238]),
    ("sienna", [160, 82, 45]),
    ("silver", [192, 192, 192]),
    ("skyblue", [135, 206, 235]),
    ("slateblue", [106, 90, 205]),
    ("slategray", [112, 128, 144]),
    ("slategrey", [112, 128, 144]),
    ("snow", [255, 250, 250]),
    ("springgreen", [0, 255, 127]),
    ("steelblue", [70, 130, 180]),
    ("tan", [210, 180, 140]),
    ("teal", [0, 128, 128]),
    ("thistle", [216, 191, 216]),
    ("tomato", [255, 99, 71]),
    ("turquoise", [64, 224, 208]),
    ("violet", [238, 130, 238]),
    ("wheat", [245, 222, 179]),
    ("white", [255, 255, 255]),
    ("whitesmoke", [245, 245, 245]),
    ("yellow", [255, 255, 0]),
    ("yellowgreen", [154, 205, 50]),
];

/// A named colour by lower-case name (plus the system colour keywords pages use).
pub fn named_color(name: &str) -> Option<Color> {
    if let Ok(i) = NAMED_COLORS.binary_search_by_key(&name, |e| e.0) {
        let [r, g, b] = NAMED_COLORS[i].1;
        return Some(Color(r, g, b, 255));
    }
    // CSS system colours, as a light theme renders them.
    Some(match name {
        "canvas" | "window" | "field" | "buttonface" | "menu" | "infobackground" | "threedface" => Color(255, 255, 255, 255),
        "canvastext" | "windowtext" | "fieldtext" | "buttontext" | "menutext" | "infotext" | "captiontext" | "activecaption" => Color(0, 0, 0, 255),
        "linktext" | "activetext" => Color(0, 0, 238, 255),
        "visitedtext" => Color(85, 26, 139, 255),
        "highlight" => Color(0, 120, 215, 255),
        "highlighttext" => Color(255, 255, 255, 255),
        "graytext" | "inactivecaptiontext" => Color(109, 109, 109, 255),
        "buttonborder" | "activeborder" | "inactiveborder" | "threedshadow" | "threedhighlight" | "threeddarkshadow" | "threedlightshadow" => {
            Color(118, 118, 118, 255)
        }
        "mark" => Color(255, 255, 0, 255),
        "marktext" => Color(0, 0, 0, 255),
        "selecteditem" => Color(0, 120, 215, 255),
        "selecteditemtext" => Color(255, 255, 255, 255),
        "accentcolor" => Color(0, 120, 215, 255),
        "accentcolortext" => Color(255, 255, 255, 255),
        _ => return None,
    })
}

/// The name of a colour if it is one of the named colours (for `getComputedStyle`
/// this is never used; colours serialise as `rgb()`), kept for hint parsing.
pub fn color_name(c: Color) -> Option<&'static str> {
    if c.3 != 255 {
        return None;
    }
    NAMED_COLORS.iter().find(|(_, v)| *v == [c.0, c.1, c.2]).map(|e| e.0)
}

/// The HTML "rules for parsing a legacy colour value" (`bgcolor="#ff0"`, `"red"`,
/// `"ff0000"`, and the padding algorithm for anything else).
pub fn parse_legacy_color(input: &str) -> Option<Color> {
    let s = input.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\u{0C}');
    if s.is_empty() {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    if lower == "transparent" {
        return None;
    }
    if let Some(c) = NAMED_COLORS.binary_search_by_key(&lower.as_str(), |e| e.0).ok().map(|i| NAMED_COLORS[i].1) {
        return Some(Color(c[0], c[1], c[2], 255));
    }
    if s.len() == 4 && s.starts_with('#') {
        if let Some(c) = parse_hex_color(&s[1..]) {
            return Some(c);
        }
    }
    // Replace non-ASCII and everything outside hex with '0'.
    let mut chars: Vec<char> = s.chars().map(|c| if c as u32 > 0xFFFF { '0' } else { c }).collect();
    if chars.len() > 128 {
        chars.truncate(128);
    }
    if chars[0] == '#' {
        chars.remove(0);
    }
    if chars.is_empty() {
        chars.push('0');
    }
    for c in chars.iter_mut() {
        if !c.is_ascii_hexdigit() {
            *c = '0';
        }
    }
    while chars.is_empty() || !chars.len().is_multiple_of(3) {
        chars.push('0');
    }
    let mut len = chars.len() / 3;
    let mut parts: Vec<Vec<char>> = chars.chunks(len).map(|c| c.to_vec()).collect();
    if len > 8 {
        for part in parts.iter_mut() {
            let drop = len - 8;
            part.drain(0..drop);
        }
        len = 8;
    }
    while len > 2 && parts.iter().all(|p| p[0] == '0') {
        for part in parts.iter_mut() {
            part.remove(0);
        }
        len -= 1;
    }
    if len > 2 {
        for part in parts.iter_mut() {
            part.truncate(2);
        }
    }
    let hex = |p: &[char]| -> u8 {
        let s: String = p.iter().collect();
        u8::from_str_radix(&s, 16).unwrap_or(0)
    };
    Some(Color(hex(&parts[0]), hex(&parts[1]), hex(&parts[2]), 255))
}

// ---------------------------------------------------------------------------------
// Images

/// The direction a linear gradient runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GradientDirection {
    /// Centi-degrees clockwise from up.
    Angle(i32),
    /// `to <side-or-corner>`: horizontal (-1 left, 0 none, 1 right) and vertical
    /// (-1 top, 0 none, 1 bottom).
    Side(i8, i8),
}

impl GradientDirection {
    /// Corners resolve to the diagonal angle of a square box; layout may refine.
    pub fn to_centi_degrees(self) -> i32 {
        match self {
            GradientDirection::Angle(a) => a,
            GradientDirection::Side(h, v) => match (h, v) {
                (0, -1) => 0,
                (1, 0) => 9000,
                (0, 1) => 18000,
                (-1, 0) => 27000,
                (1, -1) => 4500,
                (1, 1) => 13500,
                (-1, 1) => 22500,
                (-1, -1) => 31500,
                _ => 18000,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StopSpec {
    /// `None` for a colour hint (a bare position).
    pub color: Option<ColorSpec>,
    pub position: Option<LpSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ImageSpec {
    None,
    Url(String),
    Linear { direction: GradientDirection, stops: Vec<StopSpec>, repeating: bool },
    Radial { circle: bool, stops: Vec<StopSpec>, repeating: bool },
}

/// `<image> | none`.
pub fn parse_image(p: &mut Parser) -> Option<ImageSpec> {
    p.try_parse(|p| {
        if p.expect_ident_matching("none").is_some() {
            return Some(ImageSpec::None);
        }
        if let Some(u) = p.expect_url() {
            return Some(ImageSpec::Url(u));
        }
        let (name, args) = p.expect_function()?;
        let lname = name.to_ascii_lowercase();
        let lname = lname.strip_prefix("-webkit-").unwrap_or(&lname).to_owned();
        let mut inner = Parser::new(args);
        match lname.as_str() {
            "linear-gradient" | "repeating-linear-gradient" => inner.parse_entirely(|p| parse_linear_gradient(p, lname.starts_with("repeating"))),
            "radial-gradient" | "repeating-radial-gradient" => inner.parse_entirely(|p| parse_radial_gradient(p, lname.starts_with("repeating"))),
            "image-set" => {
                // Take the first image of the set.
                let first = inner.try_parse(|p| {
                    if let Some(s) = p.expect_string() {
                        return Some(ImageSpec::Url(s.to_owned()));
                    }
                    parse_image(p)
                })?;
                Some(first)
            }
            _ => None,
        }
    })
}

fn parse_linear_gradient(p: &mut Parser, repeating: bool) -> Option<ImageSpec> {
    let mut direction = GradientDirection::Side(0, 1);
    let explicit = p.try_parse(|p| {
        if let Some(a) = parse_angle(p, true) {
            p.expect_comma()?;
            return Some(GradientDirection::Angle(a));
        }
        p.expect_ident_matching("to")?;
        let (mut h, mut v) = (0i8, 0i8);
        for _ in 0..2 {
            match p.peek_ident_lower().as_deref() {
                Some("left") if h == 0 => {
                    h = -1;
                    p.next();
                }
                Some("right") if h == 0 => {
                    h = 1;
                    p.next();
                }
                Some("top") if v == 0 => {
                    v = -1;
                    p.next();
                }
                Some("bottom") if v == 0 => {
                    v = 1;
                    p.next();
                }
                _ => break,
            }
        }
        if h == 0 && v == 0 {
            return None;
        }
        p.expect_comma()?;
        Some(GradientDirection::Side(h, v))
    });
    if let Some(d) = explicit {
        direction = d;
    }
    let stops = parse_color_stops(p)?;
    Some(ImageSpec::Linear { direction, stops, repeating })
}

fn parse_radial_gradient(p: &mut Parser, repeating: bool) -> Option<ImageSpec> {
    // Optional shape/size/position prelude, accepted and mostly ignored.
    let mut circle = false;
    let prelude = p.try_parse(|p| {
        let mut any = false;
        loop {
            if let Some(kw) = p.peek_ident_lower() {
                match kw.as_str() {
                    "circle" => {
                        circle = true;
                        p.next();
                        any = true;
                        continue;
                    }
                    "ellipse" | "closest-side" | "closest-corner" | "farthest-side" | "farthest-corner" => {
                        p.next();
                        any = true;
                        continue;
                    }
                    "at" => {
                        p.next();
                        parse_position(p)?;
                        any = true;
                        continue;
                    }
                    _ => break,
                }
            }
            if parse_lp(p, Allow::NON_NEGATIVE).is_some() {
                any = true;
                continue;
            }
            break;
        }
        if any {
            p.expect_comma()?;
            Some(())
        } else {
            None
        }
    });
    if prelude.is_none() {
        circle = false;
    }
    let stops = parse_color_stops(p)?;
    Some(ImageSpec::Radial { circle, stops, repeating })
}

fn parse_color_stops(p: &mut Parser) -> Option<Vec<StopSpec>> {
    let mut stops = Vec::new();
    loop {
        if let Some(c) = parse_color(p) {
            let pos = parse_lp(p, Allow::ALL);
            stops.push(StopSpec { color: Some(c.clone()), position: pos });
            // A second position makes a second stop with the same colour.
            if let Some(pos2) = parse_lp(p, Allow::ALL) {
                stops.push(StopSpec { color: Some(c), position: Some(pos2) });
            }
        } else {
            let hint = parse_lp(p, Allow::ALL)?;
            if stops.is_empty() {
                return None;
            }
            stops.push(StopSpec { color: None, position: Some(hint) });
        }
        if p.expect_comma().is_none() {
            break;
        }
    }
    if stops.iter().filter(|s| s.color.is_some()).count() < 2 {
        return None;
    }
    Some(stops)
}

/// `<position>`: one or two keywords/lengths, or the four-value form. Returns the
/// horizontal and vertical offsets.
pub fn parse_position(p: &mut Parser) -> Option<(LpSpec, LpSpec)> {
    p.try_parse(|p| {
        #[derive(Clone, Copy, PartialEq)]
        enum Kw {
            Left,
            Right,
            Top,
            Bottom,
            Center,
        }
        let kw = |p: &mut Parser| -> Option<Kw> {
            p.try_parse(|p| match p.expect_ident_lower()?.as_str() {
                "left" => Some(Kw::Left),
                "right" => Some(Kw::Right),
                "top" => Some(Kw::Top),
                "bottom" => Some(Kw::Bottom),
                "center" => Some(Kw::Center),
                _ => None,
            })
        };
        let pct = |v: i64| LpSpec::Percent(Number { micro: v * 1_000_000, int: true });
        enum Item {
            K(Kw),
            L(LpSpec),
        }
        let mut items = Vec::new();
        for _ in 0..4 {
            if let Some(k) = kw(p) {
                items.push(Item::K(k));
            } else if let Some(l) = parse_lp(p, Allow::ALL) {
                items.push(Item::L(l));
            } else {
                break;
            }
        }
        let horiz = |k: Kw| matches!(k, Kw::Left | Kw::Right | Kw::Center);
        let vert = |k: Kw| matches!(k, Kw::Top | Kw::Bottom | Kw::Center);
        let kw_h = |k: Kw| match k {
            Kw::Left => pct(0),
            Kw::Right => pct(100),
            _ => pct(50),
        };
        let kw_v = |k: Kw| match k {
            Kw::Top => pct(0),
            Kw::Bottom => pct(100),
            _ => pct(50),
        };
        match items.len() {
            1 => match &items[0] {
                Item::K(k) if horiz(*k) => Some((kw_h(*k), pct(50))),
                Item::K(k) => Some((pct(50), kw_v(*k))),
                Item::L(l) => Some((l.clone(), pct(50))),
            },
            2 => match (&items[0], &items[1]) {
                (Item::K(a), Item::K(b)) if horiz(*a) && vert(*b) => Some((kw_h(*a), kw_v(*b))),
                (Item::K(a), Item::K(b)) if vert(*a) && horiz(*b) => Some((kw_h(*b), kw_v(*a))),
                (Item::K(a), Item::L(l)) if horiz(*a) => Some((kw_h(*a), l.clone())),
                (Item::L(l), Item::K(b)) if vert(*b) => Some((l.clone(), kw_v(*b))),
                (Item::L(a), Item::L(b)) => Some((a.clone(), b.clone())),
                _ => None,
            },
            3 | 4 => {
                // `<edge> <offset>? <edge> <offset>?`
                let mut i = 0;
                let mut h: Option<LpSpec> = None;
                let mut v: Option<LpSpec> = None;
                while i < items.len() {
                    let Item::K(k) = &items[i] else { return None };
                    let k = *k;
                    i += 1;
                    let offset = if let Some(Item::L(l)) = items.get(i) {
                        i += 1;
                        Some(l.clone())
                    } else {
                        None
                    };
                    let value = |base: LpSpec, off: Option<LpSpec>, from_end: bool| -> Option<LpSpec> {
                        match off {
                            None => Some(base),
                            Some(o) if !from_end => Some(o),
                            Some(o) => Some(LpSpec::Calc(Box::new(CalcNode::Sum(vec![
                                CalcNode::Percent(Number::from_i64(100)),
                                CalcNode::Neg(Box::new(match o {
                                    LpSpec::Length(l) => CalcNode::Length(l),
                                    LpSpec::Percent(n) => CalcNode::Percent(n),
                                    LpSpec::Calc(c) => *c,
                                })),
                            ])))),
                        }
                    };
                    match k {
                        Kw::Left if h.is_none() => h = value(pct(0), offset, false),
                        Kw::Right if h.is_none() => h = value(pct(100), offset, true),
                        Kw::Top if v.is_none() => v = value(pct(0), offset, false),
                        Kw::Bottom if v.is_none() => v = value(pct(100), offset, true),
                        Kw::Center if offset.is_none() && h.is_none() => h = Some(pct(50)),
                        Kw::Center if offset.is_none() && v.is_none() => v = Some(pct(50)),
                        _ => return None,
                    }
                }
                Some((h?, v?))
            }
            _ => None,
        }
    })
}

// ---------------------------------------------------------------------------------
// Strings, identifiers, tokens as text

/// A custom identifier (not a CSS-wide keyword or `default`).
pub fn parse_custom_ident(p: &mut Parser) -> Option<String> {
    p.try_parse(|p| {
        let s = p.expect_ident()?;
        match s.to_ascii_lowercase().as_str() {
            "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default" => None,
            _ => Some(s.to_owned()),
        }
    })
}

/// Serialises component values back to CSS text (for custom property values,
/// `getComputedStyle` of unparsed values and error messages).
pub fn serialize_component_values(items: &[ComponentValue]) -> String {
    let mut out = String::new();
    for i in items {
        serialize_component_value(i, &mut out);
    }
    out
}

pub fn serialize_component_value(v: &ComponentValue, out: &mut String) {
    match v {
        ComponentValue::Token(t) => serialize_token(t, out),
        ComponentValue::Function { name, args } => {
            out.push_str(name);
            out.push('(');
            for a in args {
                serialize_component_value(a, out);
            }
            out.push(')');
        }
        ComponentValue::Block { open, contents } => {
            let (o, c) = match open {
                Token::OpenSquare => ('[', ']'),
                Token::OpenParen => ('(', ')'),
                _ => ('{', '}'),
            };
            out.push(o);
            for a in contents {
                serialize_component_value(a, out);
            }
            out.push(c);
        }
    }
}

fn serialize_token(t: &Token, out: &mut String) {
    match t {
        Token::Ident(s) => out.push_str(s),
        Token::Function(s) => {
            out.push_str(s);
            out.push('(');
        }
        Token::AtKeyword(s) => {
            out.push('@');
            out.push_str(s);
        }
        Token::Hash { value, .. } => {
            out.push('#');
            out.push_str(value);
        }
        Token::String(s) => {
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\a "),
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Token::BadString | Token::BadUrl => {}
        Token::Url(u) => {
            out.push_str("url(\"");
            out.push_str(u);
            out.push_str("\")");
        }
        Token::Delim(c) => out.push(*c),
        Token::Number { text, .. } | Token::Percentage { text, .. } | Token::Dimension { text, .. } => {
            out.push_str(text);
            if let Token::Percentage { .. } = t {
                out.push('%');
            }
            if let Token::Dimension { unit, .. } = t {
                out.push_str(unit);
            }
        }
        Token::Whitespace => out.push(' '),
        Token::Cdo => out.push_str("<!--"),
        Token::Cdc => out.push_str("-->"),
        Token::Colon => out.push(':'),
        Token::Semicolon => out.push(';'),
        Token::Comma => out.push(','),
        Token::OpenSquare => out.push('['),
        Token::CloseSquare => out.push(']'),
        Token::OpenParen => out.push('('),
        Token::CloseParen => out.push(')'),
        Token::OpenCurly => out.push('{'),
        Token::CloseCurly => out.push('}'),
    }
}

/// Whether a value contains `var()` anywhere (then it is substituted at computed-value
/// time before parsing).
pub fn contains_var(items: &[ComponentValue]) -> bool {
    items.iter().any(|v| match v {
        ComponentValue::Function { name, args } => name.eq_ignore_ascii_case("var") || contains_var(args),
        ComponentValue::Block { contents, .. } => contains_var(contents),
        _ => false,
    })
}

// ---------------------------------------------------------------------------------
// Token constructors, for presentational hints and tests

pub fn tok_ident(s: &str) -> ComponentValue {
    ComponentValue::Token(Token::Ident(s.to_owned()))
}
pub fn tok_ws() -> ComponentValue {
    ComponentValue::Token(Token::Whitespace)
}
pub fn tok_comma() -> ComponentValue {
    ComponentValue::Token(Token::Comma)
}
pub fn tok_string(s: &str) -> ComponentValue {
    ComponentValue::Token(Token::String(s.to_owned()))
}
pub fn tok_number(n: Number) -> ComponentValue {
    ComponentValue::Token(Token::Number { text: number_text(n), value: n })
}
pub fn tok_int(v: i64) -> ComponentValue {
    tok_number(Number::from_i64(v))
}
pub fn tok_dimension(n: Number, unit: &str) -> ComponentValue {
    ComponentValue::Token(Token::Dimension { text: number_text(n), value: n, unit: unit.to_owned() })
}
pub fn tok_px(v: i64) -> ComponentValue {
    tok_dimension(Number::from_i64(v), "px")
}
pub fn tok_percent(n: Number) -> ComponentValue {
    ComponentValue::Token(Token::Percentage { text: number_text(n), value: n })
}
pub fn tok_hash(s: &str) -> ComponentValue {
    ComponentValue::Token(Token::Hash { value: s.to_owned(), id: false })
}
pub fn tok_color(c: Color) -> ComponentValue {
    ComponentValue::Function {
        name: "rgba".into(),
        args: vec![tok_int(c.0 as i64), tok_comma(), tok_int(c.1 as i64), tok_comma(), tok_int(c.2 as i64), tok_comma(), tok_number(Number { micro: c.3 as i64 * 1_000_000 / 255, int: false })],
    }
}

/// Decimal text of a number in millionths, trimmed (`1.5`, `-2`, `0.25`).
pub fn number_text(n: Number) -> String {
    let neg = n.micro < 0;
    let abs = n.micro.unsigned_abs();
    let int = abs / 1_000_000;
    let frac = abs % 1_000_000;
    let mut s = String::new();
    if neg && (int != 0 || frac != 0) {
        s.push('-');
    }
    s.push_str(&int.to_string());
    if frac != 0 {
        let mut f = format!("{frac:06}");
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
    }
    s
}

#[cfg(test)]
pub(crate) mod test_util {
    //! A tiny tokenizer for tests, so value parsers can be exercised before the css
    //! module's tokenizer is used (the cascade uses the real one).
    use super::*;

    pub fn tokenize(src: &str) -> Vec<ComponentValue> {
        let mut out = Vec::new();
        let chars: Vec<char> = src.chars().collect();
        let mut i = 0;
        parse_list(&chars, &mut i, None, &mut out);
        out
    }

    fn is_name_start(c: char) -> bool {
        c.is_ascii_alphabetic() || c == '_' || c as u32 > 0x7F
    }
    fn is_name(c: char) -> bool {
        is_name_start(c) || c.is_ascii_digit() || c == '-'
    }

    fn parse_list(chars: &[char], i: &mut usize, close: Option<char>, out: &mut Vec<ComponentValue>) {
        while *i < chars.len() {
            let c = chars[*i];
            if Some(c) == close {
                *i += 1;
                return;
            }
            if c.is_whitespace() {
                while *i < chars.len() && chars[*i].is_whitespace() {
                    *i += 1;
                }
                out.push(tok_ws());
                continue;
            }
            match c {
                '"' | '\'' => {
                    *i += 1;
                    let mut s = String::new();
                    while *i < chars.len() && chars[*i] != c {
                        if chars[*i] == '\\' && *i + 1 < chars.len() {
                            *i += 1;
                        }
                        s.push(chars[*i]);
                        *i += 1;
                    }
                    *i += 1;
                    out.push(tok_string(&s));
                }
                '#' => {
                    *i += 1;
                    let mut s = String::new();
                    while *i < chars.len() && is_name(chars[*i]) {
                        s.push(chars[*i]);
                        *i += 1;
                    }
                    out.push(ComponentValue::Token(Token::Hash { id: s.chars().next().is_some_and(is_name_start), value: s }));
                }
                ',' => {
                    *i += 1;
                    out.push(tok_comma());
                }
                ':' => {
                    *i += 1;
                    out.push(ComponentValue::Token(Token::Colon));
                }
                ';' => {
                    *i += 1;
                    out.push(ComponentValue::Token(Token::Semicolon));
                }
                '(' | '[' | '{' => {
                    *i += 1;
                    let (open, cl) = match c {
                        '(' => (Token::OpenParen, ')'),
                        '[' => (Token::OpenSquare, ']'),
                        _ => (Token::OpenCurly, '}'),
                    };
                    let mut contents = Vec::new();
                    parse_list(chars, i, Some(cl), &mut contents);
                    out.push(ComponentValue::Block { open, contents });
                }
                c if c.is_ascii_digit() || ((c == '.' || c == '+' || c == '-') && number_ahead(chars, *i)) => {
                    let start = *i;
                    if c == '+' || c == '-' {
                        *i += 1;
                    }
                    while *i < chars.len() && (chars[*i].is_ascii_digit() || chars[*i] == '.') {
                        *i += 1;
                    }
                    if *i < chars.len() && (chars[*i] == 'e' || chars[*i] == 'E') {
                        let mut j = *i + 1;
                        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                            j += 1;
                        }
                        if j < chars.len() && chars[j].is_ascii_digit() {
                            *i = j;
                            while *i < chars.len() && chars[*i].is_ascii_digit() {
                                *i += 1;
                            }
                        }
                    }
                    let text: String = chars[start..*i].iter().collect();
                    let value = Number::parse(&text).unwrap();
                    if *i < chars.len() && chars[*i] == '%' {
                        *i += 1;
                        out.push(ComponentValue::Token(Token::Percentage { text, value }));
                    } else if *i < chars.len() && is_name_start(chars[*i]) {
                        let mut unit = String::new();
                        while *i < chars.len() && is_name(chars[*i]) {
                            unit.push(chars[*i]);
                            *i += 1;
                        }
                        out.push(ComponentValue::Token(Token::Dimension { text, value, unit }));
                    } else {
                        out.push(ComponentValue::Token(Token::Number { text, value }));
                    }
                }
                c if is_name_start(c) || (c == '-' && *i + 1 < chars.len() && (is_name_start(chars[*i + 1]) || chars[*i + 1] == '-')) => {
                    let mut name = String::new();
                    while *i < chars.len() && (is_name(chars[*i]) || chars[*i] == '\\') {
                        if chars[*i] == '\\' && *i + 1 < chars.len() {
                            *i += 1;
                        }
                        name.push(chars[*i]);
                        *i += 1;
                    }
                    if *i < chars.len() && chars[*i] == '(' {
                        *i += 1;
                        if name.eq_ignore_ascii_case("url") {
                            let mut j = *i;
                            while j < chars.len() && chars[j].is_whitespace() {
                                j += 1;
                            }
                            if j < chars.len() && chars[j] != '"' && chars[j] != '\'' {
                                let mut u = String::new();
                                while j < chars.len() && chars[j] != ')' {
                                    u.push(chars[j]);
                                    j += 1;
                                }
                                *i = j + 1;
                                out.push(ComponentValue::Token(Token::Url(u.trim().to_owned())));
                                continue;
                            }
                        }
                        let mut args = Vec::new();
                        parse_list(chars, i, Some(')'), &mut args);
                        out.push(ComponentValue::Function { name, args });
                    } else {
                        out.push(tok_ident(&name));
                    }
                }
                c => {
                    *i += 1;
                    out.push(ComponentValue::Token(Token::Delim(c)));
                }
            }
        }
    }

    fn number_ahead(chars: &[char], i: usize) -> bool {
        let c = chars[i];
        if c.is_ascii_digit() {
            return true;
        }
        if c == '.' {
            return chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
        }
        // + or -
        match chars.get(i + 1) {
            Some(d) if d.is_ascii_digit() => true,
            Some('.') => chars.get(i + 2).is_some_and(|d| d.is_ascii_digit()),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::tokenize;
    use super::*;
    use crate::style::computed::LengthPercentage;

    fn lp(src: &str) -> Option<LengthPercentage> {
        let toks = tokenize(src);
        let mut p = Parser::new(&toks);
        let v = p.parse_entirely(|p| parse_lp(p, Allow::ALL))?;
        v.compute(&LengthContext::default())
    }

    fn color(src: &str) -> Option<Color> {
        let toks = tokenize(src);
        let mut p = Parser::new(&toks);
        p.parse_entirely(parse_color).map(|c| c.resolve(Color(1, 2, 3, 255)))
    }

    #[test]
    fn lengths_in_every_unit() {
        assert_eq!(lp("10px"), Some(LengthPercentage::Length(Au::from_px_i32(10))));
        assert_eq!(lp("0"), Some(LengthPercentage::ZERO));
        assert_eq!(lp("1in"), Some(LengthPercentage::Length(Au::from_px_i32(96))));
        assert_eq!(lp("2.54cm"), Some(LengthPercentage::Length(Au::from_px_i32(96))));
        assert_eq!(lp("25.4mm"), Some(LengthPercentage::Length(Au::from_px_i32(96))));
        assert_eq!(lp("72pt"), Some(LengthPercentage::Length(Au::from_px_i32(96))));
        assert_eq!(lp("1pc"), Some(LengthPercentage::Length(Au::from_px_i32(16))));
        assert_eq!(lp("4Q"), Some(LengthPercentage::Length(Au::from_f64_px(3.779528))));
        assert_eq!(lp("2em"), Some(LengthPercentage::Length(Au::from_px_i32(32))));
        assert_eq!(lp("1.5rem"), Some(LengthPercentage::Length(Au::from_px_i32(24))));
        assert_eq!(lp("2ex"), Some(LengthPercentage::Length(Au::from_px_i32(16))));
        assert_eq!(lp("4ch"), Some(LengthPercentage::Length(Au::from_px_i32(32))));
        assert_eq!(lp("10vw"), Some(LengthPercentage::Length(Au::from_px_i32(128))));
        assert_eq!(lp("10vh"), Some(LengthPercentage::Length(Au::from_px_i32(80))));
        assert_eq!(lp("10vmin"), Some(LengthPercentage::Length(Au::from_px_i32(80))));
        assert_eq!(lp("10vmax"), Some(LengthPercentage::Length(Au::from_px_i32(128))));
        assert_eq!(lp("1lh"), Some(LengthPercentage::Length(Au::from_f64_px(19.2))));
        assert_eq!(lp("50%"), Some(LengthPercentage::Percent(5000)));
        assert_eq!(lp("12.5%"), Some(LengthPercentage::Percent(1250)));
        assert_eq!(lp("0.5px"), Some(LengthPercentage::Length(Au(32))));
        assert_eq!(lp("10"), None);
        assert_eq!(lp("10furlongs"), None);
        assert_eq!(lp("auto"), None);
        assert_eq!(lp("10px 5px"), None);
    }

    #[test]
    fn calc_arithmetic_and_units() {
        assert_eq!(lp("calc(10px + 2em)"), Some(LengthPercentage::Length(Au::from_px_i32(42))));
        assert_eq!(lp("calc(100% - 20px)"), Some(LengthPercentage::Calc(Au::from_px_i32(-20), 10000)));
        assert_eq!(lp("calc(2 * 10px)"), Some(LengthPercentage::Length(Au::from_px_i32(20))));
        assert_eq!(lp("calc(10px * 2)"), Some(LengthPercentage::Length(Au::from_px_i32(20))));
        assert_eq!(lp("calc(30px / 3)"), Some(LengthPercentage::Length(Au::from_px_i32(10))));
        assert_eq!(lp("calc((10px + 5px) * 2)"), Some(LengthPercentage::Length(Au::from_px_i32(30))));
        assert_eq!(lp("calc(50% / 2)"), Some(LengthPercentage::Percent(2500)));
        assert_eq!(lp("min(10px, 5px, 8px)"), Some(LengthPercentage::Length(Au::from_px_i32(5))));
        assert_eq!(lp("max(10px, 1em)"), Some(LengthPercentage::Length(Au::from_px_i32(16))));
        assert_eq!(lp("clamp(10px, 50px, 20px)"), Some(LengthPercentage::Length(Au::from_px_i32(20))));
        assert_eq!(lp("clamp(10px, 5px, 20px)"), Some(LengthPercentage::Length(Au::from_px_i32(10))));
        assert_eq!(lp("calc(min(10px, 20px) + 1px)"), Some(LengthPercentage::Length(Au::from_px_i32(11))));
        assert_eq!(lp("calc(1px - -1px)"), Some(LengthPercentage::Length(Au::from_px_i32(2))));
        assert_eq!(lp("calc(-10px)"), Some(LengthPercentage::Length(Au::from_px_i32(-10))));
        // Failures: unit algebra.
        assert_eq!(lp("calc(10px * 2px)"), None);
        assert_eq!(lp("calc(10px / 2px)"), None);
        assert_eq!(lp("calc(10px + 2)"), None);
        assert_eq!(lp("calc(10 / 0)"), None);
        assert_eq!(lp("calc(10px +)"), None);
        assert_eq!(lp("calc()"), None);
        assert_eq!(lp("calc(2)"), None);
        assert_eq!(lp("clamp(1px, 2px)"), None);
        // Mixed length/percentage comparisons wait for the percentage base.
        let px = Au::from_px_i32;
        assert_eq!(lp("min(10px, 50%)"), Some(LengthPercentage::Clamp { lo: None, v: (px(10), 0), hi: Some((px(0), 5000)) }));
        assert_eq!(lp("min(10px, 50%)").unwrap().resolve(px(100)), px(10));
        assert_eq!(lp("min(10px, 50%)").unwrap().resolve(px(10)), px(5));
        assert_eq!(lp("max(10px, 50%)").unwrap().resolve(px(10)), px(10));
        assert_eq!(lp("max(10px, 50%)").unwrap().resolve(px(100)), px(50));
        // The stripe container: min(1080px, calc(100% - 2 * 32px)).
        let w = lp("min(1080px, calc(100% - 2 * 32px))").unwrap();
        assert_eq!(w, LengthPercentage::Clamp { lo: None, v: (px(1080), 0), hi: Some((px(-64), 10000)) });
        assert_eq!(w.resolve(px(1280)), px(1080));
        assert_eq!(w.resolve(px(800)), px(736));
        // Pure lengths fold among themselves before the deferred comparison.
        assert_eq!(lp("min(10px, 5px, 50%)").unwrap().resolve(px(100)), px(5));
        // clamp(): the lower bound wins.
        let c = lp("clamp(20px, 50%, 100px)").unwrap();
        assert_eq!(c.resolve(px(10)), px(20));
        assert_eq!(c.resolve(px(100)), px(50));
        assert_eq!(c.resolve(px(1000)), px(100));
        // Arithmetic around a deferred comparison shifts and scales its bounds.
        let m = lp("calc(min(10px, 50%) * 2 + 1px)").unwrap();
        assert_eq!(m.resolve(px(100)), px(21));
        assert_eq!(m.resolve(px(10)), px(11));
        assert_eq!(lp("calc(min(10px, 50%) + max(1px, 1%))"), None);
    }

    #[test]
    fn numbers_and_integers() {
        let toks = tokenize("calc(2 * 3)");
        assert_eq!(Parser::new(&toks).parse_entirely(parse_number_spec).map(|n| n.micro), Some(6_000_000));
        let toks = tokenize("3");
        assert_eq!(Parser::new(&toks).parse_entirely(parse_integer_spec), Some(3));
        let toks = tokenize("3.5");
        assert_eq!(Parser::new(&toks).parse_entirely(|p| p.expect_integer()), None);
    }

    #[test]
    fn angles_and_times() {
        let a = |s: &str| {
            let toks = tokenize(s);
            Parser::new(&toks).parse_entirely(|p| parse_angle(p, true))
        };
        assert_eq!(a("90deg"), Some(9000));
        assert_eq!(a("100grad"), Some(9000));
        assert_eq!(a("0.5turn"), Some(18000));
        assert_eq!(a("3.141593rad"), Some(18000));
        assert_eq!(a("-45deg"), Some(-4500));
        assert_eq!(a("0"), Some(0));
        assert_eq!(a("10px"), None);
        let t = |s: &str| {
            let toks = tokenize(s);
            Parser::new(&toks).parse_entirely(parse_time)
        };
        assert_eq!(t("1.5s"), Some(1500));
        assert_eq!(t("250ms"), Some(250));
        assert_eq!(t("2"), None);
    }

    #[test]
    fn colours() {
        assert_eq!(color("#f00"), Some(Color(255, 0, 0, 255)));
        assert_eq!(color("#f008"), Some(Color(255, 0, 0, 136)));
        assert_eq!(color("#ff0000"), Some(Color(255, 0, 0, 255)));
        assert_eq!(color("#ff000080"), Some(Color(255, 0, 0, 128)));
        assert_eq!(color("#ff00"), Some(Color(255, 255, 0, 0)));
        assert_eq!(color("#ff000"), None);
        assert_eq!(color("#ggg"), None);
        assert_eq!(color("rgb(1, 2, 3)"), Some(Color(1, 2, 3, 255)));
        assert_eq!(color("rgba(1, 2, 3, 0.5)"), Some(Color(1, 2, 3, 128)));
        assert_eq!(color("rgb(1 2 3 / 50%)"), Some(Color(1, 2, 3, 128)));
        assert_eq!(color("rgb(100%, 0%, 50%)"), Some(Color(255, 0, 128, 255)));
        assert_eq!(color("rgb(300, -5, 3)"), Some(Color(255, 0, 3, 255)));
        assert_eq!(color("rgb(1, 2)"), None);
        assert_eq!(color("rgb(1 2 3 4)"), None);
        assert_eq!(color("hsl(0, 100%, 50%)"), Some(Color(255, 0, 0, 255)));
        assert_eq!(color("hsl(120deg 100% 50%)"), Some(Color(0, 255, 0, 255)));
        assert_eq!(color("hsl(240 100% 50% / 0.5)"), Some(Color(0, 0, 255, 128)));
        assert_eq!(color("hsla(0, 0%, 50%, 1)"), Some(Color(128, 128, 128, 255)));
        assert_eq!(color("hsl(0.5turn 100% 50%)"), Some(Color(0, 255, 255, 255)));
        assert_eq!(color("hwb(0 0% 0%)"), Some(Color(255, 0, 0, 255)));
        assert_eq!(color("hwb(0 50% 50%)"), Some(Color(128, 128, 128, 255)));
        assert_eq!(color("hwb(120 20% 20%)"), Some(Color(51, 204, 51, 255)));
        assert_eq!(color("red"), Some(Color(255, 0, 0, 255)));
        assert_eq!(color("RebeccaPurple"), Some(Color(102, 51, 153, 255)));
        assert_eq!(color("transparent"), Some(Color(0, 0, 0, 0)));
        assert_eq!(color("currentColor"), Some(Color(1, 2, 3, 255)));
        assert_eq!(color("notacolor"), None);
        assert_eq!(color("color-mix(in srgb, red, blue)"), Some(Color(128, 0, 128, 255)));
        assert_eq!(color("color-mix(in srgb, red 25%, blue)"), Some(Color(64, 0, 191, 255)));
        assert_eq!(color("color-mix(in srgb, red 30%, blue 30%)"), Some(Color(128, 0, 128, 153)));
        assert_eq!(color("color-mix(in lab, red, blue)"), None);
        assert_eq!(NAMED_COLORS.len(), 148);
        assert!(NAMED_COLORS.windows(2).all(|w| w[0].0 < w[1].0), "table is sorted for binary search");
    }

    #[test]
    fn legacy_colours() {
        assert_eq!(parse_legacy_color("red"), Some(Color(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color("#f00"), Some(Color(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color("ff0000"), Some(Color(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color("#FF0000"), Some(Color(255, 0, 0, 255)));
        assert_eq!(parse_legacy_color("chucknorris"), Some(Color(0xc0, 0x00, 0x00, 255)));
        assert_eq!(parse_legacy_color("transparent"), None);
        assert_eq!(parse_legacy_color(""), None);
        assert_eq!(parse_legacy_color("#abcdefabcdef"), Some(Color(0xab, 0xef, 0xcd, 255)));
    }

    #[test]
    fn images() {
        let img = |s: &str| {
            let toks = tokenize(s);
            Parser::new(&toks).parse_entirely(parse_image)
        };
        assert_eq!(img("none"), Some(ImageSpec::None));
        assert_eq!(img("url(a.png)"), Some(ImageSpec::Url("a.png".into())));
        assert_eq!(img("url(\"a b.png\")"), Some(ImageSpec::Url("a b.png".into())));
        match img("linear-gradient(to right, red, blue 50%, 75%, green)") {
            Some(ImageSpec::Linear { direction, stops, repeating: false }) => {
                assert_eq!(direction, GradientDirection::Side(1, 0));
                assert_eq!(stops.len(), 4);
                assert!(stops[2].color.is_none());
            }
            other => panic!("{other:?}"),
        }
        match img("linear-gradient(45deg, red 0 50%, blue)") {
            Some(ImageSpec::Linear { direction: GradientDirection::Angle(4500), stops, .. }) => assert_eq!(stops.len(), 3),
            other => panic!("{other:?}"),
        }
        match img("linear-gradient(red, blue)") {
            Some(ImageSpec::Linear { direction: GradientDirection::Side(0, 1), .. }) => {}
            other => panic!("{other:?}"),
        }
        match img("radial-gradient(circle at center, red, blue)") {
            Some(ImageSpec::Radial { circle: true, stops, .. }) => assert_eq!(stops.len(), 2),
            other => panic!("{other:?}"),
        }
        match img("-webkit-linear-gradient(to top, red, blue)") {
            Some(ImageSpec::Linear { direction: GradientDirection::Side(0, -1), .. }) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(img("linear-gradient(red)"), None);
        assert_eq!(img("linear-gradient(to nowhere, red, blue)"), None);
        assert_eq!(img("linear-gradient(50%, red, blue)"), None);
        assert_eq!(img("foo(1)"), None);
    }

    #[test]
    fn positions() {
        let pos = |s: &str| {
            let toks = tokenize(s);
            Parser::new(&toks).parse_entirely(parse_position).map(|(a, b)| (a.compute(&LengthContext::default()).unwrap(), b.compute(&LengthContext::default()).unwrap()))
        };
        let pct = |v| LengthPercentage::Percent(v);
        assert_eq!(pos("center"), Some((pct(5000), pct(5000))));
        assert_eq!(pos("left"), Some((pct(0), pct(5000))));
        assert_eq!(pos("top"), Some((pct(5000), pct(0))));
        assert_eq!(pos("right bottom"), Some((pct(10000), pct(10000))));
        assert_eq!(pos("bottom right"), Some((pct(10000), pct(10000))));
        assert_eq!(pos("10px 20%"), Some((LengthPercentage::Length(Au::from_px_i32(10)), pct(2000))));
        assert_eq!(pos("right 10px bottom 20px"), Some((LengthPercentage::Calc(Au::from_px_i32(-10), 10000), LengthPercentage::Calc(Au::from_px_i32(-20), 10000))));
        assert_eq!(pos("left left"), None);
        assert_eq!(pos("10px left"), None);
    }

    #[test]
    fn serialization_round_trip() {
        let toks = tokenize("1px solid rgb(1, 2, 3) \"a\" url(x)");
        assert_eq!(serialize_component_values(&toks), "1px solid rgb(1, 2, 3) \"a\" url(\"x\")");
        assert_eq!(number_text(Number { micro: 1_500_000, int: false }), "1.5");
        assert_eq!(number_text(Number { micro: -250_000, int: false }), "-0.25");
        assert_eq!(number_text(Number::from_i64(12)), "12");
        assert!(contains_var(&tokenize("calc(var(--x) + 1px)")));
        assert!(!contains_var(&tokenize("1px")));
    }

    #[test]
    fn css_wide_keywords() {
        let toks = tokenize("inherit");
        assert_eq!(parse_css_wide(&mut Parser::new(&toks)), Some(CssWide::Inherit));
        let toks = tokenize("revert-layer");
        assert_eq!(parse_css_wide(&mut Parser::new(&toks)), Some(CssWide::RevertLayer));
        let toks = tokenize("inherit 1px");
        assert_eq!(parse_css_wide(&mut Parser::new(&toks)), None);
    }
}
