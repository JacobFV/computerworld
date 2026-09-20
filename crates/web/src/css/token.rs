//! CSS Syntax Level 3 tokens and component values. Numbers are kept as their decimal
//! text plus a parsed fixed-point value, so no `f64` reaches layout.

/// A CSS number: the integer part and up to six decimal digits, sign included, as a
/// value in millionths (`1.5` is 1_500_000). `int` is set when the token had no `.`/`e`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Number {
    pub micro: i64,
    pub int: bool,
}

impl Number {
    pub const ZERO: Number = Number { micro: 0, int: true };
    pub fn from_i64(v: i64) -> Number {
        Number { micro: v * 1_000_000, int: true }
    }
    /// Parses `[+-]?digits[.digits][e[+-]digits]`; `None` if not a number.
    pub fn parse(s: &str) -> Option<Number> {
        let b = s.as_bytes();
        let mut i = 0;
        let mut neg = false;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            neg = b[i] == b'-';
            i += 1;
        }
        let mut int_part: i64 = 0;
        let mut digits = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            int_part = int_part.saturating_mul(10).saturating_add((b[i] - b'0') as i64);
            i += 1;
            digits += 1;
        }
        let mut frac: i64 = 0;
        let mut frac_digits = 0;
        let mut is_int = true;
        if i < b.len() && b[i] == b'.' {
            is_int = false;
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                if frac_digits < 6 {
                    frac = frac * 10 + (b[i] - b'0') as i64;
                    frac_digits += 1;
                }
                i += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return None;
        }
        let mut exp: i32 = 0;
        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            let mut j = i + 1;
            let mut eneg = false;
            if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                eneg = b[j] == b'-';
                j += 1;
            }
            let mut e = 0i32;
            let mut ed = 0;
            while j < b.len() && b[j].is_ascii_digit() {
                e = e.saturating_mul(10).saturating_add((b[j] - b'0') as i32);
                j += 1;
                ed += 1;
            }
            if ed > 0 {
                is_int = false;
                exp = if eneg { -e } else { e };
                i = j;
            }
        }
        if i != b.len() {
            return None;
        }
        while frac_digits < 6 {
            frac *= 10;
            frac_digits += 1;
        }
        let mut micro = int_part.saturating_mul(1_000_000).saturating_add(frac);
        match exp.cmp(&0) {
            std::cmp::Ordering::Greater => {
                for _ in 0..exp.min(18) {
                    micro = micro.saturating_mul(10);
                }
            }
            std::cmp::Ordering::Less => {
                for _ in 0..(-exp).min(18) {
                    micro /= 10;
                }
            }
            std::cmp::Ordering::Equal => {}
        }
        Some(Number { micro: if neg { -micro } else { micro }, int: is_int })
    }
    pub fn to_f64(self) -> f64 {
        self.micro as f64 / 1e6
    }
    /// Value in thousandths, rounded half away from zero.
    pub fn milli(self) -> i32 {
        let v = self.micro;
        let r = if v >= 0 { (v + 500) / 1000 } else { (v - 500) / 1000 };
        r.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    pub fn round(self) -> i32 {
        let v = self.micro;
        let r = if v >= 0 { (v + 500_000) / 1_000_000 } else { (v - 500_000) / 1_000_000 };
        r.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }
    pub fn is_zero(self) -> bool {
        self.micro == 0
    }
    pub fn is_negative(self) -> bool {
        self.micro < 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Ident(String),
    /// `name(`; the name without the paren.
    Function(String),
    /// `@name`.
    AtKeyword(String),
    /// `#name`; `id` is true when it is a valid identifier (an id selector).
    Hash { value: String, id: bool },
    String(String),
    BadString,
    /// `url(...)` unquoted form, value already unescaped.
    Url(String),
    BadUrl,
    Delim(char),
    Number { text: String, value: Number },
    Percentage { text: String, value: Number },
    Dimension { text: String, value: Number, unit: String },
    Whitespace,
    /// `<!--`
    Cdo,
    /// `-->`
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
}

/// A parsed component value: a token, or a block/function with its contents.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComponentValue {
    Token(Token),
    /// `name(` ... `)`
    Function { name: String, args: Vec<ComponentValue> },
    /// `{ ... }`, `[ ... ]` or `( ... )`.
    Block { open: Token, contents: Vec<ComponentValue> },
}

impl ComponentValue {
    pub fn is_whitespace(&self) -> bool {
        matches!(self, ComponentValue::Token(Token::Whitespace))
    }
    pub fn as_ident(&self) -> Option<&str> {
        match self {
            ComponentValue::Token(Token::Ident(s)) => Some(s),
            _ => None,
        }
    }
}

/// One declaration inside a style rule or a `style` attribute.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Declaration {
    /// Lower-cased, except custom properties which keep their case after `--`.
    pub name: String,
    /// Component values with leading and trailing whitespace trimmed.
    pub value: Vec<ComponentValue>,
    pub important: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(Number::parse("12").unwrap(), Number { micro: 12_000_000, int: true });
        assert_eq!(Number::parse("-1.5").unwrap().micro, -1_500_000);
        assert_eq!(Number::parse(".5").unwrap().micro, 500_000);
        assert_eq!(Number::parse("+0.25").unwrap().milli(), 250);
        assert_eq!(Number::parse("1e3").unwrap().micro, 1_000_000_000);
        assert_eq!(Number::parse("2.5e-1").unwrap().micro, 250_000);
        assert!(Number::parse("abc").is_none());
        assert!(Number::parse("1.").unwrap().int == false || Number::parse("1.").unwrap().micro == 1_000_000);
        assert_eq!(Number::parse("0.9999999").unwrap().micro, 999_999);
        assert_eq!(Number::parse("-0.5").unwrap().round(), -1);
        assert_eq!(Number::parse("0.5").unwrap().round(), 1);
    }
}
