//! Formula parsing and printing. Precedence is Excel's: reference operators, then
//! negation, percent, `^` (left-associative, so `2^3^2` is 64 and `-2^2` is 4), `*` `/`,
//! `+` `-`, `&`, and the comparisons.
use crate::address::{column_index, CellRef, MAX_COLS, MAX_ROWS};
use crate::value::{general, ErrorKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RangeKind {
    Cells,
    /// `A:C`, whole columns.
    Columns,
    /// `2:5`, whole rows.
    Rows,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}
impl Op {
    fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Pow => "^",
            Self::Concat => "&",
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }
    fn precedence(self) -> u8 {
        match self {
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge => 1,
            Self::Concat => 2,
            Self::Add | Self::Sub => 3,
            Self::Mul | Self::Div => 4,
            Self::Pow => 5,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Error(ErrorKind),
    Ref {
        sheet: Option<String>,
        cell: CellRef,
    },
    Range {
        sheet: Option<String>,
        start: CellRef,
        end: CellRef,
        kind: RangeKind,
    },
    Name(String),
    Neg(Box<Expr>),
    Plus(Box<Expr>),
    Percent(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Array(Vec<Vec<Expr>>),
    /// An omitted argument, as in `IF(A1,,2)`.
    Missing,
    /// Parentheses the author wrote, kept so printing round-trips.
    Group(Box<Expr>),
}
impl Eq for Expr {}

/// A parsed formula. It serialises as its text and is parsed again on the way in, so
/// a saved workbook holds exactly what the formula bar shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formula {
    pub expr: Expr,
}
impl Formula {
    pub fn parse(text: &str) -> Result<Self, String> {
        parse(text).map(|expr| Self { expr })
    }
    /// Formula text without the leading `=`.
    pub fn text(&self) -> String {
        print(&self.expr)
    }
}
impl Serialize for Formula {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.text())
    }
}
impl<'de> Deserialize<'de> for Formula {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Formula::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Bool(bool),
    Err(ErrorKind),
    Ref(Option<String>, CellRef),
    Range(Option<String>, CellRef, CellRef, RangeKind),
    Ident(String),
    Func(String),
    Op(&'static str),
    Eof,
}

fn sheet_prefix(s: &str) -> Option<(String, usize)> {
    // 'Quoted Sheet'!  or  Plain!
    if let Some(rest) = s.strip_prefix('\'') {
        let mut name = String::new();
        let mut chars = rest.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\'' {
                if matches!(chars.peek(), Some((_, '\''))) {
                    name.push('\'');
                    chars.next();
                    continue;
                }
                let after = &rest[i + 1..];
                return after.starts_with('!').then(|| (name, i + 3));
            }
            name.push(c);
        }
        return None;
    }
    let end = s.find('!')?;
    let name = &s[..end];
    (!name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.'))
    .then(|| (name.to_owned(), end + 1))
}
/// A reference at the start of `s`: `A1`, `$A$1`, `A1:B2`, `A:B`, `1:2`. Returns it and
/// the bytes consumed.
fn reference(s: &str) -> Option<(Tok, usize, Option<String>)> {
    let (sheet, skip) = match sheet_prefix(s) {
        Some((n, k)) => (Some(n), k),
        None => (None, 0),
    };
    let body = &s[skip..];
    let cell_len = |t: &str| -> Option<(CellRef, usize)> {
        let b = t.as_bytes();
        let mut i = 0;
        if b.first() == Some(&b'$') {
            i += 1;
        }
        let letters = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }
        if i == letters || i - letters > 3 {
            return None;
        }
        if b.get(i) == Some(&b'$') {
            i += 1;
        }
        let digits = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits {
            return None;
        }
        // A reference is not followed by more name characters (LOG10( is a function).
        if b.get(i)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'(' || *c == b'.')
        {
            return None;
        }
        CellRef::parse(&t[..i]).map(|r| (r, i))
    };
    if let Some((a, n)) = cell_len(body) {
        if body[n..].starts_with(':') {
            if let Some((b, m)) = cell_len(&body[n + 1..]) {
                return Some((
                    Tok::Range(sheet.clone(), a, b, RangeKind::Cells),
                    skip + n + 1 + m,
                    sheet,
                ));
            }
        }
        return Some((Tok::Ref(sheet.clone(), a), skip + n, sheet));
    }
    // Whole columns A:C and whole rows 2:5.
    let part = |t: &str, letters: bool| -> Option<(u32, bool, usize)> {
        let b = t.as_bytes();
        let mut i = 0;
        let abs = b.first() == Some(&b'$');
        if abs {
            i += 1;
        }
        let start = i;
        while i < b.len()
            && (if letters {
                b[i].is_ascii_alphabetic()
            } else {
                b[i].is_ascii_digit()
            })
        {
            i += 1;
        }
        if i == start {
            return None;
        }
        let v = if letters {
            column_index(&t[start..i])?
        } else {
            let r: u32 = t[start..i].parse().ok()?;
            if r == 0 || r > MAX_ROWS {
                return None;
            }
            r - 1
        };
        Some((v, abs, i))
    };
    for letters in [true, false] {
        if let Some((a, a_abs, n)) = part(body, letters) {
            if body[n..].starts_with(':') {
                if let Some((b, b_abs, m)) = part(&body[n + 1..], letters) {
                    let end = n + 1 + m;
                    if body
                        .as_bytes()
                        .get(end)
                        .is_some_and(|c| c.is_ascii_alphanumeric())
                    {
                        continue;
                    }
                    let (start, stop, kind) = if letters {
                        (
                            CellRef {
                                row: 0,
                                col: a.min(b),
                                row_abs: true,
                                col_abs: a_abs,
                            },
                            CellRef {
                                row: MAX_ROWS - 1,
                                col: a.max(b),
                                row_abs: true,
                                col_abs: b_abs,
                            },
                            RangeKind::Columns,
                        )
                    } else {
                        (
                            CellRef {
                                row: a.min(b),
                                col: 0,
                                row_abs: a_abs,
                                col_abs: true,
                            },
                            CellRef {
                                row: a.max(b),
                                col: MAX_COLS - 1,
                                row_abs: b_abs,
                                col_abs: true,
                            },
                            RangeKind::Rows,
                        )
                    };
                    return Some((
                        Tok::Range(sheet.clone(), start, stop, kind),
                        skip + end,
                        sheet,
                    ));
                }
            }
        }
    }
    None
}
fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    let b = src.as_bytes();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        let rest = &src[i..];
        // References come first: A1 must not read as a name.
        let after_value = matches!(
            out.last(),
            Some(
                Tok::Num(_)
                    | Tok::Str(_)
                    | Tok::Ref(..)
                    | Tok::Range(..)
                    | Tok::Ident(_)
                    | Tok::Op(")")
            )
        );
        if !after_value || rest.starts_with('\'') {
            if let Some((tok, n, _)) = reference(rest) {
                out.push(tok);
                i += n;
                continue;
            }
        }
        if c == '"' {
            let mut s = String::new();
            let mut j = i + 1;
            loop {
                let Some(ch) = src[j..].chars().next() else {
                    return Err("unterminated text in formula".into());
                };
                if ch == '"' {
                    if src[j + 1..].starts_with('"') {
                        s.push('"');
                        j += 2;
                        continue;
                    }
                    j += 1;
                    break;
                }
                s.push(ch);
                j += ch.len_utf8();
            }
            out.push(Tok::Str(s));
            i = j;
            continue;
        }
        if c == '#' {
            let codes = [
                "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#REF!", "#VALUE!",
            ];
            if let Some(code) = codes
                .iter()
                .find(|k| rest.to_ascii_uppercase().starts_with(**k))
            {
                out.push(Tok::Err(ErrorKind::parse(code).unwrap_or(ErrorKind::Value)));
                i += code.len();
                continue;
            }
            return Err(format!("unknown error value in {rest}"));
        }
        if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
                let mut k = j + 1;
                if k < b.len() && (b[k] == b'+' || b[k] == b'-') {
                    k += 1;
                }
                if k < b.len() && b[k].is_ascii_digit() {
                    while k < b.len() && b[k].is_ascii_digit() {
                        k += 1;
                    }
                    j = k;
                }
            }
            let v: f64 = src[i..j]
                .parse()
                .map_err(|_| format!("bad number {}", &src[i..j]))?;
            out.push(Tok::Num(v));
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '\\' {
            let mut j = i;
            while j < b.len() {
                let ch = src[j..].chars().next().unwrap_or(' ');
                if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '\\' {
                    j += ch.len_utf8();
                } else {
                    break;
                }
            }
            let word = &src[i..j];
            let next = src[j..].trim_start();
            if next.starts_with('(') {
                out.push(Tok::Func(word.to_ascii_uppercase()));
            } else if word.eq_ignore_ascii_case("TRUE") {
                out.push(Tok::Bool(true));
            } else if word.eq_ignore_ascii_case("FALSE") {
                out.push(Tok::Bool(false));
            } else {
                out.push(Tok::Ident(word.to_owned()));
            }
            i = j;
            continue;
        }
        let two = rest.get(..2).unwrap_or("");
        let op: &'static str = match two {
            "<=" => "<=",
            ">=" => ">=",
            "<>" => "<>",
            _ => match c {
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                '^' => "^",
                '&' => "&",
                '=' => "=",
                '<' => "<",
                '>' => ">",
                '%' => "%",
                '(' => "(",
                ')' => ")",
                ',' => ",",
                ';' => ";",
                '{' => "{",
                '}' => "}",
                _ => return Err(format!("unexpected character {c}")),
            },
        };
        out.push(Tok::Op(op));
        i += op.len();
    }
    out.push(Tok::Eof);
    Ok(out)
}

struct P {
    toks: Vec<Tok>,
    pos: usize,
}
impl P {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos]
    }
    fn next(&mut self) -> Tok {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn is(&self, op: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == op)
    }
    fn eat(&mut self, op: &str) -> bool {
        if self.is(op) {
            self.next();
            true
        } else {
            false
        }
    }
    fn binary(&mut self, min: u8) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Op("+") => Op::Add,
                Tok::Op("-") => Op::Sub,
                Tok::Op("*") => Op::Mul,
                Tok::Op("/") => Op::Div,
                Tok::Op("^") => Op::Pow,
                Tok::Op("&") => Op::Concat,
                Tok::Op("=") => Op::Eq,
                Tok::Op("<>") => Op::Ne,
                Tok::Op("<") => Op::Lt,
                Tok::Op("<=") => Op::Le,
                Tok::Op(">") => Op::Gt,
                Tok::Op(">=") => Op::Ge,
                _ => return Ok(left),
            };
            if op.precedence() < min {
                return Ok(left);
            }
            self.next();
            // Every Excel binary operator is left-associative, `^` included.
            let right = self.binary(op.precedence() + 1)?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("+") {
            return Ok(Expr::Plus(Box::new(self.unary()?)));
        }
        let mut e = self.primary()?;
        while self.eat("%") {
            e = Expr::Percent(Box::new(e));
        }
        Ok(e)
    }
    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Tok::Num(n) => Ok(Expr::Number(n)),
            Tok::Str(s) => Ok(Expr::Text(s)),
            Tok::Bool(b) => Ok(Expr::Bool(b)),
            Tok::Err(e) => Ok(Expr::Error(e)),
            Tok::Ref(sheet, cell) => Ok(Expr::Ref { sheet, cell }),
            Tok::Range(sheet, start, end, kind) => Ok(Expr::Range {
                sheet,
                start,
                end,
                kind,
            }),
            Tok::Ident(name) => Ok(Expr::Name(name)),
            Tok::Func(name) => {
                self.next(); // (
                let mut args = Vec::new();
                if !self.is(")") {
                    loop {
                        if self.is(",") || self.is(")") {
                            args.push(Expr::Missing);
                        } else {
                            args.push(self.binary(0)?);
                        }
                        if !self.eat(",") {
                            break;
                        }
                        if self.is(")") {
                            args.push(Expr::Missing);
                            break;
                        }
                    }
                }
                if !self.eat(")") {
                    return Err(format!("missing ) after arguments to {name}"));
                }
                Ok(Expr::Call(name, args))
            }
            Tok::Op("(") => {
                let e = self.binary(0)?;
                if !self.eat(")") {
                    return Err("missing )".into());
                }
                Ok(Expr::Group(Box::new(e)))
            }
            Tok::Op("{") => {
                let mut rows = vec![vec![]];
                loop {
                    let neg = self.eat("-");
                    let item = match self.next() {
                        Tok::Num(n) => Expr::Number(if neg { -n } else { n }),
                        Tok::Str(s) if !neg => Expr::Text(s),
                        Tok::Bool(b) if !neg => Expr::Bool(b),
                        Tok::Err(e) if !neg => Expr::Error(e),
                        _ => {
                            return Err(
                                "array constants hold numbers, text, booleans or errors".into()
                            )
                        }
                    };
                    rows.last_mut().unwrap().push(item);
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat(";") {
                        rows.push(vec![]);
                        continue;
                    }
                    if self.eat("}") {
                        break;
                    }
                    return Err("malformed array constant".into());
                }
                if rows.iter().any(|r| r.len() != rows[0].len()) {
                    return Err("array constant rows differ in length".into());
                }
                Ok(Expr::Array(rows))
            }
            Tok::Eof => Err("the formula ends too soon".into()),
            Tok::Op(o) => Err(format!("unexpected {o}")),
        }
    }
}
/// Parse formula text (with or without the leading `=`).
/// A reference as it appears in formula text being typed: its byte span, the sheet it
/// names (if any) and the cells it covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefSpan {
    pub start: usize,
    pub end: usize,
    pub sheet: Option<String>,
    pub range: crate::address::Range,
}
/// Every cell or range reference in formula text, even text that does not parse yet
/// (a formula still being typed), skipping string literals and names such as
/// `LOG10`. The editor colours each one and outlines its cells, as Excel does.
pub fn reference_spans(text: &str) -> Vec<RefSpan> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'$';
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'"' {
                    if b.get(i + 1) == Some(&b'"') {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        let at_start = i == 0 || !word(b[i - 1]) && b[i - 1] != b'\'' && b[i - 1] != b'!';
        if at_start && (c.is_ascii_alphanumeric() || c == b'$' || c == b'\'' || c == b'_') {
            if let Some((tok, n, sheet)) = reference(&text[i..]) {
                let range = match tok {
                    Tok::Ref(_, r) => crate::address::Range::single(r.cell()),
                    Tok::Range(_, a, z, _) => crate::address::Range::new(a.cell(), z.cell()),
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                out.push(RefSpan {
                    start: i,
                    end: i + n,
                    sheet,
                    range,
                });
                i += n;
                continue;
            }
            // Skip the rest of a name so its tail is not read as a reference.
            while i < b.len() && (word(b[i]) || b[i] == b'\'') {
                i += 1;
            }
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}
pub fn parse(text: &str) -> Result<Expr, String> {
    let body = text.strip_prefix('=').unwrap_or(text);
    let mut p = P {
        toks: tokenize(body)?,
        pos: 0,
    };
    if matches!(p.peek(), Tok::Eof) {
        return Err("the formula is empty".into());
    }
    let e = p.binary(0)?;
    if !matches!(p.peek(), Tok::Eof) {
        return Err("there is a problem with this formula".into());
    }
    Ok(e)
}

pub fn quote_sheet(name: &str) -> String {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
        && reference(name).is_none();
    if plain {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}
fn print_sheet(sheet: &Option<String>) -> String {
    sheet
        .as_ref()
        .map(|s| format!("{}!", quote_sheet(s)))
        .unwrap_or_default()
}
/// Canonical formula text (no leading `=`).
pub fn print(e: &Expr) -> String {
    match e {
        Expr::Number(n) => general(*n),
        Expr::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
        Expr::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Expr::Error(k) => k.code().into(),
        Expr::Ref { sheet, cell } => format!("{}{}", print_sheet(sheet), cell.a1()),
        Expr::Range {
            sheet,
            start,
            end,
            kind,
        } => {
            let s = print_sheet(sheet);
            match kind {
                RangeKind::Cells => format!("{s}{}:{}", start.a1(), end.a1()),
                RangeKind::Columns => format!(
                    "{s}{}{}:{}{}",
                    if start.col_abs { "$" } else { "" },
                    crate::address::column_name(start.col),
                    if end.col_abs { "$" } else { "" },
                    crate::address::column_name(end.col)
                ),
                RangeKind::Rows => format!(
                    "{s}{}{}:{}{}",
                    if start.row_abs { "$" } else { "" },
                    start.row + 1,
                    if end.row_abs { "$" } else { "" },
                    end.row + 1
                ),
            }
        }
        Expr::Name(n) => n.clone(),
        Expr::Neg(a) => format!("-{}", print(a)),
        Expr::Plus(a) => format!("+{}", print(a)),
        Expr::Percent(a) => format!("{}%", print(a)),
        Expr::Bin(op, a, b) => format!("{}{}{}", print(a), op.symbol(), print(b)),
        Expr::Call(name, args) => format!(
            "{name}({})",
            args.iter().map(print).collect::<Vec<_>>().join(",")
        ),
        Expr::Array(rows) => format!(
            "{{{}}}",
            rows.iter()
                .map(|r| r.iter().map(print).collect::<Vec<_>>().join(","))
                .collect::<Vec<_>>()
                .join(";")
        ),
        Expr::Missing => String::new(),
        Expr::Group(a) => format!("({})", print(a)),
    }
}

/// Rewrite every reference in an expression. `f` gets the sheet name (if any) and a
/// reference and returns its replacement, or `None` for `#REF!`.
pub fn map_refs(e: &Expr, f: &mut dyn FnMut(&Option<String>, CellRef) -> Option<CellRef>) -> Expr {
    let mut m = |x: &Expr| map_refs(x, f);
    match e {
        Expr::Ref { sheet, cell } => match f(sheet, *cell) {
            Some(c) => Expr::Ref {
                sheet: sheet.clone(),
                cell: c,
            },
            None => Expr::Error(ErrorKind::Ref),
        },
        Expr::Range {
            sheet,
            start,
            end,
            kind,
        } => match (f(sheet, *start), f(sheet, *end)) {
            (Some(a), Some(b)) => Expr::Range {
                sheet: sheet.clone(),
                start: a,
                end: b,
                kind: *kind,
            },
            _ => Expr::Error(ErrorKind::Ref),
        },
        Expr::Neg(a) => Expr::Neg(Box::new(m(a))),
        Expr::Plus(a) => Expr::Plus(Box::new(m(a))),
        Expr::Percent(a) => Expr::Percent(Box::new(m(a))),
        Expr::Group(a) => Expr::Group(Box::new(m(a))),
        Expr::Bin(op, a, b) => Expr::Bin(*op, Box::new(m(a)), Box::new(m(b))),
        Expr::Call(n, args) => Expr::Call(n.clone(), args.iter().map(&mut m).collect()),
        other => other.clone(),
    }
}
/// Every reference in an expression, with its sheet name.
pub fn references(e: &Expr, out: &mut Vec<(Option<String>, CellRef, CellRef)>) {
    match e {
        Expr::Ref { sheet, cell } => out.push((sheet.clone(), *cell, *cell)),
        Expr::Range {
            sheet, start, end, ..
        } => out.push((sheet.clone(), *start, *end)),
        Expr::Neg(a) | Expr::Plus(a) | Expr::Percent(a) | Expr::Group(a) => references(a, out),
        Expr::Bin(_, a, b) => {
            references(a, out);
            references(b, out);
        }
        Expr::Call(_, args) => {
            for a in args {
                references(a, out);
            }
        }
        _ => {}
    }
}
/// Names and function calls, for dependency and volatility analysis.
pub fn walk(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Neg(a) | Expr::Plus(a) | Expr::Percent(a) | Expr::Group(a) => walk(a, f),
        Expr::Bin(_, a, b) => {
            walk(a, f);
            walk(b, f);
        }
        Expr::Call(_, args) => {
            for a in args {
                walk(a, f);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn round(text: &str) -> String {
        print(&parse(text).unwrap_or_else(|e| panic!("{text}: {e}")))
    }
    #[test]
    fn references_are_found_in_formulas_still_being_typed() {
        let text = "=SUM(B2:B5)+C2*'My Sheet'!$C$3+LOG10(\"A1\")+SUM(D";
        let spans: Vec<(String, Option<String>, String)> = reference_spans(text)
            .into_iter()
            .map(|s| (text[s.start..s.end].to_owned(), s.sheet, s.range.a1()))
            .collect();
        assert_eq!(
            spans,
            vec![
                ("B2:B5".into(), None, "B2:B5".into()),
                ("C2".into(), None, "C2".into()),
                (
                    "'My Sheet'!$C$3".into(),
                    Some("My Sheet".into()),
                    "C3".into()
                ),
            ]
        );
        // A reference typed so far is coloured as soon as it is one.
        assert_eq!(reference_spans("=A1+").len(), 1);
        assert!(reference_spans("=\"B2\"&X").is_empty());
    }
    #[test]
    fn formulas_parse_and_print_back() {
        assert_eq!(round("=SUM(A1:B7)*2"), "SUM(A1:B7)*2");
        assert_eq!(
            round("=sum($A$1, 'My Sheet'!C3:D4, Sheet2!A:A)"),
            "SUM($A$1,'My Sheet'!C3:D4,Sheet2!A:A)"
        );
        assert_eq!(
            round("=IF(A1>=10,\"big \"\"one\"\"\",)"),
            "IF(A1>=10,\"big \"\"one\"\"\",)"
        );
        assert_eq!(round("=-2^2"), "-2^2");
        assert_eq!(round("=(1+2)*3%"), "(1+2)*3%");
        assert_eq!(round("=LOG10(100)+total_sales"), "LOG10(100)+total_sales");
        assert_eq!(round("={1,2;3,4}"), "{1,2;3,4}");
        assert_eq!(round("=2:5"), "2:5");
        assert_eq!(round("=#N/A"), "#N/A");
    }
    #[test]
    fn precedence_matches_excel() {
        // ^ is left-associative and binds looser than negation.
        let e = parse("=2^3^2").unwrap();
        assert!(
            matches!(e, Expr::Bin(Op::Pow, ref a, _) if matches!(**a, Expr::Bin(Op::Pow, _, _)))
        );
        assert!(
            matches!(parse("=-2^2").unwrap(), Expr::Bin(Op::Pow, ref a, _) if matches!(**a, Expr::Neg(_)))
        );
        assert!(matches!(
            parse("=1+2&3").unwrap(),
            Expr::Bin(Op::Concat, _, _)
        ));
        assert!(matches!(parse("=1&2=12").unwrap(), Expr::Bin(Op::Eq, _, _)));
        assert!(parse("=SUM(1,2").is_err());
        assert!(parse("=1+").is_err());
    }
}
