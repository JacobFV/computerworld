//! A full `sed`: the real line-editing language, not a `s///` wrapper.
//!
//! Every command of the POSIX set plus the GNU addressing extensions people actually
//! type — `N~step`, `addr,+N`, `addr,~N`, `0,/re/`, `!` — runs over a proper cycle
//! with a pattern space, a hold space, an append queue and branch labels. Basic and
//! extended regular expressions are both translated to the host engine; the one thing
//! this world cannot honour is a backreference *inside* a pattern (`\(a\)\1`), which
//! the engine does not implement, so it is refused by name instead of silently
//! matching something else.
use crate::shell::{self, Fail};
use crate::Computer;
use std::collections::BTreeMap;

// ---------------------------------------------------------------- regex

/// Translates a POSIX BRE or ERE into the host engine's syntax.
pub(crate) fn translate(pattern: &str, extended: bool) -> Result<String, Fail> {
    let b: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // `*` is a literal at the start of an expression; `^`/`$` anchor only at the edges.
    let mut at_start = true;
    while i < b.len() {
        let ch = b[i];
        if ch == '[' {
            let (text, used) = bracket(&b, i)?;
            out.push_str(&text);
            i += used;
            at_start = false;
            continue;
        }
        if ch == '\\' {
            let Some(next) = b.get(i + 1).copied() else {
                return Err(Fail::usage("sed: trailing backslash in regular expression"));
            };
            i += 2;
            match next {
                '1'..='9' => {
                    return Err(Fail::usage(format!(
                        "sed: backreference `\\{next}` inside a pattern is not supported by this \
                         world's regex engine; rewrite the expression without it"
                    )))
                }
                '(' | ')' | '{' | '}' | '|' | '+' | '?' if !extended => {
                    out.push(next);
                    at_start = next == '(' || next == '|';
                }
                'n' => {
                    out.push_str("\\n");
                    at_start = false;
                }
                't' => {
                    out.push_str("\\t");
                    at_start = false;
                }
                'r' => {
                    out.push_str("\\r");
                    at_start = false;
                }
                'w' | 'W' | 's' | 'S' | 'b' | 'B' | 'd' | 'D' => {
                    out.push('\\');
                    out.push(next);
                    at_start = false;
                }
                // GNU's word-edge operators; this engine has no lookaround, so both
                // collapse onto a word boundary, which is the same answer in practice.
                '<' | '>' => {
                    out.push_str("\\b");
                    at_start = false;
                }
                '`' => {
                    out.push_str("\\A");
                    at_start = false;
                }
                '\'' => {
                    out.push_str("\\z");
                    at_start = false;
                }
                other => {
                    out.push_str(&regex::escape(&other.to_string()));
                    at_start = false;
                }
            }
            continue;
        }
        i += 1;
        match ch {
            '(' | ')' | '{' | '}' | '|' | '+' | '?' if !extended => {
                out.push_str(&regex::escape(&ch.to_string()));
                at_start = false;
            }
            '(' | '|' if extended => {
                out.push(ch);
                at_start = true;
            }
            '*' if at_start => {
                out.push_str("\\*");
                at_start = false;
            }
            '^' => {
                if at_start {
                    out.push('^');
                } else {
                    out.push_str("\\^");
                }
                // `^` does not itself consume an operand, so `^*` is still a literal.
            }
            '$' => {
                // An anchor only at the very end, before `\)` or before `\|`.
                let ends = i >= b.len()
                    || (!extended
                        && b.get(i) == Some(&'\\')
                        && matches!(b.get(i + 1), Some(')') | Some('|')))
                    || (extended && matches!(b.get(i), Some(')') | Some('|')));
                out.push_str(if ends { "$" } else { "\\$" });
                at_start = false;
            }
            other => {
                out.push(other);
                at_start = false;
            }
        }
    }
    Ok(out)
}
/// Copies a bracket expression through unchanged; `]` first and `[:class:]` included.
fn bracket(b: &[char], start: usize) -> Result<(String, usize), Fail> {
    let mut out = String::from("[");
    let mut i = start + 1;
    if b.get(i) == Some(&'^') {
        out.push('^');
        i += 1;
    }
    if b.get(i) == Some(&']') {
        out.push_str("\\]");
        i += 1;
    }
    while i < b.len() && b[i] != ']' {
        if b[i] == '[' && matches!(b.get(i + 1), Some(':') | Some('.') | Some('=')) {
            let kind = b[i + 1];
            let close = format!("{kind}]");
            let rest: String = b[i..].iter().collect();
            match rest[2..].find(&close) {
                Some(k) => {
                    out.push_str(&rest[..2 + k + 2]);
                    i += rest[..2 + k + 2].chars().count();
                    continue;
                }
                None => return Err(Fail::usage("sed: unterminated character class")),
            }
        }
        if b[i] == '\\' && i + 1 < b.len() {
            // Inside a bracket POSIX has no escapes, but every engine accepts `\]`
            // and `\\`; pass both through so the host engine agrees.
            out.push('\\');
            out.push(b[i + 1]);
            i += 2;
            continue;
        }
        if b[i] == '[' {
            out.push_str("\\[");
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    if i >= b.len() {
        return Err(Fail::usage("sed: unterminated address regex"));
    }
    out.push(']');
    Ok((out, i + 1 - start))
}
fn build(pattern: &str, extended: bool, ignore_case: bool, multiline: bool) -> Result<Regex, Fail> {
    let source = translate(pattern, extended)?;
    regex::RegexBuilder::new(&source)
        .case_insensitive(ignore_case)
        .multi_line(multiline)
        .build()
        .map(Regex)
        .map_err(|e| Fail::usage(format!("sed: bad regular expression /{pattern}/: {e}")))
}
#[derive(Clone, Debug)]
struct Regex(regex::Regex);

// ---------------------------------------------------------------- program

#[derive(Clone, Debug)]
enum Addr {
    Line(usize),
    Last,
    /// `first~step`: every step-th line from `first`.
    Step(usize, usize),
    /// An empty `//` reuses the last regular expression applied.
    Re(Option<Regex>),
}
#[derive(Clone, Debug)]
enum End {
    Addr(Addr),
    /// `addr,+N`
    Plus(usize),
    /// `addr,~N`: up to the next line whose number is a multiple of N.
    Multiple(usize),
}
#[derive(Clone, Debug)]
enum Sel {
    All,
    One(Addr),
    Range(Addr, End),
}
#[derive(Clone, Debug)]
enum Part {
    Literal(String),
    Whole,
    Group(usize),
    Case(char),
}
#[derive(Clone, Debug)]
enum Kind {
    /// Opens a `{ … }` block; `end` is the index just past its closing brace.
    Block(usize),
    BlockEnd,
    Subst {
        re: Regex,
        parts: Vec<Part>,
        global: bool,
        nth: usize,
        print: bool,
        write: Option<String>,
    },
    Transliterate(Vec<char>, Vec<char>),
    Print,
    PrintFirst,
    Delete,
    DeleteFirst,
    Append(String),
    Insert(String),
    Change(String),
    ReadFile(String),
    ReadLine(String),
    WriteFile(String),
    WriteFirst(String),
    Next,
    NextAppend,
    Hold,
    HoldAppend,
    Get,
    GetAppend,
    Exchange,
    Branch(Option<String>),
    BranchIfSub(Option<String>),
    BranchIfNoSub(Option<String>),
    Label(String),
    Quit(i32),
    QuitSilent(i32),
    LineNumber,
    List(Option<usize>),
    Zap,
    FileName,
    Nop,
}
#[derive(Clone, Debug)]
struct Inst {
    sel: Sel,
    negate: bool,
    kind: Kind,
}

struct Compiler {
    b: Vec<char>,
    i: usize,
    extended: bool,
    out: Vec<Inst>,
    open: Vec<usize>,
}
impl Compiler {
    fn peek(&self) -> Option<char> {
        self.b.get(self.i).copied()
    }
    fn skip_blank(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) {
            self.i += 1;
        }
    }
    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\n') | Some(';')) {
            self.i += 1;
        }
    }
    fn number(&mut self) -> usize {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        self.b[start..self.i]
            .iter()
            .collect::<String>()
            .parse()
            .unwrap_or(0)
    }
    /// Reads `/re/` or `\cREc`, plus the trailing `I`/`M` modifiers.
    fn regex_address(&mut self) -> Result<Addr, Fail> {
        let delim = match self.peek() {
            Some('/') => {
                self.i += 1;
                '/'
            }
            Some('\\') => {
                self.i += 1;
                let d = self
                    .peek()
                    .ok_or_else(|| Fail::usage("sed: `\\` must be followed by a delimiter"))?;
                self.i += 1;
                d
            }
            _ => unreachable!(),
        };
        let mut body = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    return Err(Fail::usage("sed: unterminated address regex"));
                }
                Some('\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(c) if c == delim => body.push(delim),
                        Some('n') if delim != 'n' => body.push('\n'),
                        Some(c) => {
                            body.push('\\');
                            body.push(c);
                        }
                        None => return Err(Fail::usage("sed: unterminated address regex")),
                    }
                    self.i += 1;
                }
                Some(c) if c == delim => {
                    self.i += 1;
                    break;
                }
                Some(c) => {
                    body.push(c);
                    self.i += 1;
                }
            }
        }
        let (mut ignore_case, mut multiline) = (false, false);
        loop {
            match self.peek() {
                Some('I') => ignore_case = true,
                Some('M') => multiline = true,
                _ => break,
            }
            self.i += 1;
        }
        if body.is_empty() && !ignore_case && !multiline {
            return Ok(Addr::Re(None));
        }
        Ok(Addr::Re(Some(build(
            &body,
            self.extended,
            ignore_case,
            multiline,
        )?)))
    }
    fn address(&mut self) -> Result<Option<Addr>, Fail> {
        match self.peek() {
            Some('$') => {
                self.i += 1;
                Ok(Some(Addr::Last))
            }
            Some('/') | Some('\\') => Ok(Some(self.regex_address()?)),
            Some(c) if c.is_ascii_digit() => {
                let n = self.number();
                if self.peek() == Some('~') {
                    self.i += 1;
                    let step = self.number();
                    return Ok(Some(Addr::Step(n, step)));
                }
                Ok(Some(Addr::Line(n)))
            }
            _ => Ok(None),
        }
    }
    /// The text operand of `a`, `i` and `c`: either `a\` + following lines, or GNU's
    /// one-line `a text`.
    fn text_operand(&mut self) -> String {
        self.skip_blank();
        if self.peek() == Some('\\') {
            self.i += 1;
            if self.peek() == Some('\n') {
                self.i += 1;
            }
        }
        let mut out = String::new();
        loop {
            match self.peek() {
                None => break,
                Some('\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some('\n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('n') => out.push('\n'),
                        Some('\\') => out.push('\\'),
                        Some(c) => out.push(c),
                        None => {}
                    }
                    self.i += 1;
                }
                Some('\n') => {
                    self.i += 1;
                    break;
                }
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
        out
    }
    /// A filename operand runs to the end of the line, semicolons included.
    fn file_operand(&mut self) -> Result<String, Fail> {
        self.skip_blank();
        let start = self.i;
        while !matches!(self.peek(), None | Some('\n')) {
            self.i += 1;
        }
        let name: String = self.b[start..self.i].iter().collect();
        if name.is_empty() {
            return Err(Fail::usage("sed: missing filename"));
        }
        Ok(name)
    }
    fn label_operand(&mut self) -> Option<String> {
        self.skip_blank();
        let start = self.i;
        while !matches!(self.peek(), None | Some('\n') | Some(';') | Some('}')) {
            self.i += 1;
        }
        let name: String = self.b[start..self.i].iter().collect();
        let name = name.trim().to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
    fn exit_operand(&mut self) -> Result<i32, Fail> {
        self.skip_blank();
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        if self.i == start {
            return Ok(0);
        }
        self.b[start..self.i]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|_| Fail::usage("sed: `q` expects a numeric exit code"))
    }
    fn compile(&mut self) -> Result<Vec<Inst>, Fail> {
        loop {
            self.skip_separators();
            let Some(c) = self.peek() else { break };
            if c == '#' {
                while !matches!(self.peek(), None | Some('\n')) {
                    self.i += 1;
                }
                continue;
            }
            if c == '}' {
                self.i += 1;
                let Some(open) = self.open.pop() else {
                    return Err(Fail::usage("sed: unexpected `}`"));
                };
                self.out.push(Inst {
                    sel: Sel::All,
                    negate: false,
                    kind: Kind::BlockEnd,
                });
                let end = self.out.len();
                if let Kind::Block(slot) = &mut self.out[open].kind {
                    *slot = end;
                }
                continue;
            }
            let first = self.address()?;
            let sel = match first {
                None => Sel::All,
                Some(a) => {
                    self.skip_blank();
                    if self.peek() == Some(',') {
                        self.i += 1;
                        self.skip_blank();
                        let end = match self.peek() {
                            Some('+') => {
                                self.i += 1;
                                End::Plus(self.number())
                            }
                            Some('~') => {
                                self.i += 1;
                                End::Multiple(self.number())
                            }
                            _ => End::Addr(self.address()?.ok_or_else(|| {
                                Fail::usage("sed: a range needs a second address")
                            })?),
                        };
                        Sel::Range(a, end)
                    } else {
                        Sel::One(a)
                    }
                }
            };
            // Line 0 exists only as the start of a `0,/re/` range: GNU rejects it
            // anywhere else rather than quietly matching nothing.
            let zero_ok = matches!(
                &sel,
                Sel::Range(Addr::Line(0), End::Addr(Addr::Re(_))) | Sel::All
            );
            let mentions_zero = match &sel {
                Sel::One(Addr::Line(0)) => true,
                Sel::Range(a, b) => {
                    matches!(a, Addr::Line(0)) || matches!(b, End::Addr(Addr::Line(0)))
                }
                _ => false,
            };
            if mentions_zero && !zero_ok {
                return Err(Fail::usage(
                    "sed: invalid usage of line address 0; only `0,/regexp/` is allowed",
                ));
            }
            self.skip_blank();
            let mut negate = false;
            while self.peek() == Some('!') {
                negate = !negate;
                self.i += 1;
                self.skip_blank();
            }
            let Some(verb) = self.peek() else {
                return Err(Fail::usage("sed: missing command after address"));
            };
            self.i += 1;
            let kind = match verb {
                '{' => {
                    self.open.push(self.out.len());
                    Kind::Block(0)
                }
                's' => self.substitution()?,
                'y' => self.transliteration()?,
                'p' => Kind::Print,
                'P' => Kind::PrintFirst,
                'd' => Kind::Delete,
                'D' => Kind::DeleteFirst,
                'a' => Kind::Append(self.text_operand()),
                'i' => Kind::Insert(self.text_operand()),
                'c' => Kind::Change(self.text_operand()),
                'r' => Kind::ReadFile(self.file_operand()?),
                'R' => Kind::ReadLine(self.file_operand()?),
                'w' => Kind::WriteFile(self.file_operand()?),
                'W' => Kind::WriteFirst(self.file_operand()?),
                'n' => Kind::Next,
                'N' => Kind::NextAppend,
                'h' => Kind::Hold,
                'H' => Kind::HoldAppend,
                'g' => Kind::Get,
                'G' => Kind::GetAppend,
                'x' => Kind::Exchange,
                'z' => Kind::Zap,
                'F' => Kind::FileName,
                'b' => Kind::Branch(self.label_operand()),
                't' => Kind::BranchIfSub(self.label_operand()),
                'T' => Kind::BranchIfNoSub(self.label_operand()),
                ':' => {
                    let name = self
                        .label_operand()
                        .ok_or_else(|| Fail::usage("sed: `:` needs a label"))?;
                    if !matches!(sel, Sel::All) {
                        return Err(Fail::usage("sed: a label cannot take an address"));
                    }
                    Kind::Label(name)
                }
                'q' => Kind::Quit(self.exit_operand()?),
                'Q' => Kind::QuitSilent(self.exit_operand()?),
                '=' => Kind::LineNumber,
                'l' => {
                    self.skip_blank();
                    if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        Kind::List(Some(self.number()))
                    } else {
                        Kind::List(None)
                    }
                }
                '#' => Kind::Nop,
                other => {
                    return Err(Fail::usage(format!(
                        "sed: unknown command `{other}`\n{SED_COMMANDS}"
                    )))
                }
            };
            self.out.push(Inst { sel, negate, kind });
            self.skip_blank();
            // A block opens straight onto its first command; everything else must be
            // followed by a separator so `sed '1pX'` cannot pass silently.
            if verb == '{' {
                continue;
            }
            match self.peek() {
                None | Some('\n') | Some(';') | Some('}') | Some('#') => {}
                Some(other) => {
                    return Err(Fail::usage(format!(
                        "sed: unexpected `{other}` after command `{verb}`"
                    )))
                }
            }
        }
        if !self.open.is_empty() {
            return Err(Fail::usage("sed: unmatched `{`"));
        }
        Ok(std::mem::take(&mut self.out))
    }
    fn delimited(&mut self, delim: char) -> Result<String, Fail> {
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    return Err(Fail::usage(format!(
                        "sed: unterminated `{delim}`-delimited expression"
                    )))
                }
                Some('\\') => {
                    self.i += 1;
                    match self.peek() {
                        Some(c) if c == delim => out.push(delim),
                        Some(c) => {
                            out.push('\\');
                            out.push(c);
                        }
                        None => return Err(Fail::usage("sed: trailing backslash")),
                    }
                    self.i += 1;
                }
                Some(c) if c == delim => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }
    fn substitution(&mut self) -> Result<Kind, Fail> {
        let delim = self
            .peek()
            .filter(|c| *c != '\n' && *c != '\\')
            .ok_or_else(|| Fail::usage("sed: `s` needs a delimiter"))?;
        self.i += 1;
        let pattern = self.delimited(delim)?;
        let replacement = self.delimited(delim)?;
        let (mut global, mut print, mut ignore_case, mut multiline) = (false, false, false, false);
        let mut nth = 0usize;
        let mut write = None;
        loop {
            match self.peek() {
                Some('g') => global = true,
                Some('p') => print = true,
                Some('i') | Some('I') => ignore_case = true,
                Some('m') | Some('M') => multiline = true,
                Some('e') => {
                    return Err(Fail::usage(
                        "sed: the `e` substitution flag (run the pattern space as a command) is \
                         not modelled; use a shell pipeline instead",
                    ))
                }
                Some('w') => {
                    self.i += 1;
                    write = Some(self.file_operand()?);
                    break;
                }
                Some(c) if c.is_ascii_digit() => {
                    nth = self.number();
                    if nth == 0 {
                        return Err(Fail::usage(
                            "sed: number option to `s` command may not be zero",
                        ));
                    }
                    continue;
                }
                _ => break,
            }
            self.i += 1;
        }
        Ok(Kind::Subst {
            re: build(&pattern, self.extended, ignore_case, multiline)?,
            parts: replacement_parts(&replacement),
            global,
            nth: nth.max(1),
            print,
            write,
        })
    }
    fn transliteration(&mut self) -> Result<Kind, Fail> {
        let delim = self
            .peek()
            .filter(|c| *c != '\n')
            .ok_or_else(|| Fail::usage("sed: `y` needs a delimiter"))?;
        self.i += 1;
        let from = unescape_y(&self.delimited(delim)?);
        let to = unescape_y(&self.delimited(delim)?);
        if from.len() != to.len() || from.is_empty() {
            return Err(Fail::usage(
                "sed: strings for `y` command are of unequal length",
            ));
        }
        Ok(Kind::Transliterate(from, to))
    }
}
const SED_COMMANDS: &str =
    "sed commands: { } s y p P d D a i c r R w W n N h H g G x b t T : q Q = l z F #";
fn unescape_y(s: &str) -> Vec<char> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && i + 1 < b.len() {
            out.push(match b[i + 1] {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                other => other,
            });
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}
fn replacement_parts(s: &str) -> Vec<Part> {
    let b: Vec<char> = s.chars().collect();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    let flush = |literal: &mut String, parts: &mut Vec<Part>| {
        if !literal.is_empty() {
            parts.push(Part::Literal(std::mem::take(literal)));
        }
    };
    while i < b.len() {
        match b[i] {
            '&' => {
                flush(&mut literal, &mut parts);
                parts.push(Part::Whole);
                i += 1;
            }
            '\\' if i + 1 < b.len() => {
                let c = b[i + 1];
                i += 2;
                match c {
                    '0'..='9' => {
                        flush(&mut literal, &mut parts);
                        parts.push(Part::Group(c.to_digit(10).unwrap() as usize));
                    }
                    'n' => literal.push('\n'),
                    't' => literal.push('\t'),
                    'r' => literal.push('\r'),
                    '\n' => literal.push('\n'),
                    'L' | 'U' | 'l' | 'u' | 'E' => {
                        flush(&mut literal, &mut parts);
                        parts.push(Part::Case(c));
                    }
                    other => literal.push(other),
                }
            }
            other => {
                literal.push(other);
                i += 1;
            }
        }
    }
    flush(&mut literal, &mut parts);
    parts
}

// ---------------------------------------------------------------- execution

struct Machine<'a> {
    prog: &'a [Inst],
    labels: BTreeMap<String, usize>,
    /// Per-instruction range state: open, and the line the range must close on.
    open: Vec<bool>,
    close_at: Vec<usize>,
    pattern: String,
    hold: String,
    out: String,
    files: BTreeMap<String, String>,
    last_re: Option<Regex>,
    replaced: bool,
    quit: Option<i32>,
    quit_silently: bool,
    appends: Vec<Append>,
    line: usize,
    filename: String,
    steps: usize,
}
enum Append {
    Text(String),
    File(String),
}
/// Reading a whole stream up front is what a simulated filesystem allows; the cycle
/// below then behaves exactly as sed's does.
struct Stream {
    lines: Vec<String>,
    at: usize,
    final_newline: bool,
}
fn split_lines(text: &str) -> Stream {
    if text.is_empty() {
        return Stream {
            lines: Vec::new(),
            at: 0,
            final_newline: true,
        };
    }
    let final_newline = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    Stream {
        lines: body.split('\n').map(str::to_string).collect(),
        at: 0,
        final_newline,
    }
}

impl Machine<'_> {
    fn hit(&mut self, addr: &Addr, last: bool) -> Result<bool, Fail> {
        Ok(match addr {
            Addr::Line(n) => *n == self.line,
            Addr::Last => last,
            Addr::Step(first, step) => {
                if *step == 0 {
                    self.line == *first
                } else {
                    self.line >= *first && (self.line - *first).is_multiple_of(*step)
                }
            }
            Addr::Re(re) => {
                let re = match re {
                    Some(r) => {
                        self.last_re = Some(r.clone());
                        r.clone()
                    }
                    None => self
                        .last_re
                        .clone()
                        .ok_or_else(|| Fail::usage("sed: no previous regular expression"))?,
                };
                re.0.is_match(&self.pattern)
            }
        })
    }
    fn selected(&mut self, pc: usize, last: bool) -> Result<bool, Fail> {
        let inst = &self.prog[pc];
        let sel = inst.sel.clone();
        let negate = inst.negate;
        let raw = match sel {
            Sel::All => true,
            Sel::One(a) => self.hit(&a, last)?,
            Sel::Range(start, end) => {
                if self.open[pc] {
                    let closes = match &end {
                        End::Addr(Addr::Line(n)) => self.line >= *n,
                        End::Addr(a) => self.hit(a, last)?,
                        End::Plus(_) | End::Multiple(_) => self.line >= self.close_at[pc],
                    };
                    if closes {
                        self.open[pc] = false;
                    }
                    true
                } else {
                    // `0,/re/` lets the end address match the very first line.
                    let zero_start = matches!(start, Addr::Line(0));
                    let starts = if zero_start {
                        self.line >= 1 && !self.open[pc] && self.close_at[pc] == 0
                    } else {
                        self.hit(&start, last)?
                    };
                    if !starts {
                        false
                    } else {
                        match &end {
                            End::Addr(Addr::Line(n)) => self.open[pc] = *n > self.line,
                            End::Addr(a) => {
                                if zero_start {
                                    self.close_at[pc] = 1;
                                    self.open[pc] = !self.hit(a, last)?;
                                } else {
                                    self.open[pc] = true;
                                }
                            }
                            End::Plus(n) => {
                                self.close_at[pc] = self.line + n;
                                self.open[pc] = *n > 0;
                            }
                            End::Multiple(n) => {
                                let next = if *n == 0 {
                                    self.line
                                } else {
                                    self.line.div_ceil(*n) * *n
                                };
                                self.close_at[pc] = next.max(self.line);
                                self.open[pc] = self.close_at[pc] > self.line;
                            }
                        }
                        true
                    }
                }
            }
        };
        Ok(raw != negate)
    }
    fn write_out(&mut self, name: &str, text: &str) {
        if name == "/dev/stdout" {
            self.out.push_str(text);
        } else {
            self.files
                .entry(name.to_string())
                .or_default()
                .push_str(text);
        }
    }
    fn emit_pattern(&mut self, stream: &Stream, forced: bool) {
        // The last line of an input without a trailing newline keeps its shape.
        let last = stream.at >= stream.lines.len();
        let newline = !(last && !stream.final_newline);
        let text = if newline {
            format!("{}\n", self.pattern)
        } else {
            self.pattern.clone()
        };
        let _ = forced;
        self.out.push_str(&text);
    }
    fn flush_appends(&mut self, c: &Computer) -> Result<(), Fail> {
        for a in std::mem::take(&mut self.appends) {
            match a {
                Append::Text(t) => {
                    self.out.push_str(&t);
                    self.out.push('\n');
                }
                Append::File(name) => {
                    if let Ok(bytes) = c.vfs.read_as(&c.resolve(&name), &c.user) {
                        self.out.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
            }
        }
        Ok(())
    }
}
/// `l`: the pattern space in an unambiguous form, wrapped at `width` columns.
fn listing(text: &str, width: usize) -> String {
    let mut body = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => body.push_str("\\\\"),
            '\u{7}' => body.push_str("\\a"),
            '\u{8}' => body.push_str("\\b"),
            '\u{c}' => body.push_str("\\f"),
            '\n' => body.push_str("\\n"),
            '\r' => body.push_str("\\r"),
            '\t' => body.push_str("\\t"),
            '\u{b}' => body.push_str("\\v"),
            c if (c as u32) < 32 || (c as u32) == 127 => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    body.push_str(&format!("\\{b:03o}"));
                }
            }
            c if (c as u32) > 127 => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).as_bytes() {
                    body.push_str(&format!("\\{b:03o}"));
                }
            }
            c => body.push(c),
        }
    }
    if width <= 1 {
        return format!("{body}$\n");
    }
    let mut out = String::new();
    let mut column = 0;
    for ch in body.chars() {
        if column == width - 1 {
            out.push_str("\\\n");
            column = 0;
        }
        out.push(ch);
        column += 1;
    }
    out.push_str("$\n");
    out
}
fn expand(parts: &[Part], caps: &regex::Captures) -> String {
    let mut out = String::new();
    let mut mode: Option<char> = None;
    let mut once: Option<char> = None;
    let push = |out: &mut String, text: &str, mode: &mut Option<char>, once: &mut Option<char>| {
        for ch in text.chars() {
            let c = match once.take() {
                Some('u') => ch.to_uppercase().next().unwrap_or(ch),
                Some('l') => ch.to_lowercase().next().unwrap_or(ch),
                _ => match mode {
                    Some('U') => ch.to_uppercase().next().unwrap_or(ch),
                    Some('L') => ch.to_lowercase().next().unwrap_or(ch),
                    _ => ch,
                },
            };
            out.push(c);
        }
    };
    for part in parts {
        match part {
            Part::Literal(s) => push(&mut out, s, &mut mode, &mut once),
            Part::Whole => push(
                &mut out,
                caps.get(0).map_or("", |m| m.as_str()),
                &mut mode,
                &mut once,
            ),
            Part::Group(n) => push(
                &mut out,
                caps.get(*n).map_or("", |m| m.as_str()),
                &mut mode,
                &mut once,
            ),
            Part::Case('E') => {
                mode = None;
                once = None;
            }
            Part::Case(c @ ('U' | 'L')) => mode = Some(*c),
            Part::Case(c) => once = Some(*c),
        }
    }
    out
}
/// Applies `s` to the pattern space, honouring `N`, `g` and their combination.
fn substitute(
    re: &regex::Regex,
    parts: &[Part],
    subject: &str,
    nth: usize,
    global: bool,
) -> Option<String> {
    let mut out = String::new();
    let mut at = 0;
    let mut seen = 0usize;
    let mut changed = false;
    let mut previous_end: Option<usize> = None;
    while at <= subject.len() {
        let Some(caps) = re.captures_at(subject, at) else {
            break;
        };
        let m = caps.get(0).unwrap();
        // An empty match that touches the end of the previous one is not a match:
        // `echo aaa | sed 's/a*/X/g'` is `X`, not `XX`.
        if m.start() == m.end() && previous_end == Some(m.start()) {
            out.push_str(&subject[at..m.start()]);
            let Some(ch) = subject[m.start()..].chars().next() else {
                at = m.start();
                break;
            };
            out.push(ch);
            at = m.start() + ch.len_utf8();
            previous_end = None;
            continue;
        }
        seen += 1;
        let replace = if global { seen >= nth } else { seen == nth };
        out.push_str(&subject[at..m.start()]);
        if replace {
            out.push_str(&expand(parts, &caps));
            changed = true;
        } else {
            out.push_str(m.as_str());
        }
        previous_end = Some(m.end());
        if m.end() == m.start() {
            match subject[m.end()..].chars().next() {
                Some(ch) => {
                    out.push(ch);
                    at = m.end() + ch.len_utf8();
                }
                None => {
                    at = subject.len() + 1;
                    break;
                }
            }
        } else {
            at = m.end();
        }
        if changed && !global {
            break;
        }
    }
    if !changed {
        return None;
    }
    if at <= subject.len() {
        out.push_str(&subject[at..]);
    }
    Some(out)
}

#[allow(clippy::too_many_lines)]
fn run_stream(m: &mut Machine, stream: &mut Stream, c: &Computer, quiet: bool) -> Result<(), Fail> {
    'cycle: while stream.at < stream.lines.len() {
        m.pattern = stream.lines[stream.at].clone();
        stream.at += 1;
        m.line += 1;
        m.replaced = false;
        let mut pc = 0;
        let mut print = !quiet;
        'restart: loop {
            while pc < m.prog.len() {
                m.steps += 1;
                if m.steps > 5_000_000 {
                    return Err(Fail::usage("sed: script exceeded 5000000 steps"));
                }
                let last = stream.at >= stream.lines.len();
                if matches!(m.prog[pc].kind, Kind::BlockEnd | Kind::Label(_) | Kind::Nop) {
                    pc += 1;
                    continue;
                }
                if !m.selected(pc, last)? {
                    pc = match m.prog[pc].kind {
                        Kind::Block(end) => end,
                        _ => pc + 1,
                    };
                    continue;
                }
                match m.prog[pc].kind.clone() {
                    Kind::Block(_) | Kind::BlockEnd | Kind::Label(_) | Kind::Nop => pc += 1,
                    Kind::Subst {
                        re,
                        parts,
                        global,
                        nth,
                        print: p,
                        write,
                    } => {
                        m.last_re = Some(re.clone());
                        if let Some(text) = substitute(&re.0, &parts, &m.pattern, nth, global) {
                            m.pattern = text;
                            m.replaced = true;
                            if p {
                                let line = format!("{}\n", m.pattern);
                                m.out.push_str(&line);
                            }
                            if let Some(file) = write {
                                let line = format!("{}\n", m.pattern);
                                m.write_out(&file, &line);
                            }
                        }
                        pc += 1;
                    }
                    Kind::Transliterate(from, to) => {
                        m.pattern = m
                            .pattern
                            .chars()
                            .map(|ch| match from.iter().position(|f| *f == ch) {
                                Some(k) => to[k],
                                None => ch,
                            })
                            .collect();
                        pc += 1;
                    }
                    Kind::Print => {
                        let line = format!("{}\n", m.pattern);
                        m.out.push_str(&line);
                        pc += 1;
                    }
                    Kind::PrintFirst => {
                        let head = m.pattern.split('\n').next().unwrap_or("").to_string();
                        m.out.push_str(&format!("{head}\n"));
                        pc += 1;
                    }
                    Kind::Delete => {
                        m.flush_appends(c)?;
                        continue 'cycle;
                    }
                    Kind::DeleteFirst => match m.pattern.split_once('\n') {
                        Some((_, rest)) => {
                            m.pattern = rest.to_string();
                            m.flush_appends(c)?;
                            pc = 0;
                            continue 'restart;
                        }
                        None => {
                            m.flush_appends(c)?;
                            continue 'cycle;
                        }
                    },
                    Kind::Append(text) => {
                        m.appends.push(Append::Text(text));
                        pc += 1;
                    }
                    Kind::Insert(text) => {
                        m.out.push_str(&format!("{text}\n"));
                        pc += 1;
                    }
                    Kind::Change(text) => {
                        // For a range, `c` prints once, at the end of the range.
                        let ending = !matches!(m.prog[pc].sel, Sel::Range(..)) || !m.open[pc];
                        if ending {
                            m.out.push_str(&format!("{text}\n"));
                        }
                        m.flush_appends(c)?;
                        continue 'cycle;
                    }
                    Kind::ReadFile(name) => {
                        m.appends.push(Append::File(name));
                        pc += 1;
                    }
                    Kind::ReadLine(name) => {
                        // `R` reads one line per invocation; a simulated world keeps
                        // the cursor in the file table.
                        let key = format!("\u{0}R{name}");
                        let text = match m.files.get(&key) {
                            Some(t) => t.clone(),
                            None => {
                                let t = c
                                    .vfs
                                    .read_as(&c.resolve(&name), &c.user)
                                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                                    .unwrap_or_default();
                                m.files.insert(key.clone(), t.clone());
                                t
                            }
                        };
                        let mut rest = text;
                        if !rest.is_empty() {
                            let line = match rest.split_once('\n') {
                                Some((head, tail)) => {
                                    let head = head.to_string();
                                    rest = tail.to_string();
                                    head
                                }
                                None => {
                                    let head = rest.clone();
                                    rest = String::new();
                                    head
                                }
                            };
                            m.appends.push(Append::Text(line));
                            m.files.insert(key, rest);
                        }
                        pc += 1;
                    }
                    Kind::WriteFile(name) => {
                        let line = format!("{}\n", m.pattern);
                        m.write_out(&name, &line);
                        pc += 1;
                    }
                    Kind::WriteFirst(name) => {
                        let head = m.pattern.split('\n').next().unwrap_or("").to_string();
                        m.write_out(&name, &format!("{head}\n"));
                        pc += 1;
                    }
                    Kind::Next => {
                        if print {
                            m.emit_pattern(stream, false);
                        }
                        m.flush_appends(c)?;
                        if stream.at >= stream.lines.len() {
                            return Ok(());
                        }
                        m.pattern = stream.lines[stream.at].clone();
                        stream.at += 1;
                        m.line += 1;
                        print = !quiet;
                        pc += 1;
                    }
                    Kind::NextAppend => {
                        m.flush_appends(c)?;
                        if stream.at >= stream.lines.len() {
                            // GNU prints the pattern space and stops; POSIX drops it.
                            if print {
                                m.emit_pattern(stream, false);
                            }
                            return Ok(());
                        }
                        m.pattern.push('\n');
                        m.pattern.push_str(&stream.lines[stream.at]);
                        stream.at += 1;
                        m.line += 1;
                        pc += 1;
                    }
                    Kind::Hold => {
                        m.hold = m.pattern.clone();
                        pc += 1;
                    }
                    Kind::HoldAppend => {
                        m.hold.push('\n');
                        let p = m.pattern.clone();
                        m.hold.push_str(&p);
                        pc += 1;
                    }
                    Kind::Get => {
                        m.pattern = m.hold.clone();
                        pc += 1;
                    }
                    Kind::GetAppend => {
                        m.pattern.push('\n');
                        let h = m.hold.clone();
                        m.pattern.push_str(&h);
                        pc += 1;
                    }
                    Kind::Exchange => {
                        std::mem::swap(&mut m.pattern, &mut m.hold);
                        pc += 1;
                    }
                    Kind::Zap => {
                        m.pattern.clear();
                        pc += 1;
                    }
                    Kind::FileName => {
                        let name = m.filename.clone();
                        m.out.push_str(&format!("{name}\n"));
                        pc += 1;
                    }
                    Kind::Branch(label) => match label {
                        None => break,
                        Some(name) => {
                            pc = *m.labels.get(&name).ok_or_else(|| {
                                Fail::usage(format!("sed: can't find label for jump to `{name}`"))
                            })?;
                        }
                    },
                    Kind::BranchIfSub(label) | Kind::BranchIfNoSub(label) => {
                        let want = matches!(m.prog[pc].kind, Kind::BranchIfSub(_));
                        let take = m.replaced == want;
                        if want {
                            m.replaced = false;
                        }
                        if !take {
                            pc += 1;
                        } else {
                            match label {
                                None => break,
                                Some(name) => {
                                    pc = *m.labels.get(&name).ok_or_else(|| {
                                        Fail::usage(format!(
                                            "sed: can't find label for jump to `{name}`"
                                        ))
                                    })?;
                                }
                            }
                        }
                    }
                    Kind::Quit(code) => {
                        m.quit = Some(code);
                        if print {
                            m.emit_pattern(stream, false);
                        }
                        m.flush_appends(c)?;
                        return Ok(());
                    }
                    Kind::QuitSilent(code) => {
                        m.quit = Some(code);
                        m.quit_silently = true;
                        return Ok(());
                    }
                    Kind::LineNumber => {
                        let n = m.line;
                        m.out.push_str(&format!("{n}\n"));
                        pc += 1;
                    }
                    Kind::List(width) => {
                        let text = listing(&m.pattern, width.unwrap_or(70));
                        m.out.push_str(&text);
                        pc += 1;
                    }
                }
            }
            break;
        }
        if print {
            m.emit_pattern(stream, false);
        }
        m.flush_appends(c)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- command

pub(crate) fn execute(
    c: &mut Computer,
    args: &[String],
    input: &str,
    t: u64,
) -> Result<String, Fail> {
    let mut scripts: Vec<String> = Vec::new();
    let mut have_script = false;
    let mut quiet = false;
    let mut extended = false;
    let mut separate = false;
    let mut in_place: Option<String> = None;
    let mut operands: Vec<String> = Vec::new();
    let mut i = 0;
    let mut end_of_options = false;
    while i < args.len() {
        let a = args[i].clone();
        if end_of_options || a == "-" || !a.starts_with('-') || a.len() == 1 {
            operands.push(a);
            i += 1;
            continue;
        }
        if a == "--" {
            end_of_options = true;
            i += 1;
            continue;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (long, None),
            };
            let mut argument = |name: &str| -> Result<String, Fail> {
                match inline.clone() {
                    Some(v) => Ok(v),
                    None => {
                        i += 1;
                        args.get(i)
                            .cloned()
                            .ok_or_else(|| shell::missing_argument("sed", name))
                    }
                }
            };
            match name {
                "quiet" | "silent" => quiet = true,
                "regexp-extended" => extended = true,
                "separate" => separate = true,
                "expression" => {
                    scripts.push(argument("expression")?);
                    have_script = true;
                }
                "file" => {
                    let path = argument("file")?;
                    scripts.push(shell::read_text(c, "sed", &path, input)?);
                    have_script = true;
                }
                "in-place" => in_place = Some(inline.clone().unwrap_or_default()),
                "version" => return Ok(String::from("sed (computerworld) POSIX profile\n")),
                _ => return Err(shell::unrecognized_option("sed", name)),
            }
            i += 1;
            continue;
        }
        let letters: Vec<char> = a[1..].chars().collect();
        let mut j = 0;
        while j < letters.len() {
            let ch = letters[j];
            match ch {
                'n' => quiet = true,
                'E' | 'r' => extended = true,
                's' => separate = true,
                'z' => {
                    return Err(Fail::usage(
                        "sed: -z (NUL-separated lines) is not modelled by this world",
                    ))
                }
                'i' => {
                    in_place = Some(letters[j + 1..].iter().collect());
                    j = letters.len();
                    continue;
                }
                'e' | 'f' => {
                    let tail: String = letters[j + 1..].iter().collect();
                    let value = if tail.is_empty() {
                        i += 1;
                        args.get(i)
                            .cloned()
                            .ok_or_else(|| shell::missing_argument("sed", &ch.to_string()))?
                    } else {
                        tail
                    };
                    if ch == 'e' {
                        scripts.push(value);
                    } else {
                        scripts.push(shell::read_text(c, "sed", &value, input)?);
                    }
                    have_script = true;
                    j = letters.len();
                    continue;
                }
                other => return Err(shell::invalid_option("sed", other)),
            }
            j += 1;
        }
        i += 1;
    }
    if !have_script {
        if operands.is_empty() {
            return Err(Fail::usage(format!(
                "sed: no script specified\n{}",
                shell::usage_line("sed")
            )));
        }
        scripts.push(operands.remove(0));
    }
    let source = scripts.join("\n");
    let mut compiler = Compiler {
        b: source.chars().collect(),
        i: 0,
        extended,
        out: Vec::new(),
        open: Vec::new(),
    };
    let prog = compiler.compile()?;
    let mut labels = BTreeMap::new();
    for (k, inst) in prog.iter().enumerate() {
        if let Kind::Label(name) = &inst.kind {
            labels.insert(name.clone(), k);
        }
    }
    if in_place.is_some() && operands.is_empty() {
        return Err(Fail::usage("sed: no input files while in place editing"));
    }
    let separate = separate || in_place.is_some();

    let mut machine = Machine {
        prog: &prog,
        labels,
        open: vec![false; prog.len()],
        close_at: vec![0; prog.len()],
        pattern: String::new(),
        hold: String::new(),
        out: String::new(),
        files: BTreeMap::new(),
        last_re: None,
        replaced: false,
        quit: None,
        quit_silently: false,
        appends: Vec::new(),
        line: 0,
        filename: String::from("-"),
        steps: 0,
    };
    let sources: Vec<String> = if operands.is_empty() {
        vec![String::from("-")]
    } else {
        operands.clone()
    };
    let mut written: Vec<(String, String)> = Vec::new();
    if separate {
        for name in &sources {
            let text = shell::read_text(c, "sed", name, input)?;
            let mut stream = split_lines(&text);
            machine.line = 0;
            machine.filename = name.clone();
            machine.open.iter_mut().for_each(|v| *v = false);
            let before = machine.out.len();
            run_stream(&mut machine, &mut stream, c, quiet)?;
            if in_place.is_some() {
                let produced = machine.out.split_off(before);
                written.push((name.clone(), produced));
            }
            if machine.quit.is_some() {
                break;
            }
        }
    } else {
        // One stream: `$` means the last line of the last file, and numbering runs on.
        let mut text = String::new();
        for name in &sources {
            text.push_str(&shell::read_text(c, "sed", name, input)?);
        }
        let mut stream = split_lines(&text);
        machine.filename = sources.first().cloned().unwrap_or_else(|| "-".into());
        run_stream(&mut machine, &mut stream, c, quiet)?;
    }
    for (name, text) in written {
        let suffix = in_place.clone().unwrap_or_default();
        let path = c.resolve(&name);
        if !suffix.is_empty() {
            let backup = if suffix.contains('*') {
                let base = name.rsplit('/').next().unwrap_or(&name);
                let dir = c.resolve(&name);
                let dir = dir
                    .rsplit_once('/')
                    .map_or(String::from("/"), |(d, _)| d.to_string());
                let replaced = suffix.replace('*', base);
                c.resolve(&format!("{dir}/{replaced}"))
            } else {
                format!("{path}{suffix}")
            };
            let original = c
                .vfs
                .read_as(&path, &c.user)
                .map_err(|e| Fail::io("sed", &name, &e))?;
            c.vfs
                .write_as(&backup, &original, &c.user, t)
                .map_err(|e| Fail::io("sed", &backup, &e))?;
        }
        c.vfs
            .write_as(&path, text.as_bytes(), &c.user, t)
            .map_err(|e| Fail::io("sed", &name, &e))?;
    }
    let files: Vec<(String, String)> = machine
        .files
        .iter()
        .filter(|(k, _)| !k.starts_with('\u{0}'))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (name, text) in files {
        if name == "/dev/stderr" {
            continue;
        }
        let path = c.resolve(&name);
        c.vfs
            .write_as(&path, text.as_bytes(), &c.user, t)
            .map_err(|e| Fail::io("sed", &name, &e))?;
    }
    let out = std::mem::take(&mut machine.out);
    match machine.quit {
        Some(code) if code != 0 => Err(Fail::new(String::new(), code).with_output(out)),
        _ => Ok(out),
    }
}
