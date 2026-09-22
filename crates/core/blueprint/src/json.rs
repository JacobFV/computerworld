//! JSON, read and written the way `JSON.stringify` writes it.
//!
//! `worlds/company-2026/world.json` is generated, checked into git and hashed by
//! the determinism corpus, so the bytes this writes have to match the bytes the
//! JavaScript generator wrote — down to the lowercase `\u001f`, the unescaped
//! `/`, and the raw UTF-8 for everything above ASCII. The parser is the other
//! half of that contract: it keeps every number as the text it was written with,
//! and refuses a literal that is not already in the shortest round-tripping form,
//! so a rewrite can never silently reformat someone's file.
use crate::node::{Map, Node};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for JsonError {}

/// One step of the path to a node, for the compaction predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

#[derive(Clone, Copy)]
pub enum Format<'a> {
    /// No whitespace at all: `{"a":1,"b":[1,2]}`.
    Compact,
    /// Two-space indent, `": "` after keys.
    Pretty,
    /// Two-space indent, except that any node the predicate accepts is written
    /// compactly. The predicate receives the path from the root to that node.
    PrettyExcept(&'a dyn Fn(&[Step<'_>]) -> bool),
}

/// Deep enough for anything a person writes, shallow enough that a hostile file
/// cannot walk the stack off its end: the reference world nests fourteen deep.
const MAX_DEPTH: usize = 512;

pub fn parse(text: &str) -> Result<Node, JsonError> {
    // A generator or an editor may have put a BOM in front; it is not a value.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        at: 0,
        line: 1,
    };
    let node = parser.value(0)?;
    parser.skip_whitespace();
    if parser.at < parser.bytes.len() {
        let found = parser.found();
        return parser.fail(format!("expected end of input, found {found}"));
    }
    Ok(node)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn fail<T>(&self, message: impl Into<String>) -> Result<T, JsonError> {
        Err(JsonError {
            line: self.line,
            message: message.into(),
        })
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn at_digit(&self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9'))
    }

    /// What sits at the cursor, for an error message.
    fn found(&self) -> String {
        match self.peek() {
            None => "end of input".to_string(),
            Some(b) if b.is_ascii_graphic() => format!("`{}`", b as char),
            Some(b) => format!("byte 0x{b:02x}"),
        }
    }

    /// The source between two cursor positions. Every cut is made at an ASCII
    /// delimiter, so it always lands on a character boundary.
    fn slice(&self, from: usize, to: usize) -> &'a str {
        &self.text[from..to]
    }

    fn skip_whitespace(&mut self) {
        while let Some(b) = self.peek() {
            match b {
                b'\n' => {
                    self.line += 1;
                    self.at += 1;
                }
                b' ' | b'\t' | b'\r' => self.at += 1,
                _ => break,
            }
        }
    }

    fn expect(&mut self, byte: u8, what: &str) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.at += 1;
            return Ok(());
        }
        let found = self.found();
        self.fail(format!("expected {what}, found {found}"))
    }

    fn value(&mut self, depth: usize) -> Result<Node, JsonError> {
        if depth > MAX_DEPTH {
            return self.fail(format!("nested more than {MAX_DEPTH} deep"));
        }
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Node::String(self.string()?)),
            Some(b't') if self.rest().starts_with(b"true") => {
                self.at += 4;
                Ok(Node::Bool(true))
            }
            Some(b'f') if self.rest().starts_with(b"false") => {
                self.at += 5;
                Ok(Node::Bool(false))
            }
            Some(b'n') if self.rest().starts_with(b"null") => {
                self.at += 4;
                Ok(Node::Null)
            }
            // `NaN` and `Infinity` are JavaScript, not JSON, and a file that has
            // them has no faithful representation here at all.
            Some(b'N') | Some(b'I') => self.fail("NaN and Infinity are not JSON"),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            _ => {
                let found = self.found();
                self.fail(format!("expected a value, found {found}"))
            }
        }
    }

    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.at..]
    }

    fn object(&mut self, depth: usize) -> Result<Node, JsonError> {
        self.at += 1; // `{`
        let mut map = Map::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Node::Map(map));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                let found = self.found();
                return self.fail(format!("expected a quoted key, found {found}"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':', "`:` after a key")?;
            let value = self.value(depth + 1)?;
            // A config format that dropped one of two same-named keys would hide
            // the mistake in the file it wrote back out.
            if map.contains_key(&key) {
                return self.fail(format!("duplicate key `{key}`"));
            }
            map.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Node::Map(map));
                }
                _ => {
                    let found = self.found();
                    return self.fail(format!("expected `,` or `}}`, found {found}"));
                }
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Node, JsonError> {
        self.at += 1; // `[`
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Node::List(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Node::List(items));
                }
                _ => {
                    let found = self.found();
                    return self.fail(format!("expected `,` or `]`, found {found}"));
                }
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1; // `"`
        let mut out = String::new();
        let mut run = self.at; // start of the stretch that needs no unescaping
        loop {
            match self.peek() {
                None => return self.fail("unterminated string"),
                Some(b'"') => {
                    out.push_str(self.slice(run, self.at));
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    out.push_str(self.slice(run, self.at));
                    self.at += 1;
                    self.escape(&mut out)?;
                    run = self.at;
                }
                Some(b) if b < 0x20 => {
                    return self.fail(format!("unescaped control character 0x{b:02x} in a string"));
                }
                Some(_) => self.at += 1,
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let byte = match self.peek() {
            Some(b) => b,
            None => return self.fail("unterminated escape"),
        };
        self.at += 1;
        let ch = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(out),
            _ if byte.is_ascii_graphic() => {
                return self.fail(format!("unknown escape `\\{}`", byte as char));
            }
            _ => return self.fail(format!("unknown escape `\\` before byte 0x{byte:02x}")),
        };
        out.push(ch);
        Ok(())
    }

    fn unicode_escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let first = self.hex4()?;
        let code = match first {
            // A `String` cannot hold half a pair, so the low half must be here.
            0xd800..=0xdbff => {
                if !self.rest().starts_with(b"\\u") {
                    return self.fail(format!(
                        "`\\u{first:04x}` is half a surrogate pair with no low half after it"
                    ));
                }
                self.at += 2;
                let second = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&second) {
                    return self.fail(format!(
                        "`\\u{first:04x}` needs a low surrogate after it, found `\\u{second:04x}`"
                    ));
                }
                0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
            }
            0xdc00..=0xdfff => {
                return self.fail(format!(
                    "`\\u{first:04x}` is a low surrogate with no high half before it"
                ));
            }
            other => other,
        };
        match char::from_u32(code) {
            Some(ch) => out.push(ch),
            None => return self.fail(format!("`\\u{code:04x}` is not a character")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut code = 0u32;
        for _ in 0..4 {
            let digit = match self.peek() {
                Some(b) => match (b as char).to_digit(16) {
                    Some(d) => d,
                    None => {
                        let found = self.found();
                        return self.fail(format!("expected a hex digit, found {found}"));
                    }
                },
                None => return self.fail("expected four hex digits, found end of input"),
            };
            self.at += 1;
            code = code * 16 + digit;
        }
        Ok(code)
    }

    fn number(&mut self) -> Result<Node, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.at += 1;
                if self.at_digit() {
                    return self.fail("a number may not have a leading zero");
                }
            }
            Some(b'1'..=b'9') => {
                while self.at_digit() {
                    self.at += 1;
                }
            }
            _ => {
                let found = self.found();
                return self.fail(format!("expected a digit, found {found}"));
            }
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !self.at_digit() {
                let found = self.found();
                return self.fail(format!("expected a digit after `.`, found {found}"));
            }
            while self.at_digit() {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !self.at_digit() {
                let found = self.found();
                return self.fail(format!("expected a digit in the exponent, found {found}"));
            }
            while self.at_digit() {
                self.at += 1;
            }
        }
        let literal = self.slice(start, self.at);
        let value: f64 = match literal.parse() {
            Ok(value) => value,
            Err(_) => return self.fail(format!("`{literal}` is not a number")),
        };
        if !value.is_finite() {
            return self.fail(format!("`{literal}` is too large to be a finite number"));
        }
        // The writer emits this text verbatim, so anything the generator would
        // have written differently has to be refused here rather than quietly
        // rewritten on the next save.
        let canonical = canonical_number(value);
        if canonical != literal {
            return self.fail(format!(
                "`{literal}` is not in canonical form; write it `{canonical}`"
            ));
        }
        Ok(Node::Number(literal.to_string()))
    }
}

/// A number as JavaScript's `Number#toString` writes it: plain decimal for
/// magnitudes in `[1e-6, 1e21)` and an exponent outside that range. Rust's `{}`
/// agrees in the middle and disagrees at both ends, which is the whole reason
/// this exists.
fn canonical_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string(); // also `-0`, which JavaScript writes as `0`
    }
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_string();
    }
    let mut out = String::new();
    let magnitude = if value < 0.0 {
        out.push('-');
        -value
    } else {
        value
    };
    // `{:e}` gives the shortest round-tripping digits and a base-ten exponent,
    // which is exactly the `s`, `k` and `n` the ECMAScript rule is written in.
    let scientific = format!("{magnitude:e}");
    let (mantissa, exponent) = match scientific.split_once('e') {
        Some(split) => split,
        None => return scientific,
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let at = n as usize;
        out.push_str(&digits[..at]);
        out.push('.');
        out.push_str(&digits[at..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        if n - 1 < 0 {
            out.push('-');
        } else {
            out.push('+');
        }
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

pub fn write(node: &Node, format: Format<'_>) -> String {
    let mut out = String::new();
    match format {
        Format::Compact => write_compact(node, &mut out),
        Format::Pretty => write_pretty(node, 0, None, &mut Vec::new(), &mut out),
        Format::PrettyExcept(keep_compact) => {
            write_pretty(node, 0, Some(keep_compact), &mut Vec::new(), &mut out);
        }
    }
    out
}

type Predicate<'a> = &'a dyn Fn(&[Step<'_>]) -> bool;

fn write_pretty<'a>(
    node: &'a Node,
    depth: usize,
    keep_compact: Option<Predicate<'_>>,
    path: &mut Vec<Step<'a>>,
    out: &mut String,
) {
    if let Some(predicate) = keep_compact {
        if predicate(path) {
            write_compact(node, out);
            return;
        }
    }
    match node {
        Node::List(items) if !items.is_empty() => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('\n');
                indent(depth + 1, out);
                path.push(Step::Index(index));
                write_pretty(item, depth + 1, keep_compact, path, out);
                path.pop();
            }
            out.push('\n');
            indent(depth, out);
            out.push(']');
        }
        Node::Map(map) if !map.is_empty() => {
            out.push('{');
            for (index, (key, value)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('\n');
                indent(depth + 1, out);
                write_string(key, out);
                out.push_str(": ");
                path.push(Step::Key(key));
                write_pretty(value, depth + 1, keep_compact, path, out);
                path.pop();
            }
            out.push('\n');
            indent(depth, out);
            out.push('}');
        }
        other => write_compact(other, out),
    }
}

fn write_compact(node: &Node, out: &mut String) {
    match node {
        Node::Null => out.push_str("null"),
        Node::Bool(true) => out.push_str("true"),
        Node::Bool(false) => out.push_str("false"),
        Node::Number(text) => out.push_str(text),
        Node::String(text) => write_string(text, out),
        Node::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        Node::Map(map) => {
            out.push('{');
            for (index, (key, value)) in map.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_compact(value, out);
            }
            out.push('}');
        }
    }
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// `JSON.stringify`'s `QuoteJSONString`: the five short escapes, lowercase
/// `\u00xx` for the rest of C0, and every other character — `/`, DEL, U+2028,
/// all of it — straight through as UTF-8.
fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{9}' => out.push_str("\\t"),
            '\u{a}' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\u{d}' => out.push_str("\\r"),
            ch if (ch as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Node {
        parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn rejected(text: &str) -> String {
        match parse(text) {
            Ok(node) => panic!("{text} parsed as {node}"),
            Err(error) => error.message,
        }
    }

    fn compact(node: &Node) -> String {
        write(node, Format::Compact)
    }

    fn pretty(node: &Node) -> String {
        write(node, Format::Pretty)
    }

    #[test]
    fn scalars_round_trip() {
        assert_eq!(parsed("null"), Node::Null);
        assert_eq!(parsed(" true "), Node::Bool(true));
        assert_eq!(parsed("false"), Node::Bool(false));
        assert_eq!(parsed("\"hi\""), Node::string("hi"));
        assert_eq!(parsed("12"), Node::Number("12".into()));
        assert_eq!(compact(&parsed("null")), "null");
        assert_eq!(compact(&parsed("true")), "true");
        assert_eq!(compact(&parsed("false")), "false");
        assert_eq!(compact(&parsed("-3.25")), "-3.25");
    }

    #[test]
    fn a_bom_and_trailing_whitespace_are_allowed() {
        assert_eq!(parsed("\u{feff}{}"), Node::Map(Map::new()));
        assert_eq!(parsed("1 \n\t\r"), Node::Number("1".into()));
        assert_eq!(rejected("1 2"), "expected end of input, found `2`");
        assert_eq!(rejected("{} {}"), "expected end of input, found `{`");
    }

    #[test]
    fn every_escape_is_read_and_written_back() {
        let node = parsed(r#""q\" b\\ s\/ b\b f\f n\n r\r t\t u\u0041""#);
        assert_eq!(
            node.as_str().unwrap(),
            "q\" b\\ s/ b\u{8} f\u{c} n\n r\r t\t uA"
        );
        assert_eq!(
            compact(&node),
            "\"q\\\" b\\\\ s/ b\\b f\\f n\\n r\\r t\\t uA\""
        );
    }

    #[test]
    fn control_characters_are_lowercase_u_escapes_and_slash_and_del_are_not_escaped() {
        assert_eq!(compact(&Node::string("\u{1f}")), r#""\u001f""#);
        assert_eq!(compact(&Node::string("\u{0}")), r#""\u0000""#);
        assert_eq!(compact(&Node::string("\u{b}")), r#""\u000b""#);
        assert_eq!(compact(&Node::string("\u{1b}")), r#""\u001b""#);
        assert_eq!(compact(&Node::string("a/b")), r#""a/b""#);
        assert_eq!(compact(&Node::string("\u{7f}")), "\"\u{7f}\"");
        // Non-ASCII goes out raw, U+2028 and U+2029 included.
        assert_eq!(
            compact(&Node::string("é\u{2028}\u{2029}😀")),
            "\"é\u{2028}\u{2029}😀\""
        );
    }

    #[test]
    fn a_raw_control_character_in_a_string_is_rejected() {
        assert_eq!(
            rejected("\"a\nb\""),
            "unescaped control character 0x0a in a string"
        );
        assert_eq!(
            rejected("\"\t\""),
            "unescaped control character 0x09 in a string"
        );
        assert_eq!(rejected("\"unterminated"), "unterminated string");
    }

    #[test]
    fn surrogate_pairs_pair_up_and_halves_do_not() {
        assert_eq!(parsed(r#""\ud83d\ude00""#).as_str().unwrap(), "😀");
        assert_eq!(parsed(r#""\uD83D\uDE00""#).as_str().unwrap(), "😀");
        assert!(rejected(r#""\ud83d""#).contains("no low half after it"));
        assert!(rejected(r#""\ud83dx""#).contains("no low half after it"));
        assert!(rejected(r#""\ud83d\u0041""#).contains("needs a low surrogate"));
        assert!(rejected(r#""\ude00""#).contains("no high half before it"));
        assert_eq!(rejected(r#""\u00g0""#), "expected a hex digit, found `g`");
        assert_eq!(rejected(r#""\u12""#), "expected a hex digit, found `\"`");
        assert_eq!(rejected(r#""\x""#), "unknown escape `\\x`");
    }

    #[test]
    fn malformed_input_is_rejected() {
        assert_eq!(rejected("[1,]"), "expected a value, found `]`");
        assert_eq!(rejected("{\"a\":1,}"), "expected a quoted key, found `}`");
        assert_eq!(rejected("{'a':1}"), "expected a quoted key, found `'`");
        assert_eq!(rejected("{a:1}"), "expected a quoted key, found `a`");
        assert_eq!(rejected("// c\n1"), "expected a value, found `/`");
        assert_eq!(rejected("[1 /* c */]"), "expected `,` or `]`, found `/`");
        assert_eq!(rejected("NaN"), "NaN and Infinity are not JSON");
        assert_eq!(rejected("Infinity"), "NaN and Infinity are not JSON");
        assert_eq!(rejected("-Infinity"), "expected a digit, found `I`");
        assert_eq!(rejected("+1"), "expected a value, found `+`");
        assert_eq!(rejected("01"), "a number may not have a leading zero");
        assert_eq!(rejected("-01"), "a number may not have a leading zero");
        assert_eq!(rejected(".5"), "expected a value, found `.`");
        assert_eq!(
            rejected("5."),
            "expected a digit after `.`, found end of input"
        );
        assert_eq!(
            rejected("1e"),
            "expected a digit in the exponent, found end of input"
        );
        assert_eq!(
            rejected("1e+"),
            "expected a digit in the exponent, found end of input"
        );
        assert_eq!(rejected(""), "expected a value, found end of input");
        assert_eq!(rejected("{\"a\" 1}"), "expected `:` after a key, found `1`");
        assert_eq!(
            rejected("{\"a\":1 \"b\":2}"),
            "expected `,` or `}`, found `\"`"
        );
        assert_eq!(rejected("tru"), "expected a value, found `t`");
        assert_eq!(rejected("[1,2"), "expected `,` or `]`, found end of input");
    }

    #[test]
    fn a_duplicate_key_is_an_error() {
        assert_eq!(rejected(r#"{"a":1,"a":2}"#), "duplicate key `a`");
        assert_eq!(
            rejected(r#"{"a":1,"b":{"c":1,"c":1}}"#),
            "duplicate key `c`"
        );
        assert!(parse(r#"{"a":1,"b":2}"#).is_ok());
    }

    #[test]
    fn errors_carry_the_line_they_happened_on() {
        let error = parse("{\n  \"a\": 1,\n  \"a\": 2\n}").unwrap_err();
        assert_eq!(error.line, 3);
        assert_eq!(error.to_string(), "line 3: duplicate key `a`");
        assert_eq!(parse("[\n1,\n2,\n]").unwrap_err().line, 4);
        assert_eq!(parse("1").map(|_| ()).unwrap_or(()), ());
    }

    #[test]
    fn key_order_survives() {
        let node = parsed(r#"{"z":1,"a":2,"m":3}"#);
        let map = node.as_map().unwrap();
        assert_eq!(map.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
        assert_eq!(compact(&node), r#"{"z":1,"a":2,"m":3}"#);
    }

    #[test]
    fn empty_containers_have_no_inner_newline() {
        assert_eq!(pretty(&parsed("{}")), "{}");
        assert_eq!(pretty(&parsed("[]")), "[]");
        assert_eq!(
            pretty(&parsed(r#"{"a":{},"b":[]}"#)),
            "{\n  \"a\": {},\n  \"b\": []\n}"
        );
        assert_eq!(compact(&parsed(r#"{"a":{},"b":[]}"#)), r#"{"a":{},"b":[]}"#);
    }

    #[test]
    fn nesting_indents_two_spaces_a_level() {
        let node = parsed(r#"{"a":[1,{"b":[true,null]}]}"#);
        assert_eq!(
            pretty(&node),
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": [\n        true,\n        null\n      ]\n    }\n  ]\n}"
        );
        assert_eq!(compact(&node), r#"{"a":[1,{"b":[true,null]}]}"#);
        assert!(!pretty(&node).ends_with('\n'));
    }

    #[test]
    fn pretty_except_compacts_the_nodes_the_predicate_picks() {
        let node = parsed(r#"{"keep":[{"id":1},{"id":2}],"other":{"x":[1,2]}}"#);
        let at_a_list_item =
            |path: &[Step<'_>]| matches!(path, [Step::Key("keep"), Step::Index(_)]);
        assert_eq!(
            write(&node, Format::PrettyExcept(&at_a_list_item)),
            "{\n  \"keep\": [\n    {\"id\":1},\n    {\"id\":2}\n  ],\n  \"other\": {\n    \"x\": [\n      1,\n      2\n    ]\n  }\n}"
        );
        // The root is offered the empty path, and compacting it compacts all.
        let everything = |path: &[Step<'_>]| path.is_empty();
        assert_eq!(
            write(&node, Format::PrettyExcept(&everything)),
            compact(&node)
        );
        let nothing = |_: &[Step<'_>]| false;
        assert_eq!(write(&node, Format::PrettyExcept(&nothing)), pretty(&node));
    }

    #[test]
    fn the_predicate_sees_the_whole_path() {
        let node = parsed(r#"{"a":[{"b":1}]}"#);
        // The predicate is a `&dyn Fn`, so what it records goes through a cell.
        let log = std::cell::RefCell::new(Vec::new());
        let record = |path: &[Step<'_>]| {
            log.borrow_mut().push(format!("{path:?}"));
            false
        };
        let _ = write(&node, Format::PrettyExcept(&record));
        let paths = log.into_inner();
        assert_eq!(paths.len(), 4);
        assert_eq!(paths[0], "[]");
        assert_eq!(paths[1], "[Key(\"a\")]");
        assert_eq!(paths[2], "[Key(\"a\"), Index(0)]");
        assert_eq!(paths[3], "[Key(\"a\"), Index(0), Key(\"b\")]");
    }

    #[test]
    fn numbers_are_kept_as_written() {
        for literal in [
            "0",
            "1",
            "-1",
            "1.5",
            "-3.25",
            "0.0015",
            "1e+21",
            "1e-7",
            "1234567890123",
            "100000000000000000000",
            "0.000001",
            "5e-324",
            "1.7976931348623157e+308",
        ] {
            let node = parsed(literal);
            assert_eq!(node, Node::Number(literal.to_string()), "{literal}");
            assert_eq!(compact(&node), literal);
        }
    }

    #[test]
    fn a_non_canonical_number_is_rejected_and_the_canonical_form_named() {
        assert_eq!(
            rejected("1.0"),
            "`1.0` is not in canonical form; write it `1`"
        );
        assert_eq!(
            rejected("1e3"),
            "`1e3` is not in canonical form; write it `1000`"
        );
        assert_eq!(
            rejected("-0"),
            "`-0` is not in canonical form; write it `0`"
        );
        assert_eq!(
            rejected("1E3"),
            "`1E3` is not in canonical form; write it `1000`"
        );
        assert!(rejected("0.10").contains("write it `0.1`"));
        assert!(rejected("1e21").contains("write it `1e+21`"));
        assert!(rejected("0.0000001").contains("write it `1e-7`"));
        assert!(rejected("1e400").contains("too large"));
    }

    #[test]
    fn canonical_number_writes_what_javascript_writes() {
        assert_eq!(canonical_number(0.0), "0");
        assert_eq!(canonical_number(-0.0), "0");
        assert_eq!(canonical_number(1.0), "1");
        assert_eq!(canonical_number(1.5), "1.5");
        assert_eq!(canonical_number(0.0015), "0.0015");
        assert_eq!(canonical_number(1e21), "1e+21");
        assert_eq!(canonical_number(1e-7), "1e-7");
        assert_eq!(canonical_number(1234567890123.0), "1234567890123");
        assert_eq!(canonical_number(-3.25), "-3.25");
        assert_eq!(canonical_number(1e20), "100000000000000000000");
        assert_eq!(canonical_number(1e-6), "0.000001");
        assert_eq!(canonical_number(1.5e22), "1.5e+22");
        assert_eq!(canonical_number(-1e-7), "-1e-7");
        assert_eq!(canonical_number(123.456), "123.456");
        assert_eq!(canonical_number(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(canonical_number(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(
            canonical_number(f64::MIN_POSITIVE),
            "2.2250738585072014e-308"
        );
        assert_eq!(canonical_number(5e-324), "5e-324");
        assert_eq!(canonical_number(9007199254740993.0), "9007199254740992");
    }

    #[test]
    fn a_document_survives_the_whole_round_trip() {
        let text = concat!(
            "{\n",
            "  \"name\": \"a/b\\tc\",\n",
            "  \"list\": [\n",
            "    1,\n",
            "    -2.5,\n",
            "    null,\n",
            "    false\n",
            "  ],\n",
            "  \"empty\": {},\n",
            "  \"deep\": {\n",
            "    \"u\": \"\\u001f\"\n",
            "  }\n",
            "}"
        );
        let node = parsed(text);
        assert_eq!(pretty(&node), text);
        assert_eq!(pretty(&parsed(&pretty(&node))), text);
    }
}
