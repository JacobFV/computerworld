//! SQL tokenizer following SQLite's lexical rules.
use crate::SqlError;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// Bare or quoted identifier. `quoted` identifiers are never keywords.
    Ident {
        name: String,
        quoted: bool,
    },
    Str(String),
    Blob(Vec<u8>),
    Int(String),
    Real(String),
    /// `?`, `?NNN`, `:name`, `@name`, `$name`.
    Param(String),
    Op(&'static str),
    Semi,
    Eof,
}
#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    /// Byte offsets into the statement text.
    pub start: usize,
    pub end: usize,
}
impl Token {
    /// The bare identifier text in upper case, when this is an unquoted identifier.
    pub fn keyword(&self) -> Option<String> {
        match &self.tok {
            Tok::Ident {
                name,
                quoted: false,
            } => Some(name.to_ascii_uppercase()),
            _ => None,
        }
    }
}

const OPS: &[&str] = &[
    "||", "<<", ">>", "<=", ">=", "==", "!=", "<>", "->>", "->", "(", ")", ",", ".", "+", "-", "*",
    "/", "%", "<", ">", "=", "&", "|", "~",
];

pub fn tokenize(sql: &str) -> Result<Vec<Token>, SqlError> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        let start = i;
        let tok = if c == b';' {
            i += 1;
            Tok::Semi
        } else if c == b'\'' {
            let (text, next) = quoted(sql, i, b'\'')?;
            i = next;
            Tok::Str(text)
        } else if c == b'"' || c == b'`' {
            let (name, next) = quoted(sql, i, c)?;
            i = next;
            Tok::Ident { name, quoted: true }
        } else if c == b'[' {
            let end = sql[i..]
                .find(']')
                .ok_or_else(|| SqlError::syntax_at("unrecognized token: \"[\"", i))?;
            let name = sql[i + 1..i + end].to_owned();
            i += end + 1;
            Tok::Ident { name, quoted: true }
        } else if (c == b'x' || c == b'X') && b.get(i + 1) == Some(&b'\'') {
            let (text, next) = quoted(sql, i + 1, b'\'')?;
            let bytes = crate::value::unhex(&text).ok_or_else(|| {
                SqlError::syntax_at(format!("unrecognized token: \"{}\"", &sql[i..next]), i)
            })?;
            i = next;
            Tok::Blob(bytes)
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let (tok, next) = number(sql, i)?;
            i = next;
            tok
        } else if c == b'?' {
            i += 1;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            Tok::Param(sql[start..i].to_owned())
        } else if matches!(c, b':' | b'@' | b'$') {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] >= 0x80) {
                i += 1;
            }
            if i == start + 1 {
                return Err(SqlError::syntax_at(
                    format!("unrecognized token: \"{}\"", c as char),
                    start,
                ));
            }
            Tok::Param(sql[start..i].to_owned())
        } else if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$' || b[i] >= 0x80)
            {
                i += 1;
            }
            Tok::Ident {
                name: sql[start..i].to_owned(),
                quoted: false,
            }
        } else if let Some(op) = OPS.iter().find(|op| sql[i..].starts_with(**op)) {
            i += op.len();
            Tok::Op(op)
        } else {
            let ch = sql[i..].chars().next().unwrap_or('?');
            return Err(SqlError::syntax_at(
                format!("unrecognized token: \"{ch}\""),
                i,
            ));
        };
        out.push(Token { tok, start, end: i });
    }
    out.push(Token {
        tok: Tok::Eof,
        start: sql.len(),
        end: sql.len(),
    });
    Ok(out)
}
fn quoted(sql: &str, start: usize, q: u8) -> Result<(String, usize), SqlError> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = start + 1;
    loop {
        if i >= b.len() {
            return Err(SqlError::syntax_at(
                format!("unrecognized token: \"{}\"", &sql[start..]),
                start,
            ));
        }
        if b[i] == q {
            if b.get(i + 1) == Some(&q) {
                out.push(q);
                i += 2;
                continue;
            }
            return Ok((String::from_utf8_lossy(&out).into_owned(), i + 1));
        }
        out.push(b[i]);
        i += 1;
    }
}
fn number(sql: &str, start: usize) -> Result<(Tok, usize), SqlError> {
    let b = sql.as_bytes();
    let mut i = start;
    if b[i] == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X')) {
        i += 2;
        while i < b.len() && b[i].is_ascii_hexdigit() {
            i += 1;
        }
        return Ok((Tok::Int(sql[start..i].to_owned()), i));
    }
    let mut real = false;
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_') {
        i += 1;
    }
    if i < b.len() && b[i] == b'.' {
        real = true;
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            real = true;
            i = j;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
    if i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
        let mut j = i;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
            j += 1;
        }
        return Err(SqlError::syntax_at(
            format!("unrecognized token: \"{}\"", &sql[start..j]),
            start,
        ));
    }
    let text = sql[start..i].replace('_', "");
    Ok((
        if real {
            Tok::Real(text)
        } else {
            Tok::Int(text)
        },
        i,
    ))
}

/// Token classes `sqlite3_complete` tells apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Semi,
    Ws,
    Other,
    Explain,
    Create,
    Temp,
    Trigger,
    End,
}
/// `sqlite3_complete`'s state machine: a `;` inside `CREATE TRIGGER … BEGIN … END`
/// only ends the statement when it follows `END`.
fn step(state: usize, c: Class) -> usize {
    const TRANS: [[usize; 8]; 8] = [
        // Semi Ws Other Explain Create Temp Trigger End
        [1, 0, 2, 3, 4, 2, 2, 2], // 0 invalid
        [1, 1, 2, 3, 4, 2, 2, 2], // 1 start
        [1, 2, 2, 2, 2, 2, 2, 2], // 2 normal
        [1, 3, 3, 2, 4, 2, 2, 2], // 3 explain
        [1, 4, 2, 2, 2, 4, 5, 2], // 4 create
        [6, 5, 5, 5, 5, 5, 5, 5], // 5 trigger
        [6, 6, 5, 5, 5, 5, 5, 7], // 6 semi
        [1, 7, 5, 5, 5, 5, 5, 5], // 7 end
    ];
    TRANS[state][c as usize]
}
/// Classified tokens, each with the byte offset just past it, and whether every
/// quote, bracket and comment was closed.
fn classes(sql: &str) -> (Vec<(Class, usize)>, bool) {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80;
    while i < b.len() {
        match b[i] {
            q @ (b'\'' | b'"' | b'`') => {
                i += 1;
                loop {
                    if i >= b.len() {
                        return (out, false);
                    }
                    if b[i] == q {
                        if b.get(i + 1) == Some(&q) {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
                out.push((Class::Other, i));
            }
            b'[' => {
                while i < b.len() && b[i] != b']' {
                    i += 1;
                }
                if i >= b.len() {
                    return (out, false);
                }
                i += 1;
                out.push((Class::Other, i));
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                out.push((Class::Ws, i));
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                if i >= b.len() {
                    return (out, false);
                }
                i += 2;
                out.push((Class::Ws, i));
            }
            b';' => {
                i += 1;
                out.push((Class::Semi, i));
            }
            c if c.is_ascii_whitespace() => {
                i += 1;
                out.push((Class::Ws, i));
            }
            c if word(c) => {
                let start = i;
                while i < b.len() && word(b[i]) {
                    i += 1;
                }
                let class = match sql[start..i].to_ascii_uppercase().as_str() {
                    "EXPLAIN" => Class::Explain,
                    "CREATE" => Class::Create,
                    "TEMP" | "TEMPORARY" => Class::Temp,
                    "TRIGGER" => Class::Trigger,
                    "END" => Class::End,
                    _ => Class::Other,
                };
                out.push((class, i));
            }
            _ => {
                i += 1;
                out.push((Class::Other, i));
            }
        }
    }
    (out, true)
}
/// Split a script into complete statements (text including the trailing `;`), the
/// way `sqlite3_complete` decides where one ends: a `;` inside a string, a comment or
/// a trigger's `BEGIN … END` body does not end one. Trailing text without a `;` is
/// returned as a final statement.
pub fn split_statements(sql: &str) -> Vec<(usize, String)> {
    let (toks, _) = classes(sql);
    let mut out = Vec::new();
    let mut start = 0;
    let mut state = 0;
    for (class, end) in toks {
        state = step(state, class);
        if class == Class::Semi && state == 1 {
            out.push((start, sql[start..end].to_owned()));
            start = end;
            state = 0;
        }
    }
    let rest = &sql[start.min(sql.len())..];
    if !rest.trim().is_empty() {
        out.push((start, rest.to_owned()));
    }
    out
}
/// `sqlite3_complete`: does the text end with a `;` that closes a statement, outside
/// quotes, comments and trigger bodies (ignoring whitespace and comments after it)?
pub fn is_complete(sql: &str) -> bool {
    let (toks, closed) = classes(sql);
    closed && toks.iter().fold(0, |st, (c, _)| step(st, *c)) == 1
}
/// Whether text holds nothing but whitespace and comments.
pub fn is_blank(sql: &str) -> bool {
    tokenize(sql).is_ok_and(|t| t.iter().all(|t| matches!(t.tok, Tok::Eof | Tok::Semi)))
}
