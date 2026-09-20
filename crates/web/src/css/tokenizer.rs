//! CSS Syntax Level 3 tokenizer (https://www.w3.org/TR/css-syntax-3/#tokenization).
//! Input is preprocessed (CRLF/CR/FF to LF, NUL to U+FFFD), comments are dropped, and
//! every token of the specification is produced, including the bad-string and bad-url
//! recovery tokens. Numbers keep their source text next to the parsed `Number`.

use super::token::{Number, Token};

const REPLACEMENT: char = '\u{FFFD}';

/// Runs the input preprocessing step: newlines normalised to LF, NUL replaced.
pub fn preprocess(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{C}' => out.push('\n'),
            '\0' => out.push(REPLACEMENT),
            c => out.push(c),
        }
    }
    out
}

/// Tokenizes a whole stylesheet or fragment. Comments are removed.
pub fn tokenize(src: &str) -> Vec<Token> {
    let text = preprocess(src);
    let mut t = Tokenizer { chars: text.chars().collect(), pos: 0 };
    let mut out = Vec::new();
    while let Some(tok) = t.next_token() {
        out.push(tok);
    }
    out
}

pub fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c as u32 >= 0x80
}

pub fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

fn is_whitespace(c: char) -> bool {
    c == '\n' || c == '\t' || c == ' '
}

fn is_non_printable(c: char) -> bool {
    let u = c as u32;
    u <= 0x08 || u == 0x0B || (0x0E..=0x1F).contains(&u) || u == 0x7F
}

/// Would the pair `(a, b)` start a valid escape? (`a` is the backslash; EOF after it
/// is valid and decodes to U+FFFD.)
fn valid_escape(a: Option<char>, b: Option<char>) -> bool {
    a == Some('\\') && b != Some('\n')
}

/// Whether the three code points would start an identifier.
pub fn would_start_ident(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
    match a {
        Some('-') => match b {
            Some(x) if is_name_start(x) || x == '-' => true,
            _ => valid_escape(b, c),
        },
        Some(x) if is_name_start(x) => true,
        Some('\\') => valid_escape(a, b),
        _ => false,
    }
}

fn would_start_number(a: Option<char>, b: Option<char>, c: Option<char>) -> bool {
    match a {
        Some('+') | Some('-') => match b {
            Some(x) if x.is_ascii_digit() => true,
            Some('.') => matches!(c, Some(x) if x.is_ascii_digit()),
            _ => false,
        },
        Some('.') => matches!(b, Some(x) if x.is_ascii_digit()),
        Some(x) => x.is_ascii_digit(),
        None => false,
    }
}

struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn peek(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }
    fn next(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }
    fn reconsume(&mut self) {
        self.pos -= 1;
    }

    fn consume_comments(&mut self) {
        while self.peek(0) == Some('/') && self.peek(1) == Some('*') {
            self.pos += 2;
            loop {
                match self.next() {
                    None => return,
                    Some('*') if self.peek(0) == Some('/') => {
                        self.pos += 1;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        self.consume_comments();
        let c = self.next()?;
        Some(match c {
            c if is_whitespace(c) => {
                while matches!(self.peek(0), Some(x) if is_whitespace(x)) {
                    self.pos += 1;
                }
                Token::Whitespace
            }
            '"' | '\'' => self.consume_string(c),
            '#' => {
                let (a, b, c2) = (self.peek(0), self.peek(1), self.peek(2));
                if matches!(a, Some(x) if is_name_char(x)) || valid_escape(a, b) {
                    let id = would_start_ident(a, b, c2);
                    let value = self.consume_name();
                    Token::Hash { value, id }
                } else {
                    Token::Delim('#')
                }
            }
            '(' => Token::OpenParen,
            ')' => Token::CloseParen,
            '+' => {
                self.reconsume();
                if would_start_number(self.peek(0), self.peek(1), self.peek(2)) {
                    self.consume_numeric()
                } else {
                    self.pos += 1;
                    Token::Delim('+')
                }
            }
            ',' => Token::Comma,
            '-' => {
                self.reconsume();
                if would_start_number(self.peek(0), self.peek(1), self.peek(2)) {
                    self.consume_numeric()
                } else if self.peek(1) == Some('-') && self.peek(2) == Some('>') {
                    self.pos += 3;
                    Token::Cdc
                } else if would_start_ident(self.peek(0), self.peek(1), self.peek(2)) {
                    self.consume_ident_like()
                } else {
                    self.pos += 1;
                    Token::Delim('-')
                }
            }
            '.' => {
                self.reconsume();
                if would_start_number(self.peek(0), self.peek(1), self.peek(2)) {
                    self.consume_numeric()
                } else {
                    self.pos += 1;
                    Token::Delim('.')
                }
            }
            ':' => Token::Colon,
            ';' => Token::Semicolon,
            '<' => {
                if self.peek(0) == Some('!') && self.peek(1) == Some('-') && self.peek(2) == Some('-') {
                    self.pos += 3;
                    Token::Cdo
                } else {
                    Token::Delim('<')
                }
            }
            '@' => {
                if would_start_ident(self.peek(0), self.peek(1), self.peek(2)) {
                    Token::AtKeyword(self.consume_name())
                } else {
                    Token::Delim('@')
                }
            }
            '[' => Token::OpenSquare,
            '\\' => {
                if valid_escape(Some('\\'), self.peek(0)) {
                    self.reconsume();
                    self.consume_ident_like()
                } else {
                    Token::Delim('\\')
                }
            }
            ']' => Token::CloseSquare,
            '{' => Token::OpenCurly,
            '}' => Token::CloseCurly,
            c if c.is_ascii_digit() => {
                self.reconsume();
                self.consume_numeric()
            }
            c if is_name_start(c) => {
                self.reconsume();
                self.consume_ident_like()
            }
            c => Token::Delim(c),
        })
    }

    fn consume_string(&mut self, quote: char) -> Token {
        let mut s = String::new();
        loop {
            match self.next() {
                None => return Token::String(s),
                Some(c) if c == quote => return Token::String(s),
                Some('\n') => {
                    self.reconsume();
                    return Token::BadString;
                }
                Some('\\') => match self.peek(0) {
                    None => {}
                    Some('\n') => {
                        self.pos += 1;
                    }
                    Some(_) => s.push(self.consume_escape()),
                },
                Some(c) => s.push(c),
            }
        }
    }

    /// The backslash has been consumed and the next code point is a valid escape.
    fn consume_escape(&mut self) -> char {
        match self.next() {
            None => REPLACEMENT,
            Some(c) if c.is_ascii_hexdigit() => {
                let mut v = c.to_digit(16).unwrap();
                let mut n = 1;
                while n < 6 {
                    match self.peek(0) {
                        Some(h) if h.is_ascii_hexdigit() => {
                            v = v * 16 + h.to_digit(16).unwrap();
                            self.pos += 1;
                            n += 1;
                        }
                        _ => break,
                    }
                }
                if matches!(self.peek(0), Some(w) if is_whitespace(w)) {
                    self.pos += 1;
                }
                if v == 0 || (0xD800..=0xDFFF).contains(&v) || v > 0x10FFFF {
                    REPLACEMENT
                } else {
                    char::from_u32(v).unwrap_or(REPLACEMENT)
                }
            }
            Some(c) => c,
        }
    }

    fn consume_name(&mut self) -> String {
        let mut s = String::new();
        loop {
            match self.next() {
                Some(c) if is_name_char(c) => s.push(c),
                Some('\\') if valid_escape(Some('\\'), self.peek(0)) => s.push(self.consume_escape()),
                Some(_) => {
                    self.reconsume();
                    return s;
                }
                None => return s,
            }
        }
    }

    fn consume_ident_like(&mut self) -> Token {
        let name = self.consume_name();
        if name.eq_ignore_ascii_case("url") && self.peek(0) == Some('(') {
            self.pos += 1;
            while matches!(self.peek(0), Some(w) if is_whitespace(w)) && matches!(self.peek(1), Some(w) if is_whitespace(w)) {
                self.pos += 1;
            }
            let quoted = match self.peek(0) {
                Some('"') | Some('\'') => true,
                Some(w) if is_whitespace(w) => matches!(self.peek(1), Some('"') | Some('\'')),
                _ => false,
            };
            if quoted {
                Token::Function(name)
            } else {
                self.consume_url()
            }
        } else if self.peek(0) == Some('(') {
            self.pos += 1;
            Token::Function(name)
        } else {
            Token::Ident(name)
        }
    }

    fn consume_url(&mut self) -> Token {
        let mut s = String::new();
        while matches!(self.peek(0), Some(w) if is_whitespace(w)) {
            self.pos += 1;
        }
        loop {
            match self.next() {
                Some(')') => return Token::Url(s),
                None => return Token::Url(s),
                Some(w) if is_whitespace(w) => {
                    while matches!(self.peek(0), Some(w) if is_whitespace(w)) {
                        self.pos += 1;
                    }
                    match self.peek(0) {
                        Some(')') => {
                            self.pos += 1;
                            return Token::Url(s);
                        }
                        None => return Token::Url(s),
                        _ => {
                            self.consume_bad_url_remnants();
                            return Token::BadUrl;
                        }
                    }
                }
                Some(c) if c == '"' || c == '\'' || c == '(' || is_non_printable(c) => {
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some('\\') => {
                    if valid_escape(Some('\\'), self.peek(0)) {
                        s.push(self.consume_escape());
                    } else {
                        self.consume_bad_url_remnants();
                        return Token::BadUrl;
                    }
                }
                Some(c) => s.push(c),
            }
        }
    }

    fn consume_bad_url_remnants(&mut self) {
        loop {
            match self.next() {
                Some(')') | None => return,
                Some('\\') if valid_escape(Some('\\'), self.peek(0)) => {
                    self.consume_escape();
                }
                _ => {}
            }
        }
    }

    fn consume_numeric(&mut self) -> Token {
        let mut text = String::new();
        if matches!(self.peek(0), Some('+') | Some('-')) {
            text.push(self.next().unwrap());
        }
        while matches!(self.peek(0), Some(d) if d.is_ascii_digit()) {
            text.push(self.next().unwrap());
        }
        if self.peek(0) == Some('.') && matches!(self.peek(1), Some(d) if d.is_ascii_digit()) {
            text.push(self.next().unwrap());
            while matches!(self.peek(0), Some(d) if d.is_ascii_digit()) {
                text.push(self.next().unwrap());
            }
        }
        if matches!(self.peek(0), Some('e') | Some('E')) {
            let exp_ok = match self.peek(1) {
                Some(d) if d.is_ascii_digit() => true,
                Some('+') | Some('-') => matches!(self.peek(2), Some(d) if d.is_ascii_digit()),
                _ => false,
            };
            if exp_ok {
                text.push(self.next().unwrap());
                if matches!(self.peek(0), Some('+') | Some('-')) {
                    text.push(self.next().unwrap());
                }
                while matches!(self.peek(0), Some(d) if d.is_ascii_digit()) {
                    text.push(self.next().unwrap());
                }
            }
        }
        let value = Number::parse(&text).unwrap_or(Number::ZERO);
        if would_start_ident(self.peek(0), self.peek(1), self.peek(2)) {
            let unit = self.consume_name();
            Token::Dimension { text, value, unit }
        } else if self.peek(0) == Some('%') {
            self.pos += 1;
            Token::Percentage { text, value }
        } else {
            Token::Number { text, value }
        }
    }
}

/// CSSOM "serialize an identifier": escapes so that tokenizing gives the same ident.
pub fn serialize_identifier(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let u = c as u32;
        let control = (0x1..=0x1F).contains(&u) || u == 0x7F;
        let leading_digit = c.is_ascii_digit() && (i == 0 || (i == 1 && chars[0] == '-'));
        if u == 0 {
            out.push(REPLACEMENT);
        } else if control || leading_digit {
            out.push_str(&format!("\\{:x} ", u));
        } else if i == 0 && c == '-' && chars.len() == 1 {
            out.push_str("\\-");
        } else if u >= 0x80 || c == '-' || c == '_' || c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

/// CSSOM "serialize a string": double-quoted with escapes.
pub fn serialize_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        let u = c as u32;
        if u == 0 {
            out.push(REPLACEMENT);
        } else if (0x1..=0x1F).contains(&u) || u == 0x7F {
            out.push_str(&format!("\\{:x} ", u));
        } else if c == '"' || c == '\\' {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// Serializes one token back to CSS text (CSSOM `serialize a CSS component value`).
pub fn serialize_token(tok: &Token, out: &mut String) {
    match tok {
        Token::Ident(s) => out.push_str(&serialize_identifier(s)),
        Token::Function(s) => {
            out.push_str(&serialize_identifier(s));
            out.push('(');
        }
        Token::AtKeyword(s) => {
            out.push('@');
            out.push_str(&serialize_identifier(s));
        }
        Token::Hash { value, id } => {
            out.push('#');
            if *id {
                out.push_str(&serialize_identifier(value));
            } else {
                for c in value.chars() {
                    if is_name_char(c) {
                        out.push(c);
                    } else {
                        out.push('\\');
                        out.push(c);
                    }
                }
            }
        }
        Token::String(s) => out.push_str(&serialize_string(s)),
        Token::BadString => out.push_str("\"\n"),
        Token::Url(s) => {
            out.push_str("url(");
            out.push_str(&serialize_string(s));
            out.push(')');
        }
        Token::BadUrl => out.push_str("url(bad url)"),
        Token::Delim(c) => {
            if *c == '\\' {
                out.push_str("\\\n");
            } else {
                out.push(*c);
            }
        }
        Token::Number { text, .. } => out.push_str(text),
        Token::Percentage { text, .. } => {
            out.push_str(text);
            out.push('%');
        }
        Token::Dimension { text, unit, .. } => {
            out.push_str(text);
            // A unit that could be read as an exponent (`e-2`, `e5`, or a bare `e` that a
            // following signed number would join) has its `e` escaped; after the escape
            // the rest of the name follows literally.
            let needs_escape = unit.starts_with(['e', 'E']) && (unit.len() == 1 || unit[1..].starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+'));
            if needs_escape {
                out.push_str(&format!("\\{:x} ", unit.chars().next().unwrap() as u32));
                for c in unit[1..].chars() {
                    if is_name_char(c) {
                        out.push(c);
                    } else {
                        out.push('\\');
                        out.push(c);
                    }
                }
            } else {
                out.push_str(&serialize_identifier(unit));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn idents(src: &str) -> Vec<String> {
        tokenize(src)
            .into_iter()
            .filter_map(|t| match t {
                Token::Ident(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn preprocessing() {
        assert_eq!(preprocess("a\r\nb\rc\x0Cd\0e"), "a\nb\nc\nd\u{FFFD}e");
        assert_eq!(tokenize("a\r\n\r\nb"), vec![Token::Ident("a".into()), Token::Whitespace, Token::Ident("b".into())]);
    }

    #[test]
    fn comments_are_dropped_and_unterminated_ok() {
        assert_eq!(tokenize("/* x */a/**/b/* eof"), vec![Token::Ident("a".into()), Token::Ident("b".into())]);
        assert_eq!(tokenize("/*/*///** /* **/*//* "), vec![Token::Delim('/'), Token::Delim('*'), Token::Delim('/')]);
    }

    #[test]
    fn escapes() {
        assert_eq!(idents("\\30red \\00030 red \\30\r\nred"), vec!["0red", "0red", "0red"]);
        assert_eq!(idents("\\0000000red \\1100000red \\D800red"), vec!["\u{FFFD}0red", "\u{FFFD}0red", "\u{FFFD}red"]);
        assert_eq!(idents("\\red \\.red \\ red"), vec!["red", ".red", " red"]);
        assert_eq!(idents("\\376\\37 6\\000376\\0000376\\"), vec!["Ͷ76Ͷ76\u{FFFD}"]);
        assert_eq!(tokenize("\\\nred"), vec![Token::Delim('\\'), Token::Whitespace, Token::Ident("red".into())]);
        assert_eq!(idents("\\-red -\\-red --red"), vec!["-red", "--red", "--red"]);
    }

    #[test]
    fn strings_and_bad_strings() {
        assert_eq!(tokenize("'a\\\nb'"), vec![Token::String("ab".into())]);
        assert_eq!(tokenize("\"Lo\\rem \\130 ps\\u m\""), vec![Token::String("Lorem İpsu m".into())]);
        assert_eq!(tokenize("'a\nb"), vec![Token::BadString, Token::Whitespace, Token::Ident("b".into())]);
        assert_eq!(tokenize("'eof"), vec![Token::String("eof".into())]);
        assert_eq!(tokenize("''"), vec![Token::String(String::new())]);
    }

    #[test]
    fn urls() {
        assert_eq!(tokenize("url(foo)"), vec![Token::Url("foo".into())]);
        assert_eq!(tokenize("URL( \t foo \n)"), vec![Token::Url("foo".into())]);
        assert_eq!(tokenize("url()"), vec![Token::Url(String::new())]);
        assert_eq!(tokenize("url("), vec![Token::Url(String::new())]);
        assert_eq!(tokenize("url(a b) x"), vec![Token::BadUrl, Token::Whitespace, Token::Ident("x".into())]);
        assert_eq!(tokenize("url(a(b)"), vec![Token::BadUrl]);
        assert_eq!(tokenize("url(a\\(b)"), vec![Token::Url("a(b".into())]);
        assert_eq!(tokenize("url(a'b)"), vec![Token::BadUrl]);
        assert_eq!(tokenize("url(a\\\nb) c"), vec![Token::BadUrl, Token::Whitespace, Token::Ident("c".into())]);
        assert_eq!(tokenize("url(a\\a b)"), vec![Token::Url("a\nb".into())]);
        assert_eq!(tokenize("url(\x01)"), vec![Token::BadUrl]);
        assert_eq!(tokenize("url( 'x' )"), vec![Token::Function("url".into()), Token::Whitespace, Token::String("x".into()), Token::Whitespace, Token::CloseParen]);
        assert_eq!(tokenize("url('x')"), vec![Token::Function("url".into()), Token::String("x".into()), Token::CloseParen]);
        assert_eq!(tokenize("url\\ (foo)"), vec![Token::Function("url ".into()), Token::Ident("foo".into()), Token::CloseParen]);
        assert_eq!(tokenize("url (foo)"), vec![Token::Ident("url".into()), Token::Whitespace, Token::OpenParen, Token::Ident("foo".into()), Token::CloseParen]);
        assert_eq!(tokenize("url(a\\"), vec![Token::Url("a\u{FFFD}".into())]);
    }

    #[test]
    fn numbers_percentages_dimensions() {
        let n = |t: &str, v: i64, int: bool| Token::Number { text: t.into(), value: Number { micro: v, int } };
        assert_eq!(tokenize("12 +34 -45 .67 +.89 -.01"), vec![n("12", 12_000_000, true), Token::Whitespace, n("+34", 34_000_000, true), Token::Whitespace, n("-45", -45_000_000, true), Token::Whitespace, n(".67", 670_000, false), Token::Whitespace, n("+.89", 890_000, false), Token::Whitespace, n("-.01", -10_000, false)]);
        assert_eq!(tokenize("12e2"), vec![n("12e2", 1_200_000_000, false)]);
        assert_eq!(tokenize("-45E-0"), vec![n("-45E-0", -45_000_000, false)]);
        assert_eq!(tokenize("3."), vec![n("3", 3_000_000, true), Token::Delim('.')]);
        assert_eq!(tokenize("3e-2.1"), vec![n("3e-2", 30_000, false), n(".1", 100_000, false)]);
        assert_eq!(tokenize("3\\65-2"), vec![Token::Dimension { text: "3".into(), value: Number::from_i64(3), unit: "e-2".into() }]);
        assert_eq!(tokenize("12%"), vec![Token::Percentage { text: "12".into(), value: Number::from_i64(12) }]);
        assert_eq!(tokenize("12\\%"), vec![Token::Dimension { text: "12".into(), value: Number::from_i64(12), unit: "%".into() }]);
        assert_eq!(tokenize("1.5px"), vec![Token::Dimension { text: "1.5".into(), value: Number { micro: 1_500_000, int: false }, unit: "px".into() }]);
        assert_eq!(tokenize("12-0red"), vec![n("12", 12_000_000, true), Token::Dimension { text: "-0".into(), value: Number::from_i64(0), unit: "red".into() }]);
        assert_eq!(tokenize("12.0-red"), vec![Token::Dimension { text: "12.0".into(), value: Number { micro: 12_000_000, int: false }, unit: "-red".into() }]);
        assert_eq!(tokenize("+ 2"), vec![Token::Delim('+'), Token::Whitespace, n("2", 2_000_000, true)]);
    }

    #[test]
    fn hashes_at_keywords_functions() {
        assert_eq!(tokenize("#red0 #0red #-Red #.red"), vec![Token::Hash { value: "red0".into(), id: true }, Token::Whitespace, Token::Hash { value: "0red".into(), id: false }, Token::Whitespace, Token::Hash { value: "-Red".into(), id: true }, Token::Whitespace, Token::Delim('#'), Token::Delim('.'), Token::Ident("red".into())]);
        assert_eq!(tokenize("@media @0media @-\\-x"), vec![Token::AtKeyword("media".into()), Token::Whitespace, Token::Delim('@'), Token::Dimension { text: "0".into(), value: Number::ZERO, unit: "media".into() }, Token::Whitespace, Token::AtKeyword("--x".into())]);
        assert_eq!(tokenize("rgba(0rgba() rgba ()"), vec![Token::Function("rgba".into()), Token::Dimension { text: "0".into(), value: Number::ZERO, unit: "rgba".into() }, Token::OpenParen, Token::CloseParen, Token::Whitespace, Token::Ident("rgba".into()), Token::Whitespace, Token::OpenParen, Token::CloseParen]);
    }

    #[test]
    fn cdo_cdc_and_delims() {
        assert_eq!(tokenize("<!-- --> <!- -- ->"), vec![Token::Cdo, Token::Whitespace, Token::Cdc, Token::Whitespace, Token::Delim('<'), Token::Delim('!'), Token::Delim('-'), Token::Whitespace, Token::Ident("--".into()), Token::Whitespace, Token::Delim('-'), Token::Delim('>')]);
        assert_eq!(tokenize("red-->"), vec![Token::Ident("red--".into()), Token::Delim('>')]);
        assert_eq!(tokenize("~=|=^=$=*=||"), "~=|=^=$=*=||".chars().map(Token::Delim).collect::<Vec<_>>());
        assert_eq!(tokenize("{}[]();:,"), vec![Token::OpenCurly, Token::CloseCurly, Token::OpenSquare, Token::CloseSquare, Token::OpenParen, Token::CloseParen, Token::Semicolon, Token::Colon, Token::Comma]);
        assert_eq!(tokenize("\u{7F}\u{80}\u{81}"), vec![Token::Delim('\u{7F}'), Token::Ident("\u{80}\u{81}".into())]);
    }

    #[test]
    fn serialization_round_trips() {
        for s in ["a", "-a", "--a", "1a", "-1a", "a b", "a\"b", "é", "-", "a\u{1}b", "a.b"] {
            let ser = serialize_identifier(s);
            assert_eq!(tokenize(&ser), vec![Token::Ident(s.to_owned())], "{s:?} -> {ser:?}");
        }
        for s in ["", "a\"b", "a\\b", "line\nbreak", "é"] {
            let ser = serialize_string(s);
            assert_eq!(tokenize(&ser), vec![Token::String(s.to_owned())], "{s:?} -> {ser:?}");
        }
        for src in ["3\\65-2", "3\\65 5", "3\\45+1", "1e3", "10px", "50%", "#abc", "#0a", "url(\"x y\")", "a(", "@x", "-\\31 a", "\\-", "a\\.b"] {
            let toks = tokenize(src);
            let mut out = String::new();
            for t in &toks {
                serialize_token(t, &mut out);
            }
            assert_eq!(tokenize(&out), toks, "{src:?} -> {out:?}");
        }
    }
}
