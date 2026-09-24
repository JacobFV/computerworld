//! Tokenizer: identifiers, numbers (incl. BigInt), strings, template literals
//! (with nesting), regular-expression literals and punctuators. Each token
//! records whether a line terminator preceded it, for automatic semicolons.

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    /// An identifier written with escapes (never a keyword).
    EscapedIdent(String),
    PrivateName(String),
    Num(f64),
    BigInt(String),
    Str(String),
    /// Complete template with no substitutions: (cooked, raw).
    Template(Option<String>, String),
    TemplateHead(Option<String>, String),
    TemplateMiddle(Option<String>, String),
    TemplateTail(Option<String>, String),
    Regex(String, String),
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub nl_before: bool,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct SyntaxErr {
    pub msg: String,
    pub line: u32,
    /// 1-based column of the caret.
    pub col: u32,
    /// Number of carets to draw.
    pub len: u32,
}

const PUNCTS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=",
    "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=",
    "**", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%",
    "&", "|", "^", "!", "~", "?", ":", "=", ".", "@", "#",
];

fn is_id_start(c: char) -> bool {
    c == '$' || c == '_' || c.is_alphabetic()
}
fn is_id_part(c: char) -> bool {
    c == '$' || c == '_' || c.is_alphanumeric() || c == '\u{200c}' || c == '\u{200d}'
}
fn is_line_term(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

pub struct Lexer<'a> {
    c: &'a [char],
    pos: usize,
    line: u32,
    line_start: usize,
    out: Vec<Token>,
    /// Brace stack: true marks a `${` opened inside a template.
    braces: Vec<bool>,
}

impl<'a> Lexer<'a> {
    pub fn new(c: &'a [char]) -> Self {
        Lexer {
            c,
            pos: 0,
            line: 1,
            line_start: 0,
            out: vec![],
            braces: vec![],
        }
    }
    fn peek(&self, off: usize) -> char {
        *self.c.get(self.pos + off).unwrap_or(&'\0')
    }
    fn col(&self) -> u32 {
        (self.pos - self.line_start) as u32 + 1
    }
    fn err(&self, msg: impl Into<String>) -> SyntaxErr {
        SyntaxErr {
            msg: msg.into(),
            line: self.line,
            col: self.col(),
            len: 1,
        }
    }
    fn newline(&mut self) {
        // Called with pos just past the terminator.
        self.line += 1;
        self.line_start = self.pos;
    }

    fn regex_allowed(&self) -> bool {
        match self.out.last().map(|t| &t.tok) {
            None => true,
            Some(
                Tok::Num(_)
                | Tok::BigInt(_)
                | Tok::Str(_)
                | Tok::Template(..)
                | Tok::TemplateTail(..)
                | Tok::Regex(..)
                | Tok::PrivateName(_)
                | Tok::EscapedIdent(_),
            ) => false,
            Some(Tok::Ident(name)) => matches!(
                name.as_str(),
                "return"
                    | "typeof"
                    | "instanceof"
                    | "in"
                    | "of"
                    | "new"
                    | "delete"
                    | "void"
                    | "throw"
                    | "case"
                    | "do"
                    | "else"
                    | "yield"
                    | "await"
            ),
            Some(Tok::Punct(p)) => !matches!(*p, ")" | "]" | "}" | "++" | "--"),
            _ => true,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, SyntaxErr> {
        let mut nl = false;
        // Hashbang.
        if self.peek(0) == '#' && self.peek(1) == '!' {
            while self.pos < self.c.len() && !is_line_term(self.c[self.pos]) {
                self.pos += 1;
            }
        }
        loop {
            // Whitespace and comments.
            loop {
                let ch = self.peek(0);
                if self.pos >= self.c.len() {
                    break;
                }
                if ch == '\r' && self.peek(1) == '\n' {
                    self.pos += 2;
                    self.newline();
                    nl = true;
                } else if is_line_term(ch) {
                    self.pos += 1;
                    self.newline();
                    nl = true;
                } else if ch == ' '
                    || ch == '\t'
                    || (!ch.is_ascii() && ch.is_whitespace())
                    || ch == '\u{feff}'
                    || ch == '\u{b}'
                    || ch == '\u{c}'
                {
                    self.pos += 1;
                } else if ch == '/' && self.peek(1) == '/' {
                    while self.pos < self.c.len() && !is_line_term(self.c[self.pos]) {
                        self.pos += 1;
                    }
                } else if ch == '/' && self.peek(1) == '*' {
                    let (sl, sc) = (self.line, self.col());
                    self.pos += 2;
                    loop {
                        if self.pos >= self.c.len() {
                            return Err(SyntaxErr {
                                msg: "Invalid or unexpected token".into(),
                                line: sl,
                                col: sc,
                                len: 1,
                            });
                        }
                        let c2 = self.c[self.pos];
                        if c2 == '*' && self.peek(1) == '/' {
                            self.pos += 2;
                            break;
                        }
                        self.pos += 1;
                        if c2 == '\r' && self.peek(0) == '\n' {
                            self.pos += 1;
                        }
                        if is_line_term(c2) {
                            self.newline();
                            nl = true;
                        }
                    }
                } else {
                    break;
                }
            }
            let (line, col, start) = (self.line, self.col(), self.pos);
            if self.pos >= self.c.len() {
                self.out.push(Token {
                    tok: Tok::Eof,
                    line,
                    col,
                    end_line: line,
                    end_col: col,
                    nl_before: true,
                    start,
                    end: start,
                });
                return Ok(self.out);
            }
            let ch = self.c[self.pos];
            let tok = if is_id_start(ch) || ch == '\\' {
                let (name, escaped) = self.ident()?;
                if escaped {
                    Tok::EscapedIdent(name)
                } else {
                    Tok::Ident(name)
                }
            } else if ch == '#' && is_id_start(self.peek(1)) {
                self.pos += 1;
                let (name, _) = self.ident()?;
                Tok::PrivateName(name)
            } else if ch.is_ascii_digit() || (ch == '.' && self.peek(1).is_ascii_digit()) {
                self.number()?
            } else if ch == '"' || ch == '\'' {
                self.string(ch)?
            } else if ch == '`' {
                self.pos += 1;
                self.template(true)?
            } else if ch == '}' && self.braces.last() == Some(&true) {
                self.braces.pop();
                self.pos += 1;
                self.template(false)?
            } else if ch == '/' && self.regex_allowed() {
                self.regex()?
            } else {
                let mut found = None;
                for p in puncts_from(ch) {
                    let n = p.len();
                    if self.pos + n <= self.c.len()
                        && self.c[self.pos..self.pos + n].iter().copied().eq(p.chars())
                    {
                        // `?.` followed by a digit is `?` then a number.
                        if *p == "?." && self.peek(2).is_ascii_digit() {
                            continue;
                        }
                        found = Some(*p);
                        break;
                    }
                }
                let Some(p) = found else {
                    return Err(self.err("Invalid or unexpected token"));
                };
                self.pos += p.len();
                match p {
                    "{" => self.braces.push(false),
                    "}" => {
                        self.braces.pop();
                    }
                    _ => {}
                }
                Tok::Punct(p)
            };
            if let Tok::TemplateHead(..) | Tok::TemplateMiddle(..) = tok {
                self.braces.push(true);
            }
            let (end_line, end_col) = (self.line, self.col());
            self.out.push(Token {
                tok,
                line,
                col,
                end_line,
                end_col,
                nl_before: nl,
                start,
                end: self.pos,
            });
            nl = false;
        }
    }

    fn ident(&mut self) -> Result<(String, bool), SyntaxErr> {
        // The common case, a run of ASCII identifier characters, in one piece.
        let start = self.pos;
        let mut end = start;
        while end < self.c.len() {
            let ch = self.c[end];
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                end += 1;
            } else {
                break;
            }
        }
        let next = self.c.get(end).copied().unwrap_or('\0');
        let first_ok = end > start && !self.c[start].is_ascii_digit();
        if first_ok && (next.is_ascii() && next != '\\') {
            self.pos = end;
            return Ok((self.c[start..end].iter().collect(), false));
        }
        let mut s = String::new();
        let mut escaped = false;
        loop {
            let ch = self.peek(0);
            if ch == '\\' {
                if self.peek(1) != 'u' {
                    return Err(self.err("Invalid or unexpected token"));
                }
                self.pos += 2;
                let cp = self.unicode_escape()?;
                s.push(cp);
                escaped = true;
                continue;
            }
            if self.pos < self.c.len()
                && (if s.is_empty() {
                    is_id_start(ch)
                } else {
                    is_id_part(ch)
                })
            {
                s.push(ch);
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok((s, escaped))
    }

    fn unicode_escape(&mut self) -> Result<char, SyntaxErr> {
        // After `\u`.
        if self.peek(0) == '{' {
            self.pos += 1;
            let mut v: u32 = 0;
            let mut n = 0;
            while self.peek(0) != '}' {
                let d = self
                    .peek(0)
                    .to_digit(16)
                    .ok_or_else(|| self.err("Invalid Unicode escape sequence"))?;
                v = v
                    .checked_mul(16)
                    .and_then(|x| x.checked_add(d))
                    .ok_or_else(|| self.err("Undefined Unicode code-point"))?;
                self.pos += 1;
                n += 1;
            }
            self.pos += 1;
            if n == 0 || v > 0x10ffff {
                return Err(self.err("Undefined Unicode code-point"));
            }
            return Ok(char::from_u32(v).unwrap_or('\u{fffd}'));
        }
        let mut v: u32 = 0;
        for _ in 0..4 {
            let d = self
                .peek(0)
                .to_digit(16)
                .ok_or_else(|| self.err("Invalid Unicode escape sequence"))?;
            v = v * 16 + d;
            self.pos += 1;
        }
        // A surrogate pair written as two escapes.
        if (0xd800..0xdc00).contains(&v) && self.peek(0) == '\\' && self.peek(1) == 'u' {
            let save = self.pos;
            self.pos += 2;
            let mut lo: u32 = 0;
            let mut ok = true;
            for _ in 0..4 {
                match self.peek(0).to_digit(16) {
                    Some(d) => {
                        lo = lo * 16 + d;
                        self.pos += 1;
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok && (0xdc00..0xe000).contains(&lo) {
                let cp = 0x10000 + ((v - 0xd800) << 10) + (lo - 0xdc00);
                return Ok(char::from_u32(cp).unwrap_or('\u{fffd}'));
            }
            self.pos = save;
        }
        Ok(char::from_u32(v).unwrap_or('\u{fffd}'))
    }

    fn number(&mut self) -> Result<Tok, SyntaxErr> {
        let start = self.pos;
        let c0 = self.peek(0);
        let c1 = self.peek(1).to_ascii_lowercase();
        let radix = if c0 == '0' && (c1 == 'x' || c1 == 'o' || c1 == 'b') {
            match c1 {
                'x' => 16,
                'o' => 8,
                _ => 2,
            }
        } else {
            10
        };
        if radix != 10 {
            self.pos += 2;
            let mut digits = String::new();
            while self.pos < self.c.len() {
                let ch = self.c[self.pos];
                if ch == '_' {
                    self.pos += 1;
                    continue;
                }
                if ch.is_digit(radix) {
                    digits.push(ch);
                    self.pos += 1;
                } else {
                    break;
                }
            }
            if digits.is_empty() {
                return Err(self.err("Invalid or unexpected token"));
            }
            if self.peek(0) == 'n' {
                self.pos += 1;
                let v = u128::from_str_radix(&digits, radix).unwrap_or(0);
                return Ok(Tok::BigInt(v.to_string()));
            }
            if is_id_start(self.peek(0)) {
                return Err(self.err("Invalid or unexpected token"));
            }
            let mut v = 0f64;
            for d in digits.chars() {
                v = v * radix as f64 + d.to_digit(radix).unwrap() as f64;
            }
            return Ok(Tok::Num(v));
        }
        // Legacy octal like 0777 (sloppy mode): treat digits as octal when all < 8.
        if c0 == '0' && self.peek(1).is_ascii_digit() {
            let mut j = self.pos + 1;
            while j < self.c.len() && self.c[j].is_ascii_digit() {
                j += 1;
            }
            let text: String = self.c[self.pos + 1..j].iter().collect();
            if text.chars().all(|d| d < '8') && !(j < self.c.len() && self.c[j] == '.') {
                self.pos = j;
                let mut v = 0f64;
                for d in text.chars() {
                    v = v * 8.0 + d.to_digit(8).unwrap() as f64;
                }
                return Ok(Tok::Num(v));
            }
        }
        let mut text = String::new();
        while self.peek(0).is_ascii_digit() || self.peek(0) == '_' {
            if self.peek(0) != '_' {
                text.push(self.peek(0));
            }
            self.pos += 1;
        }
        if self.peek(0) == 'n' {
            self.pos += 1;
            let t = text.trim_start_matches('0');
            return Ok(Tok::BigInt(if t.is_empty() {
                "0".into()
            } else {
                t.to_string()
            }));
        }
        if self.peek(0) == '.' {
            text.push('.');
            self.pos += 1;
            while self.peek(0).is_ascii_digit() || self.peek(0) == '_' {
                if self.peek(0) != '_' {
                    text.push(self.peek(0));
                }
                self.pos += 1;
            }
        }
        if self.peek(0) == 'e' || self.peek(0) == 'E' {
            let save = self.pos;
            let mut exp = String::from("e");
            self.pos += 1;
            if self.peek(0) == '+' || self.peek(0) == '-' {
                exp.push(self.peek(0));
                self.pos += 1;
            }
            if self.peek(0).is_ascii_digit() {
                while self.peek(0).is_ascii_digit() {
                    exp.push(self.peek(0));
                    self.pos += 1;
                }
                text.push_str(&exp);
            } else {
                self.pos = save;
                return Err(self.err("Invalid or unexpected token"));
            }
        }
        if is_id_start(self.peek(0)) || self.peek(0).is_ascii_digit() {
            return Err(SyntaxErr {
                msg: "Invalid or unexpected token".into(),
                line: self.line,
                col: (start - self.line_start) as u32 + 1,
                len: 1,
            });
        }
        let _ = start;
        let t = if text.starts_with('.') {
            format!("0{text}")
        } else {
            text
        };
        Ok(Tok::Num(t.parse().unwrap_or(f64::NAN)))
    }

    fn escape(&mut self, cooked: &mut String, in_template: bool) -> Result<bool, SyntaxErr> {
        // At char after backslash. Returns false if the escape is invalid (templates).
        let ch = self.peek(0);
        self.pos += 1;
        match ch {
            'n' => cooked.push('\n'),
            't' => cooked.push('\t'),
            'r' => cooked.push('\r'),
            'b' => cooked.push('\x08'),
            'f' => cooked.push('\x0c'),
            'v' => cooked.push('\x0b'),
            '0' if !self.peek(0).is_ascii_digit() => cooked.push('\0'),
            '0'..='7' if !in_template => {
                let mut v = ch.to_digit(8).unwrap();
                let max = if ch <= '3' { 2 } else { 1 };
                let mut k = 0;
                while k < max && self.peek(0).is_digit(8) {
                    v = v * 8 + self.peek(0).to_digit(8).unwrap();
                    self.pos += 1;
                    k += 1;
                }
                cooked.push(char::from_u32(v).unwrap_or('\0'));
            }
            '8' | '9' if !in_template => cooked.push(ch),
            'x' => {
                let h: String = [self.peek(0), self.peek(1)].iter().collect();
                if h.chars().all(|c| c.is_ascii_hexdigit()) && h.len() == 2 {
                    self.pos += 2;
                    cooked.push(char::from_u32(u32::from_str_radix(&h, 16).unwrap()).unwrap());
                } else {
                    if in_template {
                        return Ok(false);
                    }
                    return Err(self.err("Invalid hexadecimal escape sequence"));
                }
            }
            'u' => match self.unicode_escape() {
                Ok(c) => cooked.push(c),
                Err(e) => {
                    if in_template {
                        return Ok(false);
                    }
                    return Err(e);
                }
            },
            '\r' => {
                if self.peek(0) == '\n' {
                    self.pos += 1;
                }
                self.newline();
            }
            c if is_line_term(c) => self.newline(),
            c => {
                if in_template && c.is_ascii_digit() {
                    return Ok(false);
                }
                cooked.push(c)
            }
        }
        Ok(true)
    }

    fn string(&mut self, q: char) -> Result<Tok, SyntaxErr> {
        let (sl, sc, sp) = (self.line, self.col(), self.pos);
        self.pos += 1;
        let mut s = String::new();
        loop {
            if self.pos >= self.c.len() || self.c[self.pos] == '\n' || self.c[self.pos] == '\r' {
                return Err(SyntaxErr {
                    msg: "Invalid or unexpected token".into(),
                    line: sl,
                    col: sc,
                    len: (self.pos - sp) as u32,
                });
            }
            let ch = self.c[self.pos];
            if ch == q {
                self.pos += 1;
                return Ok(Tok::Str(s));
            }
            if ch == '\\' {
                self.pos += 1;
                self.escape(&mut s, false)?;
                continue;
            }
            s.push(ch);
            self.pos += 1;
        }
    }

    /// Scans template characters up to `${` or the closing backtick.
    fn template(&mut self, head: bool) -> Result<Tok, SyntaxErr> {
        let (sl, sc) = (self.line, self.col());
        let mut cooked = String::new();
        let mut raw = String::new();
        let mut valid = true;
        loop {
            if self.pos >= self.c.len() {
                let _ = (sl, sc);
                return Err(SyntaxErr {
                    msg: "Unexpected end of input".into(),
                    line: self.line,
                    col: self.col(),
                    len: 0,
                });
            }
            let ch = self.c[self.pos];
            if ch == '`' {
                self.pos += 1;
                let c = if valid { Some(cooked) } else { None };
                return Ok(if head {
                    Tok::Template(c, raw)
                } else {
                    Tok::TemplateTail(c, raw)
                });
            }
            if ch == '$' && self.peek(1) == '{' {
                self.pos += 2;
                let c = if valid { Some(cooked) } else { None };
                return Ok(if head {
                    Tok::TemplateHead(c, raw)
                } else {
                    Tok::TemplateMiddle(c, raw)
                });
            }
            if ch == '\\' {
                let start = self.pos;
                self.pos += 1;
                if !self.escape(&mut cooked, true)? {
                    valid = false;
                }
                let r: String = self.c[start..self.pos].iter().collect();
                raw.push_str(&r.replace("\r\n", "\n").replace('\r', "\n"));
                continue;
            }
            if ch == '\r' {
                self.pos += 1;
                if self.peek(0) == '\n' {
                    self.pos += 1;
                }
                self.newline();
                cooked.push('\n');
                raw.push('\n');
                continue;
            }
            if is_line_term(ch) {
                self.pos += 1;
                self.newline();
                cooked.push(ch);
                raw.push(ch);
                continue;
            }
            cooked.push(ch);
            raw.push(ch);
            self.pos += 1;
        }
    }

    fn regex(&mut self) -> Result<Tok, SyntaxErr> {
        let (sl, sc) = (self.line, self.col());
        self.pos += 1;
        let mut body = String::new();
        let mut in_class = false;
        loop {
            if self.pos >= self.c.len() || is_line_term(self.c[self.pos]) {
                return Err(SyntaxErr {
                    msg: "Invalid regular expression: missing /".into(),
                    line: sl,
                    col: sc,
                    len: 1,
                });
            }
            let ch = self.c[self.pos];
            self.pos += 1;
            if ch == '\\' {
                body.push(ch);
                if self.pos < self.c.len() {
                    body.push(self.c[self.pos]);
                    self.pos += 1;
                }
                continue;
            }
            if ch == '[' {
                in_class = true;
            } else if ch == ']' {
                in_class = false;
            } else if ch == '/' && !in_class {
                break;
            }
            body.push(ch);
        }
        let mut flags = String::new();
        while is_id_part(self.peek(0)) {
            flags.push(self.peek(0));
            self.pos += 1;
        }
        for (i, f) in flags.chars().enumerate() {
            if !"dgimsuyv".contains(f) || flags.chars().take(i).any(|g| g == f) {
                return Err(SyntaxErr {
                    msg: "Invalid regular expression flags".into(),
                    line: sl,
                    col: sc,
                    len: 1,
                });
            }
        }
        Ok(Tok::Regex(body, flags))
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, SyntaxErr> {
    let chars: Vec<char> = src.chars().collect();
    tokenize_chars(&chars)
}

/// `tokenize` over the source's characters, already collected.
pub fn tokenize_chars(chars: &[char]) -> Result<Vec<Token>, SyntaxErr> {
    Lexer::new(chars).tokenize()
}

/// The punctuators that start with `c`, in `PUNCTS` order (longest first), so
/// the first that matches is the one a scan of all of them finds.
fn puncts_from(c: char) -> &'static [&'static str] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<Vec<&'static str>>> = OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = vec![Vec::new(); 128];
        for p in PUNCTS {
            let b = p.as_bytes()[0] as usize;
            t[b].push(*p);
        }
        t
    });
    if (c as u32) < 128 {
        &t[c as usize]
    } else {
        &[]
    }
}
