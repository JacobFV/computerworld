//! A bounded synthetic shell. Lexical quoting is resolved before operators; all
//! commands act only on the computer substrate and explicit network adapter.
use crate::{CommandResult, Computer, ShellHost};
use std::collections::BTreeMap;
/// A failure carries its status so consumers classify by exit code, never by matching
/// error text. 127 no such command, 126 found but not executable, 2 the flag or syntax
/// is outside the simulated surface, 1 a modelled negative or operational error.
#[derive(Debug, Clone)]
pub(crate) struct Fail {
    text: String,
    code: i32,
    /// Output produced before failing: `grep -c` prints 0 and still exits 1.
    out: String,
    /// The text is already a complete message: do not prefix it with the command
    /// name. A nested shell's diagnostics are its own, and are carried through even
    /// when it exits 0, so a script never loses what it printed to stderr.
    raw: bool,
}
impl Fail {
    pub(crate) fn new(text: impl Into<String>, code: i32) -> Self {
        Self {
            text: text.into(),
            code,
            out: String::new(),
            raw: false,
        }
    }
    /// Carries a nested run's two streams and status through unchanged.
    fn nested(r: CommandResult) -> Self {
        Self {
            text: r.stderr,
            code: r.exit_code,
            out: r.stdout,
            raw: true,
        }
    }
    pub(crate) fn with_output(mut self, out: String) -> Self {
        self.out = out;
        self
    }
    /// The text is already a complete diagnostic; do not prefix the command name.
    pub(crate) fn raw(mut self) -> Self {
        self.raw = true;
        self
    }
    pub(crate) fn with_code(mut self, code: i32) -> Self {
        self.code = code;
        self
    }
    /// Unsupported flag or malformed invocation: refused loudly, never ignored.
    pub(crate) fn usage(text: impl Into<String>) -> Self {
        Self::new(text, 2)
    }
    /// The error contract's canonical shape: `cmd: subject: reason`. Every command
    /// that names an operand in a diagnostic builds it here, so the wording is one
    /// decision rather than a hundred.
    pub(crate) fn op(cmd: &str, subject: impl std::fmt::Display, reason: &str) -> Self {
        Self::new(format!("{cmd}: {subject}: {reason}"), 1)
    }
    /// A VFS error reported against the operand the caller actually typed, in GNU
    /// coreutils' wording (`cat: nope: No such file or directory`). The VFS spells its
    /// own paths canonically, which is rarely what the user wrote.
    pub(crate) fn io(cmd: &str, subject: impl std::fmt::Display, e: &crate::VfsError) -> Self {
        Self::op(cmd, subject, vfs_reason(e))
    }
}
/// GNU's `strerror` wording for the errors this VFS can raise. Consumers port scripts
/// by matching these strings, so they are the coreutils spellings, not the VFS's.
pub(crate) fn vfs_reason(e: &crate::VfsError) -> &'static str {
    use crate::VfsError as V;
    match e {
        V::NotFound(_) => "No such file or directory",
        V::Exists(_) => "File exists",
        V::NotDirectory(_) => "Not a directory",
        V::IsDirectory(_) => "Is a directory",
        V::NotEmpty(_) => "Directory not empty",
        V::LinkLoop => "Too many levels of symbolic links",
        V::Permission(_) => "Permission denied",
        V::Invalid(_) => "Invalid argument",
    }
}
/// Reads a file for a text utility, reporting failure as `cmd: operand: reason`.
/// `-` means standard input, as every GNU text tool accepts.
pub(crate) fn read_text(
    c: &Computer,
    cmd: &str,
    operand: &str,
    stdin: &str,
) -> Result<String, Fail> {
    Ok(String::from_utf8_lossy(&read_bytes(c, cmd, operand, stdin)?).into_owned())
}
pub(crate) fn read_bytes(
    c: &Computer,
    cmd: &str,
    operand: &str,
    stdin: &str,
) -> Result<Vec<u8>, Fail> {
    if operand == "-" {
        return Ok(stdin.as_bytes().to_vec());
    }
    let path = c.resolve(operand);
    if c.vfs.stat(&path).is_ok_and(|m| m.is_dir) {
        return Err(Fail::op(cmd, operand, "Is a directory"));
    }
    c.vfs
        .read_as(&path, &c.user)
        .map_err(|e| Fail::io(cmd, operand, &e))
}
impl From<String> for Fail {
    fn from(text: String) -> Self {
        Self::new(text, 1)
    }
}
impl From<&str> for Fail {
    fn from(text: &str) -> Self {
        Self::new(text, 1)
    }
}
impl From<crate::VfsError> for Fail {
    fn from(e: crate::VfsError) -> Self {
        Self::new(e.to_string(), 1)
    }
}
/// Commands implemented in-process. `which` reports a nominal path for these because
/// the VFS holds no binaries; the roster is the honest answer to "is this available?".
pub(crate) const BUILTINS: &[&str] = &[
    ":",
    "[",
    "[[",
    "apt",
    "apt-get",
    "awk",
    "base64",
    "basename",
    "bash",
    "break",
    "brew",
    "cat",
    "cd",
    "chmod",
    "clear",
    "cmp",
    "comm",
    "continue",
    "cp",
    "curl",
    "cut",
    "date",
    "df",
    "diff",
    "dirname",
    "du",
    "echo",
    "env",
    "exit",
    "expand",
    "export",
    "false",
    "file",
    "find",
    "fold",
    "getopts",
    "git",
    "grep",
    "head",
    "hexdump",
    "hostname",
    "ip",
    "join",
    "kill",
    "ln",
    "local",
    "ls",
    "md5sum",
    "mkdir",
    "mv",
    "nl",
    "node",
    "npm",
    "nproc",
    "od",
    "paste",
    "pip",
    "printenv",
    "printf",
    "ps",
    "pwd",
    "python",
    "python3",
    "read",
    "readlink",
    "realpath",
    "return",
    "rev",
    "rm",
    "rmdir",
    "sed",
    "seq",
    "service",
    "sh",
    "sha1sum",
    "sha256sum",
    "shift",
    "shuf",
    "sleep",
    "sort",
    "source",
    "split",
    "sqlite3",
    "stat",
    "strings",
    "sudo",
    "systemctl",
    "tail",
    "tee",
    "test",
    "touch",
    "tr",
    "true",
    "uname",
    "unexpand",
    "uniq",
    "unset",
    "uptime",
    "wc",
    "wget",
    "which",
    "whoami",
    "xargs",
    "xxd",
    "yes",
];
#[derive(Clone, Debug)]
enum Token {
    Word(Vec<(String, u8)>),
    Op(String),
}
fn lex(s: &str, ps: bool) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    let mut parts = vec![];
    let mut word = false;
    // Index at which the previous word ended, so `2>` is distinguished from `2 >`.
    let mut word_end = usize::MAX;
    let mut heredocs: Vec<(usize, String, u8, bool)> = Vec::new();
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\'' || ch == '"' {
            word = true;
            let q = ch;
            i += 1;
            let mut text = String::new();
            while i < chars.len() && chars[i] != q {
                if q == '"' && chars[i] == '$' && chars.get(i + 1) == Some(&'(') {
                    let end = balanced_end(&chars, i + 1)?;
                    text.extend(chars[i..end].iter());
                    i = end;
                    continue;
                }
                if chars[i] == if ps { '`' } else { '\\' } && q == '"' && i + 1 < chars.len() {
                    text.push(chars[i]);
                    i += 1;
                }
                text.push(chars[i]);
                i += 1;
            }
            if i == chars.len() {
                return Err("unterminated quote".into());
            }
            parts.push((text, u8::from(q == '"')));
            i += 1;
            continue;
        }
        if ch == '`' && !ps {
            word = true;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '`' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                i += 1;
            }
            if i == chars.len() {
                return Err("unterminated backquote".into());
            }
            let body: String = chars[start..i].iter().collect();
            parts.push((format!("$({body})"), 3));
            i += 1;
            continue;
        }
        if ch == if ps { '`' } else { '\\' } {
            word = true;
            i += 1;
            if i < chars.len() {
                parts.push((chars[i].to_string(), 0));
                i += 1;
            }
            continue;
        }
        if ch == '#' && !word {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == ' ' || ch == '\t' || ";\n|&<>()".contains(ch) {
            if word {
                out.push(Token::Word(std::mem::take(&mut parts)));
                word = false;
                word_end = i;
            }
            if ch == ' ' || ch == '\t' {
                i += 1;
                continue;
            }
            if ch == '<' && chars.get(i + 1) == Some(&'<') {
                i += 2;
                let strip = chars.get(i) == Some(&'-');
                if strip {
                    i += 1;
                }
                while chars.get(i).is_some_and(|c| *c == ' ' || *c == '\t') {
                    i += 1;
                }
                let quote = chars.get(i).copied().filter(|c| *c == '\'' || *c == '"');
                if quote.is_some() {
                    i += 1;
                }
                let start = i;
                while i < chars.len()
                    && if let Some(q) = quote {
                        chars[i] != q
                    } else {
                        !chars[i].is_whitespace() && !";&|<>".contains(chars[i])
                    }
                {
                    i += 1;
                }
                let delimiter: String = chars[start..i].iter().collect();
                if delimiter.is_empty() {
                    return Err("missing heredoc delimiter".into());
                }
                if quote.is_some() {
                    if i == chars.len() {
                        return Err("unterminated heredoc delimiter".into());
                    }
                    i += 1;
                }
                out.push(Token::Op("<<".into()));
                let index = out.len();
                out.push(Token::Word(vec![(String::new(), 0)]));
                heredocs.push((index, delimiter, u8::from(quote.is_none()), strip));
                continue;
            }
            if ch == '&' && chars.get(i + 1) == Some(&'>') {
                let mut op = "&>".to_string();
                i += 2;
                if chars.get(i) == Some(&'>') {
                    op.push('>');
                    i += 1;
                }
                out.push(Token::Op(op));
                continue;
            }
            let start = i;
            let mut op = ch.to_string();
            if i + 1 < chars.len() && ((ch == '|' || ch == '&' || ch == '>') && chars[i + 1] == ch)
            {
                op.push(ch);
                i += 1;
            }
            if ch == '>' {
                // A descriptor prefix only counts when glued on: `echo 2 > f` redirects stdout.
                if let Some(Token::Word(v)) = out.last() {
                    if word_end == start && v.len() == 1 && (v[0].0 == "1" || v[0].0 == "2") {
                        let fd = v[0].0.clone();
                        out.pop();
                        op = format!("{fd}{op}");
                    }
                }
                // `2>&1` duplicates a descriptor and takes no path operand.
                if chars.get(i + 1) == Some(&'&')
                    && chars.get(i + 2).is_some_and(char::is_ascii_digit)
                {
                    op.push('&');
                    op.push(chars[i + 2]);
                    i += 2;
                }
            }
            out.push(Token::Op(op));
            i += 1;
            if ch == '\n' {
                for (index, delimiter, flags, strip) in heredocs.drain(..) {
                    let mut body = String::new();
                    let mut found = false;
                    while i < chars.len() {
                        let start = i;
                        while i < chars.len() && chars[i] != '\n' {
                            i += 1;
                        }
                        let raw: String = chars[start..i].iter().collect();
                        if i < chars.len() {
                            i += 1;
                        }
                        let line = if strip {
                            raw.trim_start_matches('\t')
                        } else {
                            &raw
                        };
                        if line == delimiter {
                            found = true;
                            break;
                        }
                        body.push_str(line);
                        body.push('\n');
                    }
                    if !found {
                        return Err("unterminated heredoc".into());
                    }
                    out[index] = Token::Word(vec![(body, flags)]);
                }
            }
            continue;
        }
        word = true;
        let mut text = String::new();
        while i < chars.len()
            && !" \t\n;|&<>()\"'".contains(chars[i])
            && chars[i] != if ps { '`' } else { '\\' }
            && !(chars[i] == '`' && !ps)
        {
            if chars[i] == '$' && chars.get(i + 1) == Some(&'(') {
                let end = balanced_end(&chars, i + 1)?;
                text.extend(chars[i..end].iter());
                i = end;
            } else {
                text.push(chars[i]);
                i += 1;
            }
        }
        parts.push((text, 3));
    }
    if word {
        out.push(Token::Word(parts));
    }
    if !heredocs.is_empty() {
        return Err("heredoc requires newline and body".into());
    }
    Ok(out)
}
fn balanced_end(chars: &[char], start: usize) -> Result<usize, String> {
    let mut depth = 1;
    let mut i = start + 1;
    let mut quote = None;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            i += 2;
            continue;
        }
        if let Some(q) = quote {
            if q == ch {
                quote = None
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch)
        } else if ch == '(' {
            depth += 1;
            if depth > 32 {
                return Err("substitution nesting exceeds 32".into());
            }
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return Ok(i + 1);
            }
        }
        i += 1;
    }
    Err("unterminated substitution".into())
}
fn expand_commands(
    text: &str,
    c: &mut Computer,
    status: i32,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, String> {
    let chars: Vec<_> = text.chars().collect();
    let mut i = 0;
    let mut out = String::new();
    let mut plain = String::new();
    while i < chars.len() {
        if chars[i] == '$' && chars.get(i + 1) == Some(&'(') {
            out.push_str(&expand(&plain, c, status));
            plain.clear();
            let end = balanced_end(&chars, i + 1)?;
            let body: String = chars[i + 2..end - 1].iter().collect();
            if body.starts_with('(') && body.ends_with(')') {
                out.push_str(&arithmetic(&body[1..body.len() - 1], c)?.to_string());
            } else {
                let saved_env = c.env.clone();
                let saved_cwd = c.cwd.clone();
                let r = execute_inner(c, &body, tick, host, depth + 1);
                c.env = saved_env;
                c.cwd = saved_cwd;
                out.push_str(r.stdout.trim_end_matches('\n'));
            }
            i = end;
        } else if chars[i] == '\\'
            && i + 1 < chars.len()
            && matches!(chars[i + 1], '$' | '"' | '\\')
        {
            out.push_str(&expand(&plain, c, status));
            plain.clear();
            out.push(chars[i + 1]);
            i += 2;
        } else {
            plain.push(chars[i]);
            i += 1;
        }
    }
    out.push_str(&expand(&plain, c, status));
    Ok(out)
}
fn arithmetic(text: &str, c: &Computer) -> Result<i64, String> {
    struct Parser<'a> {
        v: Vec<char>,
        i: usize,
        c: &'a Computer,
    }
    impl Parser<'_> {
        fn space(&mut self) {
            while self.v.get(self.i).is_some_and(|c| c.is_whitespace()) {
                self.i += 1;
            }
        }
        fn expr(&mut self, min: u8) -> Result<i64, String> {
            self.space();
            let mut left = match self.v.get(self.i) {
                Some('(') => {
                    self.i += 1;
                    let n = self.expr(0)?;
                    self.space();
                    if self.v.get(self.i) != Some(&')') {
                        return Err("unclosed arithmetic group".into());
                    }
                    self.i += 1;
                    n
                }
                Some('-') => {
                    self.i += 1;
                    self.expr(3)?.checked_neg().ok_or("arithmetic overflow")?
                }
                Some('+') => {
                    self.i += 1;
                    self.expr(3)?
                }
                _ => {
                    let start = self.i;
                    while self
                        .v
                        .get(self.i)
                        .is_some_and(|c| c.is_alphanumeric() || *c == '_')
                    {
                        self.i += 1;
                    }
                    if start == self.i {
                        return Err("expected arithmetic operand".into());
                    }
                    let word: String = self.v[start..self.i].iter().collect();
                    word.parse()
                        .or_else(|_| {
                            self.c
                                .env
                                .get(&word)
                                .map(String::as_str)
                                .unwrap_or("0")
                                .parse()
                        })
                        .map_err(|_| "invalid arithmetic integer")?
                }
            };
            loop {
                self.space();
                let op = self.v.get(self.i).copied().unwrap_or(' ');
                let prec = match op {
                    '+' | '-' => 1,
                    '*' | '/' | '%' => 2,
                    _ => break,
                };
                if prec < min {
                    break;
                }
                self.i += 1;
                let r = self.expr(prec + 1)?;
                left = match op {
                    '+' => left.checked_add(r),
                    '-' => left.checked_sub(r),
                    '*' => left.checked_mul(r),
                    '/' => left.checked_div(r),
                    '%' => left.checked_rem(r),
                    _ => None,
                }
                .ok_or("arithmetic overflow or division by zero")?;
            }
            Ok(left)
        }
    }
    let mut parser = Parser {
        v: text.chars().collect(),
        i: 0,
        c,
    };
    let result = parser.expr(0)?;
    parser.space();
    if parser.i != parser.v.len() {
        return Err("unexpected arithmetic token".into());
    }
    Ok(result)
}
/// Wildcards match one path component. No host directories are consulted.
pub(crate) fn wildcard(pattern: &str, value: &str) -> bool {
    let p: Vec<_> = pattern.chars().collect();
    let v: Vec<_> = value.chars().collect();
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < v.len() {
        if i < p.len() && (p[i] == '?' || p[i] == v[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            mark = j;
        } else if let Some(s) = star {
            i = s + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}
fn glob_paths(c: &Computer, pattern: &str) -> Vec<String> {
    let abs = c.resolve(pattern);
    let components: Vec<_> = abs.split('/').filter(|s| !s.is_empty()).collect();
    let mut paths = vec![String::new()];
    for component in components {
        let mut next = vec![];
        for parent in paths {
            if component.contains('*') || component.contains('?') {
                if let Ok(entries) = c
                    .vfs
                    .list_as(if parent.is_empty() { "/" } else { &parent }, &c.user)
                {
                    for name in entries {
                        if (!name.starts_with('.') || component.starts_with('.'))
                            && wildcard(component, &name)
                        {
                            next.push(format!("{parent}/{name}"));
                        }
                    }
                }
            } else {
                next.push(format!("{parent}/{component}"));
            }
        }
        paths = next;
    }
    paths
        .into_iter()
        .filter(|p| c.vfs.stat(p).is_ok())
        .map(|p| {
            if pattern.starts_with('/') {
                p
            } else {
                p.strip_prefix(&format!("{}/", c.cwd.trim_end_matches('/')))
                    .unwrap_or(&p)
                    .to_string()
            }
        })
        .collect()
}
/// `$@`/`$*` joined by a space. Both spellings produce one string here; unquoted it is
/// then field-split like any other expansion, so `cmd $@` forwards the parameters,
/// while `"$@"` stays one word rather than one word per parameter.
fn positional(c: &Computer) -> (usize, String) {
    let n: usize = c.env.get("#").and_then(|v| v.parse().ok()).unwrap_or(0);
    let joined = (1..=n)
        .filter_map(|i| c.env.get(&i.to_string()))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (n, joined)
}
fn expand(text: &str, c: &Computer, status: i32) -> String {
    let mut out = String::new();
    let v: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < v.len() {
        if v[i] != '$' {
            out.push(v[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i < v.len() && v[i] == '?' {
            out.push_str(&status.to_string());
            i += 1;
            continue;
        }
        let brace = i < v.len() && v[i] == '{';
        if brace {
            i += 1;
        }
        // `#` and `@`/`*` are not identifier characters, so they are read here.
        if matches!(v.get(i), Some('@' | '*' | '#' | '?')) {
            let special = v[i];
            i += 1;
            let (count, joined) = positional(c);
            if special == '#' && brace && v.get(i) != Some(&'}') {
                // `${#NAME}` is the length of the value, not the parameter count.
                let start = i;
                while i < v.len() && (v[i].is_alphanumeric() || v[i] == '_') {
                    i += 1;
                }
                let name: String = v[start..i].iter().collect();
                let length = c.env.get(&name).map_or(0, |value| value.chars().count());
                out.push_str(&length.to_string());
            } else {
                out.push_str(&match special {
                    '#' => count.to_string(),
                    '?' => status.to_string(),
                    _ => joined,
                });
            }
            if brace && v.get(i) == Some(&'}') {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < v.len() && (v[i].is_alphanumeric() || v[i] == '_' || v[i] == ':') {
            if v[i] == ':' && v.get(i + 1).is_some_and(|c| "-+=?".contains(*c)) {
                break;
            }
            i += 1;
        }
        let key: String = v[start..i].iter().collect();
        let key = key.strip_prefix("env:").unwrap_or(&key);
        let mut value = c.env.get(key).cloned().unwrap_or_default();
        if brace {
            if i + 1 < v.len() && v[i] == ':' && v[i + 1] == '-' {
                i += 2;
                let start = i;
                while i < v.len() && v[i] != '}' {
                    i += 1;
                }
                if value.is_empty() {
                    value = v[start..i].iter().collect();
                }
            }
            if i < v.len() && v[i] == '}' {
                i += 1;
            }
        }
        if key.is_empty() {
            out.push('$')
        } else {
            out.push_str(&value)
        }
    }
    out
}
fn word(
    t: &Token,
    c: &mut Computer,
    status: i32,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, String> {
    match t {
        Token::Word(parts) => {
            let mut s = String::new();
            for (part, flags) in parts {
                s.push_str(&if flags & 1 != 0 {
                    expand_commands(part, c, status, tick, host, depth)?
                } else {
                    part.clone()
                });
            }
            Ok(if s == "~" || s.starts_with("~/") {
                format!(
                    "{}{}",
                    c.env.get("HOME").cloned().unwrap_or_default(),
                    &s[1..]
                )
            } else {
                s
            })
        }
        _ => Err("expected word".into()),
    }
}
/// Expands a word into the fields it becomes. An unquoted expansion is split on
/// whitespace, a quoted one never is, and an unquoted expansion that comes out empty
/// contributes no field at all — the difference between `cmd $EMPTY` and `cmd "$EMPTY"`.
/// Literal text cannot contain whitespace (the lexer would have ended the word), so
/// splitting a whole unquoted part is the same as splitting only what it expanded to.
fn word_fields(
    t: &Token,
    c: &mut Computer,
    status: i32,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<Vec<String>, String> {
    let Token::Word(parts) = t else {
        return Err("expected word".into());
    };
    let mut fields: Vec<String> = Vec::new();
    let mut pending = String::new();
    let mut started = false;
    for (part, flags) in parts {
        let text = if flags & 1 != 0 {
            expand_commands(part, c, status, tick, host, depth)?
        } else {
            part.clone()
        };
        if flags & 2 == 0 {
            // Quoted: the field exists even when the expansion is empty.
            pending.push_str(&text);
            started = true;
            continue;
        }
        if started && text.starts_with(char::is_whitespace) {
            fields.push(std::mem::take(&mut pending));
            started = false;
        }
        for (k, piece) in text.split_whitespace().enumerate() {
            if k > 0 {
                fields.push(std::mem::take(&mut pending));
            }
            pending.push_str(piece);
            started = true;
        }
        if started && text.ends_with(char::is_whitespace) {
            fields.push(std::mem::take(&mut pending));
            started = false;
        }
    }
    if started {
        fields.push(pending);
    }
    // `~` names the home directory at the head of a word only.
    if let Some(first) = fields.first_mut() {
        if first == "~" || first.starts_with("~/") {
            *first = format!(
                "{}{}",
                c.env.get("HOME").cloned().unwrap_or_default(),
                &first[1..]
            );
        }
    }
    Ok(fields)
}
pub fn execute(
    c: &mut Computer,
    source: &str,
    tick: u64,
    host: &mut dyn ShellHost,
) -> CommandResult {
    execute_inner(c, source, tick, host, 0)
}
/// Runs a command line with `stdin` already filled, and returns its three results
/// without touching the caller's. `awk`'s `| "cmd"`, `"cmd" | getline` and `system()`,
/// and `xargs`, all reach the rest of the shell through this one door, so a command
/// invoked from inside a utility behaves exactly as it does when typed.
pub(crate) fn run_piped(
    c: &mut Computer,
    source: &str,
    stdin: &str,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> CommandResult {
    if depth >= 32 {
        return CommandResult::new("shell: execution nesting exceeds 32\n", 2);
    }
    let tokens = match lex(source, c.dialect == "powershell") {
        Ok(v) => v,
        Err(e) => return CommandResult::new(format!("shell: {e}\n"), 2),
    };
    if tokens.is_empty() {
        return CommandResult::default();
    }
    let mut total = CommandResult::default();
    let mut ctx = Ctx::default();
    let code = run_list(
        c,
        &tokens,
        &mut ctx,
        tick,
        host,
        depth + 1,
        &mut total,
        0,
        stdin,
    );
    total.exit_code = code;
    total
}
fn execute_inner(
    c: &mut Computer,
    source: &str,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> CommandResult {
    if depth > 32 {
        return CommandResult::new("shell: execution nesting exceeds 32\n", 2);
    }
    if source.len() > 65536 {
        return CommandResult::new("shell: command exceeds 64 KiB limit\n", 2);
    }
    let tokens = match lex(source, c.dialect == "powershell") {
        Ok(v) => v,
        Err(e) => return CommandResult::new(format!("shell: {e}\n"), 2),
    };
    if matches!(tokens.last(),Some(Token::Op(op)) if op=="&&"||op=="||"||op=="|") {
        return CommandResult::new("shell: missing command after operator\n", 2);
    }
    if matches!(tokens.first(),Some(Token::Op(op)) if op=="&&"||op=="||"||op=="|") {
        return CommandResult::new("shell: unexpected operator\n", 2);
    }
    // Control flow is parsed into a tree; a plain list becomes a row of Simple nodes
    // that the same flat evaluator as before runs, so its semantics cannot drift.
    let nodes = match Grammar::program(&tokens) {
        Ok(v) => v,
        Err(e) => return CommandResult::new(format!("shell: {e}\n"), 2),
    };
    let pid = c.processes.spawn(1, &c.user, source, tick);
    if tokens
        .first()
        .is_some_and(|v| matches!(v,Token::Word(parts) if parts.len()==1 && parts[0].0=="sleep"))
        && (tokens.len() == 2 || tokens.len() == 3 && matches!(&tokens[2],Token::Op(op) if op=="&"))
    {
        let result = word(&tokens[1], c, 0, tick, host, depth)
            .and_then(|s| parse_duration(&s))
            .and_then(|duration| {
                tick.checked_add(duration)
                    .ok_or("sleep deadline overflow".into())
            })
            .and_then(|until| {
                c.processes.schedule_exit(pid, until, 0)?;
                if tokens.len() == 2 {
                    let now = host.advance(until - tick)?;
                    c.processes.advance(now);
                    host.cleanup_process(pid);
                }
                Ok(())
            });
        return match result {
            Ok(()) => CommandResult {
                pid,
                ..Default::default()
            },
            Err(e) => {
                let _ = c.processes.exit(pid, 1, tick);
                host.cleanup_process(pid);
                CommandResult {
                    pid,
                    ..CommandResult::error(e)
                }
            }
        };
    }
    let mut total = CommandResult::default();
    let mut ctx = Ctx::default();
    let mut previous = run_nodes(c, &nodes, &mut ctx, tick, host, depth, &mut total, 0);
    // A budget is the only signal that survives every frame; it becomes the status.
    if let Flow::Budget(message) = &ctx.flow {
        total.stderr.push_str(&format!("shell: {message}\n"));
        previous = 2;
    }
    if let Flow::Exit(code) = ctx.flow {
        previous = code;
    }
    total.exit_code = previous;
    total.pid = pid;
    let _ = c.processes.exit(pid, previous, tick);
    host.cleanup_process(pid);
    total
}
/// An and-or list: pipelines joined by `&&`/`||`, separators included. This is the
/// evaluator the shell has always had; control flow calls into it for each leaf.
#[allow(clippy::too_many_arguments)]
fn run_list(
    c: &mut Computer,
    tokens: &[Token],
    ctx: &mut Ctx,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    total: &mut CommandResult,
    mut previous: i32,
    initial: &str,
) -> i32 {
    let mut pos = 0;
    let mut gate = String::new();
    // Only the first command of the list inherits a piped-in or redirected stream.
    let mut initial = initial.to_string();
    while pos < tokens.len() {
        let allowed = match gate.as_str() {
            "&&" => previous == 0,
            "||" => previous != 0,
            _ => true,
        };
        let mut input = std::mem::take(&mut initial);
        loop {
            let start = pos;
            while pos < tokens.len()
                && !matches!(&tokens[pos],Token::Op(s) if matches!(s.as_str(),"|"|"||"|"&&"|";"|"\n"|"&"))
            {
                pos += 1;
            }
            if pos == start {
                if pos < tokens.len() {
                    pos += 1;
                    break;
                } else {
                    break;
                }
            }
            if allowed {
                let r = run_tokens(
                    c,
                    &tokens[start..pos],
                    &input,
                    tick,
                    host,
                    depth,
                    previous,
                    ctx,
                );
                previous = r.exit_code;
                total.clear |= r.clear;
                total.stderr.push_str(&r.stderr);
                input = r.stdout;
                // `break`, `continue`, `return` and an exhausted budget stop here.
                if ctx.flow != Flow::Normal {
                    total.stdout.push_str(&input);
                    return previous;
                }
            }
            let op = if let Some(Token::Op(s)) = tokens.get(pos) {
                s.clone()
            } else {
                String::new()
            };
            pos += usize::from(pos < tokens.len());
            if op == "|" {
                if pos == tokens.len() {
                    previous = 2;
                    total.stderr.push_str("shell: missing pipeline command\n");
                    break;
                }
                continue;
            }
            if allowed {
                total.stdout.push_str(&input);
            }
            gate = op;
            break;
        }
    }
    previous
}
#[allow(clippy::too_many_arguments)]
fn run_tokens(
    c: &mut Computer,
    tokens: &[Token],
    stdin: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    status: i32,
    ctx: &mut Ctx,
) -> CommandResult {
    if !ctx.charge(Budget::Step) {
        return CommandResult::new(String::new(), 2);
    }
    let mut args = vec![];
    // Descriptor table resolved at parse time so `>f 2>&1` and `2>&1 >f` differ as in bash.
    let mut fds: [Sink; 2] = [Sink::Fd(0), Sink::Fd(1)];
    let mut input = stdin.to_string();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Op(op) if op == "(" || op == ")" => {
                return CommandResult::new(format!("shell: unexpected `{op}`\n"), 2)
            }
            Token::Op(op) => {
                if let Some(source) = op.strip_suffix("&1").or_else(|| op.strip_suffix("&2")) {
                    let to = usize::from(op.ends_with('2'));
                    let from = usize::from(source.starts_with('2'));
                    fds[from] = fds[to].clone();
                    i += 1;
                    continue;
                }
                i += 1;
                let path = match tokens
                    .get(i)
                    .ok_or("missing redirection target")
                    .and_then(|v| word(v, c, status, t, host, depth).map_err(|_| "invalid target"))
                {
                    Ok(p) => {
                        if op == "<<" {
                            p
                        } else {
                            c.resolve(&p)
                        }
                    }
                    Err(e) => return CommandResult::new(format!("shell: {e}\n"), 2),
                };
                if op == "<<" {
                    input = path;
                } else if op == "<" {
                    match c.vfs.read_as(&path, &c.user) {
                        Ok(v) => input = String::from_utf8_lossy(&v).into_owned(),
                        Err(e) => return CommandResult::error(e.to_string()),
                    }
                } else {
                    let sink = Sink::File(path, op.ends_with(">>"));
                    if op.starts_with('&') {
                        fds = [sink.clone(), sink];
                    } else if op.starts_with('2') {
                        fds[1] = sink;
                    } else {
                        fds[0] = sink;
                    }
                }
            }
            v => match word_fields(v, c, status, t, host, depth) {
                Ok(fields) => {
                    // Only a `*` written in the source globs; one that arrives from a
                    // variable stays literal, so data never turns into a pattern.
                    let glob = matches!(v,Token::Word(parts) if parts.iter().any(|(p,f)| f&2!=0 && (p.contains('*')||p.contains('?'))));
                    for field in fields {
                        let paths = if glob && (field.contains('*') || field.contains('?')) {
                            glob_paths(c, &field)
                        } else {
                            vec![]
                        };
                        if paths.is_empty() {
                            args.push(field)
                        } else {
                            args.extend(paths)
                        }
                    }
                }
                Err(e) => return CommandResult::error(e),
            },
        }
        i += 1;
    }
    // A compound's shared stream reaches the command that would actually read it, and
    // that command consumes it. `read` is handled by its own builtin, which takes one
    // line and leaves the rest — that is what lets a `while read` loop advance.
    if input.is_empty()
        && ctx.stdin.is_some()
        && args.first().is_some_and(|a| a != "read")
        && reads_stdin(c, &args)
    {
        input = ctx.stdin.take().unwrap_or_default();
    }
    let mut assignments = vec![];
    while args.first().is_some_and(|s| {
        s.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
        })
    }) {
        let a = args.remove(0);
        let (k, v) = a.split_once('=').unwrap();
        assignments.push((k.to_string(), c.env.insert(k.into(), v.into())));
    }
    let persist = args.is_empty();
    let mut result = if persist {
        CommandResult::default()
    } else if let Some(r) = control_word(&args, ctx, status) {
        r
    } else if let Some(r) = shell_builtin(c, &args, &input, ctx, t, host, depth) {
        r
    } else if let Some(body) = ctx.functions.get(&args[0]).cloned() {
        call_function(c, &body, &args, ctx, t, host, depth)
    } else {
        match run(c, &args, &input, t, host, depth) {
            Ok(s) => CommandResult {
                stdout: s,
                // `clear` emits no bytes; the terminal reads this flag and erases itself.
                clear: args[0].eq_ignore_ascii_case("clear") || args[0].eq_ignore_ascii_case("cls"),
                ..CommandResult::default()
            },
            Err(f) => CommandResult {
                stdout: f.out.clone(),
                stderr: if f.text.is_empty() {
                    String::new()
                } else if f.raw || f.text.starts_with(&format!("{}: ", args[0])) {
                    format!("{}\n", f.text.trim_end_matches('\n'))
                } else {
                    format!("{}: {}\n", args[0], f.text.trim_end_matches('\n'))
                },
                exit_code: f.code,
                ..CommandResult::default()
            },
        }
    };
    if !persist {
        for (k, v) in assignments {
            if let Some(v) = v {
                c.env.insert(k, v);
            } else {
                c.env.remove(&k);
            }
        }
    }
    dispatch(c, &mut result, &fds, t);
    result
}
/// Commands whose implementation falls back to the input stream when they are not
/// given a file. Only these are handed a compound's shared stream, so a loop body that
/// ignores stdin cannot swallow the lines the loop is reading.
fn reads_stdin(c: &Computer, args: &[String]) -> bool {
    let named_file = args[1..]
        .iter()
        .any(|a| !a.starts_with('-') && c.vfs.exists(&c.resolve(a)));
    match args[0].to_ascii_lowercase().as_str() {
        "tr" | "tee" | "xargs" => true,
        "cat" | "type" | "get-content" => args.len() == 1,
        "grep" | "select-string" | "sed" | "awk" | "gawk" | "mawk" | "nawk" | "cut" | "head"
        | "tail" | "wc" | "sort" | "uniq" | "nl" | "rev" | "fold" | "expand" | "unexpand"
        | "paste" | "shuf" | "split" | "strings" | "base64" | "md5sum" | "sha1sum"
        | "sha256sum" | "xxd" | "od" | "hexdump" => !named_file,
        "sqlite3" => crate::sqlite::reads_stdin(args),
        _ => false,
    }
}
/// Sends a result's two streams where its descriptor table points. Two descriptors
/// aimed at one file must produce one write, stdout first.
fn dispatch(c: &mut Computer, result: &mut CommandResult, fds: &[Sink; 2], t: u64) {
    let mut files: Vec<(String, bool, String)> = Vec::new();
    let produced = [
        std::mem::take(&mut result.stdout),
        std::mem::take(&mut result.stderr),
    ];
    for (slot, data) in produced.into_iter().enumerate() {
        match fds[slot].clone() {
            Sink::Fd(0) => result.stdout.push_str(&data),
            Sink::Fd(_) => result.stderr.push_str(&data),
            // The bit bucket swallows the bytes and creates nothing.
            Sink::File(path, _) if path == "/dev/null" => {}
            Sink::File(path, append_mode) => match files
                .iter_mut()
                .find(|(p, a, _)| *p == path && *a == append_mode)
            {
                Some(entry) => entry.2.push_str(&data),
                None => files.push((path, append_mode, data)),
            },
        }
    }
    for (path, append_mode, data) in files {
        let written = if append_mode {
            append(c, &path, data.as_bytes(), t)
        } else {
            c.vfs.write_as(&path, data.as_bytes(), &c.user, t)
        };
        if let Err(e) = written {
            result.stderr.push_str(&format!("{e}\n"));
            result.exit_code = 1;
        }
    }
}
/// Bounds that keep a loop from hanging a deterministic simulator. Every `execute`
/// gets its own budget and nesting is capped at 32, so the total work one command can
/// do is finite; exceeding either is status 2, never a hang.
const STEP_BUDGET: u64 = 10_000;
const LOOP_BUDGET: u64 = 10_000;
#[derive(Clone, Copy)]
enum Budget {
    Step,
    Loop,
}
/// Why a statement list stopped early. `Budget` unwinds every frame at once.
#[derive(Clone, PartialEq)]
enum Flow {
    Normal,
    Break(u32),
    Continue(u32),
    Return(i32),
    /// `exit`: unwinds to the end of this shell invocation, through functions too.
    Exit(i32),
    Budget(String),
}
/// Per-invocation shell state: functions, the early-exit signal, and the budgets.
/// It is not part of `Computer`, so nothing about it is serialised or replayed.
struct Ctx {
    functions: BTreeMap<String, std::sync::Arc<Vec<Node>>>,
    flow: Flow,
    steps: u64,
    loops: u64,
    /// The shared input stream a compound was given by `< file` or a pipe. `read`
    /// consumes from it, which is what makes `while read … done < f` terminate.
    stdin: Option<String>,
    /// One frame per active function call, holding what `local` shadowed.
    locals: Vec<Vec<(String, Option<String>)>>,
    /// `getopts` position inside a cluster such as `-ab`, plus the `OPTIND` it
    /// belongs to so a script resetting `OPTIND=1` restarts cleanly.
    optpos: usize,
    optind: usize,
    /// Depth of enclosing loops and function bodies: `break` and `return` outside one
    /// are refused rather than silently doing nothing.
    in_loop: u32,
    in_function: u32,
}
impl Default for Ctx {
    fn default() -> Self {
        Self {
            functions: BTreeMap::new(),
            flow: Flow::Normal,
            steps: STEP_BUDGET,
            loops: LOOP_BUDGET,
            stdin: None,
            locals: Vec::new(),
            optpos: 0,
            optind: 0,
            in_loop: 0,
            in_function: 0,
        }
    }
}
impl Ctx {
    fn charge(&mut self, kind: Budget) -> bool {
        let (left, message) = match kind {
            Budget::Step => (
                &mut self.steps,
                format!("command budget exhausted after {STEP_BUDGET} commands"),
            ),
            Budget::Loop => (
                &mut self.loops,
                format!("loops exceeded {LOOP_BUDGET} total iterations"),
            ),
        };
        if *left == 0 {
            self.flow = Flow::Budget(message);
            return false;
        }
        *left -= 1;
        true
    }
}
/// A parsed statement. `Simple` holds the tokens of one and-or list and is handed
/// straight to `run_list`, so quoting, pipes and redirection behave as they always did.
#[derive(Clone)]
enum Node {
    Simple(Vec<Token>),
    /// `(condition, body)` per `if`/`elif`, then the `else` list.
    If {
        branches: Vec<(Vec<Node>, Vec<Node>)>,
        otherwise: Vec<Node>,
    },
    /// `words` is `None` for `for x; do`, which walks the positional parameters.
    For {
        name: String,
        words: Option<Vec<Token>>,
        body: Vec<Node>,
    },
    Loop {
        until: bool,
        condition: Vec<Node>,
        body: Vec<Node>,
    },
    Case {
        subject: Token,
        arms: Vec<(Vec<Token>, Vec<Node>)>,
    },
    Function {
        name: String,
        body: std::sync::Arc<Vec<Node>>,
    },
    Subshell(Vec<Node>),
    Group(Vec<Node>),
    /// `head GATE tail` where at least one side is compound; a plain and-or list of
    /// simple commands stays one `Simple` node and keeps the older evaluator.
    Chain {
        head: Box<Node>,
        gate: String,
        tail: Box<Node>,
    },
    /// Redirections attached to a compound command, e.g. `done < input`.
    Redirect {
        body: Box<Node>,
        ops: Vec<(String, Option<Token>)>,
    },
    /// The tokens between `[[` and `]]`; they include operators the ordinary
    /// evaluator would have split the command on, so they are kept whole.
    Conditional(Vec<Token>),
}
/// A shell variable or function name: letters, digits and `_`, never leading a digit.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
}
/// The unquoted text of a one-part word; keywords are only keywords unquoted, so
/// `"if"` and `\if` stay ordinary arguments.
fn bare(t: Option<&Token>) -> Option<&str> {
    match t {
        Some(Token::Word(parts)) if parts.len() == 1 && parts[0].1 & 2 != 0 => Some(&parts[0].0),
        _ => None,
    }
}
const KEYWORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "in", "while", "until", "do", "done", "case",
    "esac", "function", "{", "}",
];
struct Grammar<'a> {
    t: &'a [Token],
    i: usize,
}
impl<'a> Grammar<'a> {
    fn program(tokens: &'a [Token]) -> Result<Vec<Node>, String> {
        let mut g = Grammar { t: tokens, i: 0 };
        let nodes = g.list(&[])?;
        if g.i < g.t.len() {
            return Err(match &g.t[g.i] {
                Token::Op(op) => format!("unexpected `{op}`"),
                Token::Word(_) => format!("unexpected `{}`", bare(g.t.get(g.i)).unwrap_or("word")),
            });
        }
        Ok(nodes)
    }
    fn op(&self, offset: usize) -> Option<&str> {
        match self.t.get(self.i + offset) {
            Some(Token::Op(s)) => Some(s.as_str()),
            _ => None,
        }
    }
    fn word(&self, offset: usize) -> Option<&str> {
        bare(self.t.get(self.i + offset))
    }
    /// `;;` ends a `case` arm; the lexer emits it as two separators.
    fn double_semicolon(&self) -> bool {
        self.op(0) == Some(";") && self.op(1) == Some(";")
    }
    fn stops(&self, terminators: &[&str]) -> bool {
        if self.i >= self.t.len() {
            return true;
        }
        if terminators.contains(&";;") && self.double_semicolon() {
            return true;
        }
        self.op(0).is_some_and(|o| terminators.contains(&o))
            || self.word(0).is_some_and(|w| terminators.contains(&w))
    }
    fn list(&mut self, terminators: &[&str]) -> Result<Vec<Node>, String> {
        let mut nodes = Vec::new();
        loop {
            while matches!(self.op(0), Some(";" | "\n" | "&")) {
                if terminators.contains(&";;") && self.double_semicolon() {
                    break;
                }
                self.i += 1;
            }
            if self.stops(terminators) {
                return Ok(nodes);
            }
            nodes.push(self.statement()?);
            // Anything left that is not a separator is a stray operator, not silence.
            if let Some(op) = self.op(0).filter(|o| !matches!(*o, ";" | "\n" | "&" | ")")) {
                return Err(format!("unexpected `{op}`"));
            }
        }
    }
    fn expect_word(&mut self, name: &str) -> Result<(), String> {
        if self.word(0) == Some(name) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{name}`"))
        }
    }
    fn expect_op(&mut self, name: &str) -> Result<(), String> {
        if self.op(0) == Some(name) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{name}`"))
        }
    }
    /// A word that can only begin a compound command, so a gate in front of it means
    /// the pipeline continues into that compound rather than into another argument.
    fn compound_ahead(&self, offset: usize) -> bool {
        self.op(offset) == Some("(")
            || bare(self.t.get(self.i + offset)).is_some_and(|w| {
                matches!(
                    w,
                    "if" | "for" | "while" | "until" | "case" | "function" | "{" | "[["
                )
            })
    }
    fn statement(&mut self) -> Result<Node, String> {
        let mut node = self.unit()?;
        // A gate is consumed here only when `unit` stopped at one, which happens when
        // a compound follows it or the unit itself was compound.
        while let Some(gate) = self.op(0).filter(|o| matches!(*o, "|" | "&&" | "||")) {
            let gate = gate.to_string();
            self.i += 1;
            if self.i >= self.t.len() {
                return Err(format!("missing command after `{gate}`"));
            }
            node = Node::Chain {
                head: Box::new(node),
                gate,
                tail: Box::new(self.unit()?),
            };
        }
        Ok(node)
    }
    /// One pipeline element: a compound (with any redirections of its own) or a run
    /// of tokens the older and-or evaluator understands.
    fn unit(&mut self) -> Result<Node, String> {
        if self.compound_ahead(0) {
            let node = self.compound()?;
            return self.redirections(node);
        }
        self.simple()
    }
    /// Trailing `< f`, `> f`, `2>&1` and friends on a compound command.
    fn redirections(&mut self, node: Node) -> Result<Node, String> {
        let mut ops = Vec::new();
        while let Some(op) = self
            .op(0)
            .filter(|o| *o == "<" || *o == "<<" || o.contains('>'))
        {
            let op = op.to_string();
            self.i += 1;
            if op.ends_with("&1") || op.ends_with("&2") {
                ops.push((op, None));
                continue;
            }
            let target = self
                .t
                .get(self.i)
                .filter(|t| matches!(t, Token::Word(_)))
                .ok_or_else(|| format!("missing target for `{op}`"))?
                .clone();
            self.i += 1;
            ops.push((op, Some(target)));
        }
        Ok(if ops.is_empty() {
            node
        } else {
            Node::Redirect {
                body: Box::new(node),
                ops,
            }
        })
    }
    fn compound(&mut self) -> Result<Node, String> {
        if self.op(0) == Some("(") {
            self.i += 1;
            let body = self.list(&[")"])?;
            self.expect_op(")")?;
            return Ok(Node::Subshell(body));
        }
        match self.word(0) {
            Some("{") => {
                self.i += 1;
                let body = self.list(&["}"])?;
                self.expect_word("}")?;
                return Ok(Node::Group(body));
            }
            Some("if") => return self.parse_if(),
            Some("for") => return self.parse_for(),
            Some("while") => return self.parse_loop(false),
            Some("until") => return self.parse_loop(true),
            Some("case") => return self.parse_case(),
            Some("[[") => {
                self.i += 1;
                let start = self.i;
                while self.i < self.t.len() && self.word(0) != Some("]]") {
                    self.i += 1;
                }
                if self.i >= self.t.len() {
                    return Err("expected `]]`".into());
                }
                let body = self.t[start..self.i].to_vec();
                self.i += 1;
                return Ok(Node::Conditional(body));
            }
            Some("function") => {
                self.i += 1;
                let name = self.function_name()?;
                if self.op(0) == Some("(") && self.op(1) == Some(")") {
                    self.i += 2;
                }
                return self.parse_body(name);
            }
            _ => {}
        }
        Err("expected a compound command".into())
    }
    fn simple(&mut self) -> Result<Node, String> {
        if self.op(1) == Some("(") && self.op(2) == Some(")") && self.word(0).is_some() {
            let name = self.function_name()?;
            self.i += 2;
            return self.parse_body(name);
        }
        let start = self.i;
        while self.i < self.t.len() && !matches!(self.op(0), Some(";" | "\n" | "&" | ")")) {
            // A gate in front of a compound ends this run; `statement` chains them.
            if matches!(self.op(0), Some("|" | "&&" | "||")) && self.compound_ahead(1) {
                break;
            }
            self.i += 1;
        }
        if self.i == start {
            return Err(format!("unexpected `{}`", self.op(0).unwrap_or("token")));
        }
        Ok(Node::Simple(self.t[start..self.i].to_vec()))
    }
    fn function_name(&mut self) -> Result<String, String> {
        let name = self
            .word(0)
            .filter(|n| !KEYWORDS.contains(n) && valid_name(n))
            .ok_or("invalid function name")?
            .to_string();
        self.i += 1;
        Ok(name)
    }
    fn parse_body(&mut self, name: String) -> Result<Node, String> {
        while matches!(self.op(0), Some(";" | "\n")) {
            self.i += 1;
        }
        self.expect_word("{")?;
        let body = self.list(&["}"])?;
        self.expect_word("}")?;
        Ok(Node::Function {
            name,
            body: std::sync::Arc::new(body),
        })
    }
    fn parse_if(&mut self) -> Result<Node, String> {
        let mut branches = Vec::new();
        let mut otherwise = Vec::new();
        self.expect_word("if")?;
        loop {
            let condition = self.list(&["then"])?;
            self.expect_word("then")?;
            branches.push((condition, self.list(&["elif", "else", "fi"])?));
            match self.word(0) {
                Some("elif") => self.i += 1,
                Some("else") => {
                    self.i += 1;
                    otherwise = self.list(&["fi"])?;
                    break;
                }
                _ => break,
            }
        }
        self.expect_word("fi")?;
        Ok(Node::If {
            branches,
            otherwise,
        })
    }
    fn parse_for(&mut self) -> Result<Node, String> {
        self.expect_word("for")?;
        let name = self.function_name()?;
        let mut words = None;
        if self.word(0) == Some("in") {
            self.i += 1;
            let mut collected = Vec::new();
            while self.i < self.t.len() && self.op(0).is_none() && self.word(0) != Some("do") {
                collected.push(self.t[self.i].clone());
                self.i += 1;
            }
            words = Some(collected);
        }
        while matches!(self.op(0), Some(";" | "\n")) {
            self.i += 1;
        }
        self.expect_word("do")?;
        let body = self.list(&["done"])?;
        self.expect_word("done")?;
        Ok(Node::For { name, words, body })
    }
    fn parse_loop(&mut self, until: bool) -> Result<Node, String> {
        self.expect_word(if until { "until" } else { "while" })?;
        let condition = self.list(&["do"])?;
        self.expect_word("do")?;
        let body = self.list(&["done"])?;
        self.expect_word("done")?;
        Ok(Node::Loop {
            until,
            condition,
            body,
        })
    }
    fn parse_case(&mut self) -> Result<Node, String> {
        self.expect_word("case")?;
        let subject = self
            .t
            .get(self.i)
            .filter(|t| matches!(t, Token::Word(_)))
            .ok_or("case needs a word to match")?
            .clone();
        self.i += 1;
        while matches!(self.op(0), Some("\n")) {
            self.i += 1;
        }
        self.expect_word("in")?;
        let mut arms = Vec::new();
        loop {
            while matches!(self.op(0), Some(";" | "\n")) {
                self.i += 1;
            }
            if self.word(0) == Some("esac") {
                self.i += 1;
                break;
            }
            if self.i >= self.t.len() {
                return Err("expected `esac`".into());
            }
            // A leading `(` is optional, as in `(a|b)`.
            if self.op(0) == Some("(") {
                self.i += 1;
            }
            let mut patterns = Vec::new();
            loop {
                let pattern = self
                    .t
                    .get(self.i)
                    .filter(|t| matches!(t, Token::Word(_)))
                    .ok_or("case pattern expected")?
                    .clone();
                patterns.push(pattern);
                self.i += 1;
                if self.op(0) == Some("|") {
                    self.i += 1;
                } else {
                    break;
                }
            }
            self.expect_op(")")?;
            let body = self.list(&[";;", "esac"])?;
            if self.double_semicolon() {
                self.i += 2;
            }
            arms.push((patterns, body));
        }
        Ok(Node::Case { subject, arms })
    }
}
/// What a `break`/`continue` signal means to the loop that catches it.
enum Caught {
    Iterate,
    Stop,
    Propagate,
}
fn caught(ctx: &mut Ctx) -> Caught {
    match ctx.flow.clone() {
        Flow::Normal => Caught::Iterate,
        Flow::Break(n) => {
            ctx.flow = if n > 1 {
                Flow::Break(n - 1)
            } else {
                Flow::Normal
            };
            Caught::Stop
        }
        Flow::Continue(n) => {
            if n > 1 {
                ctx.flow = Flow::Continue(n - 1);
                Caught::Stop
            } else {
                ctx.flow = Flow::Normal;
                Caught::Iterate
            }
        }
        _ => Caught::Propagate,
    }
}
#[allow(clippy::too_many_arguments)]
fn run_nodes(
    c: &mut Computer,
    nodes: &[Node],
    ctx: &mut Ctx,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    total: &mut CommandResult,
    mut status: i32,
) -> i32 {
    for node in nodes {
        if ctx.flow != Flow::Normal {
            break;
        }
        status = run_node(c, node, ctx, t, host, depth, total, status);
    }
    status
}
#[allow(clippy::too_many_arguments)]
fn run_node(
    c: &mut Computer,
    node: &Node,
    ctx: &mut Ctx,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    total: &mut CommandResult,
    status: i32,
) -> i32 {
    if depth > 32 {
        ctx.flow = Flow::Budget("execution nesting exceeds 32".into());
        return 2;
    }
    match node {
        Node::Simple(tokens) => run_list(c, tokens, ctx, t, host, depth, total, status, ""),
        Node::Conditional(tokens) => match conditional(c, tokens, t, host, depth, status) {
            Ok(true) => 0,
            Ok(false) => 1,
            Err(e) => {
                total.stderr.push_str(&format!("[[: {e}\n"));
                2
            }
        },
        Node::Chain { head, gate, tail } => {
            let mut local = CommandResult::default();
            let code = run_node(c, head, ctx, t, host, depth, &mut local, status);
            total.clear |= local.clear;
            total.stderr.push_str(&local.stderr);
            if ctx.flow != Flow::Normal {
                total.stdout.push_str(&local.stdout);
                return code;
            }
            if gate == "|" {
                // A simple tail reads the bytes directly; a compound tail reads them
                // through the shared stream, which is what `read` consumes from.
                return match tail.as_ref() {
                    Node::Simple(tokens) => {
                        run_list(c, tokens, ctx, t, host, depth, total, code, &local.stdout)
                    }
                    other => {
                        let saved = ctx.stdin.replace(local.stdout);
                        let code = run_node(c, other, ctx, t, host, depth, total, code);
                        ctx.stdin = saved;
                        code
                    }
                };
            }
            total.stdout.push_str(&local.stdout);
            if (gate == "&&") == (code == 0) {
                run_node(c, tail, ctx, t, host, depth, total, code)
            } else {
                code
            }
        }
        Node::Redirect { body, ops } => {
            let mut fds: [Sink; 2] = [Sink::Fd(0), Sink::Fd(1)];
            let mut stream = None;
            for (op, target) in ops {
                if let Some(source) = op.strip_suffix("&1").or_else(|| op.strip_suffix("&2")) {
                    let to = usize::from(op.ends_with('2'));
                    fds[usize::from(source.starts_with('2'))] = fds[to].clone();
                    continue;
                }
                let Some(token) = target else { continue };
                let text = match word(token, c, status, t, host, depth) {
                    Ok(v) => v,
                    Err(e) => {
                        total.stderr.push_str(&format!("shell: {e}\n"));
                        return 2;
                    }
                };
                if op == "<<" {
                    stream = Some(text);
                } else if op == "<" {
                    match c.vfs.read_as(&c.resolve(&text), &c.user) {
                        Ok(v) => stream = Some(String::from_utf8_lossy(&v).into_owned()),
                        Err(e) => {
                            total.stderr.push_str(&format!("{e}\n"));
                            return 1;
                        }
                    }
                } else {
                    let sink = Sink::File(c.resolve(&text), op.ends_with(">>"));
                    if op.starts_with('&') {
                        fds = [sink.clone(), sink];
                    } else if op.starts_with('2') {
                        fds[1] = sink;
                    } else {
                        fds[0] = sink;
                    }
                }
            }
            // Without `< …` the body keeps whatever stream it already had.
            let replacement = stream.or_else(|| ctx.stdin.clone());
            let saved = std::mem::replace(&mut ctx.stdin, replacement);
            let mut local = CommandResult::default();
            let code = run_node(c, body, ctx, t, host, depth, &mut local, status);
            ctx.stdin = saved;
            local.exit_code = code;
            dispatch(c, &mut local, &fds, t);
            total.clear |= local.clear;
            total.stdout.push_str(&local.stdout);
            total.stderr.push_str(&local.stderr);
            if local.exit_code != code {
                return local.exit_code;
            }
            code
        }
        Node::Group(body) => run_nodes(c, body, ctx, t, host, depth, total, status),
        Node::Function { name, body } => {
            ctx.functions.insert(name.clone(), body.clone());
            0
        }
        Node::Subshell(body) => {
            // A subshell sees the parent's state and leaves none of its own behind,
            // exactly as `$( … )` already does; `break` never escapes one.
            let (env, cwd, functions) = (c.env.clone(), c.cwd.clone(), ctx.functions.clone());
            let saved = (ctx.in_loop, ctx.in_function);
            ctx.in_loop = 0;
            ctx.in_function = 0;
            let mut code = run_nodes(c, body, ctx, t, host, depth + 1, total, status);
            (c.env, c.cwd, ctx.functions) = (env, cwd, functions);
            (ctx.in_loop, ctx.in_function) = saved;
            // `exit` ends the subshell, not the shell that spawned it.
            if let Flow::Exit(n) = ctx.flow {
                code = n;
            }
            if !matches!(ctx.flow, Flow::Budget(_)) {
                ctx.flow = Flow::Normal;
            }
            code
        }
        Node::If {
            branches,
            otherwise,
        } => {
            for (condition, body) in branches {
                let tested = run_nodes(c, condition, ctx, t, host, depth, total, status);
                if ctx.flow != Flow::Normal {
                    return tested;
                }
                if tested == 0 {
                    return run_nodes(c, body, ctx, t, host, depth, total, tested);
                }
            }
            run_nodes(c, otherwise, ctx, t, host, depth, total, 0)
        }
        Node::For { name, words, body } => {
            let items = match for_items(c, words.as_deref(), ctx, t, host, depth, status) {
                Ok(v) => v,
                Err(e) => {
                    total.stderr.push_str(&format!("for: {e}\n"));
                    return 2;
                }
            };
            let mut code = 0;
            ctx.in_loop += 1;
            for item in items {
                if !ctx.charge(Budget::Loop) {
                    break;
                }
                c.env.insert(name.clone(), item);
                code = run_nodes(c, body, ctx, t, host, depth, total, code);
                match caught(ctx) {
                    Caught::Iterate => {}
                    Caught::Stop | Caught::Propagate => break,
                }
            }
            ctx.in_loop -= 1;
            code
        }
        Node::Loop {
            until,
            condition,
            body,
        } => {
            let mut code = 0;
            ctx.in_loop += 1;
            loop {
                if !ctx.charge(Budget::Loop) {
                    break;
                }
                let tested = run_nodes(c, condition, ctx, t, host, depth, total, code);
                if ctx.flow != Flow::Normal || (tested == 0) == *until {
                    break;
                }
                code = run_nodes(c, body, ctx, t, host, depth, total, code);
                match caught(ctx) {
                    Caught::Iterate => {}
                    Caught::Stop | Caught::Propagate => break,
                }
            }
            ctx.in_loop -= 1;
            code
        }
        Node::Case { subject, arms } => {
            let value = match word(subject, c, status, t, host, depth) {
                Ok(v) => v,
                Err(e) => {
                    total.stderr.push_str(&format!("case: {e}\n"));
                    return 2;
                }
            };
            for (patterns, body) in arms {
                for pattern in patterns {
                    let Ok(p) = word(pattern, c, status, t, host, depth) else {
                        continue;
                    };
                    if wildcard(&p, &value) {
                        return run_nodes(c, body, ctx, t, host, depth, total, 0);
                    }
                }
            }
            0
        }
    }
}
/// A `for` list is the one place this shell splits a word: an unquoted expansion is
/// split on whitespace and globbed, so `for f in *.txt` and `for x in $(ls)` iterate.
#[allow(clippy::too_many_arguments)]
fn for_items(
    c: &mut Computer,
    words: Option<&[Token]>,
    _ctx: &mut Ctx,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    status: i32,
) -> Result<Vec<String>, String> {
    let Some(words) = words else {
        let (n, _) = positional(c);
        return Ok((1..=n)
            .filter_map(|i| c.env.get(&i.to_string()).cloned())
            .collect());
    };
    let mut items = Vec::new();
    for token in words {
        let glob = matches!(token, Token::Word(parts) if parts.iter().any(|(p, f)| f & 2 != 0 && (p.contains('*') || p.contains('?'))));
        for piece in word_fields(token, c, status, t, host, depth)? {
            let paths = if glob && (piece.contains('*') || piece.contains('?')) {
                glob_paths(c, &piece)
            } else {
                vec![]
            };
            if paths.is_empty() {
                items.push(piece);
            } else {
                items.extend(paths);
            }
        }
    }
    Ok(items)
}
/// `break`, `continue` and `return`: the three commands that are a signal rather than
/// output. Outside their construct they are refused, not quietly ignored.
fn control_word(args: &[String], ctx: &mut Ctx, status: i32) -> Option<CommandResult> {
    let name = args[0].as_str();
    if !matches!(name, "break" | "continue" | "return" | "exit") {
        return None;
    }
    if args.len() > 2 {
        return Some(CommandResult::new(
            format!("{name}: too many arguments\n"),
            2,
        ));
    }
    let operand = args.get(1).map(String::as_str);
    if name == "exit" {
        // Bare `exit` carries the status of the last command, as bash does.
        let code = match operand.map(str::parse::<i32>) {
            None => status,
            Some(Ok(n)) => n,
            Some(Err(_)) => {
                return Some(CommandResult::new("exit: numeric argument required\n", 2))
            }
        };
        ctx.flow = Flow::Exit(code);
        return Some(CommandResult {
            exit_code: code,
            ..CommandResult::default()
        });
    }
    if name == "return" {
        if ctx.in_function == 0 {
            return Some(CommandResult::new("return: not in a function\n", 2));
        }
        let code = match operand.map(str::parse::<i32>) {
            None => 0,
            Some(Ok(n)) => n,
            Some(Err(_)) => {
                return Some(CommandResult::new("return: numeric argument required\n", 2))
            }
        };
        ctx.flow = Flow::Return(code);
        return Some(CommandResult {
            exit_code: code,
            ..CommandResult::default()
        });
    }
    if ctx.in_loop == 0 {
        return Some(CommandResult::new(format!("{name}: not in a loop\n"), 2));
    }
    let count = match operand.map(str::parse::<u32>) {
        None => 1,
        Some(Ok(n)) if n >= 1 => n,
        Some(_) => {
            return Some(CommandResult::new(
                format!("{name}: numeric argument required\n"),
                2,
            ))
        }
    };
    ctx.flow = if name == "break" {
        Flow::Break(count)
    } else {
        Flow::Continue(count)
    };
    Some(CommandResult::default())
}
/// One expanded operand of a `[[ … ]]` test. `raw` records that it was unquoted, which
/// is what decides whether `==` compares or matches a pattern.
enum Item {
    Op(String),
    Word(String, bool),
}
const CONDITIONAL_BINARY: &[&str] = &[
    "==", "=", "!=", "=~", "-eq", "-ne", "-lt", "-gt", "-le", "-ge", "-nt", "-ot",
];
/// `[[ … ]]`. Its tokens were kept whole by the parser, so `&&`, `||`, `!` and
/// parentheses mean what bash says they mean here instead of splitting the command.
fn conditional(
    c: &mut Computer,
    tokens: &[Token],
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    status: i32,
) -> Result<bool, String> {
    let mut items = Vec::new();
    for token in tokens {
        match token {
            Token::Op(op) => items.push(Item::Op(op.clone())),
            Token::Word(parts) => items.push(Item::Word(
                word(token, c, status, t, host, depth)?,
                parts.iter().any(|(_, f)| f & 2 != 0),
            )),
        }
    }
    let mut i = 0;
    let value = cond_or(c, &items, &mut i)?;
    if i != items.len() {
        return Err("trailing operand".into());
    }
    Ok(value)
}
fn cond_or(c: &Computer, items: &[Item], i: &mut usize) -> Result<bool, String> {
    let mut value = cond_and(c, items, i)?;
    while matches!(items.get(*i), Some(Item::Op(o)) if o == "||") {
        *i += 1;
        // Both sides are evaluated: nothing here has an effect to short-circuit.
        value = cond_and(c, items, i)? || value;
    }
    Ok(value)
}
fn cond_and(c: &Computer, items: &[Item], i: &mut usize) -> Result<bool, String> {
    let mut value = cond_term(c, items, i)?;
    while matches!(items.get(*i), Some(Item::Op(o)) if o == "&&") {
        *i += 1;
        value = cond_term(c, items, i)? && value;
    }
    Ok(value)
}
fn cond_term(c: &Computer, items: &[Item], i: &mut usize) -> Result<bool, String> {
    match items.get(*i) {
        Some(Item::Word(w, _)) if w == "!" => {
            *i += 1;
            Ok(!cond_term(c, items, i)?)
        }
        Some(Item::Op(o)) if o == "(" => {
            *i += 1;
            let value = cond_or(c, items, i)?;
            if !matches!(items.get(*i), Some(Item::Op(o)) if o == ")") {
                return Err("expected `)`".into());
            }
            *i += 1;
            Ok(value)
        }
        _ => cond_primary(c, items, i),
    }
}
fn cond_primary(c: &Computer, items: &[Item], i: &mut usize) -> Result<bool, String> {
    let Some(Item::Word(left, _)) = items.get(*i) else {
        return Err("expected an operand".into());
    };
    *i += 1;
    let operator = match items.get(*i) {
        // `<` and `>` reach us as operators; inside `[[ ]]` they compare strings.
        Some(Item::Op(o)) if o == "<" || o == ">" => Some(o.clone()),
        Some(Item::Word(w, _)) if CONDITIONAL_BINARY.contains(&w.as_str()) => Some(w.clone()),
        _ => None,
    };
    if let Some(operator) = operator {
        *i += 1;
        let Some(Item::Word(right, raw)) = items.get(*i) else {
            return Err(format!("`{operator}` needs a right operand"));
        };
        *i += 1;
        return match operator.as_str() {
            "==" | "=" | "!=" => {
                // An unquoted right side is a pattern, a quoted one is a literal.
                let hit = if *raw {
                    wildcard(right, left)
                } else {
                    left == right
                };
                Ok(hit == (operator != "!="))
            }
            "=~" => regex::Regex::new(right)
                .map(|re| re.is_match(left))
                .map_err(|e| e.to_string()),
            "<" => Ok(left < right),
            ">" => Ok(left > right),
            _ => test_primary(c, &[left.clone(), operator, right.clone()]).map_err(|f| f.text),
        };
    }
    if left.starts_with('-') && left.chars().count() == 2 {
        if let Some(Item::Word(operand, _)) = items.get(*i) {
            *i += 1;
            return test_primary(c, &[left.clone(), operand.clone()]).map_err(|f| f.text);
        }
    }
    Ok(!left.is_empty())
}
/// The primaries `test`, `[` and `[[` share.
fn test_primary(c: &Computer, vals: &[String]) -> Result<bool, Fail> {
    let access = |p: &String, r: bool, w: bool, x: bool| {
        c.vfs.check_access(&c.resolve(p), &c.user, r, w, x).is_ok()
    };
    let modified = |p: &String| c.vfs.stat(&c.resolve(p)).map(|m| m.modified);
    Ok(match vals {
        [flag, p] if flag == "-e" => c.vfs.stat(&c.resolve(p)).is_ok(),
        [flag, p] if flag == "-f" => c.vfs.read_as(&c.resolve(p), &c.user).is_ok(),
        [flag, p] if flag == "-d" => c.vfs.list_as(&c.resolve(p), &c.user).is_ok(),
        [flag, p] if flag == "-s" => c.vfs.stat(&c.resolve(p)).is_ok_and(|m| m.size > 0),
        [flag, p] if flag == "-L" || flag == "-h" => {
            c.vfs.lstat(&c.resolve(p)).is_ok_and(|m| m.is_symlink)
        }
        [flag, p] if flag == "-r" => access(p, true, false, false),
        [flag, p] if flag == "-w" => access(p, false, true, false),
        [flag, p] if flag == "-x" => access(p, false, false, true),
        [flag, v] if flag == "-n" => !v.is_empty(),
        [flag, v] if flag == "-z" => v.is_empty(),
        [left, op, right] => match op.as_str() {
            "=" | "==" => left == right,
            "!=" => left != right,
            // Both timestamps must exist; a missing file is never newer.
            "-nt" | "-ot" => match (modified(left), modified(right)) {
                (Ok(l), Ok(r)) => {
                    if op == "-nt" {
                        l > r
                    } else {
                        l < r
                    }
                }
                (Ok(_), Err(_)) => op == "-nt",
                (Err(_), Ok(_)) => op == "-ot",
                _ => false,
            },
            "-eq" | "-ne" | "-lt" | "-gt" | "-le" | "-ge" => {
                let l = left.parse::<i64>().map_err(|_| "invalid integer")?;
                let r = right.parse::<i64>().map_err(|_| "invalid integer")?;
                match op.as_str() {
                    "-eq" => l == r,
                    "-ne" => l != r,
                    "-lt" => l < r,
                    "-gt" => l > r,
                    "-le" => l <= r,
                    _ => l >= r,
                }
            }
            _ => return Err(Fail::usage(format!("unsupported test operator `{op}`"))),
        },
        [flag, _] if flag.starts_with('-') && flag.chars().count() == 2 => {
            return Err(Fail::usage(format!("unsupported test operator `{flag}`")))
        }
        [v] => !v.is_empty(),
        [] => false,
        _ => return Err(Fail::usage("unsupported test expression")),
    })
}
/// Pulls one line off a stream, consuming its newline. `None` means end of input.
/// The bool says a newline terminated it, which is how `read` reports end of file.
fn take_line(buffer: &mut String) -> Option<(String, bool)> {
    if buffer.is_empty() {
        return None;
    }
    match buffer.find('\n') {
        Some(k) => {
            let line = buffer[..k].to_string();
            buffer.drain(..=k);
            Some((line, true))
        }
        None => Some((std::mem::take(buffer), false)),
    }
}
/// The builtins that need the shell's own state rather than just the computer.
#[allow(clippy::too_many_arguments)]
fn shell_builtin(
    c: &mut Computer,
    args: &[String],
    input: &str,
    ctx: &mut Ctx,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Option<CommandResult> {
    match args[0].as_str() {
        "read" => Some(builtin_read(c, args, input, ctx)),
        "local" => Some(builtin_local(c, args, ctx)),
        "getopts" => Some(builtin_getopts(c, args, ctx)),
        "source" | "." => Some(builtin_source(c, args, ctx, t, host, depth)),
        "sqlite3" => Some(crate::sqlite::execute(c, args, input, t)),
        _ => None,
    }
}
/// `read [-r] [NAME…]`. Fields are split on whitespace and the last name takes the
/// remainder. Status 1 at end of input is what stops a `while read` loop.
fn builtin_read(c: &mut Computer, args: &[String], input: &str, ctx: &mut Ctx) -> CommandResult {
    let mut names = Vec::new();
    for arg in &args[1..] {
        if let Some(long) = arg.strip_prefix("--").filter(|f| !f.is_empty()) {
            return CommandResult::new(format!("{}\n", unrecognized_option("read", long).text), 2);
        }
        if let Some(flags) = arg.strip_prefix('-').filter(|f| !f.is_empty()) {
            // -r is the behaviour either way: this shell never unescapes a read line.
            if let Some(bad) = flags.chars().find(|ch| *ch != 'r') {
                return CommandResult::new(format!("{}\n", invalid_option("read", bad).text), 2);
            }
            continue;
        }
        if !valid_name(arg) {
            return CommandResult::new(format!("read: `{arg}` is not a valid name\n"), 2);
        }
        names.push(arg.clone());
    }
    // A redirect or pipe on this command is a one-shot stream; otherwise the shared
    // one a compound was given, which successive reads advance through.
    let mut private;
    let source = if input.is_empty() {
        ctx.stdin.get_or_insert_with(String::new)
    } else {
        private = input.to_string();
        &mut private
    };
    let Some((line, terminated)) = take_line(source) else {
        return CommandResult::new(String::new(), 1);
    };
    if names.is_empty() {
        c.env.insert("REPLY".into(), line.trim().to_string());
    } else {
        let mut rest = line.as_str();
        for (k, name) in names.iter().enumerate() {
            rest = rest.trim_start();
            let value = if k + 1 == names.len() {
                let all = rest;
                rest = "";
                all.trim_end().to_string()
            } else {
                let (field, tail) = match rest.find(char::is_whitespace) {
                    Some(p) => rest.split_at(p),
                    None => (rest, ""),
                };
                rest = tail;
                field.to_string()
            };
            c.env.insert(name.clone(), value);
        }
    }
    // Bash reports end of file when no newline closed the line, even having assigned.
    CommandResult::new(String::new(), i32::from(!terminated))
}
/// `local NAME[=VALUE]…`: shadows a variable until the enclosing function returns.
fn builtin_local(c: &mut Computer, args: &[String], ctx: &mut Ctx) -> CommandResult {
    if ctx.locals.is_empty() {
        return CommandResult::new("local: not in a function\n", 2);
    }
    if args.len() < 2 {
        return CommandResult::new("local: missing name\n", 2);
    }
    for arg in &args[1..] {
        let (name, value) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        if !valid_name(name) {
            return CommandResult::new(format!("local: `{name}` is not a valid name\n"), 2);
        }
        let frame = ctx.locals.last_mut().unwrap();
        if !frame.iter().any(|(k, _)| k == name) {
            frame.push((name.to_string(), c.env.get(name).cloned()));
        }
        match value {
            Some(v) => c.env.insert(name.into(), v),
            None => c.env.remove(name),
        };
    }
    CommandResult::default()
}
/// `source FILE [ARG…]` / `. FILE`: runs the file in this shell, so its variables,
/// working directory and functions persist. Bounded by the same nesting guard as a
/// script, because it is one.
fn builtin_source(
    c: &mut Computer,
    args: &[String],
    ctx: &mut Ctx,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> CommandResult {
    let Some(path) = args.get(1) else {
        return CommandResult::new(format!("{}: missing file\n", args[0]), 2);
    };
    if depth + 1 > 32 {
        return CommandResult::new("shell: execution nesting exceeds 32\n", 2);
    }
    let resolved = c.resolve(path);
    let bytes = match c.vfs.read_as(&resolved, &c.user) {
        Ok(v) => v,
        Err(e) => return CommandResult::error(format!("{}: {e}\n", args[0])),
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return CommandResult::new(format!("{}: file is not UTF-8\n", args[0]), 2);
    };
    let nodes = match lex(&text, c.dialect == "powershell").and_then(|v| Grammar::program(&v)) {
        Ok(v) => v,
        Err(e) => return CommandResult::new(format!("{}: {e}\n", args[0]), 2),
    };
    let saved = swap_positional(c, &args[1..]);
    let mut total = CommandResult::default();
    // `return` ends a sourced file, as it does a function.
    ctx.in_function += 1;
    let mut code = run_nodes(c, &nodes, ctx, t, host, depth + 1, &mut total, 0);
    ctx.in_function -= 1;
    if let Flow::Return(n) = ctx.flow {
        ctx.flow = Flow::Normal;
        code = n;
    }
    restore_positional(c, saved);
    total.exit_code = code;
    total
}
/// Replaces `$0…$#` with `values` and returns what was there.
fn swap_positional(c: &mut Computer, values: &[String]) -> Vec<(String, String)> {
    let saved: Vec<(String, String)> = c
        .env
        .iter()
        .filter(|(k, _)| *k == "#" || k.chars().all(|ch| ch.is_ascii_digit()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, _) in &saved {
        c.env.remove(k);
    }
    for (i, value) in values.iter().enumerate() {
        c.env.insert(i.to_string(), value.clone());
    }
    c.env
        .insert("#".into(), values.len().saturating_sub(1).to_string());
    saved
}
fn restore_positional(c: &mut Computer, saved: Vec<(String, String)>) {
    let live: Vec<String> = c
        .env
        .keys()
        .filter(|k| *k == "#" || k.chars().all(|ch| ch.is_ascii_digit()))
        .cloned()
        .collect();
    for k in live {
        c.env.remove(&k);
    }
    c.env.extend(saved);
}
/// `getopts OPTSTRING NAME [ARG…]`. `OPTIND` and `OPTARG` are ordinary shell
/// variables, as in bash; the position inside a cluster such as `-ab` is shell state.
fn builtin_getopts(c: &mut Computer, args: &[String], ctx: &mut Ctx) -> CommandResult {
    let ([optstring, name], rest) = match args.get(1..3) {
        Some([a, b]) => ([a.clone(), b.clone()], &args[3..]),
        _ => return CommandResult::new("getopts: usage: getopts OPTSTRING NAME [ARG…]\n", 2),
    };
    if !valid_name(&name) {
        return CommandResult::new(format!("getopts: `{name}` is not a valid name\n"), 2);
    }
    let quiet = optstring.starts_with(':');
    let spec: Vec<char> = optstring.trim_start_matches(':').chars().collect();
    let operands: Vec<String> = if rest.is_empty() {
        let (n, _) = positional(c);
        (1..=n)
            .filter_map(|i| c.env.get(&i.to_string()).cloned())
            .collect()
    } else {
        rest.to_vec()
    };
    let mut index: usize = c
        .env
        .get("OPTIND")
        .and_then(|v| v.parse().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(1);
    // A script that resets OPTIND restarts mid-cluster scanning too.
    if ctx.optind != index {
        ctx.optpos = 0;
    }
    let done = |c: &mut Computer, index: usize, ctx: &mut Ctx| {
        c.env.insert("OPTIND".into(), index.to_string());
        ctx.optind = index;
        ctx.optpos = 0;
        c.env.insert(name.clone(), "?".into());
        CommandResult::new(String::new(), 1)
    };
    loop {
        let Some(arg) = operands.get(index - 1) else {
            return done(c, index, ctx);
        };
        if arg == "--" {
            return done(c, index + 1, ctx);
        }
        if !arg.starts_with('-') || arg.len() < 2 {
            return done(c, index, ctx);
        }
        let chars: Vec<char> = arg.chars().collect();
        let position = if ctx.optpos == 0 { 1 } else { ctx.optpos };
        let Some(letter) = chars.get(position).copied() else {
            index += 1;
            ctx.optpos = 0;
            continue;
        };
        let known = spec.contains(&letter) && letter != ':';
        let takes = known && spec.iter().skip_while(|c| **c != letter).nth(1) == Some(&':');
        let report = |c: &mut Computer, ctx: &mut Ctx, value: &str, index: usize, pos: usize| {
            c.env.insert("OPTIND".into(), index.to_string());
            ctx.optind = index;
            ctx.optpos = pos;
            c.env.insert(name.clone(), value.into());
        };
        if !known {
            let (next, pos) = if position + 1 < chars.len() {
                (index, position + 1)
            } else {
                (index + 1, 0)
            };
            report(c, ctx, "?", next, pos);
            if quiet {
                c.env.insert("OPTARG".into(), letter.to_string());
                return CommandResult::default();
            }
            c.env.remove("OPTARG");
            return CommandResult {
                stderr: format!("getopts: illegal option -- {letter}\n"),
                ..CommandResult::default()
            };
        }
        if !takes {
            let (next, pos) = if position + 1 < chars.len() {
                (index, position + 1)
            } else {
                (index + 1, 0)
            };
            report(c, ctx, &letter.to_string(), next, pos);
            c.env.remove("OPTARG");
            return CommandResult::default();
        }
        // An option's argument is the rest of this word, or the whole next one.
        let glued: String = chars[position + 1..].iter().collect();
        if !glued.is_empty() {
            report(c, ctx, &letter.to_string(), index + 1, 0);
            c.env.insert("OPTARG".into(), glued);
            return CommandResult::default();
        }
        let Some(value) = operands.get(index) else {
            report(c, ctx, if quiet { ":" } else { "?" }, index + 1, 0);
            if quiet {
                c.env.insert("OPTARG".into(), letter.to_string());
                return CommandResult::default();
            }
            c.env.remove("OPTARG");
            return CommandResult {
                stderr: format!("getopts: option requires an argument -- {letter}\n"),
                ..CommandResult::default()
            };
        };
        let value = value.clone();
        report(c, ctx, &letter.to_string(), index + 2, 0);
        c.env.insert("OPTARG".into(), value);
        return CommandResult::default();
    }
}
/// A function call. Positional parameters are swapped for the duration and restored
/// afterwards; the body's output becomes this command's output, so it pipes normally.
fn call_function(
    c: &mut Computer,
    body: &[Node],
    args: &[String],
    ctx: &mut Ctx,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> CommandResult {
    if depth + 1 > 32 {
        return CommandResult::new("shell: execution nesting exceeds 32\n", 2);
    }
    let saved: Vec<(String, String)> = c
        .env
        .iter()
        .filter(|(k, _)| *k == "#" || k.chars().all(|ch| ch.is_ascii_digit()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, _) in &saved {
        // `$0` keeps naming the script, as in bash; only `$1…` are replaced.
        if k != "0" {
            c.env.remove(k);
        }
    }
    for (i, arg) in args.iter().enumerate().skip(1) {
        c.env.insert(i.to_string(), arg.clone());
    }
    c.env.insert("#".into(), (args.len() - 1).to_string());
    let mut total = CommandResult::default();
    ctx.in_function += 1;
    ctx.locals.push(Vec::new());
    let mut code = run_nodes(c, body, ctx, t, host, depth + 1, &mut total, 0);
    // `local` bindings are undone newest first, so a name shadowed twice comes back.
    for (name, previous) in ctx.locals.pop().unwrap_or_default().into_iter().rev() {
        match previous {
            Some(v) => c.env.insert(name, v),
            None => c.env.remove(&name),
        };
    }
    ctx.in_function -= 1;
    if let Flow::Return(n) = ctx.flow {
        ctx.flow = Flow::Normal;
        code = n;
    }
    for (k, _) in c
        .env
        .clone()
        .iter()
        .filter(|(k, _)| *k == "#" || k.chars().all(|ch| ch.is_ascii_digit()))
    {
        c.env.remove(k);
    }
    c.env.extend(saved);
    total.exit_code = code;
    total
}
/// Where a descriptor's bytes land: back to the caller on stdout/stderr, or into a file.
#[derive(Clone, PartialEq)]
enum Sink {
    Fd(usize),
    File(String, bool),
}
fn run(
    c: &mut Computer,
    a: &[String],
    input: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, Fail> {
    let cmd = a[0].to_ascii_lowercase();
    if c.dialect != "powershell"
        && matches!(
            cmd.as_str(),
            "write-output"
                | "get-location"
                | "set-location"
                | "get-childitem"
                | "get-content"
                | "set-content"
                | "add-content"
                | "copy-item"
                | "move-item"
                | "remove-item"
                | "select-string"
                | "get-process"
                | "stop-process"
                | "invoke-webrequest"
                | "test-path"
        )
    {
        return Err(Fail::new(format!("command not found: {}", a[0]), 127));
    }
    let args = &a[1..];
    // Language runtimes: `python3 …`, `node …`, also by absolute path.
    if let Some(runtime) = crate::runtimes::runtime_for(&a[0]) {
        let r = crate::runtimes::run_runtime(runtime, c, host, args, input, t);
        return runtime_result(r);
    }
    // The text and data utilities live in their own modules; they are consulted first
    // so the roster in `docs/shell.md` and the implementations cannot drift apart.
    if let Some(r) = crate::textutils::run(c, &cmd, args, input, t, host, depth) {
        return r;
    }
    if let Some(r) = crate::datautils::run(c, &cmd, args, input) {
        return r;
    }
    let required = |i: usize| {
        args.get(i)
            .map(String::as_str)
            .ok_or_else(|| "missing operand".to_string())
    };
    let err = Fail::from;
    match cmd.as_str() {
        "true" | ":" => Ok(String::new()),
        "false" => Err(Fail::new(String::new(), 1)),
        "echo" | "write-output" => {
            let no = args.first().is_some_and(|s| s == "-n");
            Ok(format!(
                "{}{}",
                args[usize::from(no)..].join(" "),
                if no { "" } else { "\n" }
            ))
        }
        "pwd" | "get-location" => Ok(format!("{}\n", c.cwd)),
        "whoami" => Ok(format!("{}\n", c.user)),
        "hostname" => Ok(format!("{}\n", c.id)),
        "uname" => Ok(format!("{}\n", c.os_family)),
        "date" => cmd_date(args, t),
        "cd" | "set-location" => {
            let p = c.resolve(
                args.first()
                    .map(String::as_str)
                    .unwrap_or_else(|| c.env.get("HOME").map(String::as_str).unwrap_or("/")),
            );
            c.vfs.list_as(&p, &c.user).map_err(err)?;
            c.cwd = p;
            Ok(String::new())
        }
        "env" | "printenv" => {
            // `env -i`, `env NAME=VALUE cmd` and `env cmd` would each need a different
            // model; none is implemented, so a flag is refused rather than dropped.
            let (_, names) = options(&cmd, args, "", "", &[])?;
            Ok(if let Some(k) = names.first() {
                if names.len() > 1 {
                    return Err(Fail::usage(format!(
                        "{cmd}: extra operand '{}'; this world prints the environment or \
                         one variable, it does not run a command in a modified one",
                        names[1]
                    )));
                }
                format!("{}\n", c.env.get(k).cloned().unwrap_or_default())
            } else {
                c.env.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
            })
        }
        "export" => {
            let (_, names) = options("export", args, "", "", &[])?;
            for s in &names {
                if let Some((k, v)) = s.split_once('=') {
                    c.env.insert(k.into(), v.into());
                }
            }
            Ok(String::new())
        }
        "unset" => {
            let (_, names) = options("unset", args, "", "", &[])?;
            for s in &names {
                c.env.remove(s);
            }
            Ok(String::new())
        }
        "ls" | "dir" | "get-childitem" => cmd_ls(c, args),
        "cat" | "type" | "get-content" => {
            let (opts, paths) = options(
                &cmd,
                args,
                "nbETAsvu",
                "",
                &[
                    ("number", 'n'),
                    ("number-nonblank", 'b'),
                    ("show-ends", 'E'),
                    ("show-tabs", 'T'),
                    ("show-all", 'A'),
                    ("squeeze-blank", 's'),
                    ("show-nonprinting", 'v'),
                ],
            )?;
            let mut out = String::new();
            if paths.is_empty() {
                out.push_str(input);
            }
            for path in &paths {
                out.push_str(&crate::shell::read_text(c, &cmd, path, input)?);
            }
            let plain = !"nbETAsv".chars().any(|f| flag(&opts, f));
            if plain {
                return Ok(out);
            }
            let (ends, tabs) = (
                flag(&opts, 'E') || flag(&opts, 'A'),
                flag(&opts, 'T') || flag(&opts, 'A'),
            );
            let mut rendered = String::new();
            let mut n = 0;
            let mut blank_run = 0;
            let had_newline = out.ends_with('\n') || out.is_empty();
            for line in out.strip_suffix('\n').unwrap_or(&out).split('\n') {
                if out.is_empty() {
                    break;
                }
                blank_run = if line.is_empty() { blank_run + 1 } else { 0 };
                if flag(&opts, 's') && blank_run > 1 {
                    continue;
                }
                let body = if tabs {
                    line.replace('\t', "^I")
                } else {
                    line.to_string()
                };
                let numbered = flag(&opts, 'n') || (flag(&opts, 'b') && !line.is_empty());
                if numbered {
                    n += 1;
                    rendered.push_str(&format!("{n:>6}\t"));
                } else if flag(&opts, 'b') {
                    rendered.push_str("      \t");
                }
                rendered.push_str(&body);
                if ends {
                    rendered.push('$');
                }
                rendered.push('\n');
            }
            if !had_newline {
                rendered.pop();
            }
            Ok(rendered)
        }
        "touch" => cmd_touch(c, args, t),
        "mkdir" | "md" => {
            let (opts, paths) = options(
                "mkdir",
                args,
                "pv",
                "",
                &[("parents", 'p'), ("verbose", 'v')],
            )?;
            if paths.is_empty() {
                return Err(Fail::usage(format!(
                    "mkdir: missing operand\n{}",
                    usage_line("mkdir")
                )));
            }
            let mut told = String::new();
            for p in &paths {
                let path = c.resolve(p);
                // Without -p an existing directory or a missing parent is an error.
                if !flag(&opts, 'p') {
                    if c.vfs.exists(&path) {
                        return Err(format!("cannot create directory '{p}': File exists").into());
                    }
                    let parent = path.rsplit_once('/').map_or("/", |(a, _)| a);
                    let parent = if parent.is_empty() { "/" } else { parent };
                    if !c.vfs.exists(parent) {
                        return Err(format!(
                            "cannot create directory '{p}': No such file or directory"
                        )
                        .into());
                    }
                }
                c.vfs.mkdir_all_as(&path, &c.user, t).map_err(err)?;
                if flag(&opts, 'v') {
                    told.push_str(&format!("mkdir: created directory '{p}'\n"));
                }
            }
            Ok(told)
        }
        "set-content" | "add-content" => {
            let path = c.resolve(required(0)?);
            let value = format!("{}\n", args[1..].join(" "));
            if cmd == "add-content" {
                c.vfs
                    .append(&path, value.as_bytes(), &c.user, t)
                    .map_err(err)?;
            } else {
                c.vfs
                    .write_as(&path, value.as_bytes(), &c.user, t)
                    .map_err(err)?;
            }
            Ok(String::new())
        }
        "cp" | "copy-item" => {
            let (opts, paths) = options(
                "cp",
                &powershell_switches(args),
                "rRfpv",
                "",
                &[("recursive", 'r'), ("force", 'f')],
            )?;
            let [source, destination] = paths.as_slice() else {
                return Err(Fail::usage(
                    "cp: expects exactly one source and one destination",
                ));
            };
            let from = c.resolve(source);
            let mut to = c.resolve(destination);
            if c.vfs.list_as(&to, &c.user).is_ok() {
                to = format!("{to}/{}", from.rsplit('/').next().unwrap_or("file"));
            }
            if c.vfs
                .lstat(&from)
                .map_err(|e| Fail::io("cp", source, &e))?
                .is_dir
            {
                if !(flag(&opts, 'r') || flag(&opts, 'R')) {
                    return Err(format!("-r not specified; omitting directory '{source}'").into());
                }
                for (path, _, directory) in walk_tree(c, &from) {
                    let target = format!("{to}{}", &path[from.len()..]);
                    if directory {
                        c.vfs.mkdir_all_as(&target, &c.user, t).map_err(err)?;
                    } else {
                        let bytes = c.vfs.read_as(&path, &c.user).map_err(err)?;
                        c.vfs.write_as(&target, &bytes, &c.user, t).map_err(err)?;
                    }
                }
                return Ok(String::new());
            }
            let bytes = c
                .vfs
                .read_as(&from, &c.user)
                .map_err(|e| Fail::io("cp", source, &e))?;
            c.vfs
                .write_as(&to, &bytes, &c.user, t)
                .map_err(|e| Fail::io("cp", destination, &e))?;
            Ok(String::new())
        }
        "mv" | "move-item" => {
            // `-f` and `-v` are the only flags with a meaning here; -f is already the
            // behaviour (no prompting is possible) and -v prints what moved.
            let (opts, names) = options(
                "mv",
                &powershell_switches(args),
                "fv",
                "",
                &[("force", 'f'), ("verbose", 'v')],
            )?;
            let [source, destination] = names.as_slice() else {
                return Err(Fail::usage(format!(
                    "mv: expects exactly one source and one destination\n{}",
                    usage_line("mv")
                )));
            };
            // Moving onto a directory moves the name into it, as mv does.
            let mut to = c.resolve(destination);
            if c.vfs.stat(&to).is_ok_and(|m| m.is_dir) {
                let base = source
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or("");
                to = format!("{}/{base}", to.trim_end_matches('/'));
            }
            c.vfs
                .rename_as(&c.resolve(source), &to, &c.user)
                .map_err(|e| Fail::io("mv", source, &e))?;
            Ok(if flag(&opts, 'v') {
                format!("renamed '{source}' -> '{destination}'\n")
            } else {
                String::new()
            })
        }
        "rm" | "remove-item" | "rmdir" => {
            let (opts, paths) = options(
                "rm",
                &powershell_switches(args),
                "rRfv",
                "",
                &[("recursive", 'r'), ("force", 'f')],
            )?;
            let recursive = flag(&opts, 'r') || flag(&opts, 'R') || cmd == "rmdir";
            let force = flag(&opts, 'f');
            if paths.is_empty() && !force {
                return Err(Fail::usage("rm: missing operand"));
            }
            for p in &paths {
                if let Err(e) = c.vfs.remove_as(&c.resolve(p), recursive, &c.user) {
                    if !force {
                        // GNU names the directory itself, not its contents, when -r
                        // is missing: `rm: cannot remove 'x': Is a directory`.
                        let reason = match &e {
                            crate::VfsError::NotEmpty(_) if !recursive => "Is a directory",
                            other => vfs_reason(other),
                        };
                        return Err(Fail::op("rm", format!("cannot remove '{p}'"), reason));
                    }
                }
            }
            Ok(String::new())
        }
        "chmod" => cmd_chmod(c, args),
        "ln" => {
            let (opts, names) = options("ln", args, "s", "", &[("symbolic", 's')])?;
            let [target, link] = names.as_slice() else {
                return Err(Fail::usage(format!(
                    "ln: expects exactly one target and one link name\n{}",
                    usage_line("ln")
                )));
            };
            if flag(&opts, 's') {
                c.vfs
                    .symlink_as(target, &c.resolve(link), &c.user, t)
                    .map_err(|e| Fail::io("ln", link, &e))?;
            } else {
                c.vfs
                    .hard_link_as(&c.resolve(target), &c.resolve(link), &c.user)
                    .map_err(|e| Fail::io("ln", target, &e))?;
            }
            Ok(String::new())
        }
        "grep" | "select-string" => cmd_grep(c, args, input),
        "test" | "[" | "test-path" => {
            let vals = if cmd == "[" {
                if args.last().map(String::as_str) != Some("]") {
                    return Err("missing ]".into());
                }
                &args[..args.len() - 1]
            } else {
                args
            };
            let yes = match vals {
                [v] if cmd == "test-path" => c.vfs.stat(&c.resolve(v)).is_ok(),
                _ => test_primary(c, vals)?,
            };
            if cmd == "test-path" {
                Ok(format!("{}\n", if yes { "True" } else { "False" }))
            } else if yes {
                Ok(String::new())
            } else {
                Err(Fail::new(String::new(), 1))
            }
        }
        "shift" => {
            if args.len() > 1 {
                return Err(Fail::usage("shift: too many arguments"));
            }
            let by: usize = match args.first() {
                None => 1,
                Some(v) => v
                    .parse()
                    .map_err(|_| Fail::usage("shift: numeric argument required"))?,
            };
            let count: usize = c.env.get("#").and_then(|v| v.parse().ok()).unwrap_or(0);
            // Shifting past the end is a modelled negative, not an unsupported ask.
            if by > count {
                return Err(Fail::new(String::new(), 1));
            }
            for i in 1..=count - by {
                let value = c
                    .env
                    .get(&(i + by).to_string())
                    .cloned()
                    .unwrap_or_default();
                c.env.insert(i.to_string(), value);
            }
            for i in count - by + 1..=count {
                c.env.remove(&i.to_string());
            }
            c.env.insert("#".into(), (count - by).to_string());
            Ok(String::new())
        }
        "stat" => cmd_stat(c, args),
        "sed" => crate::sed::execute(c, args, input, t),
        "awk" | "gawk" | "mawk" | "nawk" => crate::awk::execute(c, args, input, t, host, depth),
        "find" => cmd_find(c, args),
        "clear" | "cls" => {
            // No bytes: run_tokens raises CommandResult::clear for the terminal to honour.
            if let Some(first) = args.first() {
                return Err(Fail::usage(format!(
                    "clear: unrecognized operand '{first}'; clear takes no arguments"
                )));
            }
            Ok(String::new())
        }
        "nproc" => {
            let (_, rest) = options("nproc", args, "", "", &[("all", 'a')])?;
            if !rest.is_empty() {
                return Err(Fail::usage("nproc: takes no operands"));
            }
            Ok(format!("{}\n", c.hardware.cpus))
        }
        "uptime" => cmd_uptime(c, args, t),
        "which" => cmd_which(c, args),
        "du" => cmd_du(c, args),
        "df" => cmd_df(c, args),
        "ip" => cmd_ip(c, args),
        "sudo" => {
            let mut index = 0;
            let mut target = String::from("root");
            while index < args.len() {
                match args[index].as_str() {
                    "-n" | "--non-interactive" | "-E" | "--preserve-env" => index += 1,
                    "-v" | "--validate" | "-k" | "--reset-timestamp" | "-K" => {
                        return Ok(String::new())
                    }
                    "-u" | "--user" => {
                        target = args
                            .get(index + 1)
                            .cloned()
                            .ok_or_else(|| Fail::usage("sudo: option `-u` requires a user"))?;
                        index += 2;
                    }
                    "--" => {
                        index += 1;
                        break;
                    }
                    other if other.starts_with('-') => {
                        return Err(Fail::usage(format!("sudo: unsupported option `{other}`")))
                    }
                    _ => break,
                }
            }
            if index >= args.len() {
                return Err(Fail::usage("sudo: missing command"));
            }
            if depth > 32 {
                return Err("sudo: execution nesting exceeds 32".into());
            }
            // No password or sudoers model: sudo only swaps the identity access checks use.
            let saved = std::mem::replace(&mut c.user, target);
            let result = run(c, &args[index..], input, t, host, depth + 1);
            c.user = saved;
            result
        }
        "systemctl" | "service" => {
            // No flag of systemctl's is modelled (`--user`, `--now`, `--no-pager` all
            // imply machinery this world does not have), so every one is refused.
            let (_, words) = options(&cmd, args, "", "", &[])?;
            let pick = |i: usize| {
                words
                    .get(i)
                    .map(String::as_str)
                    .ok_or_else(|| Fail::usage(format!("{cmd}: missing operand")))
            };
            let (operation, name) = if cmd == "service" {
                (pick(1)?, pick(0)?)
            } else {
                (pick(0)?, pick(1)?)
            };
            let name = name.trim_end_matches(".service");
            let command = format!("service {name}");
            let running = c.processes.list().into_iter().find(|p| {
                p.command == command
                    && !matches!(
                        p.state,
                        crate::ProcessState::Zombie { .. } | crate::ProcessState::Exited { .. }
                    )
            });
            if operation == "status" {
                return if let Some(p) = running {
                    Ok(format!("{name}: active (running), pid {}\n", p.pid))
                } else {
                    Err(format!("{name}: inactive").into())
                };
            }
            if operation == "stop" || operation == "restart" {
                if let Some(p) = running.as_ref() {
                    c.processes.signal(p.pid, "TERM", &c.user, t)?;
                    if process_terminated(c, p.pid) {
                        host.cleanup_process(p.pid)
                    } else {
                        return Err(format!("{name}: process did not terminate after TERM").into());
                    }
                }
                if operation == "stop" {
                    return Ok(format!("Stopped {name}\n"));
                }
            }
            if operation == "start" || operation == "restart" {
                if running.is_some() && operation == "start" {
                    return Ok(format!("{name}: already running\n"));
                }
                let pid = c.processes.spawn(1, &c.user, &command, t);
                match host.start_service(name, pid) {
                    Ok(listener) => {
                        c.processes.own_listener(pid, listener)?;
                        return Ok(format!("Started {name}\n"));
                    }
                    Err(e) => {
                        let _ = c.processes.exit(pid, 1, t);
                        host.cleanup_process(pid);
                        return Err(e.into());
                    }
                }
            }
            Err(Fail::usage("unsupported service operation"))
        }
        "ps" | "get-process" => cmd_ps(c, args),
        "kill" | "stop-process" => {
            let (signal, index) = if required(0)?.starts_with('-') {
                let spelling = required(0)?;
                // `kill -l`, `kill -s NAME` and a long option are not modelled; only
                // `-SIGNAL` and `-N` are, so anything else is refused by name.
                let name = spelling.trim_start_matches('-').trim_start_matches("SIG");
                if spelling.starts_with("--") {
                    return Err(unrecognized_option(&cmd, spelling.trim_start_matches('-')));
                }
                // The roster the process table actually delivers; anything else is a
                // named refusal rather than a signal that quietly does nothing.
                const SIGNALS: &[&str] = &[
                    "TERM", "KILL", "INT", "STOP", "TSTP", "CONT", "HUP", "QUIT", "USR1", "USR2",
                    "0", "1", "2", "3", "9", "10", "12", "15", "18", "19",
                ];
                if !SIGNALS.contains(&name) {
                    return Err(Fail::usage(format!(
                        "{cmd}: {spelling}: invalid signal specification; this world delivers {}",
                        SIGNALS.join(" ")
                    )));
                }
                (name, 1)
            } else {
                ("TERM", 0)
            };
            let pid = required(index)?.parse().map_err(|_| "invalid pid")?;
            c.processes.signal(pid, signal, &c.user, t)?;
            if process_terminated(c, pid) {
                host.cleanup_process(pid);
            }
            Ok(String::new())
        }
        "sleep" => Err(
            "sleep must be submitted as a standalone action; advance simulated time to resume"
                .into(),
        ),
        "apt" | "apt-get" | "brew" | "winget" | "pip" | "npm" => {
            let action = required(0)?;
            if action == "list" {
                return Ok(c
                    .packages
                    .installed
                    .iter()
                    .map(|(k, v)| format!("{k} {v}\n"))
                    .collect());
            }
            let name = required(1)?;
            let result = if action == "install" {
                c.packages.install(name, &mut c.vfs, &c.user, t)
            } else if action == "remove" || action == "uninstall" {
                c.packages
                    .remove(name, &mut c.vfs)
                    .map(|_| vec![format!("Removed {name}")])
            } else {
                return Err("unsupported package operation".into());
            };
            Ok(result.map(|v| v.join("\n") + "\n")?)
        }
        "curl" | "wget" | "invoke-webrequest" => {
            let mut method = "GET".to_string();
            let mut body = vec![];
            let mut headers = BTreeMap::new();
            let mut url = None;
            let mut output = None;
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "-X" | "--request" => {
                        i += 1;
                        method = args.get(i).ok_or("missing method")?.clone()
                    }
                    "-d" | "--data" | "--data-raw" => {
                        i += 1;
                        body = args.get(i).ok_or("missing data")?.as_bytes().to_vec();
                        if method == "GET" {
                            method = "POST".into();
                        }
                    }
                    "-H" | "--header" => {
                        i += 1;
                        let (k, v) = args
                            .get(i)
                            .ok_or("missing header")?
                            .split_once(':')
                            .ok_or("invalid header")?;
                        headers.insert(k.trim().to_lowercase(), v.trim().into());
                    }
                    "-o" | "--output" => {
                        i += 1;
                        output = Some(args.get(i).ok_or("missing output path")?.clone())
                    }
                    // Accepted and inert: this adapter never writes a progress meter,
                    // never follows a redirect on its own and has no TTY to be quiet on.
                    "-f" | "--fail" | "-s" | "--silent" | "-S" | "--show-error" => {}
                    s if !s.starts_with('-') || s == "-" => url = Some(s.to_string()),
                    s if s.starts_with("--") => {
                        return Err(unrecognized_option(&cmd, s.trim_start_matches('-')))
                    }
                    s => {
                        let letter = s.chars().nth(1).unwrap_or('?');
                        return Err(invalid_option(&cmd, letter));
                    }
                }
                i += 1;
            }
            let response = host.http(cw_protocol::HttpRequest {
                method,
                url: url.ok_or("missing URL")?,
                headers,
                body,
            })?;
            if args.iter().any(|s| s == "-f" || s == "--fail") && response.status >= 400 {
                return Err(format!("HTTP {}", response.status).into());
            }
            if let Some(p) = output {
                c.vfs
                    .write_as(&c.resolve(&p), &response.body, &c.user, t)
                    .map_err(err)?;
                Ok(String::new())
            } else {
                Ok(String::from_utf8_lossy(&response.body).into_owned())
            }
        }
        "git" => {
            let result = crate::git::execute(c, args, t, host);
            if result.exit_code == 0 {
                Ok(result.stdout)
            } else {
                Err(Fail::new(result.stderr, result.exit_code))
            }
        }
        "sh" | "bash" => {
            let inline = args.first().is_some_and(|s| s == "-c");
            let source = if inline {
                required(1)?.to_string()
            } else {
                String::from_utf8(
                    c.vfs
                        .read_as(&c.resolve(required(0)?), &c.user)
                        .map_err(err)?,
                )
                .map_err(|_| "script is not UTF-8")?
            };
            let saved_env = c.env.clone();
            let saved_cwd = c.cwd.clone();
            // `sh -c SCRIPT NAME ARG…`: the operands after the script are $0, $1, …
            // For a script file the path itself is $0, so nothing is skipped.
            let operands = &args[if inline { 2.min(args.len()) } else { 0 }..];
            if !operands.is_empty() {
                for (i, arg) in operands.iter().enumerate() {
                    c.env.insert(i.to_string(), arg.clone());
                }
                c.env
                    .insert("#".into(), operands.len().saturating_sub(1).to_string());
            }
            let r = execute_inner(c, &source, t, host, depth + 1);
            c.env = saved_env;
            c.cwd = saved_cwd;
            if r.exit_code == 0 && r.stderr.is_empty() {
                Ok(r.stdout)
            } else {
                Err(Fail::nested(r))
            }
        }
        _ => {
            let candidates = if a[0].contains('/') || a[0].contains('\\') {
                vec![c.resolve(&a[0])]
            } else {
                c.env
                    .get("PATH")
                    .map(String::as_str)
                    .unwrap_or("/bin:/usr/bin")
                    .split(if c.dialect == "powershell" { ';' } else { ':' })
                    .map(|dir| c.resolve(&format!("{dir}/{}", a[0])))
                    .collect()
            };
            let path = candidates
                .into_iter()
                .find(|p| c.vfs.exists(p))
                .ok_or_else(|| Fail::new(format!("command not found: {}", a[0]), 127))?;
            c.vfs
                .check_access(&path, &c.user, true, false, true)
                .map_err(|e| Fail::new(e.to_string(), 126))?;
            let source = String::from_utf8(c.vfs.read_as(&path, &c.user).map_err(err)?)
                .map_err(|_| Fail::new("unsupported binary executable", 126))?;
            // `#!/usr/bin/env python3` and friends run under the named runtime,
            // which receives the script path as the shell resolved it.
            if let Some(runtime) = crate::runtimes::shebang_runtime(&source) {
                let mut script_args = vec![if a[0].contains('/') {
                    a[0].clone()
                } else {
                    path.clone()
                }];
                script_args.extend(args.iter().cloned());
                let r = crate::runtimes::run_runtime(runtime, c, host, &script_args, input, t);
                return runtime_result(r);
            }
            if source.starts_with("#!cw-package\n") {
                let name = source.lines().nth(1).ok_or("invalid package executable")?;
                if args.iter().any(|s| s == "--version" || s == "-V") {
                    return Ok(format!(
                        "{name} {}\n",
                        c.packages
                            .installed
                            .get(name)
                            .ok_or("package is not installed")?
                    ));
                }
                return Err(Fail::new(
                    format!("{name}: no synthetic program implementation registered"),
                    126,
                ));
            }
            if !source.starts_with("#!/bin/sh")
                && !source.starts_with("#!/bin/bash")
                && !source.starts_with("#!/usr/bin/env sh")
            {
                return Err(Fail::new("unsupported executable interpreter", 126));
            }
            let old = c.env.clone();
            let old_cwd = c.cwd.clone();
            for (i, arg) in a.iter().enumerate() {
                c.env.insert(i.to_string(), arg.clone());
            }
            c.env.insert("#".into(), args.len().to_string());
            let r = execute_inner(c, &source, t, host, depth + 1);
            c.env = old;
            c.cwd = old_cwd;
            if r.exit_code == 0 && r.stderr.is_empty() {
                Ok(r.stdout)
            } else {
                Err(Fail::nested(r))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::OfflineHost;
    fn computer() -> Computer {
        Computer::new("a", "user", "linux", true)
    }
    #[test]
    fn quoting_pipes_and_redirect() {
        let mut c = computer();
        let r = execute(
            &mut c,
            "echo 'a; b > c' > out; cat out | grep 'b >'",
            1,
            &mut OfflineHost,
        );
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "a; b > c\n");
    }
    #[test]
    fn short_circuit_and_variables() {
        let mut c = computer();
        let r = execute(
            &mut c,
            "X=hello; false && echo bad; echo \"$X\"; true || echo bad",
            1,
            &mut OfflineHost,
        );
        assert_eq!(r.stdout, "hello\n");
    }
    #[test]
    fn git_history_is_content_addressed() {
        let mut c = computer();
        let r=execute(&mut c,"git init; echo one > a; git add a; git commit -m first; git branch other; echo two > a; git add a; git commit -m second; git checkout other; cat a",1,&mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert!(r.stdout.ends_with("one\n"));
    }
}

/// A runtime's streams and status pass through untouched: its diagnostics are
/// its own (a Python traceback, a Node stack), never prefixed by the shell.
fn runtime_result(r: CommandResult) -> Result<String, Fail> {
    if r.exit_code == 0 && r.stderr.is_empty() {
        Ok(r.stdout)
    } else {
        Err(Fail::nested(r))
    }
}

fn append(c: &mut Computer, path: &str, data: &[u8], t: u64) -> Result<(), crate::VfsError> {
    if c.vfs.exists(path) {
        c.vfs.check_access(path, &c.user, false, true, false)?;
        c.vfs.append(path, data, &c.user, t)
    } else {
        c.vfs.write_as(path, data, &c.user, t)
    }
}
fn parse_duration(value: &str) -> Result<u64, String> {
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 6 || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err("duration must have at most six decimal places".into());
    }
    let seconds = seconds.parse::<u64>().map_err(|_| "invalid duration")?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().map_err(|_| "invalid fraction")?
            * 10u64.pow(6 - fraction.len() as u32)
    };
    seconds
        .checked_mul(1_000_000)
        .and_then(|n| n.checked_add(fraction))
        .ok_or("duration overflow".into())
}
#[cfg(test)]
mod advanced_tests {
    use super::*;
    use crate::OfflineHost;
    #[test]
    fn expansion_globs_arithmetic_and_regex() {
        let mut c = Computer::new("pc", "u", "linux", true);
        let r=execute(&mut c,"echo alpha > one.txt; echo beta > two.txt; echo '*.txt'; echo *.txt; echo $((2 + 3 * 4)); echo \"$(cat one.txt)\" | grep '^al.*a$'",5,&mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "*.txt\none.txt two.txt\n14\nalpha\n");
    }
    #[test]
    fn sleep_uses_simulated_deadline() {
        let mut c = Computer::new("pc", "u", "linux", true);
        let r = execute(&mut c, "sleep 0.25 &", 100, &mut OfflineHost);
        assert_eq!(r.exit_code, 0);
        assert!(matches!(
            c.processes.get(r.pid).unwrap().state,
            crate::ProcessState::Sleeping { until: 250100 }
        ));
        c.processes.advance(250100);
        assert!(matches!(
            c.processes.get(r.pid).unwrap().state,
            crate::ProcessState::Exited { code: 0 }
        ));
    }
    #[test]
    fn ignores_do_not_hide_tracked_files() {
        let mut c = Computer::new("pc", "u", "linux", true);
        let r=execute(&mut c,"mkdir repo; git -C repo init; cd repo; echo '*.tmp' > .gitignore; echo hidden > scratch.tmp; echo tracked > file; git add .; git commit -m initial; git status",2,&mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert!(!r.stdout.contains("scratch.tmp"));
    }
    #[test]
    fn actor_cannot_read_private_file() {
        let mut c = Computer::new("pc", "u", "linux", true);
        c.vfs.write("/tmp/private", b"secret", "other", 0).unwrap();
        c.vfs.chmod("/tmp/private", 0o600).unwrap();
        let r = execute(&mut c, "cat /tmp/private", 2, &mut OfflineHost);
        assert_eq!(r.exit_code, 1);
        assert!(!r.stdout.contains("secret"));
    }
}
#[cfg(test)]
mod grammar_tests {
    use super::*;
    use crate::OfflineHost;
    #[test]
    fn heredocs_and_combined_redirect_are_lexical() {
        let mut c = Computer::new("pc", "u", "linux", true);
        let result=execute(&mut c,"X=value; cat <<'END' > one\n$X; | > literal\nEND\ncat <<END > two\n$X\nEND\ncat one two; missing &> error; cat error",1,&mut OfflineHost);
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(
            result.stdout,
            "$X; | > literal\nvalue\nmissing: command not found: missing\n"
        );
    }
    #[test]
    fn malformed_pipeline_reports_error() {
        let mut c = Computer::new("pc", "u", "linux", true);
        assert_ne!(
            execute(&mut c, "echo yes &&", 1, &mut OfflineHost).exit_code,
            0
        );
    }
    #[test]
    fn blocking_sleep_advances_adapter() {
        struct Host(u64);
        impl ShellHost for Host {
            fn http(
                &mut self,
                _: cw_protocol::HttpRequest,
            ) -> Result<cw_protocol::HttpResponse, String> {
                Err("offline".into())
            }
            fn advance(&mut self, t: u64) -> Result<u64, String> {
                self.0 += t;
                Ok(self.0)
            }
        }
        let mut c = Computer::new("pc", "u", "linux", true);
        let mut host = Host(20);
        let r = execute(&mut c, "sleep 0.2", 20, &mut host);
        assert_eq!(r.exit_code, 0);
        assert_eq!(host.0, 200020);
        assert!(matches!(
            c.processes.get(r.pid).unwrap().state,
            crate::ProcessState::Exited { code: 0 }
        ));
    }
}
#[cfg(test)]
mod executable_tests {
    use super::*;
    use crate::{OfflineHost, Package};
    #[test]
    fn package_script_runs_same_shell() {
        let mut c = Computer::new("pc", "u", "linux", true);
        c.packages.register(Package {
            name: "hello".into(),
            version: "1.0.0".into(),
            dependencies: BTreeMap::new(),
            files: BTreeMap::from([(
                "/usr/bin/hello".into(),
                b"#!/bin/sh\necho hello $1".to_vec(),
            )]),
            executables: vec![],
        });
        c.packages.install("hello", &mut c.vfs, "u", 0).unwrap();
        c.vfs.chmod("/usr/bin/hello", 0o755).unwrap();
        let r = execute(&mut c, "hello world", 1, &mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "hello world\n");
    }
    #[test]
    fn recursive_script_is_bounded() {
        let mut c = Computer::new("pc", "u", "linux", true);
        c.vfs
            .write("/home/u/loop", b"#!/bin/sh\n./loop", "u", 0)
            .unwrap();
        c.vfs.chmod("/home/u/loop", 0o755).unwrap();
        let r = execute(&mut c, "./loop", 1, &mut OfflineHost);
        assert_ne!(r.exit_code, 0);
        assert!(r.stderr.contains("nesting exceeds"));
    }
    #[test]
    fn dialect_separates_commands_and_path_escapes() {
        let mut c = Computer::new("pc", "u", "linux", true);
        assert_ne!(
            execute(&mut c, "Write-Output yes", 1, &mut OfflineHost).exit_code,
            0
        );
        c.dialect = "powershell".into();
        assert_eq!(
            execute(&mut c, "Write-Output C:\\Users\\agent", 1, &mut OfflineHost).stdout,
            "C:\\Users\\agent\n"
        );
    }
}

#[cfg(test)]
mod final_semantic_tests {
    use super::*;
    use crate::OfflineHost;
    #[test]
    fn substitutions_preserve_local_scope_and_file_effects() {
        let mut c = Computer::new("pc", "u", "linux", true);
        let r=execute(&mut c,"X=outer; echo \"$(cd /tmp; X=inner; echo data > /tmp/proof; pwd)\"; pwd; echo $X; cat /tmp/proof",0,&mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "/tmp\n/home/u\nouter\ndata\n");
    }
    #[test]
    fn editing_pipeline_and_condition() {
        let mut c = Computer::new("pc", "u", "linux", true);
        let r=execute(&mut c,"echo 'Ada,review' > row; sed -i 's/review/approved/' row; cat row | cut -d , -f 2 | tr a-z A-Z; test -f row && echo exists; echo `cat row`",0,&mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(r.stdout, "APPROVED\nexists\nAda,approved\n");
    }
}

fn process_terminated(c: &Computer, pid: u64) -> bool {
    c.processes.get(pid).is_none_or(|p| {
        matches!(
            p.state,
            crate::ProcessState::Exited { .. } | crate::ProcessState::Zombie { .. }
        )
    })
}
#[cfg(test)]
mod signal_tests {
    use super::*;
    #[test]
    fn ignored_signal_keeps_listener() {
        struct Host(Vec<u64>);
        impl ShellHost for Host {
            fn http(
                &mut self,
                _: cw_protocol::HttpRequest,
            ) -> Result<cw_protocol::HttpResponse, String> {
                Err("offline".into())
            }
            fn cleanup_process(&mut self, pid: u64) {
                self.0.push(pid)
            }
        }
        let mut c = Computer::new("pc", "u", "linux", true);
        let pid = c.processes.spawn(1, "u", "service api", 0);
        c.processes
            .set_signal_disposition(pid, "TERM", crate::SignalDisposition::Ignore)
            .unwrap();
        let mut host = Host(vec![]);
        execute(&mut c, &format!("kill {pid}"), 1, &mut host);
        assert!(!host.0.contains(&pid));
        execute(&mut c, &format!("kill -9 {pid}"), 2, &mut host);
        assert!(host.0.contains(&pid));
    }
}

#[cfg(test)]
mod find_tests {
    use super::*;
    use crate::OfflineHost;
    fn machine() -> Computer {
        let mut c = Computer::new("a", "user", "linux", true);
        for dir in ["/home/user/notes", "/home/user/notes/deep"] {
            let r = execute(&mut c, &format!("mkdir -p {dir}"), 0, &mut OfflineHost);
            assert_eq!(r.exit_code, 0, "{}", r.stderr);
        }
        for path in [
            "/home/user/recipes.txt",
            "/home/user/notes/recipes.md",
            "/home/user/notes/deep/other.txt",
        ] {
            let r = execute(&mut c, &format!("echo x > {path}"), 0, &mut OfflineHost);
            assert_eq!(r.exit_code, 0, "{}", r.stderr);
        }
        c
    }
    fn run(c: &mut Computer, line: &str) -> (String, i32) {
        let r = execute(c, line, 1, &mut OfflineHost);
        (format!("{}{}", r.stdout, r.stderr), r.exit_code)
    }
    #[test]
    fn a_name_predicate_is_applied_and_a_miss_returns_nothing() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user -name 'nope'");
        assert_eq!(out, "", "a search with no matches must return no matches");
        assert_eq!(code, 0);
        let (out, _) = run(&mut c, "find /home/user -name 'recipes.txt'");
        assert_eq!(out, "/home/user/recipes.txt\n");
        let (out, _) = run(&mut c, "find /home/user -name 'recipes.*'");
        let mut lines: Vec<_> = out.lines().collect();
        lines.sort();
        assert_eq!(
            lines,
            vec!["/home/user/notes/recipes.md", "/home/user/recipes.txt"]
        );
    }
    #[test]
    fn type_and_maxdepth_narrow_the_walk() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user -type d");
        let mut dirs: Vec<_> = out.lines().collect();
        dirs.sort();
        assert!(dirs.contains(&"/home/user/notes"), "{dirs:?}");
        assert!(dirs.contains(&"/home/user/notes/deep"), "{dirs:?}");
        assert!(!dirs.contains(&"/home/user/recipes.txt"), "{dirs:?}");
        let (out, _) = run(&mut c, "find /home/user -maxdepth 1 -type f");
        assert_eq!(out, "/home/user/recipes.txt\n");
    }
    #[test]
    fn an_unsupported_predicate_fails_loudly_rather_than_being_ignored() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user -newer /etc/passwd");
        assert_ne!(code, 0, "an ignored predicate must not report success");
        assert!(out.contains("-newer"), "{out}");
        let (out, code) = run(&mut c, "find /home/user -name");
        assert_ne!(code, 0);
        assert!(out.contains("missing argument"), "{out}");
        let (_, code) = run(&mut c, "find /no/such/root -name x");
        assert_ne!(code, 0);
    }
}

/// Simulated wall clock. Tick 0 is 09:00:00 UTC on Thursday 17 September 2026 — the
/// same origin the desktop clock uses, so `date` and the GUI never disagree.
pub const EPOCH_UNIX_SECONDS: u64 = 1_789_635_600;
const WEEKDAY_ABBR: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAY_NAME: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_NAME: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub year: u64,
    pub month: u64,
    pub day: u64,
    pub hour: u64,
    pub minute: u64,
    pub second: u64,
    pub micro: u64,
    /// 0 is Sunday.
    pub weekday: u64,
    pub unix: u64,
}
/// Gregorian civil date from a simulated microsecond tick; no host clock is consulted.
pub fn clock(tick: u64) -> Clock {
    let unix = EPOCH_UNIX_SECONDS + tick / 1_000_000;
    let (days, rest) = (unix / 86_400, unix % 86_400);
    // Hinnant's civil-from-days: a March-based year puts the leap day last.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Clock {
        year: (yoe + era * 400 + i64::from(month <= 2)) as u64,
        month: month as u64,
        day: day as u64,
        hour: rest / 3600,
        minute: rest % 3600 / 60,
        second: rest % 60,
        micro: tick % 1_000_000,
        weekday: (days + 4) % 7,
        unix,
    }
}
impl Clock {
    /// `2026-09-17 09:00:00`, the stamp `stat` and `uptime -s` print.
    fn stamp(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
    /// `Sep 17 09:00`, the `ls -l` column.
    fn short(&self) -> String {
        format!(
            "{} {:2} {:02}:{:02}",
            MONTH_ABBR[self.month as usize - 1],
            self.day,
            self.hour,
            self.minute
        )
    }
}
/// GNU-style abbreviated size: exact under 1 KiB, one decimal under 10 units.
fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["", "K", "M", "G", "T", "P"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        bytes.to_string()
    } else if value < 9.95 {
        format!("{value:.1}{}", UNITS[unit])
    } else {
        format!("{}{}", value.ceil() as u64, UNITS[unit])
    }
}
/// `drwxr-xr-t` from the stored mode; setuid/setgid/sticky bits are honoured.
fn mode_string(mode: u16, is_dir: bool, is_symlink: bool) -> String {
    let kind = if is_symlink {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let mut out = String::from(kind);
    for (shift, special, low, high) in [
        (6, 0o4000, 's', 'S'),
        (3, 0o2000, 's', 'S'),
        (0, 0o1000, 't', 'T'),
    ] {
        let bits = (mode >> shift) & 7;
        out.push(if bits & 4 != 0 { 'r' } else { '-' });
        out.push(if bits & 2 != 0 { 'w' } else { '-' });
        out.push(if mode & special != 0 {
            if bits & 1 != 0 {
                low
            } else {
                high
            }
        } else if bits & 1 != 0 {
            'x'
        } else {
            '-'
        });
    }
    out
}
/// Bytes a node occupies, assuming a 4 KiB allocation unit; the VFS has no allocator,
/// so this is a stated convention rather than a measurement.
const BLOCK: u64 = 4096;
fn allocated(size: u64) -> u64 {
    size.div_ceil(BLOCK) * BLOCK
}
/// Bytes a node reports: the VFS stores a child count for directories, but every tool
/// that prints a size means the allocation unit.
fn apparent(m: &crate::Metadata) -> u64 {
    if m.is_dir {
        BLOCK
    } else {
        m.size as u64
    }
}
/// Every node beneath `root` (inclusive) with its apparent size; directories are
/// reported as one allocation unit. Walks the VFS only.
fn walk_tree(c: &Computer, root: &str) -> Vec<(String, u64, bool)> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut stack = vec![root.trim_end_matches('/').to_string()];
    while let Some(path) = stack.pop() {
        let path = if path.is_empty() {
            "/".to_string()
        } else {
            path
        };
        let Ok(meta) = c.vfs.lstat(&path) else {
            continue;
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        if meta.is_dir {
            out.push((path.clone(), BLOCK, true));
            for name in c.vfs.list(&path).unwrap_or_default() {
                stack.push(format!("{}/{name}", path.trim_end_matches('/')));
            }
        } else {
            out.push((path, meta.size as u64, false));
        }
    }
    out.sort();
    out
}

/// Option parser shared by the file utilities. Clusters (`-la`) split, long names map
/// to their short letter, and anything unlisted is a usage failure: a flag this world
/// cannot honour must never be silently dropped.
pub(crate) type Parsed = (Vec<(char, String)>, Vec<String>);
/// The one-line synopsis printed under a refused flag, exactly as GNU coreutils does.
/// A command with no entry gets the generic form; the point is that the caller always
/// sees what the command *does* accept next to what it refused.
pub(crate) fn usage_line(name: &str) -> String {
    let body = match name {
        "awk" => "awk [-F fs] [-v var=value] ['prog' | -f progfile] [file ...]",
        "sed" => "sed [-n] [-E] [-i[SUFFIX]] [-e script] [-f script-file] [file ...]",
        "xargs" => "xargs [-0rt] [-d delim] [-I replace-str] [-n max-args] [-P max-procs] [command [args...]]",
        "cut" => "cut OPTION... [FILE]...",
        "sort" => "sort [OPTION]... [FILE]...",
        "uniq" => "uniq [OPTION]... [INPUT [OUTPUT]]",
        "head" | "tail" => "head [OPTION]... [FILE]...",
        "wc" => "wc [OPTION]... [FILE]...",
        "tr" => "tr [OPTION]... SET1 [SET2]",
        "paste" => "paste [OPTION]... [FILE]...",
        "join" => "join [OPTION]... FILE1 FILE2",
        "comm" => "comm [OPTION]... FILE1 FILE2",
        "diff" => "diff [OPTION]... FILES",
        "tee" => "tee [OPTION]... [FILE]...",
        "nl" => "nl [OPTION]... [FILE]...",
        "fold" => "fold [OPTION]... [FILE]...",
        "expand" | "unexpand" => "expand [OPTION]... [FILE]...",
        "shuf" => "shuf [OPTION]... [FILE]",
        "seq" => "seq [OPTION]... LAST",
        "basename" => "basename NAME [SUFFIX] or: basename OPTION... NAME...",
        "dirname" => "dirname [OPTION] NAME...",
        "realpath" => "realpath [OPTION]... FILE...",
        "readlink" => "readlink [OPTION]... FILE...",
        "base64" => "base64 [OPTION]... [FILE]",
        "cmp" => "cmp [OPTION]... FILE1 [FILE2]",
        "split" => "split [OPTION]... [FILE [PREFIX]]",
        "strings" => "strings [OPTION]... [FILE]...",
        "file" => "file [OPTION]... FILE...",
        "od" => "od [OPTION]... [FILE]...",
        "xxd" => "xxd [OPTION]... [FILE]",
        "hexdump" => "hexdump [OPTION]... [FILE]...",
        "md5sum" | "sha1sum" | "sha256sum" => "md5sum [OPTION]... [FILE]...",
        "ls" => "ls [OPTION]... [FILE]...",
        "grep" => "grep [OPTION]... PATTERNS [FILE]...",
        "cp" => "cp [OPTION]... SOURCE... DIRECTORY",
        "mv" => "mv [OPTION]... SOURCE... DIRECTORY",
        "rm" => "rm [OPTION]... [FILE]...",
        "mkdir" => "mkdir [OPTION]... DIRECTORY...",
        "find" => "find [-H] [-L] [-P] [path...] [expression]",
        _ => return format!("Usage: {name} [OPTION]... [FILE]..."),
    };
    format!("Usage: {body}")
}
/// `cmd: invalid option -- 'x'` plus the synopsis, GNU's exact shape, status 2.
pub(crate) fn invalid_option(name: &str, ch: char) -> Fail {
    Fail::usage(format!(
        "{name}: invalid option -- '{ch}'\n{}",
        usage_line(name)
    ))
}
/// `cmd: unrecognized option '--x'` plus the synopsis, status 2.
pub(crate) fn unrecognized_option(name: &str, long: &str) -> Fail {
    Fail::usage(format!(
        "{name}: unrecognized option '--{long}'\n{}",
        usage_line(name)
    ))
}
pub(crate) fn missing_argument(name: &str, spelling: &str) -> Fail {
    Fail::usage(format!(
        "{name}: option requires an argument -- '{spelling}'\n{}",
        usage_line(name)
    ))
}
pub(crate) fn options(
    name: &str,
    args: &[String],
    allowed: &str,
    valued: &str,
    long: &[(&str, char)],
) -> Result<Parsed, Fail> {
    let mut flags = Vec::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            operands.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(rest) = arg.strip_prefix("--").filter(|s| !s.is_empty()) {
            let (key, inline) = match rest.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (rest, None),
            };
            let ch = long
                .iter()
                .find(|(n, _)| *n == key)
                .map(|(_, c)| *c)
                .ok_or_else(|| unrecognized_option(name, key))?;
            if valued.contains(ch) {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        args.get(i)
                            .cloned()
                            .ok_or_else(|| missing_argument(name, key))?
                    }
                };
                flags.push((ch, value));
            } else if inline.is_some() {
                return Err(Fail::usage(format!(
                    "{name}: option '--{key}' doesn't allow an argument\n{}",
                    usage_line(name)
                )));
            } else {
                flags.push((ch, String::new()));
            }
            i += 1;
            continue;
        }
        if arg.len() > 1 && arg.starts_with('-') {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let ch = chars[j];
                if valued.contains(ch) {
                    let tail: String = chars[j + 1..].iter().collect();
                    let value = if tail.is_empty() {
                        i += 1;
                        args.get(i)
                            .cloned()
                            .ok_or_else(|| missing_argument(name, &ch.to_string()))?
                    } else {
                        tail
                    };
                    flags.push((ch, value));
                    j = chars.len();
                } else if allowed.contains(ch) {
                    flags.push((ch, String::new()));
                    j += 1;
                } else {
                    return Err(invalid_option(name, ch));
                }
            }
            i += 1;
            continue;
        }
        operands.push(arg.clone());
        i += 1;
    }
    Ok((flags, operands))
}
pub(crate) fn flag(flags: &[(char, String)], f: char) -> bool {
    flags.iter().any(|(k, _)| *k == f)
}
pub(crate) fn value(flags: &[(char, String)], f: char) -> Option<&str> {
    flags
        .iter()
        .rev()
        .find(|(k, _)| *k == f)
        .map(|(_, v)| v.as_str())
}
/// `date +FORMAT`. Unknown conversions fail rather than being copied through, so a
/// caller never mistakes an unimplemented field for a literal.
fn strftime(format: &str, now: &Clock) -> Result<String, Fail> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(match chars[i + 1] {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            i += 2;
            continue;
        }
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let spec = *chars
            .get(i + 1)
            .ok_or_else(|| Fail::usage("date: trailing `%` in format".to_string()))?;
        let (y, mo, d) = (now.year, now.month, now.day);
        let (h, mi, s) = (now.hour, now.minute, now.second);
        match spec {
            '%' => out.push('%'),
            'Y' => out.push_str(&format!("{y:04}")),
            'y' => out.push_str(&format!("{:02}", y % 100)),
            'm' => out.push_str(&format!("{mo:02}")),
            'd' => out.push_str(&format!("{d:02}")),
            'e' => out.push_str(&format!("{d:2}")),
            'H' => out.push_str(&format!("{h:02}")),
            'M' => out.push_str(&format!("{mi:02}")),
            'S' => out.push_str(&format!("{s:02}")),
            'N' => out.push_str(&format!("{:09}", now.micro * 1000)),
            's' => out.push_str(&now.unix.to_string()),
            'F' => out.push_str(&format!("{y:04}-{mo:02}-{d:02}")),
            'T' => out.push_str(&format!("{h:02}:{mi:02}:{s:02}")),
            'D' => out.push_str(&format!("{:02}/{d:02}/{:02}", mo, y % 100)),
            'a' => out.push_str(WEEKDAY_ABBR[now.weekday as usize]),
            'A' => out.push_str(WEEKDAY_NAME[now.weekday as usize]),
            'b' | 'h' => out.push_str(MONTH_ABBR[mo as usize - 1]),
            'B' => out.push_str(MONTH_NAME[mo as usize - 1]),
            'u' => out.push_str(&(if now.weekday == 0 { 7 } else { now.weekday }).to_string()),
            'w' => out.push_str(&now.weekday.to_string()),
            'Z' => out.push_str("UTC"),
            'z' => out.push_str("+0000"),
            other => {
                return Err(Fail::usage(format!(
                    "date: unsupported conversion `%{other}`"
                )))
            }
        }
        i += 2;
    }
    Ok(out)
}
struct LsFlags {
    all: bool,
    almost: bool,
    long: bool,
    human: bool,
    classify: bool,
    reverse: bool,
    by_time: bool,
    by_size: bool,
}
/// Trailing type marker for `ls -F`; executability comes from the stored mode.
fn classify(meta: &crate::Metadata) -> char {
    if meta.is_symlink {
        '@'
    } else if meta.is_dir {
        '/'
    } else if meta.mode & 0o111 != 0 {
        '*'
    } else {
        ' '
    }
}
/// Entries are (label, absolute path) so an operand keeps the spelling the caller used.
fn ls_render(c: &Computer, entries: &[(String, String)], f: &LsFlags) -> String {
    let rows: Vec<(String, Option<crate::Metadata>)> = entries
        .iter()
        .map(|(n, path)| (n.clone(), c.vfs.lstat(path).ok()))
        .collect();
    if !f.long {
        return rows
            .iter()
            .map(|(n, m)| {
                let mark = match (f.classify, m) {
                    (true, Some(meta)) => classify(meta),
                    _ => ' ',
                };
                if mark == ' ' {
                    format!("{n}\n")
                } else {
                    format!("{n}{mark}\n")
                }
            })
            .collect();
    }
    let blocks: u64 = rows
        .iter()
        .filter_map(|(_, m)| m.as_ref())
        .map(|m| allocated(apparent(m)) / 1024)
        .sum();
    let sized: Vec<String> = rows
        .iter()
        .map(|(_, m)| match m {
            Some(meta) if f.human => human(apparent(meta)),
            Some(meta) => apparent(meta).to_string(),
            None => "?".into(),
        })
        .collect();
    let width = sized.iter().map(String::len).max().unwrap_or(1);
    let owners = rows
        .iter()
        .map(|(_, m)| m.as_ref().map_or(1, |x| x.owner.len()))
        .max()
        .unwrap_or(1);
    let mut out = format!("total {blocks}\n");
    for (((name, meta), size), (_, path)) in rows.iter().zip(sized).zip(entries) {
        let Some(m) = meta else {
            out.push_str(&format!("?????????? ? ? ? {size:>width$} ? {name}\n"));
            continue;
        };
        let target = if m.is_symlink {
            c.vfs
                .read_link(path)
                .map(|t| format!(" -> {t}"))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let mark = if f.classify { classify(m) } else { ' ' };
        out.push_str(&format!(
            "{} {:>3} {:<owners$} {:<owners$} {:>width$} {} {name}{}{}\n",
            mode_string(m.mode, m.is_dir, m.is_symlink),
            m.links,
            m.owner,
            m.owner,
            size,
            clock(m.modified).short(),
            if mark == ' ' {
                String::new()
            } else {
                mark.to_string()
            },
            target,
        ));
    }
    out
}
fn ls_block(c: &Computer, dir: &str, f: &LsFlags) -> Result<String, Fail> {
    let mut names = c.vfs.list_as(dir, &c.user)?;
    if !f.all && !f.almost {
        names.retain(|n| !n.starts_with('.'));
    }
    let meta = |n: &String| c.vfs.lstat(&crate::normalize_path(dir, n));
    if f.by_time {
        names.sort_by_key(|n| std::cmp::Reverse(meta(n).map(|m| m.modified).unwrap_or(0)));
    } else if f.by_size {
        names.sort_by_key(|n| std::cmp::Reverse(meta(n).map(|m| apparent(&m)).unwrap_or(0)));
    }
    if f.reverse {
        names.reverse();
    }
    if f.all {
        names.splice(0..0, [".".to_string(), "..".to_string()]);
    }
    let entries: Vec<(String, String)> = names
        .iter()
        .map(|n| (n.clone(), crate::normalize_path(dir, n)))
        .collect();
    Ok(ls_render(c, &entries, f))
}
/// `stat -c` conversions. The VFS keeps one timestamp and no numeric ids, so %X/%Y/%Z
/// coincide and %u/%g report the computer's fixed identity.
fn stat_format(
    c: &Computer,
    format: &str,
    path: &str,
    m: &crate::Metadata,
) -> Result<String, Fail> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let kind = if m.is_symlink {
        "symbolic link"
    } else if m.is_dir {
        "directory"
    } else {
        "regular file"
    };
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(match chars[i + 1] {
                'n' => '\n',
                't' => '\t',
                other => other,
            });
            i += 2;
            continue;
        }
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let spec = *chars
            .get(i + 1)
            .ok_or_else(|| Fail::usage("stat: trailing `%` in format".to_string()))?;
        let type_bits: u32 = if m.is_symlink {
            0o120000
        } else if m.is_dir {
            0o040000
        } else {
            0o100000
        };
        match spec {
            '%' => out.push('%'),
            'n' => out.push_str(path),
            'N' => out.push_str(&format!("'{path}'")),
            's' => out.push_str(&apparent(m).to_string()),
            'b' => out.push_str(&apparent(m).div_ceil(512).to_string()),
            'B' => out.push_str("512"),
            'o' => out.push_str(&BLOCK.to_string()),
            'f' => out.push_str(&format!("{:x}", type_bits | u32::from(m.mode))),
            'a' => out.push_str(&format!("{:o}", m.mode & 0o7777)),
            'A' => out.push_str(&mode_string(m.mode, m.is_dir, m.is_symlink)),
            'F' => out.push_str(kind),
            'U' | 'G' => out.push_str(&m.owner),
            'u' => out.push_str(&c.hardware.uid.to_string()),
            'g' => out.push_str(&c.hardware.gid.to_string()),
            'i' => out.push_str(&m.inode.to_string()),
            'h' => out.push_str(&m.links.to_string()),
            'm' => out.push('/'),
            'd' => out.push('1'),
            'X' | 'Y' | 'Z' => out.push_str(&clock(m.modified).unix.to_string()),
            'x' | 'y' | 'z' => {
                out.push_str(&format!("{}.000000000 +0000", clock(m.modified).stamp()))
            }
            other => {
                return Err(Fail::usage(format!(
                    "stat: unsupported conversion `%{other}`"
                )))
            }
        }
        i += 2;
    }
    Ok(out)
}

/// PowerShell spells switches with one dash and a whole word; fold the ones the
/// file commands accept onto their POSIX letters before parsing.
fn powershell_switches(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|a| match a.as_str() {
            "-Recurse" => "-r".to_string(),
            "-Force" => "-f".to_string(),
            other => other.to_string(),
        })
        .collect()
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_date(args: &[String], t: u64) -> Result<String, Fail> {
    let now = clock(t);
    let mut chosen = None;
    for arg in args {
        match arg.strip_prefix('+') {
            Some(f) => chosen = Some(f.to_string()),
            None if matches!(arg.as_str(), "-u" | "--utc" | "--universal") => {}
            None => {
                return Err(Fail::usage(format!(
                    "date: unsupported option `{arg}`; the simulated clock is UTC and read-only"
                )))
            }
        }
    }
    match chosen {
        Some(f) => Ok(format!("{}\n", strftime(&f, &now)?)),
        None => Ok(format!(
            "{} {} {:2} {:02}:{:02}:{:02} UTC {:04}\n",
            WEEKDAY_ABBR[now.weekday as usize],
            MONTH_ABBR[now.month as usize - 1],
            now.day,
            now.hour,
            now.minute,
            now.second,
            now.year
        )),
    }
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_ls(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, mut paths) = options(
        "ls",
        args,
        "aAldh1FrtSR",
        "",
        &[
            ("all", 'a'),
            ("almost-all", 'A'),
            ("human-readable", 'h'),
            ("reverse", 'r'),
            ("recursive", 'R'),
            ("directory", 'd'),
            ("classify", 'F'),
        ],
    )?;
    let f = LsFlags {
        all: flag(&opts, 'a'),
        almost: flag(&opts, 'A'),
        long: flag(&opts, 'l'),
        human: flag(&opts, 'h'),
        classify: flag(&opts, 'F'),
        reverse: flag(&opts, 'r'),
        by_time: flag(&opts, 't'),
        by_size: flag(&opts, 'S'),
    };
    if paths.is_empty() {
        paths.push(".".into());
    }
    let mut out = String::new();
    let mut queue: Vec<(String, String)> = Vec::new();
    for operand in &paths {
        let resolved = c.resolve(operand);
        let meta = c
            .vfs
            .lstat(&resolved)
            .map_err(|_| format!("cannot access '{operand}': No such file or directory"))?;
        if meta.is_dir && !flag(&opts, 'd') {
            queue.push((operand.clone(), resolved));
        } else {
            out.push_str(&ls_render(c, &[(operand.clone(), resolved)], &f));
        }
    }
    let titled = queue.len() > 1 || flag(&opts, 'R') || !out.is_empty();
    while let Some((label, dir)) = queue.first().cloned() {
        queue.remove(0);
        if !out.is_empty() {
            out.push('\n');
        }
        if titled {
            out.push_str(&format!("{label}:\n"));
        }
        out.push_str(&ls_block(c, &dir, &f)?);
        if flag(&opts, 'R') {
            let mut children: Vec<(String, String)> = c
                .vfs
                .list_as(&dir, &c.user)?
                .into_iter()
                .filter(|n| f.all || f.almost || !n.starts_with('.'))
                .map(|n| {
                    (
                        format!("{}/{n}", label.trim_end_matches('/')),
                        crate::normalize_path(&dir, &n),
                    )
                })
                .filter(|(_, p)| c.vfs.lstat(p).is_ok_and(|m| m.is_dir))
                .collect();
            children.append(&mut queue);
            queue = children;
        }
    }
    Ok(out)
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_grep(c: &Computer, args: &[String], input: &str) -> Result<String, Fail> {
    let (opts, mut rest) = options(
        "grep",
        args,
        "ivnclLFEGqshHwxrRo",
        "eABC",
        &[
            ("ignore-case", 'i'),
            ("invert-match", 'v'),
            ("line-number", 'n'),
            ("count", 'c'),
            ("files-with-matches", 'l'),
            ("files-without-match", 'L'),
            ("fixed-strings", 'F'),
            ("extended-regexp", 'E'),
            ("basic-regexp", 'G'),
            ("regexp", 'e'),
            ("quiet", 'q'),
            ("silent", 'q'),
            ("no-messages", 's'),
            ("no-filename", 'h'),
            ("with-filename", 'H'),
            ("word-regexp", 'w'),
            ("line-regexp", 'x'),
            ("recursive", 'r'),
            ("only-matching", 'o'),
            ("after-context", 'A'),
            ("before-context", 'B'),
            ("context", 'C'),
        ],
    )?;
    let count = |letter: char| -> Result<Option<usize>, Fail> {
        match value(&opts, letter) {
            Some(v) => v
                .parse()
                .map(Some)
                .map_err(|_| Fail::usage(format!("grep: option `-{letter}` expects a number"))),
            None => Ok(None),
        }
    };
    let both = count('C')?;
    let (after, before) = (
        count('A')?.or(both).unwrap_or(0),
        count('B')?.or(both).unwrap_or(0),
    );
    // Asking for context at all switches on the `--` group separator, even at 0.
    let context = ['A', 'B', 'C'].iter().any(|f| value(&opts, *f).is_some());
    let mut patterns: Vec<String> = opts
        .iter()
        .filter(|(k, _)| *k == 'e')
        .map(|(_, v)| v.clone())
        .collect();
    if patterns.is_empty() {
        if rest.is_empty() {
            return Err(Fail::usage("grep: missing pattern"));
        }
        patterns.push(rest.remove(0));
    }
    let recursive = flag(&opts, 'r') || flag(&opts, 'R');
    // The last of -E/-G wins, as GNU grep does.
    let extended = opts
        .iter()
        .rev()
        .find(|(k, _)| *k == 'E' || *k == 'G')
        .is_some_and(|(k, _)| *k == 'E');
    // Without -E a pattern is a *basic* regular expression: `a\+` repeats and `a+`
    // is a literal plus. Getting this backwards is the classic porting trap, so the
    // same translator sed uses does the work here.
    let expression = patterns
        .iter()
        // A pattern may itself hold newlines: `grep -e $'a\nb'` is two alternatives.
        .flat_map(|p| p.split('\n').map(str::to_string).collect::<Vec<_>>())
        .map(|p| {
            let body = if flag(&opts, 'F') {
                regex::escape(&p)
            } else {
                crate::sed::translate(&p, extended)?
            };
            Ok(if flag(&opts, 'x') {
                format!("^(?:{body})$")
            } else if flag(&opts, 'w') {
                format!("\\b(?:{body})\\b")
            } else {
                format!("(?:{body})")
            })
        })
        .collect::<Result<Vec<_>, Fail>>()?
        .join("|");
    let regex = regex::RegexBuilder::new(&expression)
        .case_insensitive(flag(&opts, 'i'))
        .build()
        .map_err(|e| Fail::usage(e.to_string()))?;
    // Sources are (label, text); with no operand the pipeline's stdin is the source.
    let mut sources: Vec<(String, String)> = Vec::new();
    for operand in &rest {
        let path = c.resolve(operand);
        let directory = c.vfs.lstat(&path).is_ok_and(|m| m.is_dir);
        if directory && !recursive {
            if !flag(&opts, 's') {
                return Err(Fail::op("grep", operand, "Is a directory"));
            }
            continue;
        }
        let members: Vec<(String, String)> = if directory {
            walk_tree(c, &path)
                .into_iter()
                .filter(|(_, _, dir)| !dir)
                .map(|(p, _, _)| (p.clone(), p))
                .collect()
        } else {
            vec![(operand.clone(), path)]
        };
        for (label, file) in members {
            match c.vfs.read_as(&file, &c.user) {
                Ok(bytes) => sources.push((label, String::from_utf8_lossy(&bytes).into_owned())),
                Err(e) if flag(&opts, 's') => {
                    let _ = e;
                }
                Err(e) => return Err(Fail::io("grep", &label, &e)),
            }
        }
    }
    if rest.is_empty() {
        sources.push((String::from("(standard input)"), input.to_string()));
    }
    let labelled = if flag(&opts, 'h') {
        false
    } else {
        flag(&opts, 'H') || recursive || sources.len() > 1
    };
    let mut out = String::new();
    let mut matched = false;
    for (label, text) in &sources {
        let hits: Vec<(usize, &str)> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| regex.is_match(line) != flag(&opts, 'v'))
            .collect();
        matched |= !hits.is_empty();
        let prefix = if labelled {
            format!("{label}:")
        } else {
            String::new()
        };
        if flag(&opts, 'q') {
            continue;
        }
        if flag(&opts, 'c') {
            out.push_str(&format!("{prefix}{}\n", hits.len()));
        } else if flag(&opts, 'l') {
            if !hits.is_empty() {
                out.push_str(&format!("{label}\n"));
            }
        } else if flag(&opts, 'L') {
            if hits.is_empty() {
                out.push_str(&format!("{label}\n"));
            }
        } else if flag(&opts, 'o') {
            // Only the matched substrings; an inverted line has none to show.
            for (n, line) in hits {
                for found in regex.find_iter(line) {
                    if flag(&opts, 'n') {
                        out.push_str(&format!("{prefix}{}:{}\n", n + 1, found.as_str()));
                    } else {
                        out.push_str(&format!("{prefix}{}\n", found.as_str()));
                    }
                }
            }
        } else if context {
            // Context lines carry `-` where a matching line carries `:`, and a gap
            // between groups prints the `--` separator, exactly as GNU grep does.
            let all: Vec<&str> = text.lines().collect();
            let matches: std::collections::BTreeSet<usize> = hits.iter().map(|(n, _)| *n).collect();
            let mut shown = std::collections::BTreeSet::new();
            for n in &matches {
                for k in n.saturating_sub(before)..=(n + after).min(all.len().saturating_sub(1)) {
                    shown.insert(k);
                }
            }
            let mut previous: Option<usize> = None;
            for n in shown {
                if previous.is_some_and(|p| n > p + 1) {
                    out.push_str("--\n");
                }
                let mark = if matches.contains(&n) { ':' } else { '-' };
                let head = if labelled {
                    format!("{label}{mark}")
                } else {
                    String::new()
                };
                if flag(&opts, 'n') {
                    out.push_str(&format!("{head}{}{mark}{}\n", n + 1, all[n]));
                } else {
                    out.push_str(&format!("{head}{}\n", all[n]));
                }
                previous = Some(n);
            }
        } else {
            for (n, line) in hits {
                if flag(&opts, 'n') {
                    out.push_str(&format!("{prefix}{}:{line}\n", n + 1));
                } else {
                    out.push_str(&format!("{prefix}{line}\n"));
                }
            }
        }
    }
    // Status is the answer: 0 found, 1 not found. -c and -L still report it.
    let found = if flag(&opts, 'L') {
        !out.is_empty()
    } else {
        matched
    };
    if found {
        Ok(out)
    } else {
        Err(Fail::new(String::new(), 1).with_output(out))
    }
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_stat(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let err = |e: crate::VfsError| Fail::from(e);
    let (opts, paths) = options(
        "stat",
        args,
        "L",
        "c",
        &[("format", 'c'), ("printf", 'c'), ("dereference", 'L')],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("stat: missing operand"));
    }
    let mut out = String::new();
    for operand in &paths {
        let path = c.resolve(operand);
        c.vfs
            .check_access(&path, &c.user, false, false, false)
            .map_err(err)?;
        // Without -L a symlink describes itself, exactly as coreutils does.
        let m = if flag(&opts, 'L') {
            c.vfs.stat(&path)
        } else {
            c.vfs.lstat(&path)
        }
        .map_err(err)?;
        if let Some(format) = value(&opts, 'c') {
            out.push_str(&stat_format(c, format, operand, &m)?);
            out.push('\n');
            continue;
        }
        let kind = if m.is_symlink {
            "symbolic link"
        } else if m.is_dir {
            "directory"
        } else {
            "regular file"
        };
        let stamp = format!("{}.000000000 +0000", clock(m.modified).stamp());
        out.push_str(&format!(
                "  File: {operand}\n  Size: {size:<10}\tBlocks: {blocks:<10} IO Block: {block:<6} {kind}\n\
                 Device: 1,0\tInode: {inode:<11} Links: {links}\n\
                 Access: ({mode:04o}/{rwx})  Uid: ({uid:5}/{owner:>8})   Gid: ({gid:5}/{owner:>8})\n\
                 Access: {stamp}\nModify: {stamp}\nChange: {stamp}\n Birth: -\n",
                size = apparent(&m),
                blocks = apparent(&m).div_ceil(512),
                block = BLOCK,
                inode = m.inode,
                links = m.links,
                mode = m.mode & 0o7777,
                rwx = mode_string(m.mode, m.is_dir, m.is_symlink),
                uid = c.hardware.uid,
                gid = c.hardware.gid,
                owner = m.owner,
            ));
    }
    Ok(out)
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_find(c: &Computer, args: &[String]) -> Result<String, Fail> {
    // Predicates are applied, never ignored: a search that silently returns the
    // wrong set is worse than one that refuses.
    let roots: Vec<&String> = args
        .iter()
        .take_while(|a| !a.starts_with('-'))
        .collect::<Vec<_>>();
    let predicates = &args[roots.len()..];
    let roots: Vec<String> = if roots.is_empty() {
        vec![c.resolve(".")]
    } else {
        roots.into_iter().map(|r| c.resolve(r)).collect()
    };
    let mut name: Option<&str> = None;
    let mut iname: Option<String> = None;
    let mut kind: Option<char> = None;
    let mut max_depth: Option<usize> = None;
    let mut index = 0;
    while index < predicates.len() {
        let value = |i: usize| {
            predicates
                .get(i)
                .map(String::as_str)
                .ok_or_else(|| format!("find: missing argument to `{}`", predicates[i - 1]))
        };
        match predicates[index].as_str() {
            "-name" => {
                name = Some(value(index + 1)?);
                index += 2;
            }
            "-iname" => {
                iname = Some(value(index + 1)?.to_lowercase());
                index += 2;
            }
            "-type" => {
                let t = value(index + 1)?;
                kind = match t {
                    "f" => Some('f'),
                    "d" => Some('d'),
                    _ => return Err(Fail::usage(format!("find: unsupported -type `{t}`"))),
                };
                index += 2;
            }
            "-maxdepth" => {
                max_depth = Some(
                    value(index + 1)?
                        .parse()
                        .map_err(|_| "find: -maxdepth expects a number".to_string())?,
                );
                index += 2;
            }
            other => {
                return Err(Fail::usage(format!(
                    "find: unsupported predicate `{other}`"
                )))
            }
        }
    }
    let mut out = String::new();
    for root in &roots {
        let root = root.trim_end_matches('/');
        let root = if root.is_empty() { "/" } else { root };
        if c.vfs.stat(root).is_err() {
            return Err(format!("find: `{root}`: No such file or directory").into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for path in c.vfs.all_files().keys() {
            let inside = path == root
                || path.starts_with(&format!("{}/", root.trim_end_matches('/')))
                || root == "/";
            if !inside {
                continue;
            }
            // Every directory on the way to a file is itself a result.
            let mut walk = Vec::new();
            let relative = path.strip_prefix(root).unwrap_or(path);
            let mut current = root.to_owned();
            walk.push((current.clone(), 0usize, true));
            let parts: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
            for (depth, part) in parts.iter().enumerate() {
                current = format!("{}/{part}", current.trim_end_matches('/'));
                walk.push((current.clone(), depth + 1, depth + 1 < parts.len()));
            }
            for (candidate, depth, directory) in walk {
                if max_depth.is_some_and(|max| depth > max) || !seen.insert(candidate.clone()) {
                    continue;
                }
                if kind.is_some_and(|k| (k == 'd') != directory) {
                    continue;
                }
                let base = candidate
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&candidate);
                if name.is_some_and(|pattern| !wildcard(pattern, base))
                    || iname
                        .as_deref()
                        .is_some_and(|pattern| !wildcard(pattern, &base.to_lowercase()))
                {
                    continue;
                }
                out.push_str(&candidate);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// One `[ugoa][-+=][rwxXst]` clause. `conditional` is `X`: execute only where the node
/// is a directory or already carries an execute bit.
struct ModeClause {
    who: &'static str,
    op: char,
    perms: String,
}
/// A chmod mode: an octal literal, or symbolic clauses applied left to right.
enum ModeSpec {
    Absolute(u16),
    Symbolic(Vec<ModeClause>),
}
impl ModeSpec {
    fn parse(spec: &str) -> Result<Self, Fail> {
        if spec.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let mode = u16::from_str_radix(spec, 8)
                .ok()
                .filter(|m| *m <= 0o7777)
                .ok_or_else(|| Fail::usage(format!("chmod: invalid octal mode `{spec}`")))?;
            return Ok(ModeSpec::Absolute(mode));
        }
        let mut clauses = Vec::new();
        for part in spec.split(',') {
            let chars: Vec<char> = part.chars().collect();
            let mut i = 0;
            while chars.get(i).is_some_and(|c| "ugoa".contains(*c)) {
                i += 1;
            }
            let who: String = chars[..i].iter().collect();
            // No umask is modelled, so a bare `+x` means `a+x`.
            let who: &'static str = if who.is_empty() || who.contains('a') {
                "ugo"
            } else if who == "u" {
                "u"
            } else if who == "g" {
                "g"
            } else if who == "o" {
                "o"
            } else if who == "ug" || who == "gu" {
                "ug"
            } else if who == "uo" || who == "ou" {
                "uo"
            } else {
                "go"
            };
            if i == chars.len() {
                return Err(Fail::usage(format!("chmod: invalid mode `{spec}`")));
            }
            while i < chars.len() {
                let op = chars[i];
                if !"+-=".contains(op) {
                    return Err(Fail::usage(format!("chmod: invalid mode `{spec}`")));
                }
                i += 1;
                let start = i;
                while chars.get(i).is_some_and(|c| "rwxXst".contains(*c)) {
                    i += 1;
                }
                if chars.get(i).is_some_and(|c| "ugo".contains(*c)) {
                    return Err(Fail::usage(format!(
                        "chmod: copying permissions (`{spec}`) is not implemented"
                    )));
                }
                clauses.push(ModeClause {
                    who,
                    op,
                    perms: chars[start..i].iter().collect(),
                });
            }
        }
        Ok(ModeSpec::Symbolic(clauses))
    }
    fn apply(&self, mode: u16, is_dir: bool) -> u16 {
        let clauses = match self {
            ModeSpec::Absolute(m) => return *m,
            ModeSpec::Symbolic(v) => v,
        };
        let mut mode = mode & 0o7777;
        for clause in clauses {
            // `X` reads the mode as it stands after the clauses before it.
            let executable = clause.perms.contains('x')
                || (clause.perms.contains('X') && (is_dir || mode & 0o111 != 0));
            let (mut value, mut mask) = (0u16, 0u16);
            for (who, shift, special) in [('u', 6, 0o4000), ('g', 3, 0o2000), ('o', 0, 0o1000)] {
                if !clause.who.contains(who) {
                    continue;
                }
                mask |= (7 << shift) | special;
                if clause.perms.contains('r') {
                    value |= 4 << shift;
                }
                if clause.perms.contains('w') {
                    value |= 2 << shift;
                }
                if executable {
                    value |= 1 << shift;
                }
                if clause.perms.contains('s') && special != 0o1000 {
                    value |= special;
                }
                if clause.perms.contains('t') && special == 0o1000 {
                    value |= special;
                }
            }
            mode = match clause.op {
                '+' => mode | value,
                '-' => mode & !value,
                _ => (mode & !mask) | value,
            };
        }
        mode
    }
}
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_chmod(c: &mut Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options(
        "chmod",
        args,
        "Rv",
        "",
        &[("recursive", 'R'), ("verbose", 'v')],
    )?;
    let Some((spec, paths)) = rest.split_first() else {
        return Err(Fail::usage("chmod: missing operand"));
    };
    if paths.is_empty() {
        return Err(Fail::usage("chmod: missing operand"));
    }
    let change = ModeSpec::parse(spec)?;
    let mut told = String::new();
    for operand in paths {
        let root = c.resolve(operand);
        let meta = c
            .vfs
            .lstat(&root)
            .map_err(|_| format!("cannot access '{operand}': No such file or directory"))?;
        let targets: Vec<(String, bool)> = if flag(&opts, 'R') && meta.is_dir {
            walk_tree(c, &root)
                .into_iter()
                .map(|(path, _, dir)| (path, dir))
                .collect()
        } else {
            vec![(root, meta.is_dir)]
        };
        for (path, is_dir) in targets {
            let current = c.vfs.lstat(&path).map_err(Fail::from)?.mode;
            let wanted = change.apply(current, is_dir);
            c.vfs.chmod_as(&path, wanted, &c.user).map_err(Fail::from)?;
            if flag(&opts, 'v') {
                told.push_str(&format!(
                    "mode of '{path}' changed from {current:04o} to {wanted:04o}\n"
                ));
            }
        }
    }
    Ok(told)
}
/// Inverse of `clock`: a civil UTC date to a simulated tick. Times before the world's
/// epoch cannot be represented, so they are refused rather than clamped.
fn tick_from_civil(y: i64, m: i64, d: i64, hh: u64, mi: u64, ss: u64) -> Result<u64, Fail> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mi > 59 || ss > 60 {
        return Err(Fail::usage("touch: timestamp is not a valid date"));
    }
    let year = y - i64::from(m <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let unix = days * 86_400 + (hh * 3600 + mi * 60 + ss) as i64;
    let offset = unix - EPOCH_UNIX_SECONDS as i64;
    if offset < 0 {
        return Err(Fail::usage(
            "touch: timestamps before the simulated epoch (2026-09-17 09:00:00 UTC) \
             cannot be represented",
        ));
    }
    Ok(offset as u64 * 1_000_000)
}
/// `-t [[CC]YY]MMDDhhmm[.ss]`.
fn touch_stamp(value: &str) -> Result<u64, Fail> {
    let invalid = || Fail::usage(format!("touch: unsupported -t stamp `{value}`"));
    let (body, seconds) = match value.split_once('.') {
        Some((b, s)) => (b, s.parse::<u64>().map_err(|_| invalid())?),
        None => (value, 0),
    };
    if !body.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid());
    }
    let number = |from: usize, len: usize| -> Result<i64, Fail> {
        body.get(from..from + len)
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())
    };
    let (year, rest) = match body.len() {
        8 => (2026, 0),
        10 => (2000 + number(0, 2)?, 2),
        12 => (number(0, 4)?, 4),
        _ => return Err(invalid()),
    };
    tick_from_civil(
        year,
        number(rest, 2)?,
        number(rest + 2, 2)?,
        number(rest + 4, 2)? as u64,
        number(rest + 6, 2)? as u64,
        seconds,
    )
}
/// `-d`: `@SECONDS` or `YYYY-MM-DD[ |T]HH:MM[:SS]`. Relative words such as `yesterday`
/// are refused: there is no host clock to resolve them against.
fn touch_date(value: &str) -> Result<u64, Fail> {
    let invalid = || {
        Fail::usage(format!(
            "touch: unsupported -d date `{value}`; use @SECONDS or YYYY-MM-DD[ HH:MM[:SS]]"
        ))
    };
    if let Some(seconds) = value.strip_prefix('@') {
        let unix: i64 = seconds.parse().map_err(|_| invalid())?;
        return tick_from_civil(1970, 1, 1, 0, 0, 0).and_then(|_| {
            u64::try_from(unix - EPOCH_UNIX_SECONDS as i64)
                .map(|v| v * 1_000_000)
                .map_err(|_| {
                    Fail::usage(
                        "touch: timestamps before the simulated epoch cannot be represented",
                    )
                })
        });
    }
    let (date, time) = match value.split_once(['T', ' ']) {
        Some((d, t)) => (d, t),
        None => (value, "00:00:00"),
    };
    let day: Vec<&str> = date.split('-').collect();
    let clock: Vec<&str> = time.trim_end_matches('Z').split(':').collect();
    if day.len() != 3 || clock.len() < 2 || clock.len() > 3 {
        return Err(invalid());
    }
    let n = |v: &str| v.parse::<i64>().map_err(|_| invalid());
    tick_from_civil(
        n(day[0])?,
        n(day[1])?,
        n(day[2])?,
        n(clock[0])? as u64,
        n(clock[1])? as u64,
        clock.get(2).map(|v| n(v)).transpose()?.unwrap_or(0) as u64,
    )
}
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_touch(c: &mut Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, paths) = options(
        "touch",
        args,
        "acm",
        "dtr",
        &[("no-create", 'c'), ("date", 'd'), ("reference", 'r')],
    )?;
    if paths.is_empty() {
        return Err(Fail::usage("touch: missing operand"));
    }
    // The VFS keeps one timestamp, so -a and -m select the same field.
    let stamp = match (value(&opts, 'r'), value(&opts, 'd'), value(&opts, 't')) {
        (Some(reference), _, _) => {
            c.vfs
                .lstat(&c.resolve(reference))
                .map_err(|_| format!("cannot stat '{reference}': No such file or directory"))?
                .modified
        }
        (_, Some(date), _) => touch_date(date)?,
        (_, _, Some(stamp)) => touch_stamp(stamp)?,
        _ => t,
    };
    for operand in &paths {
        let path = c.resolve(operand);
        if c.vfs.exists(&path) {
            c.vfs
                .set_modified_as(&path, stamp, &c.user)
                .map_err(Fail::from)?;
        } else if !flag(&opts, 'c') {
            c.vfs
                .write_as(&path, b"", &c.user, stamp)
                .map_err(Fail::from)?;
        }
    }
    Ok(String::new())
}
/// `ps` column output. No terminal and no CPU accounting are modelled, so TTY is `?`
/// and TIME is `00:00:00` for every process; everything else is read from the table.
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_ps(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, rest) = options(
        "ps",
        args,
        "efA",
        "up",
        &[
            ("json", 'j'),
            ("full", 'f'),
            ("user", 'u'),
            ("pid", 'p'),
            ("every", 'e'),
        ],
    )?;
    if let Some(operand) = rest.first() {
        return Err(Fail::usage(format!(
            "ps: unsupported operand `{operand}`; this world models `ps`, `ps -e`/`-A`, \
             `ps -f`, `-u USER`, `-p PID` and `--json`"
        )));
    }
    let pid = match value(&opts, 'p') {
        Some(v) => Some(
            v.parse::<u64>()
                .map_err(|_| Fail::usage("ps: option `-p` expects a pid"))?,
        ),
        None => None,
    };
    let owner = value(&opts, 'u');
    let everyone = flag(&opts, 'e')
        || flag(&opts, 'A')
        || flag(&opts, 'j')
        || owner.is_some()
        || pid.is_some();
    let rows: Vec<crate::Process> = c
        .processes
        .list()
        .into_iter()
        .filter(|p| pid.is_none_or(|want| p.pid == want))
        .filter(|p| owner.is_none_or(|want| p.owner == want))
        .filter(|p| everyone || p.owner == c.user)
        .collect();
    if flag(&opts, 'j') {
        return Ok(serde_json::to_string_pretty(&rows)
            .map(|s| s + "\n")
            .map_err(|e| e.to_string())?);
    }
    let command = |p: &crate::Process| match p.state {
        // A reaped-but-unwaited process is what `<defunct>` means.
        crate::ProcessState::Zombie { .. } => format!("{} <defunct>", p.command),
        _ => p.command.clone(),
    };
    let full = flag(&opts, 'f');
    let mut out = if full {
        format!(
            "{:<8} {:>7} {:>7}  {:>1} {:<5} {:<8} {:>8} {}\n",
            "UID", "PID", "PPID", "C", "STIME", "TTY", "TIME", "CMD"
        )
    } else {
        format!("{:>7} {:<8} {:>8} {}\n", "PID", "TTY", "TIME", "CMD")
    };
    for p in &rows {
        let started = clock(p.started);
        if full {
            out.push_str(&format!(
                "{:<8} {:>7} {:>7}  {:>1} {:02}:{:02} {:<8} {:>8} {}\n",
                p.owner,
                p.pid,
                p.parent,
                0,
                started.hour,
                started.minute,
                "?",
                "00:00:00",
                command(p),
            ));
        } else {
            out.push_str(&format!(
                "{:>7} {:<8} {:>8} {}\n",
                p.pid,
                "?",
                "00:00:00",
                command(p)
            ));
        }
    }
    Ok(out)
}
#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_uptime(c: &Computer, args: &[String], t: u64) -> Result<String, Fail> {
    let (opts, rest) = options("uptime", args, "ps", "", &[("pretty", 'p'), ("since", 's')])?;
    if !rest.is_empty() {
        return Err(Fail::usage("uptime: takes no operands"));
    }
    let boot = c.hardware.boot_tick;
    let elapsed = t.saturating_sub(boot) / 1_000_000;
    if flag(&opts, 's') {
        return Ok(format!("{}\n", clock(boot).stamp()));
    }
    let (days, hours, minutes) = (
        elapsed / 86_400,
        elapsed % 86_400 / 3600,
        elapsed % 3600 / 60,
    );
    let plural = |n: u64, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    if flag(&opts, 'p') {
        let mut parts = Vec::new();
        if days > 0 {
            parts.push(plural(days, "day"));
        }
        if hours > 0 {
            parts.push(plural(hours, "hour"));
        }
        parts.push(plural(minutes, "minute"));
        return Ok(format!("up {}\n", parts.join(", ")));
    }
    let span = if days > 0 {
        format!("{} days, {hours:2}:{minutes:02}", days)
    } else if hours > 0 {
        format!("{hours:2}:{minutes:02}")
    } else {
        format!("{minutes} min")
    };
    let now = clock(t);
    Ok(format!(
        " {:02}:{:02}:{:02} up {span},  1 user,  load average: 0.00, 0.00, 0.00\n",
        now.hour, now.minute, now.second
    ))
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_which(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, names) = options("which", args, "a", "", &[("all", 'a')])?;
    if names.is_empty() {
        return Err(Fail::usage("which: missing operand"));
    }
    let separator = if c.dialect == "powershell" { ';' } else { ':' };
    let mut out = String::new();
    let mut missing = 0;
    for name in &names {
        let mut hits = Vec::new();
        for dir in c
            .env
            .get("PATH")
            .map(String::as_str)
            .unwrap_or("/bin:/usr/bin")
            .split(separator)
        {
            let candidate = c.resolve(&format!("{dir}/{name}"));
            if c.vfs.exists(&candidate) {
                hits.push(candidate);
            }
        }
        // Built-ins run in-process: report the conventional path so "is it
        // available?" gets a truthful yes even though no binary exists.
        if hits.is_empty() && BUILTINS.contains(&name.to_ascii_lowercase().as_str()) {
            hits.push(format!("/usr/bin/{name}"));
        }
        if hits.is_empty() {
            missing += 1;
        }
        for hit in hits
            .iter()
            .take(if flag(&opts, 'a') { usize::MAX } else { 1 })
        {
            out.push_str(hit);
            out.push('\n');
        }
    }
    if missing == names.len() {
        Err(Fail::new(String::new(), 1).with_output(out))
    } else {
        Ok(out)
    }
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_du(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, mut paths) = options(
        "du",
        args,
        "sahkbcm",
        "d",
        &[
            ("summarize", 's'),
            ("all", 'a'),
            ("human-readable", 'h'),
            ("bytes", 'b'),
            ("total", 'c'),
            ("max-depth", 'd'),
        ],
    )?;
    if paths.is_empty() {
        paths.push(".".into());
    }
    let limit = match value(&opts, 'd') {
        Some(v) => Some(
            v.parse::<usize>()
                .map_err(|_| Fail::usage("du: --max-depth expects a number"))?,
        ),
        None => None,
    };
    let render = |bytes: u64| {
        if flag(&opts, 'b') {
            bytes.to_string()
        } else if flag(&opts, 'h') {
            human(allocated(bytes))
        } else if flag(&opts, 'm') {
            (allocated(bytes) / (1 << 20)).to_string()
        } else {
            (allocated(bytes) / 1024).to_string()
        }
    };
    let mut out = String::new();
    let mut grand = 0u64;
    for operand in &paths {
        let root = c.resolve(operand);
        c.vfs
            .lstat(&root)
            .map_err(|_| format!("cannot access '{operand}': No such file or directory"))?;
        let nodes = walk_tree(c, &root);
        // Each node's bytes roll up into every ancestor inside the root.
        let mut totals: BTreeMap<String, u64> = BTreeMap::new();
        for (path, size, _) in &nodes {
            let mut current = path.clone();
            loop {
                *totals.entry(current.clone()).or_default() += allocated(*size);
                if current == root || current.len() <= root.len() {
                    break;
                }
                current = current
                    .rsplit_once('/')
                    .map(|(a, _)| a.to_string())
                    .unwrap_or_default();
                if current.is_empty() {
                    current = "/".into();
                }
            }
        }
        grand += totals.get(&root).copied().unwrap_or(0);
        let depth_of = |path: &str| path[root.len().min(path.len())..].matches('/').count();
        let mut rows: Vec<(String, u64)> = nodes
            .iter()
            .filter(|(path, _, dir)| {
                (*dir || flag(&opts, 'a'))
                    && !flag(&opts, 's')
                    && limit.is_none_or(|max| depth_of(path) <= max)
            })
            .map(|(path, _, _)| (path.clone(), totals.get(path).copied().unwrap_or(0)))
            .collect();
        // Children before their parent, as du walks depth-first.
        rows.sort_by(|a, b| b.0.cmp(&a.0));
        rows.sort_by_key(|(path, _)| std::cmp::Reverse(depth_of(path)));
        if flag(&opts, 's') {
            rows.push((root.clone(), totals.get(&root).copied().unwrap_or(0)));
        }
        for (path, bytes) in rows {
            let label = if path == root {
                operand.clone()
            } else {
                format!("{}{}", operand.trim_end_matches('/'), &path[root.len()..])
            };
            out.push_str(&format!("{}\t{label}\n", render(bytes)));
        }
    }
    if flag(&opts, 'c') {
        out.push_str(&format!("{}\ttotal\n", render(grand)));
    }
    Ok(out)
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_df(c: &Computer, args: &[String]) -> Result<String, Fail> {
    let (opts, paths) = options(
        "df",
        args,
        "hkT",
        "",
        &[("human-readable", 'h'), ("print-type", 'T')],
    )?;
    for operand in &paths {
        c.vfs
            .lstat(&c.resolve(operand))
            .map_err(|_| format!("{operand}: No such file or directory"))?;
    }
    // One filesystem: capacity is a fixed fact, usage is summed from the VFS.
    let total = c.hardware.disk_bytes;
    let used: u64 = walk_tree(c, "/")
        .iter()
        .map(|(_, size, _)| allocated(*size))
        .sum();
    let free = total.saturating_sub(used);
    let percent = if total == 0 {
        0
    } else {
        used.div_ceil(total.div_ceil(100)).min(100)
    };
    let cell = |bytes: u64| {
        if flag(&opts, 'h') {
            human(bytes)
        } else {
            (bytes / 1024).to_string()
        }
    };
    let kind = if flag(&opts, 'T') { "ext4     " } else { "" };
    let head = if flag(&opts, 'T') { "Type     " } else { "" };
    Ok(format!(
            "Filesystem     {head}{:>10} {:>10} {:>10} Use% Mounted on\n{:<14} {kind}{:>10} {:>10} {:>10} {:>3}% /\n",
            if flag(&opts, 'h') { "Size" } else { "1K-blocks" },
            "Used",
            if flag(&opts, 'h') { "Avail" } else { "Available" },
            c.hardware.device,
            cell(total),
            cell(used),
            cell(free),
            percent
        ))
}

#[inline(never)] // Keeps run()'s frame small: shell recursion is bounded by depth, not stack.
fn cmd_ip(c: &Computer, args: &[String]) -> Result<String, Fail> {
    // -4/-6/-o/-brief would filter or reshape the printout; this world's NIC summary
    // is a fixed block, so they are refused rather than accepted and dropped.
    let (_, rest) = options("ip", args, "", "", &[])?;
    let object = rest.first().map(String::as_str).unwrap_or("");
    let action = rest.get(1).map(String::as_str).unwrap_or("show");
    if !matches!(action, "show" | "list" | "s" | "l") {
        return Err(Fail::usage(format!(
            "ip: unsupported action `{action}`; the simulated NIC is read-only"
        )));
    }
    let h = &c.hardware;
    let octets: Vec<u32> = h.ipv4.split('.').filter_map(|p| p.parse().ok()).collect();
    let address = if octets.len() == 4 {
        octets.iter().fold(0u32, |acc, o| (acc << 8) | o)
    } else {
        0
    };
    let mask = if h.prefix >= 32 {
        u32::MAX
    } else {
        !(u32::MAX >> h.prefix)
    };
    let quad = |v: u32| format!("{}.{}.{}.{}", v >> 24, v >> 16 & 255, v >> 8 & 255, v & 255);
    let (network, broadcast) = (quad(address & mask), quad(address | !mask));
    match object {
            "a" | "addr" | "address" => Ok(format!(
                "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN group default qlen 1000\n    \
                 link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00\n    \
                 inet 127.0.0.1/8 scope host lo\n       valid_lft forever preferred_lft forever\n\
                 2: {nic}: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP group default qlen 1000\n    \
                 link/ether {mac} brd ff:ff:ff:ff:ff:ff\n    \
                 inet {ip}/{prefix} brd {broadcast} scope global {nic}\n       valid_lft forever preferred_lft forever\n",
                nic = h.interface, mac = h.mac, ip = h.ipv4, prefix = h.prefix,
            )),
            "l" | "link" => Ok(format!(
                "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\n    \
                 link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00\n\
                 2: {nic}: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP mode DEFAULT group default qlen 1000\n    \
                 link/ether {mac} brd ff:ff:ff:ff:ff:ff\n",
                nic = h.interface, mac = h.mac,
            )),
            "r" | "route" => Ok(format!(
                "default via {gw} dev {nic} proto dhcp metric 100\n\
                 {network}/{prefix} dev {nic} proto kernel scope link src {ip} metric 100\n",
                gw = h.gateway, nic = h.interface, prefix = h.prefix, ip = h.ipv4,
            )),
            "" => Err(Fail::usage("ip: missing object; try `ip addr`, `ip link` or `ip route`")),
            other => Err(Fail::usage(format!(
                "ip: unsupported object `{other}`; this world models addr, link and route only"
            ))),
        }
}
