//! `find`: the complete predicate set, every one of which really filters.
//!
//! The expression is parsed into a tree with GNU's precedence — `!` binds tighter than
//! `-a`, which binds tighter than `-o` — and evaluated per visited node with real
//! short-circuiting, because `-exec`, `-delete` and `-quit` have side effects and must
//! not run for a node the expression already decided against.
//!
//! Determinism: the VFS stores a directory as a `BTreeMap`, so `list` hands back its
//! entries already sorted by name. GNU find walks in readdir order, which on a real
//! filesystem is arbitrary; here it is lexicographic and therefore reproducible. That
//! is a deliberate difference and the only one in traversal order: everything else
//! (pre-order by default, post-order under `-depth`) follows GNU.
//!
//! Paths are printed exactly as derived from the root operand the caller wrote, so
//! `find .` prints `./a/b` and `find /home/user` prints `/home/user/a/b`. A trailing
//! slash on a root is trimmed (except on `/` itself) so children are not printed with
//! a doubled separator.
//!
//! Two seams in `shell.rs` are not reachable from here and shape the implementation:
//!
//! * There is no `pub(crate)` argv-level entry point — `shell::run` and
//!   `shell::execute_inner` are private to that module — so `-exec`/`-execdir` build a
//!   command line and hand it to `shell::execute`. To keep a name containing spaces,
//!   quotes or globs from being re-lexed into several words, every argument is wrapped
//!   in single quotes with embedded single quotes written as `'\''`. That is exact for
//!   every byte a VFS name can hold. `shell::execute` restarts the shell's nesting
//!   counter at zero, so this module enforces the limit itself: an `-exec` in a `find`
//!   already nested 32 deep is refused with status 2 at parse time.
//! * A child's diagnostics must not change find's own status, so they ride along in
//!   the output string rather than in a `Fail`; find's own diagnostics become the
//!   failure text (already prefixed `find: `, so the shell passes them to stderr
//!   verbatim) and carry the partial output alongside, exactly as GNU find prints what
//!   it found and still exits non-zero.

use crate::shell::{apparent, clock, mode_string, wildcard, Fail, EPOCH_UNIX_SECONDS};
use crate::{Computer, Metadata, ShellHost, VfsError};
use std::collections::BTreeSet;

const MICROS: u64 = 1_000_000;
const DAY: u64 = 86_400 * MICROS;
const MINUTE: u64 = 60 * MICROS;
/// The shell refuses to nest deeper than this; `-exec` must not smuggle past it.
const NESTING_LIMIT: usize = 32;
/// A guard against pathological trees: the VFS bounds nesting by path length, but the
/// walk is recursive and a diagnostic beats a blown stack.
const WALK_LIMIT: usize = 512;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// GNU-style reason text for a VFS failure; `VfsError`'s own `Display` is the shell's
/// lowercase internal phrasing, and find's contract is `find: <operand>: <reason>`.
fn reason(e: &VfsError) -> &'static str {
    match e {
        VfsError::NotFound(_) => "No such file or directory",
        VfsError::Permission(_) => "Permission denied",
        VfsError::NotDirectory(_) => "Not a directory",
        VfsError::IsDirectory(_) => "Is a directory",
        VfsError::NotEmpty(_) => "Directory not empty",
        VfsError::Exists(_) => "File exists",
        VfsError::LinkLoop => "Too many levels of symbolic links",
        VfsError::Invalid(_) => "Invalid argument",
    }
}

// ---------------------------------------------------------------------------
// Expression tree
// ---------------------------------------------------------------------------

/// `N`, `+N`, `-N`: exactly, more than, less than. GNU truncates before comparing, so
/// the caller converts to whole units first.
#[derive(Debug, Clone, Copy)]
enum Cmp {
    Exact(u64),
    More(u64),
    Less(u64),
}

impl Cmp {
    fn parse(spec: &str, name: &str) -> Result<Self, Fail> {
        let (build, digits): (fn(u64) -> Cmp, &str) = match spec.as_bytes().first() {
            Some(b'+') => (Cmp::More, &spec[1..]),
            Some(b'-') => (Cmp::Less, &spec[1..]),
            _ => (Cmp::Exact, spec),
        };
        let n = digits
            .parse::<u64>()
            .map_err(|_| Fail::usage(format!("find: `{name}' expects a number, got `{spec}'")))?;
        Ok(build(n))
    }
    fn test(self, value: u64) -> bool {
        match self {
            Cmp::Exact(n) => value == n,
            Cmp::More(n) => value > n,
            Cmp::Less(n) => value < n,
        }
    }
}

/// Which timestamp a time predicate reads.
#[derive(Debug, Clone, Copy)]
enum Stamp {
    Modified,
    Accessed,
    Changed,
}

impl Stamp {
    fn of(self, m: &Metadata) -> u64 {
        match self {
            Stamp::Modified => m.modified,
            Stamp::Accessed => m.accessed,
            Stamp::Changed => m.changed,
        }
    }
}

/// `-perm MODE` is exact, `-perm -MODE` wants all of those bits, `-perm /MODE` any.
#[derive(Debug, Clone, Copy)]
enum PermMatch {
    Exact,
    All,
    Any,
}

#[derive(Debug)]
enum Test {
    Name {
        pattern: String,
        fold: bool,
    },
    Path {
        pattern: String,
        fold: bool,
    },
    Regex(regex::Regex),
    Kind(char),
    Size {
        cmp: Cmp,
        unit: u64,
    },
    Perm {
        bits: u16,
        how: PermMatch,
    },
    Time {
        stamp: Stamp,
        unit: u64,
        cmp: Cmp,
    },
    /// Strictly newer than this absolute stamp, in microseconds since the Unix epoch.
    /// Absolute rather than a world tick so a date before the world epoch still orders
    /// correctly instead of clamping to tick zero.
    Newer(i128),
    Empty,
    User(String),
    Group(String),
    NoUser,
    NoGroup,
}

#[derive(Debug, Clone, Copy)]
enum Conv {
    Path,
    Base,
    Dir,
    Links,
    Size,
    Octal,
    Modes,
    Owner,
    Group,
    Kind,
    Inode,
    Depth,
    Relative,
    Target,
    Epoch(Stamp),
    Field(Stamp, char),
}

#[derive(Debug)]
enum Piece {
    Literal(String),
    Conv(Conv),
}

#[derive(Debug)]
enum Action {
    /// `-print` (`\n`) and `-print0` (`\0`) differ only in the terminator.
    Print(char),
    Printf(Vec<Piece>),
    Ls,
    Delete,
    Quit,
    Prune,
    Exec {
        argv: Vec<String>,
        /// `-execdir`: run from the file's own directory with `./name`.
        local: bool,
        /// `+`: one run for many files, instead of one run per file.
        batch: Option<usize>,
    },
}

#[derive(Debug)]
enum Expr {
    /// A global option (`-maxdepth`, `-depth`, …): it configures the walk and is true.
    True,
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Test(Test),
    Act(Action),
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    args: &'a [String],
    i: usize,
    c: &'a Computer,
    /// The shell nesting depth `find` itself was called at.
    depth: usize,
    /// True once an action that does something is seen; GNU appends `-print` only when
    /// the expression has no side effects, which is why `-prune` does not suppress it
    /// but `-quit` does.
    effects: bool,
    post_order: bool,
    mindepth: usize,
    maxdepth: usize,
    batches: Vec<Batch>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.args.get(self.i).map(String::as_str)
    }
    fn bump(&mut self) -> Option<&'a str> {
        let v = self.peek();
        if v.is_some() {
            self.i += 1;
        }
        v
    }
    /// The argument a predicate requires. Missing is status 2, named.
    fn need(&mut self, name: &str) -> Result<String, Fail> {
        self.bump()
            .map(str::to_string)
            .ok_or_else(|| Fail::usage(format!("find: missing argument to `{name}'")))
    }
    fn count(&mut self, name: &str) -> Result<usize, Fail> {
        let raw = self.need(name)?;
        raw.parse()
            .map_err(|_| Fail::usage(format!("find: `{name}' expects a number, got `{raw}'")))
    }

    fn expression(&mut self) -> Result<Expr, Fail> {
        let e = self.disjunction()?;
        match self.peek() {
            None => Ok(e),
            Some(")") => Err(Fail::usage("find: unmatched `)'")),
            Some(other) => Err(Fail::usage(format!("find: unknown predicate `{other}`"))),
        }
    }

    fn disjunction(&mut self) -> Result<Expr, Fail> {
        let mut left = self.conjunction()?;
        while matches!(self.peek(), Some("-o" | "-or")) {
            let op = self.bump().unwrap_or("-o").to_string();
            if matches!(self.peek(), None | Some(")")) {
                return Err(Fail::usage(format!("find: missing operand after `{op}'")));
            }
            let right = self.conjunction()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn conjunction(&mut self) -> Result<Expr, Fail> {
        let mut left = self.unary()?;
        loop {
            match self.peek() {
                None | Some(")" | "-o" | "-or") => break,
                Some("-a" | "-and") => {
                    let op = self.bump().unwrap_or("-a").to_string();
                    if matches!(self.peek(), None | Some(")" | "-o" | "-or")) {
                        return Err(Fail::usage(format!("find: missing operand after `{op}'")));
                    }
                }
                // Adjacency is an implicit -a.
                Some(_) => {}
            }
            let right = self.unary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, Fail> {
        if matches!(self.peek(), Some("!" | "-not")) {
            let op = self.bump().unwrap_or("!").to_string();
            if matches!(self.peek(), None | Some(")" | "-o" | "-or" | "-a" | "-and")) {
                return Err(Fail::usage(format!("find: missing operand after `{op}'")));
            }
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, Fail> {
        if self.peek() == Some("(") {
            self.i += 1;
            if self.peek() == Some(")") {
                return Err(Fail::usage("find: empty `( )' group"));
            }
            let inner = self.disjunction()?;
            if self.bump() != Some(")") {
                return Err(Fail::usage("find: expected `)'"));
            }
            return Ok(inner);
        }
        let token = self
            .bump()
            .ok_or_else(|| Fail::usage("find: missing expression"))?
            .to_string();
        let t = token.as_str();
        let test = |t| Ok(Expr::Test(t));
        match t {
            "-name" | "-iname" => test(Test::Name {
                pattern: self.need(t)?,
                fold: t == "-iname",
            }),
            "-path" | "-ipath" | "-wholename" => test(Test::Path {
                pattern: self.need(t)?,
                fold: t == "-ipath",
            }),
            "-regex" | "-iregex" => {
                let body = self.need(t)?;
                // The whole path must match, as GNU's -regex does. The syntax is the
                // `regex` crate's, the same engine `grep`, `sed` and `[[ =~ ]]` use in
                // this shell, so one dialect is learned rather than three.
                let source = if t == "-iregex" {
                    format!("(?is)^(?:{body})$")
                } else {
                    format!("(?s)^(?:{body})$")
                };
                test(Test::Regex(
                    regex::Regex::new(&source)
                        .map_err(|e| Fail::usage(format!("find: `{t}': {e}")))?,
                ))
            }
            "-xtype" => {
                let _ = self.need(t)?;
                Err(Fail::usage(
                    "find: unknown predicate `-xtype` (this world never follows symbolic links)",
                ))
            }
            "-type" => {
                let kind = self.need(t)?;
                match kind.as_str() {
                    "f" | "d" | "l" => test(Test::Kind(kind.chars().next().unwrap_or('f'))),
                    "b" | "c" | "p" | "s" => Err(Fail::usage(format!(
                        "find: `-type {kind}': this world models no block, character, \
                         FIFO or socket nodes"
                    ))),
                    other => Err(Fail::usage(format!(
                        "find: unknown `-type' letter `{other}'"
                    ))),
                }
            }
            "-size" => {
                let spec = self.need(t)?;
                let (digits, unit) = match spec.chars().last() {
                    Some('c') => (&spec[..spec.len() - 1], 1),
                    Some('w') => (&spec[..spec.len() - 1], 2),
                    Some('b') => (&spec[..spec.len() - 1], 512),
                    Some('k') => (&spec[..spec.len() - 1], 1024),
                    Some('M') => (&spec[..spec.len() - 1], 1024 * 1024),
                    Some('G') => (&spec[..spec.len() - 1], 1024 * 1024 * 1024),
                    Some(c) if c.is_ascii_digit() => (spec.as_str(), 512),
                    _ => {
                        return Err(Fail::usage(format!(
                            "find: unknown `-size' unit in `{spec}' (use c, w, b, k, M or G)"
                        )))
                    }
                };
                test(Test::Size {
                    cmp: Cmp::parse(digits, "-size")?,
                    unit,
                })
            }
            "-perm" => {
                let spec = self.need(t)?;
                let (how, body) =
                    match spec.as_bytes().first() {
                        Some(b'-') => (PermMatch::All, &spec[1..]),
                        Some(b'/') => (PermMatch::Any, &spec[1..]),
                        Some(b'+') => return Err(Fail::usage(
                            "find: `-perm +MODE' was withdrawn by GNU find; write `-perm /MODE'",
                        )),
                        _ => (PermMatch::Exact, spec.as_str()),
                    };
                test(Test::Perm {
                    bits: parse_mode(body)?,
                    how,
                })
            }
            "-mtime" | "-atime" | "-ctime" | "-mmin" | "-amin" | "-cmin" => {
                let spec = self.need(t)?;
                let stamp = match t.as_bytes()[1] {
                    b'a' => Stamp::Accessed,
                    b'c' => Stamp::Changed,
                    _ => Stamp::Modified,
                };
                let unit = if t.ends_with("min") { MINUTE } else { DAY };
                test(Test::Time {
                    stamp,
                    unit,
                    cmp: Cmp::parse(&spec, t)?,
                })
            }
            "-newer" => {
                let operand = self.need(t)?;
                let meta = self
                    .c
                    .vfs
                    .lstat(&self.c.resolve(&operand))
                    .map_err(|e| Fail::new(format!("find: '{operand}': {}", reason(&e)), 1))?;
                test(Test::Newer(unix_micros(meta.modified)))
            }
            "-newermt" => {
                let spec = self.need(t)?;
                test(Test::Newer(parse_date(&spec)?))
            }
            "-empty" => test(Test::Empty),
            "-user" => test(Test::User(self.need(t)?)),
            "-group" => test(Test::Group(self.need(t)?)),
            "-nouser" => test(Test::NoUser),
            "-nogroup" => test(Test::NoGroup),
            "-maxdepth" => {
                self.maxdepth = self.count(t)?;
                Ok(Expr::True)
            }
            "-mindepth" => {
                self.mindepth = self.count(t)?;
                Ok(Expr::True)
            }
            "-depth" | "-d" => {
                self.post_order = true;
                Ok(Expr::True)
            }
            "-print" | "-print0" => {
                self.effects = true;
                Ok(Expr::Act(Action::Print(if t == "-print0" {
                    '\0'
                } else {
                    '\n'
                })))
            }
            "-printf" => {
                let format = self.need(t)?;
                self.effects = true;
                Ok(Expr::Act(Action::Printf(parse_format(&format)?)))
            }
            "-ls" => {
                self.effects = true;
                Ok(Expr::Act(Action::Ls))
            }
            "-delete" => {
                // GNU turns on -depth for -delete, otherwise a directory is visited
                // before the children that keep it from being removable.
                self.effects = true;
                self.post_order = true;
                Ok(Expr::Act(Action::Delete))
            }
            "-quit" => {
                self.effects = true;
                Ok(Expr::Act(Action::Quit))
            }
            "-prune" => Ok(Expr::Act(Action::Prune)),
            "-exec" | "-execdir" => {
                if self.depth >= NESTING_LIMIT {
                    return Err(Fail::usage(format!(
                        "find: `{t}': execution nesting exceeds {NESTING_LIMIT}"
                    )));
                }
                let mut argv: Vec<String> = Vec::new();
                let plus = loop {
                    let word = self.need(t)?;
                    if word == ";" {
                        break false;
                    }
                    if word == "+" && argv.last().is_some_and(|w| w == "{}") {
                        argv.pop();
                        break true;
                    }
                    argv.push(word);
                };
                if argv.is_empty() {
                    return Err(Fail::usage(format!("find: `{t}' needs a command")));
                }
                self.effects = true;
                let local = t == "-execdir";
                let batch = plus.then(|| {
                    self.batches.push(Batch {
                        argv: argv.clone(),
                        local,
                        files: Vec::new(),
                        cwd: None,
                    });
                    self.batches.len() - 1
                });
                Ok(Expr::Act(Action::Exec { argv, local, batch }))
            }
            "-ok" | "-okdir" => Err(Fail::usage(format!(
                "find: unknown predicate `{t}` (this world has no interactive terminal)"
            ))),
            "-follow" | "-L" | "-H" => Err(Fail::usage(format!(
                "find: unsupported option `{t}`: only `-P' (never follow symbolic links) \
                 is implemented"
            ))),
            "-P" => Ok(Expr::True),
            other => Err(Fail::usage(format!("find: unknown predicate `{other}`"))),
        }
    }
}

/// `0755` or `u+w,go-r`. Symbolic clauses are applied to a starting mode of zero, which
/// is what GNU find does: `-perm u+w` is `-perm 0200`, not "whatever umask says".
fn parse_mode(spec: &str) -> Result<u16, Fail> {
    if spec.is_empty() {
        return Err(Fail::usage("find: `-perm' expects a mode"));
    }
    if spec.bytes().all(|b| b.is_ascii_digit()) {
        return u16::from_str_radix(spec, 8)
            .ok()
            .filter(|m| *m <= 0o7777)
            .ok_or_else(|| Fail::usage(format!("find: invalid octal mode `{spec}'")));
    }
    let mut mode: u16 = 0;
    for clause in spec.split(',') {
        let cut = clause
            .find(['+', '-', '='])
            .ok_or_else(|| Fail::usage(format!("find: invalid mode `{spec}'")))?;
        let (who, rest) = clause.split_at(cut);
        let op = rest.as_bytes()[0];
        let perms = &rest[1..];
        let who: Vec<char> = if who.is_empty() || who.contains('a') {
            vec!['u', 'g', 'o']
        } else {
            who.chars().collect()
        };
        if let Some(bad) = who.iter().find(|c| !"ugo".contains(**c)) {
            return Err(Fail::usage(format!(
                "find: invalid `who' character `{bad}' in mode `{spec}'"
            )));
        }
        let mut low: u16 = 0;
        let mut special: u16 = 0;
        for p in perms.chars() {
            match p {
                'r' => low |= 4,
                'w' => low |= 2,
                'x' => low |= 1,
                's' => special |= 0o6000,
                't' => special |= 0o1000,
                'X' => {
                    return Err(Fail::usage(
                        "find: `-perm' symbolic `X' depends on the file being tested and is \
                         not implemented; write `x' or an octal mode",
                    ))
                }
                other => {
                    return Err(Fail::usage(format!(
                        "find: invalid permission character `{other}' in mode `{spec}'"
                    )))
                }
            }
        }
        let mut bits: u16 = 0;
        let mut cover: u16 = 0;
        for w in &who {
            let shift = match w {
                'u' => 6,
                'g' => 3,
                _ => 0,
            };
            bits |= low << shift;
            cover |= 7 << shift;
            match (w, special & 0o6000 != 0) {
                ('u', true) => bits |= 0o4000,
                ('g', true) => bits |= 0o2000,
                _ => {}
            }
        }
        if special & 0o1000 != 0 {
            bits |= 0o1000;
        }
        if special & 0o6000 != 0 {
            cover |= 0o6000;
        }
        if special & 0o1000 != 0 {
            cover |= 0o1000;
        }
        match op {
            b'+' => mode |= bits,
            b'-' => mode &= !bits,
            _ => mode = (mode & !cover) | bits,
        }
    }
    Ok(mode)
}

/// Days since 1970-01-01 from a civil date (Hinnant); no host clock is consulted.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `-newermt`: an absolute stamp only. `YYYY-MM-DD`, optionally `HH:MM[:SS]` after a
/// space or `T`, or `@SECONDS` since the Unix epoch. Relative words (`yesterday`,
/// `2 days ago`) are refused by name: they need a host clock this world does not have,
/// and the world clock is the tick the caller passed, not "now".
///
/// Returns microseconds since the Unix epoch, so a stamp before the world epoch still
/// compares correctly rather than clamping to tick zero.
fn parse_date(spec: &str) -> Result<i128, Fail> {
    if let Some(seconds) = spec.strip_prefix('@') {
        let unix: i64 = seconds
            .parse()
            .map_err(|_| Fail::usage(format!("find: `-newermt @{seconds}' is not a number")))?;
        return Ok(i128::from(unix) * i128::from(MICROS));
    }
    let (date, time) = match spec.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (spec, None),
    };
    let bad = || {
        Fail::usage(format!(
            "find: `-newermt {spec}': expected YYYY-MM-DD[ |T]HH:MM[:SS] or @SECONDS; \
             relative times need a host clock this world does not have"
        ))
    };
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 || parts[0].len() != 4 {
        return Err(bad());
    }
    let (y, m, d) = (
        parts[0].parse::<i64>().map_err(|_| bad())?,
        parts[1].parse::<i64>().map_err(|_| bad())?,
        parts[2].parse::<i64>().map_err(|_| bad())?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    let (mut hh, mut mm, mut ss) = (0i64, 0i64, 0i64);
    if let Some(time) = time {
        let fields: Vec<&str> = time.split(':').collect();
        if fields.len() < 2 || fields.len() > 3 {
            return Err(bad());
        }
        hh = fields[0].parse().map_err(|_| bad())?;
        mm = fields[1].parse().map_err(|_| bad())?;
        if let Some(s) = fields.get(2) {
            ss = s.parse().map_err(|_| bad())?;
        }
        if hh > 23 || mm > 59 || ss > 60 {
            return Err(bad());
        }
    }
    Ok(
        i128::from(days_from_civil(y, m, d) * 86_400 + hh * 3_600 + mm * 60 + ss)
            * i128::from(MICROS),
    )
}

/// A world tick as microseconds since the Unix epoch.
fn unix_micros(tick: u64) -> i128 {
    i128::from(EPOCH_UNIX_SECONDS) * i128::from(MICROS) + i128::from(tick)
}

/// `-printf`: every conversion is named here, and anything else is refused rather than
/// copied through, so a caller never mistakes an unimplemented field for a literal.
fn parse_format(format: &str) -> Result<Vec<Piece>, Fail> {
    let chars: Vec<char> = format.chars().collect();
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let next = chars.get(i + 1).ok_or_else(|| {
                    Fail::usage("find: `-printf' format ends with a lone backslash")
                })?;
                literal.push(match next {
                    'n' => '\n',
                    't' => '\t',
                    '0' => '\0',
                    '\\' => '\\',
                    other => {
                        return Err(Fail::usage(format!(
                            "find: unknown `-printf' escape `\\{other}'"
                        )))
                    }
                });
                i += 2;
            }
            '%' => {
                let next = *chars
                    .get(i + 1)
                    .ok_or_else(|| Fail::usage("find: `-printf' format ends with a lone `%'"))?;
                if next == '%' {
                    literal.push('%');
                    i += 2;
                    continue;
                }
                if !literal.is_empty() {
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                }
                let (conv, width) = match next {
                    'p' => (Conv::Path, 2),
                    'f' => (Conv::Base, 2),
                    'h' => (Conv::Dir, 2),
                    'n' => (Conv::Links, 2),
                    's' => (Conv::Size, 2),
                    'm' => (Conv::Octal, 2),
                    'M' => (Conv::Modes, 2),
                    'u' => (Conv::Owner, 2),
                    'g' => (Conv::Group, 2),
                    'y' => (Conv::Kind, 2),
                    'i' => (Conv::Inode, 2),
                    'd' => (Conv::Depth, 2),
                    'P' => (Conv::Relative, 2),
                    'l' => (Conv::Target, 2),
                    'T' | 'A' | 'C' => {
                        let stamp = match next {
                            'A' => Stamp::Accessed,
                            'C' => Stamp::Changed,
                            _ => Stamp::Modified,
                        };
                        let field = *chars.get(i + 2).ok_or_else(|| {
                            Fail::usage(format!("find: `-printf' `%{next}' needs a field letter"))
                        })?;
                        let conv = match field {
                            '@' => Conv::Epoch(stamp),
                            'Y' | 'm' | 'd' | 'H' | 'M' | 'S' => Conv::Field(stamp, field),
                            other => {
                                return Err(Fail::usage(format!(
                                    "find: unknown `-printf' directive `%{next}{other}'"
                                )))
                            }
                        };
                        (conv, 3)
                    }
                    other => {
                        return Err(Fail::usage(format!(
                            "find: unknown `-printf' directive `%{other}'"
                        )))
                    }
                };
                pieces.push(Piece::Conv(conv));
                i += width;
            }
            other => {
                literal.push(other);
                i += 1;
            }
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    Ok(pieces)
}

// ---------------------------------------------------------------------------
// Walk
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Batch {
    argv: Vec<String>,
    local: bool,
    files: Vec<String>,
    /// The directory `-execdir` runs from; a batch is flushed when it changes.
    cwd: Option<String>,
}

struct Node {
    display: String,
    resolved: String,
    meta: Metadata,
    depth: usize,
}

struct Walk<'a> {
    out: String,
    errors: Vec<String>,
    /// A modelled failure that is not itself a message (a non-zero `-exec … +`).
    failed: bool,
    quit: bool,
    pruned: bool,
    post_order: bool,
    mindepth: usize,
    maxdepth: usize,
    batches: Vec<Batch>,
    /// The root operand this subtree came from, for `%P` and the relative depth.
    root: String,
    users: &'a BTreeSet<String>,
    groups: &'a BTreeSet<String>,
    t: u64,
}

impl Walk<'_> {
    fn diagnose(&mut self, operand: &str, e: &VfsError) {
        self.errors
            .push(format!("find: '{operand}': {}", reason(e)));
    }
}

/// Single-quote one argument for the shell line `-exec` builds. Every byte survives:
/// a single quote closes the quoting, escapes itself and reopens it.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

fn parent_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((head, _)) => head.to_string(),
    }
}

fn base_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        _ if path == "/" => "/".to_string(),
        _ => path.to_string(),
    }
}

fn run_line(
    c: &mut Computer,
    w: &mut Walk,
    host: &mut dyn ShellHost,
    argv: &[String],
    cwd: Option<String>,
) -> i32 {
    let line = argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    // The command runs in a child on a real system, so neither its working directory
    // nor `-execdir`'s temporary one may leak back into the shell.
    let target = cwd.unwrap_or_else(|| c.cwd.clone());
    let saved = std::mem::replace(&mut c.cwd, target);
    let result = crate::shell::execute(c, &line, w.t, host);
    c.cwd = saved;
    w.out.push_str(&result.stdout);
    // The child's diagnostics are its own: GNU passes them to find's stderr and leaves
    // find's exit status alone. `run` returns one string and `Fail::nested` is private,
    // so they are carried in that string rather than dropped — shell.rs's own rule is
    // that a nested run never loses what it printed. The status is still the child's.
    w.out.push_str(&result.stderr);
    result.exit_code
}

fn flush(c: &mut Computer, w: &mut Walk, host: &mut dyn ShellHost, id: usize) {
    if w.batches[id].files.is_empty() {
        return;
    }
    let mut argv = w.batches[id].argv.clone();
    argv.append(&mut w.batches[id].files);
    let cwd = w.batches[id].cwd.take();
    if run_line(c, w, host, &argv, cwd) != 0 {
        w.failed = true;
    }
}

fn exec(
    c: &mut Computer,
    w: &mut Walk,
    host: &mut dyn ShellHost,
    argv: &[String],
    local: bool,
    batch: Option<usize>,
    n: &Node,
) -> bool {
    let dir = parent_of(&n.resolved);
    let name = if local {
        format!("./{}", base_of(&n.display))
    } else {
        n.display.clone()
    };
    let cwd = local.then(|| dir.clone());
    let Some(id) = batch else {
        let line: Vec<String> = argv.iter().map(|a| a.replace("{}", &name)).collect();
        return run_line(c, w, host, &line, cwd) == 0;
    };
    // `-execdir … +` groups per directory, as GNU does, so `./name` stays meaningful.
    if w.batches[id].local && w.batches[id].cwd.as_deref() != Some(dir.as_str()) {
        flush(c, w, host, id);
    }
    w.batches[id].cwd = cwd.or_else(|| w.batches[id].cwd.clone());
    w.batches[id].files.push(name);
    true
}

fn render(w: &Walk, pieces: &[Piece], n: &Node) -> String {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            Piece::Literal(s) => out.push_str(s),
            Piece::Conv(conv) => out.push_str(&convert(w, *conv, n)),
        }
    }
    out
}

fn convert(w: &Walk, conv: Conv, n: &Node) -> String {
    let m = &n.meta;
    match conv {
        Conv::Path => n.display.clone(),
        Conv::Base => base_of(&n.display),
        Conv::Dir => {
            let head = parent_of(&n.display);
            if n.display.contains('/') {
                head
            } else {
                ".".into()
            }
        }
        Conv::Links => m.links.to_string(),
        Conv::Size => apparent(m).to_string(),
        Conv::Octal => format!("{:o}", m.mode & 0o7777),
        Conv::Modes => mode_string(m.mode, m.is_dir, m.is_symlink),
        Conv::Owner => m.owner.clone(),
        Conv::Group => m.group.clone(),
        Conv::Kind => kind_letter(m).to_string(),
        Conv::Inode => m.inode.to_string(),
        Conv::Depth => n.depth.to_string(),
        Conv::Relative => relative(&w.root, &n.display),
        Conv::Target => m.target.clone().unwrap_or_default(),
        Conv::Epoch(stamp) => {
            let tick = stamp.of(m);
            format!(
                "{}.{:06}",
                EPOCH_UNIX_SECONDS + tick / MICROS,
                tick % MICROS
            )
        }
        Conv::Field(stamp, field) => {
            let at = clock(stamp.of(m));
            match field {
                'Y' => format!("{:04}", at.year),
                'm' => format!("{:02}", at.month),
                'd' => format!("{:02}", at.day),
                'H' => format!("{:02}", at.hour),
                'M' => format!("{:02}", at.minute),
                _ => format!("{:02}", at.second),
            }
        }
    }
}

fn kind_letter(m: &Metadata) -> char {
    if m.is_symlink {
        'l'
    } else if m.is_dir {
        'd'
    } else {
        'f'
    }
}

/// `%P`: the path with its starting point removed.
fn relative(root: &str, display: &str) -> String {
    display
        .strip_prefix(root)
        .unwrap_or(display)
        .trim_start_matches('/')
        .to_string()
}

/// `-ls`: inode, 1 KiB blocks, mode, links, owner, group, size, stamp, path. The
/// column widths are fixed here rather than fitted to the widest row, because find
/// streams its output and GNU does the same.
fn long(n: &Node) -> String {
    let m = &n.meta;
    let bytes = apparent(m);
    let at = clock(m.modified);
    let target = match &m.target {
        Some(t) => format!(" -> {t}"),
        None => String::new(),
    };
    format!(
        "{:>9} {:>6} {} {:>3} {:<8} {:<8} {:>8} {} {:2} {:02}:{:02} {}{}\n",
        m.inode,
        bytes.div_ceil(1024),
        mode_string(m.mode, m.is_dir, m.is_symlink),
        m.links,
        m.owner,
        m.group,
        bytes,
        MONTHS[(at.month as usize).clamp(1, 12) - 1],
        at.day,
        at.hour,
        at.minute,
        n.display,
        target
    )
}

fn matches(w: &Walk, test: &Test, n: &Node) -> bool {
    let m = &n.meta;
    match test {
        Test::Name { pattern, fold } => {
            let name = base_of(&n.display);
            if *fold {
                wildcard(&pattern.to_lowercase(), &name.to_lowercase())
            } else {
                wildcard(pattern, &name)
            }
        }
        Test::Path { pattern, fold } => {
            if *fold {
                wildcard(&pattern.to_lowercase(), &n.display.to_lowercase())
            } else {
                wildcard(pattern, &n.display)
            }
        }
        Test::Regex(re) => re.is_match(&n.display),
        Test::Kind('d') => m.is_dir,
        Test::Kind('l') => m.is_symlink,
        Test::Kind(_) => !m.is_dir && !m.is_symlink,
        Test::Size { cmp, unit } => cmp.test(apparent(m).div_ceil(*unit)),
        Test::Perm { bits, how } => {
            let mode = m.mode & 0o7777;
            match how {
                PermMatch::Exact => mode == *bits,
                PermMatch::All => mode & bits == *bits,
                PermMatch::Any => *bits == 0 || mode & bits != 0,
            }
        }
        Test::Time { stamp, unit, cmp } => {
            // GNU truncates: -mtime 1 is "strictly between 24 and 48 hours old".
            cmp.test(w.t.saturating_sub(stamp.of(m)) / unit)
        }
        Test::Newer(stamp) => unix_micros(m.modified) > *stamp,
        // A directory's stored size is its child count, which is exactly the question
        // -empty asks; a symbolic link is never empty.
        Test::Empty => !m.is_symlink && m.size == 0,
        Test::User(name) => m.owner == *name,
        Test::Group(name) => m.group == *name,
        Test::NoUser => !w.users.contains(&m.owner),
        Test::NoGroup => !w.groups.contains(&m.group),
    }
}

fn act(
    c: &mut Computer,
    w: &mut Walk,
    host: &mut dyn ShellHost,
    action: &Action,
    n: &Node,
) -> bool {
    match action {
        Action::Print(end) => {
            w.out.push_str(&n.display);
            w.out.push(*end);
            true
        }
        Action::Printf(pieces) => {
            let text = render(w, pieces, n);
            w.out.push_str(&text);
            true
        }
        Action::Ls => {
            let row = long(n);
            w.out.push_str(&row);
            true
        }
        Action::Delete => {
            if n.display == "." || n.resolved == "/" {
                w.errors
                    .push(format!("find: refusing to delete '{}'", n.display));
                w.failed = true;
                return false;
            }
            let user = c.user.clone();
            match c.vfs.remove_as(&n.resolved, false, &user) {
                Ok(()) => true,
                Err(e) => {
                    w.errors.push(format!(
                        "find: cannot delete '{}': {}",
                        n.display,
                        reason(&e)
                    ));
                    w.failed = true;
                    false
                }
            }
        }
        Action::Quit => {
            w.quit = true;
            true
        }
        Action::Prune => {
            if n.meta.is_dir {
                w.pruned = true;
            }
            true
        }
        Action::Exec { argv, local, batch } => exec(c, w, host, argv, *local, *batch, n),
    }
}

fn eval(c: &mut Computer, w: &mut Walk, host: &mut dyn ShellHost, e: &Expr, n: &Node) -> bool {
    match e {
        Expr::True => true,
        Expr::Not(inner) => !eval(c, w, host, inner, n),
        Expr::And(a, b) => eval(c, w, host, a, n) && eval(c, w, host, b, n),
        Expr::Or(a, b) => eval(c, w, host, a, n) || eval(c, w, host, b, n),
        Expr::Test(t) => matches(w, t, n),
        Expr::Act(a) => act(c, w, host, a, n),
    }
}

fn visit(
    c: &mut Computer,
    w: &mut Walk,
    host: &mut dyn ShellHost,
    e: &Expr,
    display: &str,
    depth: usize,
) {
    if w.quit {
        return;
    }
    if depth > WALK_LIMIT {
        w.errors.push(format!(
            "find: '{display}': directory nesting exceeds {WALK_LIMIT}"
        ));
        w.failed = true;
        return;
    }
    let resolved = c.resolve(display);
    let meta = match c.vfs.lstat(&resolved) {
        Ok(m) => m,
        Err(err) => {
            w.diagnose(display, &err);
            return;
        }
    };
    let node = Node {
        display: display.to_string(),
        resolved,
        meta,
        depth,
    };
    let mut pruned = false;
    let selected = depth >= w.mindepth && depth <= w.maxdepth;
    if !w.post_order && selected {
        w.pruned = false;
        eval(c, w, host, e, &node);
        pruned = w.pruned;
    }
    if node.meta.is_dir && depth < w.maxdepth && !pruned && !w.quit {
        let user = c.user.clone();
        match c.vfs.list_as(&node.resolved, &user) {
            Ok(names) => {
                let stem = display.trim_end_matches('/');
                for name in names {
                    visit(c, w, host, e, &format!("{stem}/{name}"), depth + 1);
                    if w.quit {
                        break;
                    }
                }
            }
            Err(err) => w.diagnose(display, &err),
        }
    }
    if w.post_order && selected && !w.quit {
        w.pruned = false;
        eval(c, w, host, e, &node);
    }
}

/// Names the world can account for. There is no passwd database in the substrate, so
/// `-nouser` asks a narrower but honest question: is this owner one the computer knows
/// — its own user, root, or a line in `/etc/passwd`?
fn roster(c: &Computer, file: &str, extra: &[&str]) -> BTreeSet<String> {
    let mut set: BTreeSet<String> = extra.iter().map(|s| (*s).to_string()).collect();
    if let Ok(bytes) = c.vfs.read(file) {
        for line in String::from_utf8_lossy(&bytes).lines() {
            if let Some(name) = line.split(':').next().filter(|s| !s.is_empty()) {
                set.insert(name.to_string());
            }
        }
    }
    set
}

pub(crate) fn run(
    c: &mut Computer,
    args: &[String],
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, Fail> {
    let mut i = 0;
    // The -P/-L/-H family precedes the roots. Only -P, the default, is honest here.
    while let Some(arg) = args.get(i).map(String::as_str) {
        match arg {
            "-P" => i += 1,
            "-L" | "-H" | "-follow" => {
                return Err(Fail::usage(format!(
                    "find: unsupported option `{arg}`: only `-P' (never follow symbolic \
                     links) is implemented"
                )))
            }
            _ => break,
        }
    }
    let mut roots: Vec<String> = Vec::new();
    while let Some(arg) = args.get(i) {
        if arg.starts_with('-') || arg == "(" || arg == ")" || arg == "!" || arg == "," {
            break;
        }
        roots.push(arg.clone());
        i += 1;
    }
    if roots.is_empty() {
        roots.push(".".into());
    }
    let mut parser = Parser {
        args: &args[i..],
        i: 0,
        c,
        depth,
        effects: false,
        post_order: false,
        mindepth: 0,
        maxdepth: usize::MAX,
        batches: Vec::new(),
    };
    let expression = if parser.args.is_empty() {
        Expr::True
    } else {
        parser.expression()?
    };
    // GNU appends -print when nothing in the expression has a side effect; -prune has
    // none, which is why `find . -prune` still prints `.`.
    let expression = if parser.effects {
        expression
    } else {
        Expr::And(
            Box::new(expression),
            Box::new(Expr::Act(Action::Print('\n'))),
        )
    };
    let (post_order, mindepth, maxdepth, batches) = (
        parser.post_order,
        parser.mindepth,
        parser.maxdepth,
        parser.batches,
    );
    let users = roster(c, "/etc/passwd", &["root", c.user.as_str()]);
    // A new node's group is its owner's login group (the Debian one-group-per-user
    // convention the VFS follows), so the computer's own user names a group too.
    let groups = roster(
        c,
        "/etc/group",
        &["root", crate::vfs::DEFAULT_GROUP, c.user.as_str()],
    );
    let mut w = Walk {
        out: String::new(),
        errors: Vec::new(),
        failed: false,
        quit: false,
        pruned: false,
        post_order,
        mindepth,
        maxdepth,
        batches,
        root: String::new(),
        users: &users,
        groups: &groups,
        t,
    };
    for root in &roots {
        if w.quit {
            break;
        }
        let stem = if root.len() > 1 {
            root.trim_end_matches('/')
        } else {
            root.as_str()
        };
        w.root = stem.to_string();
        visit(c, &mut w, host, &expression, stem, 0);
    }
    for id in 0..w.batches.len() {
        flush(c, &mut w, host, id);
    }
    if w.errors.is_empty() && !w.failed {
        return Ok(w.out);
    }
    // GNU find prints what it found and still exits non-zero: the diagnostics go to
    // stderr, the matches it did produce go to stdout.
    Err(Fail::new(w.errors.join("\n"), 1).with_output(w.out))
}

#[cfg(test)]
mod tests {
    use crate::{shell, Computer, OfflineHost};

    /// A computer with a small tree: three directories, four files, one symlink.
    /// The shell builds what the shell owns; the symlink, the zero-byte file and the
    /// modes go straight to the VFS so this fixture cannot drift with another command.
    fn machine() -> Computer {
        let mut c = Computer::new("box", "user", "linux", true);
        for line in [
            "mkdir -p /home/user/t/sub",
            "mkdir -p /home/user/t/empty",
            "echo hello > /home/user/t/a.txt",
            "echo deep > /home/user/t/sub/b.log",
            "echo up > /home/user/t/sub/C.TXT",
        ] {
            let r = shell::execute(&mut c, line, 0, &mut OfflineHost);
            assert_eq!(r.exit_code, 0, "setup `{line}`: {}", r.stderr);
        }
        c.vfs
            .write("/home/user/t/blank.txt", b"", "user", 0)
            .expect("zero-byte file");
        c.vfs
            .symlink("a.txt", "/home/user/t/link", "user", 0)
            .expect("symlink");
        c.vfs.chmod("/home/user/t/a.txt", 0o644).expect("chmod");
        c.vfs.chmod("/home/user/t/sub/b.log", 0o755).expect("chmod");
        c
    }

    /// stdout, stderr and status, with the world clock at `t`.
    fn at(c: &mut Computer, line: &str, t: u64) -> (String, String, i32) {
        let r = shell::execute(c, line, t, &mut OfflineHost);
        (r.stdout, r.stderr, r.exit_code)
    }
    fn run(c: &mut Computer, line: &str) -> (String, i32) {
        let (out, err, code) = at(c, line, 1);
        (format!("{out}{err}"), code)
    }
    fn lines(out: &str) -> Vec<String> {
        out.lines().map(str::to_string).collect()
    }

    #[test]
    fn find_name_and_iname_filter_on_the_basename() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user/t -name 'a.txt'");
        assert_eq!(out, "/home/user/t/a.txt\n");
        assert_eq!(code, 0);
        let (out, _) = run(&mut c, "find /home/user/t -name 'nothing'");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -name '*.TXT'");
        assert_eq!(out, "/home/user/t/sub/C.TXT\n");
        let (out, _) = run(&mut c, "find /home/user/t -iname 'c.txt'");
        assert_eq!(out, "/home/user/t/sub/C.TXT\n");
        let (out, _) = run(&mut c, "find /home/user/t -iname 'zzz'");
        assert_eq!(out, "");
    }

    #[test]
    fn find_path_and_ipath_match_the_whole_path() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -path '*/sub/*'");
        assert_eq!(
            lines(&out),
            ["/home/user/t/sub/C.TXT", "/home/user/t/sub/b.log"]
        );
        let (out, _) = run(&mut c, "find /home/user/t -path '*/nope/*'");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -ipath '*/SUB/*.LOG'");
        assert_eq!(out, "/home/user/t/sub/b.log\n");
        let (out, _) = run(&mut c, "find /home/user/t -ipath '*/SUB/*.zzz'");
        assert_eq!(out, "");
    }

    #[test]
    fn find_regex_and_iregex_anchor_the_whole_path() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -regex '.*/a[.]t.t'");
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _) = run(&mut c, "find /home/user/t -regex 'a[.]txt'");
        assert_eq!(
            out, "",
            "the regex is anchored to the whole path, not the name"
        );
        let (out, _) = run(&mut c, "find /home/user/t -iregex '.*/c[.]txt'");
        assert_eq!(out, "/home/user/t/sub/C.TXT\n");
        let (out, code) = run(&mut c, "find /home/user/t -regex '['");
        assert_eq!(code, 2, "{out}");
    }

    #[test]
    fn find_type_selects_files_directories_and_links_and_refuses_device_nodes() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -type d");
        assert_eq!(
            lines(&out),
            ["/home/user/t", "/home/user/t/empty", "/home/user/t/sub"]
        );
        let (out, _) = run(&mut c, "find /home/user/t -type l");
        assert_eq!(out, "/home/user/t/link\n");
        let (out, _) = run(&mut c, "find /home/user/t -type f -name 'link'");
        assert_eq!(out, "", "a symbolic link is not a regular file");
        let (out, code) = run(&mut c, "find /home/user/t -type b");
        assert_eq!(code, 2);
        assert!(out.contains("-type b"), "{out}");
        let (_, code) = run(&mut c, "find /home/user/t -type q");
        assert_eq!(code, 2);
    }

    #[test]
    fn find_size_understands_every_unit_and_sign() {
        let mut c = machine();
        c.vfs
            .write("/home/user/t/big", &vec![b'x'; 3000], "user", 0)
            .expect("big file");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size -1c");
        assert_eq!(out, "/home/user/t/blank.txt\n");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size 3000c");
        assert_eq!(out, "/home/user/t/big\n");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size +2k");
        assert_eq!(out, "/home/user/t/big\n", "3000 bytes rounds up to 3 KiB");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size 3k");
        assert_eq!(out, "/home/user/t/big\n");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size +1M");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size -1G");
        assert_eq!(
            out, "/home/user/t/blank.txt\n",
            "GNU rounds up first, so a 3000-byte file is one whole G unit"
        );
        let (out, _) = run(&mut c, "find /home/user/t -type f -size +5b");
        assert_eq!(out, "/home/user/t/big\n", "bare b is 512-byte blocks");
        let (out, _) = run(&mut c, "find /home/user/t -type f -size +5");
        assert_eq!(out, "/home/user/t/big\n", "a bare number is blocks too");
        let (_, code) = run(&mut c, "find /home/user/t -size 3Q");
        assert_eq!(code, 2);
    }

    #[test]
    fn find_perm_matches_exactly_all_bits_or_any_bit() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm 644");
        assert!(out.contains("/home/user/t/a.txt"), "{out}");
        assert!(!out.contains("b.log"), "{out}");
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm 777");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm -111");
        assert_eq!(out, "/home/user/t/sub/b.log\n", "all of these bits");
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm -7777");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm /111");
        assert_eq!(out, "/home/user/t/sub/b.log\n", "any of these bits");
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm /4000");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -type f -perm -u+x");
        assert_eq!(
            out, "/home/user/t/sub/b.log\n",
            "symbolic modes are honoured"
        );
        let (out, code) = run(&mut c, "find /home/user/t -perm -u+X");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains('X'), "{out}");
    }

    #[test]
    fn find_time_predicates_measure_back_from_the_world_clock() {
        let mut c = machine();
        // The tree was written at tick 0; run the search three days later.
        let three_days = 3 * 86_400 * 1_000_000;
        let (out, _, _) = at(&mut c, "find /home/user/t -name a.txt -mtime 3", three_days);
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _, _) = at(&mut c, "find /home/user/t -name a.txt -mtime 2", three_days);
        assert_eq!(out, "", "GNU truncates: exactly three days old is -mtime 3");
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -name a.txt -mtime +2",
            three_days,
        );
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -name a.txt -mtime -1",
            three_days,
        );
        assert_eq!(out, "");
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -name a.txt -mmin +10",
            three_days,
        );
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -name a.txt -mmin -10",
            three_days,
        );
        assert_eq!(out, "");
        for spec in ["-atime 3", "-ctime 3", "-amin +10", "-cmin +10"] {
            let line = format!("find /home/user/t -name a.txt {spec}");
            let (out, _, _) = at(&mut c, &line, three_days);
            assert_eq!(out, "/home/user/t/a.txt\n", "{spec}");
        }
        let (_, code) = run(&mut c, "find /home/user/t -mtime x");
        assert_eq!(code, 2);
    }

    #[test]
    fn find_newer_compares_against_a_file_and_newermt_against_a_stamp() {
        let mut c = machine();
        let later = 5 * 86_400 * 1_000_000;
        let r = shell::execute(
            &mut c,
            "echo fresh > /home/user/t/new.txt",
            later,
            &mut OfflineHost,
        );
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        let (out, _, _) = at(&mut c, "find /home/user/t -newer /home/user/t/a.txt", later);
        assert_eq!(out, "/home/user/t/new.txt\n");
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -newer /home/user/t/new.txt",
            later,
        );
        assert_eq!(out, "", "nothing is newer than the newest file");
        let (out, _, _) = at(&mut c, "find /home/user/t -type f -newermt @0", later);
        assert!(out.contains("/home/user/t/a.txt"), "{out}");
        // The world epoch is 2026-09-17; a stamp after it excludes the tick-0 tree.
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -newermt '2026-09-19 00:00'",
            later,
        );
        assert_eq!(out, "/home/user/t/new.txt\n");
        let (out, _, _) = at(
            &mut c,
            "find /home/user/t -newermt 2030-01-01T00:00:00",
            later,
        );
        assert_eq!(out, "");
        let (out, code) = run(&mut c, "find /home/user/t -newermt yesterday");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("yesterday"), "{out}");
        let (out, code) = run(&mut c, "find /home/user/t -newer /no/such/file");
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("No such file or directory"), "{out}");
    }

    #[test]
    fn find_empty_matches_zero_byte_files_and_childless_directories() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -empty");
        assert_eq!(
            lines(&out),
            ["/home/user/t/blank.txt", "/home/user/t/empty"]
        );
        let (out, _) = run(&mut c, "find /home/user/t -type d -empty -name sub");
        assert_eq!(out, "", "a directory with children is not empty");
    }

    #[test]
    fn find_user_and_group_predicates_filter_by_ownership() {
        let mut c = machine();
        let (out, _) = run(
            &mut c,
            "find /home/user/t -maxdepth 1 -user user -name a.txt",
        );
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _) = run(&mut c, "find /home/user/t -user root");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -maxdepth 0 -group user");
        assert_eq!(out, "/home/user/t\n");
        let (out, _) = run(&mut c, "find /home/user/t -group nobody");
        assert_eq!(out, "");
        let (out, _) = run(&mut c, "find /home/user/t -nouser");
        assert_eq!(out, "", "every node here is owned by a user the box knows");
        c.vfs
            .chown_as(
                "/home/user/t/a.txt",
                Some("ghost"),
                Some("ghouls"),
                "root",
                0,
                false,
            )
            .expect("chown");
        let (out, _) = run(&mut c, "find /home/user/t -nouser");
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _) = run(&mut c, "find /home/user/t -nogroup");
        assert_eq!(out, "/home/user/t/a.txt\n");
    }

    #[test]
    fn find_maxdepth_and_mindepth_bound_the_walk() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -maxdepth 0");
        assert_eq!(out, "/home/user/t\n");
        let (out, _) = run(&mut c, "find /home/user/t -maxdepth 1 -type f");
        assert_eq!(
            lines(&out),
            ["/home/user/t/a.txt", "/home/user/t/blank.txt"]
        );
        let (out, _) = run(&mut c, "find /home/user/t -mindepth 2");
        assert_eq!(
            lines(&out),
            ["/home/user/t/sub/C.TXT", "/home/user/t/sub/b.log"]
        );
        let (_, code) = run(&mut c, "find /home/user/t -maxdepth x");
        assert_eq!(code, 2);
    }

    #[test]
    fn find_prune_stops_the_descent_but_keeps_the_default_print() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -name sub -prune -o -print");
        assert_eq!(
            lines(&out),
            [
                "/home/user/t",
                "/home/user/t/a.txt",
                "/home/user/t/blank.txt",
                "/home/user/t/empty",
                "/home/user/t/link",
            ]
        );
        let (out, _) = run(&mut c, "find /home/user/t -maxdepth 1 -name sub -prune");
        assert_eq!(
            out, "/home/user/t/sub\n",
            "-prune has no side effect, so -print is added"
        );
    }

    #[test]
    fn find_boolean_operators_group_and_short_circuit_with_real_precedence() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -type f -name '*.txt'");
        assert_eq!(
            lines(&out),
            ["/home/user/t/a.txt", "/home/user/t/blank.txt"]
        );
        let (out, _) = run(&mut c, "find /home/user/t -name '*.log' -o -name '*.TXT'");
        assert_eq!(
            lines(&out),
            ["/home/user/t/sub/C.TXT", "/home/user/t/sub/b.log"]
        );
        let (out, _) = run(&mut c, "find /home/user/t ! -type d -a ! -type l");
        assert_eq!(
            lines(&out),
            [
                "/home/user/t/a.txt",
                "/home/user/t/blank.txt",
                "/home/user/t/sub/C.TXT",
                "/home/user/t/sub/b.log",
            ]
        );
        // -a binds tighter than -o: `d -a name=empty` OR `name=link`.
        let (loose, _) = run(
            &mut c,
            "find /home/user/t -type d -a -name empty -o -name link",
        );
        assert_eq!(lines(&loose), ["/home/user/t/empty", "/home/user/t/link"]);
        // Parentheses change that: d AND (empty OR link) keeps only the directory.
        let (tight, _) = run(
            &mut c,
            "find /home/user/t -type d -a '(' -name empty -o -name link ')'",
        );
        assert_eq!(lines(&tight), ["/home/user/t/empty"]);
        // ! binds tighter than -a.
        let (out, _) = run(&mut c, "find /home/user/t ! -type f -a -name link");
        assert_eq!(out, "/home/user/t/link\n");
        let (out, code) = run(&mut c, "find /home/user/t -name a.txt -o");
        assert_eq!(code, 2, "{out}");
        let (out, code) = run(&mut c, "find /home/user/t '(' -type f");
        assert_eq!(code, 2, "{out}");
    }

    #[test]
    fn find_short_circuits_before_a_side_effecting_action() {
        let mut c = machine();
        // The left side is false for everything, so -delete must never run.
        let (_, code) = run(&mut c, "find /home/user/t -name 'zzz' -a -delete");
        assert_eq!(code, 0);
        let (out, _) = run(&mut c, "find /home/user/t -type f");
        assert_eq!(out.lines().count(), 4, "{out}");
    }

    #[test]
    fn find_print0_separates_with_nul() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t -type d -print0");
        assert_eq!(out, "/home/user/t\0/home/user/t/empty\0/home/user/t/sub\0");
    }

    #[test]
    fn find_printf_renders_every_supported_conversion() {
        let mut c = machine();
        let (out, _) = run(
            &mut c,
            r"find /home/user/t -name a.txt -printf '%p|%f|%h|%P|%d\n'",
        );
        assert_eq!(out, "/home/user/t/a.txt|a.txt|/home/user/t|a.txt|1\n");
        let (out, _) = run(
            &mut c,
            r"find /home/user/t -name a.txt -printf '%n|%s|%m|%M|%y\n'",
        );
        assert_eq!(out, "1|6|644|-rw-r--r--|f\n");
        let (out, _) = run(&mut c, r"find /home/user/t -name a.txt -printf '%u:%g\n'");
        assert_eq!(
            out, "user:user\n",
            "a new node lands in its owner's login group"
        );
        let (out, _) = run(&mut c, r"find /home/user/t -maxdepth 0 -printf '%y %P\n'");
        assert_eq!(out, "d \n", "a starting point has an empty %P");
        let (out, _) = run(&mut c, r"find /home/user/t -name link -printf '%y %l\n'");
        assert_eq!(out, "l a.txt\n");
        let (out, _) = run(&mut c, r"find /home/user/t -name a.txt -printf '%i\n'");
        assert!(out.trim().parse::<u64>().is_ok(), "{out}");
        let (out, _) = run(
            &mut c,
            r"find /home/user/t -name a.txt -printf '%T@|%A@|%C@\n'",
        );
        assert_eq!(
            out,
            "1789635600.000000|1789635600.000000|1789635600.000000\n"
        );
        let (out, _) = run(
            &mut c,
            r"find /home/user/t -name a.txt -printf '%TY-%Tm-%Td %TH:%TM:%TS\n'",
        );
        assert_eq!(out, "2026-09-17 09:00:00\n");
        let (out, _) = run(&mut c, r"find /home/user/t -name a.txt -printf 'a%%b\tc\n'");
        assert_eq!(out, "a%b\tc\n");
        let (out, _) = run(&mut c, r"find /home/user/t -name a.txt -printf 'x\0y'");
        assert_eq!(out, "x\0y");
    }

    #[test]
    fn find_printf_refuses_a_conversion_it_does_not_implement() {
        let mut c = machine();
        let (out, code) = run(&mut c, r"find /home/user/t -printf '%k\n'");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("%k"), "{out}");
        let (out, code) = run(&mut c, r"find /home/user/t -printf '%TQ\n'");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("%TQ"), "{out}");
        let (out, code) = run(&mut c, r"find /home/user/t -printf 'a\q'");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("\\q"), "{out}");
    }

    #[test]
    fn find_ls_prints_a_long_listing_row_per_match() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user/t -name a.txt -ls");
        assert_eq!(code, 0);
        let row = out.trim_end();
        assert!(row.contains("-rw-r--r--"), "{row}");
        assert!(row.contains(" user     user "), "{row}");
        assert!(row.ends_with("/home/user/t/a.txt"), "{row}");
        assert!(row.contains("Sep 17 09:00"), "{row}");
        let (out, _) = run(&mut c, "find /home/user/t -name link -ls");
        assert!(out.contains("-> a.txt"), "{out}");
        let (out, _) = run(&mut c, "find /home/user/t -name nothing -ls");
        assert_eq!(out, "");
    }

    #[test]
    fn find_delete_removes_a_non_empty_tree_depth_first() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user/t/sub -delete");
        assert_eq!(code, 0, "{out}");
        assert_eq!(out, "", "-delete replaces the default -print");
        assert!(!c.vfs.exists("/home/user/t/sub"), "the tree is gone");
        assert!(c.vfs.exists("/home/user/t/a.txt"), "siblings survive");
        // A selective delete only removes what the expression picked.
        let (_, code) = run(&mut c, "find /home/user/t -name 'a.txt' -delete");
        assert_eq!(code, 0);
        assert!(!c.vfs.exists("/home/user/t/a.txt"));
        assert!(c.vfs.exists("/home/user/t/blank.txt"));
    }

    #[test]
    fn find_delete_reports_a_failure_without_abandoning_the_walk() {
        let mut c = machine();
        let r = shell::execute(&mut c, "chmod 500 /home/user/t/sub", 0, &mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        let (out, code) = run(&mut c, "find /home/user/t/sub -name 'b.log' -delete");
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("cannot delete"), "{out}");
        assert!(c.vfs.exists("/home/user/t/sub/b.log"));
    }

    #[test]
    fn find_exec_runs_one_command_per_file_and_substitutes_braces() {
        let mut c = machine();
        let (out, code) = run(
            &mut c,
            "find /home/user/t -type f -name '*.txt' -exec echo saw {} ';'",
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            lines(&out),
            ["saw /home/user/t/a.txt", "saw /home/user/t/blank.txt"]
        );
        // A non-zero -exec makes the predicate false, so the -print never happens.
        let (out, _) = run(
            &mut c,
            "find /home/user/t -name a.txt -exec false ';' -print",
        );
        assert_eq!(out, "");
        let (out, _) = run(
            &mut c,
            "find /home/user/t -name a.txt -exec true ';' -print",
        );
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, code) = run(&mut c, "find /home/user/t -exec echo hi");
        assert_eq!(code, 2, "{out}");
        // A child that writes a diagnostic is not find's failure; the text survives.
        let (out, code) = run(
            &mut c,
            "find /home/user/t -name a.txt -exec cat /no/such/file ';'",
        );
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("cat:"), "{out}");
    }

    #[test]
    fn find_exec_plus_batches_every_match_into_one_command() {
        let mut c = machine();
        let (out, code) = run(
            &mut c,
            "find /home/user/t -type f -name '*.txt' -exec echo {} +",
        );
        assert_eq!(code, 0, "{out}");
        assert_eq!(out, "/home/user/t/a.txt /home/user/t/blank.txt\n");
        let (out, _) = run(&mut c, "find /home/user/t -name 'nothing' -exec echo {} +");
        assert_eq!(out, "", "an empty batch never runs the command");
        // A failing batch is find's own failure, as GNU documents for `-exec … +`.
        let (out, code) = run(&mut c, "find /home/user/t -name a.txt -exec false {} +");
        assert_eq!(code, 1, "{out}");
    }

    #[test]
    fn find_exec_quotes_a_name_containing_spaces() {
        let mut c = machine();
        c.vfs
            .write("/home/user/t/two words.txt", b"x", "user", 0)
            .expect("spaced name");
        let (out, _) = run(
            &mut c,
            "find /home/user/t -name 'two words.txt' -exec echo [{}] ';'",
        );
        assert_eq!(out, "[/home/user/t/two words.txt]\n");
    }

    #[test]
    fn find_execdir_runs_from_the_files_own_directory() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user/t -name 'b.log' -execdir pwd ';'");
        assert_eq!(code, 0, "{out}");
        assert_eq!(out, "/home/user/t/sub\n");
        let (out, _) = run(
            &mut c,
            "find /home/user/t -name 'b.log' -execdir echo {} ';'",
        );
        assert_eq!(out, "./b.log\n");
        let (out, _) = run(&mut c, "find /home/user/t -name '*.log' -execdir echo {} +");
        assert_eq!(out, "./b.log\n");
        assert_eq!(c.cwd, "/home/user", "the working directory is restored");
    }

    #[test]
    fn find_quit_stops_the_walk_at_the_first_match() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user/t -type f -print -quit");
        assert_eq!(code, 0);
        assert_eq!(out, "/home/user/t/a.txt\n");
        let (out, _) = run(&mut c, "find /home/user/t -name 'zzz' -print -quit");
        assert_eq!(out, "", "nothing matches, so the walk runs to the end");
    }

    #[test]
    fn find_depth_visits_children_before_their_parent() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t/sub -depth");
        assert_eq!(
            lines(&out),
            [
                "/home/user/t/sub/C.TXT",
                "/home/user/t/sub/b.log",
                "/home/user/t/sub"
            ]
        );
        let (out, _) = run(&mut c, "find /home/user/t/sub");
        assert_eq!(
            lines(&out),
            [
                "/home/user/t/sub",
                "/home/user/t/sub/C.TXT",
                "/home/user/t/sub/b.log"
            ]
        );
    }

    #[test]
    fn find_takes_several_roots_and_defaults_to_the_working_directory() {
        let mut c = machine();
        let (out, _) = run(&mut c, "find /home/user/t/empty /home/user/t/sub -type d");
        assert_eq!(lines(&out), ["/home/user/t/empty", "/home/user/t/sub"]);
        let r = shell::execute(
            &mut c,
            "cd /home/user/t && find . -maxdepth 1 -type d",
            1,
            &mut OfflineHost,
        );
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(lines(&r.stdout), [".", "./empty", "./sub"]);
        let r = shell::execute(
            &mut c,
            "cd /home/user/t && find -name a.txt",
            1,
            &mut OfflineHost,
        );
        assert_eq!(r.stdout, "./a.txt\n", "no root operand means `.`");
    }

    #[test]
    fn find_names_a_missing_root_and_keeps_going() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /no/such/root -name x");
        assert_eq!(code, 1, "{out}");
        assert_eq!(out, "find: '/no/such/root': No such file or directory\n");
        let (out, code) = run(&mut c, "find /no/such/root /home/user/t/empty");
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("No such file or directory"), "{out}");
    }

    #[test]
    fn find_reports_an_unreadable_directory_without_aborting() {
        let mut c = machine();
        let r = shell::execute(&mut c, "chmod 000 /home/user/t/sub", 0, &mut OfflineHost);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        let (out, code) = run(&mut c, "find /home/user/t");
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("Permission denied"), "{out}");
    }

    #[test]
    fn find_refuses_an_unknown_predicate_by_name_with_status_two() {
        let mut c = machine();
        let (out, code) = run(&mut c, "find /home/user/t -frobnicate");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("unknown predicate"), "{out}");
        assert!(out.contains("-frobnicate"), "{out}");
        let (out, code) = run(&mut c, "find /home/user/t -name");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("missing argument"), "{out}");
        let (out, code) = run(&mut c, "find -L /home/user/t");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("-L"), "{out}");
        let (out, code) = run(&mut c, "find /home/user/t -follow");
        assert_eq!(code, 2, "{out}");
        assert!(out.contains("-follow"), "{out}");
    }
}
