//! Cell values and Excel's coercion and comparison rules.
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorKind {
    Null,
    Div0,
    Value,
    Ref,
    Name,
    Num,
    NA,
    /// A cell that depends on itself.
    Circular,
}
impl ErrorKind {
    /// The code Excel displays and stores.
    pub fn code(self) -> &'static str {
        match self {
            Self::Null => "#NULL!",
            Self::Div0 => "#DIV/0!",
            Self::Value => "#VALUE!",
            Self::Ref => "#REF!",
            Self::Name => "#NAME?",
            Self::Num => "#NUM!",
            Self::NA => "#N/A",
            Self::Circular => "#CIRCULAR!",
        }
    }
    pub fn parse(code: &str) -> Option<Self> {
        Some(match code.to_ascii_uppercase().as_str() {
            "#NULL!" => Self::Null,
            "#DIV/0!" => Self::Div0,
            "#VALUE!" => Self::Value,
            "#REF!" => Self::Ref,
            "#NAME?" => Self::Name,
            "#NUM!" => Self::Num,
            "#N/A" => Self::NA,
            _ => return None,
        })
    }
    /// ERROR.TYPE's number.
    pub fn number(self) -> f64 {
        match self {
            Self::Null => 1.0,
            Self::Div0 => 2.0,
            Self::Value => 3.0,
            Self::Ref | Self::Circular => 4.0,
            Self::Name => 5.0,
            Self::Num => 6.0,
            Self::NA => 7.0,
        }
    }
}

/// A computed cell value. Numbers are always finite: an overflow is `#NUM!`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    #[default]
    Empty,
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,
            (Self::Number(a), Self::Number(b)) => a.to_bits() == b.to_bits(),
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Error(a), Self::Error(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for Value {}

impl Value {
    /// A number, or `#NUM!` when the arithmetic left the finite doubles.
    pub fn number(x: f64) -> Self {
        if x.is_finite() {
            // Excel has no negative zero.
            Self::Number(if x == 0.0 { 0.0 } else { x })
        } else {
            Self::Error(ErrorKind::Num)
        }
    }
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
    pub fn error(&self) -> Option<ErrorKind> {
        match self {
            Self::Error(e) => Some(*e),
            _ => None,
        }
    }
    /// Coerce for arithmetic: blanks are 0, booleans 1/0, numeric text its number.
    pub fn to_number(&self) -> Result<f64, ErrorKind> {
        match self {
            Self::Empty => Ok(0.0),
            Self::Number(n) => Ok(*n),
            Self::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Self::Text(t) => parse_number_text(t).ok_or(ErrorKind::Value),
            Self::Error(e) => Err(*e),
        }
    }
    /// Coerce for text operations: numbers in General format, booleans in capitals.
    pub fn to_text(&self) -> Result<String, ErrorKind> {
        match self {
            Self::Empty => Ok(String::new()),
            Self::Number(n) => Ok(general(*n)),
            Self::Text(t) => Ok(t.clone()),
            Self::Bool(b) => Ok(if *b { "TRUE" } else { "FALSE" }.into()),
            Self::Error(e) => Err(*e),
        }
    }
    /// Coerce for logic: numbers are true when non-zero, text "TRUE"/"FALSE".
    pub fn to_bool(&self) -> Result<bool, ErrorKind> {
        match self {
            Self::Empty => Ok(false),
            Self::Number(n) => Ok(*n != 0.0),
            Self::Bool(b) => Ok(*b),
            Self::Text(t) => match t.to_ascii_uppercase().as_str() {
                "TRUE" => Ok(true),
                "FALSE" => Ok(false),
                _ => Err(ErrorKind::Value),
            },
            Self::Error(e) => Err(*e),
        }
    }
    /// What the cell shows with no number format: General numbers, TRUE/FALSE, codes.
    pub fn display(&self) -> String {
        match self {
            Self::Error(e) => e.code().into(),
            other => other.to_text().unwrap_or_default(),
        }
    }
}

/// Excel's General rendering of a number: up to 15 significant digits, switching to
/// scientific notation for very large or very small magnitudes.
pub fn general(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let a = x.abs();
    if !(1e-9..1e15).contains(&a) {
        let s = cw_determinism::math::format_significant(x, 6);
        // Excel writes exponents as E+nn.
        return s.replace('e', "E");
    }
    cw_determinism::math::format_significant(x, 15)
}

/// Numeric text as Excel reads it in arithmetic: optional sign, thousands commas,
/// decimals, an exponent, a trailing percent, or a parenthesised negative.
pub fn parse_number_text(text: &str) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let (t, negate) = match t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(inner) => (inner.trim(), true),
        None => (t, false),
    };
    let (t, percent) = match t.strip_suffix('%') {
        Some(x) => (x.trim_end(), true),
        None => (t, false),
    };
    let t = t.trim_start_matches('+');
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t),
    };
    let t = t
        .strip_prefix('$')
        .or_else(|| t.strip_prefix('€'))
        .or_else(|| t.strip_prefix('£'))
        .unwrap_or(t);
    // Thousands separators only between digit groups of three.
    let (int, rest) = match t.find(['.', 'e', 'E']) {
        Some(i) => t.split_at(i),
        None => (t, ""),
    };
    if int.contains(',') {
        let groups: Vec<&str> = int.split(',').collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return None;
        }
    }
    let plain = format!("{}{rest}", int.replace(',', ""));
    if plain.is_empty()
        || !plain
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        || !plain.starts_with(|c: char| c.is_ascii_digit() || c == '.')
    {
        return None;
    }
    let mut v: f64 = plain.parse().ok()?;
    if percent {
        v /= 100.0;
    }
    v *= sign;
    if negate {
        v = -v;
    }
    v.is_finite().then_some(v)
}

/// Ordering used by sorting and lookups: numbers < text < booleans < errors, with
/// text compared case-insensitively. Blanks sort after everything.
pub fn compare(a: &Value, b: &Value) -> Ordering {
    let rank = |v: &Value| match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Bool(_) => 2,
        Value::Error(_) => 3,
        Value::Empty => 4,
    };
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Value::Text(x), Value::Text(y)) => text_cmp(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => rank(a).cmp(&rank(b)),
    }
}
pub fn text_cmp(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// Comparison operators in formulas: blanks compare as 0 or "", otherwise
/// numbers < text < booleans and text ignores case.
pub fn formula_compare(a: &Value, b: &Value) -> Result<Ordering, ErrorKind> {
    if let Some(e) = a.error().or(b.error()) {
        return Err(e);
    }
    let fill = |v: &Value, other: &Value| match (v, other) {
        (Value::Empty, Value::Text(_)) => Value::Text(String::new()),
        (Value::Empty, Value::Bool(_)) => Value::Bool(false),
        (Value::Empty, _) => Value::Number(0.0),
        (v, _) => v.clone(),
    };
    let (x, y) = (fill(a, b), fill(b, a));
    Ok(compare(&x, &y))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn general_format_matches_excel() {
        assert_eq!(general(0.1 + 0.2), "0.3");
        assert_eq!(general(1.0 / 3.0), "0.333333333333333");
        assert_eq!(general(1234567.0), "1234567");
        assert_eq!(general(1e20), "1E+20");
        assert_eq!(general(-2.5), "-2.5");
    }
    #[test]
    fn text_coerces_the_way_arithmetic_does() {
        assert_eq!(parse_number_text("1,234.5"), Some(1234.5));
        assert_eq!(parse_number_text("12%"), Some(0.12));
        assert_eq!(parse_number_text("$5"), Some(5.0));
        assert_eq!(parse_number_text("(3)"), Some(-3.0));
        assert_eq!(parse_number_text("1,23"), None);
        assert_eq!(parse_number_text("abc"), None);
        assert_eq!(Value::Text("x".into()).to_number(), Err(ErrorKind::Value));
        assert_eq!(Value::Empty.to_number(), Ok(0.0));
    }
    #[test]
    fn comparisons_order_types_and_ignore_case() {
        assert_eq!(
            formula_compare(&Value::Text("a".into()), &Value::Text("A".into())),
            Ok(Ordering::Equal)
        );
        assert_eq!(
            formula_compare(&Value::Number(1e9), &Value::Text("a".into())),
            Ok(Ordering::Less)
        );
        assert_eq!(
            formula_compare(&Value::Empty, &Value::Number(0.0)),
            Ok(Ordering::Equal)
        );
        assert_eq!(
            formula_compare(&Value::Bool(false), &Value::Text("z".into())),
            Ok(Ordering::Greater)
        );
    }
}
