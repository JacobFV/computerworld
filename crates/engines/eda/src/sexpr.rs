//! KiCad's S-expression file syntax: lists, bare symbols and double-quoted strings with
//! backslash escapes. The writer indents the way KiCad 8 does — one tab per level, short
//! lists of atoms kept on one line — so the files read like the real thing.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sexp {
    List(Vec<Sexp>),
    /// A bare token: a keyword or a number.
    Atom(String),
    /// A quoted string.
    Str(String),
}

impl Sexp {
    pub fn list(items: Vec<Sexp>) -> Self {
        Self::List(items)
    }
    pub fn atom(s: impl Into<String>) -> Self {
        Self::Atom(s.into())
    }
    pub fn string(s: impl Into<String>) -> Self {
        Self::Str(s.into())
    }
    /// `(head args…)`.
    pub fn node(head: &str, args: Vec<Sexp>) -> Self {
        let mut v = Vec::with_capacity(args.len() + 1);
        v.push(Self::atom(head));
        v.extend(args);
        Self::List(v)
    }
    pub fn head(&self) -> Option<&str> {
        match self {
            Self::List(items) => match items.first() {
                Some(Self::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }
    pub fn items(&self) -> &[Sexp] {
        match self {
            Self::List(items) => items,
            _ => &[],
        }
    }
    /// The text of an atom or string.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Atom(a) | Self::Str(a) => Some(a),
            Self::List(_) => None,
        }
    }
    /// Child lists whose head is `name`.
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items()
            .iter()
            .skip(1)
            .filter(move |c| c.head() == Some(name))
    }
    pub fn child(&self, name: &str) -> Option<&Sexp> {
        self.items().iter().skip(1).find(|c| c.head() == Some(name))
    }
    /// The `i`-th argument (after the head) as text.
    pub fn arg(&self, i: usize) -> Option<&str> {
        self.items().get(i + 1).and_then(Sexp::text)
    }
    /// `(name value)` child's first argument.
    pub fn value_of(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.arg(0))
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }
    fn is_flat(&self) -> bool {
        match self {
            Self::List(items) => {
                items.iter().all(|i| !matches!(i, Self::List(_)))
                    || (items.len() <= 4
                        && items
                            .iter()
                            .all(|i| i.items().iter().all(|j| !matches!(j, Self::List(_)))))
                        && self.flat_len() < 80
            }
            _ => true,
        }
    }
    fn flat_len(&self) -> usize {
        match self {
            Self::List(items) => 2 + items.iter().map(|i| i.flat_len() + 1).sum::<usize>(),
            Self::Atom(a) => a.len(),
            Self::Str(s) => s.len() + 2,
        }
    }
    fn write_flat(&self, out: &mut String) {
        match self {
            Self::List(items) => {
                out.push('(');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    item.write_flat(out);
                }
                out.push(')');
            }
            Self::Atom(a) => out.push_str(a),
            Self::Str(s) => quote(s, out),
        }
    }
    fn write(&self, out: &mut String, depth: usize) {
        match self {
            Self::List(items) if !self.is_flat() => {
                out.push('(');
                // The head and leading atoms stay on the opening line.
                let mut i = 0;
                while i < items.len() && !matches!(items[i], Self::List(_)) {
                    if i > 0 {
                        out.push(' ');
                    }
                    items[i].write_flat(out);
                    i += 1;
                }
                for item in &items[i..] {
                    out.push('\n');
                    for _ in 0..=depth {
                        out.push('\t');
                    }
                    item.write(out, depth + 1);
                }
                out.push('\n');
                for _ in 0..depth {
                    out.push('\t');
                }
                out.push(')');
            }
            other => other.write_flat(out),
        }
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parse one S-expression document.
pub fn parse(text: &str) -> Result<Sexp, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut line = 1;
    let mut stack: Vec<Vec<Sexp>> = Vec::new();
    let mut done: Option<Sexp> = None;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '(' => {
                if done.is_some() {
                    return Err(format!("line {line}: text after the end of the document"));
                }
                stack.push(Vec::new());
                i += 1;
            }
            ')' => {
                let list = stack
                    .pop()
                    .ok_or_else(|| format!("line {line}: unbalanced ')'"))?;
                let node = Sexp::List(list);
                match stack.last_mut() {
                    Some(parent) => parent.push(node),
                    None => done = Some(node),
                }
                i += 1;
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    let Some(&ch) = chars.get(i) else {
                        return Err(format!("line {line}: unterminated string"));
                    };
                    i += 1;
                    match ch {
                        '"' => break,
                        '\\' => {
                            let esc = chars
                                .get(i)
                                .copied()
                                .ok_or_else(|| format!("line {line}: unterminated escape"))?;
                            i += 1;
                            s.push(match esc {
                                'n' => '\n',
                                't' => '\t',
                                other => other,
                            });
                        }
                        '\n' => {
                            line += 1;
                            s.push('\n');
                        }
                        other => s.push(other),
                    }
                }
                stack
                    .last_mut()
                    .ok_or_else(|| format!("line {line}: string outside a list"))?
                    .push(Sexp::Str(s));
            }
            _ => {
                let start = i;
                while i < chars.len()
                    && !chars[i].is_whitespace()
                    && !matches!(chars[i], '(' | ')' | '"')
                {
                    i += 1;
                }
                let atom: String = chars[start..i].iter().collect();
                stack
                    .last_mut()
                    .ok_or_else(|| format!("line {line}: \"{atom}\" outside a list"))?
                    .push(Sexp::Atom(atom));
            }
        }
    }
    if !stack.is_empty() {
        return Err("unbalanced '(': the file ends inside a list".into());
    }
    done.ok_or_else(|| "the file is empty".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_nested_lists_and_escaped_strings() {
        let text = r#"(kicad_sch (version 20231120) (title_block (title "A \"quoted\" \\ title")) (wire (pts (xy 1 2) (xy 3.5 -4))))"#;
        let doc = parse(text).unwrap();
        assert_eq!(doc.head(), Some("kicad_sch"));
        assert_eq!(doc.value_of("version"), Some("20231120"));
        assert_eq!(
            doc.child("title_block").unwrap().value_of("title"),
            Some("A \"quoted\" \\ title")
        );
        let again = parse(&doc.to_text()).unwrap();
        assert_eq!(doc, again);
        assert!(doc.to_text().contains("\n\t(version 20231120)"));
    }
    #[test]
    fn malformed_documents_are_refused() {
        assert!(parse("(a (b)").is_err());
        assert!(parse("(a))").is_err());
        assert!(parse("(a \"open)").is_err());
        assert!(parse("").is_err());
    }
}
