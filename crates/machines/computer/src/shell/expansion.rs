use super::*;

pub(super) fn expand_commands(
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
pub(super) fn arithmetic(text: &str, c: &Computer) -> Result<i64, String> {
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
/// A `[...]` bracket expression starting at `p[start]`. Returns the index just past the
/// closing `]` and whether `ch` is in the set. `[!…]` and `[^…]` negate, a `]` first in
/// the set is a literal, and `-` first or last is a literal. An unterminated `[` is not
/// a bracket expression at all, which is why this returns `None` for it.
pub(super) fn bracket(p: &[char], start: usize, ch: char) -> Option<(usize, bool)> {
    let mut i = start + 1;
    let negated = matches!(p.get(i), Some('!' | '^'));
    if negated {
        i += 1;
    }
    let first = i;
    let mut hit = false;
    while i < p.len() {
        if p[i] == ']' && i > first {
            return Some((i + 1, hit != negated));
        }
        // `a-z`, but a `-` with nothing after it (or just before `]`) is a literal.
        if p.get(i + 1) == Some(&'-') && p.get(i + 2).is_some_and(|c| *c != ']') {
            let (lo, hi) = (p[i], p[i + 2]);
            if lo <= ch && ch <= hi {
                hit = true;
            }
            i += 3;
            continue;
        }
        if p[i] == ch {
            hit = true;
        }
        i += 1;
    }
    None
}
/// Wildcards match one path component: `*`, `?` and `[...]` classes. No host
/// directories are consulted.
pub(crate) fn wildcard(pattern: &str, value: &str) -> bool {
    let p: Vec<_> = pattern.chars().collect();
    let v: Vec<_> = value.chars().collect();
    // One pattern atom against one character: the next pattern index, or None.
    let atom = |i: usize, ch: char| -> Option<usize> {
        match p[i] {
            '?' => Some(i + 1),
            '[' => match bracket(&p, i, ch) {
                Some((next, true)) => Some(next),
                Some((_, false)) => None,
                None if ch == '[' => Some(i + 1),
                None => None,
            },
            c if c == ch => Some(i + 1),
            _ => None,
        }
    };
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < v.len() {
        if i < p.len() && p[i] == '*' {
            star = Some(i);
            i += 1;
            mark = j;
        } else if let Some(next) = if i < p.len() { atom(i, v[j]) } else { None } {
            i = next;
            j += 1;
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
/// True when the word holds a pattern character a glob would act on.
pub(super) fn has_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}
/// Brace expansion: `{a,b,c}` and the numeric range `{1..5}` (with an optional step,
/// `{1..9..2}`), nested and repeated, left to right. It runs before globbing, as bash
/// runs it, and it is pure text: a brace that expands to nothing still produces a word.
/// A brace with no comma and no range is left alone, so `${…}` leftovers and a literal
/// `{}` (the one `find -exec` wants) survive untouched.
pub(super) fn brace_expand(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    // The first unescaped `{` with a matching `}` at the same depth.
    let mut depth = 0;
    let mut open = None;
    for (i, ch) in chars.iter().enumerate() {
        match ch {
            '{' => {
                if depth == 0 {
                    open = Some(i);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let (start, end) = (open.unwrap(), i);
                    let body: String = chars[start + 1..end].iter().collect();
                    let prefix: String = chars[..start].iter().collect();
                    let suffix: String = chars[end + 1..].iter().collect();
                    let Some(items) = brace_items(&body) else {
                        // Not an expansion; keep scanning after this brace.
                        open = None;
                        continue;
                    };
                    let mut out = Vec::new();
                    for item in items {
                        for tail in brace_expand(&format!("{prefix}{item}{suffix}")) {
                            out.push(tail);
                        }
                    }
                    return out;
                }
            }
            _ => {}
        }
    }
    vec![word.to_string()]
}
/// The alternatives a brace body stands for, or `None` when it is not an expansion.
pub(super) fn brace_items(body: &str) -> Option<Vec<String>> {
    if let Some((lo, rest)) = body.split_once("..") {
        let (hi, step) = match rest.split_once("..") {
            Some((h, s)) => (h, s.parse::<i64>().ok()?),
            None => (rest, 1),
        };
        if step == 0 {
            return None;
        }
        if let (Ok(a), Ok(b)) = (lo.parse::<i64>(), hi.parse::<i64>()) {
            let step = step.abs() * if a <= b { 1 } else { -1 };
            let mut out = Vec::new();
            let mut v = a;
            while (step > 0 && v <= b) || (step < 0 && v >= b) {
                out.push(v.to_string());
                v += step;
                if out.len() > 10_000 {
                    return None;
                }
            }
            return Some(out);
        }
        // `{a..e}`: a character range.
        let (a, b) = (lo.chars().next()?, hi.chars().next()?);
        if lo.chars().count() == 1 && hi.chars().count() == 1 && step.abs() == 1 {
            let (a, b) = (a as u32, b as u32);
            let range: Vec<String> = if a <= b {
                (a..=b)
                    .filter_map(char::from_u32)
                    .map(String::from)
                    .collect()
            } else {
                (b..=a)
                    .rev()
                    .filter_map(char::from_u32)
                    .map(String::from)
                    .collect()
            };
            return Some(range);
        }
        return None;
    }
    // Split on commas at brace depth zero.
    let mut items = vec![String::new()];
    let mut depth = 0;
    for ch in body.chars() {
        match ch {
            '{' => {
                depth += 1;
                items.last_mut().unwrap().push(ch);
            }
            '}' => {
                depth -= 1;
                items.last_mut().unwrap().push(ch);
            }
            ',' if depth == 0 => items.push(String::new()),
            _ => items.last_mut().unwrap().push(ch),
        }
    }
    (items.len() > 1).then_some(items)
}
pub(super) fn glob_paths(c: &Computer, pattern: &str) -> Vec<String> {
    let abs = c.resolve(pattern);
    let components: Vec<_> = abs.split('/').filter(|s| !s.is_empty()).collect();
    let mut paths = vec![String::new()];
    for component in components {
        let mut next = vec![];
        for parent in paths {
            if has_glob(component) {
                if let Ok(mut entries) = c
                    .vfs
                    .list_as(if parent.is_empty() { "/" } else { &parent }, &c.user)
                {
                    entries.sort();
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
        // `lstat`, not `stat`: a dangling symlink is still a name in the directory,
        // and `ls`/`rm` must see it.
        .filter(|p| c.vfs.lstat(p).is_ok())
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
pub(super) fn positional(c: &Computer) -> (usize, String) {
    let n: usize = c.env.get("#").and_then(|v| v.parse().ok()).unwrap_or(0);
    let joined = (1..=n)
        .filter_map(|i| c.env.get(&i.to_string()))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (n, joined)
}
pub(super) fn expand(text: &str, c: &Computer, status: i32) -> String {
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
pub(super) fn word(
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
pub(super) fn word_fields(
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
