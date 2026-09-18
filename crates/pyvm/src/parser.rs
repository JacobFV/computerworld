//! Recursive-descent parser for Python 3 producing `ast` nodes.
use crate::ast::*;
use crate::lexer::{self, FPart, SyntaxErr, Tok, Token};
use std::rc::Rc;

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: u32,
}

type PResult<T> = Result<T, SyntaxErr>;

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

pub fn parse_module(src: &str) -> PResult<(Vec<Stmt>, Vec<lexer::Warning>)> {
    let (toks, warnings) = lexer::tokenize(src)?;
    let mut p = Parser {
        toks,
        pos: 0,
        depth: 0,
    };
    let mut body = vec![];
    while !p.at_eof() {
        if p.at(&Tok::Newline) {
            p.pos += 1;
            continue;
        }
        if p.at(&Tok::Indent) {
            let t = p.cur().clone();
            return Err(SyntaxErr::indent("unexpected indent", t.line, t.col));
        }
        body.extend(p.statement()?);
    }
    Ok((body, warnings))
}

fn mk(kind: ExprKind, t: &Token) -> Expr {
    Expr {
        kind,
        line: t.line,
        col: t.col,
        end_col: t.end_col,
    }
}

impl Parser {
    fn cur(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }
    fn tok(&self) -> &Tok {
        &self.cur().tok
    }
    fn peek_tok(&self, off: usize) -> &Tok {
        &self.toks[(self.pos + off).min(self.toks.len() - 1)].tok
    }
    fn at(&self, t: &Tok) -> bool {
        self.tok() == t
    }
    fn at_eof(&self) -> bool {
        matches!(self.tok(), Tok::Eof)
    }
    fn at_op(&self, op: &str) -> bool {
        matches!(self.tok(), Tok::Op(o) if *o == op)
    }
    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.tok(), Tok::Name(n) if n == kw)
    }
    fn eat_op(&mut self, op: &str) -> bool {
        if self.at_op(op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn prev_end(&self) -> (u32, u32) {
        let t = &self.toks[self.pos.saturating_sub(1)];
        (t.end_line, t.end_col)
    }
    fn error_here(&self, msg: impl Into<String>) -> SyntaxErr {
        let t = self.cur();
        let msg = msg.into();
        if matches!(t.tok, Tok::Indent) {
            return SyntaxErr::indent("unexpected indent", t.line, t.col);
        }
        if matches!(t.tok, Tok::Eof) && msg == "invalid syntax" {
            let (l, c) = self.prev_end();
            return SyntaxErr::new("invalid syntax", l, c);
        }
        let mut e = SyntaxErr::new(msg, t.line, t.col);
        e.end_col = t.end_col.max(t.col + 1) + 1;
        e
    }
    fn invalid(&self) -> SyntaxErr {
        self.error_here("invalid syntax")
    }
    fn expect_op(&mut self, op: &str) -> PResult<()> {
        if self.eat_op(op) {
            return Ok(());
        }
        if op == ":" {
            let (l, c) = self.prev_end();
            // CPython points just past the header when the colon is missing.
            if matches!(self.tok(), Tok::Newline | Tok::Eof)
                || self.cur().line != l
                || !matches!(self.tok(), Tok::Op(_))
            {
                return Err(SyntaxErr::new("expected ':'", l, c));
            }
            return Err(self.invalid());
        }
        Err(self.invalid())
    }
    fn name(&mut self) -> PResult<String> {
        match self.tok().clone() {
            Tok::Name(n) if !KEYWORDS.contains(&n.as_str()) => {
                self.pos += 1;
                Ok(n)
            }
            _ => Err(self.invalid()),
        }
    }
    fn end_simple(&mut self) -> PResult<()> {
        if self.at(&Tok::Newline) {
            self.pos += 1;
            Ok(())
        } else if self.at_eof() {
            Ok(())
        } else {
            Err(self.invalid())
        }
    }

    // ---------- statements ----------

    fn statement(&mut self) -> PResult<Vec<Stmt>> {
        let t = self.cur().clone();
        let line = t.line;
        let s = |kind| Stmt { kind, line };
        if let Tok::Name(n) = &t.tok {
            match n.as_str() {
                "if" => return Ok(vec![self.if_stmt()?]),
                "while" => {
                    self.pos += 1;
                    let cond = self.named_expr()?;
                    self.expect_op(":")?;
                    let body = self.block("while", line)?;
                    let orelse = if self.at_kw("else") {
                        let l = self.cur().line;
                        self.pos += 1;
                        self.expect_op(":")?;
                        self.block("else", l)?
                    } else {
                        vec![]
                    };
                    return Ok(vec![s(StmtKind::While(cond, body, orelse))]);
                }
                "for" => return Ok(vec![self.for_stmt(false)?]),
                "try" => return Ok(vec![self.try_stmt()?]),
                "with" => return Ok(vec![self.with_stmt(false)?]),
                "def" => return Ok(vec![self.funcdef(vec![], false)?]),
                "class" => return Ok(vec![self.classdef(vec![])?]),
                "async" => {
                    self.pos += 1;
                    if self.at_kw("def") {
                        return Ok(vec![self.funcdef(vec![], true)?]);
                    }
                    if self.at_kw("for") {
                        return Ok(vec![self.for_stmt(true)?]);
                    }
                    if self.at_kw("with") {
                        return Ok(vec![self.with_stmt(true)?]);
                    }
                    return Err(self.invalid());
                }
                "match" => {
                    if let Some(m) = self.try_match()? {
                        return Ok(vec![m]);
                    }
                }
                _ => {}
            }
        }
        if self.at_op("@") {
            let mut decorators = vec![];
            while self.eat_op("@") {
                decorators.push(self.named_expr()?);
                if !self.at(&Tok::Newline) {
                    return Err(self.invalid());
                }
                self.pos += 1;
            }
            if self.at_kw("def") {
                return Ok(vec![self.funcdef(decorators, false)?]);
            }
            if self.at_kw("async") {
                self.pos += 1;
                return Ok(vec![self.funcdef(decorators, true)?]);
            }
            if self.at_kw("class") {
                return Ok(vec![self.classdef(decorators)?]);
            }
            return Err(self.invalid());
        }
        self.simple_stmts()
    }

    fn simple_stmts(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = vec![self.small_stmt()?];
        while self.eat_op(";") {
            if self.at(&Tok::Newline) || self.at_eof() {
                break;
            }
            out.push(self.small_stmt()?);
        }
        self.end_simple()?;
        Ok(out)
    }

    fn block(&mut self, what: &str, header_line: u32) -> PResult<Vec<Stmt>> {
        if self.at(&Tok::Newline) {
            self.pos += 1;
            if !self.at(&Tok::Indent) {
                let t = self.cur();
                let what = match what {
                    "if" | "elif" | "while" | "for" | "try" | "with" | "else" | "finally"
                    | "match" => format!("'{what}' statement"),
                    "except" => "'except' statement".into(),
                    "case" => "'case' statement".into(),
                    "def" => "function definition".into(),
                    "class" => "class definition".into(),
                    other => format!("'{other}' statement"),
                };
                return Err(SyntaxErr::indent(
                    format!("expected an indented block after {what} on line {header_line}"),
                    t.line,
                    t.col,
                ));
            }
            self.pos += 1;
            let mut body = vec![];
            while !self.at(&Tok::Dedent) && !self.at_eof() {
                if self.at(&Tok::Newline) {
                    self.pos += 1;
                    continue;
                }
                body.extend(self.statement()?);
            }
            if self.at(&Tok::Dedent) {
                self.pos += 1;
            }
            Ok(body)
        } else {
            self.simple_stmts()
        }
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let line = self.cur().line;
        let kw = if self.at_kw("if") { "if" } else { "elif" };
        self.pos += 1;
        let cond = self.named_expr()?;
        self.expect_op(":")?;
        let body = self.block(kw, line)?;
        let orelse = if self.at_kw("elif") {
            vec![self.if_stmt()?]
        } else if self.at_kw("else") {
            let l = self.cur().line;
            self.pos += 1;
            self.expect_op(":")?;
            self.block("else", l)?
        } else {
            vec![]
        };
        Ok(Stmt {
            kind: StmtKind::If(cond, body, orelse),
            line,
        })
    }

    fn for_stmt(&mut self, is_async: bool) -> PResult<Stmt> {
        let line = self.cur().line;
        self.pos += 1;
        let target = self.target_list()?;
        if !self.eat_kw("in") {
            return Err(self.invalid());
        }
        let iter = self.star_expressions()?;
        self.expect_op(":")?;
        let body = self.block("for", line)?;
        let orelse = if self.at_kw("else") {
            let l = self.cur().line;
            self.pos += 1;
            self.expect_op(":")?;
            self.block("else", l)?
        } else {
            vec![]
        };
        Ok(Stmt {
            kind: StmtKind::For(target, iter, body, orelse, is_async),
            line,
        })
    }

    fn target_list(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let first = self.star_target()?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.at_kw("in") || self.at_op("=") {
                break;
            }
            items.push(self.star_target()?);
        }
        Ok(mk(ExprKind::Tuple(items), &t))
    }
    fn star_target(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        if self.eat_op("*") {
            let inner = self.bitor()?;
            let e = mk(ExprKind::Starred(Box::new(inner)), &t);
            check_target(&e, false)?;
            return Ok(e);
        }
        let e = self.bitor()?;
        check_target(&e, false)?;
        Ok(e)
    }

    fn try_stmt(&mut self) -> PResult<Stmt> {
        let line = self.cur().line;
        self.pos += 1;
        self.expect_op(":")?;
        let body = self.block("try", line)?;
        let mut handlers = vec![];
        while self.at_kw("except") {
            let hl = self.cur().line;
            self.pos += 1;
            if self.at_op("*") {
                self.pos += 1;
            }
            let (typ, name) = if self.at_op(":") {
                (None, None)
            } else {
                let mut typ = self.expression()?;
                if self.at_op(",") {
                    // `except A, B:` is Python 2 syntax.
                    let t = self.cur().clone();
                    let mut items = vec![typ];
                    while self.eat_op(",") {
                        if self.at_op(":") || self.at_kw("as") {
                            break;
                        }
                        items.push(self.expression()?);
                    }
                    if self.at_kw("as") || self.at_op(":") {
                        return Err(SyntaxErr::new(
                            "multiple exception types must be parenthesized",
                            t.line,
                            typ_col(&items[0]),
                        ));
                    }
                    typ = mk(ExprKind::Tuple(items), &t);
                }
                let name = if self.eat_kw("as") {
                    Some(self.name()?)
                } else {
                    None
                };
                (Some(typ), name)
            };
            self.expect_op(":")?;
            let hbody = self.block("except", hl)?;
            handlers.push(Handler {
                typ,
                name,
                body: hbody,
                line: hl,
            });
        }
        let mut orelse = vec![];
        let mut finalbody = vec![];
        if !handlers.is_empty() && self.at_kw("else") {
            let l = self.cur().line;
            self.pos += 1;
            self.expect_op(":")?;
            orelse = self.block("else", l)?;
        }
        if self.at_kw("finally") {
            let l = self.cur().line;
            self.pos += 1;
            self.expect_op(":")?;
            finalbody = self.block("finally", l)?;
        }
        if handlers.is_empty() && finalbody.is_empty() {
            return Err(self.error_here("expected 'except' or 'finally' block"));
        }
        Ok(Stmt {
            kind: StmtKind::Try(body, handlers, orelse, finalbody),
            line,
        })
    }

    fn with_stmt(&mut self, is_async: bool) -> PResult<Stmt> {
        let line = self.cur().line;
        self.pos += 1;
        let mut items = vec![];
        // Parenthesized form: with (a as b, c as d):
        let save = self.pos;
        let mut done = false;
        if self.at_op("(") {
            self.pos += 1;
            let attempt: PResult<Vec<(Expr, Option<Expr>)>> = (|| {
                let mut v = vec![];
                loop {
                    let e = self.expression()?;
                    let t = if self.eat_kw("as") {
                        let t = self.star_target()?;
                        Some(t)
                    } else {
                        None
                    };
                    v.push((e, t));
                    if !self.eat_op(",") || self.at_op(")") {
                        break;
                    }
                }
                if !self.eat_op(")") || !self.at_op(":") {
                    return Err(self.invalid());
                }
                Ok(v)
            })();
            match attempt {
                Ok(v) => {
                    items = v;
                    done = true;
                }
                Err(_) => self.pos = save,
            }
        }
        if !done {
            loop {
                let e = self.expression()?;
                let t = if self.eat_kw("as") {
                    Some(self.star_target()?)
                } else {
                    None
                };
                items.push((e, t));
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        self.expect_op(":")?;
        let body = self.block("with", line)?;
        Ok(Stmt {
            kind: StmtKind::With(items, body, is_async),
            line,
        })
    }

    fn funcdef(&mut self, decorators: Vec<Expr>, is_async: bool) -> PResult<Stmt> {
        let line = self.cur().line;
        self.pos += 1; // def
        let name = self.name()?;
        if !self.at_op("(") {
            return Err(self.error_here("expected '('"));
        }
        self.pos += 1;
        let params = self.params(")", true)?;
        if !self.eat_op(")") {
            return Err(self.invalid());
        }
        let returns = if self.eat_op("->") {
            Some(self.expression()?)
        } else {
            None
        };
        self.expect_op(":")?;
        let body = self.block("def", line)?;
        Ok(Stmt {
            kind: StmtKind::FunctionDef(Rc::new(FuncDef {
                name,
                params,
                body,
                decorators,
                returns,
                is_async,
                is_lambda: false,
                line,
            })),
            line,
        })
    }

    fn params(&mut self, close: &str, annotations: bool) -> PResult<Params> {
        let mut p = Params::default();
        let mut seen_star = false;
        let mut seen_default = false;
        loop {
            if self.at_op(close) {
                break;
            }
            let t = self.cur().clone();
            if self.eat_op("/") {
                if seen_star || !p.posonly.is_empty() || p.args.is_empty() {
                    return Err(SyntaxErr::new(
                        "at least one argument must precede /",
                        t.line,
                        t.col,
                    ));
                }
                p.posonly = std::mem::take(&mut p.args);
            } else if self.eat_op("**") {
                let name = self.name()?;
                let annotation = if annotations && self.eat_op(":") {
                    Some(self.expression()?)
                } else {
                    None
                };
                p.kwarg = Some(Arg {
                    name,
                    annotation,
                    line: t.line,
                });
                self.eat_op(",");
                if !self.at_op(close) {
                    return Err(SyntaxErr::new(
                        "arguments cannot follow var-keyword argument",
                        self.cur().line,
                        self.cur().col,
                    ));
                }
                break;
            } else if self.eat_op("*") {
                if seen_star {
                    return Err(SyntaxErr::new(
                        "* argument may appear only once",
                        t.line,
                        t.col,
                    ));
                }
                seen_star = true;
                if !self.at_op(",") && !self.at_op(close) {
                    let name = self.name()?;
                    let annotation = if annotations && self.eat_op(":") {
                        Some(self.expression()?)
                    } else {
                        None
                    };
                    p.vararg = Some(Arg {
                        name,
                        annotation,
                        line: t.line,
                    });
                } else if self.at_op(close) {
                    return Err(SyntaxErr::new(
                        "named arguments must follow bare *",
                        t.line,
                        t.col,
                    ));
                }
            } else {
                let name = self.name()?;
                let annotation = if annotations && self.eat_op(":") {
                    Some(self.expression()?)
                } else {
                    None
                };
                let arg = Arg {
                    name: name.clone(),
                    annotation,
                    line: t.line,
                };
                let all = p
                    .posonly
                    .iter()
                    .chain(p.args.iter())
                    .chain(p.kwonly.iter())
                    .chain(p.vararg.iter());
                if all.clone().any(|a| a.name == name) {
                    return Err(SyntaxErr::new(
                        format!("duplicate argument '{name}' in function definition"),
                        t.line,
                        t.col,
                    ));
                }
                let default = if self.eat_op("=") {
                    Some(self.expression()?)
                } else {
                    None
                };
                if seen_star {
                    p.kwonly.push(arg);
                    p.kw_defaults.push(default);
                } else {
                    if let Some(d) = default {
                        p.defaults.push(d);
                        seen_default = true;
                    } else if seen_default {
                        return Err(SyntaxErr::new(
                            "non-default argument follows default argument",
                            t.line,
                            t.col,
                        ));
                    }
                    p.args.push(arg);
                }
            }
            if !self.eat_op(",") {
                break;
            }
        }
        Ok(p)
    }

    fn classdef(&mut self, decorators: Vec<Expr>) -> PResult<Stmt> {
        let line = self.cur().line;
        self.pos += 1;
        let name = self.name()?;
        let (mut bases, mut keywords) = (vec![], vec![]);
        if self.eat_op("(") {
            let (a, k) = self.call_args()?;
            bases = a;
            keywords = k;
        }
        self.expect_op(":")?;
        let body = self.block("class", line)?;
        Ok(Stmt {
            kind: StmtKind::ClassDef(Rc::new(ClassDef {
                name,
                bases,
                keywords,
                body,
                decorators,
                line,
            })),
            line,
        })
    }

    fn try_match(&mut self) -> PResult<Option<Stmt>> {
        let save = self.pos;
        let line = self.cur().line;
        // `match` must be followed by an expression and a ':' NEWLINE INDENT 'case'.
        match self.peek_tok(1) {
            Tok::Op(o) if !matches!(*o, "(" | "[" | "{" | "-" | "*" | "~") => return Ok(None),
            Tok::Newline | Tok::Eof => return Ok(None),
            _ => {}
        }
        self.pos += 1;
        let subject = match self.star_named_expressions_subject() {
            Ok(e) => e,
            Err(_) => {
                self.pos = save;
                return Ok(None);
            }
        };
        if !self.at_op(":") || !matches!(self.peek_tok(1), Tok::Newline) {
            self.pos = save;
            return Ok(None);
        }
        self.pos += 2;
        if !self.at(&Tok::Indent) {
            let t = self.cur();
            return Err(SyntaxErr::indent(
                format!("expected an indented block after 'match' statement on line {line}"),
                t.line,
                t.col,
            ));
        }
        self.pos += 1;
        let mut cases = vec![];
        while self.at_kw("case") {
            let cl = self.cur().line;
            self.pos += 1;
            let pattern = self.patterns()?;
            let guard = if self.eat_kw("if") {
                Some(self.named_expr()?)
            } else {
                None
            };
            self.expect_op(":")?;
            let body = self.block("case", cl)?;
            cases.push(MatchCase {
                pattern,
                guard,
                body,
            });
            while self.at(&Tok::Newline) {
                self.pos += 1;
            }
        }
        if cases.is_empty() {
            return Err(self.invalid());
        }
        if self.at(&Tok::Dedent) {
            self.pos += 1;
        } else if !self.at_eof() {
            return Err(self.invalid());
        }
        Ok(Some(Stmt {
            kind: StmtKind::Match(subject, cases),
            line,
        }))
    }

    fn star_named_expressions_subject(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let first = self.named_expr_or_star()?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.at_op(":") {
                break;
            }
            items.push(self.named_expr_or_star()?);
        }
        Ok(mk(ExprKind::Tuple(items), &t))
    }

    fn patterns(&mut self) -> PResult<Pattern> {
        let first = self.as_pattern()?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.at_op(":") || self.at_kw("if") {
                break;
            }
            items.push(self.as_pattern()?);
        }
        Ok(Pattern::Sequence(items))
    }
    fn as_pattern(&mut self) -> PResult<Pattern> {
        let p = self.or_pattern()?;
        if self.eat_kw("as") {
            let n = self.name()?;
            return Ok(Pattern::As(Box::new(p), n));
        }
        Ok(p)
    }
    fn or_pattern(&mut self) -> PResult<Pattern> {
        let first = self.closed_pattern()?;
        if !self.at_op("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat_op("|") {
            alts.push(self.closed_pattern()?);
        }
        Ok(Pattern::Or(alts))
    }
    fn closed_pattern(&mut self) -> PResult<Pattern> {
        let t = self.cur().clone();
        match &t.tok {
            Tok::Op("*") => {
                self.pos += 1;
                let n = self.name()?;
                Ok(Pattern::Star(if n == "_" { None } else { Some(n) }))
            }
            Tok::Op("(") | Tok::Op("[") => {
                let close = if t.tok == Tok::Op("(") { ")" } else { "]" };
                self.pos += 1;
                let mut items = vec![];
                let mut trailing_comma = false;
                while !self.at_op(close) {
                    items.push(self.as_pattern()?);
                    trailing_comma = self.eat_op(",");
                    if !trailing_comma {
                        break;
                    }
                }
                if !self.eat_op(close) {
                    return Err(self.invalid());
                }
                if close == ")" && items.len() == 1 && !trailing_comma {
                    return Ok(items.pop().unwrap());
                }
                Ok(Pattern::Sequence(items))
            }
            Tok::Op("{") => {
                self.pos += 1;
                let (mut keys, mut patterns, mut rest) = (vec![], vec![], None);
                while !self.at_op("}") {
                    if self.eat_op("**") {
                        rest = Some(self.name()?);
                    } else {
                        keys.push(self.pattern_value()?);
                        self.expect_op(":")?;
                        patterns.push(self.as_pattern()?);
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
                if !self.eat_op("}") {
                    return Err(self.invalid());
                }
                Ok(Pattern::Mapping {
                    keys,
                    patterns,
                    rest,
                })
            }
            Tok::Name(n) if n == "None" => {
                self.pos += 1;
                Ok(Pattern::Singleton(Const::None))
            }
            Tok::Name(n) if n == "True" => {
                self.pos += 1;
                Ok(Pattern::Singleton(Const::True))
            }
            Tok::Name(n) if n == "False" => {
                self.pos += 1;
                Ok(Pattern::Singleton(Const::False))
            }
            Tok::Name(n) if !KEYWORDS.contains(&n.as_str()) => {
                // Capture, dotted value, or class pattern.
                let mut e = mk(ExprKind::Name(n.clone()), &t);
                self.pos += 1;
                let mut dotted = false;
                while self.eat_op(".") {
                    let attr = self.name()?;
                    e = mk(ExprKind::Attribute(Box::new(e), attr), &t);
                    dotted = true;
                }
                if self.eat_op("(") {
                    let (mut args, mut kwargs) = (vec![], vec![]);
                    while !self.at_op(")") {
                        if let (Tok::Name(k), Tok::Op("=")) = (self.tok().clone(), self.peek_tok(1))
                        {
                            self.pos += 2;
                            kwargs.push((k, self.as_pattern()?));
                        } else {
                            args.push(self.as_pattern()?);
                        }
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    if !self.eat_op(")") {
                        return Err(self.invalid());
                    }
                    return Ok(Pattern::Class {
                        cls: e,
                        args,
                        kwargs,
                    });
                }
                if dotted {
                    return Ok(Pattern::Value(e));
                }
                Ok(Pattern::Capture(if n == "_" {
                    None
                } else {
                    Some(n.clone())
                }))
            }
            _ => Ok(Pattern::Value(self.pattern_value()?)),
        }
    }
    fn pattern_value(&mut self) -> PResult<Expr> {
        // Literals, negative numbers, complex literals, dotted names.
        self.sum()
    }

    fn small_stmt(&mut self) -> PResult<Stmt> {
        let t = self.cur().clone();
        let line = t.line;
        let s = |kind| Stmt { kind, line };
        if let Tok::Name(n) = &t.tok {
            match n.as_str() {
                "pass" => {
                    self.pos += 1;
                    return Ok(s(StmtKind::Pass));
                }
                "break" => {
                    self.pos += 1;
                    return Ok(s(StmtKind::Break));
                }
                "continue" => {
                    self.pos += 1;
                    return Ok(s(StmtKind::Continue));
                }
                "return" => {
                    self.pos += 1;
                    let v = if self.at(&Tok::Newline) || self.at_op(";") || self.at_eof() {
                        None
                    } else {
                        Some(self.star_expressions()?)
                    };
                    return Ok(s(StmtKind::Return(v)));
                }
                "raise" => {
                    self.pos += 1;
                    let (mut exc, mut cause) = (None, None);
                    if !(self.at(&Tok::Newline) || self.at_op(";") || self.at_eof()) {
                        exc = Some(self.expression()?);
                        if self.eat_kw("from") {
                            cause = Some(self.expression()?);
                        }
                    }
                    return Ok(s(StmtKind::Raise(exc, cause)));
                }
                "global" | "nonlocal" => {
                    self.pos += 1;
                    let mut names = vec![self.name()?];
                    while self.eat_op(",") {
                        names.push(self.name()?);
                    }
                    return Ok(s(if n == "global" {
                        StmtKind::Global(names)
                    } else {
                        StmtKind::Nonlocal(names)
                    }));
                }
                "del" => {
                    self.pos += 1;
                    let mut targets = vec![];
                    loop {
                        let e = self.bitor()?;
                        check_target(&e, true)?;
                        targets.push(e);
                        if !self.eat_op(",") || self.at(&Tok::Newline) {
                            break;
                        }
                    }
                    return Ok(s(StmtKind::Delete(targets)));
                }
                "assert" => {
                    self.pos += 1;
                    let test = self.expression()?;
                    let msg = if self.eat_op(",") {
                        Some(self.expression()?)
                    } else {
                        None
                    };
                    return Ok(s(StmtKind::Assert(test, msg)));
                }
                "import" => {
                    self.pos += 1;
                    let mut names = vec![];
                    loop {
                        let name = self.dotted_name()?;
                        let asname = if self.eat_kw("as") {
                            Some(self.name()?)
                        } else {
                            None
                        };
                        names.push(Alias { name, asname });
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    return Ok(s(StmtKind::Import(names)));
                }
                "from" => {
                    self.pos += 1;
                    let mut level = 0;
                    loop {
                        if self.eat_op(".") {
                            level += 1;
                        } else if self.eat_op("...") {
                            level += 3;
                        } else {
                            break;
                        }
                    }
                    let module = if self.at_kw("import") {
                        None
                    } else {
                        Some(self.dotted_name()?)
                    };
                    if !self.eat_kw("import") {
                        return Err(self.invalid());
                    }
                    let mut names = vec![];
                    if self.eat_op("*") {
                        names.push(Alias {
                            name: "*".into(),
                            asname: None,
                        });
                    } else {
                        let paren = self.eat_op("(");
                        loop {
                            if paren && self.at_op(")") {
                                break;
                            }
                            let name = self.name()?;
                            let asname = if self.eat_kw("as") {
                                Some(self.name()?)
                            } else {
                                None
                            };
                            names.push(Alias { name, asname });
                            if !self.eat_op(",") {
                                break;
                            }
                        }
                        if paren && !self.eat_op(")") {
                            return Err(self.invalid());
                        }
                        if !paren && names.is_empty() {
                            return Err(self.invalid());
                        }
                    }
                    return Ok(s(StmtKind::ImportFrom(module, names, level)));
                }
                "print"
                    // Python 2 print statement gets CPython's targeted hint.
                    if matches!(self.peek_tok(1), Tok::Str(_) | Tok::Int(..) | Tok::Name(_))
                        && !matches!(self.peek_tok(1), Tok::Name(k) if KEYWORDS.contains(&k.as_str()) && k != "None" && k != "True" && k != "False")
                    => {
                        let mut e = SyntaxErr::new(
                            "Missing parentheses in call to 'print'. Did you mean print(...)?",
                            t.line,
                            t.col,
                        );
                        e.end_col = t.col + 1;
                        return Err(e);
                    }
                _ => {}
            }
        }
        // Expression, assignment, augmented assignment, annotated assignment.
        let first = if self.at_kw("yield") {
            self.yield_expr()?
        } else {
            self.star_expressions()?
        };
        if self.at_op("=") {
            let mut targets = vec![first];
            let mut value;
            loop {
                let eq = self.cur().clone();
                self.pos += 1;
                value = if self.at_kw("yield") {
                    self.yield_expr()?
                } else {
                    self.star_expressions()?
                };
                let _ = eq;
                if !self.at_op("=") {
                    break;
                }
                targets.push(value);
            }
            for t in &targets {
                check_target(t, false)?;
            }
            return Ok(s(StmtKind::Assign(targets, value)));
        }
        if let Tok::Op(op) = self.tok().clone() {
            let aug = match op {
                "+=" => Some(BinOp::Add),
                "-=" => Some(BinOp::Sub),
                "*=" => Some(BinOp::Mul),
                "/=" => Some(BinOp::Div),
                "//=" => Some(BinOp::FloorDiv),
                "%=" => Some(BinOp::Mod),
                "**=" => Some(BinOp::Pow),
                "<<=" => Some(BinOp::LShift),
                ">>=" => Some(BinOp::RShift),
                "|=" => Some(BinOp::BitOr),
                "^=" => Some(BinOp::BitXor),
                "&=" => Some(BinOp::BitAnd),
                "@=" => Some(BinOp::MatMul),
                _ => None,
            };
            if let Some(bop) = aug {
                match &first.kind {
                    ExprKind::Name(_) | ExprKind::Attribute(..) | ExprKind::Subscript(..) => {}
                    _ => {
                        return Err(SyntaxErr::new(
                            format!(
                                "'{}' is an illegal expression for augmented assignment",
                                expr_desc(&first)
                            ),
                            first.line,
                            first.col,
                        ))
                    }
                }
                self.pos += 1;
                let value = if self.at_kw("yield") {
                    self.yield_expr()?
                } else {
                    self.star_expressions()?
                };
                return Ok(s(StmtKind::AugAssign(first, bop, value)));
            }
            if op == ":" {
                self.pos += 1;
                let simple = matches!(first.kind, ExprKind::Name(_));
                check_target(&first, false)?;
                let ann = self.expression()?;
                let value = if self.eat_op("=") {
                    Some(self.star_expressions()?)
                } else {
                    None
                };
                return Ok(s(StmtKind::AnnAssign(first, ann, value, simple)));
            }
        }
        if !(self.at(&Tok::Newline) || self.at_op(";") || self.at_eof()) {
            // Two expressions side by side: CPython suggests a missing comma.
            let t2 = self.cur().clone();
            if t2.line == first.line
                && matches!(
                    t2.tok,
                    Tok::Name(_) | Tok::Int(..) | Tok::Str(_) | Tok::Float(_)
                )
                && !matches!(&t2.tok, Tok::Name(k) if KEYWORDS.contains(&k.as_str()))
                && matches!(
                    first.kind,
                    ExprKind::Name(_) | ExprKind::Const(_) | ExprKind::Attribute(..)
                )
            {
                return Err(SyntaxErr::new(
                    "invalid syntax. Perhaps you forgot a comma?",
                    first.line,
                    first.col,
                ));
            }
            if let (Tok::Op("="), true) = (&t2.tok, false) {
                unreachable!()
            }
            return Err(self.invalid());
        }
        Ok(s(StmtKind::Expr(first)))
    }

    fn dotted_name(&mut self) -> PResult<String> {
        let mut n = self.name()?;
        while self.eat_op(".") {
            n.push('.');
            n.push_str(&self.name()?);
        }
        Ok(n)
    }

    // ---------- expressions ----------

    fn yield_expr(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        self.pos += 1;
        if self.eat_kw("from") {
            let e = self.expression()?;
            return Ok(mk(ExprKind::YieldFrom(Box::new(e)), &t));
        }
        if self.at(&Tok::Newline)
            || self.at_op(")")
            || self.at_op("]")
            || self.at_op("}")
            || self.at_op("=")
            || self.at_op(";")
            || self.at_eof()
        {
            return Ok(mk(ExprKind::Yield(None), &t));
        }
        let e = self.star_expressions()?;
        Ok(mk(ExprKind::Yield(Some(Box::new(e))), &t))
    }

    /// Comma-separated expressions forming a tuple when there is a comma.
    pub fn star_expressions(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let first = self.star_expression()?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.expr_ends() {
                break;
            }
            items.push(self.star_expression()?);
        }
        Ok(mk(ExprKind::Tuple(items), &t))
    }
    fn expr_ends(&self) -> bool {
        match self.tok() {
            Tok::Newline | Tok::Eof => true,
            Tok::Op(o) => matches!(
                *o,
                "=" | ")"
                    | "]"
                    | "}"
                    | ";"
                    | ":"
                    | "+="
                    | "-="
                    | "*="
                    | "/="
                    | "//="
                    | "%="
                    | "**="
                    | ">>="
                    | "<<="
                    | "|="
                    | "^="
                    | "&="
                    | "@="
            ),
            Tok::Name(n) => n == "in" || n == "for" || n == "if" || n == "else",
            _ => false,
        }
    }
    fn star_expression(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        if self.eat_op("*") {
            let e = self.bitor()?;
            return Ok(mk(ExprKind::Starred(Box::new(e)), &t));
        }
        self.expression()
    }
    fn named_expr_or_star(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        if self.eat_op("*") {
            let e = self.bitor()?;
            return Ok(mk(ExprKind::Starred(Box::new(e)), &t));
        }
        self.named_expr()
    }
    pub fn named_expr(&mut self) -> PResult<Expr> {
        if let (Tok::Name(n), Tok::Op(":=")) = (self.tok().clone(), self.peek_tok(1)) {
            let t = self.cur().clone();
            self.pos += 2;
            let v = self.expression()?;
            return Ok(mk(
                ExprKind::NamedExpr(Box::new(mk(ExprKind::Name(n), &t)), Box::new(v)),
                &t,
            ));
        }
        let e = self.expression()?;
        if self.at_op(":=") {
            return Err(SyntaxErr::new(
                format!("cannot use assignment expressions with {}", expr_desc(&e)),
                e.line,
                e.col,
            ));
        }
        Ok(e)
    }

    pub fn expression(&mut self) -> PResult<Expr> {
        self.depth += 1;
        if self.depth > 200 {
            let t = self.cur();
            return Err(SyntaxErr::new("too many nested parentheses", t.line, t.col));
        }
        let r = self.expression_inner();
        self.depth -= 1;
        r
    }
    fn expression_inner(&mut self) -> PResult<Expr> {
        if self.at_kw("lambda") {
            return self.lambda();
        }
        let t = self.cur().clone();
        let body = self.disjunction()?;
        if self.at_kw("if") {
            self.pos += 1;
            let cond = self.disjunction()?;
            if !self.eat_kw("else") {
                return Err(self.error_here("expected 'else' after 'if' expression"));
            }
            let orelse = self.expression()?;
            let mut e = mk(
                ExprKind::IfExp(Box::new(cond), Box::new(body), Box::new(orelse)),
                &t,
            );
            e.end_col = self.prev_end().1;
            return Ok(e);
        }
        Ok(body)
    }
    fn lambda(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        self.pos += 1;
        let params = self.params(":", false)?;
        if !self.eat_op(":") {
            return Err(self.invalid());
        }
        let body = self.expression()?;
        let line = t.line;
        let def = FuncDef {
            name: "<lambda>".into(),
            params,
            body: vec![Stmt {
                kind: StmtKind::Return(Some(body)),
                line,
            }],
            decorators: vec![],
            returns: None,
            is_async: false,
            is_lambda: true,
            line,
        };
        Ok(mk(ExprKind::Lambda(Rc::new(def)), &t))
    }
    fn disjunction(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let first = self.conjunction()?;
        if !self.at_kw("or") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_kw("or") {
            items.push(self.conjunction()?);
        }
        let mut e = mk(ExprKind::BoolOp(false, items), &t);
        e.end_col = self.prev_end().1;
        Ok(e)
    }
    fn conjunction(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let first = self.inversion()?;
        if !self.at_kw("and") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_kw("and") {
            items.push(self.inversion()?);
        }
        let mut e = mk(ExprKind::BoolOp(true, items), &t);
        e.end_col = self.prev_end().1;
        Ok(e)
    }
    fn inversion(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        if self.eat_kw("not") {
            let e = self.inversion()?;
            let mut r = mk(ExprKind::UnaryOp(UnaryOp::Not, Box::new(e)), &t);
            r.end_col = self.prev_end().1;
            return Ok(r);
        }
        self.comparison()
    }
    fn comparison(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let left = self.bitor()?;
        let mut ops = vec![];
        loop {
            let op = match self.tok() {
                Tok::Op("==") => CmpOp::Eq,
                Tok::Op("!=") => CmpOp::NotEq,
                Tok::Op("<") => CmpOp::Lt,
                Tok::Op("<=") => CmpOp::LtE,
                Tok::Op(">") => CmpOp::Gt,
                Tok::Op(">=") => CmpOp::GtE,
                Tok::Name(n) if n == "in" => CmpOp::In,
                Tok::Name(n) if n == "is" => {
                    if matches!(self.peek_tok(1), Tok::Name(m) if m == "not") {
                        self.pos += 1;
                        CmpOp::IsNot
                    } else {
                        CmpOp::Is
                    }
                }
                Tok::Name(n) if n == "not" => {
                    if matches!(self.peek_tok(1), Tok::Name(m) if m == "in") {
                        self.pos += 1;
                        CmpOp::NotIn
                    } else {
                        break;
                    }
                }
                _ => break,
            };
            self.pos += 1;
            ops.push((op, self.bitor()?));
        }
        if ops.is_empty() {
            return Ok(left);
        }
        let mut e = mk(ExprKind::Compare(Box::new(left), ops), &t);
        e.end_col = self.prev_end().1;
        Ok(e)
    }
    fn binary_level(
        &mut self,
        ops: &[(&str, BinOp)],
        next: fn(&mut Self) -> PResult<Expr>,
    ) -> PResult<Expr> {
        let t = self.cur().clone();
        let mut left = next(self)?;
        'outer: loop {
            for (sym, op) in ops {
                if self.at_op(sym) {
                    self.pos += 1;
                    let right = next(self)?;
                    let mut e = mk(ExprKind::BinOp(*op, Box::new(left), Box::new(right)), &t);
                    e.end_col = self.prev_end().1;
                    left = e;
                    continue 'outer;
                }
            }
            break;
        }
        Ok(left)
    }
    fn bitor(&mut self) -> PResult<Expr> {
        self.binary_level(&[("|", BinOp::BitOr)], Self::bitxor)
    }
    fn bitxor(&mut self) -> PResult<Expr> {
        self.binary_level(&[("^", BinOp::BitXor)], Self::bitand)
    }
    fn bitand(&mut self) -> PResult<Expr> {
        self.binary_level(&[("&", BinOp::BitAnd)], Self::shift)
    }
    fn shift(&mut self) -> PResult<Expr> {
        self.binary_level(&[("<<", BinOp::LShift), (">>", BinOp::RShift)], Self::sum)
    }
    fn sum(&mut self) -> PResult<Expr> {
        self.binary_level(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::term)
    }
    fn term(&mut self) -> PResult<Expr> {
        self.binary_level(
            &[
                ("*", BinOp::Mul),
                ("/", BinOp::Div),
                ("//", BinOp::FloorDiv),
                ("%", BinOp::Mod),
                ("@", BinOp::MatMul),
            ],
            Self::factor,
        )
    }
    fn factor(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let op = match self.tok() {
            Tok::Op("-") => Some(UnaryOp::Neg),
            Tok::Op("+") => Some(UnaryOp::Pos),
            Tok::Op("~") => Some(UnaryOp::Invert),
            _ => None,
        };
        if let Some(op) = op {
            self.pos += 1;
            self.depth += 1;
            if self.depth > 200 {
                return Err(SyntaxErr::new("too many nested parentheses", t.line, t.col));
            }
            let e = self.factor();
            self.depth -= 1;
            let e = e?;
            // Fold negative numeric literals so -5 is a constant.
            if op == UnaryOp::Neg {
                if let ExprKind::Const(Const::Int(ref d, r)) = e.kind {
                    if !d.starts_with('-') {
                        let mut r2 = mk(ExprKind::Const(Const::Int(format!("-{d}"), r)), &t);
                        r2.end_col = e.end_col;
                        return Ok(r2);
                    }
                }
                if let ExprKind::Const(Const::Float(f)) = e.kind {
                    let mut r2 = mk(ExprKind::Const(Const::Float(-f)), &t);
                    r2.end_col = e.end_col;
                    return Ok(r2);
                }
            }
            let mut r = mk(ExprKind::UnaryOp(op, Box::new(e)), &t);
            r.end_col = self.prev_end().1;
            return Ok(r);
        }
        self.power()
    }
    fn power(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let base = if self.at_kw("await") {
            self.pos += 1;
            let e = self.primary()?;
            mk(ExprKind::Await(Box::new(e)), &t)
        } else {
            self.primary()?
        };
        if self.eat_op("**") {
            let exp = self.factor()?;
            let mut e = mk(
                ExprKind::BinOp(BinOp::Pow, Box::new(base), Box::new(exp)),
                &t,
            );
            e.end_col = self.prev_end().1;
            return Ok(e);
        }
        Ok(base)
    }
    fn primary(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let mut e = self.atom()?;
        loop {
            if self.at_op(".") {
                self.pos += 1;
                let name = match self.tok().clone() {
                    Tok::Name(n) => {
                        self.pos += 1;
                        n
                    }
                    _ => return Err(self.invalid()),
                };
                let mut n = mk(ExprKind::Attribute(Box::new(e), name), &t);
                n.end_col = self.prev_end().1;
                e = n;
            } else if self.at_op("(") {
                self.pos += 1;
                let (args, keywords) = self.call_args()?;
                let mut n = mk(
                    ExprKind::Call {
                        func: Box::new(e),
                        args,
                        keywords,
                    },
                    &t,
                );
                n.end_col = self.prev_end().1;
                e = n;
            } else if self.at_op("[") {
                self.pos += 1;
                let idx = self.subscript()?;
                if !self.eat_op("]") {
                    return Err(self.invalid());
                }
                let mut n = mk(ExprKind::Subscript(Box::new(e), Box::new(idx)), &t);
                n.end_col = self.prev_end().1;
                e = n;
            } else {
                break;
            }
        }
        Ok(e)
    }
    /// Parses arguments after `(` through the closing `)`.
    fn call_args(&mut self) -> PResult<(Vec<Expr>, Vec<Keyword>)> {
        let mut args = vec![];
        let mut keywords: Vec<Keyword> = vec![];
        loop {
            if self.at_op(")") {
                break;
            }
            let t = self.cur().clone();
            if self.eat_op("**") {
                let v = self.expression()?;
                keywords.push(Keyword {
                    name: None,
                    value: v,
                });
            } else if self.eat_op("*") {
                let v = self.expression()?;
                args.push(mk(ExprKind::Starred(Box::new(v)), &t));
            } else if let (Tok::Name(n), Tok::Op("=")) = (self.tok().clone(), self.peek_tok(1)) {
                self.pos += 2;
                let v = self.expression()?;
                if keywords
                    .iter()
                    .any(|k| k.name.as_deref() == Some(n.as_str()))
                {
                    return Err(SyntaxErr::new(
                        format!("keyword argument repeated: {n}"),
                        t.line,
                        t.col,
                    ));
                }
                keywords.push(Keyword {
                    name: Some(n),
                    value: v,
                });
            } else {
                let v = self.named_expr()?;
                if self.at_kw("for") || self.at_kw("async") {
                    let gens = self.comp_for()?;
                    let g = mk(ExprKind::GenExp(Box::new(v), gens), &t);
                    args.push(g);
                } else {
                    if !keywords.is_empty() && keywords.iter().any(|k| k.name.is_some()) {
                        return Err(SyntaxErr::new(
                            "positional argument follows keyword argument",
                            v.line,
                            v.col,
                        ));
                    }
                    if self.at_op("=") {
                        return Err(SyntaxErr::new(
                            "expression cannot contain assignment, perhaps you meant \"==\"?",
                            v.line,
                            v.col,
                        ));
                    }
                    args.push(v);
                }
            }
            if !self.eat_op(",") {
                break;
            }
        }
        if !self.eat_op(")") {
            if self.at(&Tok::Newline) || self.at_eof() {
                return Err(self.invalid());
            }
            let t = self.cur().clone();
            if matches!(t.tok, Tok::Name(_) | Tok::Int(..) | Tok::Str(_)) {
                if let Some(prev) = args.last() {
                    return Err(SyntaxErr::new(
                        "invalid syntax. Perhaps you forgot a comma?",
                        prev.line,
                        prev.col,
                    ));
                }
            }
            return Err(self.invalid());
        }
        Ok((args, keywords))
    }
    fn subscript(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let first = self.slice_item()?;
        if !self.at_op(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.at_op("]") {
                break;
            }
            items.push(self.slice_item()?);
        }
        Ok(mk(ExprKind::Tuple(items), &t))
    }
    fn slice_item(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let lower = if self.at_op(":") {
            None
        } else if self.at_op("*") {
            return self.star_expression();
        } else {
            let e = self.named_expr()?;
            if !self.at_op(":") {
                return Ok(e);
            }
            Some(Box::new(e))
        };
        self.expect_op(":")?;
        let upper = if self.at_op(":") || self.at_op("]") || self.at_op(",") {
            None
        } else {
            Some(Box::new(self.expression()?))
        };
        let step = if self.eat_op(":") {
            if self.at_op("]") || self.at_op(",") {
                None
            } else {
                Some(Box::new(self.expression()?))
            }
        } else {
            None
        };
        Ok(mk(ExprKind::Slice(lower, upper, step), &t))
    }
    fn comp_for(&mut self) -> PResult<Vec<Comprehension>> {
        let mut gens = vec![];
        loop {
            let is_async = self.eat_kw("async");
            if !self.eat_kw("for") {
                break;
            }
            let target = self.target_list()?;
            if !self.eat_kw("in") {
                return Err(self.invalid());
            }
            let iter = self.disjunction()?;
            let mut ifs = vec![];
            while self.at_kw("if") {
                self.pos += 1;
                ifs.push(self.disjunction()?);
            }
            gens.push(Comprehension {
                target,
                iter,
                ifs,
                is_async,
            });
            if !self.at_kw("for") && !self.at_kw("async") {
                break;
            }
        }
        Ok(gens)
    }
    fn atom(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        match &t.tok {
            Tok::Name(n) => {
                let kind = match n.as_str() {
                    "None" => ExprKind::Const(Const::None),
                    "True" => ExprKind::Const(Const::True),
                    "False" => ExprKind::Const(Const::False),
                    _ if KEYWORDS.contains(&n.as_str()) => {
                        return Err(self.invalid());
                    }
                    _ => ExprKind::Name(n.clone()),
                };
                self.pos += 1;
                Ok(mk(kind, &t))
            }
            Tok::Int(d, r) => {
                self.pos += 1;
                Ok(mk(ExprKind::Const(Const::Int(d.clone(), *r)), &t))
            }
            Tok::Float(f) => {
                self.pos += 1;
                Ok(mk(ExprKind::Const(Const::Float(*f)), &t))
            }
            Tok::Imag(f) => {
                self.pos += 1;
                Ok(mk(ExprKind::Const(Const::Imag(*f)), &t))
            }
            Tok::Str(_) | Tok::Bytes(_) | Tok::FStr(_) => self.strings(),
            Tok::Op("...") => {
                self.pos += 1;
                Ok(mk(ExprKind::Const(Const::Ellipsis), &t))
            }
            Tok::Op("(") => {
                self.pos += 1;
                if self.eat_op(")") {
                    let mut e = mk(ExprKind::Tuple(vec![]), &t);
                    e.end_col = self.prev_end().1;
                    return Ok(e);
                }
                if self.at_kw("yield") {
                    let e = self.yield_expr()?;
                    if !self.eat_op(")") {
                        return Err(self.invalid());
                    }
                    return Ok(e);
                }
                let first = self.named_expr_or_star()?;
                if self.at_kw("for") || self.at_kw("async") {
                    let gens = self.comp_for()?;
                    if !self.eat_op(")") {
                        return Err(self.invalid());
                    }
                    return Ok(mk(ExprKind::GenExp(Box::new(first), gens), &t));
                }
                if self.eat_op(")") {
                    if matches!(first.kind, ExprKind::Starred(_)) {
                        return Err(SyntaxErr::new(
                            "cannot use starred expression here",
                            first.line,
                            first.col,
                        ));
                    }
                    // Parenthesized expression keeps its inner position.
                    return Ok(first);
                }
                if !self.at_op(",") {
                    return Err(self.paren_error(&first));
                }
                let mut items = vec![first];
                while self.eat_op(",") {
                    if self.at_op(")") {
                        break;
                    }
                    items.push(self.named_expr_or_star()?);
                }
                if !self.eat_op(")") {
                    return Err(self.paren_error(items.last().unwrap()));
                }
                let mut e = mk(ExprKind::Tuple(items), &t);
                e.end_col = self.prev_end().1;
                Ok(e)
            }
            Tok::Op("[") => {
                self.pos += 1;
                let mut items = vec![];
                if self.eat_op("]") {
                    return Ok(mk(ExprKind::List(items), &t));
                }
                let first = self.named_expr_or_star()?;
                if self.at_kw("for") || self.at_kw("async") {
                    let gens = self.comp_for()?;
                    if !self.eat_op("]") {
                        return Err(self.invalid());
                    }
                    return Ok(mk(ExprKind::ListComp(Box::new(first), gens), &t));
                }
                items.push(first);
                while self.eat_op(",") {
                    if self.at_op("]") {
                        break;
                    }
                    items.push(self.named_expr_or_star()?);
                }
                if !self.eat_op("]") {
                    return Err(self.paren_error(items.last().unwrap()));
                }
                let mut e = mk(ExprKind::List(items), &t);
                e.end_col = self.prev_end().1;
                Ok(e)
            }
            Tok::Op("{") => {
                self.pos += 1;
                if self.eat_op("}") {
                    return Ok(mk(ExprKind::Dict(vec![]), &t));
                }
                if self.eat_op("**") {
                    let v = self.bitor()?;
                    return self.dict_rest(t, vec![(None, v)]);
                }
                let first = self.named_expr_or_star()?;
                if self.eat_op(":") {
                    let v = self.expression()?;
                    if self.at_kw("for") || self.at_kw("async") {
                        let gens = self.comp_for()?;
                        if !self.eat_op("}") {
                            return Err(self.invalid());
                        }
                        return Ok(mk(
                            ExprKind::DictComp(Box::new(first), Box::new(v), gens),
                            &t,
                        ));
                    }
                    return self.dict_rest(t, vec![(Some(first), v)]);
                }
                if self.at_kw("for") || self.at_kw("async") {
                    let gens = self.comp_for()?;
                    if !self.eat_op("}") {
                        return Err(self.invalid());
                    }
                    return Ok(mk(ExprKind::SetComp(Box::new(first), gens), &t));
                }
                let mut items = vec![first];
                while self.eat_op(",") {
                    if self.at_op("}") {
                        break;
                    }
                    items.push(self.named_expr_or_star()?);
                }
                if !self.eat_op("}") {
                    return Err(self.paren_error(items.last().unwrap()));
                }
                Ok(mk(ExprKind::Set(items), &t))
            }
            Tok::Op("*") => Err(SyntaxErr::new(
                "cannot use starred expression here",
                t.line,
                t.col,
            )),
            Tok::Indent => Err(SyntaxErr::indent("unexpected indent", t.line, t.col)),
            _ => Err(self.invalid()),
        }
    }
    fn paren_error(&self, last: &Expr) -> SyntaxErr {
        let t = self.cur();
        if matches!(
            t.tok,
            Tok::Name(_) | Tok::Int(..) | Tok::Str(_) | Tok::Float(_)
        ) && !matches!(&t.tok, Tok::Name(k) if KEYWORDS.contains(&k.as_str()))
        {
            return SyntaxErr::new(
                "invalid syntax. Perhaps you forgot a comma?",
                last.line,
                last.col,
            );
        }
        self.invalid()
    }
    fn dict_rest(&mut self, t: Token, mut items: Vec<(Option<Expr>, Expr)>) -> PResult<Expr> {
        while self.eat_op(",") {
            if self.at_op("}") {
                break;
            }
            if self.eat_op("**") {
                let v = self.bitor()?;
                items.push((None, v));
            } else {
                let k = self.expression()?;
                self.expect_op(":")?;
                let v = self.expression()?;
                items.push((Some(k), v));
            }
        }
        if !self.eat_op("}") {
            return Err(self.invalid());
        }
        Ok(mk(ExprKind::Dict(items), &t))
    }
    fn strings(&mut self) -> PResult<Expr> {
        let t = self.cur().clone();
        let mut parts: Vec<FStrPart> = vec![];
        let mut bytes: Option<Vec<u8>> = None;
        let mut is_f = false;
        let mut any_str = false;
        loop {
            match self.tok().clone() {
                Tok::Str(s) => {
                    any_str = true;
                    push_lit(&mut parts, s);
                }
                Tok::Bytes(b) => bytes.get_or_insert_with(Vec::new).extend(b),
                Tok::FStr(fp) => {
                    is_f = true;
                    any_str = true;
                    for part in fp {
                        match part {
                            FPart::Lit(s) => push_lit(&mut parts, s),
                            other => parts.extend(self.fpart(other)?),
                        }
                    }
                }
                _ => break,
            }
            self.pos += 1;
        }
        if bytes.is_some() && any_str {
            return Err(SyntaxErr::new(
                "cannot mix bytes and nonbytes literals",
                t.line,
                t.col,
            ));
        }
        let mut e = if let Some(b) = bytes {
            mk(ExprKind::Const(Const::Bytes(b)), &t)
        } else if !is_f {
            let s = match parts.pop() {
                Some(FStrPart::Lit(s)) => s,
                _ => String::new(),
            };
            mk(ExprKind::Const(Const::Str(s)), &t)
        } else {
            mk(ExprKind::JoinedStr(parts), &t)
        };
        e.end_col = self.prev_end().1;
        Ok(e)
    }
    fn fpart(&mut self, part: FPart) -> PResult<Vec<FStrPart>> {
        let FPart::Expr {
            src,
            line,
            col,
            conversion,
            spec,
            debug,
        } = part
        else {
            unreachable!()
        };
        let toks = lexer::tokenize_expr(&src, line, col)?;
        let mut sub = Parser {
            toks,
            pos: 0,
            depth: self.depth + 1,
        };
        if sub.depth > 50 {
            return Err(SyntaxErr::new(
                "f-string: expressions nested too deeply",
                line,
                col,
            ));
        }
        // The expression was wrapped in parentheses so it can span lines.
        sub.pos = 1;
        let value = if sub.at_kw("yield") {
            sub.yield_expr()?
        } else {
            sub.star_expressions()?
        };
        if !sub.at_op(")") {
            return Err(SyntaxErr::new("f-string: invalid syntax", line, col));
        }
        let spec = match spec {
            None => None,
            Some(sp) => {
                let mut v = vec![];
                for p in sp {
                    match p {
                        FPart::Lit(s) => push_lit(&mut v, s),
                        other => v.extend(self.fpart(other)?),
                    }
                }
                Some(v)
            }
        };
        let mut out = vec![];
        if let Some(d) = debug {
            out.push(FStrPart::Lit(d));
        }
        out.push(FStrPart::Expr {
            value: Box::new(value),
            conversion,
            spec,
        });
        Ok(out)
    }
}

fn push_lit(parts: &mut Vec<FStrPart>, s: String) {
    if let Some(FStrPart::Lit(prev)) = parts.last_mut() {
        prev.push_str(&s);
    } else {
        parts.push(FStrPart::Lit(s));
    }
}

fn typ_col(e: &Expr) -> u32 {
    e.col
}

pub fn expr_desc(e: &Expr) -> &'static str {
    match &e.kind {
        ExprKind::Call { .. } => "function call",
        ExprKind::Const(Const::None) => "None",
        ExprKind::Const(Const::True) => "True",
        ExprKind::Const(Const::False) => "False",
        ExprKind::Const(Const::Ellipsis) => "ellipsis",
        ExprKind::Const(_) | ExprKind::JoinedStr(_) => "literal",
        ExprKind::BinOp(..) | ExprKind::UnaryOp(..) => "expression",
        ExprKind::BoolOp(..) => "expression",
        ExprKind::Compare(..) => "comparison",
        ExprKind::Lambda(_) => "lambda",
        ExprKind::IfExp(..) => "conditional expression",
        ExprKind::ListComp(..) => "list comprehension",
        ExprKind::SetComp(..) => "set comprehension",
        ExprKind::DictComp(..) => "dict comprehension",
        ExprKind::GenExp(..) => "generator expression",
        ExprKind::Dict(_) => "dict literal",
        ExprKind::Set(_) => "set display",
        ExprKind::Yield(_) | ExprKind::YieldFrom(_) => "yield expression",
        ExprKind::Await(_) => "await expression",
        ExprKind::NamedExpr(..) => "named expression",
        ExprKind::Attribute(..) => "attribute",
        ExprKind::Subscript(..) => "subscript",
        ExprKind::Starred(_) => "starred",
        ExprKind::Name(_) => "name",
        ExprKind::List(_) => "list",
        ExprKind::Tuple(_) => "tuple",
        ExprKind::Slice(..) => "slice",
    }
}

fn check_target(e: &Expr, del: bool) -> PResult<()> {
    match &e.kind {
        ExprKind::Name(_) | ExprKind::Attribute(..) | ExprKind::Subscript(..) => Ok(()),
        ExprKind::Tuple(items) | ExprKind::List(items) => {
            let mut stars = 0;
            for i in items {
                if matches!(i.kind, ExprKind::Starred(_)) {
                    stars += 1;
                }
                check_target(i, del)?;
            }
            if stars > 1 {
                return Err(SyntaxErr::new(
                    "multiple starred expressions in assignment",
                    e.line,
                    e.col,
                ));
            }
            Ok(())
        }
        ExprKind::Starred(inner) if !del => check_target(inner, del),
        _ => {
            let what = expr_desc(e);
            let verb = if del { "delete" } else { "assign to" };
            let msg = match (&e.kind, del) {
                (ExprKind::Call { .. }, false)
                | (ExprKind::Const(_), false)
                | (ExprKind::Compare(..), false)
                | (ExprKind::BinOp(..), false) => {
                    format!("cannot {verb} {what} here. Maybe you meant '==' instead of '='?")
                }
                _ => format!("cannot {verb} {what}"),
            };
            Err(SyntaxErr::new(msg, e.line, e.col))
        }
    }
}
