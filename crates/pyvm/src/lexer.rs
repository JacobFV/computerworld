//! Tokenizer: physical lines to logical tokens with INDENT/DEDENT, implicit joining
//! inside brackets, backslash continuation, every string prefix and f-strings.

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Name(String),
    /// Integer literal digits without underscores or prefix, with its radix.
    Int(String, u32),
    Float(f64),
    Imag(f64),
    Str(String),
    Bytes(Vec<u8>),
    FStr(Vec<FPart>),
    Op(&'static str),
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FPart {
    Lit(String),
    Expr {
        src: String,
        line: u32,
        col: u32,
        conversion: Option<char>,
        spec: Option<Vec<FPart>>,
        /// `f"{x=}"` keeps the expression text (with `=`) as a literal prefix.
        debug: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Clone, Debug)]
pub struct SyntaxErr {
    pub kind: &'static str, // "SyntaxError" | "IndentationError" | "TabError"
    pub msg: String,
    pub line: u32,
    /// 1-based column of the caret, as CPython reports `offset`.
    pub col: u32,
    pub end_col: u32,
}
impl SyntaxErr {
    pub fn new(msg: impl Into<String>, line: u32, col: u32) -> Self {
        Self {
            kind: "SyntaxError",
            msg: msg.into(),
            line,
            col: col + 1,
            end_col: col + 2,
        }
    }
    pub fn indent(msg: impl Into<String>, line: u32, col: u32) -> Self {
        Self {
            kind: "IndentationError",
            msg: msg.into(),
            line,
            col: col + 1,
            end_col: col + 2,
        }
    }
}

const OPS: &[&str] = &[
    "**=", "//=", ">>=", "<<=", "...", "->", ":=", "**", "//", "<<", ">>", "<=", ">=", "==", "!=",
    "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "@=", "+", "-", "*", "/", "%", "@", "&", "|",
    "^", "~", "<", ">", "(", ")", "[", "]", "{", "}", ",", ":", ".", ";", "=", "!",
];

pub struct Warning {
    pub line: u32,
    pub msg: String,
}

pub struct Lexer<'a> {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    line_start: usize,
    indents: Vec<u32>,
    depth: Vec<(char, u32, u32)>,
    out: Vec<Token>,
    at_line_start: bool,
    pub warnings: Vec<Warning>,
    _src: &'a str,
    /// Line offset for sub-lexers (f-string expressions).
    base_line: u32,
    base_col: u32,
}

fn is_id_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}
fn is_id_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            line_start: 0,
            indents: vec![0],
            depth: vec![],
            out: vec![],
            at_line_start: true,
            warnings: vec![],
            _src: src,
            base_line: 0,
            base_col: 0,
        }
    }
    fn col(&self) -> u32 {
        (self.pos - self.line_start) as u32
    }
    fn peek(&self, off: usize) -> char {
        *self.chars.get(self.pos + off).unwrap_or(&'\0')
    }
    fn push(&mut self, tok: Tok, line: u32, col: u32) {
        let (end_line, end_col) = (self.line, self.col());
        self.out.push(Token {
            tok,
            line: line + self.base_line,
            col: if line == 1 { col + self.base_col } else { col },
            end_line: end_line + self.base_line,
            end_col,
        });
    }
    fn err(&self, msg: impl Into<String>) -> SyntaxErr {
        SyntaxErr::new(msg, self.line + self.base_line, self.col())
    }

    pub fn tokenize(mut self) -> Result<(Vec<Token>, Vec<Warning>), SyntaxErr> {
        loop {
            if self.at_line_start && self.depth.is_empty() {
                // Measure indentation of this physical line.
                let mut width = 0u32;
                let start = self.pos;
                while self.pos < self.chars.len() {
                    match self.chars[self.pos] {
                        ' ' => width += 1,
                        '\t' => width = (width / 8 + 1) * 8,
                        '\x0c' => width = 0,
                        _ => break,
                    }
                    self.pos += 1;
                }
                let c = self.peek(0);
                if c == '#' || c == '\n' || c == '\r' || c == '\\' && self.peek(1) == '\n' {
                    // Blank or comment-only line: no indentation tokens.
                    if c == '#' {
                        while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                            self.pos += 1;
                        }
                    }
                    if self.pos < self.chars.len() {
                        if self.chars[self.pos] == '\\' {
                            self.pos += 1;
                        }
                        if self.peek(0) == '\r' {
                            self.pos += 1;
                        }
                        self.pos += 1;
                        self.line += 1;
                        self.line_start = self.pos;
                        continue;
                    }
                }
                if self.pos >= self.chars.len() {
                    break;
                }
                let _ = start;
                let current = *self.indents.last().unwrap();
                if width > current {
                    // An unexpected INDENT is the parser's to report, so a
                    // missing ':' on the line above is diagnosed first.
                    self.indents.push(width);
                    self.push(Tok::Indent, self.line, 0);
                } else if width < current {
                    while width < *self.indents.last().unwrap() {
                        self.indents.pop();
                        self.push(Tok::Dedent, self.line, width);
                    }
                    if width != *self.indents.last().unwrap() {
                        let mut e = SyntaxErr::indent(
                            "unindent does not match any outer indentation level",
                            self.line + self.base_line,
                            width,
                        );
                        e.kind = "IndentationError";
                        return Err(e);
                    }
                } else if self.expects_block() {
                    // Handled by the parser ("expected an indented block").
                }
                self.at_line_start = false;
            }
            if self.pos >= self.chars.len() {
                break;
            }
            let c = self.chars[self.pos];
            let (line, col) = (self.line, self.col());
            match c {
                ' ' | '\t' | '\x0c' => {
                    self.pos += 1;
                }
                '\r' => {
                    self.pos += 1;
                }
                '\n' => {
                    self.pos += 1;
                    if self.depth.is_empty() {
                        if !matches!(self.out.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
                            self.push(Tok::Newline, line, col);
                        }
                        self.at_line_start = true;
                    }
                    self.line += 1;
                    self.line_start = self.pos;
                }
                '#' => {
                    while self.pos < self.chars.len() && self.chars[self.pos] != '\n' {
                        self.pos += 1;
                    }
                }
                '\\' => {
                    let mut p = self.pos + 1;
                    if self.chars.get(p) == Some(&'\r') {
                        p += 1;
                    }
                    if self.chars.get(p) == Some(&'\n') {
                        self.pos = p + 1;
                        self.line += 1;
                        self.line_start = self.pos;
                        if self.pos >= self.chars.len() {
                            return Err(self.err("unexpected EOF while parsing"));
                        }
                    } else {
                        return Err(SyntaxErr::new(
                            "unexpected character after line continuation character",
                            line + self.base_line,
                            col + 1,
                        ));
                    }
                }
                _ if c.is_ascii_digit() || (c == '.' && self.peek(1).is_ascii_digit()) => {
                    self.number()?;
                }
                _ if is_id_start(c) => {
                    let start = self.pos;
                    while self.pos < self.chars.len() && is_id_continue(self.chars[self.pos]) {
                        self.pos += 1;
                    }
                    let word: String = self.chars[start..self.pos].iter().collect();
                    let lower = word.to_ascii_lowercase();
                    let q = self.peek(0);
                    if (q == '\'' || q == '"')
                        && matches!(
                            lower.as_str(),
                            "r" | "b" | "f" | "u" | "rb" | "br" | "fr" | "rf"
                        )
                    {
                        self.string(&lower, line, col)?;
                    } else {
                        self.push(Tok::Name(word), line, col);
                    }
                }
                '\'' | '"' => self.string("", line, col)?,
                _ => {
                    let mut matched = None;
                    for op in OPS {
                        let n = op.len();
                        if self.pos + n <= self.chars.len()
                            && self.chars[self.pos..self.pos + n]
                                .iter()
                                .copied()
                                .eq(op.chars())
                        {
                            matched = Some(*op);
                            break;
                        }
                    }
                    let Some(op) = matched else {
                        if c == '$' || c == '?' || c == '`' {
                            return Err(SyntaxErr::new(
                                "invalid syntax",
                                line + self.base_line,
                                col,
                            ));
                        }
                        return Err(SyntaxErr::new(
                            format!("invalid character '{c}' (U+{:04X})", c as u32),
                            line + self.base_line,
                            col,
                        ));
                    };
                    if op == "!" {
                        return Err(SyntaxErr::new("invalid syntax", line + self.base_line, col));
                    }
                    self.pos += op.len();
                    match op {
                        "(" | "[" | "{" => self.depth.push((op.chars().next().unwrap(), line, col)),
                        ")" | "]" | "}" => {
                            let close = op.chars().next().unwrap();
                            match self.depth.pop() {
                                None => {
                                    return Err(SyntaxErr::new(
                                        format!("unmatched '{close}'"),
                                        line + self.base_line,
                                        col,
                                    ))
                                }
                                Some((open, oline, _)) => {
                                    let expect = match open {
                                        '(' => ')',
                                        '[' => ']',
                                        _ => '}',
                                    };
                                    if expect != close {
                                        let msg = if oline == line {
                                            format!("closing parenthesis '{close}' does not match opening parenthesis '{open}'")
                                        } else {
                                            format!("closing parenthesis '{close}' does not match opening parenthesis '{open}' on line {}", oline + self.base_line)
                                        };
                                        return Err(SyntaxErr::new(
                                            msg,
                                            line + self.base_line,
                                            col,
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    self.push(Tok::Op(op), line, col);
                }
            }
        }
        if let Some((open, line, col)) = self.depth.last() {
            return Err(SyntaxErr::new(
                format!("'{open}' was never closed"),
                *line + self.base_line,
                *col,
            ));
        }
        if !matches!(self.out.last().map(|t| &t.tok), Some(Tok::Newline) | None) {
            let (l, c) = (self.line, self.col());
            self.push(Tok::Newline, l, c);
        }
        while self.indents.len() > 1 {
            self.indents.pop();
            let l = self.line;
            self.push(Tok::Dedent, l, 0);
        }
        let l = self.line;
        self.push(Tok::Eof, l, 0);
        Ok((self.out, self.warnings))
    }

    /// Whether the previous logical line ended with `:` (a block opener).
    fn expects_block(&self) -> bool {
        let n = self.out.len();
        n >= 2 && matches!(self.out[n - 2].tok, Tok::Op(":"))
    }

    fn number(&mut self) -> Result<(), SyntaxErr> {
        let (line, col) = (self.line, self.col());
        let start = self.pos;
        let c0 = self.peek(0);
        let c1 = self.peek(1).to_ascii_lowercase();
        if c0 == '0' && (c1 == 'x' || c1 == 'o' || c1 == 'b') {
            let radix = match c1 {
                'x' => 16,
                'o' => 8,
                _ => 2,
            };
            self.pos += 2;
            let mut digits = String::new();
            while self.pos < self.chars.len() {
                let ch = self.chars[self.pos];
                if ch == '_' {
                    self.pos += 1;
                    continue;
                }
                if ch.is_ascii_alphanumeric() {
                    if ch.to_digit(radix).is_none() {
                        let name = match radix {
                            16 => "hexadecimal",
                            8 => "octal",
                            _ => "binary",
                        };
                        return Err(SyntaxErr::new(
                            format!("invalid digit '{ch}' in {name} literal"),
                            line + self.base_line,
                            self.col(),
                        ));
                    }
                    digits.push(ch);
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if digits.is_empty() {
                return Err(SyntaxErr::new("invalid syntax", line + self.base_line, col));
            }
            self.push(Tok::Int(digits, radix), line, col);
            return Ok(());
        }
        let mut text = String::new();
        let mut is_float = false;
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            if ch.is_ascii_digit() {
                text.push(ch);
            } else if ch == '_' && self.peek(1).is_ascii_digit() {
            } else if ch == '.' && !is_float && !text.contains('e') {
                is_float = true;
                text.push('.');
            } else if (ch == 'e' || ch == 'E') && !text.contains('e') {
                let n = self.peek(1);
                let n2 = self.peek(2);
                if n.is_ascii_digit() || ((n == '+' || n == '-') && n2.is_ascii_digit()) {
                    is_float = true;
                    text.push('e');
                    if n == '+' || n == '-' {
                        text.push(n);
                        self.pos += 1;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
            self.pos += 1;
        }
        let ch = self.peek(0);
        if ch == 'j' || ch == 'J' {
            self.pos += 1;
            let v: f64 = text.parse().unwrap_or(0.0);
            self.push(Tok::Imag(v), line, col);
            return Ok(());
        }
        if is_id_start(ch) {
            let _ = start;
            let _ = is_float;
            return Err(SyntaxErr::new(
                "invalid decimal literal",
                line + self.base_line,
                self.col() - 1,
            ));
        }
        if is_float {
            let v: f64 = text.parse().unwrap_or(f64::INFINITY);
            self.push(Tok::Float(v), line, col);
        } else {
            let t = text.trim_start_matches('0');
            if t.len() != text.len() && !t.is_empty() {
                return Err(SyntaxErr::new(
                    "leading zeros in decimal integer literals are not permitted; use an 0o prefix for octal integers",
                    line + self.base_line,
                    col,
                ));
            }
            self.push(
                Tok::Int(if t.is_empty() { "0".into() } else { t.into() }, 10),
                line,
                col,
            );
        }
        Ok(())
    }

    fn string(&mut self, prefix: &str, line: u32, col: u32) -> Result<(), SyntaxErr> {
        let raw = prefix.contains('r');
        let bytes = prefix.contains('b');
        let fmt = prefix.contains('f');
        let q = self.chars[self.pos];
        let triple = self.peek(1) == q && self.peek(2) == q;
        self.pos += if triple { 3 } else { 1 };
        if fmt {
            let parts = self.fstring_body(q, triple, raw, line, col, false)?;
            self.push(Tok::FStr(parts), line, col);
            return Ok(());
        }
        let mut body = String::new();
        loop {
            if self.pos >= self.chars.len() {
                return Err(self.unterminated(triple, line, col));
            }
            let ch = self.chars[self.pos];
            if ch == q && (!triple || (self.peek(1) == q && self.peek(2) == q)) {
                self.pos += if triple { 3 } else { 1 };
                break;
            }
            if ch == '\n' {
                if !triple {
                    return Err(self.unterminated(false, line, col));
                }
                self.line += 1;
                self.line_start = self.pos + 1;
            }
            if ch == '\\' {
                let next = self.peek(1);
                if next == '\n' {
                    if raw {
                        body.push('\\');
                        body.push('\n');
                    }
                    self.pos += 2;
                    self.line += 1;
                    self.line_start = self.pos;
                    continue;
                }
                if next == '\0' && self.pos + 1 >= self.chars.len() {
                    return Err(self.unterminated(triple, line, col));
                }
                body.push('\\');
                body.push(next);
                self.pos += 2;
                continue;
            }
            body.push(ch);
            self.pos += 1;
        }
        if bytes {
            if let Some(bad) = body.chars().find(|c| !c.is_ascii()) {
                let _ = bad;
                return Err(SyntaxErr::new(
                    "bytes can only contain ASCII literal characters",
                    line + self.base_line,
                    col,
                ));
            }
            let v = if raw {
                body.into_bytes()
            } else {
                let s = unescape(&body, true, line, &mut self.warnings)?;
                s.chars().map(|c| c as u32 as u8).collect()
            };
            self.push(Tok::Bytes(v), line, col);
        } else {
            let v = if raw {
                body
            } else {
                unescape(&body, false, line + self.base_line, &mut self.warnings)?
            };
            self.push(Tok::Str(v), line, col);
        }
        Ok(())
    }

    fn unterminated(&self, triple: bool, line: u32, col: u32) -> SyntaxErr {
        if triple {
            SyntaxErr::new(
                format!(
                    "unterminated triple-quoted string literal (detected at line {})",
                    self.line + self.base_line
                ),
                line + self.base_line,
                col,
            )
        } else {
            SyntaxErr::new(
                format!(
                    "unterminated string literal (detected at line {})",
                    line + self.base_line
                ),
                line + self.base_line,
                col,
            )
        }
    }

    /// Scans an f-string body up to its closing quote (or, for a format spec,
    /// up to the `}` that ends the replacement field).
    fn fstring_body(
        &mut self,
        q: char,
        triple: bool,
        raw: bool,
        line: u32,
        col: u32,
        in_spec: bool,
    ) -> Result<Vec<FPart>, SyntaxErr> {
        let mut parts = vec![];
        let mut lit = String::new();
        loop {
            if self.pos >= self.chars.len() {
                return Err(self.unterminated(triple, line, col));
            }
            let ch = self.chars[self.pos];
            if in_spec && ch == '}' {
                break;
            }
            if !in_spec && ch == q && (!triple || (self.peek(1) == q && self.peek(2) == q)) {
                self.pos += if triple { 3 } else { 1 };
                break;
            }
            if ch == '\n' {
                if !triple {
                    return Err(self.unterminated(false, line, col));
                }
                self.line += 1;
                self.line_start = self.pos + 1;
            }
            if ch == '{' {
                if self.peek(1) == '{' && !in_spec {
                    lit.push('{');
                    self.pos += 2;
                    continue;
                }
                if !lit.is_empty() {
                    let l = std::mem::take(&mut lit);
                    let l = if raw {
                        l
                    } else {
                        unescape(&l, false, line + self.base_line, &mut self.warnings)?
                    };
                    parts.push(FPart::Lit(l));
                }
                self.pos += 1;
                parts.push(self.fstring_field(q, triple, raw, line, col)?);
                continue;
            }
            if ch == '}' {
                if self.peek(1) == '}' {
                    lit.push('}');
                    self.pos += 2;
                    continue;
                }
                return Err(SyntaxErr::new(
                    "f-string: single '}' is not allowed",
                    self.line + self.base_line,
                    self.col(),
                ));
            }
            if ch == '\\' && !raw {
                let next = self.peek(1);
                if next == '\n' {
                    self.pos += 2;
                    self.line += 1;
                    self.line_start = self.pos;
                    continue;
                }
                lit.push('\\');
                lit.push(next);
                self.pos += 2;
                continue;
            }
            lit.push(ch);
            self.pos += 1;
        }
        if !lit.is_empty() {
            let l = if raw {
                lit
            } else {
                unescape(&lit, false, line + self.base_line, &mut self.warnings)?
            };
            parts.push(FPart::Lit(l));
        }
        Ok(parts)
    }

    fn fstring_field(
        &mut self,
        q: char,
        triple: bool,
        raw: bool,
        line: u32,
        col: u32,
    ) -> Result<FPart, SyntaxErr> {
        let (eline, ecol) = (self.line + self.base_line, self.col());
        let start = self.pos;
        let mut depth = 0i32;
        let mut end = None;
        let mut conversion = None;
        let mut debug = None;
        // Find the end of the expression: a top-level `!`, `:`, `=` (debug) or `}`.
        while self.pos < self.chars.len() {
            let ch = self.chars[self.pos];
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' => depth -= 1,
                '}' if depth > 0 => depth -= 1,
                '}' => {
                    end = Some(self.pos);
                    break;
                }
                '\'' | '"' => {
                    // A nested string literal (PEP 701 allows the same quote).
                    let quote = ch;
                    let tri = self.peek(1) == quote && self.peek(2) == quote;
                    if !tri && quote == q && !triple && false {
                        break;
                    }
                    self.pos += if tri { 3 } else { 1 };
                    while self.pos < self.chars.len() {
                        let c2 = self.chars[self.pos];
                        if c2 == '\\' {
                            self.pos += 2;
                            continue;
                        }
                        if c2 == quote && (!tri || (self.peek(1) == quote && self.peek(2) == quote))
                        {
                            break;
                        }
                        if c2 == '\n' {
                            self.line += 1;
                            self.line_start = self.pos + 1;
                        }
                        self.pos += 1;
                    }
                    self.pos += if tri { 3 } else { 1 };
                    continue;
                }
                '!' if depth == 0 && self.peek(1) != '=' => {
                    end = Some(self.pos);
                    break;
                }
                ':' if depth == 0 => {
                    end = Some(self.pos);
                    break;
                }
                '=' if depth == 0
                    && self.peek(1) != '='
                    && !matches!(
                        self.chars.get(self.pos.wrapping_sub(1)),
                        Some('=' | '!' | '<' | '>')
                    ) =>
                {
                    // Self-documenting expression `{x=}`.
                    let text: String = self.chars[start..=self.pos].iter().collect();
                    let expr_end = self.pos;
                    self.pos += 1;
                    // Allow whitespace after '='
                    let mut ws = String::new();
                    while self.peek(0) == ' ' {
                        ws.push(' ');
                        self.pos += 1;
                    }
                    debug = Some(format!("{text}{ws}"));
                    end = Some(expr_end);
                    break;
                }
                '\n' => {
                    if !triple {
                        return Err(self.unterminated(false, line, col));
                    }
                    self.line += 1;
                    self.line_start = self.pos + 1;
                }
                _ => {}
            }
            self.pos += 1;
        }
        let Some(end) = end else {
            return Err(self.unterminated(triple, line, col));
        };
        let src: String = self.chars[start..end].iter().collect();
        if src.trim().is_empty() {
            return Err(SyntaxErr::new(
                "f-string: valid expression required before '}'",
                eline,
                ecol,
            ));
        }
        if self.peek(0) == '!' {
            let c = self.peek(1);
            if !matches!(c, 's' | 'r' | 'a') {
                return Err(SyntaxErr::new(
                    "f-string: invalid conversion character: expected 's', 'r', or 'a'",
                    self.line + self.base_line,
                    self.col(),
                ));
            }
            conversion = Some(c);
            self.pos += 2;
        }
        let mut spec = None;
        if self.peek(0) == ':' {
            self.pos += 1;
            spec = Some(self.fstring_body(q, triple, raw, line, col, true)?);
        }
        if self.peek(0) != '}' {
            return Err(SyntaxErr::new(
                "f-string: expecting '}'",
                self.line + self.base_line,
                self.col(),
            ));
        }
        self.pos += 1;
        if debug.is_some() && conversion.is_none() && spec.is_none() {
            conversion = Some('r');
        }
        Ok(FPart::Expr {
            src,
            line: eline,
            col: ecol,
            conversion,
            spec,
            debug,
        })
    }
}

/// Processes backslash escapes of a non-raw literal.
pub fn unescape(
    s: &str,
    bytes: bool,
    line: u32,
    warnings: &mut Vec<Warning>,
) -> Result<String, SyntaxErr> {
    if !s.contains('\\') {
        return Ok(s.to_string());
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let n = *chars.get(i + 1).unwrap_or(&'\\');
        i += 2;
        match n {
            '\n' => {}
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            '"' => out.push('"'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0c'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'v' => out.push('\x0b'),
            '0'..='7' => {
                let mut v = n.to_digit(8).unwrap();
                let mut k = 0;
                while k < 2 && i < chars.len() && chars[i].is_digit(8) {
                    v = v * 8 + chars[i].to_digit(8).unwrap();
                    i += 1;
                    k += 1;
                }
                out.push(char::from_u32(v).unwrap_or('\u{fffd}'));
            }
            'x' => {
                let hex: String = chars.iter().skip(i).take(2).collect();
                if hex.len() < 2 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(SyntaxErr::new(
                        format!(
                            "({}) truncated \\xXX escape",
                            if bytes { "value error" } else { "unicode error" }
                        )
                        .replace(
                            "(unicode error) truncated",
                            "(unicode error) 'unicodeescape' codec can't decode bytes in position 0-1: truncated",
                        ),
                        line,
                        0,
                    ));
                }
                i += 2;
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
            }
            'u' | 'U' if !bytes => {
                let n_digits = if n == 'u' { 4 } else { 8 };
                let hex: String = chars.iter().skip(i).take(n_digits).collect();
                if hex.len() < n_digits || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(SyntaxErr::new(
                        format!("(unicode error) 'unicodeescape' codec can't decode bytes in position 0-{}: truncated \\{}{} escape", n_digits + 1, n, "X".repeat(n_digits)),
                        line,
                        0,
                    ));
                }
                i += n_digits;
                match char::from_u32(u32::from_str_radix(&hex, 16).unwrap()) {
                    Some(ch) => out.push(ch),
                    None => {
                        return Err(SyntaxErr::new(
                            "(unicode error) 'unicodeescape' codec can't decode bytes: illegal Unicode character",
                            line,
                            0,
                        ))
                    }
                }
            }
            'N' if !bytes && chars.get(i) == Some(&'{') => {
                let end = chars[i..].iter().position(|&c| c == '}').map(|p| p + i);
                let Some(end) = end else {
                    return Err(SyntaxErr::new("(unicode error) 'unicodeescape' codec can't decode bytes: malformed \\N character escape", line, 0));
                };
                let name: String = chars[i + 1..end].iter().collect();
                i = end + 1;
                match unicode_name(&name) {
                    Some(ch) => out.push(ch),
                    None => {
                        return Err(SyntaxErr::new(
                            "(unicode error) 'unicodeescape' codec can't decode bytes: unknown Unicode character name",
                            line,
                            0,
                        ))
                    }
                }
            }
            other => {
                warnings.push(Warning {
                    line,
                    msg: format!("invalid escape sequence '\\{other}'"),
                });
                out.push('\\');
                out.push(other);
            }
        }
    }
    Ok(out)
}

fn unicode_name(name: &str) -> Option<char> {
    Some(match name.to_ascii_uppercase().as_str() {
        "BULLET" => '\u{2022}',
        "EM DASH" => '\u{2014}',
        "EN DASH" => '\u{2013}',
        "DEGREE SIGN" => '\u{b0}',
        "GREEK SMALL LETTER ALPHA" => '\u{3b1}',
        "GREEK SMALL LETTER BETA" => '\u{3b2}',
        "GREEK SMALL LETTER PI" => '\u{3c0}',
        "GREEK CAPITAL LETTER DELTA" => '\u{394}',
        "COPYRIGHT SIGN" => '\u{a9}',
        "CHECK MARK" => '\u{2713}',
        "HEAVY CHECK MARK" => '\u{2714}',
        "BLACK STAR" => '\u{2605}',
        "WHITE STAR" => '\u{2606}',
        "SNOWMAN" => '\u{2603}',
        "EURO SIGN" => '\u{20ac}',
        "POUND SIGN" => '\u{a3}',
        "NO-BREAK SPACE" => '\u{a0}',
        "HORIZONTAL ELLIPSIS" => '\u{2026}',
        "RIGHTWARDS ARROW" => '\u{2192}',
        "LEFTWARDS ARROW" => '\u{2190}',
        "INFINITY" => '\u{221e}',
        "MULTIPLICATION SIGN" => '\u{d7}',
        "DIVISION SIGN" => '\u{f7}',
        "SPACE" => ' ',
        "LATIN SMALL LETTER A" => 'a',
        "LATIN CAPITAL LETTER A" => 'A',
        "GRINNING FACE" => '\u{1f600}',
        _ => return None,
    })
}

pub fn tokenize(src: &str) -> Result<(Vec<Token>, Vec<Warning>), SyntaxErr> {
    Lexer::new(src).tokenize()
}

/// Tokenizes an f-string replacement expression that sits at (line, col).
pub fn tokenize_expr(src: &str, line: u32, col: u32) -> Result<Vec<Token>, SyntaxErr> {
    let wrapped = format!("({src})");
    let mut lx = Lexer::new(&wrapped);
    lx.base_line = line.saturating_sub(1);
    lx.base_col = col.saturating_sub(1);
    lx.tokenize().map(|(t, _)| t)
}
