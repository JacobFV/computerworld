//! A bounded synthetic shell. Lexical quoting is resolved before operators; all
//! commands act only on the computer substrate and explicit network adapter.
use crate::{CommandResult, Computer, ShellHost};
use std::collections::BTreeMap;
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
        if ch == ' ' || ch == '\t' || ";\n|&<>".contains(ch) {
            if word {
                out.push(Token::Word(std::mem::take(&mut parts)));
                word = false;
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
            let mut op = ch.to_string();
            if i + 1 < chars.len() && ((ch == '|' || ch == '&' || ch == '>') && chars[i + 1] == ch)
            {
                op.push(ch);
                i += 1;
            }
            if ch == '>' {
                if let Some(Token::Word(v)) = out.last() {
                    if v.len() == 1 && v[0].0 == "2" {
                        out.pop();
                        op = format!("2{op}");
                    }
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
            && !" \t\n;|&<>\"'".contains(chars[i])
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
pub fn execute(
    c: &mut Computer,
    source: &str,
    tick: u64,
    host: &mut dyn ShellHost,
) -> CommandResult {
    execute_inner(c, source, tick, host, 0)
}
fn execute_inner(
    c: &mut Computer,
    source: &str,
    tick: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> CommandResult {
    if depth > 32 {
        return CommandResult::error("shell: execution nesting exceeds 32");
    }
    if source.len() > 65536 {
        return CommandResult::error("shell: command exceeds 64 KiB limit");
    }
    let tokens = match lex(source, c.dialect == "powershell") {
        Ok(v) => v,
        Err(e) => return CommandResult::error(format!("shell: {e}\n")),
    };
    if matches!(tokens.last(),Some(Token::Op(op)) if op=="&&"||op=="||"||op=="|") {
        return CommandResult::error("shell: missing command after operator");
    }
    if matches!(tokens.first(),Some(Token::Op(op)) if op=="&&"||op=="||"||op=="|") {
        return CommandResult::error("shell: unexpected operator");
    }
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
    let mut pos = 0;
    let mut previous = 0;
    let mut gate = String::new();
    while pos < tokens.len() {
        let allowed = match gate.as_str() {
            "&&" => previous == 0,
            "||" => previous != 0,
            _ => true,
        };
        let mut input = String::new();
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
                let r = run_tokens(c, &tokens[start..pos], &input, tick, host, depth, previous);
                previous = r.exit_code;
                total.stderr.push_str(&r.stderr);
                input = r.stdout;
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
    total.exit_code = previous;
    total.pid = pid;
    let _ = c.processes.exit(pid, previous, tick);
    host.cleanup_process(pid);
    total
}
fn run_tokens(
    c: &mut Computer,
    tokens: &[Token],
    stdin: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
    status: i32,
) -> CommandResult {
    let mut args = vec![];
    let mut redirects = vec![];
    let mut input = stdin.to_string();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Op(op) => {
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
                    Err(e) => return CommandResult::error(e),
                };
                if op == "<<" {
                    input = path;
                } else if op == "<" {
                    match c.vfs.read_as(&path, &c.user) {
                        Ok(v) => input = String::from_utf8_lossy(&v).into_owned(),
                        Err(e) => return CommandResult::error(e.to_string()),
                    }
                } else {
                    redirects.push((op.clone(), path));
                }
            }
            v => match word(v, c, status, t, host, depth) {
                Ok(s) => {
                    let glob = matches!(v,Token::Word(parts) if parts.iter().any(|(p,f)| f&2!=0 && (p.contains('*')||p.contains('?'))));
                    if glob {
                        let paths = glob_paths(c, &s);
                        if paths.is_empty() {
                            args.push(s)
                        } else {
                            args.extend(paths)
                        }
                    } else {
                        args.push(s)
                    }
                }
                Err(e) => return CommandResult::error(e),
            },
        }
        i += 1;
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
    } else {
        match run(c, &args, &input, t, host, depth) {
            Ok(s) => CommandResult::success(s),
            Err(e) => CommandResult::error(if e.is_empty() {
                String::new()
            } else {
                format!("{}: {e}\n", args[0])
            }),
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
    for (op, path) in redirects {
        let data = if op.starts_with('&') {
            format!(
                "{}{}",
                std::mem::take(&mut result.stdout),
                std::mem::take(&mut result.stderr)
            )
        } else if op.starts_with('2') {
            std::mem::take(&mut result.stderr)
        } else {
            std::mem::take(&mut result.stdout)
        };
        let written = if op.ends_with(">>") {
            append(c, &path, data.as_bytes(), t)
        } else {
            c.vfs.write_as(&path, data.as_bytes(), &c.user, t)
        };
        if let Err(e) = written {
            result.stderr.push_str(&format!("{e}\n"));
            result.exit_code = 1;
        }
    }
    result
}
fn run(
    c: &mut Computer,
    a: &[String],
    input: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, String> {
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
        return Err(format!("command not found: {}", a[0]));
    }
    let args = &a[1..];
    let required = |i: usize| {
        args.get(i)
            .map(String::as_str)
            .ok_or_else(|| "missing operand".to_string())
    };
    let err = |e: crate::VfsError| e.to_string();
    match cmd.as_str() {
        "true" => Ok(String::new()),
        "false" => Err(String::new()),
        "echo" | "write-output" => {
            let no = args.first().is_some_and(|s| s == "-n");
            Ok(format!(
                "{}{}",
                args[usize::from(no)..].join(" "),
                if no { "" } else { "\n" }
            ))
        }
        "printf" => {
            let mut format = required(0)?.replace("\\n", "\n").replace("\\t", "\t");
            for arg in &args[1..] {
                if let Some(p) = format.find("%s").or_else(|| format.find("%d")) {
                    format.replace_range(p..p + 2, arg);
                }
            }
            Ok(format.replace("%%", "%"))
        }
        "pwd" | "get-location" => Ok(format!("{}\n", c.cwd)),
        "whoami" => Ok(format!("{}\n", c.user)),
        "hostname" => Ok(format!("{}\n", c.id)),
        "uname" => Ok(format!("{}\n", c.os_family)),
        "date" => Ok(format!("{t}\n")),
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
        "env" | "printenv" => Ok(if let Some(k) = args.first() {
            format!("{}\n", c.env.get(k).cloned().unwrap_or_default())
        } else {
            c.env.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
        }),
        "export" => {
            for s in args {
                if let Some((k, v)) = s.split_once('=') {
                    c.env.insert(k.into(), v.into());
                }
            }
            Ok(String::new())
        }
        "unset" => {
            for s in args {
                c.env.remove(s);
            }
            Ok(String::new())
        }
        "ls" | "dir" | "get-childitem" => {
            let path = args
                .iter()
                .find(|s| !s.starts_with('-'))
                .map(String::as_str)
                .unwrap_or(".");
            Ok(c.vfs
                .list(&c.resolve(path))
                .map_err(err)?
                .into_iter()
                .map(|p| format!("{p}\n"))
                .collect())
        }
        "cat" | "type" | "get-content" => {
            if args.is_empty() {
                return Ok(input.into());
            }
            let mut out = String::new();
            for path in args {
                out.push_str(&String::from_utf8_lossy(
                    &c.vfs.read_as(&c.resolve(path), &c.user).map_err(err)?,
                ));
            }
            Ok(out)
        }
        "touch" => {
            for p in args {
                let p = c.resolve(p);
                if c.vfs.read_as(&p, &c.user).is_err() {
                    c.vfs.write_as(&p, b"", &c.user, t).map_err(err)?;
                }
            }
            Ok(String::new())
        }
        "mkdir" | "md" => {
            for p in args.iter().filter(|s| !s.starts_with('-')) {
                c.vfs.mkdir_all_as(&c.resolve(p), &c.user, t).map_err(err)?;
            }
            Ok(String::new())
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
            let from = c.resolve(required(0)?);
            let mut to = c.resolve(required(1)?);
            if c.vfs.list_as(&to, &c.user).is_ok() {
                to = format!("{to}/{}", from.rsplit('/').next().unwrap_or("file"));
            }
            let bytes = c.vfs.read_as(&from, &c.user).map_err(err)?;
            c.vfs.write_as(&to, &bytes, &c.user, t).map_err(err)?;
            Ok(String::new())
        }
        "mv" | "move-item" => {
            c.vfs
                .rename_as(&c.resolve(required(0)?), &c.resolve(required(1)?), &c.user)
                .map_err(err)?;
            Ok(String::new())
        }
        "rm" | "remove-item" | "rmdir" => {
            let recursive = args
                .iter()
                .any(|s| s == "-r" || s == "-rf" || s == "-Recurse");
            let force = args.iter().any(|s| s == "-f" || s == "-rf");
            for p in args.iter().filter(|s| !s.starts_with('-')) {
                if let Err(e) = c.vfs.remove_as(&c.resolve(p), recursive, &c.user) {
                    if !force {
                        return Err(e.to_string());
                    }
                }
            }
            Ok(String::new())
        }
        "chmod" => {
            let mode = u16::from_str_radix(required(0)?, 8).map_err(|_| "invalid octal mode")?;
            for p in &args[1..] {
                c.vfs.chmod_as(&c.resolve(p), mode, &c.user).map_err(err)?;
            }
            Ok(String::new())
        }
        "ln" => {
            if args.first().is_some_and(|s| s == "-s") {
                c.vfs
                    .symlink_as(required(1)?, &c.resolve(required(2)?), &c.user, t)
                    .map_err(err)?;
            } else {
                c.vfs
                    .hard_link_as(&c.resolve(required(0)?), &c.resolve(required(1)?), &c.user)
                    .map_err(err)?;
            }
            Ok(String::new())
        }
        "grep" | "select-string" => {
            let invert = args.iter().any(|s| s == "-v");
            let insensitive = args.iter().any(|s| s == "-i");
            let vals: Vec<_> = args.iter().filter(|s| !s.starts_with('-')).collect();
            let pattern = vals.first().ok_or("missing pattern")?;
            let mut text = input.to_string();
            if vals.len() > 1 {
                text.clear();
                for p in &vals[1..] {
                    text.push_str(&String::from_utf8_lossy(
                        &c.vfs.read_as(&c.resolve(p), &c.user).map_err(err)?,
                    ));
                }
            }
            let fixed = args.iter().any(|s| s == "-F");
            let expression = if fixed {
                regex::escape(pattern)
            } else {
                pattern.to_string()
            };
            let regex = regex::RegexBuilder::new(&expression)
                .case_insensitive(insensitive)
                .build()
                .map_err(|e| e.to_string())?;
            let mut out = String::new();
            for l in text.lines() {
                let matched = regex.is_match(l);
                if matched != invert {
                    out.push_str(l);
                    out.push('\n');
                }
            }
            if out.is_empty() {
                return Err(String::new());
            }
            Ok(out)
        }
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
                [flag, p] if flag == "-e" => c.vfs.stat(&c.resolve(p)).is_ok(),
                [flag, p] if flag == "-f" => c.vfs.read_as(&c.resolve(p), &c.user).is_ok(),
                [flag, p] if flag == "-d" => c.vfs.list_as(&c.resolve(p), &c.user).is_ok(),
                [flag, v] if flag == "-n" => !v.is_empty(),
                [flag, v] if flag == "-z" => v.is_empty(),
                [left, op, right] => match op.as_str() {
                    "=" | "==" => left == right,
                    "!=" => left != right,
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
                    _ => return Err("unsupported test operator".into()),
                },
                [v] => {
                    if cmd == "test-path" {
                        c.vfs.stat(&c.resolve(v)).is_ok()
                    } else {
                        !v.is_empty()
                    }
                }
                [] => false,
                _ => return Err("unsupported test expression".into()),
            };
            if cmd == "test-path" {
                Ok(format!("{}\n", if yes { "True" } else { "False" }))
            } else if yes {
                Ok(String::new())
            } else {
                Err(String::new())
            }
        }
        "stat" => {
            let path = c.resolve(required(0)?);
            c.vfs
                .check_access(&path, &c.user, true, false, false)
                .map_err(err)?;
            serde_json::to_string_pretty(&c.vfs.stat(&path).map_err(err)?)
                .map(|s| s + "\n")
                .map_err(|e| e.to_string())
        }
        "sed" => {
            let inplace = args.first().is_some_and(|s| s == "-i");
            let expression = required(usize::from(inplace))?;
            let mut chars = expression.chars();
            if chars.next() != Some('s') {
                return Err("supported sed syntax: s/PATTERN/REPLACEMENT/[g]".into());
            }
            let delimiter = chars.next().ok_or("missing sed delimiter")?;
            let mut fields = vec![String::new()];
            let mut escaped = false;
            for ch in chars {
                if escaped {
                    if ch != delimiter {
                        fields.last_mut().unwrap().push('\\');
                    }
                    fields.last_mut().unwrap().push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == delimiter {
                    fields.push(String::new());
                } else {
                    fields.last_mut().unwrap().push(ch)
                }
            }
            if fields.len() != 3 {
                return Err("malformed sed substitution".into());
            }
            let regex = regex::Regex::new(&fields[0]).map_err(|e| e.to_string())?;
            let replace = |text: &str| {
                if fields[2].contains('g') {
                    regex.replace_all(text, fields[1].as_str()).into_owned()
                } else {
                    text.split_inclusive('\n')
                        .map(|line| regex.replace(line, fields[1].as_str()).into_owned())
                        .collect::<String>()
                }
            };
            let files = &args[usize::from(inplace) + 1..];
            if files.is_empty() {
                if inplace {
                    return Err("sed -i requires file".into());
                }
                return Ok(replace(input));
            }
            let mut out = String::new();
            for file in files {
                let path = c.resolve(file);
                let text = String::from_utf8_lossy(&c.vfs.read_as(&path, &c.user).map_err(err)?)
                    .into_owned();
                let changed = replace(&text);
                if inplace {
                    c.vfs
                        .write_as(&path, changed.as_bytes(), &c.user, t)
                        .map_err(err)?;
                } else {
                    out.push_str(&changed)
                }
            }
            Ok(out)
        }
        "tr" => {
            let delete = args.first().is_some_and(|s| s == "-d");
            let from = character_set(required(usize::from(delete))?);
            let to = if delete {
                vec![]
            } else {
                character_set(required(1)?)
            };
            if !delete && to.is_empty() {
                return Err("empty translation set".into());
            }
            Ok(input
                .chars()
                .filter_map(|ch| {
                    if let Some(i) = from.iter().position(|c| *c == ch) {
                        if delete {
                            None
                        } else {
                            Some(to[i.min(to.len() - 1)])
                        }
                    } else {
                        Some(ch)
                    }
                })
                .collect())
        }
        "cut" => {
            let mut delimiter = '\t';
            let mut fields = vec![];
            let mut i = 0;
            while i < args.len() {
                if args[i] == "-d" {
                    i += 1;
                    delimiter = required(i)?.chars().next().ok_or("empty delimiter")?
                } else if args[i] == "-f" {
                    i += 1;
                    fields = required(i)?
                        .split(',')
                        .map(|s| s.parse::<usize>().map_err(|_| "invalid field"))
                        .collect::<Result<Vec<_>, _>>()?;
                } else {
                    return Err("cut supports -d DELIMITER -f FIELDS over stdin".into());
                }
                i += 1;
            }
            if fields.contains(&0) || fields.is_empty() {
                return Err("fields are numbered from 1".into());
            }
            Ok(input
                .lines()
                .map(|line| {
                    let parts = line.split(delimiter).collect::<Vec<_>>();
                    format!(
                        "{}\n",
                        fields
                            .iter()
                            .filter_map(|n| parts.get(n - 1).copied())
                            .collect::<Vec<_>>()
                            .join(&delimiter.to_string())
                    )
                })
                .collect())
        }
        "head" | "tail" | "wc" | "sort" | "uniq" => {
            let mut n = 10;
            let mut path = None;
            let mut i = 0;
            while i < args.len() {
                if args[i] == "-n" {
                    i += 1;
                    n = args
                        .get(i)
                        .and_then(|s| s.parse().ok())
                        .ok_or("invalid count")?;
                } else if !args[i].starts_with('-') {
                    path = Some(&args[i]);
                }
                i += 1;
            }
            let text = if let Some(p) = path {
                String::from_utf8_lossy(&c.vfs.read_as(&c.resolve(p), &c.user).map_err(err)?)
                    .into_owned()
            } else {
                input.into()
            };
            let mut lines: Vec<_> = text.lines().collect();
            match cmd.as_str() {
                "wc" => Ok(if args.iter().any(|s| s == "-l") {
                    format!("{}\n", text.bytes().filter(|b| *b == b'\n').count())
                } else {
                    format!(
                        "{} {} {}\n",
                        text.lines().count(),
                        text.split_whitespace().count(),
                        text.len()
                    )
                }),
                "head" => Ok(lines
                    .into_iter()
                    .take(n)
                    .map(|l| format!("{l}\n"))
                    .collect()),
                "tail" => {
                    let skip = lines.len().saturating_sub(n);
                    Ok(lines
                        .into_iter()
                        .skip(skip)
                        .map(|l| format!("{l}\n"))
                        .collect())
                }
                "sort" => {
                    lines.sort();
                    if args.iter().any(|s| s == "-r") {
                        lines.reverse();
                    }
                    Ok(lines.into_iter().map(|l| format!("{l}\n")).collect())
                }
                _ => {
                    lines.dedup();
                    Ok(lines.into_iter().map(|l| format!("{l}\n")).collect())
                }
            }
        }
        "find" => {
            let prefix = c.resolve(args.first().map(String::as_str).unwrap_or("."));
            Ok(c.vfs
                .all_files()
                .keys()
                .filter(|p| {
                    p.starts_with(&format!("{}/", prefix.trim_end_matches('/'))) || **p == prefix
                })
                .map(|p| format!("{p}\n"))
                .collect())
        }
        "systemctl" | "service" => {
            let (operation, name) = if cmd == "service" {
                (required(1)?, required(0)?)
            } else {
                (required(0)?, required(1)?)
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
                    Err(format!("{name}: inactive"))
                };
            }
            if operation == "stop" || operation == "restart" {
                if let Some(p) = running.as_ref() {
                    c.processes.signal(p.pid, "TERM", &c.user, t)?;
                    if process_terminated(c, p.pid) {
                        host.cleanup_process(p.pid)
                    } else {
                        return Err(format!("{name}: process did not terminate after TERM"));
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
                        return Err(e);
                    }
                }
            }
            Err("unsupported service operation".into())
        }
        "ps" | "get-process" => serde_json::to_string_pretty(&c.processes.list())
            .map(|s| s + "\n")
            .map_err(|e| e.to_string()),
        "kill" | "stop-process" => {
            let (signal, index) = if required(0)?.starts_with('-') {
                (
                    required(0)?
                        .trim_start_matches('-')
                        .trim_start_matches("SIG"),
                    1,
                )
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
            result.map(|v| v.join("\n") + "\n")
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
                    s if !s.starts_with('-') => url = Some(s.to_string()),
                    _ => {}
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
                return Err(format!("HTTP {}", response.status));
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
                Err(result.stderr)
            }
        }
        "sh" | "bash" => {
            let source = if args.first().is_some_and(|s| s == "-c") {
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
            let r = execute_inner(c, &source, t, host, depth + 1);
            c.env = saved_env;
            c.cwd = saved_cwd;
            if r.exit_code == 0 {
                Ok(r.stdout)
            } else {
                Err(r.stderr)
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
                .ok_or_else(|| format!("command not found: {}", a[0]))?;
            c.vfs
                .check_access(&path, &c.user, true, false, true)
                .map_err(err)?;
            let source = String::from_utf8(c.vfs.read_as(&path, &c.user).map_err(err)?)
                .map_err(|_| "unsupported binary executable")?;
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
                return Err(format!(
                    "{name}: no synthetic program implementation registered"
                ));
            }
            if !source.starts_with("#!/bin/sh")
                && !source.starts_with("#!/bin/bash")
                && !source.starts_with("#!/usr/bin/env sh")
            {
                return Err("unsupported executable interpreter".into());
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
            if r.exit_code == 0 {
                Ok(r.stdout)
            } else {
                Err(r.stderr)
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

fn character_set(value: &str) -> Vec<char> {
    let text = value.replace("\\n", "\n").replace("\\t", "\t");
    let chars: Vec<_> = text.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '-' {
            for n in chars[i] as u32..=chars[i + 2] as u32 {
                if let Some(ch) = char::from_u32(n) {
                    out.push(ch);
                }
            }
            i += 3;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
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
