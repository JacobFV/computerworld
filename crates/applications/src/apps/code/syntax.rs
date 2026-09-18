//! Syntax highlighting: one real tokenizer per language, colours from VS Code's Dark
//! Modern / Light Modern token rules (Dark+ / Light+), and bracket pair colourisation.
//! Everything is a pure function of the text, so a frame is a function of the document.
use cw_scene::Color;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Rust,
    Json,
    Markdown,
    Html,
    Css,
    Shell,
    #[default]
    PlainText,
}
impl Language {
    pub const ALL: [Language; 10] = [
        Language::Css,
        Language::Html,
        Language::JavaScript,
        Language::Json,
        Language::Markdown,
        Language::PlainText,
        Language::Python,
        Language::Rust,
        Language::Shell,
        Language::TypeScript,
    ];
    pub fn from_path(path: &str) -> Self {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let lower = name.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            ".bashrc" | ".profile" | ".zshrc" | ".bash_profile"
        ) {
            return Self::Shell;
        }
        let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext {
            "py" | "pyw" | "pyi" => Self::Python,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "rs" => Self::Rust,
            "json" | "jsonc" | "code-workspace" => Self::Json,
            "md" | "markdown" => Self::Markdown,
            "html" | "htm" | "xhtml" => Self::Html,
            "css" | "scss" | "less" => Self::Css,
            "sh" | "bash" | "zsh" => Self::Shell,
            _ => Self::PlainText,
        }
    }
    /// Name the status bar shows.
    pub fn name(self) -> &'static str {
        match self {
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::Rust => "Rust",
            Self::Json => "JSON",
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Shell => "Shell Script",
            Self::PlainText => "Plain Text",
        }
    }
    /// VS Code's language identifier.
    pub fn id(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
            Self::Json => "json",
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Css => "css",
            Self::Shell => "shellscript",
            Self::PlainText => "plaintext",
        }
    }
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id)
    }
    /// Info strings a Markdown fence may name.
    fn from_fence(info: &str) -> Self {
        match info.trim().to_ascii_lowercase().as_str() {
            "py" | "python" | "python3" => Self::Python,
            "js" | "javascript" | "node" => Self::JavaScript,
            "ts" | "typescript" => Self::TypeScript,
            "rs" | "rust" => Self::Rust,
            "json" | "jsonc" => Self::Json,
            "html" => Self::Html,
            "css" => Self::Css,
            "sh" | "bash" | "shell" | "console" | "zsh" => Self::Shell,
            "md" | "markdown" => Self::Markdown,
            _ => Self::PlainText,
        }
    }
    /// The token `Toggle Line Comment` inserts, or a block pair for languages without one.
    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Self::Python | Self::Shell => Some("#"),
            Self::JavaScript | Self::TypeScript | Self::Rust | Self::Json => Some("//"),
            _ => None,
        }
    }
    pub fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Html | Self::Markdown => Some(("<!--", "-->")),
            Self::Css | Self::JavaScript | Self::TypeScript | Self::Rust => Some(("/*", "*/")),
            _ => None,
        }
    }
    /// Command that runs a file of this language in a terminal, if one does.
    pub fn runner(self, windows: bool) -> Option<&'static str> {
        match self {
            Self::Python if windows => Some("python"),
            Self::Python => Some("python3"),
            Self::JavaScript => Some("node"),
            Self::Shell => Some("bash"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Text,
    Comment,
    /// Storage and language constants: `def`, `const`, `fn`, `True`, `self`.
    Keyword,
    /// Flow control: `if`, `return`, `import`.
    Control,
    Str,
    Escape,
    Number,
    Function,
    Type,
    Variable,
    Constant,
    Regex,
    Tag,
    TagPunct,
    Attribute,
    Heading,
    Bold,
    Code,
    Link,
    Quote,
    ListMarker,
    Selector,
    Property,
    AtRule,
    Operator,
    /// A bracket at nesting level `n % 3`, as bracket pair colourisation paints it.
    Bracket(u8),
    /// A closing bracket with nothing to close.
    BadBracket,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub tok: Tok,
}

/// Token colour under the Dark Modern or Light Modern theme.
pub fn color(tok: Tok, dark: bool) -> Color {
    let hex = |v: u32| Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
    let (d, l) = match tok {
        Tok::Text => (0xCCCCCC, 0x3B3B3B),
        Tok::Comment => (0x6A9955, 0x008000),
        Tok::Keyword => (0x569CD6, 0x0000FF),
        Tok::Control => (0xC586C0, 0xAF00DB),
        Tok::Str => (0xCE9178, 0xA31515),
        Tok::Escape => (0xD7BA7D, 0xEE0000),
        Tok::Number => (0xB5CEA8, 0x098658),
        Tok::Function => (0xDCDCAA, 0x795E26),
        Tok::Type => (0x4EC9B0, 0x267F99),
        Tok::Variable => (0x9CDCFE, 0x001080),
        Tok::Constant => (0x4FC1FF, 0x0070C1),
        Tok::Regex => (0xD16969, 0x811F3F),
        Tok::Tag => (0x569CD6, 0x800000),
        Tok::TagPunct => (0x808080, 0x800000),
        Tok::Attribute => (0x9CDCFE, 0xE50000),
        Tok::Heading => (0x569CD6, 0x800000),
        Tok::Bold => (0x569CD6, 0x000080),
        Tok::Code => (0xCE9178, 0x800000),
        Tok::Link => (0x4DAAFC, 0x005FB8),
        Tok::Quote => (0x6A9955, 0x0451A5),
        Tok::ListMarker => (0x6796E6, 0x0451A5),
        Tok::Selector => (0xD7BA7D, 0x800000),
        Tok::Property => (0x9CDCFE, 0xE50000),
        Tok::AtRule => (0xC586C0, 0xAF00DB),
        Tok::Operator => (0xD4D4D4, 0x000000),
        Tok::Bracket(0) => (0xFFD700, 0x0431FA),
        Tok::Bracket(1) => (0xDA70D6, 0x319331),
        Tok::Bracket(_) => (0x179FFF, 0x7B3814),
        Tok::BadBracket => (0xFF1212, 0xFF1212),
    };
    hex(if dark { d } else { l })
}

/// Every token of `text`, in order and non-overlapping; gaps are plain text.
pub fn spans(lang: Language, text: &str) -> Vec<Span> {
    let mut out = match lang {
        Language::Python => code(text, &PYTHON),
        Language::JavaScript => code(text, &JAVASCRIPT),
        Language::TypeScript => code(text, &TYPESCRIPT),
        Language::Rust => code(text, &RUST),
        Language::Json => json(text),
        Language::Markdown => markdown(text),
        Language::Html => html(text),
        Language::Css => css(text),
        Language::Shell => shell(text),
        Language::PlainText => Vec::new(),
    };
    if lang != Language::Markdown && lang != Language::PlainText {
        colour_brackets(text, &mut out);
    }
    out
}

/// `spans`, cut at line boundaries: one list per line, offsets relative to the line.
pub fn lines(lang: Language, text: &str) -> Vec<Vec<Span>> {
    let mut starts = vec![0];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    let mut out: Vec<Vec<Span>> = vec![Vec::new(); starts.len()];
    let mut line = 0;
    for span in spans(lang, text) {
        let mut start = span.start;
        while start < span.end {
            while line + 1 < starts.len() && starts[line + 1] <= start {
                line += 1;
            }
            let line_end = starts
                .get(line + 1)
                .map_or(text.len(), |next| next - 1)
                .max(starts[line]);
            let end = span.end.min(line_end);
            if end > start {
                out[line].push(Span {
                    start: start - starts[line],
                    end: end - starts[line],
                    tok: span.tok,
                });
            }
            if line + 1 >= starts.len() {
                break;
            }
            start = starts[line + 1].max(end);
        }
    }
    out
}

fn colour_brackets(text: &str, spans: &mut [Span]) {
    let bytes = text.as_bytes();
    let mut stack: Vec<u8> = Vec::new();
    for span in spans.iter_mut() {
        if span.tok != Tok::Bracket(0) {
            continue;
        }
        let b = bytes[span.start];
        match b {
            b'(' | b'[' | b'{' => {
                span.tok = Tok::Bracket((stack.len() % 3) as u8);
                stack.push(b);
            }
            _ => {
                let open = match b {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                if stack.last() == Some(&open) {
                    stack.pop();
                    span.tok = Tok::Bracket((stack.len() % 3) as u8);
                } else {
                    span.tok = Tok::BadBracket;
                }
            }
        }
    }
}

struct Scan<'a> {
    t: &'a str,
    b: &'a [u8],
    i: usize,
    out: Vec<Span>,
}
impl<'a> Scan<'a> {
    fn new(t: &'a str) -> Self {
        Self {
            t,
            b: t.as_bytes(),
            i: 0,
            out: Vec::new(),
        }
    }
    fn push(&mut self, start: usize, end: usize, tok: Tok) {
        if end > start {
            self.out.push(Span { start, end, tok });
        }
    }
    fn peek(&self, k: usize) -> u8 {
        self.b.get(self.i + k).copied().unwrap_or(0)
    }
    fn at(&self, s: &str) -> bool {
        self.t[self.i..].starts_with(s)
    }
    /// Advance one character, respecting UTF-8.
    fn bump(&mut self) {
        let len = self.t[self.i..].chars().next().map_or(1, char::len_utf8);
        self.i += len;
    }
    fn ident_end(&self, from: usize) -> usize {
        let mut j = from;
        for c in self.t[from..].chars() {
            if c == '_' || c.is_alphanumeric() {
                j += c.len_utf8();
            } else {
                break;
            }
        }
        j
    }
    fn to_eol(&self) -> usize {
        self.t[self.i..]
            .find('\n')
            .map_or(self.t.len(), |n| self.i + n)
    }
    fn next_nonspace(&self, from: usize) -> u8 {
        self.b[from.min(self.b.len())..]
            .iter()
            .copied()
            .find(|c| *c != b' ' && *c != b'\t')
            .unwrap_or(0)
    }
}
fn ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

struct Cfg {
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    nested: bool,
    control: &'static [&'static str],
    keyword: &'static [&'static str],
    types: &'static [&'static str],
    /// Words whose next identifier is a function name, and a type name.
    def_words: &'static [&'static str],
    type_words: &'static [&'static str],
    python: bool,
    js: bool,
    rust: bool,
}
const PYTHON: Cfg = Cfg {
    line: &["#"],
    block: None,
    nested: false,
    control: &[
        "if", "elif", "else", "for", "while", "try", "except", "finally", "with", "return",
        "yield", "break", "continue", "pass", "raise", "import", "from", "as", "assert", "await",
        "del", "match", "case",
    ],
    keyword: &[
        "def", "class", "lambda", "global", "nonlocal", "async", "in", "is", "not", "and", "or",
        "True", "False", "None", "self", "cls",
    ],
    types: &[
        "int",
        "str",
        "float",
        "bool",
        "list",
        "dict",
        "set",
        "tuple",
        "bytes",
        "object",
        "Exception",
        "ValueError",
        "TypeError",
        "KeyError",
    ],
    def_words: &["def"],
    type_words: &["class"],
    python: true,
    js: false,
    rust: false,
};
const JS_CONTROL: &[&str] = &[
    "if", "else", "for", "while", "do", "switch", "case", "default", "break", "continue", "return",
    "throw", "try", "catch", "finally", "import", "export", "from", "await", "yield",
];
const JAVASCRIPT: Cfg = Cfg {
    line: &["//"],
    block: Some(("/*", "*/")),
    nested: false,
    control: JS_CONTROL,
    keyword: &[
        "var",
        "let",
        "const",
        "function",
        "class",
        "new",
        "delete",
        "typeof",
        "instanceof",
        "in",
        "of",
        "this",
        "super",
        "true",
        "false",
        "null",
        "undefined",
        "async",
        "static",
        "get",
        "set",
        "extends",
        "void",
    ],
    types: &[
        "Object", "Array", "String", "Number", "Boolean", "Promise", "Map", "Set", "Error", "JSON",
        "Math", "console",
    ],
    def_words: &["function"],
    type_words: &["class", "extends", "new"],
    python: false,
    js: true,
    rust: false,
};
const TYPESCRIPT: Cfg = Cfg {
    keyword: &[
        "var",
        "let",
        "const",
        "function",
        "class",
        "new",
        "delete",
        "typeof",
        "instanceof",
        "in",
        "of",
        "this",
        "super",
        "true",
        "false",
        "null",
        "undefined",
        "async",
        "static",
        "get",
        "set",
        "extends",
        "void",
        "interface",
        "type",
        "enum",
        "implements",
        "public",
        "private",
        "protected",
        "readonly",
        "declare",
        "namespace",
        "abstract",
        "as",
        "keyof",
    ],
    types: &[
        "string", "number", "boolean", "any", "unknown", "never", "object", "Object", "Array",
        "Promise", "Record", "Map", "Set", "Error", "console",
    ],
    type_words: &[
        "class",
        "extends",
        "new",
        "interface",
        "type",
        "enum",
        "implements",
    ],
    ..JAVASCRIPT
};
const RUST: Cfg = Cfg {
    line: &["//"],
    block: Some(("/*", "*/")),
    nested: true,
    control: &[
        "if", "else", "for", "while", "loop", "match", "return", "break", "continue", "use", "mod",
        "await", "in",
    ],
    keyword: &[
        "fn", "let", "mut", "const", "static", "struct", "enum", "trait", "impl", "type", "pub",
        "crate", "super", "self", "Self", "where", "as", "ref", "move", "unsafe", "async", "dyn",
        "extern", "true", "false",
    ],
    types: &[
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64", "bool", "char", "str", "String", "Vec", "Option", "Result", "Box",
    ],
    def_words: &["fn"],
    type_words: &["struct", "enum", "trait", "impl", "type"],
    python: false,
    js: false,
    rust: true,
};

/// Scan a string starting at the quote at `s.i` (after any prefix), emitting escapes.
fn string(s: &mut Scan<'_>, start: usize, raw: bool, python: bool, multiline_quote: bool) {
    let q = s.peek(0);
    let triple = python && s.peek(1) == q && s.peek(2) == q;
    let close_len = if triple { 3 } else { 1 };
    s.i += close_len;
    let mut seg = start;
    loop {
        if s.i >= s.b.len() {
            s.push(seg, s.i, Tok::Str);
            return;
        }
        let c = s.b[s.i];
        if c == b'\\' && !raw {
            s.push(seg, s.i, Tok::Str);
            let esc = s.i;
            s.i += 1;
            if s.i < s.b.len() {
                s.bump();
            }
            s.push(esc, s.i, Tok::Escape);
            seg = s.i;
            continue;
        }
        if c == q && (!triple || (s.peek(1) == q && s.peek(2) == q)) {
            s.i += close_len;
            s.push(seg, s.i, Tok::Str);
            return;
        }
        if c == b'\n' && !triple && !multiline_quote {
            s.push(seg, s.i, Tok::Str);
            return;
        }
        s.bump();
    }
}

fn code(text: &str, cfg: &Cfg) -> Vec<Span> {
    let mut s = Scan::new(text);
    let mut pending: Option<Tok> = None;
    // Whether the last significant token ends a value, so `/` is division, not a regex.
    let mut value = false;
    while s.i < s.b.len() {
        let c = s.b[s.i];
        if c == b' ' || c == b'\t' || c == b'\r' || c == b'\n' {
            s.i += 1;
            continue;
        }
        let start = s.i;
        if cfg.line.iter().any(|l| s.at(l)) {
            let end = s.to_eol();
            s.push(start, end, Tok::Comment);
            s.i = end;
            continue;
        }
        if let Some((open, close)) = cfg.block {
            if s.at(open) {
                let mut depth = 0;
                while s.i < s.b.len() {
                    if s.at(open) {
                        depth += 1;
                        s.i += open.len();
                    } else if s.at(close) {
                        depth -= 1;
                        s.i += close.len();
                        if depth == 0 || !cfg.nested {
                            break;
                        }
                    } else {
                        s.bump();
                    }
                }
                s.push(start, s.i, Tok::Comment);
                continue;
            }
        }
        let ch = s.t[s.i..].chars().next().unwrap_or(' ');
        // String prefixes: Python r/b/f/u, Rust r"…" / r#"…"# / b"…".
        if ident_start(ch) {
            let end = s.ident_end(start);
            let word = &s.t[start..end];
            let next = s.b.get(end).copied().unwrap_or(0);
            let lower = word.to_ascii_lowercase();
            if cfg.python
                && matches!(next, b'"' | b'\'')
                && matches!(
                    lower.as_str(),
                    "r" | "b" | "f" | "u" | "rb" | "br" | "fr" | "rf"
                )
            {
                s.i = end;
                string(&mut s, start, lower.contains('r'), true, false);
                value = true;
                continue;
            }
            if cfg.rust && (word == "r" || word == "br") && (next == b'"' || next == b'#') {
                let mut j = end;
                let mut hashes = 0;
                while s.b.get(j) == Some(&b'#') {
                    hashes += 1;
                    j += 1;
                }
                if s.b.get(j) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let body = j + 1;
                    let stop = s.t[body..]
                        .find(&close)
                        .map_or(s.t.len(), |n| body + n + close.len());
                    s.push(start, stop, Tok::Str);
                    s.i = stop;
                    value = true;
                    continue;
                }
            }
            if cfg.rust && word == "b" && next == b'\'' {
                s.i = end;
                string(&mut s, start, false, false, false);
                value = true;
                continue;
            }
            let after = s.next_nonspace(end);
            let tok = if let Some(tok) = pending.take() {
                tok
            } else if cfg.control.contains(&word) {
                Tok::Control
            } else if cfg.keyword.contains(&word) {
                Tok::Keyword
            } else if cfg.rust && next == b'!' && s.b.get(end + 1) != Some(&b'=') {
                s.push(start, end + 1, Tok::Function);
                s.i = end + 1;
                value = false;
                continue;
            } else if after == b'(' {
                Tok::Function
            } else if cfg.types.contains(&word) {
                Tok::Type
            } else if word.len() > 1
                && word
                    .chars()
                    .all(|c| c.is_uppercase() || c == '_' || c.is_numeric())
                && word.chars().next().is_some_and(char::is_uppercase)
            {
                Tok::Constant
            } else if word.chars().next().is_some_and(char::is_uppercase) && !cfg.python {
                Tok::Type
            } else {
                Tok::Variable
            };
            if cfg.def_words.contains(&word) {
                pending = Some(Tok::Function);
            } else if cfg.type_words.contains(&word) {
                pending = Some(Tok::Type);
            }
            s.push(start, end, tok);
            s.i = end;
            value = !matches!(tok, Tok::Control | Tok::Keyword)
                || matches!(word, "this" | "self" | "true" | "false" | "null" | "None");
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && s.peek(1).is_ascii_digit()) {
            let mut j = s.i + 1;
            while j < s.b.len() {
                let d = s.b[j];
                if d.is_ascii_alphanumeric()
                    || d == b'_'
                    || (d == b'.' && s.b.get(j + 1).is_some_and(u8::is_ascii_digit))
                {
                    j += 1;
                } else {
                    break;
                }
            }
            s.push(start, j, Tok::Number);
            s.i = j;
            value = true;
            continue;
        }
        match c {
            b'"' | b'\'' => {
                if cfg.rust && c == b'\'' {
                    // A lifetime is `'ident` not closed by a quote.
                    let end = s.ident_end(s.i + 1);
                    if end > s.i + 1 && s.b.get(end) != Some(&b'\'') {
                        s.push(start, end, Tok::Keyword);
                        s.i = end;
                        continue;
                    }
                }
                string(&mut s, start, false, cfg.python, cfg.rust && c == b'"');
                value = true;
            }
            b'`' if cfg.js => {
                // Template literal: may span lines; `${…}` stays part of the string.
                s.i += 1;
                let mut seg = start;
                while s.i < s.b.len() && s.b[s.i] != b'`' {
                    if s.b[s.i] == b'\\' {
                        s.push(seg, s.i, Tok::Str);
                        let esc = s.i;
                        s.i += 1;
                        if s.i < s.b.len() {
                            s.bump();
                        }
                        s.push(esc, s.i, Tok::Escape);
                        seg = s.i;
                    } else {
                        s.bump();
                    }
                }
                s.i = (s.i + 1).min(s.b.len());
                s.push(seg, s.i, Tok::Str);
                value = true;
            }
            b'@' if cfg.python || cfg.js => {
                let end = s.ident_end(s.i + 1);
                s.push(start, end.max(s.i + 1), Tok::Function);
                s.i = end.max(s.i + 1);
            }
            b'/' if cfg.js && !value => {
                // Regex literal: to the closing slash on the same line, classes respected.
                let mut j = s.i + 1;
                let mut class = false;
                let mut closed = None;
                while j < s.b.len() && s.b[j] != b'\n' {
                    match s.b[j] {
                        b'\\' => j += 1,
                        b'[' => class = true,
                        b']' => class = false,
                        b'/' if !class => {
                            closed = Some(j);
                            break;
                        }
                        _ => {}
                    }
                    j += 1;
                }
                match closed {
                    Some(end) if end > s.i + 1 => {
                        let mut stop = end + 1;
                        while s.b.get(stop).is_some_and(u8::is_ascii_alphabetic) {
                            stop += 1;
                        }
                        s.push(start, stop, Tok::Regex);
                        s.i = stop;
                        value = true;
                    }
                    _ => {
                        s.push(start, start + 1, Tok::Operator);
                        s.i += 1;
                    }
                }
            }
            b'(' | b'[' | b'{' | b')' | b']' | b'}' => {
                s.push(start, start + 1, Tok::Bracket(0));
                s.i += 1;
                value = matches!(c, b')' | b']' | b'}');
            }
            b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'<' | b'>' | b'!' | b'&' | b'|' | b'^'
            | b'~' | b'?' | b':' => {
                s.push(start, start + 1, Tok::Operator);
                s.i += 1;
                value = false;
            }
            _ => {
                s.bump();
                value = false;
            }
        }
    }
    s.out
}

fn json(text: &str) -> Vec<Span> {
    let mut s = Scan::new(text);
    while s.i < s.b.len() {
        let c = s.b[s.i];
        let start = s.i;
        if s.at("//") {
            let end = s.to_eol();
            s.push(start, end, Tok::Comment);
            s.i = end;
            continue;
        }
        if s.at("/*") {
            let end = s.t[s.i + 2..].find("*/").map_or(s.t.len(), |n| s.i + n + 4);
            s.push(start, end, Tok::Comment);
            s.i = end;
            continue;
        }
        match c {
            b'"' => {
                string(&mut s, start, false, false, false);
                if s.next_nonspace(s.i) == b':' {
                    // A key: the whole run, escapes included, is a property name.
                    s.out.retain(|sp| sp.start < start);
                    s.push(start, s.i, Tok::Property);
                }
            }
            b'-' | b'0'..=b'9' => {
                let mut j = s.i + 1;
                while j < s.b.len()
                    && matches!(s.b[j], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    j += 1;
                }
                s.push(start, j, Tok::Number);
                s.i = j;
            }
            b'{' | b'}' | b'[' | b']' => {
                s.push(start, start + 1, Tok::Bracket(0));
                s.i += 1;
            }
            _ if c.is_ascii_alphabetic() => {
                let end = s.ident_end(start);
                if matches!(&s.t[start..end], "true" | "false" | "null") {
                    s.push(start, end, Tok::Keyword);
                }
                s.i = end;
            }
            _ => s.bump(),
        }
    }
    s.out
}

fn markdown(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut offset = 0;
    let mut fence: Option<(String, Language, usize)> = None;
    for line in text.split('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if let Some((marker, lang, body)) = &fence {
            if trimmed.starts_with(marker.as_str()) {
                // The fenced block is highlighted as the language its info string names.
                let block = &text[*body..offset.max(*body)];
                for span in spans(*lang, block) {
                    out.push(Span {
                        start: span.start + body,
                        end: span.end + body,
                        tok: span.tok,
                    });
                }
                out.push(Span {
                    start: offset + indent,
                    end: offset + line.len(),
                    tok: Tok::TagPunct,
                });
                fence = None;
            }
            offset += line.len() + 1;
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = trimmed[..3].to_owned();
            let info = trimmed[3..].to_owned();
            out.push(Span {
                start: offset + indent,
                end: offset + line.len(),
                tok: Tok::TagPunct,
            });
            fence = Some((marker, Language::from_fence(&info), offset + line.len() + 1));
            offset += line.len() + 1;
            continue;
        }
        let hashes = trimmed.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with([' ', '\t']) || trimmed == "#"
        {
            out.push(Span {
                start: offset + indent,
                end: offset + line.len(),
                tok: Tok::Heading,
            });
        } else if trimmed.starts_with('>') {
            out.push(Span {
                start: offset + indent,
                end: offset + line.len(),
                tok: Tok::Quote,
            });
        } else {
            let mut body = indent;
            let marker = if trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("+ ")
            {
                1
            } else {
                let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 && trimmed[digits..].starts_with(". ") {
                    digits + 1
                } else {
                    0
                }
            };
            if marker > 0 {
                out.push(Span {
                    start: offset + indent,
                    end: offset + indent + marker,
                    tok: Tok::ListMarker,
                });
                body += marker;
            }
            inline_markdown(line, offset, body, &mut out);
        }
        offset += line.len() + 1;
    }
    out
}
fn inline_markdown(line: &str, offset: usize, from: usize, out: &mut Vec<Span>) {
    let b = line.as_bytes();
    let mut i = from;
    while i < b.len() {
        let rest = &line[i..];
        let close = |open: &str, close: &str| -> Option<usize> {
            rest.strip_prefix(open)?
                .find(close)
                .filter(|n| *n > 0)
                .map(|n| open.len() + n + close.len())
        };
        let (len, tok) = if b[i] == b'`' {
            (close("`", "`"), Tok::Code)
        } else if rest.starts_with("**") || rest.starts_with("__") {
            (close(&rest[..2], &rest[..2]), Tok::Bold)
        } else if b[i] == b'[' {
            // [text](url): the text as a link title, the target as a link.
            match rest.find("](") {
                Some(mid) if !rest[1..mid].contains(']') => match rest[mid..].find(')') {
                    Some(end) => {
                        out.push(Span {
                            start: offset + i,
                            end: offset + i + mid + 1,
                            tok: Tok::Str,
                        });
                        out.push(Span {
                            start: offset + i + mid + 2,
                            end: offset + i + mid + end,
                            tok: Tok::Link,
                        });
                        i += mid + end + 1;
                        continue;
                    }
                    None => (None, Tok::Text),
                },
                _ => (None, Tok::Text),
            }
        } else {
            (None, Tok::Text)
        };
        match len {
            Some(len) => {
                out.push(Span {
                    start: offset + i,
                    end: offset + i + len,
                    tok,
                });
                i += len;
            }
            None => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
}

fn html(text: &str) -> Vec<Span> {
    let mut s = Scan::new(text);
    let lower = text.to_ascii_lowercase();
    while s.i < s.b.len() {
        let start = s.i;
        if s.at("<!--") {
            let end = s.t[s.i..].find("-->").map_or(s.t.len(), |n| s.i + n + 3);
            s.push(start, end, Tok::Comment);
            s.i = end;
            continue;
        }
        if s.b[s.i] == b'&' {
            let end = s.t[s.i..]
                .find(';')
                .filter(|n| {
                    *n < 12
                        && s.t[s.i + 1..s.i + n]
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'#')
                })
                .map(|n| s.i + n + 1);
            if let Some(end) = end {
                s.push(start, end, Tok::Keyword);
                s.i = end;
                continue;
            }
        }
        if s.b[s.i] == b'<'
            && (s.peek(1).is_ascii_alphabetic() || s.peek(1) == b'/' || s.peek(1) == b'!')
        {
            let closing = s.peek(1) == b'/';
            let punct = if closing || s.peek(1) == b'!' { 2 } else { 1 };
            s.push(start, start + punct, Tok::TagPunct);
            s.i += punct;
            let name_start = s.i;
            while s.i < s.b.len() && (s.b[s.i].is_ascii_alphanumeric() || s.b[s.i] == b'-') {
                s.i += 1;
            }
            let name = lower[name_start..s.i].to_owned();
            s.push(name_start, s.i, Tok::Tag);
            // Attributes until the tag closes.
            while s.i < s.b.len() && s.b[s.i] != b'>' {
                let c = s.b[s.i];
                if c == b'"' || c == b'\'' {
                    let q = c;
                    let a = s.i;
                    s.i += 1;
                    while s.i < s.b.len() && s.b[s.i] != q {
                        s.i += 1;
                    }
                    s.i = (s.i + 1).min(s.b.len());
                    s.push(a, s.i, Tok::Str);
                } else if c.is_ascii_alphabetic() || c == b'-' || c == b':' || c == b'@' {
                    let a = s.i;
                    while s.i < s.b.len()
                        && (s.b[s.i].is_ascii_alphanumeric()
                            || matches!(s.b[s.i], b'-' | b':' | b'@' | b'_'))
                    {
                        s.i += 1;
                    }
                    s.push(a, s.i, Tok::Attribute);
                } else if c == b'/' {
                    s.push(s.i, s.i + 1, Tok::TagPunct);
                    s.i += 1;
                } else {
                    s.bump();
                }
            }
            if s.i < s.b.len() {
                s.push(s.i, s.i + 1, Tok::TagPunct);
                s.i += 1;
            }
            // Embedded script and style are highlighted as JavaScript and CSS.
            if !closing && (name == "script" || name == "style") {
                let close = format!("</{name}");
                let body = s.i;
                let end = lower[body..].find(&close).map_or(text.len(), |n| body + n);
                let lang = if name == "script" {
                    Language::JavaScript
                } else {
                    Language::Css
                };
                let mut inner = spans(lang, &text[body..end]);
                for span in &mut inner {
                    span.start += body;
                    span.end += body;
                    if let Tok::Bracket(_) = span.tok {
                        span.tok = Tok::Text;
                    }
                }
                s.out.extend(inner);
                s.i = end;
            }
            continue;
        }
        s.bump();
    }
    s.out
}

fn css(text: &str) -> Vec<Span> {
    let mut s = Scan::new(text);
    let mut depth = 0usize;
    // Inside a declaration's value, after its colon.
    let mut in_value = false;
    while s.i < s.b.len() {
        let c = s.b[s.i];
        let start = s.i;
        if s.at("/*") {
            let end = s.t[s.i + 2..].find("*/").map_or(s.t.len(), |n| s.i + n + 4);
            s.push(start, end, Tok::Comment);
            s.i = end;
            continue;
        }
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => s.i += 1,
            b'{' => {
                s.push(start, start + 1, Tok::Bracket(0));
                s.i += 1;
                depth += 1;
                in_value = false;
            }
            b'}' => {
                s.push(start, start + 1, Tok::Bracket(0));
                s.i += 1;
                depth = depth.saturating_sub(1);
                in_value = false;
            }
            b';' => {
                s.i += 1;
                in_value = false;
            }
            b'"' | b'\'' => string(&mut s, start, false, false, false),
            b'@' => {
                let end = s.ident_end(s.i + 1);
                let mut end = end;
                while s.b.get(end) == Some(&b'-') {
                    end = s.ident_end(end + 1);
                }
                s.push(start, end, Tok::AtRule);
                s.i = end;
            }
            b':' if in_value || depth > 0 && !selector_ahead(&s) => {
                s.i += 1;
                in_value = true;
            }
            _ if depth > 0 && !in_value && !selector_ahead(&s) => {
                // A property name.
                let mut j = s.i;
                while j < s.b.len()
                    && (s.b[j].is_ascii_alphanumeric() || s.b[j] == b'-' || s.b[j] == b'_')
                {
                    j += 1;
                }
                if j == s.i {
                    s.bump();
                } else {
                    s.push(start, j, Tok::Property);
                    s.i = j;
                }
            }
            _ if in_value => {
                if c.is_ascii_digit() || (c == b'.' || c == b'-') && s.peek(1).is_ascii_digit() {
                    let mut j = s.i + 1;
                    while j < s.b.len()
                        && (s.b[j].is_ascii_alphanumeric() || s.b[j] == b'.' || s.b[j] == b'%')
                    {
                        j += 1;
                    }
                    s.push(start, j, Tok::Number);
                    s.i = j;
                } else if c == b'#' {
                    let mut j = s.i + 1;
                    while j < s.b.len() && s.b[j].is_ascii_hexdigit() {
                        j += 1;
                    }
                    s.push(start, j, Tok::Str);
                    s.i = j;
                } else if c.is_ascii_alphabetic() || c == b'-' {
                    let mut j = s.i;
                    while j < s.b.len() && (s.b[j].is_ascii_alphanumeric() || s.b[j] == b'-') {
                        j += 1;
                    }
                    let tok = if s.b.get(j) == Some(&b'(') {
                        Tok::Function
                    } else if &s.t[start..j] == "important" {
                        Tok::Keyword
                    } else {
                        Tok::Str
                    };
                    s.push(start, j, tok);
                    s.i = j;
                } else if c == b'(' || c == b')' {
                    s.push(start, start + 1, Tok::Bracket(0));
                    s.i += 1;
                } else {
                    s.bump();
                }
            }
            _ => {
                // Selector text, up to the brace or the separating comma.
                let mut j = s.i;
                while j < s.b.len() && !matches!(s.b[j], b'{' | b',' | b'\n' | b'}' | b'/') {
                    j += 1;
                }
                let trimmed = s.t[start..j].trim_end().len();
                s.push(start, start + trimmed, Tok::Selector);
                s.i = j.max(s.i + 1);
                if j < s.b.len() && s.b[j] == b',' {
                    s.i = j + 1;
                }
            }
        }
    }
    s.out
}
/// Inside a block, whether what follows is a nested rule's selector rather than a
/// declaration: a `{` arrives before any `;` or `}`.
fn selector_ahead(s: &Scan<'_>) -> bool {
    for &c in &s.b[s.i..] {
        match c {
            b'{' => return true,
            b';' | b'}' => return false,
            _ => {}
        }
    }
    false
}

fn shell(text: &str) -> Vec<Span> {
    const CONTROL: &[&str] = &[
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case",
        "esac", "function", "select", "return", "exit", "break", "continue", "local", "export",
        "source",
    ];
    let mut s = Scan::new(text);
    let mut command = true;
    while s.i < s.b.len() {
        let c = s.b[s.i];
        let start = s.i;
        match c {
            b'\n' | b';' | b'|' | b'&' | b'(' | b'{' | b'`' => {
                if matches!(c, b'|' | b'&' | b';') {
                    s.push(start, start + 1, Tok::Operator);
                }
                if matches!(c, b'(' | b'{') {
                    s.push(start, start + 1, Tok::Bracket(0));
                }
                s.i += 1;
                command = true;
            }
            b')' | b'}' => {
                s.push(start, start + 1, Tok::Bracket(0));
                s.i += 1;
            }
            b' ' | b'\t' | b'\r' => s.i += 1,
            b'#' => {
                let end = s.to_eol();
                s.push(start, end, Tok::Comment);
                s.i = end;
            }
            b'\'' => {
                let end = s.t[s.i + 1..].find('\'').map_or(s.t.len(), |n| s.i + n + 2);
                s.push(start, end, Tok::Str);
                s.i = end;
                command = false;
            }
            b'"' => {
                s.i += 1;
                let mut seg = start;
                while s.i < s.b.len() && s.b[s.i] != b'"' {
                    if s.b[s.i] == b'\\' {
                        s.i += 2;
                        continue;
                    }
                    if s.b[s.i] == b'$' {
                        s.push(seg, s.i, Tok::Str);
                        let v = s.i;
                        s.i = variable_end(&s, s.i);
                        s.push(v, s.i, Tok::Variable);
                        seg = s.i;
                        continue;
                    }
                    s.bump();
                }
                s.i = (s.i + 1).min(s.b.len());
                s.push(seg, s.i, Tok::Str);
                command = false;
            }
            b'$' => {
                s.i = variable_end(&s, s.i);
                s.push(start, s.i, Tok::Variable);
                command = false;
            }
            b'>' | b'<' | b'=' | b'!' => {
                s.push(start, start + 1, Tok::Operator);
                s.i += 1;
            }
            _ => {
                let mut j = s.i;
                while j < s.b.len()
                    && !matches!(
                        s.b[j],
                        b' ' | b'\t'
                            | b'\n'
                            | b';'
                            | b'|'
                            | b'&'
                            | b'('
                            | b')'
                            | b'"'
                            | b'\''
                            | b'$'
                            | b'>'
                            | b'<'
                            | b'{'
                            | b'}'
                            | b'`'
                    )
                {
                    j += 1;
                }
                if j == s.i {
                    s.bump();
                    continue;
                }
                let word = &s.t[start..j];
                if let Some(eq) = word.find('=').filter(|_| command) {
                    // NAME=value
                    s.push(start, start + eq, Tok::Variable);
                    s.push(start + eq, start + eq + 1, Tok::Operator);
                    s.i = start + eq + 1;
                    continue;
                }
                let tok = if CONTROL.contains(&word) {
                    Tok::Control
                } else if word.bytes().all(|b| b.is_ascii_digit()) {
                    Tok::Number
                } else if command {
                    Tok::Function
                } else {
                    Tok::Text
                };
                if tok != Tok::Text {
                    s.push(start, j, tok);
                }
                s.i = j;
                // After a keyword such as `then` or `do`, a command follows.
                command = tok == Tok::Control
                    && !matches!(
                        word,
                        "in" | "for" | "case" | "function" | "local" | "export"
                    );
            }
        }
    }
    s.out
}
fn variable_end(s: &Scan<'_>, from: usize) -> usize {
    let next = s.b.get(from + 1).copied().unwrap_or(0);
    if next == b'{' {
        return s.t[from..].find('}').map_or(s.t.len(), |n| from + n + 1);
    }
    if next == b'(' {
        return from + 1;
    }
    if matches!(next, b'?' | b'@' | b'#' | b'*' | b'$' | b'!' | b'0'..=b'9') {
        return from + 2;
    }
    s.ident_end(from + 1).max(from + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn toks(lang: Language, text: &str) -> Vec<(String, Tok)> {
        spans(lang, text)
            .into_iter()
            .map(|s| (text[s.start..s.end].to_owned(), s.tok))
            .collect()
    }
    fn tok_of(lang: Language, text: &str, word: &str) -> Tok {
        toks(lang, text)
            .into_iter()
            .find(|(t, _)| t == word)
            .unwrap_or_else(|| panic!("{word} not tokenized in {text:?}"))
            .1
    }
    #[test]
    fn python_keywords_strings_comments_and_definitions() {
        let src = "def greet(name):\n    \"\"\"Say hi.\n    twice\"\"\"\n    if name is None: return 0x1F  # done\n    print(f\"hi {name}\\n\")\nclass Box: pass\n";
        assert_eq!(tok_of(Language::Python, src, "def"), Tok::Keyword);
        assert_eq!(tok_of(Language::Python, src, "greet"), Tok::Function);
        assert_eq!(tok_of(Language::Python, src, "if"), Tok::Control);
        assert_eq!(tok_of(Language::Python, src, "None"), Tok::Keyword);
        assert_eq!(tok_of(Language::Python, src, "0x1F"), Tok::Number);
        assert_eq!(tok_of(Language::Python, src, "# done"), Tok::Comment);
        assert_eq!(tok_of(Language::Python, src, "print"), Tok::Function);
        assert_eq!(tok_of(Language::Python, src, "\\n"), Tok::Escape);
        assert_eq!(tok_of(Language::Python, src, "Box"), Tok::Type);
        // A triple-quoted string spans lines, and the line splitter keeps both halves.
        let lines = lines(Language::Python, src);
        assert!(lines[1].iter().any(|s| s.tok == Tok::Str));
        assert!(lines[2].iter().any(|s| s.tok == Tok::Str));
    }
    #[test]
    fn javascript_regex_templates_and_division() {
        let src = "const re = /a+b/g; let x = a / b;\nconsole.log(`v ${x}`) // end";
        assert_eq!(tok_of(Language::JavaScript, src, "const"), Tok::Keyword);
        assert_eq!(tok_of(Language::JavaScript, src, "/a+b/g"), Tok::Regex);
        assert!(toks(Language::JavaScript, src)
            .iter()
            .any(|(t, k)| t == "/" && *k == Tok::Operator));
        assert_eq!(tok_of(Language::JavaScript, src, "`v ${x}`"), Tok::Str);
        assert_eq!(tok_of(Language::JavaScript, src, "log"), Tok::Function);
        assert_eq!(tok_of(Language::JavaScript, src, "// end"), Tok::Comment);
        let ts = "interface User { id: number }";
        assert_eq!(tok_of(Language::TypeScript, ts, "interface"), Tok::Keyword);
        assert_eq!(tok_of(Language::TypeScript, ts, "User"), Tok::Type);
        assert_eq!(tok_of(Language::TypeScript, ts, "number"), Tok::Type);
    }
    #[test]
    fn rust_macros_lifetimes_raw_strings_and_nested_comments() {
        let src = "fn main<'a>() { println!(\"x\"); let r = r#\"raw \"q\"\"#; /* a /* b */ c */ }";
        assert_eq!(tok_of(Language::Rust, src, "main"), Tok::Function);
        assert_eq!(tok_of(Language::Rust, src, "'a"), Tok::Keyword);
        assert_eq!(tok_of(Language::Rust, src, "println!"), Tok::Function);
        assert_eq!(tok_of(Language::Rust, src, "r#\"raw \"q\"\"#"), Tok::Str);
        assert_eq!(
            tok_of(Language::Rust, src, "/* a /* b */ c */"),
            Tok::Comment
        );
    }
    #[test]
    fn json_keys_values_and_bracket_levels() {
        let src = "{\"a\": [1, true, \"s\"], \"b\": {\"c\": null}}";
        assert_eq!(tok_of(Language::Json, src, "\"a\""), Tok::Property);
        assert_eq!(tok_of(Language::Json, src, "\"s\""), Tok::Str);
        assert_eq!(tok_of(Language::Json, src, "true"), Tok::Keyword);
        assert_eq!(tok_of(Language::Json, src, "1"), Tok::Number);
        let brackets: Vec<Tok> = spans(Language::Json, src)
            .into_iter()
            .filter(|s| matches!(s.tok, Tok::Bracket(_)))
            .map(|s| s.tok)
            .collect();
        assert_eq!(brackets[0], Tok::Bracket(0));
        assert_eq!(brackets[1], Tok::Bracket(1));
        assert_eq!(*brackets.last().unwrap(), Tok::Bracket(0));
        assert!(spans(Language::Json, "]")
            .iter()
            .any(|s| s.tok == Tok::BadBracket));
    }
    #[test]
    fn markdown_headings_code_links_and_fenced_languages() {
        let src = "# Title\nSome `code` and **bold** [site](http://x.y)\n- item\n```python\ndef f(): pass\n```\n> quote";
        assert_eq!(tok_of(Language::Markdown, src, "# Title"), Tok::Heading);
        assert_eq!(tok_of(Language::Markdown, src, "`code`"), Tok::Code);
        assert_eq!(tok_of(Language::Markdown, src, "**bold**"), Tok::Bold);
        assert_eq!(tok_of(Language::Markdown, src, "http://x.y"), Tok::Link);
        assert_eq!(tok_of(Language::Markdown, src, "-"), Tok::ListMarker);
        assert_eq!(tok_of(Language::Markdown, src, "def"), Tok::Keyword);
        assert_eq!(tok_of(Language::Markdown, src, "> quote"), Tok::Quote);
    }
    #[test]
    fn html_tags_attributes_and_embedded_css_and_js() {
        let src = "<!-- c --><div class=\"x\">&amp;</div><style>p { color: red; }</style><script>let a = 1;</script>";
        assert_eq!(tok_of(Language::Html, src, "<!-- c -->"), Tok::Comment);
        assert_eq!(tok_of(Language::Html, src, "div"), Tok::Tag);
        assert_eq!(tok_of(Language::Html, src, "class"), Tok::Attribute);
        assert_eq!(tok_of(Language::Html, src, "\"x\""), Tok::Str);
        assert_eq!(tok_of(Language::Html, src, "&amp;"), Tok::Keyword);
        assert_eq!(tok_of(Language::Html, src, "color"), Tok::Property);
        assert_eq!(tok_of(Language::Html, src, "let"), Tok::Keyword);
    }
    #[test]
    fn css_selectors_properties_values_and_at_rules() {
        let src = "@media screen { .card:hover, #id > p { margin: 0 4px; color: #fff; background: rgb(1,2,3) } }";
        assert_eq!(tok_of(Language::Css, src, "@media"), Tok::AtRule);
        assert_eq!(tok_of(Language::Css, src, ".card:hover"), Tok::Selector);
        assert_eq!(tok_of(Language::Css, src, "margin"), Tok::Property);
        assert_eq!(tok_of(Language::Css, src, "4px"), Tok::Number);
        assert_eq!(tok_of(Language::Css, src, "#fff"), Tok::Str);
        assert_eq!(tok_of(Language::Css, src, "rgb"), Tok::Function);
    }
    #[test]
    fn shell_commands_variables_and_keywords() {
        let src =
            "#!/bin/bash\nNAME=world\nif [ -n \"$NAME\" ]; then echo \"hi $NAME\" | tr a b; fi\n";
        assert_eq!(tok_of(Language::Shell, src, "#!/bin/bash"), Tok::Comment);
        assert_eq!(tok_of(Language::Shell, src, "NAME"), Tok::Variable);
        assert_eq!(tok_of(Language::Shell, src, "if"), Tok::Control);
        assert_eq!(tok_of(Language::Shell, src, "echo"), Tok::Function);
        assert_eq!(tok_of(Language::Shell, src, "tr"), Tok::Function);
        assert_eq!(tok_of(Language::Shell, src, "$NAME"), Tok::Variable);
    }
    #[test]
    fn languages_come_from_extensions_and_colours_follow_the_theme() {
        assert_eq!(Language::from_path("/a/b/main.py"), Language::Python);
        assert_eq!(Language::from_path("x.tsx"), Language::TypeScript);
        assert_eq!(Language::from_path("run.sh"), Language::Shell);
        assert_eq!(Language::from_path("README"), Language::PlainText);
        assert_eq!(color(Tok::Comment, true), Color::rgb(0x6A, 0x99, 0x55));
        assert_eq!(color(Tok::Keyword, false), Color::rgb(0, 0, 0xFF));
        // Multi-byte text never splits a character.
        let src = "s = \"héllo – ünïcode\" # ñ\n";
        for line in lines(Language::Python, src) {
            for span in line {
                assert!(src.is_char_boundary(span.start) && src.is_char_boundary(span.end));
            }
        }
    }
}
