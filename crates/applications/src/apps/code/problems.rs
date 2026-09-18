//! Diagnostics: what a program run in the terminal actually reported (CPython and Node
//! tracebacks, bash line errors) and what a JSON document's own parser says about it.
//! Nothing here is guessed from source text; a problem exists because a tool said so.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// Absolute path of the file the tool blamed.
    pub path: String,
    /// 1-based, as every tool prints them.
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub severity: Severity,
    /// Who reported it: `python3`, `node`, `bash` or `json`.
    pub source: String,
}

/// Collapse `.` and `..` and join a relative path onto `cwd`.
pub fn resolve(cwd: &str, path: &str) -> String {
    let joined = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{}/{path}", cwd.trim_end_matches('/'))
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

fn digits(s: &str) -> Option<usize> {
    let n: String = s.chars().take_while(char::is_ascii_digit).collect();
    n.parse().ok()
}

/// Problems in what `program` printed when it ran in `cwd`.
pub fn from_output(program: &str, stdout: &str, stderr: &str, cwd: &str) -> Vec<Problem> {
    let name = program.rsplit('/').next().unwrap_or(program);
    let text = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    match name {
        "python" | "python3" => python(text, cwd, name),
        "node" => node(text, cwd),
        "bash" | "sh" => bash(text, cwd, name),
        _ => {
            // A command typed at the prompt is recognised by what it printed.
            let mut out = python(text, cwd, "python3");
            if out.is_empty() {
                out = node(text, cwd);
            }
            out
        }
    }
}

/// CPython: the innermost `File "…", line N` frame, and the final exception line.
fn python(text: &str, cwd: &str, source: &str) -> Vec<Problem> {
    let mut frame: Option<(String, usize)> = None;
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("File \"") else {
            continue;
        };
        let Some((path, tail)) = rest.split_once('"') else {
            continue;
        };
        let Some(n) = tail.strip_prefix(", line ").and_then(digits) else {
            continue;
        };
        if path.starts_with('<') {
            continue;
        }
        frame = Some((path.to_owned(), n));
    }
    let Some((path, line)) = frame else {
        return vec![];
    };
    let message = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty() && !l.starts_with(' ') && !l.starts_with('\t'))
        .unwrap_or("error")
        .trim()
        .to_owned();
    // Carets mark the column under the quoted source line, which CPython indents by
    // four spaces.
    let col = text
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('^') && l.trim().chars().all(|c| c == '^' || c == '~'))
        .map_or(1, |l| {
            (l.len() - l.trim_start().len()).saturating_sub(4) + 1
        });
    vec![Problem {
        path: resolve(cwd, &path),
        line,
        col,
        message,
        severity: Severity::Error,
        source: source.into(),
    }]
}

/// Node: the `file.js:N` header above the source excerpt, or the first stack frame, and
/// the `SomethingError: …` line.
fn node(text: &str, cwd: &str) -> Vec<Problem> {
    let script = |s: &str| {
        let s = s.trim();
        let (path, n) = s.rsplit_once(':')?;
        let n = digits(n).filter(|_| n.bytes().all(|b| b.is_ascii_digit()))?;
        (path.ends_with(".js") || path.ends_with(".mjs") || path.ends_with(".cjs"))
            .then(|| (path.to_owned(), n))
    };
    let frame = |s: &str| -> Option<(String, usize, usize)> {
        let s = s.trim().strip_prefix("at ")?;
        let inner = match (s.rfind('('), s.ends_with(')')) {
            (Some(open), true) => &s[open + 1..s.len() - 1],
            _ => s,
        };
        let (rest, col) = inner.rsplit_once(':')?;
        let (path, line) = rest.rsplit_once(':')?;
        if path.starts_with("node:") || !path.contains('/') && !path.ends_with("js") {
            return None;
        }
        Some((path.to_owned(), digits(line)?, digits(col)?))
    };
    let header = text.lines().find_map(script);
    let stack = text.lines().find_map(frame);
    let (path, line, col) = match (header, stack) {
        (Some((p, l)), Some((sp, sl, sc))) if sp == p && sl == l => (p, l, sc),
        (Some((p, l)), _) => (p, l, 1),
        (None, Some(frame)) => frame,
        (None, None) => return vec![],
    };
    let message = text
        .lines()
        .find(|l| {
            let head = l.split(':').next().unwrap_or("");
            !l.starts_with(' ')
                && l.contains(": ")
                && head.ends_with("Error")
                && head
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '[' || c == ']' || c == ' ')
        })
        .unwrap_or("Uncaught exception")
        .trim()
        .to_owned();
    vec![Problem {
        path: resolve(cwd, &path),
        line,
        col,
        message,
        severity: Severity::Error,
        source: "node".into(),
    }]
}

/// bash: `script.sh: line N: message`, one per line reported.
fn bash(text: &str, cwd: &str, source: &str) -> Vec<Problem> {
    text.lines()
        .filter_map(|l| {
            let (path, rest) = l.split_once(": line ")?;
            let (n, message) = rest.split_once(": ")?;
            Some(Problem {
                path: resolve(cwd, path),
                line: n.trim().parse().ok()?,
                col: 1,
                message: message.trim().to_owned(),
                severity: Severity::Error,
                source: source.into(),
            })
        })
        .collect()
}

/// Strip `//` and `/* */` comments outside strings, for JSON with Comments.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                out.push(if b[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            out.push_str("  ");
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}
/// JSON strips trailing commas too when comments are allowed.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_str = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            if c == '"' && chars.get(i.wrapping_sub(1)) != Some(&'\\') {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}' | ']')) {
                out.push(' ');
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// What the JSON parser says about `text`. `comments` accepts JSON with Comments, as
/// VS Code does for `settings.json` and `.jsonc` files.
pub fn json(path: &str, text: &str, comments: bool) -> Vec<Problem> {
    if text.trim().is_empty() {
        return vec![];
    }
    let source = if comments {
        strip_trailing_commas(&strip_comments(text))
    } else {
        text.to_owned()
    };
    match serde_json::from_str::<serde_json::Value>(&source) {
        Ok(_) => vec![],
        Err(e) => {
            let full = e.to_string();
            let message = full.split(" at line ").next().unwrap_or(&full).to_owned();
            let mut message = message;
            if let Some(first) = message.get(..1) {
                message = format!("{}{}", first.to_uppercase(), &message[1..]);
            }
            vec![Problem {
                path: path.to_owned(),
                line: e.line().max(1),
                col: e.column().max(1),
                message,
                severity: Severity::Error,
                source: "json".into(),
            }]
        }
    }
}
/// Parse JSON with Comments into a value (settings.json).
pub fn parse_jsonc(text: &str) -> Option<serde_json::Value> {
    serde_json::from_str(&strip_trailing_commas(&strip_comments(text))).ok()
}

/// Fuzzy subsequence match, VS Code style: case-insensitive, rewarding consecutive
/// characters and word starts. Returns the score and matched character indices.
pub fn fuzzy(query: &str, target: &str) -> Option<(i32, Vec<usize>)> {
    let q: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if q.is_empty() {
        return Some((0, vec![]));
    }
    let t: Vec<char> = target.chars().collect();
    let lower: Vec<char> = t
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let mut hits = Vec::with_capacity(q.len());
    let mut score = 0;
    let mut qi = 0;
    for (i, c) in lower.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let start = i == 0
                || matches!(t[i - 1], '/' | '\\' | '_' | '-' | '.' | ' ' | ':')
                || (t[i].is_uppercase() && t[i - 1].is_lowercase());
            score += 1;
            if start {
                score += 8;
            }
            if hits.last() == Some(&(i.wrapping_sub(1))) {
                score += 5;
            }
            if t[i] == query.chars().nth(qi).unwrap_or(' ') {
                score += 1;
            }
            hits.push(i);
            qi += 1;
        }
    }
    (qi == q.len()).then(|| (score - (t.len() as i32 / 8), hits))
}
/// Match a workspace path, preferring hits in the file name as Quick Open does.
pub fn fuzzy_path(query: &str, rel: &str) -> Option<i32> {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    if let Some((score, _)) = fuzzy(query, base) {
        return Some(score + 100);
    }
    fuzzy(query, rel).map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpython_tracebacks_name_the_innermost_frame_and_the_exception() {
        let err = "Traceback (most recent call last):\n  File \"/home/a/p/main.py\", line 9, in <module>\n    main()\n  File \"/home/a/p/util.py\", line 3, in main\n    print(x)\n          ^\nNameError: name 'x' is not defined\n";
        let p = from_output("python3", "", err, "/home/a/p");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].path, "/home/a/p/util.py");
        assert_eq!(p[0].line, 3);
        assert_eq!(p[0].message, "NameError: name 'x' is not defined");
        // Relative paths resolve against the directory the program ran in.
        let err = "  File \"src/../app.py\", line 2\n    def f(\n         ^\nSyntaxError: '(' was never closed\n";
        let p = from_output("python3", "", err, "/w");
        assert_eq!((p[0].path.as_str(), p[0].line), ("/w/app.py", 2));
        assert!(p[0].message.starts_with("SyntaxError"));
        assert!(from_output("python3", "ok\n", "", "/w").is_empty());
    }
    #[test]
    fn node_errors_use_the_header_line_and_stack_column() {
        let err = "/w/app.js:4\n  foo();\n  ^\n\nReferenceError: foo is not defined\n    at Object.<anonymous> (/w/app.js:4:3)\n    at node:internal/main:1:1\n";
        let p = from_output("node", "", err, "/w");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].line, p[0].col), (4, 3));
        assert_eq!(p[0].message, "ReferenceError: foo is not defined");
        let only_stack = "Error: boom\n    at run (/w/lib.js:10:5)\n";
        let p = from_output("node", "", only_stack, "/w");
        assert_eq!(
            (p[0].path.as_str(), p[0].line, p[0].col),
            ("/w/lib.js", 10, 5)
        );
    }
    #[test]
    fn bash_line_errors_and_command_not_found_are_distinct() {
        let p = from_output(
            "bash",
            "",
            "run.sh: line 2: nosuch: command not found\n",
            "/w",
        );
        assert_eq!((p[0].path.as_str(), p[0].line), ("/w/run.sh", 2));
        assert_eq!(p[0].message, "nosuch: command not found");
        // A shell that cannot find the interpreter blames no file.
        assert!(from_output("python3", "", "python3: command not found\n", "/w").is_empty());
    }
    #[test]
    fn json_problems_come_from_the_parser() {
        let p = json("/w/a.json", "{\n  \"a\": 1,\n  \"b\" 2\n}", false);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].line, 3);
        assert!(p[0].message.starts_with("Expected"), "{}", p[0].message);
        assert!(json("/w/a.json", "{\"a\": [1, 2]}", false).is_empty());
        // Comments are an error in JSON and fine in JSON with Comments.
        let commented = "{\n  // theme\n  \"x\": 1,\n}";
        assert_eq!(json("/w/a.json", commented, false).len(), 1);
        assert!(json("/w/settings.json", commented, true).is_empty());
        assert_eq!(parse_jsonc(commented).unwrap()["x"], 1);
    }
    #[test]
    fn fuzzy_matching_prefers_word_starts_and_file_names() {
        assert!(fuzzy("mpy", "main.py").is_some());
        assert!(fuzzy("xyz", "main.py").is_none());
        let (_, hits) = fuzzy("tt", "Toggle Terminal").unwrap();
        assert_eq!(hits, vec![0, 7]);
        assert!(fuzzy("tog term", "View: Toggle Terminal").is_some());
        assert!(
            fuzzy_path("app", "src/app.js").unwrap()
                > fuzzy_path("app", "apps/index.js").unwrap_or(0) - 200
        );
        assert!(
            fuzzy_path("main", "src/main.py").unwrap()
                > fuzzy_path("main", "main/other.txt").unwrap()
        );
    }
}
