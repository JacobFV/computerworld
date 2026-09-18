//! SQLite's dynamic values, type affinity and comparison rules.
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::fmt;

/// One SQL value. `Real` never holds NaN: SQLite turns NaN into NULL.
#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Real(a), Self::Real(b)) => a.to_bits() == b.to_bits(),
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Blob(a), Self::Blob(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for Value {}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => s.serialize_unit(),
            Self::Integer(i) => s.serialize_i64(*i),
            Self::Real(r) if r.is_finite() => s.serialize_f64(*r),
            Self::Real(r) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("real", if *r > 0.0 { "inf" } else { "-inf" })?;
                m.end()
            }
            Self::Text(t) => s.serialize_str(t),
            Self::Blob(b) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("blob", &hex(b))?;
                m.end()
            }
        }
    }
}
impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Value;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an SQL value")
            }
            fn visit_unit<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_none<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
                Ok(Value::Integer(i64::from(v)))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
                Ok(Value::Integer(v))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
                i64::try_from(v)
                    .map(Value::Integer)
                    .map_err(|_| E::custom("integer out of range"))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
                Ok(Value::Real(v))
            }
            fn visit_str<E>(self, v: &str) -> Result<Value, E> {
                Ok(Value::Text(v.to_owned()))
            }
            fn visit_string<E>(self, v: String) -> Result<Value, E> {
                Ok(Value::Text(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
                let (key, value): (String, String) = map
                    .next_entry()?
                    .ok_or_else(|| de::Error::custom("empty value"))?;
                match (key.as_str(), value.as_str()) {
                    ("blob", hexed) => unhex(hexed)
                        .map(Value::Blob)
                        .ok_or_else(|| de::Error::custom("invalid blob")),
                    ("real", "inf") => Ok(Value::Real(f64::INFINITY)),
                    ("real", "-inf") => Ok(Value::Real(f64::NEG_INFINITY)),
                    _ => Err(de::Error::custom("unknown value encoding")),
                }
            }
        }
        d.deserialize_any(V)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    text.as_bytes()
        .chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}

/// Render a double the way SQLite's `%!.15g` does: at most 15 significant digits and
/// always with a decimal point or exponent, so `1.0` stays distinguishable from `1`.
pub fn format_real(r: f64) -> String {
    if r.is_infinite() {
        return if r > 0.0 { "Inf".into() } else { "-Inf".into() };
    }
    let mut s = cw_determinism::math::format_significant(r, 15);
    if let Some(e) = s.find('e') {
        if !s[..e].contains('.') {
            s.insert_str(e, ".0");
        }
    } else if !s.contains('.') {
        s.push_str(".0");
    }
    s
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Integer(_) => "integer",
            Self::Real(_) => "real",
            Self::Text(_) => "text",
            Self::Blob(_) => "blob",
        }
    }
    /// A real that is NaN becomes NULL, as SQLite stores it.
    pub fn real(r: f64) -> Self {
        if r.is_nan() {
            Self::Null
        } else {
            Self::Real(r)
        }
    }
    /// Text form, as `CAST(x AS TEXT)` and the CLI produce it.
    pub fn to_text(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Integer(i) => i.to_string(),
            Self::Real(r) => format_real(*r),
            Self::Text(t) => t.clone(),
            Self::Blob(b) => String::from_utf8_lossy(b).into_owned(),
        }
    }
    /// Numeric value for arithmetic: text and blobs are read by their longest numeric
    /// prefix (so `'12abc'` is 12 and `'abc'` is 0).
    pub fn to_number(&self) -> Value {
        match self {
            Self::Null => Self::Null,
            Self::Integer(_) | Self::Real(_) => self.clone(),
            Self::Text(t) => numeric_prefix(t),
            Self::Blob(b) => numeric_prefix(&String::from_utf8_lossy(b)),
        }
    }
    pub fn to_f64(&self) -> Option<f64> {
        match self.to_number() {
            Self::Integer(i) => Some(i as f64),
            Self::Real(r) => Some(r),
            _ => None,
        }
    }
    pub fn to_i64(&self) -> Option<i64> {
        match self.to_number() {
            Self::Integer(i) => Some(i),
            Self::Real(r) => Some(real_to_int(r)),
            _ => None,
        }
    }
    /// SQL truth: NULL is unknown (`None`), numbers are true when non-zero.
    pub fn truth(&self) -> Option<bool> {
        match self.to_number() {
            Self::Null => None,
            Self::Integer(i) => Some(i != 0),
            Self::Real(r) => Some(r != 0.0),
            _ => Some(false),
        }
    }
    /// Text that SQLite quotes it as (`quote()`, `.dump`, `.mode insert`).
    pub fn quoted(&self) -> String {
        match self {
            Self::Null => "NULL".into(),
            Self::Integer(i) => i.to_string(),
            Self::Real(r) => {
                if r.is_infinite() {
                    if *r > 0.0 { "1e999" } else { "-1e999" }.into()
                } else {
                    format_real(*r)
                }
            }
            Self::Text(t) => format!("'{}'", t.replace('\'', "''")),
            Self::Blob(b) => format!("X'{}'", hex(b)),
        }
    }
}
/// Saturating conversion SQLite applies for CAST(real AS INTEGER).
pub fn real_to_int(r: f64) -> i64 {
    if r.is_nan() {
        0
    } else if r >= 9_223_372_036_854_775_808.0 {
        i64::MAX
    } else if r <= -9_223_372_036_854_775_808.0 {
        i64::MIN
    } else {
        r as i64
    }
}

/// Parse the longest numeric prefix of `text` (after leading spaces).
pub fn numeric_prefix(text: &str) -> Value {
    let t = text.trim_start();
    let b = t.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut int_end = i;
    let mut is_real = false;
    if i < b.len() && b[i] == b'.' {
        let mut j = i + 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 || i > digits_start {
            is_real = true;
            i = j;
        }
    }
    if i == digits_start {
        return Value::Integer(0);
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            is_real = true;
            i = j;
        }
    }
    if !is_real {
        int_end = int_end.max(digits_start);
        if let Ok(v) = t[..int_end].parse::<i64>() {
            return Value::Integer(v);
        }
    }
    let r: f64 = t[..i].parse().unwrap_or(0.0);
    Value::real(r)
}
/// Whether `text` is, in its entirety (ignoring surrounding spaces), a number; and if so
/// the value text affinity conversion would produce.
pub fn exact_number(text: &str) -> Option<Value> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let b = t.as_bytes();
    let mut i = 0;
    if b[0] == b'+' || b[0] == b'-' {
        i = 1;
    }
    let mut digits = 0;
    let mut real = false;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        digits += 1;
    }
    if i < b.len() && b[i] == b'.' {
        real = true;
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        real = true;
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
    }
    if i != b.len() {
        return None;
    }
    if !real {
        if let Ok(v) = t.parse::<i64>() {
            return Some(Value::Integer(v));
        }
    }
    let r: f64 = t.parse().ok()?;
    Some(Value::real(r))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Affinity {
    Integer,
    Text,
    #[default]
    Blob,
    Real,
    Numeric,
}
impl Affinity {
    /// Column affinity from a declared type, by SQLite's five rules in order.
    pub fn from_type(declared: &str) -> Self {
        let t = declared.to_ascii_uppercase();
        if t.contains("INT") {
            Self::Integer
        } else if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") {
            Self::Text
        } else if t.contains("BLOB") || t.trim().is_empty() {
            Self::Blob
        } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
            Self::Real
        } else {
            Self::Numeric
        }
    }
    /// Convert a value being stored in (or compared with) a column of this affinity.
    pub fn apply(self, v: Value) -> Value {
        match self {
            Self::Blob => v,
            Self::Text => match v {
                Value::Integer(_) | Value::Real(_) => Value::Text(v.to_text()),
                other => other,
            },
            Self::Numeric | Self::Integer => match v {
                Value::Text(ref t) => match exact_number(t) {
                    Some(Value::Real(r)) => real_as_integer(r).unwrap_or(Value::Real(r)),
                    Some(n) => n,
                    None => v,
                },
                Value::Real(r) => real_as_integer(r).unwrap_or(v),
                other => other,
            },
            Self::Real => match v {
                Value::Text(ref t) => match exact_number(t) {
                    Some(Value::Integer(i)) => Value::Real(i as f64),
                    Some(n) => n,
                    None => v,
                },
                Value::Integer(i) => Value::Real(i as f64),
                other => other,
            },
        }
    }
    pub fn numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Real | Self::Numeric)
    }
}
/// A real with no fractional part that fits an integer is stored as one under
/// INTEGER/NUMERIC affinity.
fn real_as_integer(r: f64) -> Option<Value> {
    if r == r.trunc() && r.abs() < 9.007_199_254_740_992e15 {
        Some(Value::Integer(r as i64))
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Collation {
    #[default]
    Binary,
    NoCase,
    RTrim,
}
impl Collation {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "BINARY" => Some(Self::Binary),
            "NOCASE" => Some(Self::NoCase),
            "RTRIM" => Some(Self::RTrim),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Binary => "BINARY",
            Self::NoCase => "NOCASE",
            Self::RTrim => "RTRIM",
        }
    }
    pub fn compare_text(self, a: &str, b: &str) -> Ordering {
        match self {
            Self::Binary => a.as_bytes().cmp(b.as_bytes()),
            Self::NoCase => a
                .bytes()
                .map(|c| c.to_ascii_lowercase())
                .cmp(b.bytes().map(|c| c.to_ascii_lowercase())),
            Self::RTrim => a.trim_end_matches(' ').cmp(b.trim_end_matches(' ')),
        }
    }
}

/// Exact comparison of an integer with a double.
pub fn int_real_cmp(i: i64, r: f64) -> Ordering {
    if r < -9_223_372_036_854_775_808.0 {
        return Ordering::Greater;
    }
    if r >= 9_223_372_036_854_775_808.0 {
        return Ordering::Less;
    }
    let t = r.trunc();
    let ti = t as i64;
    match i.cmp(&ti) {
        Ordering::Equal => {
            let frac = r - t;
            if frac > 0.0 {
                Ordering::Less
            } else if frac < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        other => other,
    }
}

/// SQLite's total order for sorting and comparison: NULL < numbers < text < blob.
pub fn compare(a: &Value, b: &Value, collation: Collation) -> Ordering {
    use Value::*;
    let class = |v: &Value| match v {
        Null => 0,
        Integer(_) | Real(_) => 1,
        Text(_) => 2,
        Blob(_) => 3,
    };
    match (a, b) {
        (Integer(x), Integer(y)) => x.cmp(y),
        (Real(x), Real(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Integer(x), Real(y)) => int_real_cmp(*x, *y),
        (Real(x), Integer(y)) => int_real_cmp(*y, *x).reverse(),
        (Text(x), Text(y)) => collation.compare_text(x, y),
        (Blob(x), Blob(y)) => x.cmp(y),
        _ => class(a).cmp(&class(b)),
    }
}

/// An index or grouping key with SQLite ordering built in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key(pub Vec<Value>);
impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.0.iter().zip(&other.0) {
            let o = compare(a, b, Collation::Binary);
            if o != Ordering::Equal {
                return o;
            }
        }
        self.0.len().cmp(&other.0.len())
    }
}
impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
/// Fold a value by a collation so binary comparison of folded keys equals collated
/// comparison of the originals.
pub fn fold(v: Value, collation: Collation) -> Value {
    match (v, collation) {
        (Value::Text(t), Collation::NoCase) => Value::Text(t.to_ascii_lowercase()),
        (Value::Text(t), Collation::RTrim) => Value::Text(t.trim_end_matches(' ').to_owned()),
        (v, _) => v,
    }
}
/// Key equality for DISTINCT, GROUP BY, UNION and IN: NULLs group together.
pub fn same(a: &Value, b: &Value) -> bool {
    compare(a, b, Collation::Binary) == Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reals_print_like_sqlite() {
        assert_eq!(format_real(1.0), "1.0");
        assert_eq!(format_real(0.1), "0.1");
        assert_eq!(format_real(2.5), "2.5");
        assert_eq!(format_real(1e20), "1.0e+20");
        assert_eq!(format_real(1.0 / 3.0), "0.333333333333333");
        assert_eq!(format_real(-0.5), "-0.5");
    }
    #[test]
    fn affinity_converts_like_sqlite() {
        assert_eq!(Affinity::from_type("VARCHAR(20)"), Affinity::Text);
        assert_eq!(Affinity::from_type("BIGINT"), Affinity::Integer);
        assert_eq!(Affinity::from_type("DECIMAL(10,2)"), Affinity::Numeric);
        assert_eq!(Affinity::from_type(""), Affinity::Blob);
        assert_eq!(
            Affinity::Integer.apply(Value::Text("42".into())),
            Value::Integer(42)
        );
        assert_eq!(
            Affinity::Numeric.apply(Value::Text("3.0".into())),
            Value::Integer(3)
        );
        assert_eq!(
            Affinity::Integer.apply(Value::Text("4x".into())),
            Value::Text("4x".into())
        );
        assert_eq!(
            Affinity::Text.apply(Value::Real(1.5)),
            Value::Text("1.5".into())
        );
        assert_eq!(Affinity::Real.apply(Value::Integer(2)), Value::Real(2.0));
    }
    #[test]
    fn numbers_compare_exactly_across_types() {
        assert_eq!(
            compare(&Value::Integer(1), &Value::Real(1.5), Collation::Binary),
            Ordering::Less
        );
        assert_eq!(
            compare(&Value::Integer(2), &Value::Real(2.0), Collation::Binary),
            Ordering::Equal
        );
        assert_eq!(
            compare(&Value::Null, &Value::Integer(0), Collation::Binary),
            Ordering::Less
        );
        assert_eq!(
            compare(
                &Value::Integer(9),
                &Value::Text("1".into()),
                Collation::Binary
            ),
            Ordering::Less
        );
        assert_eq!(numeric_prefix("12abc"), Value::Integer(12));
        assert_eq!(numeric_prefix(" 1.5e2x"), Value::Real(150.0));
        assert_eq!(numeric_prefix("abc"), Value::Integer(0));
    }
    #[test]
    fn values_round_trip_through_json() {
        for v in [
            Value::Null,
            Value::Integer(-7),
            Value::Real(1.0),
            Value::Real(f64::INFINITY),
            Value::Text("x".into()),
            Value::Blob(vec![0, 255]),
        ] {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&json).unwrap(), v, "{json}");
        }
    }
}
