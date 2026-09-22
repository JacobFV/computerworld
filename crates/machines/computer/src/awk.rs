//! A POSIX awk: lexer, parser and interpreter.
//!
//! The language is the real one — patterns and ranges, `BEGIN`/`END`, fields and
//! field assignment, the built-in variables, user functions with local parameters,
//! arrays (including multidimensional ones and `delete`), the string and maths
//! library, `printf`, output redirection and every `getline` form. What it is *not*
//! is concurrent: a `print | "cmd"` pipe buffers its text and runs the command when
//! the pipe is closed (explicitly, or when the program ends), because this world has
//! no second process to schedule. That is the one documented departure, and it makes
//! the classic `print | "sort"` idiom behave exactly as people expect while staying
//! deterministic.
use crate::shell::{self, Fail};
use crate::{Computer, ShellHost};
use std::collections::BTreeMap;
use std::rc::Rc;

// ---------------------------------------------------------------- values

/// An awk scalar. `StrNum` is a string that came from input and therefore compares
/// numerically when both sides look numeric — the rule that makes `$1 == 10` work on
/// a file containing `10.0`.
#[derive(Clone, Debug)]
pub(crate) enum Value {
    Uninit,
    Num(f64),
    Str(String),
    StrNum(String, f64),
}
impl Value {
    fn num(&self) -> f64 {
        match self {
            Value::Uninit => 0.0,
            Value::Num(n) | Value::StrNum(_, n) => *n,
            Value::Str(s) => str_to_num(s),
        }
    }
    fn truthy(&self) -> bool {
        match self {
            Value::Uninit => false,
            Value::Num(n) | Value::StrNum(_, n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
        }
    }
    /// True when the value takes part in a numeric comparison.
    fn numeric(&self) -> bool {
        matches!(self, Value::Uninit | Value::Num(_) | Value::StrNum(..))
    }
    fn text(&self, convfmt: &str) -> String {
        match self {
            Value::Uninit => String::new(),
            Value::Str(s) | Value::StrNum(s, _) => s.clone(),
            Value::Num(n) => num_to_str(*n, convfmt),
        }
    }
}
/// A string read from input becomes a `StrNum` only if it is *entirely* a number,
/// blanks aside. `"10x"` stays a string, so `$1 == 10` is false for it.
fn input_value(s: &str) -> Value {
    match looks_numeric(s) {
        Some(n) => Value::StrNum(s.to_string(), n),
        None => Value::Str(s.to_string()),
    }
}
fn looks_numeric(s: &str) -> Option<f64> {
    let t = s.trim_matches([' ', '\t', '\n']);
    if t.is_empty() {
        return None;
    }
    // `nan`/`inf` are not input numbers in awk; only decimal constants are.
    if t.chars()
        .any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E')
    {
        return None;
    }
    t.parse::<f64>().ok()
}
/// The numeric prefix of a string, as awk's string-to-number conversion defines it.
fn str_to_num(s: &str) -> f64 {
    let t = s.trim_start_matches([' ', '\t', '\n']);
    let b: Vec<char> = t.chars().collect();
    let mut i = 0;
    if i < b.len() && (b[i] == '+' || b[i] == '-') {
        i += 1;
    }
    let start_digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i < b.len() && b[i] == '.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i == start_digits || (i == start_digits + 1 && b[start_digits] == '.') {
        return 0.0;
    }
    let mantissa = i;
    if i < b.len() && (b[i] == 'e' || b[i] == 'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == '+' || b[j] == '-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        } else {
            i = mantissa;
        }
    }
    b[..i].iter().collect::<String>().parse().unwrap_or(0.0)
}
/// Integral values print without a fraction; everything else goes through CONVFMT.
fn num_to_str(n: f64, convfmt: &str) -> String {
    if n.is_nan() {
        return String::from("nan");
    }
    if n.is_infinite() {
        return String::from(if n < 0.0 { "-inf" } else { "inf" });
    }
    if n == n.trunc() && n.abs() < 1e16 {
        return format!("{}", n as i64);
    }
    format_spec(convfmt, &[Value::Num(n)]).unwrap_or_else(|_| format!("{n}"))
}

// ---------------------------------------------------------------- lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ere(String),
    /// A name immediately followed by `(`: a call, never a concatenation.
    Call(String),
    Name(String),
    Builtin(String),
    Word(String),
    Op(String),
    Newline,
}
const KEYWORDS: &[&str] = &[
    "BEGIN", "END", "function", "func", "if", "else", "while", "for", "do", "break", "continue",
    "next", "nextfile", "exit", "return", "delete", "in", "getline", "print", "printf",
];
const BUILTINS: &[&str] = &[
    "length", "substr", "index", "split", "sub", "gsub", "match", "sprintf", "sin", "cos", "atan2",
    "exp", "log", "sqrt", "int", "rand", "srand", "tolower", "toupper", "system", "close",
    "fflush",
];
fn lex(src: &str) -> Result<Vec<Tok>, Fail> {
    let b: Vec<char> = src.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let ch = b[i];
        if ch == '\\' && b.get(i + 1) == Some(&'\n') {
            i += 2;
            continue;
        }
        if ch == ' ' || ch == '\t' || ch == '\r' {
            i += 1;
            continue;
        }
        if ch == '#' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '\n' {
            i += 1;
            // A newline after an operator, `{`, `,` or a keyword that must be followed
            // by more program is not a terminator: swallow it here so the parser never
            // has to guess.
            let joins = match out.last() {
                None | Some(Tok::Newline) => true,
                Some(Tok::Op(op)) => !matches!(op.as_str(), ")" | "]" | "}" | "++" | "--" | "$"),
                Some(Tok::Word(w)) => {
                    matches!(w.as_str(), "do" | "else" | "," | "&&" | "||")
                }
                _ => false,
            };
            if !joins {
                out.push(Tok::Newline);
            }
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && b.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            if i < b.len() && b[i] == '.' {
                i += 1;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < b.len() && (b[i] == 'e' || b[i] == 'E') {
                let mut j = i + 1;
                if j < b.len() && (b[j] == '+' || b[j] == '-') {
                    j += 1;
                }
                if j < b.len() && b[j].is_ascii_digit() {
                    while j < b.len() && b[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            let text: String = b[start..i].iter().collect();
            out.push(Tok::Num(
                text.parse()
                    .map_err(|_| Fail::usage(format!("awk: bad number `{text}`")))?,
            ));
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let name: String = b[start..i].iter().collect();
            if KEYWORDS.contains(&name.as_str()) {
                out.push(Tok::Word(if name == "func" {
                    "function".into()
                } else {
                    name
                }));
            } else if BUILTINS.contains(&name.as_str()) {
                out.push(Tok::Builtin(name));
            } else if b.get(i) == Some(&'(') {
                out.push(Tok::Call(name));
            } else {
                out.push(Tok::Name(name));
            }
            continue;
        }
        if ch == '"' {
            i += 1;
            let mut s = String::new();
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' && i + 1 < b.len() {
                    i += 1;
                    let (text, used) = string_escape(&b, i);
                    s.push_str(&text);
                    i += used;
                } else if b[i] == '\n' {
                    return Err(Fail::usage("awk: newline in string"));
                } else {
                    s.push(b[i]);
                    i += 1;
                }
            }
            if i >= b.len() {
                return Err(Fail::usage("awk: unterminated string"));
            }
            i += 1;
            out.push(Tok::Str(s));
            continue;
        }
        if ch == '/' && regex_position(out.last()) {
            i += 1;
            let mut s = String::new();
            let mut in_bracket = false;
            while i < b.len() && (in_bracket || b[i] != '/') {
                if b[i] == '\\' && i + 1 < b.len() {
                    if b[i + 1] == '/' {
                        s.push('/');
                    } else {
                        s.push('\\');
                        s.push(b[i + 1]);
                    }
                    i += 2;
                    continue;
                }
                if b[i] == '\n' {
                    return Err(Fail::usage("awk: newline in regular expression"));
                }
                if b[i] == '[' {
                    in_bracket = true;
                } else if b[i] == ']' {
                    in_bracket = false;
                }
                s.push(b[i]);
                i += 1;
            }
            if i >= b.len() {
                return Err(Fail::usage("awk: unterminated regular expression"));
            }
            i += 1;
            out.push(Tok::Ere(s));
            continue;
        }
        // Operators, longest first.
        const THREE: &[&str] = &["**="];
        const TWO: &[&str] = &[
            "+=", "-=", "*=", "/=", "%=", "^=", "==", "!=", "<=", ">=", "&&", "||", "++", "--",
            ">>", "!~", "**",
        ];
        let rest: String = b[i..(i + 3).min(b.len())].iter().collect();
        if let Some(op) = THREE.iter().find(|o| rest.starts_with(**o)) {
            out.push(Tok::Op(String::from("^=")));
            let _ = op;
            i += 3;
            continue;
        }
        if let Some(op) = TWO.iter().find(|o| rest.starts_with(**o)) {
            out.push(Tok::Op(if *op == "**" {
                String::from("^")
            } else {
                (*op).to_string()
            }));
            i += 2;
            continue;
        }
        if "{}()[];,+-*/%^=<>!?:~$|&".contains(ch) {
            out.push(Tok::Op(ch.to_string()));
            i += 1;
            continue;
        }
        return Err(Fail::usage(format!(
            "awk: unexpected character `{ch}` in program"
        )));
    }
    Ok(out)
}
/// After a value a `/` divides; anywhere else it opens a regular expression.
fn regex_position(previous: Option<&Tok>) -> bool {
    match previous {
        Some(Tok::Num(_) | Tok::Str(_) | Tok::Name(_) | Tok::Ere(_)) => false,
        Some(Tok::Op(op)) => !matches!(op.as_str(), ")" | "]" | "++" | "--"),
        _ => true,
    }
}
/// One backslash escape inside a string literal; returns the text and how many source
/// characters it consumed (the backslash itself is already skipped).
fn string_escape(b: &[char], i: usize) -> (String, usize) {
    match b.get(i) {
        Some('n') => ("\n".into(), 1),
        Some('t') => ("\t".into(), 1),
        Some('r') => ("\r".into(), 1),
        Some('\\') => ("\\".into(), 1),
        Some('"') => ("\"".into(), 1),
        Some('/') => ("/".into(), 1),
        Some('a') => ("\u{7}".into(), 1),
        Some('b') => ("\u{8}".into(), 1),
        Some('f') => ("\u{c}".into(), 1),
        Some('v') => ("\u{b}".into(), 1),
        Some(c) if c.is_digit(8) => {
            let mut used = 0;
            let mut code = 0u32;
            while used < 3 && b.get(i + used).is_some_and(|c| c.is_digit(8)) {
                code = code * 8 + b[i + used].to_digit(8).unwrap();
                used += 1;
            }
            (char::from_u32(code).unwrap_or('\0').to_string(), used)
        }
        // An unknown escape keeps the backslash, which is what awk does and what makes
        // `"\\." ` and `"\."` both usable as dynamic regexes.
        Some(c) => (format!("\\{c}"), 1),
        None => ("\\".into(), 0),
    }
}

// ---------------------------------------------------------------- AST

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Str(String),
    /// A bare `/re/` in a value position means `$0 ~ /re/`.
    Regex(String),
    Var(String),
    Field(Box<Expr>),
    Index(String, Vec<Expr>),
    Group(Vec<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(String, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    In(Vec<Expr>, String),
    Match(bool, Box<Expr>, Box<Expr>),
    Rel(String, Box<Expr>, Box<Expr>),
    Concat(Box<Expr>, Box<Expr>),
    Arith(char, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Pos(Box<Expr>),
    Not(Box<Expr>),
    IncDec {
        pre: bool,
        inc: bool,
        target: Box<Expr>,
    },
    Call(String, Vec<Expr>),
    Builtin(String, Vec<Expr>),
    Getline {
        source: GetSource,
        var: Option<Box<Expr>>,
    },
}
#[derive(Clone, Debug)]
enum GetSource {
    Main,
    File(Box<Expr>),
    Cmd(Box<Expr>),
}
#[derive(Clone, Debug)]
enum Redirect {
    Truncate(Expr),
    Append(Expr),
    Pipe(Expr),
}
#[derive(Clone, Debug)]
enum Stmt {
    Expr(Expr),
    Print(Vec<Expr>, Option<Redirect>),
    Printf(Vec<Expr>, Option<Redirect>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(
        Option<Box<Stmt>>,
        Option<Expr>,
        Option<Box<Stmt>>,
        Box<Stmt>,
    ),
    ForIn(String, String, Box<Stmt>),
    Delete(String, Vec<Expr>),
    Next,
    NextFile,
    Exit(Option<Expr>),
    Return(Option<Expr>),
    Break,
    Continue,
    Nop,
}
#[derive(Clone, Debug)]
enum Pattern {
    Begin,
    End,
    Always,
    Expr(Expr),
    Range(Expr, Expr),
}
#[derive(Clone, Debug)]
struct Rule {
    pattern: Pattern,
    action: Option<Vec<Stmt>>,
}
#[derive(Clone, Debug)]
struct Func {
    params: Vec<String>,
    body: Vec<Stmt>,
    /// Parameters the body uses as arrays. awk decides this from use, and it decides
    /// whether an unset caller variable is shared by reference or copied by value.
    array_params: Vec<bool>,
}
#[derive(Default)]
struct Program {
    rules: Vec<Rule>,
    funcs: BTreeMap<String, Func>,
}

// ---------------------------------------------------------------- parser

struct Parser {
    t: Vec<Tok>,
    i: usize,
}
impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn at_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if o == op)
    }
    fn at_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(o)) if o == w)
    }
    fn eat_op(&mut self, op: &str) -> bool {
        let hit = self.at_op(op);
        if hit {
            self.i += 1;
        }
        hit
    }
    fn eat_word(&mut self, w: &str) -> bool {
        let hit = self.at_word(w);
        if hit {
            self.i += 1;
        }
        hit
    }
    fn expect_op(&mut self, op: &str) -> Result<(), Fail> {
        if self.eat_op(op) {
            Ok(())
        } else {
            Err(Fail::usage(format!(
                "awk: syntax error: expected `{op}`, found {}",
                self.describe()
            )))
        }
    }
    fn describe(&self) -> String {
        match self.peek() {
            None => "end of program".into(),
            Some(Tok::Newline) => "end of line".into(),
            Some(Tok::Num(n)) => format!("`{n}`"),
            Some(Tok::Str(s)) => format!("string `{s}`"),
            Some(Tok::Ere(s)) => format!("/{s}/"),
            Some(Tok::Name(s) | Tok::Call(s) | Tok::Builtin(s) | Tok::Word(s)) => format!("`{s}`"),
            Some(Tok::Op(s)) => format!("`{s}`"),
        }
    }
    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) || self.at_op(";") {
            self.i += 1;
        }
    }
    fn skip_terminators(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.i += 1;
        }
    }

    fn program(&mut self) -> Result<Program, Fail> {
        let mut p = Program::default();
        self.skip_newlines();
        while self.peek().is_some() {
            if self.eat_word("function") {
                let name = match self.peek().cloned() {
                    Some(Tok::Call(n) | Tok::Name(n)) => {
                        self.i += 1;
                        n
                    }
                    _ => return Err(Fail::usage("awk: `function` needs a name")),
                };
                self.expect_op("(")?;
                let mut params = Vec::new();
                while !self.at_op(")") {
                    match self.peek().cloned() {
                        Some(Tok::Name(n)) => {
                            self.i += 1;
                            params.push(n);
                        }
                        _ => return Err(Fail::usage("awk: bad parameter list")),
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                    self.skip_terminators();
                }
                self.expect_op(")")?;
                self.skip_terminators();
                let body = self.block()?;
                let array_params = vec![false; params.len()];
                p.funcs.insert(
                    name,
                    Func {
                        params,
                        body,
                        array_params,
                    },
                );
            } else if self.eat_word("BEGIN") {
                self.skip_terminators();
                p.rules.push(Rule {
                    pattern: Pattern::Begin,
                    action: Some(self.block()?),
                });
            } else if self.eat_word("END") {
                self.skip_terminators();
                p.rules.push(Rule {
                    pattern: Pattern::End,
                    action: Some(self.block()?),
                });
            } else if self.at_op("{") {
                p.rules.push(Rule {
                    pattern: Pattern::Always,
                    action: Some(self.block()?),
                });
            } else {
                // A pattern is a full expression: `>` is a comparison here, unlike
                // inside a `print` list where it redirects.
                let first = self.expr(false)?;
                let pattern = if self.eat_op(",") {
                    self.skip_terminators();
                    Pattern::Range(first, self.expr(false)?)
                } else {
                    Pattern::Expr(first)
                };
                let action = if self.at_op("{") {
                    Some(self.block()?)
                } else {
                    None
                };
                p.rules.push(Rule { pattern, action });
            }
            self.skip_newlines();
        }
        mark_array_params(&mut p);
        Ok(p)
    }
    fn block(&mut self) -> Result<Vec<Stmt>, Fail> {
        self.expect_op("{")?;
        let mut body = Vec::new();
        self.skip_newlines();
        while !self.at_op("}") {
            if self.peek().is_none() {
                return Err(Fail::usage("awk: unexpected end of program, missing `}`"));
            }
            body.push(self.statement()?);
            self.skip_newlines();
        }
        self.expect_op("}")?;
        Ok(body)
    }
    fn statement(&mut self) -> Result<Stmt, Fail> {
        if self.at_op("{") {
            return Ok(Stmt::Block(self.block()?));
        }
        if self.eat_op(";") {
            return Ok(Stmt::Nop);
        }
        if self.eat_word("if") {
            self.expect_op("(")?;
            let cond = self.expr(false)?;
            self.expect_op(")")?;
            self.skip_newlines();
            let then = Box::new(self.statement()?);
            let save = self.i;
            self.skip_newlines();
            let otherwise = if self.eat_word("else") {
                self.skip_newlines();
                Some(Box::new(self.statement()?))
            } else {
                self.i = save;
                None
            };
            return Ok(Stmt::If(cond, then, otherwise));
        }
        if self.eat_word("while") {
            self.expect_op("(")?;
            let cond = self.expr(false)?;
            self.expect_op(")")?;
            self.skip_newlines();
            if self.at_op(";") {
                self.i += 1;
                return Ok(Stmt::While(cond, Box::new(Stmt::Nop)));
            }
            return Ok(Stmt::While(cond, Box::new(self.statement()?)));
        }
        if self.eat_word("do") {
            self.skip_newlines();
            let body = Box::new(self.statement()?);
            self.skip_newlines();
            if !self.eat_word("while") {
                return Err(Fail::usage("awk: `do` body must be followed by `while`"));
            }
            self.expect_op("(")?;
            let cond = self.expr(false)?;
            self.expect_op(")")?;
            return Ok(Stmt::DoWhile(body, cond));
        }
        if self.eat_word("for") {
            self.expect_op("(")?;
            // `for (k in arr)` — distinguished from a three-part header by lookahead.
            if let (Some(Tok::Name(k)), Some(Tok::Word(w))) =
                (self.t.get(self.i).cloned(), self.t.get(self.i + 1).cloned())
            {
                if w == "in" {
                    if let Some(Tok::Name(arr)) = self.t.get(self.i + 2).cloned() {
                        if matches!(self.t.get(self.i + 3), Some(Tok::Op(o)) if o == ")") {
                            self.i += 4;
                            self.skip_newlines();
                            return Ok(Stmt::ForIn(k, arr, Box::new(self.statement()?)));
                        }
                    }
                }
            }
            let init = if self.at_op(";") {
                None
            } else {
                Some(Box::new(self.simple_statement()?))
            };
            self.expect_op(";")?;
            self.skip_terminators();
            let cond = if self.at_op(";") {
                None
            } else {
                Some(self.expr(false)?)
            };
            self.expect_op(";")?;
            self.skip_terminators();
            let step = if self.at_op(")") {
                None
            } else {
                Some(Box::new(self.simple_statement()?))
            };
            self.expect_op(")")?;
            self.skip_newlines();
            return Ok(Stmt::For(init, cond, step, Box::new(self.statement()?)));
        }
        if self.eat_word("break") {
            return Ok(Stmt::Break);
        }
        if self.eat_word("continue") {
            return Ok(Stmt::Continue);
        }
        if self.eat_word("next") {
            return Ok(Stmt::Next);
        }
        if self.eat_word("nextfile") {
            return Ok(Stmt::NextFile);
        }
        if self.eat_word("exit") {
            let v = if self.ends_statement() {
                None
            } else {
                Some(self.expr(true)?)
            };
            return Ok(Stmt::Exit(v));
        }
        if self.eat_word("return") {
            let v = if self.ends_statement() {
                None
            } else {
                Some(self.expr(true)?)
            };
            return Ok(Stmt::Return(v));
        }
        self.simple_statement()
    }
    fn ends_statement(&self) -> bool {
        matches!(self.peek(), None | Some(Tok::Newline))
            || self.at_op(";")
            || self.at_op("}")
            || self.at_op(")")
    }
    /// A statement that can appear in a `for` header: `print`, `delete` or an expression.
    fn simple_statement(&mut self) -> Result<Stmt, Fail> {
        if self.eat_word("delete") {
            let name = match self.peek().cloned() {
                Some(Tok::Name(n) | Tok::Call(n)) => {
                    self.i += 1;
                    n
                }
                _ => return Err(Fail::usage("awk: `delete` needs an array name")),
            };
            let mut subs = Vec::new();
            if self.eat_op("[") {
                loop {
                    subs.push(self.expr(false)?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op("]")?;
            } else if self.eat_op("(") {
                // `delete (arr[i])` is accepted by every awk in the wild.
                loop {
                    subs.push(self.expr(false)?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
            }
            return Ok(Stmt::Delete(name, subs));
        }
        let printing = if self.eat_word("print") {
            Some(false)
        } else if self.eat_word("printf") {
            Some(true)
        } else {
            None
        };
        if let Some(is_printf) = printing {
            let mut args = Vec::new();
            if !self.ends_statement() && !self.at_redirect() {
                loop {
                    args.push(self.expr(true)?);
                    if !self.eat_op(",") {
                        break;
                    }
                    self.skip_terminators();
                }
            }
            // `print (a, b) > "f"` — a single parenthesised group is the argument list.
            if args.len() == 1 {
                if let Expr::Group(items) = &args[0] {
                    if items.len() > 1 {
                        args = items.clone();
                    }
                }
            }
            let redirect = if self.eat_op(">") {
                Some(Redirect::Truncate(self.expr(true)?))
            } else if self.eat_op(">>") {
                Some(Redirect::Append(self.expr(true)?))
            } else if self.eat_op("|") {
                Some(Redirect::Pipe(self.expr(true)?))
            } else {
                None
            };
            if is_printf && args.is_empty() {
                return Err(Fail::usage("awk: printf needs a format string"));
            }
            return Ok(if is_printf {
                Stmt::Printf(args, redirect)
            } else {
                Stmt::Print(args, redirect)
            });
        }
        Ok(Stmt::Expr(self.expr(false)?))
    }
    fn at_redirect(&self) -> bool {
        self.at_op(">") || self.at_op(">>") || self.at_op("|")
    }

    /// `no_gt` is set inside a `print` list, where `>` starts a redirection instead of
    /// a comparison — the one context-sensitive corner of the grammar.
    fn expr(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let lhs = self.ternary(no_gt)?;
        const OPS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "^="];
        if let Some(Tok::Op(op)) = self.peek() {
            if OPS.contains(&op.as_str()) && is_lvalue(&lhs) {
                let op = op.clone();
                self.i += 1;
                self.skip_terminators();
                let rhs = self.expr(no_gt)?;
                return Ok(if op == "=" {
                    Expr::Assign(Box::new(lhs), Box::new(rhs))
                } else {
                    Expr::OpAssign(op, Box::new(lhs), Box::new(rhs))
                });
            }
        }
        Ok(lhs)
    }
    fn ternary(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let cond = self.or(no_gt)?;
        if self.eat_op("?") {
            self.skip_terminators();
            let a = self.expr(no_gt)?;
            self.skip_terminators();
            self.expect_op(":")?;
            self.skip_terminators();
            let b = self.expr(no_gt)?;
            return Ok(Expr::Cond(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }
    fn or(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.and(no_gt)?;
        while self.eat_op("||") {
            self.skip_terminators();
            lhs = Expr::Or(Box::new(lhs), Box::new(self.and(no_gt)?));
        }
        Ok(lhs)
    }
    fn and(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.in_expr(no_gt)?;
        while self.eat_op("&&") {
            self.skip_terminators();
            lhs = Expr::And(Box::new(lhs), Box::new(self.in_expr(no_gt)?));
        }
        Ok(lhs)
    }
    fn in_expr(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.match_expr(no_gt)?;
        while self.at_word("in") {
            self.i += 1;
            let name = match self.peek().cloned() {
                Some(Tok::Name(n)) => {
                    self.i += 1;
                    n
                }
                _ => return Err(Fail::usage("awk: `in` needs an array name")),
            };
            let subs = match lhs {
                Expr::Group(items) => items,
                other => vec![other],
            };
            lhs = Expr::In(subs, name);
        }
        Ok(lhs)
    }
    fn match_expr(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.relational(no_gt)?;
        loop {
            let negated = if self.at_op("~") {
                false
            } else if self.at_op("!~") {
                true
            } else {
                return Ok(lhs);
            };
            self.i += 1;
            let rhs = self.relational(no_gt)?;
            lhs = Expr::Match(negated, Box::new(lhs), Box::new(rhs));
        }
    }
    fn relational(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let lhs = self.pipe_getline(no_gt)?;
        let op = match self.peek() {
            Some(Tok::Op(o))
                if matches!(o.as_str(), "<" | "<=" | "==" | "!=" | ">=")
                    || (o == ">" && !no_gt) =>
            {
                o.clone()
            }
            _ => return Ok(lhs),
        };
        self.i += 1;
        let rhs = self.pipe_getline(no_gt)?;
        Ok(Expr::Rel(op, Box::new(lhs), Box::new(rhs)))
    }
    /// `"cmd" | getline [var]` binds tighter than comparison, so `… | getline x > 0`
    /// tests the result rather than redirecting.
    fn pipe_getline(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.concat(no_gt)?;
        while self.at_op("|")
            && matches!(self.t.get(self.i + 1), Some(Tok::Word(w)) if w == "getline")
        {
            self.i += 2;
            let var = self.optional_lvalue()?;
            lhs = Expr::Getline {
                source: GetSource::Cmd(Box::new(lhs)),
                var: var.map(Box::new),
            };
        }
        Ok(lhs)
    }
    fn starts_operand(&self) -> bool {
        match self.peek() {
            Some(Tok::Num(_) | Tok::Str(_) | Tok::Ere(_) | Tok::Name(_) | Tok::Call(_)) => true,
            Some(Tok::Builtin(_)) => true,
            Some(Tok::Word(w)) => w == "getline",
            Some(Tok::Op(o)) => matches!(o.as_str(), "$" | "(" | "!" | "++" | "--" | "-" | "+"),
            _ => false,
        }
    }
    fn concat(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.additive(no_gt)?;
        // `-` and `+` after a complete operand are always arithmetic, never the start
        // of a concatenated negative number; `additive` has already taken them.
        while self.starts_operand() && !self.at_op("-") && !self.at_op("+") {
            lhs = Expr::Concat(Box::new(lhs), Box::new(self.additive(no_gt)?));
        }
        Ok(lhs)
    }
    fn additive(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.multiplicative(no_gt)?;
        loop {
            let op = if self.at_op("+") {
                '+'
            } else if self.at_op("-") {
                '-'
            } else {
                return Ok(lhs);
            };
            self.i += 1;
            lhs = Expr::Arith(op, Box::new(lhs), Box::new(self.multiplicative(no_gt)?));
        }
    }
    fn multiplicative(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut lhs = self.unary(no_gt)?;
        loop {
            let op = if self.at_op("*") {
                '*'
            } else if self.at_op("/") {
                '/'
            } else if self.at_op("%") {
                '%'
            } else {
                return Ok(lhs);
            };
            self.i += 1;
            lhs = Expr::Arith(op, Box::new(lhs), Box::new(self.unary(no_gt)?));
        }
    }
    fn unary(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        if self.eat_op("!") {
            return Ok(Expr::Not(Box::new(self.unary(no_gt)?)));
        }
        if self.eat_op("-") {
            return Ok(Expr::Neg(Box::new(self.unary(no_gt)?)));
        }
        if self.eat_op("+") {
            return Ok(Expr::Pos(Box::new(self.unary(no_gt)?)));
        }
        self.power(no_gt)
    }
    fn power(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let base = self.postfix(no_gt)?;
        if self.eat_op("^") {
            // Right associative, and `-` binds looser: `2^-1` and `2^3^2` both work.
            return Ok(Expr::Arith(
                '^',
                Box::new(base),
                Box::new(self.unary(no_gt)?),
            ));
        }
        Ok(base)
    }
    fn postfix(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        let mut e = self.primary(no_gt)?;
        loop {
            if is_lvalue(&e) && (self.at_op("++") || self.at_op("--")) {
                let inc = self.at_op("++");
                self.i += 1;
                e = Expr::IncDec {
                    pre: false,
                    inc,
                    target: Box::new(e),
                };
            } else {
                return Ok(e);
            }
        }
    }
    fn optional_lvalue(&mut self) -> Result<Option<Expr>, Fail> {
        match self.peek().cloned() {
            Some(Tok::Name(n)) => {
                self.i += 1;
                if self.eat_op("[") {
                    let mut subs = Vec::new();
                    loop {
                        subs.push(self.expr(false)?);
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_op("]")?;
                    Ok(Some(Expr::Index(n, subs)))
                } else {
                    Ok(Some(Expr::Var(n)))
                }
            }
            Some(Tok::Op(o)) if o == "$" => {
                self.i += 1;
                Ok(Some(Expr::Field(Box::new(self.primary(false)?))))
            }
            _ => Ok(None),
        }
    }
    fn primary(&mut self, no_gt: bool) -> Result<Expr, Fail> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(Expr::Num(n))
            }
            Some(Tok::Str(s)) => {
                self.i += 1;
                Ok(Expr::Str(s))
            }
            Some(Tok::Ere(s)) => {
                self.i += 1;
                Ok(Expr::Regex(s))
            }
            Some(Tok::Op(o)) if o == "$" => {
                self.i += 1;
                Ok(Expr::Field(Box::new(self.postfix(no_gt)?)))
            }
            Some(Tok::Op(o)) if o == "++" || o == "--" => {
                self.i += 1;
                let inc = o == "++";
                let target = self.primary(no_gt)?;
                if !is_lvalue(&target) {
                    return Err(Fail::usage(format!("awk: `{o}` needs a variable")));
                }
                Ok(Expr::IncDec {
                    pre: true,
                    inc,
                    target: Box::new(target),
                })
            }
            Some(Tok::Op(o)) if o == "(" => {
                self.i += 1;
                self.skip_terminators();
                let mut items = vec![self.expr(false)?];
                while self.eat_op(",") {
                    self.skip_terminators();
                    items.push(self.expr(false)?);
                }
                self.expect_op(")")?;
                Ok(Expr::Group(items))
            }
            Some(Tok::Word(w)) if w == "getline" => {
                self.i += 1;
                let var = self.optional_lvalue()?;
                if self.eat_op("<") {
                    let file = self.concat(no_gt)?;
                    return Ok(Expr::Getline {
                        source: GetSource::File(Box::new(file)),
                        var: var.map(Box::new),
                    });
                }
                Ok(Expr::Getline {
                    source: GetSource::Main,
                    var: var.map(Box::new),
                })
            }
            Some(Tok::Builtin(name)) => {
                self.i += 1;
                let mut args = Vec::new();
                if self.eat_op("(") {
                    self.skip_terminators();
                    if !self.at_op(")") {
                        loop {
                            args.push(self.expr(false)?);
                            if !self.eat_op(",") {
                                break;
                            }
                            self.skip_terminators();
                        }
                    }
                    self.expect_op(")")?;
                } else if name != "length" {
                    return Err(Fail::usage(format!("awk: `{name}` needs an argument list")));
                }
                Ok(Expr::Builtin(name, args))
            }
            Some(Tok::Call(name)) => {
                self.i += 1;
                self.expect_op("(")?;
                self.skip_terminators();
                let mut args = Vec::new();
                if !self.at_op(")") {
                    loop {
                        args.push(self.expr(false)?);
                        if !self.eat_op(",") {
                            break;
                        }
                        self.skip_terminators();
                    }
                }
                self.expect_op(")")?;
                Ok(Expr::Call(name, args))
            }
            Some(Tok::Name(n)) => {
                self.i += 1;
                if self.eat_op("[") {
                    let mut subs = Vec::new();
                    loop {
                        subs.push(self.expr(false)?);
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_op("]")?;
                    Ok(Expr::Index(n, subs))
                } else {
                    Ok(Expr::Var(n))
                }
            }
            _ => Err(Fail::usage(format!(
                "awk: syntax error at {}",
                self.describe()
            ))),
        }
    }
}
fn is_lvalue(e: &Expr) -> bool {
    matches!(e, Expr::Var(_) | Expr::Field(_) | Expr::Index(..))
}
/// Decides, once, which parameters each function uses as an array: subscripted,
/// walked with `for … in`, deleted, filled by `split`, or handed on to another
/// function in an array position. The last case makes this a fixpoint.
fn mark_array_params(p: &mut Program) {
    let names: Vec<String> = p.funcs.keys().cloned().collect();
    loop {
        let mut changed = false;
        for name in &names {
            let func = p.funcs[name].clone();
            let mut marks = func.array_params.clone();
            let mut note = |n: &str| {
                if let Some(i) = func.params.iter().position(|q| q == n) {
                    if !marks[i] {
                        marks[i] = true;
                        changed = true;
                    }
                }
            };
            let mut seen: Vec<&Expr> = Vec::new();
            let mut stack: Vec<&Stmt> = func.body.iter().collect();
            while let Some(stmt) = stack.pop() {
                match stmt {
                    Stmt::Block(body) => stack.extend(body.iter()),
                    Stmt::If(e, a, b) => {
                        seen.push(e);
                        stack.push(a);
                        if let Some(b) = b {
                            stack.push(b);
                        }
                    }
                    Stmt::While(e, a) | Stmt::DoWhile(a, e) => {
                        seen.push(e);
                        stack.push(a);
                    }
                    Stmt::For(init, cond, step, body) => {
                        if let Some(i) = init {
                            stack.push(i);
                        }
                        if let Some(c) = cond {
                            seen.push(c);
                        }
                        if let Some(s) = step {
                            stack.push(s);
                        }
                        stack.push(body);
                    }
                    Stmt::ForIn(_, array, body) => {
                        note(array);
                        stack.push(body);
                    }
                    Stmt::Delete(array, subs) => {
                        note(array);
                        seen.extend(subs.iter());
                    }
                    Stmt::Expr(e) | Stmt::Exit(Some(e)) | Stmt::Return(Some(e)) => seen.push(e),
                    Stmt::Print(args, _) | Stmt::Printf(args, _) => seen.extend(args.iter()),
                    _ => {}
                }
            }
            while let Some(e) = seen.pop() {
                match e {
                    Expr::Index(array, subs) => {
                        note(array);
                        seen.extend(subs.iter());
                    }
                    Expr::In(subs, array) => {
                        note(array);
                        seen.extend(subs.iter());
                    }
                    Expr::Builtin(builtin, args) => {
                        if builtin == "split" {
                            if let Some(Expr::Var(n)) = args.get(1) {
                                note(n);
                            }
                        }
                        if builtin == "length" {
                            // `length(a)` alone does not prove `a` is an array.
                        }
                        seen.extend(args.iter());
                    }
                    Expr::Call(callee, args) => {
                        if let Some(target) = p.funcs.get(callee) {
                            for (i, a) in args.iter().enumerate() {
                                if target.array_params.get(i).copied().unwrap_or(false) {
                                    if let Expr::Var(n) = a {
                                        note(n);
                                    }
                                }
                            }
                        }
                        seen.extend(args.iter());
                    }
                    Expr::Assign(a, b)
                    | Expr::OpAssign(_, a, b)
                    | Expr::Or(a, b)
                    | Expr::And(a, b)
                    | Expr::Match(_, a, b)
                    | Expr::Rel(_, a, b)
                    | Expr::Concat(a, b)
                    | Expr::Arith(_, a, b) => {
                        seen.push(a);
                        seen.push(b);
                    }
                    Expr::Cond(a, b, d) => {
                        seen.push(a);
                        seen.push(b);
                        seen.push(d);
                    }
                    Expr::Neg(a) | Expr::Pos(a) | Expr::Not(a) | Expr::Field(a) => seen.push(a),
                    Expr::IncDec { target, .. } => seen.push(target),
                    Expr::Group(items) => seen.extend(items.iter()),
                    Expr::Getline { source, var } => {
                        match source {
                            GetSource::File(e) | GetSource::Cmd(e) => seen.push(e),
                            GetSource::Main => {}
                        }
                        if let Some(v) = var {
                            seen.push(v);
                        }
                    }
                    _ => {}
                }
            }
            if let Some(slot) = p.funcs.get_mut(name) {
                slot.array_params = marks;
            }
        }
        if !changed {
            return;
        }
    }
}

// ---------------------------------------------------------------- runtime cells

#[derive(Clone)]
enum Cell {
    Scalar(Value),
    Array(Rc<std::cell::RefCell<BTreeMap<String, Value>>>),
}
fn new_array() -> Cell {
    Cell::Array(Rc::new(std::cell::RefCell::new(BTreeMap::new())))
}
enum Flow {
    Normal,
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    Return(Value),
}
/// An open output stream. A pipe holds its text until it is closed, then runs.
struct Stream {
    text: String,
    kind: StreamKind,
}
enum StreamKind {
    File { append: bool },
    Pipe,
}
struct Reader {
    text: String,
    at: usize,
}

// ---------------------------------------------------------------- interpreter

struct Interp<'a> {
    prog: Program,
    globals: BTreeMap<String, Cell>,
    frames: Vec<BTreeMap<String, Cell>>,
    record: String,
    fields: Vec<String>,
    /// `$0` is valid; only set when fields change without a rebuild being needed yet.
    out: String,
    err: String,
    streams: BTreeMap<String, Stream>,
    readers: BTreeMap<String, Reader>,
    regexes: BTreeMap<String, Rc<regex::Regex>>,
    ranges: Vec<bool>,
    flow: Flow,
    status: i32,
    seed: u64,
    rng: u64,
    calls: usize,
    steps: usize,
    // Main input.
    operands: Vec<String>,
    next_operand: usize,
    current: Option<Reader>,
    used_stdin: bool,
    stdin: String,
    c: &'a mut Computer,
    host: &'a mut dyn ShellHost,
    t: u64,
    depth: usize,
}

const STEP_BUDGET: usize = 2_000_000;

impl<'a> Interp<'a> {
    fn charge(&mut self) -> Result<(), Fail> {
        self.steps += 1;
        if self.steps > STEP_BUDGET {
            return Err(Fail::usage(format!(
                "awk: program exceeded {STEP_BUDGET} evaluation steps"
            )));
        }
        Ok(())
    }
    // ---- variables
    fn lookup(&self, name: &str) -> Option<&Cell> {
        if let Some(frame) = self.frames.last() {
            if let Some(cell) = frame.get(name) {
                return Some(cell);
            }
        }
        self.globals.get(name)
    }
    fn local(&self, name: &str) -> bool {
        self.frames.last().is_some_and(|f| f.contains_key(name))
    }
    fn get_var(&mut self, name: &str) -> Value {
        if name == "NF" {
            return Value::Num(self.fields.len() as f64);
        }
        match self.lookup(name) {
            Some(Cell::Scalar(v)) => v.clone(),
            Some(Cell::Array(_)) => Value::Uninit,
            None => Value::Uninit,
        }
    }
    fn set_var(&mut self, name: &str, v: Value) {
        if name == "NF" {
            let want = v.num().max(0.0) as usize;
            self.fields.resize(want, String::new());
            self.rebuild_record();
            return;
        }
        let cell = Cell::Scalar(v);
        if self.local(name) {
            self.frames.last_mut().unwrap().insert(name.into(), cell);
        } else {
            self.globals.insert(name.into(), cell);
        }
        if name == "RS" || name == "FS" {
            // Nothing cached; the next split reads the new value.
        }
    }
    fn text_var(&mut self, name: &str) -> String {
        let convfmt = self.convfmt();
        self.get_var(name).text(&convfmt)
    }
    fn convfmt(&self) -> String {
        match self.globals.get("CONVFMT") {
            Some(Cell::Scalar(v)) => v.text("%.6g"),
            _ => String::from("%.6g"),
        }
    }
    fn array(
        &mut self,
        name: &str,
    ) -> Result<Rc<std::cell::RefCell<BTreeMap<String, Value>>>, Fail> {
        let in_frame = self.local(name);
        let slot = if in_frame {
            self.frames.last_mut().unwrap()
        } else {
            &mut self.globals
        };
        match slot.get(name) {
            Some(Cell::Array(a)) => Ok(a.clone()),
            Some(Cell::Scalar(Value::Uninit)) | None => {
                let cell = new_array();
                slot.insert(name.into(), cell.clone());
                match cell {
                    Cell::Array(a) => Ok(a),
                    Cell::Scalar(_) => unreachable!(),
                }
            }
            Some(Cell::Scalar(_)) => Err(Fail::new(
                format!("awk: `{name}` is a scalar, not an array"),
                2,
            )),
        }
    }
    // ---- fields
    fn set_record(&mut self, text: String) {
        self.record = text;
        let fs = self.text_var("FS");
        let paragraph = self.text_var("RS").is_empty();
        self.fields = self.split_text(&self.record.clone(), &fs, paragraph);
    }
    fn split_text(&mut self, text: &str, fs: &str, paragraph: bool) -> Vec<String> {
        if text.is_empty() {
            return Vec::new();
        }
        if fs == " " {
            return text
                .split([' ', '\t', '\n'])
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
        }
        if fs.is_empty() {
            return text.chars().map(|c| c.to_string()).collect();
        }
        let chars: Vec<char> = fs.chars().collect();
        if chars.len() == 1 && chars[0] != '\\' && !paragraph {
            let sep = chars[0];
            if sep == '\t' || !"\\^$.[]|()*+?{}".contains(sep) {
                return text.split(sep).map(str::to_string).collect();
            }
        }
        // In paragraph mode a newline always separates fields, whatever FS says.
        let pattern = if paragraph {
            format!("(?:{})|\n", regex_source(fs))
        } else {
            regex_source(fs)
        };
        match self.compile(&pattern) {
            Ok(re) => re.split(text).map(str::to_string).collect(),
            Err(_) => vec![text.to_string()],
        }
    }
    fn rebuild_record(&mut self) {
        let ofs = self.text_var("OFS");
        self.record = self.fields.join(&ofs);
    }
    fn field(&mut self, n: i64) -> Result<Value, Fail> {
        if n < 0 {
            return Err(Fail::new("awk: attempt to access field -1", 2));
        }
        if n == 0 {
            return Ok(input_value(&self.record.clone()));
        }
        Ok(match self.fields.get(n as usize - 1) {
            Some(s) => input_value(s),
            None => Value::Uninit,
        })
    }
    fn set_field(&mut self, n: i64, text: String) -> Result<(), Fail> {
        if n < 0 {
            return Err(Fail::new("awk: attempt to assign to field -1", 2));
        }
        if n == 0 {
            self.set_record(text);
            return Ok(());
        }
        let idx = n as usize - 1;
        if idx >= self.fields.len() {
            self.fields.resize(idx + 1, String::new());
        }
        self.fields[idx] = text;
        self.rebuild_record();
        Ok(())
    }
    // ---- regex
    fn compile(&mut self, pattern: &str) -> Result<Rc<regex::Regex>, Fail> {
        if let Some(re) = self.regexes.get(pattern) {
            return Ok(re.clone());
        }
        let re = regex::Regex::new(pattern)
            .map_err(|e| Fail::new(format!("awk: bad regular expression /{pattern}/: {e}"), 2))?;
        let re = Rc::new(re);
        self.regexes.insert(pattern.to_string(), re.clone());
        Ok(re)
    }
    fn regex_of(&mut self, e: &Expr) -> Result<Rc<regex::Regex>, Fail> {
        let source = match e {
            Expr::Regex(r) => regex_source(r),
            other => {
                let convfmt = self.convfmt();
                regex_source(&self.eval(other)?.text(&convfmt))
            }
        };
        self.compile(&source)
    }

    // ---- output
    fn emit(&mut self, target: &Option<Redirect>, text: &str) -> Result<(), Fail> {
        let Some(redirect) = target else {
            self.out.push_str(text);
            return Ok(());
        };
        let convfmt = self.convfmt();
        let (name, kind) = match redirect {
            Redirect::Truncate(e) => (
                self.eval(e)?.text(&convfmt),
                StreamKind::File { append: false },
            ),
            Redirect::Append(e) => (
                self.eval(e)?.text(&convfmt),
                StreamKind::File { append: true },
            ),
            Redirect::Pipe(e) => (self.eval(e)?.text(&convfmt), StreamKind::Pipe),
        };
        self.streams
            .entry(name)
            .or_insert(Stream {
                text: String::new(),
                kind,
            })
            .text
            .push_str(text);
        Ok(())
    }
    fn close_stream(&mut self, name: &str) -> Result<i32, Fail> {
        if let Some(stream) = self.streams.remove(name) {
            match stream.kind {
                StreamKind::File { append } => {
                    if name == "/dev/stdout" || name == "-" {
                        self.out.push_str(&stream.text);
                    } else if name == "/dev/stderr" {
                        self.err.push_str(&stream.text);
                    } else {
                        let path = self.c.resolve(name);
                        let r = if append {
                            self.c
                                .vfs
                                .append(&path, stream.text.as_bytes(), &self.c.user, self.t)
                        } else {
                            self.c
                                .vfs
                                .write_as(&path, stream.text.as_bytes(), &self.c.user, self.t)
                        };
                        r.map_err(|e| Fail::io("awk", name, &e))?;
                    }
                }
                StreamKind::Pipe => {
                    let r =
                        shell::run_piped(self.c, name, &stream.text, self.t, self.host, self.depth);
                    self.out.push_str(&r.stdout);
                    self.err.push_str(&r.stderr);
                    return Ok(r.exit_code);
                }
            }
            return Ok(0);
        }
        if self.readers.remove(name).is_some() {
            return Ok(0);
        }
        Ok(-1)
    }
    fn flush_all(&mut self) -> Result<(), Fail> {
        // Files first, then pipes, so a pipe reading a file it was just given sees it.
        let names: Vec<String> = self
            .streams
            .iter()
            .filter(|(_, s)| matches!(s.kind, StreamKind::File { .. }))
            .map(|(k, _)| k.clone())
            .chain(
                self.streams
                    .iter()
                    .filter(|(_, s)| matches!(s.kind, StreamKind::Pipe))
                    .map(|(k, _)| k.clone()),
            )
            .collect();
        for name in names {
            self.close_stream(&name)?;
        }
        Ok(())
    }

    // ---- input
    fn record_separator(&mut self) -> String {
        self.text_var("RS")
    }
    /// Pulls the next record out of a reader using the *current* RS, so a program may
    /// change RS mid-stream exactly as awk allows.
    fn take_record(&mut self, reader_key: Option<&str>) -> Result<Option<String>, Fail> {
        let rs = self.record_separator();
        let (text, at) = match reader_key {
            Some(k) => match self.readers.get(k) {
                Some(r) => (r.text.clone(), r.at),
                None => return Ok(None),
            },
            None => match &self.current {
                Some(r) => (r.text.clone(), r.at),
                None => return Ok(None),
            },
        };
        if at >= text.len() {
            return Ok(None);
        }
        let rest = &text[at..];
        let (record, consumed) = if rs.is_empty() {
            // Paragraph mode: skip leading blank lines, read to a blank line.
            let trimmed = rest.trim_start_matches('\n');
            let skipped = rest.len() - trimmed.len();
            if trimmed.is_empty() {
                (String::new(), rest.len())
            } else {
                match trimmed.find("\n\n") {
                    Some(k) => {
                        // The separator run is measured from the blank line, not from
                        // the start of the record.
                        let after = trimmed[k..].trim_start_matches('\n');
                        let used = trimmed[k..].len() - after.len();
                        (trimmed[..k].to_string(), skipped + k + used)
                    }
                    None => (
                        trimmed.trim_end_matches('\n').to_string(),
                        skipped + trimmed.len(),
                    ),
                }
            }
        } else if rs.chars().count() == 1 {
            let sep = rs.chars().next().unwrap();
            match rest.find(sep) {
                Some(k) => (rest[..k].to_string(), k + sep.len_utf8()),
                None => (rest.trim_end_matches(sep).to_string(), rest.len()),
            }
        } else {
            let re = self.compile(&regex_source(&rs))?;
            match re.find(rest) {
                Some(m) if !m.as_str().is_empty() => (rest[..m.start()].to_string(), m.end()),
                _ => (rest.to_string(), rest.len()),
            }
        };
        if record.is_empty() && consumed == 0 {
            return Ok(None);
        }
        let new_at = at + consumed;
        match reader_key {
            Some(k) => {
                if let Some(r) = self.readers.get_mut(k) {
                    r.at = new_at;
                }
            }
            None => {
                if let Some(r) = &mut self.current {
                    r.at = new_at;
                }
            }
        }
        Ok(Some(record))
    }
    /// Advances the main input, opening the next file operand when needed. Returns the
    /// record, or None when every operand is exhausted.
    fn next_main_record(&mut self) -> Result<Option<String>, Fail> {
        loop {
            if self.current.is_some() {
                if let Some(record) = self.take_record(None)? {
                    let nr = self.get_var("NR").num() + 1.0;
                    self.set_var("NR", Value::Num(nr));
                    let fnr = self.get_var("FNR").num() + 1.0;
                    self.set_var("FNR", Value::Num(fnr));
                    return Ok(Some(record));
                }
                self.current = None;
            }
            if self.next_operand >= self.operands.len() {
                if self.used_stdin || self.operands.iter().any(|o| !is_assignment(o)) {
                    return Ok(None);
                }
                self.used_stdin = true;
                self.current = Some(Reader {
                    text: self.stdin.clone(),
                    at: 0,
                });
                self.set_var("FILENAME", Value::Str(String::new()));
                self.set_var("FNR", Value::Num(0.0));
                continue;
            }
            let operand = self.operands[self.next_operand].clone();
            self.next_operand += 1;
            if let Some((name, raw)) = split_assignment(&operand) {
                let text = unescape_command_line(&raw);
                self.set_var(&name, input_value(&text));
                continue;
            }
            let text = if operand == "-" || operand == "/dev/stdin" {
                self.used_stdin = true;
                self.stdin.clone()
            } else {
                shell::read_text(self.c, "awk", &operand, &self.stdin)?
            };
            self.current = Some(Reader { text, at: 0 });
            self.set_var("FILENAME", Value::Str(operand));
            self.set_var("FNR", Value::Num(0.0));
        }
    }

    // ---- evaluation
    fn eval(&mut self, e: &Expr) -> Result<Value, Fail> {
        self.charge()?;
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Regex(r) => {
                let re = self.compile(&regex_source(r))?;
                let record = self.record.clone();
                Ok(Value::Num(f64::from(u8::from(re.is_match(&record)))))
            }
            Expr::Var(name) => Ok(self.get_var(name)),
            Expr::Field(n) => {
                let idx = self.eval(n)?.num();
                self.field(idx as i64)
            }
            Expr::Index(name, subs) => {
                let key = self.subscript(subs)?;
                let arr = self.array(name)?;
                let hit = arr.borrow().get(&key).cloned();
                Ok(match hit {
                    Some(v) => v,
                    None => {
                        // Referencing a subscript creates it, as POSIX requires.
                        arr.borrow_mut().insert(key, Value::Uninit);
                        Value::Uninit
                    }
                })
            }
            Expr::Group(items) => match items.split_first() {
                Some((first, [])) => self.eval(first),
                _ => Err(Fail::usage("awk: an expression list is not a value here")),
            },
            Expr::Assign(target, source) => {
                let v = self.eval(source)?;
                self.assign(target, v.clone())?;
                Ok(v)
            }
            Expr::OpAssign(op, target, source) => {
                let rhs = self.eval(source)?.num();
                let lhs = self.eval(target)?.num();
                let v = Value::Num(arith(op.as_bytes()[0] as char, lhs, rhs)?);
                self.assign(target, v.clone())?;
                Ok(v)
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.eval(a)
                } else {
                    self.eval(b)
                }
            }
            Expr::Or(a, b) => {
                let hit = self.eval(a)?.truthy() || self.eval(b)?.truthy();
                Ok(Value::Num(f64::from(u8::from(hit))))
            }
            Expr::And(a, b) => {
                let hit = self.eval(a)?.truthy() && self.eval(b)?.truthy();
                Ok(Value::Num(f64::from(u8::from(hit))))
            }
            Expr::Not(a) => Ok(Value::Num(f64::from(u8::from(!self.eval(a)?.truthy())))),
            Expr::Neg(a) => Ok(Value::Num(-self.eval(a)?.num())),
            Expr::Pos(a) => Ok(Value::Num(self.eval(a)?.num())),
            Expr::In(subs, name) => {
                let key = self.subscript(subs)?;
                let arr = self.array(name)?;
                let hit = arr.borrow().contains_key(&key);
                Ok(Value::Num(f64::from(u8::from(hit))))
            }
            Expr::Match(negated, lhs, rhs) => {
                let convfmt = self.convfmt();
                let text = self.eval(lhs)?.text(&convfmt);
                let re = self.regex_of(rhs)?;
                Ok(Value::Num(f64::from(u8::from(
                    re.is_match(&text) != *negated,
                ))))
            }
            Expr::Rel(op, a, b) => {
                let (x, y) = (self.eval(a)?, self.eval(b)?);
                let order = if x.numeric() && y.numeric() {
                    x.num().partial_cmp(&y.num())
                } else {
                    let convfmt = self.convfmt();
                    Some(x.text(&convfmt).cmp(&y.text(&convfmt)))
                };
                let hit = match order {
                    None => op == "!=",
                    Some(o) => match op.as_str() {
                        "<" => o.is_lt(),
                        "<=" => o.is_le(),
                        ">" => o.is_gt(),
                        ">=" => o.is_ge(),
                        "==" => o.is_eq(),
                        _ => o.is_ne(),
                    },
                };
                Ok(Value::Num(f64::from(u8::from(hit))))
            }
            Expr::Concat(a, b) => {
                let convfmt = self.convfmt();
                let mut s = self.eval(a)?.text(&convfmt);
                s.push_str(&self.eval(b)?.text(&convfmt));
                Ok(Value::Str(s))
            }
            Expr::Arith(op, a, b) => {
                let (x, y) = (self.eval(a)?.num(), self.eval(b)?.num());
                Ok(Value::Num(arith(*op, x, y)?))
            }
            Expr::IncDec { pre, inc, target } => {
                let old = self.eval(target)?.num();
                let new = if *inc { old + 1.0 } else { old - 1.0 };
                self.assign(target, Value::Num(new))?;
                Ok(Value::Num(if *pre { new } else { old }))
            }
            Expr::Call(name, args) => self.call(name, args),
            Expr::Builtin(name, args) => self.builtin(name, args),
            Expr::Getline { source, var } => self.getline(source, var.as_deref()),
        }
    }
    fn subscript(&mut self, subs: &[Expr]) -> Result<String, Fail> {
        let convfmt = self.convfmt();
        if subs.len() == 1 {
            return Ok(self.eval(&subs[0])?.text(&convfmt));
        }
        let sep = self.text_var("SUBSEP");
        let mut parts = Vec::with_capacity(subs.len());
        for s in subs {
            parts.push(self.eval(s)?.text(&convfmt));
        }
        Ok(parts.join(&sep))
    }
    fn assign(&mut self, target: &Expr, v: Value) -> Result<(), Fail> {
        match target {
            Expr::Var(name) => {
                self.set_var(name, v);
                Ok(())
            }
            Expr::Field(n) => {
                let idx = self.eval(n)?.num() as i64;
                let convfmt = self.convfmt();
                self.set_field(idx, v.text(&convfmt))
            }
            Expr::Index(name, subs) => {
                let key = self.subscript(subs)?;
                let arr = self.array(name)?;
                arr.borrow_mut().insert(key, v);
                Ok(())
            }
            _ => Err(Fail::usage(
                "awk: assignment to something that is not a variable",
            )),
        }
    }
    fn getline(&mut self, source: &GetSource, var: Option<&Expr>) -> Result<Value, Fail> {
        let convfmt = self.convfmt();
        let record = match source {
            GetSource::Main => match self.next_main_record()? {
                Some(r) => r,
                None => return Ok(Value::Num(0.0)),
            },
            GetSource::File(e) => {
                let name = self.eval(e)?.text(&convfmt);
                if !self.readers.contains_key(&name) {
                    let text = match shell::read_text(self.c, "awk", &name, &self.stdin) {
                        Ok(t) => t,
                        // A getline that cannot open its file returns -1, it does not abort.
                        Err(_) => return Ok(Value::Num(-1.0)),
                    };
                    self.readers.insert(name.clone(), Reader { text, at: 0 });
                }
                match self.take_record(Some(&name))? {
                    Some(r) => {
                        if var.is_none() {
                            let nr = self.get_var("NR").num() + 1.0;
                            self.set_var("NR", Value::Num(nr));
                        }
                        r
                    }
                    None => return Ok(Value::Num(0.0)),
                }
            }
            GetSource::Cmd(e) => {
                let name = self.eval(e)?.text(&convfmt);
                if !self.readers.contains_key(&name) {
                    let r = shell::run_piped(self.c, &name, "", self.t, self.host, self.depth);
                    self.err.push_str(&r.stderr);
                    self.readers.insert(
                        name.clone(),
                        Reader {
                            text: r.stdout,
                            at: 0,
                        },
                    );
                }
                match self.take_record(Some(&name))? {
                    Some(r) => {
                        let nr = self.get_var("NR").num() + 1.0;
                        self.set_var("NR", Value::Num(nr));
                        r
                    }
                    None => return Ok(Value::Num(0.0)),
                }
            }
        };
        match var {
            Some(target) => self.assign(target, input_value(&record))?,
            None => {
                self.set_record(record);
            }
        }
        Ok(Value::Num(1.0))
    }
    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, Fail> {
        let Some(func) = self.prog.funcs.get(name).cloned() else {
            return Err(Fail::new(
                format!("awk: calling undefined function {name}"),
                2,
            ));
        };
        if args.len() > func.params.len() {
            return Err(Fail::new(
                format!(
                    "awk: function {name} called with {} args, accepts {}",
                    args.len(),
                    func.params.len()
                ),
                2,
            ));
        }
        self.calls += 1;
        if self.calls > 256 {
            self.calls -= 1;
            return Err(Fail::new("awk: function call nesting exceeds 256", 2));
        }
        let mut frame: BTreeMap<String, Cell> = BTreeMap::new();
        for (i, param) in func.params.iter().enumerate() {
            let cell = match args.get(i) {
                // An array argument is passed by reference, as awk specifies. The
                // callee's own use of the parameter decides it for an unset name.
                Some(Expr::Var(n))
                    if self.is_array(n) || func.array_params.get(i).copied().unwrap_or(false) =>
                {
                    Cell::Array(self.array(n)?)
                }
                Some(e) => Cell::Scalar(self.eval(e)?),
                None => Cell::Scalar(Value::Uninit),
            };
            frame.insert(param.clone(), cell);
        }
        // An unset extra parameter must be usable as either a scalar or a new array,
        // so it starts as an uninitialised scalar and `array()` promotes it in place.
        self.frames.push(frame);
        let r = self.exec_list(&func.body);
        self.frames.pop();
        self.calls -= 1;
        r?;
        Ok(match std::mem::replace(&mut self.flow, Flow::Normal) {
            Flow::Return(v) => v,
            Flow::Exit => {
                self.flow = Flow::Exit;
                Value::Uninit
            }
            other => {
                self.flow = other;
                Value::Uninit
            }
        })
    }
    fn is_array(&self, name: &str) -> bool {
        matches!(self.lookup(name), Some(Cell::Array(_)))
    }

    // ---- statements
    fn exec_list(&mut self, body: &[Stmt]) -> Result<(), Fail> {
        for s in body {
            self.exec(s)?;
            if !matches!(self.flow, Flow::Normal) {
                return Ok(());
            }
        }
        Ok(())
    }
    fn exec(&mut self, s: &Stmt) -> Result<(), Fail> {
        self.charge()?;
        match s {
            Stmt::Nop => {}
            Stmt::Expr(e) => {
                self.eval(e)?;
            }
            Stmt::Block(body) => self.exec_list(body)?,
            Stmt::Print(args, redirect) => {
                let ofs = self.text_var("OFS");
                let ors = self.text_var("ORS");
                let ofmt = self.text_var("OFMT");
                let ofmt = if ofmt.is_empty() {
                    String::from("%.6g")
                } else {
                    ofmt
                };
                let mut parts = Vec::with_capacity(args.len().max(1));
                if args.is_empty() {
                    parts.push(self.record.clone());
                } else {
                    for a in args {
                        let v = self.eval(a)?;
                        // Output uses OFMT for non-integral numbers, CONVFMT elsewhere.
                        parts.push(match &v {
                            Value::Num(n) if *n != n.trunc() => num_to_str(*n, &ofmt),
                            other => other.text(&self.convfmt()),
                        });
                    }
                }
                let text = format!("{}{ors}", parts.join(&ofs));
                self.emit(redirect, &text)?;
            }
            Stmt::Printf(args, redirect) => {
                let convfmt = self.convfmt();
                let fmt = self.eval(&args[0])?.text(&convfmt);
                let mut values = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    values.push(self.eval(a)?);
                }
                let text = format_spec(&fmt, &values)?;
                self.emit(redirect, &text)?;
            }
            Stmt::If(cond, then, otherwise) => {
                if self.eval(cond)?.truthy() {
                    self.exec(then)?;
                } else if let Some(e) = otherwise {
                    self.exec(e)?;
                }
            }
            Stmt::While(cond, body) => {
                while self.eval(cond)?.truthy() {
                    self.exec(body)?;
                    if self.loop_flow()? {
                        break;
                    }
                }
            }
            Stmt::DoWhile(body, cond) => loop {
                self.exec(body)?;
                if self.loop_flow()? {
                    break;
                }
                if !self.eval(cond)?.truthy() {
                    break;
                }
            },
            Stmt::For(init, cond, step, body) => {
                if let Some(i) = init {
                    self.exec(i)?;
                }
                loop {
                    if let Some(c) = cond {
                        if !self.eval(c)?.truthy() {
                            break;
                        }
                    }
                    self.exec(body)?;
                    if self.loop_flow()? {
                        break;
                    }
                    if let Some(s) = step {
                        self.exec(s)?;
                    }
                }
            }
            Stmt::ForIn(var, name, body) => {
                let arr = self.array(name)?;
                // Keys are snapshotted, so deleting inside the loop is safe. The order
                // is the sorted one: POSIX leaves it unspecified, this world fixes it.
                let keys: Vec<String> = arr.borrow().keys().cloned().collect();
                for k in keys {
                    self.set_var(var, input_value(&k));
                    self.exec(body)?;
                    if self.loop_flow()? {
                        break;
                    }
                }
            }
            Stmt::Delete(name, subs) => {
                if subs.is_empty() {
                    let arr = self.array(name)?;
                    arr.borrow_mut().clear();
                } else {
                    let key = self.subscript(subs)?;
                    let arr = self.array(name)?;
                    arr.borrow_mut().remove(&key);
                }
            }
            Stmt::Break => self.flow = Flow::Break,
            Stmt::Continue => self.flow = Flow::Continue,
            Stmt::Next => self.flow = Flow::Next,
            Stmt::NextFile => self.flow = Flow::NextFile,
            Stmt::Exit(v) => {
                if let Some(e) = v {
                    self.status = self.eval(e)?.num() as i32;
                }
                self.flow = Flow::Exit;
            }
            Stmt::Return(v) => {
                let value = match v {
                    Some(e) => self.eval(e)?,
                    None => Value::Uninit,
                };
                self.flow = Flow::Return(value);
            }
        }
        Ok(())
    }
    /// Consumes `break`/`continue` inside a loop; returns true when the loop must stop.
    fn loop_flow(&mut self) -> Result<bool, Fail> {
        Ok(match self.flow {
            Flow::Break => {
                self.flow = Flow::Normal;
                true
            }
            Flow::Continue => {
                self.flow = Flow::Normal;
                false
            }
            Flow::Normal => false,
            _ => true,
        })
    }

    // ---- builtins
    fn builtin(&mut self, name: &str, args: &[Expr]) -> Result<Value, Fail> {
        let convfmt = self.convfmt();
        let arity = |want: std::ops::RangeInclusive<usize>| -> Result<(), Fail> {
            if want.contains(&args.len()) {
                Ok(())
            } else {
                Err(Fail::new(
                    format!("awk: {name} takes {:?} arguments, got {}", want, args.len()),
                    2,
                ))
            }
        };
        match name {
            "length" => {
                arity(0..=1)?;
                Ok(Value::Num(match args.first() {
                    None => self.record.chars().count() as f64,
                    Some(Expr::Var(n)) if self.is_array(n) => {
                        let arr = self.array(n)?;
                        let len = arr.borrow().len();
                        len as f64
                    }
                    Some(e) => self.eval(e)?.text(&convfmt).chars().count() as f64,
                }))
            }
            "substr" => {
                arity(2..=3)?;
                let s: Vec<char> = self.eval(&args[0])?.text(&convfmt).chars().collect();
                let m = round_half_up(self.eval(&args[1])?.num());
                // POSIX: the result is the characters whose positions p satisfy
                // m <= p < m+n, clipped to the string. Negative starts are legal.
                let start = m;
                let end = match args.get(2) {
                    Some(e) => start + round_half_up(self.eval(e)?.num()),
                    None => f64::INFINITY,
                };
                let lo = start.max(1.0);
                let hi = end.min(s.len() as f64 + 1.0);
                if hi <= lo {
                    return Ok(Value::Str(String::new()));
                }
                let (lo, hi) = (lo as usize - 1, hi as usize - 1);
                Ok(Value::Str(s[lo..hi].iter().collect()))
            }
            "index" => {
                arity(2..=2)?;
                let s = self.eval(&args[0])?.text(&convfmt);
                let t = self.eval(&args[1])?.text(&convfmt);
                Ok(Value::Num(match s.find(&t) {
                    Some(byte) => s[..byte].chars().count() as f64 + 1.0,
                    None => 0.0,
                }))
            }
            "split" => {
                arity(2..=3)?;
                let text = self.eval(&args[0])?.text(&convfmt);
                let Expr::Var(array_name) = &args[1] else {
                    return Err(Fail::new(
                        "awk: split's second argument must be an array",
                        2,
                    ));
                };
                let fs = match args.get(2) {
                    Some(Expr::Regex(r)) => r.clone(),
                    Some(e) => self.eval(e)?.text(&convfmt),
                    None => self.text_var("FS"),
                };
                let parts = self.split_text(&text, &fs, false);
                let arr = self.array(array_name)?;
                arr.borrow_mut().clear();
                for (i, p) in parts.iter().enumerate() {
                    arr.borrow_mut().insert((i + 1).to_string(), input_value(p));
                }
                Ok(Value::Num(parts.len() as f64))
            }
            "sub" | "gsub" => {
                arity(2..=3)?;
                let re = self.regex_of(&args[0])?;
                let repl = self.eval(&args[1])?.text(&convfmt);
                let target = args
                    .get(2)
                    .cloned()
                    .unwrap_or(Expr::Field(Box::new(Expr::Num(0.0))));
                if !is_lvalue(&target) {
                    return Err(Fail::new(
                        format!("awk: {name}'s third argument must be assignable"),
                        2,
                    ));
                }
                let subject = self.eval(&target)?.text(&convfmt);
                let (text, n) = substitute(&re, &repl, &subject, name == "gsub");
                if n > 0 {
                    self.assign(&target, Value::Str(text))?;
                }
                Ok(Value::Num(n as f64))
            }
            "match" => {
                arity(2..=2)?;
                let s = self.eval(&args[0])?.text(&convfmt);
                let re = self.regex_of(&args[1])?;
                match re.find(&s) {
                    Some(m) => {
                        let start = s[..m.start()].chars().count() + 1;
                        let len = m.as_str().chars().count();
                        self.set_var("RSTART", Value::Num(start as f64));
                        self.set_var("RLENGTH", Value::Num(len as f64));
                        Ok(Value::Num(start as f64))
                    }
                    None => {
                        self.set_var("RSTART", Value::Num(0.0));
                        self.set_var("RLENGTH", Value::Num(-1.0));
                        Ok(Value::Num(0.0))
                    }
                }
            }
            "sprintf" => {
                if args.is_empty() {
                    return Err(Fail::new("awk: sprintf needs a format string", 2));
                }
                let fmt = self.eval(&args[0])?.text(&convfmt);
                let mut values = Vec::new();
                for a in &args[1..] {
                    values.push(self.eval(a)?);
                }
                Ok(Value::Str(format_spec(&fmt, &values)?))
            }
            "toupper" | "tolower" => {
                arity(1..=1)?;
                let s = self.eval(&args[0])?.text(&convfmt);
                Ok(Value::Str(if name == "toupper" {
                    s.to_uppercase()
                } else {
                    s.to_lowercase()
                }))
            }
            "sin" | "cos" | "exp" | "log" | "sqrt" | "int" => {
                arity(1..=1)?;
                let x = self.eval(&args[0])?.num();
                Ok(Value::Num(match name {
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "exp" => x.exp(),
                    "log" => x.ln(),
                    "sqrt" => x.sqrt(),
                    _ => x.trunc(),
                }))
            }
            "atan2" => {
                arity(2..=2)?;
                let y = self.eval(&args[0])?.num();
                let x = self.eval(&args[1])?.num();
                Ok(Value::Num(y.atan2(x)))
            }
            "rand" => {
                arity(0..=0)?;
                Ok(Value::Num(self.next_random()))
            }
            "srand" => {
                arity(0..=1)?;
                let previous = self.seed;
                let seed = match args.first() {
                    Some(e) => self.eval(e)?.num() as i64 as u64,
                    // With no argument awk seeds from the time of day; here that is the
                    // simulated tick, so a replay reproduces the same sequence.
                    None => self.t,
                };
                self.seed = seed;
                self.rng = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                Ok(Value::Num(previous as f64))
            }
            "system" => {
                arity(1..=1)?;
                let command = self.eval(&args[0])?.text(&convfmt);
                // Everything buffered is flushed first, exactly as awk promises.
                self.flush_all()?;
                let r = shell::run_piped(self.c, &command, "", self.t, self.host, self.depth);
                self.out.push_str(&r.stdout);
                self.err.push_str(&r.stderr);
                Ok(Value::Num(f64::from(r.exit_code)))
            }
            "close" => {
                arity(1..=1)?;
                let target = self.eval(&args[0])?.text(&convfmt);
                Ok(Value::Num(f64::from(self.close_stream(&target)?)))
            }
            "fflush" => {
                arity(0..=1)?;
                match args.first() {
                    None => self.flush_all()?,
                    Some(e) => {
                        let target = self.eval(e)?.text(&convfmt);
                        self.close_stream(&target)?;
                    }
                }
                Ok(Value::Num(0.0))
            }
            _ => Err(Fail::new(format!("awk: unknown function {name}"), 2)),
        }
    }
    /// A 48-bit LCG, seeded from the world rather than the host clock.
    fn next_random(&mut self) -> f64 {
        self.rng = self.rng.wrapping_mul(0x5DEE_CE66D).wrapping_add(0xB) & 0xFFFF_FFFF_FFFF;
        (self.rng >> 16) as f64 / f64::from(u32::MAX)
    }

    // ---- driver
    fn run_rules(&mut self, phase: Phase) -> Result<(), Fail> {
        let rules = self.prog.rules.clone();
        for (i, rule) in rules.iter().enumerate() {
            let selected = match (&rule.pattern, phase) {
                (Pattern::Begin, Phase::Begin) | (Pattern::End, Phase::End) => true,
                (Pattern::Always, Phase::Main) => true,
                (Pattern::Expr(e), Phase::Main) => self.eval(e)?.truthy(),
                (Pattern::Range(a, b), Phase::Main) => {
                    if self.ranges[i] {
                        if self.eval(b)?.truthy() {
                            self.ranges[i] = false;
                        }
                        true
                    } else if self.eval(a)?.truthy() {
                        // A one-line range closes on the same record.
                        self.ranges[i] = !self.eval(b)?.truthy();
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if !selected {
                continue;
            }
            match &rule.action {
                None => {
                    let record = self.record.clone();
                    let ors = self.text_var("ORS");
                    self.out.push_str(&record);
                    self.out.push_str(&ors);
                }
                Some(body) => self.exec_list(body)?,
            }
            match self.flow {
                Flow::Normal => {}
                Flow::Next | Flow::NextFile => return Ok(()),
                Flow::Exit => return Ok(()),
                _ => {
                    self.flow = Flow::Normal;
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Begin,
    Main,
    End,
}
fn arith(op: char, x: f64, y: f64) -> Result<f64, Fail> {
    Ok(match op {
        '+' => x + y,
        '-' => x - y,
        '*' => x * y,
        '^' => x.powf(y),
        '/' => {
            if y == 0.0 {
                return Err(Fail::new("awk: division by zero", 2));
            }
            x / y
        }
        '%' => {
            if y == 0.0 {
                return Err(Fail::new("awk: division by zero in %", 2));
            }
            x % y
        }
        _ => return Err(Fail::new(format!("awk: unknown operator {op}"), 2)),
    })
}
fn round_half_up(v: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    (v + 0.5).floor()
}
/// `sub`/`gsub` replacement: `&` is the matched text, `\&` a literal ampersand.
fn substitute(re: &regex::Regex, repl: &str, subject: &str, global: bool) -> (String, usize) {
    let mut out = String::new();
    let mut count = 0;
    let mut at = 0;
    let mut previous_end: Option<usize> = None;
    while at <= subject.len() {
        let Some(m) = re.find_at(subject, at) else {
            break;
        };
        // An empty match that touches the end of the previous one is not a match:
        // `gsub(/l*/,"-")` over "hello" gives `-h-e-o-`, with no dash between the l's.
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
        out.push_str(&subject[at..m.start()]);
        let matched = m.as_str();
        let mut chars = repl.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '&' => out.push_str(matched),
                '\\' => match chars.peek() {
                    Some('&') => {
                        out.push('&');
                        chars.next();
                    }
                    Some('\\') => {
                        out.push('\\');
                        chars.next();
                    }
                    _ => out.push('\\'),
                },
                other => out.push(other),
            }
        }
        count += 1;
        previous_end = Some(m.end());
        if m.end() == m.start() {
            at = m.start();
            if !global {
                break;
            }
            let Some(ch) = subject[at..].chars().next() else {
                at = subject.len();
                break;
            };
            out.push(ch);
            at += ch.len_utf8();
        } else {
            at = m.end();
        }
        if !global {
            break;
        }
    }
    out.push_str(&subject[at.min(subject.len())..]);
    (out, count)
}
/// Translates an ERE into the syntax the host regex engine accepts. The only
/// deliberate extension is `\b`/`\y` as a word boundary, which is what scripts in the
/// wild mean by it.
pub(crate) fn regex_source(ere: &str) -> String {
    let b: Vec<char> = ere.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_bracket = false;
    while i < b.len() {
        let ch = b[i];
        if in_bracket {
            out.push(ch);
            if ch == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if ch == ']' {
                in_bracket = false;
            }
            i += 1;
            continue;
        }
        if ch == '[' {
            in_bracket = true;
            out.push(ch);
            i += 1;
            // A `]` or `^]` directly after `[` is a literal bracket.
            if b.get(i) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            if b.get(i) == Some(&']') {
                out.push_str("\\]");
                i += 1;
            }
            continue;
        }
        if ch != '\\' {
            out.push(ch);
            i += 1;
            continue;
        }
        let Some(next) = b.get(i + 1).copied() else {
            out.push_str("\\\\");
            i += 1;
            continue;
        };
        i += 2;
        match next {
            'n' | 't' | 'r' | 'f' | 'd' | 'D' | 'w' | 'W' | 's' | 'S' | 'A' | 'z' | 'x' | 'p'
            | 'P' | 'b' | 'B' => {
                out.push('\\');
                out.push(next);
            }
            'y' => out.push_str("\\b"),
            'a' => out.push_str("\\x07"),
            'v' => out.push_str("\\x0B"),
            c if c.is_ascii_digit() => {
                out.push('\\');
                out.push(c);
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out
}

// ---------------------------------------------------------------- printf

/// The full `printf` conversion set, shared by awk's `printf`/`sprintf` and by the
/// shell's own `printf` builtin, so the two never disagree.
pub(crate) fn format_spec(fmt: &str, args: &[Value]) -> Result<String, Fail> {
    let b: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut arg = 0;
    let mut i = 0;
    let next = |arg: &mut usize| -> Value {
        let v = args.get(*arg).cloned().unwrap_or(Value::Uninit);
        *arg += 1;
        v
    };
    while i < b.len() {
        if b[i] != '%' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        if b.get(i) == Some(&'%') {
            out.push('%');
            i += 1;
            continue;
        }
        let mut minus = false;
        let mut plus = false;
        let mut space = false;
        let mut zero = false;
        let mut alt = false;
        while let Some(c) = b.get(i) {
            match c {
                '-' => minus = true,
                '+' => plus = true,
                ' ' => space = true,
                '0' => zero = true,
                '#' => alt = true,
                _ => break,
            }
            i += 1;
        }
        let mut width: Option<usize> = None;
        if b.get(i) == Some(&'*') {
            i += 1;
            let w = next(&mut arg).num();
            if w < 0.0 {
                minus = true;
            }
            width = Some(w.abs() as usize);
        } else {
            let start = i;
            while b.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            if i > start {
                width = b[start..i].iter().collect::<String>().parse().ok();
            }
        }
        let mut precision: Option<usize> = None;
        if b.get(i) == Some(&'.') {
            i += 1;
            if b.get(i) == Some(&'*') {
                i += 1;
                precision = Some(next(&mut arg).num().max(0.0) as usize);
            } else {
                let start = i;
                while b.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
                precision = Some(
                    b[start..i]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0usize),
                );
            }
        }
        let Some(conv) = b.get(i).copied() else {
            return Err(Fail::usage(format!(
                "printf: `{fmt}` ends with an incomplete conversion"
            )));
        };
        i += 1;
        let mut body = match conv {
            'd' | 'i' => {
                let v = next(&mut arg).num();
                let n = if v.is_nan() { 0 } else { v.trunc() as i64 };
                let mut s = n.unsigned_abs().to_string();
                if let Some(p) = precision {
                    while s.len() < p {
                        s.insert(0, '0');
                    }
                }
                let sign = if n < 0 {
                    "-"
                } else if plus {
                    "+"
                } else if space {
                    " "
                } else {
                    ""
                };
                format!("{sign}{s}")
            }
            'o' | 'x' | 'X' | 'u' => {
                let v = next(&mut arg).num();
                let n = if v.is_nan() { 0 } else { v.trunc() as i64 };
                let magnitude = if conv == 'u' && n < 0 {
                    u128::from(n as u64)
                } else {
                    u128::from(n.unsigned_abs())
                };
                let mut s = match conv {
                    'o' => format!("{magnitude:o}"),
                    'x' => format!("{magnitude:x}"),
                    'X' => format!("{magnitude:X}"),
                    _ => format!("{magnitude}"),
                };
                if let Some(p) = precision {
                    while s.len() < p {
                        s.insert(0, '0');
                    }
                }
                if alt {
                    match conv {
                        'o' if !s.starts_with('0') => s.insert(0, '0'),
                        'x' => s.insert_str(0, "0x"),
                        'X' => s.insert_str(0, "0X"),
                        _ => {}
                    }
                }
                if n < 0 && conv != 'u' {
                    s.insert(0, '-');
                }
                s
            }
            'c' => {
                let v = next(&mut arg);
                match &v {
                    Value::Num(n) => char::from_u32(*n as u32)
                        .map(String::from)
                        .unwrap_or_default(),
                    other => {
                        let s = other.text("%.6g");
                        s.chars().next().map(String::from).unwrap_or_default()
                    }
                }
            }
            // `%b` is the shell's "string with escapes expanded"; awk never emits it
            // but accepting it keeps one formatter behind both printfs.
            's' | 'b' => {
                let s = next(&mut arg).text("%.6g");
                let s = if conv == 'b' {
                    crate::textutils::printf_escapes(&s)
                } else {
                    s
                };
                match precision {
                    Some(p) => s.chars().take(p).collect(),
                    None => s,
                }
            }
            'e' | 'E' | 'f' | 'F' | 'g' | 'G' | 'a' | 'A' => {
                let v = next(&mut arg).num();
                let p = precision.unwrap_or(6);
                let mut s = match conv {
                    'f' | 'F' => format!("{:.*}", p, v.abs()),
                    'e' | 'E' => exponential(v.abs(), p, conv == 'E'),
                    'a' | 'A' => format!("{:.*}", p, v.abs()),
                    _ => general(v.abs(), if p == 0 { 1 } else { p }, conv == 'G', alt),
                };
                let sign = if v.is_sign_negative() && (v != 0.0 || 1.0 / v < 0.0) {
                    "-"
                } else if plus {
                    "+"
                } else if space {
                    " "
                } else {
                    ""
                };
                s.insert_str(0, sign);
                s
            }
            other => {
                return Err(Fail::usage(format!(
                    "printf: unsupported conversion `%{other}`"
                )))
            }
        };
        if let Some(w) = width {
            let len = body.chars().count();
            if len < w {
                let pad = w - len;
                if minus {
                    body.push_str(&" ".repeat(pad));
                } else if zero && !matches!(conv, 's' | 'c') && precision.is_none() {
                    // Zeros go after the sign, never before it: `%05d` of -7 is -0007.
                    let at = body
                        .chars()
                        .next()
                        .filter(|c| matches!(c, '-' | '+' | ' '))
                        .map_or(0, char::len_utf8);
                    body.insert_str(at, &"0".repeat(pad));
                } else {
                    body.insert_str(0, &" ".repeat(pad));
                }
            }
        }
        out.push_str(&body);
    }
    Ok(out)
}
fn exponential(v: f64, precision: usize, upper: bool) -> String {
    let s = format!("{v:.*e}", precision);
    // Rust writes `1.5e2`; C writes `1.500000e+02`.
    let (mantissa, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
    let value: i32 = exp.parse().unwrap_or(0);
    let text = format!(
        "{mantissa}{}{}{:02}",
        if upper { 'E' } else { 'e' },
        if value < 0 { '-' } else { '+' },
        value.abs()
    );
    text
}
fn general(v: f64, precision: usize, upper: bool, alt: bool) -> String {
    if v == 0.0 {
        return if alt {
            format!("{:.*}", precision - 1, 0.0)
        } else {
            String::from("0")
        };
    }
    let exp = v.abs().log10().floor() as i32;
    let mut s = if exp < -4 || exp >= precision as i32 {
        let e = exponential(v, precision - 1, upper);
        if alt {
            e
        } else {
            let (m, x) = e.split_once(['e', 'E']).unwrap_or((e.as_str(), ""));
            let m = trim_zeros(m);
            format!("{m}{}{x}", if upper { 'E' } else { 'e' })
        }
    } else {
        let digits = (precision as i32 - 1 - exp).max(0) as usize;
        format!("{v:.*}", digits)
    };
    if !alt && !s.contains(['e', 'E']) {
        s = trim_zeros(&s);
    }
    s
}
fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0');
    t.trim_end_matches('.').to_string()
}

// ---------------------------------------------------------------- command

fn is_assignment(operand: &str) -> bool {
    split_assignment(operand).is_some()
}
fn split_assignment(operand: &str) -> Option<(String, String)> {
    let (name, value) = operand.split_once('=')?;
    if name.is_empty() {
        return None;
    }
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some((name.to_string(), value.to_string()))
}
/// `-v x='a\tb'` and `x=… ` operands honour C escapes, as POSIX requires.
fn unescape_command_line(value: &str) -> String {
    let b: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\\' && i + 1 < b.len() {
            let (text, used) = string_escape(&b, i + 1);
            out.push_str(&text);
            i += 1 + used;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}
/// `-F t` means a tab, a historical awk wart every implementation keeps.
fn field_separator(raw: &str) -> String {
    if raw == "t" {
        return String::from("\t");
    }
    unescape_command_line(raw)
}

pub(crate) fn execute(
    c: &mut Computer,
    args: &[String],
    input: &str,
    t: u64,
    host: &mut dyn ShellHost,
    depth: usize,
) -> Result<String, Fail> {
    // Hand-rolled because awk's operand grammar is not getopt's: the program text is a
    // positional argument only when no -f was given, and everything after it is data.
    let mut fs: Option<String> = None;
    let mut assigns: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut source = String::new();
    let mut have_source = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            i += 1;
            break;
        }
        if a == "-" || !a.starts_with('-') {
            break;
        }
        let (letter, inline) = {
            let mut chars = a[1..].chars();
            let letter = chars.next().unwrap_or('\0');
            let rest: String = chars.collect();
            (letter, rest)
        };
        if a.starts_with("--") {
            let name = a.trim_start_matches('-');
            let (name, inline) = match name.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (name, None),
            };
            let ch = match name {
                "field-separator" => 'F',
                "assign" => 'v',
                "file" | "source" if name == "file" => 'f',
                "source" => 'e',
                "version" => {
                    return Ok(String::from("awk (computerworld) POSIX profile\n"));
                }
                _ => return Err(shell::unrecognized_option("awk", name)),
            };
            let v = match inline {
                Some(v) => v,
                None => {
                    i += 1;
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| shell::missing_argument("awk", name))?
                }
            };
            match ch {
                'F' => fs = Some(field_separator(&v)),
                'v' => assigns.push(v),
                'f' => {
                    let text = shell::read_text(c, "awk", &v, input)?;
                    source.push_str(&text);
                    source.push('\n');
                    have_source = true;
                }
                _ => {
                    source.push_str(&v);
                    source.push('\n');
                    have_source = true;
                }
            }
            i += 1;
            continue;
        }
        if !matches!(letter, 'F' | 'v' | 'f') {
            return Err(shell::invalid_option("awk", letter));
        }
        let v = if inline.is_empty() {
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| shell::missing_argument("awk", &letter.to_string()))?
        } else {
            inline
        };
        match letter {
            'F' => fs = Some(field_separator(&v)),
            'v' => assigns.push(v),
            _ => {
                let text = shell::read_text(c, "awk", &v, input)?;
                source.push_str(&text);
                source.push('\n');
                have_source = true;
            }
        }
        i += 1;
    }
    if !have_source {
        let Some(program) = args.get(i) else {
            return Err(Fail::usage(format!(
                "awk: no program text\n{}",
                shell::usage_line("awk")
            )));
        };
        source = program.clone();
        i += 1;
    }
    files.extend(args[i.min(args.len())..].iter().cloned());

    let tokens = lex(&source)?;
    let mut parser = Parser { t: tokens, i: 0 };
    let prog = parser.program()?;
    let ranges = vec![false; prog.rules.len()];
    let needs_input = prog
        .rules
        .iter()
        .any(|r| !matches!(r.pattern, Pattern::Begin));

    let mut globals: BTreeMap<String, Cell> = BTreeMap::new();
    for (k, v) in [
        ("FS", " "),
        ("OFS", " "),
        ("ORS", "\n"),
        ("RS", "\n"),
        ("SUBSEP", "\u{1c}"),
        ("CONVFMT", "%.6g"),
        ("OFMT", "%.6g"),
        ("FILENAME", ""),
    ] {
        globals.insert(k.into(), Cell::Scalar(Value::Str(v.into())));
    }
    for k in ["NR", "FNR", "RSTART"] {
        globals.insert(k.into(), Cell::Scalar(Value::Num(0.0)));
    }
    globals.insert("RLENGTH".into(), Cell::Scalar(Value::Num(-1.0)));
    let environ = Rc::new(std::cell::RefCell::new(
        c.env
            .iter()
            .map(|(k, v)| (k.clone(), input_value(v)))
            .collect::<BTreeMap<String, Value>>(),
    ));
    globals.insert("ENVIRON".into(), Cell::Array(environ));
    if let Some(fs) = fs {
        globals.insert("FS".into(), Cell::Scalar(Value::Str(fs)));
    }
    let seed = t ^ 0x2545_F491_4F6C_DD1D;
    let mut interp = Interp {
        prog,
        globals,
        frames: Vec::new(),
        record: String::new(),
        fields: Vec::new(),
        out: String::new(),
        err: String::new(),
        streams: BTreeMap::new(),
        readers: BTreeMap::new(),
        regexes: BTreeMap::new(),
        ranges,
        flow: Flow::Normal,
        status: 0,
        seed: 0,
        rng: seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1),
        calls: 0,
        steps: 0,
        operands: files,
        next_operand: 0,
        current: None,
        used_stdin: false,
        stdin: input.to_string(),
        c,
        host,
        t,
        depth,
    };
    for a in &assigns {
        let Some((name, raw)) = split_assignment(a) else {
            return Err(Fail::usage(format!(
                "awk: `-v {a}` is not a NAME=VALUE assignment"
            )));
        };
        let text = unescape_command_line(&raw);
        interp.set_var(&name, input_value(&text));
    }

    let outcome = (|| -> Result<(), Fail> {
        interp.run_rules(Phase::Begin)?;
        let exiting = matches!(interp.flow, Flow::Exit);
        interp.flow = Flow::Normal;
        if !exiting && (needs_input || interp.next_operand < interp.operands.len()) {
            'input: while let Some(record) = interp.next_main_record()? {
                interp.set_record(record);
                interp.run_rules(Phase::Main)?;
                match interp.flow {
                    Flow::Exit => break 'input,
                    Flow::NextFile => {
                        interp.current = None;
                        interp.flow = Flow::Normal;
                    }
                    _ => interp.flow = Flow::Normal,
                }
            }
        }
        interp.flow = Flow::Normal;
        interp.run_rules(Phase::End)?;
        interp.flow = Flow::Normal;
        interp.flush_all()
    })();
    let mut out = std::mem::take(&mut interp.out);
    let mut err = std::mem::take(&mut interp.err);
    let status = interp.status;
    match outcome {
        Err(f) => Err(f.with_output(out)),
        Ok(()) if status != 0 || !err.is_empty() => {
            if err.ends_with('\n') {
                err.pop();
            }
            if out.is_empty() {
                out = String::new();
            }
            Err(Fail::new(err, status).raw().with_output(out))
        }
        Ok(()) => Ok(out),
    }
}

/// A shell operand seen as an awk scalar: numeric when it reads as a number, so the
/// shell's own `printf` applies the same conversion rules awk does.
pub(crate) fn scalar(s: &str) -> Value {
    input_value(s)
}
pub(crate) fn scalar_num(v: f64) -> Value {
    Value::Num(v)
}
