//! A deterministic backtracking regular-expression engine for the simulated
//! Python (`re`) and JavaScript (`RegExp`) runtimes.
//!
//! Patterns compile to a small instruction program run by a backtracking VM with
//! an explicit stack, so long inputs never deepen the Rust call stack. Every
//! execution has a step budget: catastrophic patterns end with [`Exhausted`]
//! instead of hanging the simulator.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Python,
    JavaScript,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    /// Python `re.X`.
    pub verbose: bool,
    /// Python `re.A`: `\w \d \s \b` are ASCII-only.
    pub ascii: bool,
    /// JavaScript `u` flag.
    pub unicode: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub position: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exhausted;

pub type Slots = Vec<Option<(usize, usize)>>;

const STEP_LIMIT: u64 = 20_000_000;

#[derive(Clone, Debug)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

#[derive(Clone, Debug)]
struct Class {
    items: Vec<ClassItem>,
    negated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Assert {
    LineStart,
    TextStart,
    LineEnd,
    /// Python `$` without MULTILINE: end, or before a final newline.
    PyEnd,
    TextEnd,
    WordBoundary,
    NotWordBoundary,
}

#[derive(Clone, Debug)]
enum Node {
    Empty,
    Char(char, bool),
    Any(bool),
    Class(Class, bool),
    Assert(Assert),
    Group(Box<Node>, Option<usize>),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
        greedy: bool,
        possessive: bool,
    },
    Backref(usize, bool),
    NamedBackref(String, bool, usize),
    Look {
        node: Box<Node>,
        ahead: bool,
        negate: bool,
    },
    Atomic(Box<Node>),
}

#[derive(Clone, Debug)]
enum Inst {
    Char(char, bool),
    Any(bool),
    Class(usize, bool),
    Split(usize, usize),
    Jmp(usize),
    Save(usize),
    Assert(Assert),
    Backref(usize, bool),
    Look {
        prog: usize,
        ahead: bool,
        negate: bool,
        width: Option<usize>,
    },
    Atomic(usize),
    Mark(usize),
    Progress(usize),
    Match,
}

#[derive(Clone, Debug)]
pub struct Regex {
    progs: Vec<Vec<Inst>>,
    classes: Vec<Class>,
    ngroups: usize,
    names: Vec<(String, usize)>,
    nregs: usize,
    flags: Flags,
    flavor: Flavor,
    anchored_start: bool,
    first_char: Option<char>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

struct Parser<'a> {
    c: Vec<char>,
    i: usize,
    flavor: Flavor,
    flags: Flags,
    ngroups: usize,
    names: Vec<(String, usize)>,
    pattern: &'a str,
    open_groups: Vec<usize>,
}

fn is_word(c: char, ascii: bool) -> bool {
    if ascii {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        c.is_alphanumeric() || c == '_'
    }
}
fn is_digit(c: char, ascii: bool) -> bool {
    if ascii {
        c.is_ascii_digit()
    } else {
        c.is_ascii_digit() || (c.is_numeric() && c.to_digit(10).is_some())
    }
}
fn is_space(c: char, ascii: bool) -> bool {
    if ascii {
        matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
    } else {
        c.is_whitespace() || matches!(c, '\x1c'..='\x1f')
    }
}

impl<'a> Parser<'a> {
    fn err(&self, msg: impl Into<String>, pos: usize) -> Error {
        let msg = msg.into();
        match self.flavor {
            Flavor::Python => Error {
                message: format!("{msg} at position {pos}"),
                position: pos,
            },
            Flavor::JavaScript => Error {
                message: format!("Invalid regular expression: /{}/: {msg}", self.pattern),
                position: pos,
            },
        }
    }
    fn py(&self) -> bool {
        self.flavor == Flavor::Python
    }
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn skip_verbose(&mut self) {
        if !self.flags.verbose {
            return;
        }
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => self.i += 1,
                Some('#') => {
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn parse(&mut self) -> Result<Node, Error> {
        // Leading global inline flags (Python).
        let node = self.alternation()?;
        if self.i < self.c.len() {
            if self.c[self.i] == ')' {
                return Err(if self.py() {
                    self.err("unbalanced parenthesis", self.i)
                } else {
                    self.err("Unmatched ')'", self.i)
                });
            }
            return Err(self.err("unexpected character", self.i));
        }
        Ok(node)
    }

    fn alternation(&mut self) -> Result<Node, Error> {
        let mut alts = vec![self.concat()?];
        while self.peek() == Some('|') {
            self.i += 1;
            alts.push(self.concat()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Node::Alt(alts)
        })
    }

    fn concat(&mut self) -> Result<Node, Error> {
        let mut items = vec![];
        loop {
            self.skip_verbose();
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            let start = self.i;
            let atom = self.atom()?;
            let atom = match atom {
                Some(a) => a,
                None => continue,
            };
            let node = self.quantifier(atom, start)?;
            items.push(node);
        }
        Ok(match items.len() {
            0 => Node::Empty,
            1 => items.pop().unwrap(),
            _ => Node::Concat(items),
        })
    }

    fn parse_brace(&mut self) -> Option<(u32, Option<u32>)> {
        // At '{'. Returns None (and leaves position) if not a valid quantifier.
        let save = self.i;
        self.i += 1;
        let mut min = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                min.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        let max;
        if self.peek() == Some(',') {
            self.i += 1;
            let mut mx = String::new();
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    mx.push(c);
                    self.i += 1;
                } else {
                    break;
                }
            }
            if self.peek() != Some('}') || (min.is_empty() && (mx.is_empty() || !self.py())) {
                if !(self.peek() == Some('}') && min.is_empty() && self.py()) {
                    self.i = save;
                    return None;
                }
            }
            max = if mx.is_empty() {
                None
            } else {
                Some(mx.parse().unwrap_or(u32::MAX))
            };
        } else {
            if self.peek() != Some('}') || min.is_empty() {
                self.i = save;
                return None;
            }
            max = Some(min.parse().unwrap_or(u32::MAX));
        }
        self.i += 1; // '}'
        let min = if min.is_empty() {
            0
        } else {
            min.parse().unwrap_or(u32::MAX)
        };
        Some((min, max))
    }

    fn quantifier(&mut self, atom: Node, start: usize) -> Result<Node, Error> {
        let mut node = atom;
        let mut quantified = false;
        loop {
            self.skip_verbose();
            let qpos = self.i;
            let (min, max) = match self.peek() {
                Some('*') => {
                    self.i += 1;
                    (0, None)
                }
                Some('+') => {
                    self.i += 1;
                    (1, None)
                }
                Some('?') => {
                    self.i += 1;
                    (0, Some(1))
                }
                Some('{') => match self.parse_brace() {
                    Some(q) => q,
                    None => break,
                },
                _ => break,
            };
            if quantified {
                return Err(if self.py() {
                    self.err("multiple repeat", qpos)
                } else {
                    self.err("Nothing to repeat", qpos)
                });
            }
            if matches!(node, Node::Assert(_) | Node::Empty)
                || matches!(&node, Node::Look { .. } if self.py())
            {
                let _ = start;
                return Err(if self.py() {
                    self.err("nothing to repeat", qpos)
                } else {
                    self.err("Nothing to repeat", qpos)
                });
            }
            if let Some(mx) = max {
                if mx < min {
                    return Err(if self.py() {
                        self.err("min repeat greater than max repeat", qpos + 1)
                    } else {
                        self.err("numbers out of order in {} quantifier", qpos)
                    });
                }
            }
            let mut greedy = true;
            let mut possessive = false;
            if self.peek() == Some('?') {
                self.i += 1;
                greedy = false;
            } else if self.peek() == Some('+') && self.py() {
                self.i += 1;
                possessive = true;
            }
            node = Node::Repeat {
                node: Box::new(node),
                min,
                max,
                greedy,
                possessive,
            };
            quantified = true;
        }
        Ok(node)
    }

    fn atom(&mut self) -> Result<Option<Node>, Error> {
        let start = self.i;
        let c = self.c[self.i];
        self.i += 1;
        let ic = self.flags.ignore_case;
        Ok(Some(match c {
            '.' => Node::Any(self.flags.dot_all),
            '^' => Node::Assert(if self.flags.multiline {
                Assert::LineStart
            } else {
                Assert::TextStart
            }),
            '$' => Node::Assert(if self.flags.multiline {
                Assert::LineEnd
            } else if self.py() {
                Assert::PyEnd
            } else {
                Assert::TextEnd
            }),
            '(' => return self.group(start).map(Some),
            '[' => Node::Class(self.class(start)?, ic),
            '\\' => self.escape(start)?,
            '*' | '+' | '?' => {
                return Err(if self.py() {
                    self.err("nothing to repeat", start)
                } else {
                    self.err("Nothing to repeat", start)
                })
            }
            '{' if !self.py() && self.flags.unicode => {
                return Err(self.err("Lone quantifier brackets", start))
            }
            '{' => {
                self.i = start;
                if self.parse_brace().is_some() {
                    return Err(if self.py() {
                        self.err("nothing to repeat", start)
                    } else {
                        self.err("Nothing to repeat", start)
                    });
                }
                self.i = start + 1;
                Node::Char('{', ic)
            }
            '}' if !self.py() && self.flags.unicode => {
                return Err(self.err("Lone quantifier brackets", start))
            }
            ']' if !self.py() && self.flags.unicode => {
                return Err(self.err("Lone quantifier brackets", start))
            }
            c => Node::Char(c, ic),
        }))
    }

    fn group_name(&mut self, close: char) -> Result<String, Error> {
        let start = self.i;
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c == close {
                break;
            }
            name.push(c);
            self.i += 1;
        }
        if self.peek() != Some(close) {
            return Err(if self.py() {
                self.err("missing >, unterminated name", start)
            } else {
                self.err("Invalid capture group name", start)
            });
        }
        self.i += 1;
        if name.is_empty() {
            return Err(if self.py() {
                self.err("missing group name", start)
            } else {
                self.err("Invalid capture group name", start)
            });
        }
        let valid = name
            .chars()
            .next()
            .is_some_and(|c| c == '_' || c.is_alphabetic())
            && name
                .chars()
                .all(|c| c == '_' || c.is_alphanumeric() || (!self.py() && c == '$'));
        if !valid {
            return Err(if self.py() {
                self.err(format!("bad character in group name '{name}'"), start)
            } else {
                self.err("Invalid capture group name", start)
            });
        }
        Ok(name)
    }

    fn close_group(&mut self, start: usize) -> Result<(), Error> {
        if self.peek() != Some(')') {
            return Err(if self.py() {
                self.err("missing ), unterminated subpattern", start)
            } else {
                self.err("Unterminated group", start)
            });
        }
        self.i += 1;
        Ok(())
    }

    fn group(&mut self, start: usize) -> Result<Node, Error> {
        if self.peek() == Some('?') {
            self.i += 1;
            let c = self.peek();
            match c {
                Some(':') => {
                    self.i += 1;
                    let inner = self.alternation()?;
                    self.close_group(start)?;
                    return Ok(Node::Group(Box::new(inner), None));
                }
                Some('=') | Some('!') => {
                    self.i += 1;
                    let inner = self.alternation()?;
                    self.close_group(start)?;
                    return Ok(Node::Look {
                        node: Box::new(inner),
                        ahead: true,
                        negate: c == Some('!'),
                    });
                }
                Some('>') if self.py() => {
                    self.i += 1;
                    let inner = self.alternation()?;
                    self.close_group(start)?;
                    return Ok(Node::Atomic(Box::new(inner)));
                }
                Some('<') => {
                    self.i += 1;
                    match self.peek() {
                        Some('=') | Some('!') => {
                            let neg = self.peek() == Some('!');
                            self.i += 1;
                            let inner = self.alternation()?;
                            self.close_group(start)?;
                            if self.py() && fixed_width(&inner).is_none() {
                                return Err(
                                    self.err("look-behind requires fixed-width pattern", start)
                                );
                            }
                            return Ok(Node::Look {
                                node: Box::new(inner),
                                ahead: false,
                                negate: neg,
                            });
                        }
                        _ => {
                            if self.py() {
                                let ch = self.peek().map(|c| c.to_string()).unwrap_or_default();
                                return Err(
                                    self.err(format!("unknown extension ?<{ch}"), start + 1)
                                );
                            }
                            let name = self.group_name('>')?;
                            return self.capture(start, Some(name));
                        }
                    }
                }
                Some('P') if self.py() => {
                    self.i += 1;
                    match self.peek() {
                        Some('<') => {
                            self.i += 1;
                            let name = self.group_name('>')?;
                            return self.capture(start, Some(name));
                        }
                        Some('=') => {
                            self.i += 1;
                            let name_pos = self.i;
                            let name = self.group_name(')')?;
                            let Some(idx) =
                                self.names.iter().find(|(n, _)| *n == name).map(|x| x.1)
                            else {
                                return Err(
                                    self.err(format!("unknown group name '{name}'"), name_pos)
                                );
                            };
                            if self.open_groups.contains(&idx) {
                                return Err(self.err("cannot refer to an open group", start));
                            }
                            return Ok(Node::Backref(idx, self.flags.ignore_case));
                        }
                        other => {
                            let ch = other.map(|c| c.to_string()).unwrap_or_default();
                            return Err(self.err(format!("unknown extension ?P{ch}"), start + 1));
                        }
                    }
                }
                Some('#') if self.py() => {
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        if c == ')' {
                            return Ok(Node::Empty);
                        }
                    }
                    return Err(self.err("missing ), unterminated comment", start));
                }
                _ => {
                    // Inline flags: (?imsx) or scoped (?i:...) / (?-i:...).
                    if self.py() {
                        return self.inline_flags(start);
                    }
                    return Err(self.err("Invalid group", start));
                }
            }
        }
        self.capture(start, None)
    }

    fn inline_flags(&mut self, start: usize) -> Result<Node, Error> {
        let mut on = Flags::default();
        let mut off = Flags::default();
        let mut negative = false;
        loop {
            match self.peek() {
                Some('i') => set_flag(if negative { &mut off } else { &mut on }, 'i'),
                Some('m') => set_flag(if negative { &mut off } else { &mut on }, 'm'),
                Some('s') => set_flag(if negative { &mut off } else { &mut on }, 's'),
                Some('x') => set_flag(if negative { &mut off } else { &mut on }, 'x'),
                Some('a') => set_flag(&mut on, 'a'),
                Some('u') | Some('L') => {}
                Some('-') => negative = true,
                Some(')') => {
                    self.i += 1;
                    if start != 0 && !self.global_flags_ok(start) {
                        return Err(
                            self.err("global flags not at the start of the expression", start)
                        );
                    }
                    self.flags = merge(self.flags, on, off);
                    return Ok(Node::Empty);
                }
                Some(':') => {
                    self.i += 1;
                    let saved = self.flags;
                    self.flags = merge(self.flags, on, off);
                    let inner = self.alternation();
                    self.flags = saved;
                    let inner = inner?;
                    self.close_group(start)?;
                    return Ok(Node::Group(Box::new(inner), None));
                }
                Some(c) => {
                    return Err(self.err(format!("unknown extension ?{c}"), start + 1));
                }
                None => return Err(self.err("missing -, : or )", self.i)),
            }
            self.i += 1;
        }
    }
    fn global_flags_ok(&self, start: usize) -> bool {
        // Only other global flag groups may precede.
        let before: String = self.c[..start].iter().collect();
        let mut rest = before.as_str();
        while let Some(r) = rest.strip_prefix("(?") {
            match r.find(')') {
                Some(p) if r[..p].chars().all(|c| "imsxauL".contains(c)) => rest = &r[p + 1..],
                _ => return false,
            }
        }
        rest.is_empty()
    }

    fn capture(&mut self, start: usize, name: Option<String>) -> Result<Node, Error> {
        self.ngroups += 1;
        let idx = self.ngroups;
        if let Some(n) = &name {
            if let Some((_, prev)) = self.names.iter().find(|(x, _)| x == n) {
                return Err(if self.py() {
                    self.err(
                        format!(
                            "redefinition of group name '{n}' as group {idx}; was group {prev}"
                        ),
                        start + 4,
                    )
                } else {
                    self.err("Duplicate capture group name", start)
                });
            }
            self.names.push((n.clone(), idx));
        }
        self.open_groups.push(idx);
        let inner = self.alternation()?;
        self.open_groups.pop();
        self.close_group(start)?;
        Ok(Node::Group(Box::new(inner), Some(idx)))
    }

    fn class_escape(&mut self, start: usize) -> Result<ClassOrChar, Error> {
        let Some(c) = self.peek() else {
            return Err(if self.py() {
                self.err("bad escape (end of pattern)", start)
            } else {
                self.err("\\ at end of pattern", start)
            });
        };
        self.i += 1;
        let ascii = self.flags.ascii || !self.py();
        Ok(match c {
            'd' => ClassOrChar::Item(ClassItem::Digit(false)),
            'D' => ClassOrChar::Item(ClassItem::Digit(true)),
            'w' => ClassOrChar::Item(ClassItem::Word(false)),
            'W' => ClassOrChar::Item(ClassItem::Word(true)),
            's' => ClassOrChar::Item(ClassItem::Space(false)),
            'S' => ClassOrChar::Item(ClassItem::Space(true)),
            _ => {
                let _ = ascii;
                self.i -= 1;
                ClassOrChar::Char(self.char_escape(start, true)?)
            }
        })
    }

    /// Escapes that denote a single character. Position is just after the backslash.
    fn char_escape(&mut self, start: usize, in_class: bool) -> Result<char, Error> {
        let c = self.c[self.i];
        self.i += 1;
        let hex = |p: &mut Self, n: usize| -> Result<char, Error> {
            let digits: String = p.c.iter().skip(p.i).take(n).collect();
            if digits.len() < n || !digits.chars().all(|d| d.is_ascii_hexdigit()) {
                return Err(if p.py() {
                    p.err(format!("incomplete escape \\{c}{digits}"), start)
                } else {
                    p.err("Invalid escape", start)
                });
            }
            p.i += n;
            char::from_u32(u32::from_str_radix(&digits, 16).unwrap())
                .ok_or_else(|| p.err("bad escape", start))
        };
        Ok(match c {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'f' => '\x0c',
            'v' => '\x0b',
            'a' if self.py() => '\x07',
            'b' if in_class => '\x08',
            '0' => {
                // Octal (Python) or NUL (JS).
                if self.py() {
                    let mut v = 0u32;
                    let mut k = 0;
                    while k < 2 && self.peek().is_some_and(|d| d.is_digit(8)) {
                        v = v * 8 + self.peek().unwrap().to_digit(8).unwrap();
                        self.i += 1;
                        k += 1;
                    }
                    char::from_u32(v).unwrap_or('\0')
                } else {
                    '\0'
                }
            }
            'x' => hex(self, 2)?,
            'u' => {
                if !self.py() && self.peek() == Some('{') && self.flags.unicode {
                    self.i += 1;
                    let mut digits = String::new();
                    while let Some(d) = self.peek() {
                        self.i += 1;
                        if d == '}' {
                            break;
                        }
                        digits.push(d);
                    }
                    char::from_u32(
                        u32::from_str_radix(&digits, 16)
                            .map_err(|_| self.err("Invalid Unicode escape", start))?,
                    )
                    .ok_or_else(|| self.err("Invalid Unicode escape", start))?
                } else {
                    hex(self, 4)?
                }
            }
            'U' if self.py() => hex(self, 8)?,
            'c' if !self.py() => {
                let l = self.peek().unwrap_or('@');
                self.i += 1;
                char::from_u32(l as u32 % 32).unwrap_or('\0')
            }
            '1'..='7' if in_class && self.py() => {
                let mut v = c.to_digit(8).unwrap();
                let mut k = 0;
                while k < 2 && self.peek().is_some_and(|d| d.is_digit(8)) {
                    v = v * 8 + self.peek().unwrap().to_digit(8).unwrap();
                    self.i += 1;
                    k += 1;
                }
                char::from_u32(v).unwrap_or('\0')
            }
            c if c.is_ascii_alphanumeric() => {
                if self.py() {
                    return Err(self.err(format!("bad escape \\{c}"), start));
                }
                if self.flags.unicode {
                    return Err(self.err("Invalid escape", start));
                }
                c
            }
            c => c,
        })
    }

    fn escape(&mut self, start: usize) -> Result<Node, Error> {
        let Some(c) = self.peek() else {
            return Err(if self.py() {
                self.err("bad escape (end of pattern)", start)
            } else {
                self.err("\\ at end of pattern", start)
            });
        };
        let ic = self.flags.ignore_case;
        let class = |item: ClassItem| {
            Node::Class(
                Class {
                    items: vec![item],
                    negated: false,
                },
                false,
            )
        };
        match c {
            'd' | 'D' | 'w' | 'W' | 's' | 'S' => {
                self.i += 1;
                let neg = c.is_uppercase();
                return Ok(class(match c.to_ascii_lowercase() {
                    'd' => ClassItem::Digit(neg),
                    'w' => ClassItem::Word(neg),
                    _ => ClassItem::Space(neg),
                }));
            }
            'b' => {
                self.i += 1;
                return Ok(Node::Assert(Assert::WordBoundary));
            }
            'B' => {
                self.i += 1;
                return Ok(Node::Assert(Assert::NotWordBoundary));
            }
            'A' if self.py() => {
                self.i += 1;
                return Ok(Node::Assert(Assert::TextStart));
            }
            'Z' if self.py() => {
                self.i += 1;
                return Ok(Node::Assert(Assert::TextEnd));
            }
            'k' if !self.py()
                && (self.flags.unicode
                    || !self.names.is_empty()
                    || self.pattern.contains("(?<")) =>
            {
                self.i += 1;
                if self.peek() != Some('<') {
                    return Err(self.err("Invalid named reference", start));
                }
                self.i += 1;
                let name = self.group_name('>')?;
                return Ok(Node::NamedBackref(name, ic, start));
            }
            '1'..='9' => {
                // Backreference: up to two digits (Python), greedy digits (JS).
                let mut digits = String::new();
                let save = self.i;
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() && (digits.len() < 2 || !self.py()) {
                        digits.push(d);
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                let n: usize = digits.parse().unwrap();
                if self.py() {
                    // Three octal digits make an octal escape.
                    if digits.len() == 2
                        && self.peek().is_some_and(|d| d.is_digit(8))
                        && digits.chars().all(|d| d.is_digit(8))
                    {
                        let d3 = self.peek().unwrap();
                        self.i += 1;
                        let v = u32::from_str_radix(&format!("{digits}{d3}"), 8).unwrap();
                        return Ok(Node::Char(char::from_u32(v).unwrap_or('\0'), ic));
                    }
                    if n > self.ngroups || self.open_groups.contains(&n) {
                        if self.open_groups.contains(&n) {
                            return Err(self.err("cannot refer to an open group", start));
                        }
                        return Err(self.err(format!("invalid group reference {n}"), start + 1));
                    }
                    return Ok(Node::Backref(n, ic));
                }
                // JS: resolved after parsing; forward references match empty.
                let _ = save;
                return Ok(Node::Backref(n, ic));
            }
            _ => {}
        }
        let ch = self.char_escape(start, false)?;
        Ok(Node::Char(ch, ic))
    }

    fn class(&mut self, start: usize) -> Result<Class, Error> {
        let mut items = vec![];
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.i += 1;
        }
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(if self.py() {
                    self.err("unterminated character set", start)
                } else {
                    self.err("Unterminated character class", start)
                });
            };
            if c == ']' && !(first && self.py()) {
                self.i += 1;
                break;
            }
            if c == ']' && first && !self.py() {
                self.i += 1;
                break;
            }
            first = false;
            let item_start = self.i;
            let lo = if c == '\\' {
                self.i += 1;
                match self.class_escape(item_start)? {
                    ClassOrChar::Item(it) => {
                        items.push(it);
                        continue;
                    }
                    ClassOrChar::Char(ch) => ch,
                }
            } else {
                self.i += 1;
                c
            };
            // Range?
            if self.peek() == Some('-') && self.c.get(self.i + 1).is_some_and(|n| *n != ']') {
                self.i += 1;
                let hc = self.c[self.i];
                let hi = if hc == '\\' {
                    let esc_start = self.i;
                    self.i += 1;
                    match self.class_escape(esc_start)? {
                        ClassOrChar::Char(ch) => ch,
                        ClassOrChar::Item(_) => {
                            return Err(if self.py() {
                                self.err(
                                    format!(
                                        "bad character range {}-{}",
                                        lo,
                                        self.c[esc_start..self.i].iter().collect::<String>()
                                    ),
                                    item_start,
                                )
                            } else {
                                self.err("Invalid character class", item_start)
                            });
                        }
                    }
                } else {
                    self.i += 1;
                    hc
                };
                if hi < lo {
                    return Err(if self.py() {
                        self.err(format!("bad character range {lo}-{hi}"), item_start)
                    } else {
                        self.err("Range out of order in character class", item_start)
                    });
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Class { items, negated })
    }
}

enum ClassOrChar {
    Item(ClassItem),
    Char(char),
}

fn set_flag(f: &mut Flags, c: char) {
    match c {
        'i' => f.ignore_case = true,
        'm' => f.multiline = true,
        's' => f.dot_all = true,
        'x' => f.verbose = true,
        'a' => f.ascii = true,
        _ => {}
    }
}
fn merge(base: Flags, on: Flags, off: Flags) -> Flags {
    Flags {
        ignore_case: (base.ignore_case || on.ignore_case) && !off.ignore_case,
        multiline: (base.multiline || on.multiline) && !off.multiline,
        dot_all: (base.dot_all || on.dot_all) && !off.dot_all,
        verbose: (base.verbose || on.verbose) && !off.verbose,
        ascii: base.ascii || on.ascii,
        unicode: base.unicode,
    }
}

/// Width of a fixed-width pattern (for Python look-behind), in characters.
fn fixed_width(n: &Node) -> Option<usize> {
    match n {
        Node::Empty | Node::Assert(_) | Node::Look { .. } => Some(0),
        Node::Char(..) | Node::Any(_) | Node::Class(..) => Some(1),
        Node::Group(inner, _) | Node::Atomic(inner) => fixed_width(inner),
        Node::Concat(items) => items.iter().map(fixed_width).sum(),
        Node::Alt(items) => {
            let first = fixed_width(&items[0])?;
            for it in &items[1..] {
                if fixed_width(it)? != first {
                    return None;
                }
            }
            Some(first)
        }
        Node::Repeat { node, min, max, .. } => {
            if Some(*min) == *max {
                Some(fixed_width(node)? * *min as usize)
            } else {
                None
            }
        }
        Node::Backref(..) | Node::NamedBackref(..) => None,
    }
}

fn can_be_empty(n: &Node) -> bool {
    match n {
        Node::Empty | Node::Assert(_) | Node::Look { .. } => true,
        Node::Char(..) | Node::Any(_) | Node::Class(..) => false,
        Node::Group(inner, _) | Node::Atomic(inner) => can_be_empty(inner),
        Node::Concat(items) => items.iter().all(can_be_empty),
        Node::Alt(items) => items.iter().any(can_be_empty),
        Node::Repeat { node, min, .. } => *min == 0 || can_be_empty(node),
        Node::Backref(..) | Node::NamedBackref(..) => true,
    }
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

struct Compiler {
    progs: Vec<Vec<Inst>>,
    classes: Vec<Class>,
    nregs: usize,
    names: Vec<(String, usize)>,
    size: usize,
}

impl Compiler {
    fn emit(&mut self, p: usize, i: Inst) -> Result<usize, Error> {
        self.size += 1;
        if self.size > 200_000 {
            return Err(Error {
                message: "pattern too large".into(),
                position: 0,
            });
        }
        self.progs[p].push(i);
        Ok(self.progs[p].len() - 1)
    }
    fn pc(&self, p: usize) -> usize {
        self.progs[p].len()
    }
    fn node(&mut self, p: usize, n: &Node) -> Result<(), Error> {
        match n {
            Node::Empty => {}
            Node::Char(c, ic) => {
                self.emit(p, Inst::Char(*c, *ic))?;
            }
            Node::Any(dotall) => {
                self.emit(p, Inst::Any(*dotall))?;
            }
            Node::Class(c, ic) => {
                self.classes.push(c.clone());
                let idx = self.classes.len() - 1;
                self.emit(p, Inst::Class(idx, *ic))?;
            }
            Node::Assert(a) => {
                self.emit(p, Inst::Assert(*a))?;
            }
            Node::Group(inner, cap) => {
                if let Some(g) = cap {
                    self.emit(p, Inst::Save(2 * g))?;
                    self.node(p, inner)?;
                    self.emit(p, Inst::Save(2 * g + 1))?;
                } else {
                    self.node(p, inner)?;
                }
            }
            Node::Concat(items) => {
                for it in items {
                    self.node(p, it)?;
                }
            }
            Node::Alt(items) => {
                let mut jumps = vec![];
                for (k, it) in items.iter().enumerate() {
                    if k + 1 < items.len() {
                        let split = self.emit(p, Inst::Split(0, 0))?;
                        self.node(p, it)?;
                        jumps.push(self.emit(p, Inst::Jmp(0))?);
                        let next = self.pc(p);
                        self.progs[p][split] = Inst::Split(split + 1, next);
                    } else {
                        self.node(p, it)?;
                    }
                }
                let end = self.pc(p);
                for j in jumps {
                    self.progs[p][j] = Inst::Jmp(end);
                }
            }
            Node::Repeat {
                node,
                min,
                max,
                greedy,
                possessive,
            } => {
                if *possessive {
                    let inner = Node::Repeat {
                        node: node.clone(),
                        min: *min,
                        max: *max,
                        greedy: true,
                        possessive: false,
                    };
                    return self.node(p, &Node::Atomic(Box::new(inner)));
                }
                let empty = can_be_empty(node);
                for _ in 0..*min {
                    self.node(p, node)?;
                }
                match max {
                    None => {
                        // x*
                        let reg = self.nregs;
                        if empty {
                            self.nregs += 1;
                        }
                        let l1 = self.emit(p, Inst::Split(0, 0))?;
                        if empty {
                            self.emit(p, Inst::Mark(reg))?;
                        }
                        self.node(p, node)?;
                        if empty {
                            self.emit(p, Inst::Progress(reg))?;
                        }
                        self.emit(p, Inst::Jmp(l1))?;
                        let end = self.pc(p);
                        self.progs[p][l1] = if *greedy {
                            Inst::Split(l1 + 1, end)
                        } else {
                            Inst::Split(end, l1 + 1)
                        };
                    }
                    Some(mx) => {
                        let extra = mx.saturating_sub(*min);
                        let mut splits = vec![];
                        for _ in 0..extra {
                            let s = self.emit(p, Inst::Split(0, 0))?;
                            splits.push(s);
                            self.node(p, node)?;
                        }
                        let end = self.pc(p);
                        for s in splits {
                            self.progs[p][s] = if *greedy {
                                Inst::Split(s + 1, end)
                            } else {
                                Inst::Split(end, s + 1)
                            };
                        }
                    }
                }
            }
            Node::Backref(g, ic) => {
                self.emit(p, Inst::Backref(*g, *ic))?;
            }
            Node::NamedBackref(name, ic, pos) => {
                let g = self
                    .names
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|x| x.1)
                    .ok_or_else(|| Error {
                        message: "Invalid named capture referenced".into(),
                        position: *pos,
                    })?;
                self.emit(p, Inst::Backref(g, *ic))?;
            }
            Node::Look {
                node,
                ahead,
                negate,
            } => {
                let sub = self.progs.len();
                self.progs.push(vec![]);
                self.node(sub, node)?;
                self.emit(sub, Inst::Match)?;
                let width = if *ahead { None } else { fixed_width(node) };
                self.emit(
                    p,
                    Inst::Look {
                        prog: sub,
                        ahead: *ahead,
                        negate: *negate,
                        width,
                    },
                )?;
            }
            Node::Atomic(node) => {
                let sub = self.progs.len();
                self.progs.push(vec![]);
                self.node(sub, node)?;
                self.emit(sub, Inst::Match)?;
                self.emit(p, Inst::Atomic(sub))?;
            }
        }
        Ok(())
    }
}

fn first_char(n: &Node) -> Option<char> {
    match n {
        Node::Char(c, false) => Some(*c),
        Node::Concat(items) => items.first().and_then(first_char),
        Node::Group(inner, _) => first_char(inner),
        Node::Repeat { node, min, .. } if *min > 0 => first_char(node),
        _ => None,
    }
}
fn starts_anchored(n: &Node) -> bool {
    match n {
        Node::Assert(Assert::TextStart) => true,
        Node::Concat(items) => items.first().is_some_and(starts_anchored),
        Node::Group(inner, _) => starts_anchored(inner),
        _ => false,
    }
}

impl Regex {
    pub fn new(pattern: &str, flavor: Flavor, flags: Flags) -> Result<Regex, Error> {
        let mut p = Parser {
            c: pattern.chars().collect(),
            i: 0,
            flavor,
            flags,
            ngroups: 0,
            names: vec![],
            pattern,
            open_groups: vec![],
        };
        let node = p.parse()?;
        let final_flags = p.flags;
        let mut c = Compiler {
            progs: vec![vec![]],
            classes: vec![],
            nregs: 0,
            names: p.names.clone(),
            size: 0,
        };
        c.node(0, &node)?;
        c.emit(0, Inst::Match)?;
        if flavor == Flavor::JavaScript {
            // Backreferences past the group count are octal/identity escapes in JS;
            // we treat them as never-participating groups (empty match).
        }
        Ok(Regex {
            progs: c.progs,
            classes: c.classes,
            ngroups: p.ngroups,
            names: p.names,
            nregs: c.nregs,
            flags: final_flags,
            flavor,
            anchored_start: starts_anchored(&node),
            first_char: first_char(&node),
        })
    }
    pub fn group_count(&self) -> usize {
        self.ngroups
    }
    pub fn group_names(&self) -> &[(String, usize)] {
        &self.names
    }
    pub fn flags(&self) -> Flags {
        self.flags
    }

    pub fn exec(
        &self,
        text: &[char],
        start: usize,
        anchored: bool,
        end_anchor: bool,
    ) -> Result<Option<Slots>, Exhausted> {
        self.exec_opts(text, start, anchored, end_anchor, None)
    }

    /// Like [`Regex::exec`]; `forbid_empty_at` rejects an empty match starting at
    /// that position (how `findall`/`sub` step past empty matches).
    pub fn exec_opts(
        &self,
        text: &[char],
        start: usize,
        anchored: bool,
        end_anchor: bool,
        forbid_empty_at: Option<usize>,
    ) -> Result<Option<Slots>, Exhausted> {
        let mut steps = 0u64;
        let nslots = 2 * (self.ngroups + 1);
        let mut pos = start;
        loop {
            if pos > text.len() {
                return Ok(None);
            }
            if !anchored {
                if let Some(fc) = self.first_char {
                    match text[pos..].iter().position(|c| *c == fc) {
                        Some(off) => pos += off,
                        None => return Ok(None),
                    }
                }
            }
            let mut caps: Vec<Option<usize>> = vec![None; nslots];
            let mut regs = vec![usize::MAX; self.nregs];
            let m = Matcher {
                re: self,
                text,
                end_anchor,
                forbid: forbid_empty_at,
            };
            if let Some(end) = m.run(0, pos, pos, &mut caps, &mut regs, &mut steps)? {
                caps[0] = Some(pos);
                caps[1] = Some(end);
                let mut out = Vec::with_capacity(self.ngroups + 1);
                for g in 0..=self.ngroups {
                    out.push(match (caps[2 * g], caps[2 * g + 1]) {
                        (Some(a), Some(b)) if a <= b => Some((a, b)),
                        _ => None,
                    });
                }
                return Ok(Some(out));
            }
            if anchored || (self.anchored_start && !self.flags.multiline) {
                return Ok(None);
            }
            pos += 1;
        }
    }
}

struct Matcher<'r, 't> {
    re: &'r Regex,
    text: &'t [char],
    end_anchor: bool,
    forbid: Option<usize>,
}

enum Frame {
    Alt(usize, usize),
    Cap(usize, Option<usize>),
    Reg(usize, usize),
}

fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

impl<'r, 't> Matcher<'r, 't> {
    fn class_matches(&self, idx: usize, c: char, ic: bool) -> bool {
        let cls = &self.re.classes[idx];
        let ascii = self.re.flags.ascii || self.re.flavor == Flavor::JavaScript;
        let test = |ch: char| {
            cls.items.iter().any(|it| match it {
                ClassItem::Range(lo, hi) => *lo <= ch && ch <= *hi,
                ClassItem::Digit(neg) => is_digit(ch, ascii) != *neg,
                ClassItem::Word(neg) => is_word(ch, ascii) != *neg,
                ClassItem::Space(neg) => {
                    let sp = if self.re.flavor == Flavor::JavaScript {
                        ch.is_whitespace() || ch == '\u{feff}'
                    } else {
                        is_space(ch, ascii)
                    };
                    sp != *neg
                }
            })
        };
        let mut hit = test(c);
        if !hit && ic {
            let lower = fold(c);
            let upper: Vec<char> = c.to_uppercase().collect();
            hit = test(lower) || (upper.len() == 1 && test(upper[0]));
        }
        hit != cls.negated
    }
    fn word_at(&self, i: usize) -> bool {
        let ascii = self.re.flags.ascii || self.re.flavor == Flavor::JavaScript;
        self.text.get(i).is_some_and(|c| is_word(*c, ascii))
    }
    fn assert(&self, a: Assert, pos: usize) -> bool {
        let n = self.text.len();
        match a {
            Assert::TextStart => pos == 0,
            Assert::LineStart => pos == 0 || self.text[pos - 1] == '\n',
            Assert::TextEnd => pos == n,
            Assert::PyEnd => pos == n || (pos + 1 == n && self.text[pos] == '\n'),
            Assert::LineEnd => pos == n || self.text[pos] == '\n',
            Assert::WordBoundary | Assert::NotWordBoundary => {
                let before = pos > 0 && self.word_at(pos - 1);
                let after = self.word_at(pos);
                (before != after) == (a == Assert::WordBoundary)
            }
        }
    }

    /// Runs program `prog` from `pos`; returns the end position of a match.
    fn run(
        &self,
        prog: usize,
        pos: usize,
        match_start: usize,
        caps: &mut Vec<Option<usize>>,
        regs: &mut Vec<usize>,
        steps: &mut u64,
    ) -> Result<Option<usize>, Exhausted> {
        let code = &self.re.progs[prog];
        let mut stack: Vec<Frame> = Vec::new();
        let mut pc = 0usize;
        let mut pos = pos;
        let text = self.text;
        loop {
            *steps += 1;
            if *steps > STEP_LIMIT {
                return Err(Exhausted);
            }
            let ok = match &code[pc] {
                Inst::Char(c, ic) => {
                    if pos < text.len() && (text[pos] == *c || (*ic && fold(text[pos]) == fold(*c)))
                    {
                        pos += 1;
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Any(dotall) => {
                    let nl = match self.re.flavor {
                        Flavor::Python => pos < text.len() && text[pos] == '\n',
                        Flavor::JavaScript => {
                            pos < text.len()
                                && matches!(text[pos], '\n' | '\r' | '\u{2028}' | '\u{2029}')
                        }
                    };
                    if pos < text.len() && (*dotall || !nl) {
                        pos += 1;
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Class(idx, ic) => {
                    if pos < text.len() && self.class_matches(*idx, text[pos], *ic) {
                        pos += 1;
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Split(a, b) => {
                    stack.push(Frame::Alt(*b, pos));
                    pc = *a;
                    true
                }
                Inst::Jmp(t) => {
                    pc = *t;
                    true
                }
                Inst::Save(s) => {
                    stack.push(Frame::Cap(*s, caps[*s]));
                    caps[*s] = Some(pos);
                    pc += 1;
                    true
                }
                Inst::Assert(a) => {
                    if self.assert(*a, pos) {
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Backref(g, ic) => {
                    let (s, e) = (
                        caps.get(2 * g).copied().flatten(),
                        caps.get(2 * g + 1).copied().flatten(),
                    );
                    match (s, e) {
                        (Some(s), Some(e)) if s <= e => {
                            let len = e - s;
                            if pos + len <= text.len()
                                && (0..len).all(|k| {
                                    let (x, y) = (text[s + k], text[pos + k]);
                                    x == y || (*ic && fold(x) == fold(y))
                                })
                            {
                                pos += len;
                                pc += 1;
                                true
                            } else {
                                false
                            }
                        }
                        _ => {
                            // Unset group: fails in Python, matches empty in JS.
                            if self.re.flavor == Flavor::JavaScript {
                                pc += 1;
                                true
                            } else {
                                false
                            }
                        }
                    }
                }
                Inst::Look {
                    prog: sub,
                    ahead,
                    negate,
                    width,
                } => {
                    let snapshot = caps.clone();
                    let matched = if *ahead {
                        let inner = Matcher {
                            re: self.re,
                            text,
                            end_anchor: false,
                            forbid: None,
                        };
                        inner.run(*sub, pos, pos, caps, regs, steps)?.is_some()
                    } else {
                        let starts: Vec<usize> = match width {
                            Some(w) => {
                                if *w <= pos {
                                    vec![pos - w]
                                } else {
                                    vec![]
                                }
                            }
                            None => (0..=pos).rev().collect(),
                        };
                        let mut found = false;
                        for st in starts {
                            let inner = Matcher {
                                re: self.re,
                                text: &text[..pos],
                                end_anchor: true,
                                forbid: None,
                            };
                            let mut trial = caps.clone();
                            if let Some(end) = inner.run(*sub, st, st, &mut trial, regs, steps)? {
                                if end == pos {
                                    *caps = trial;
                                    found = true;
                                    break;
                                }
                            }
                        }
                        found
                    };
                    if *negate {
                        *caps = snapshot;
                        if matched {
                            false
                        } else {
                            pc += 1;
                            true
                        }
                    } else if matched {
                        for (i, old) in snapshot.into_iter().enumerate() {
                            if caps[i] != old {
                                stack.push(Frame::Cap(i, old));
                            }
                        }
                        pc += 1;
                        true
                    } else {
                        *caps = snapshot;
                        false
                    }
                }
                Inst::Atomic(sub) => {
                    let snapshot = caps.clone();
                    let inner = Matcher {
                        re: self.re,
                        text,
                        end_anchor: false,
                        forbid: None,
                    };
                    match inner.run(*sub, pos, pos, caps, regs, steps)? {
                        Some(end) => {
                            for (i, old) in snapshot.into_iter().enumerate() {
                                if caps[i] != old {
                                    stack.push(Frame::Cap(i, old));
                                }
                            }
                            pos = end;
                            pc += 1;
                            true
                        }
                        None => {
                            *caps = snapshot;
                            false
                        }
                    }
                }
                Inst::Mark(r) => {
                    stack.push(Frame::Reg(*r, regs[*r]));
                    regs[*r] = pos;
                    pc += 1;
                    true
                }
                Inst::Progress(r) => {
                    if regs[*r] == pos {
                        false
                    } else {
                        pc += 1;
                        true
                    }
                }
                Inst::Match => {
                    let at_end_ok = !self.end_anchor || pos == text.len();
                    let empty_forbidden =
                        prog == 0 && self.forbid == Some(match_start) && pos == match_start;
                    if at_end_ok && !empty_forbidden {
                        return Ok(Some(pos));
                    }
                    false
                }
            };
            if ok {
                continue;
            }
            // Backtrack.
            loop {
                match stack.pop() {
                    None => return Ok(None),
                    Some(Frame::Alt(p2, q)) => {
                        pc = p2;
                        pos = q;
                        break;
                    }
                    Some(Frame::Cap(s, old)) => caps[s] = old,
                    Some(Frame::Reg(r, old)) => regs[r] = old,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers for the language runtimes
// ---------------------------------------------------------------------------

/// `re.escape` (CPython 3.7+: only special characters are escaped).
pub fn escape_python(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "()[]{}?*+-|^$\\.&~# \t\n\r\x0b\x0c".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Expands an `re.sub` replacement template.
pub fn expand_python_template(
    template: &str,
    groups: &[Option<String>],
    names: &[(String, usize)],
) -> Result<String, Error> {
    let c: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let bad = |msg: String, pos: usize| Error {
        message: format!("{msg} at position {pos}"),
        position: pos,
    };
    let group = |n: usize, pos: usize| -> Result<String, Error> {
        if n >= groups.len() {
            return Err(bad(format!("invalid group reference {n}"), pos));
        }
        Ok(groups[n].clone().unwrap_or_default())
    };
    while i < c.len() {
        if c[i] != '\\' {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let Some(&e) = c.get(i) else {
            return Err(bad("bad escape (end of pattern)".into(), start));
        };
        i += 1;
        match e {
            'g' => {
                if c.get(i) != Some(&'<') {
                    return Err(bad("missing <".into(), i));
                }
                let close = c[i..].iter().position(|x| *x == '>').map(|p| p + i);
                let Some(close) = close else {
                    return Err(bad("missing >, unterminated name".into(), i + 1));
                };
                let name: String = c[i + 1..close].iter().collect();
                i = close + 1;
                if name.is_empty() {
                    return Err(bad("missing group name".into(), start + 3));
                }
                if let Ok(n) = name.parse::<usize>() {
                    out.push_str(&group(n, start + 3)?);
                } else if let Some((_, idx)) = names.iter().find(|(n, _)| *n == name) {
                    out.push_str(&group(*idx, start + 3)?);
                } else if name.chars().all(|ch| ch == '_' || ch.is_alphanumeric()) {
                    return Err(Error {
                        message: format!("unknown group name '{name}'"),
                        position: start + 3,
                    });
                } else {
                    return Err(bad(
                        format!("bad character in group name '{name}'"),
                        start + 3,
                    ));
                }
            }
            '0' => {
                let mut v = 0u32;
                let mut k = 0;
                while k < 2 && c.get(i).is_some_and(|d| d.is_digit(8)) {
                    v = v * 8 + c[i].to_digit(8).unwrap();
                    i += 1;
                    k += 1;
                }
                out.push(char::from_u32(v).unwrap_or('\0'));
            }
            '1'..='9' => {
                let mut n = e.to_digit(10).unwrap() as usize;
                if c.get(i).is_some_and(|d| d.is_ascii_digit()) {
                    // Three octal digits form an octal escape.
                    if c.get(i + 1).is_some_and(|d| d.is_digit(8))
                        && e.is_digit(8)
                        && c[i].is_digit(8)
                    {
                        let v =
                            u32::from_str_radix(&format!("{e}{}{}", c[i], c[i + 1]), 8).unwrap();
                        out.push(char::from_u32(v).unwrap_or('\0'));
                        i += 2;
                        continue;
                    }
                    n = n * 10 + c[i].to_digit(10).unwrap() as usize;
                    i += 1;
                }
                out.push_str(&group(n, start + 1)?);
            }
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            'f' => out.push('\x0c'),
            'v' => out.push('\x0b'),
            'a' => out.push('\x07'),
            'b' => out.push('\x08'),
            '\\' => out.push('\\'),
            e if e.is_ascii_alphabetic() => {
                return Err(bad(format!("bad escape \\{e}"), start));
            }
            e => {
                out.push('\\');
                out.push(e);
            }
        }
    }
    Ok(out)
}

/// Expands a JavaScript `String.prototype.replace` replacement string.
pub fn expand_js_replacement(
    template: &str,
    matched: &str,
    before: &str,
    after: &str,
    groups: &[Option<String>],
    names: &[(String, usize)],
) -> String {
    let c: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '$' || i + 1 >= c.len() {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let n = c[i + 1];
        match n {
            '$' => {
                out.push('$');
                i += 2;
            }
            '&' => {
                out.push_str(matched);
                i += 2;
            }
            '`' => {
                out.push_str(before);
                i += 2;
            }
            '\'' => {
                out.push_str(after);
                i += 2;
            }
            '<' if !names.is_empty() => match c[i + 2..].iter().position(|x| *x == '>') {
                Some(p) => {
                    let name: String = c[i + 2..i + 2 + p].iter().collect();
                    if let Some((_, idx)) = names.iter().find(|(nm, _)| *nm == name) {
                        out.push_str(groups.get(*idx).cloned().flatten().as_deref().unwrap_or(""));
                    }
                    i += 3 + p;
                }
                None => {
                    out.push('$');
                    i += 1;
                }
            },
            d if d.is_ascii_digit() => {
                let ngroups = groups.len().saturating_sub(1);
                let one = d.to_digit(10).unwrap() as usize;
                let two = c
                    .get(i + 2)
                    .and_then(|x| x.to_digit(10))
                    .map(|x| one * 10 + x as usize);
                if let Some(t) = two.filter(|t| *t >= 1 && *t <= ngroups) {
                    out.push_str(groups[t].as_deref().unwrap_or(""));
                    i += 3;
                } else if one >= 1 && one <= ngroups {
                    out.push_str(groups[one].as_deref().unwrap_or(""));
                    i += 2;
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn py(p: &str) -> Regex {
        Regex::new(p, Flavor::Python, Flags::default()).unwrap()
    }
    fn find(re: &Regex, s: &str) -> Option<(usize, usize)> {
        let t: Vec<char> = s.chars().collect();
        re.exec(&t, 0, false, false).unwrap().map(|g| g[0].unwrap())
    }
    #[test]
    fn basics() {
        assert_eq!(find(&py("b+"), "aabbbc"), Some((2, 5)));
        assert_eq!(find(&py("a|bc"), "xbc"), Some((1, 3)));
        assert_eq!(find(&py(r"\d{2,3}"), "a12345"), Some((1, 4)));
        assert_eq!(find(&py(r"\d{2,3}?"), "a12345"), Some((1, 3)));
        assert_eq!(find(&py(r"(a)\1"), "xaa"), Some((1, 3)));
        assert_eq!(find(&py(r"(?<=a)b"), "cbab"), Some((3, 4)));
        assert_eq!(find(&py(r"foo(?!bar)"), "foobar foobaz"), Some((7, 10)));
        assert_eq!(find(&py(r"^$"), ""), Some((0, 0)));
        assert_eq!(find(&py(r"(a*)*b"), "aaab"), Some((0, 4)));
    }
    #[test]
    fn errors_match_cpython() {
        let e = Regex::new("(", Flavor::Python, Flags::default()).unwrap_err();
        assert_eq!(
            e.message,
            "missing ), unterminated subpattern at position 0"
        );
        let e = Regex::new("*", Flavor::Python, Flags::default()).unwrap_err();
        assert_eq!(e.message, "nothing to repeat at position 0");
        let e = Regex::new("[a", Flavor::Python, Flags::default()).unwrap_err();
        assert_eq!(e.message, "unterminated character set at position 0");
    }
    #[test]
    fn catastrophic_patterns_terminate() {
        let re = py(r"(a*)*$x");
        let t: Vec<char> = "a".repeat(40).chars().collect();
        let _ = re.exec(&t, 0, false, false);
    }
}
