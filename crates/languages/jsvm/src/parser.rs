//! Recursive-descent parser for ECMAScript 2022 scripts and modules, with
//! V8-compatible syntax error messages.

use crate::ast::*;
use crate::lexer::{SyntaxErr, Tok, Token};
use std::rc::Rc;

const KEYWORDS: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "enum",
];
const STRICT_RESERVED: &[&str] = &[
    "implements",
    "interface",
    "let",
    "package",
    "private",
    "protected",
    "public",
    "static",
    "yield",
];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

type PResult<T> = Result<T, SyntaxErr>;

#[derive(Clone, Copy, Default)]
struct Ctx {
    in_function: bool,
    is_async: bool,
    is_generator: bool,
    strict: bool,
    super_call: bool,
    super_prop: bool,
    /// Inside a class field initialiser (no `arguments`).
    in_class_field: bool,
    loop_depth: u32,
    switch_depth: u32,
}

pub struct Parser<'a> {
    toks: Vec<Token>,
    i: usize,
    _src: &'a [char],
    ctx: Ctx,
    labels: Vec<(Name, bool)>,
    is_module: bool,
    /// Top-level await / import / export seen (module syntax detection).
    pub saw_module_syntax: bool,
    /// Allow `await` at top level (module goal).
    top_level_await: bool,
}

fn pos_of(t: &Token) -> Pos {
    Pos {
        line: t.line,
        col: t.col,
    }
}

/// Cooked strings, raw strings and substitutions of a template literal.
type TemplateParts = (Vec<Option<Rc<str>>>, Vec<Rc<str>>, Vec<Expr>);

impl<'a> Parser<'a> {
    pub fn new(toks: Vec<Token>, src: &'a [char], is_module: bool) -> Self {
        Parser {
            toks,
            i: 0,
            _src: src,
            ctx: Ctx {
                strict: is_module,
                ..Ctx::default()
            },
            labels: vec![],
            is_module,
            saw_module_syntax: false,
            top_level_await: is_module,
        }
    }

    // ----- token helpers -----
    fn peek(&self) -> &Token {
        &self.toks[self.i]
    }
    fn peek_at(&self, n: usize) -> &Token {
        let j = (self.i + n).min(self.toks.len() - 1);
        &self.toks[j]
    }
    fn tok(&self) -> &Tok {
        &self.toks[self.i].tok
    }
    fn advance(&mut self) -> Token {
        let t = self.toks[self.i].clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn pos(&self) -> Pos {
        pos_of(self.peek())
    }
    fn prev_end(&self) -> usize {
        if self.i == 0 {
            0
        } else {
            self.toks[self.i - 1].end
        }
    }
    fn is_punct(&self, p: &str) -> bool {
        matches!(self.tok(), Tok::Punct(q) if *q == p)
    }
    fn is_punct_at(&self, n: usize, p: &str) -> bool {
        matches!(&self.peek_at(n).tok, Tok::Punct(q) if *q == p)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is_punct(p) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, p: &str) -> PResult<()> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.tok(), Tok::Ident(s) if s == k)
    }
    fn is_kw_at(&self, n: usize, k: &str) -> bool {
        matches!(&self.peek_at(n).tok, Tok::Ident(s) if s == k)
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn expect_kw(&mut self, k: &str) -> PResult<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    pub fn err_at(&self, t: &Token, msg: impl Into<String>) -> SyntaxErr {
        let len = if matches!(t.tok, Tok::Eof) {
            0
        } else if t.line == t.end_line {
            t.end_col.saturating_sub(t.col).max(1)
        } else {
            1
        };
        SyntaxErr {
            msg: msg.into(),
            line: t.line,
            col: t.col,
            len,
        }
    }
    /// Error spanning from `start` to the end of the previous token.
    fn span_err(&self, start: &Token, msg: impl Into<String>) -> SyntaxErr {
        let end = self.toks[self.i.saturating_sub(1)].clone();
        let len = if end.end_line == start.line && end.end_col > start.col {
            end.end_col - start.col
        } else {
            1
        };
        SyntaxErr {
            msg: msg.into(),
            line: start.line,
            col: start.col,
            len,
        }
    }

    fn err_here(&self, msg: impl Into<String>) -> SyntaxErr {
        self.err_at(self.peek(), msg)
    }
    fn err_pos(&self, p: Pos, len: u32, msg: impl Into<String>) -> SyntaxErr {
        SyntaxErr {
            msg: msg.into(),
            line: p.line,
            col: p.col,
            len,
        }
    }

    fn unexpected(&self) -> SyntaxErr {
        let t = self.peek();
        let msg = match &t.tok {
            Tok::Eof => "Unexpected end of input".to_string(),
            Tok::Punct(p) => format!("Unexpected token '{p}'"),
            Tok::Ident(s) => {
                if is_keyword(s) || (s == "await" && self.ctx.is_async) {
                    format!("Unexpected token '{s}'")
                } else if self.ctx.strict && STRICT_RESERVED.contains(&s.as_str()) {
                    "Unexpected strict mode reserved word".to_string()
                } else {
                    format!("Unexpected identifier '{s}'")
                }
            }
            Tok::EscapedIdent(s) => format!("Unexpected identifier '{s}'"),
            Tok::PrivateName(s) => format!("Unexpected identifier '#{s}'"),
            Tok::Num(_) | Tok::BigInt(_) => "Unexpected number".to_string(),
            Tok::Str(_) => "Unexpected string".to_string(),
            Tok::Template(..)
            | Tok::TemplateHead(..)
            | Tok::TemplateMiddle(..)
            | Tok::TemplateTail(..) => "Unexpected template string".to_string(),
            Tok::Regex(..) => "Unexpected regular expression".to_string(),
        };
        self.err_at(t, msg)
    }

    fn consume_semicolon(&mut self) -> PResult<()> {
        if self.eat(";") {
            return Ok(());
        }
        if self.is_punct("}") || matches!(self.tok(), Tok::Eof) || self.peek().nl_before {
            return Ok(());
        }
        Err(self.unexpected())
    }

    /// An identifier usable as a binding or reference here.
    fn ident_name_here(&self) -> Option<Name> {
        match self.tok() {
            Tok::Ident(s) => {
                if is_keyword(s) {
                    return None;
                }
                if self.ctx.strict && STRICT_RESERVED.contains(&s.as_str()) {
                    return None;
                }
                if s == "await"
                    && (self.ctx.is_async || (self.top_level_await && !self.ctx.in_function))
                {
                    return None;
                }
                if s == "yield" && self.ctx.is_generator {
                    return None;
                }
                Some(Rc::from(s.as_str()))
            }
            Tok::EscapedIdent(s) => Some(Rc::from(s.as_str())),
            _ => None,
        }
    }

    fn binding_ident(&mut self) -> PResult<Name> {
        match self.ident_name_here() {
            Some(n) => {
                if self.ctx.strict && (&*n == "eval" || &*n == "arguments") {
                    return Err(self.err_here("Unexpected eval or arguments in strict mode"));
                }
                self.advance();
                Ok(n)
            }
            None => Err(self.unexpected()),
        }
    }

    /// IdentifierName (keywords allowed), e.g. after `.`.
    fn ident_name_any(&mut self) -> PResult<Name> {
        match self.tok().clone() {
            Tok::Ident(s) | Tok::EscapedIdent(s) => {
                self.advance();
                Ok(Rc::from(s.as_str()))
            }
            _ => Err(self.unexpected()),
        }
    }

    // ----- program -----
    pub fn parse_program(&mut self) -> PResult<Program> {
        let strict = self.directive_strict();
        if strict {
            self.ctx.strict = true;
        }
        let mut body = vec![];
        while !matches!(self.tok(), Tok::Eof) {
            body.push(self.statement_list_item(true)?);
        }
        Ok(Program {
            body,
            is_module: self.is_module,
            strict: self.ctx.strict,
        })
    }

    fn directive_strict(&self) -> bool {
        let mut j = self.i;
        while let Tok::Str(s) = &self.toks[j].tok {
            let raw_len = self.toks[j].end - self.toks[j].start;
            if s == "use strict" && raw_len == 12 {
                return true;
            }
            j += 1;
            if matches!(&self.toks[j].tok, Tok::Punct(";")) {
                j += 1;
            }
        }
        false
    }

    #[allow(clippy::single_match)]
    fn statement_list_item(&mut self, top: bool) -> PResult<Stmt> {
        let pos = self.pos();
        match self.tok().clone() {
            Tok::Ident(k) => match k.as_str() {
                "function" => {
                    let f = self.function(false, true, pos)?;
                    return Ok(Stmt {
                        kind: StmtKind::Func(f),
                        pos,
                    });
                }
                "async" if self.is_kw_at(1, "function") && !self.peek_at(1).nl_before => {
                    self.advance();
                    let f = self.function(true, true, pos)?;
                    return Ok(Stmt {
                        kind: StmtKind::Func(f),
                        pos,
                    });
                }
                "class" => {
                    let c = self.class(true)?;
                    return Ok(Stmt {
                        kind: StmtKind::Class(c),
                        pos,
                    });
                }
                "const" => return self.lexical_decl(VarKind::Const),
                "let" if self.let_is_decl() => return self.lexical_decl(VarKind::Let),
                "import" if !self.is_punct_at(1, "(") && !self.is_punct_at(1, ".") => {
                    if !top || self.ctx.in_function {
                        return Err(self.unexpected());
                    }
                    self.saw_module_syntax = true;
                    return self.import_decl();
                }
                "export" => {
                    if !top || self.ctx.in_function {
                        return Err(self.unexpected());
                    }
                    self.saw_module_syntax = true;
                    return self.export_decl();
                }
                _ => {}
            },
            _ => {}
        }
        self.statement()
    }

    fn let_is_decl(&self) -> bool {
        match &self.peek_at(1).tok {
            Tok::Ident(s) => !(s == "in" || s == "instanceof"),
            Tok::EscapedIdent(_) => true,
            Tok::Punct("[") | Tok::Punct("{") => true,
            _ => false,
        }
    }

    fn lexical_decl(&mut self, kind: VarKind) -> PResult<Stmt> {
        let pos = self.pos();
        self.advance();
        let decls = self.declarators(kind, false)?;
        self.consume_semicolon()?;
        Ok(Stmt {
            kind: StmtKind::Var(kind, decls),
            pos,
        })
    }

    fn declarators(&mut self, kind: VarKind, no_in: bool) -> PResult<Vec<Declarator>> {
        let mut out = vec![];
        loop {
            let tpos = self.i;
            let dpos = self.pos();
            if kind != VarKind::Var && self.is_kw("let") {
                return Err(self.err_here("let is disallowed as a lexically bound name"));
            }
            let target = self.binding_target()?;
            let init = if self.eat("=") {
                Some(self.assign(no_in)?)
            } else {
                let for_in_of = self.is_kw("of") || self.is_kw("in");
                if !for_in_of || !no_in {
                    if kind == VarKind::Const {
                        let t = self.toks[tpos].clone();
                        return Err(self.err_at(&t, "Missing initializer in const declaration"));
                    }
                    if !matches!(target, Pattern::Ident(..)) {
                        let t = self.toks[tpos].clone();
                        return Err(
                            self.err_at(&t, "Missing initializer in destructuring declaration")
                        );
                    }
                }
                None
            };
            out.push(Declarator {
                target,
                init,
                pos: dpos,
            });
            if !self.eat(",") {
                break;
            }
        }
        Ok(out)
    }

    fn binding_target(&mut self) -> PResult<Pattern> {
        let pos = self.pos();
        if self.eat("[") {
            let mut elems = vec![];
            let mut rest = None;
            loop {
                if self.eat("]") {
                    break;
                }
                if self.is_punct(",") {
                    self.advance();
                    elems.push(None);
                    continue;
                }
                if self.eat("...") {
                    rest = Some(Box::new(self.binding_target()?));
                    if !self.is_punct("]") {
                        return Err(self.err_here("Rest element must be last element"));
                    }
                    self.advance();
                    break;
                }
                let target = self.binding_target()?;
                let default = if self.eat("=") {
                    Some(self.assign(false)?)
                } else {
                    None
                };
                elems.push(Some(PatElem { target, default }));
                if !self.is_punct("]") {
                    self.expect(",")?;
                }
            }
            return Ok(Pattern::Array { elems, rest });
        }
        if self.eat("{") {
            let mut props = vec![];
            let mut rest = None;
            loop {
                if self.eat("}") {
                    break;
                }
                if self.eat("...") {
                    let p = self.pos();
                    let n = self.binding_ident()?;
                    rest = Some(Box::new(Pattern::Ident(n, p)));
                    if !self.is_punct("}") {
                        return Err(self.err_here("Rest element must be last element"));
                    }
                    self.advance();
                    break;
                }
                let kpos = self.pos();
                let shorthand_name = self.ident_name_here();
                let key = self.prop_key()?;
                let value = if self.eat(":") {
                    let target = self.binding_target()?;
                    let default = if self.eat("=") {
                        Some(self.assign(false)?)
                    } else {
                        None
                    };
                    PatElem { target, default }
                } else {
                    let Some(n) = shorthand_name else {
                        return Err(self.unexpected());
                    };
                    let default = if self.eat("=") {
                        Some(self.assign(false)?)
                    } else {
                        None
                    };
                    PatElem {
                        target: Pattern::Ident(n, kpos),
                        default,
                    }
                };
                props.push(PatProp { key, value });
                if !self.is_punct("}") {
                    self.expect(",")?;
                }
            }
            return Ok(Pattern::Object { props, rest });
        }
        let n = self.binding_ident()?;
        Ok(Pattern::Ident(n, pos))
    }

    fn prop_key(&mut self) -> PResult<PropKey> {
        match self.tok().clone() {
            Tok::Ident(s) | Tok::EscapedIdent(s) => {
                self.advance();
                Ok(PropKey::Lit(Rc::from(s.as_str())))
            }
            Tok::Str(s) => {
                self.advance();
                Ok(PropKey::Lit(Rc::from(s.as_str())))
            }
            Tok::Num(n) => {
                self.advance();
                Ok(PropKey::Lit(Rc::from(
                    crate::numconv::number_to_string(n).as_str(),
                )))
            }
            Tok::BigInt(s) => {
                self.advance();
                Ok(PropKey::Lit(Rc::from(s.as_str())))
            }
            Tok::PrivateName(s) => {
                self.advance();
                Ok(PropKey::Private(Rc::from(s.as_str())))
            }
            Tok::Punct("[") => {
                self.advance();
                let e = self.assign(false)?;
                self.expect("]")?;
                Ok(PropKey::Computed(Box::new(e)))
            }
            _ => Err(self.unexpected()),
        }
    }

    // ----- statements -----
    fn statement(&mut self) -> PResult<Stmt> {
        let pos = self.pos();
        let t = self.peek().clone();
        let kind = match &t.tok {
            Tok::Punct("{") => StmtKind::Block(self.block()?),
            Tok::Punct(";") => {
                self.advance();
                StmtKind::Empty
            }
            Tok::Ident(k) => match k.as_str() {
                "var" => {
                    self.advance();
                    let d = self.declarators(VarKind::Var, false)?;
                    self.consume_semicolon()?;
                    StmtKind::Var(VarKind::Var, d)
                }
                "if" => {
                    self.advance();
                    self.expect("(")?;
                    let test = self.expression(false)?;
                    self.expect(")")?;
                    let cons = self.sub_statement()?;
                    let alt = if self.eat_kw("else") {
                        Some(Box::new(self.sub_statement()?))
                    } else {
                        None
                    };
                    StmtKind::If(test, Box::new(cons), alt)
                }
                "for" => return self.for_statement(),
                "while" => {
                    self.advance();
                    self.expect("(")?;
                    let test = self.expression(false)?;
                    self.expect(")")?;
                    let body = self.loop_body()?;
                    StmtKind::While(test, Box::new(body))
                }
                "do" => {
                    self.advance();
                    let body = self.loop_body()?;
                    self.expect_kw("while")?;
                    self.expect("(")?;
                    let test = self.expression(false)?;
                    self.expect(")")?;
                    self.eat(";");
                    StmtKind::DoWhile(Box::new(body), test)
                }
                "continue" | "break" => {
                    let is_break = k == "break";
                    self.advance();
                    let label = if !self.peek().nl_before {
                        if let Some(n) = self.ident_name_here() {
                            let lt = self.peek().clone();
                            match self.labels.iter().rev().find(|(l, _)| **l == *n) {
                                None => {
                                    return Err(self.err_at(&lt, format!("Undefined label '{n}'")))
                                }
                                Some((_, is_loop)) => {
                                    if !is_break && !is_loop {
                                        return Err(self.err_at(&lt, format!("Illegal continue statement: '{n}' does not denote an iteration statement")));
                                    }
                                }
                            }
                            self.advance();
                            Some(n)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if label.is_none() {
                        if is_break && self.ctx.loop_depth == 0 && self.ctx.switch_depth == 0 {
                            return Err(self.err_at(&t, "Illegal break statement"));
                        }
                        if !is_break && self.ctx.loop_depth == 0 {
                            return Err(self.err_at(
                                &t,
                                "Illegal continue statement: no surrounding iteration statement",
                            ));
                        }
                    }
                    self.consume_semicolon()?;
                    if is_break {
                        StmtKind::Break(label)
                    } else {
                        StmtKind::Continue(label)
                    }
                }
                "return" => {
                    if !self.ctx.in_function && self.is_module {
                        return Err(self.err_at(&t, "Illegal return statement"));
                    }
                    self.advance();
                    let arg = if self.is_punct(";")
                        || self.is_punct("}")
                        || matches!(self.tok(), Tok::Eof)
                        || self.peek().nl_before
                    {
                        None
                    } else {
                        Some(self.expression(false)?)
                    };
                    self.consume_semicolon()?;
                    StmtKind::Return(arg)
                }
                "throw" => {
                    self.advance();
                    if self.peek().nl_before {
                        return Err(self.err_here("Illegal newline after throw"));
                    }
                    let e = self.expression(false)?;
                    self.consume_semicolon()?;
                    StmtKind::Throw(e)
                }
                "try" => {
                    self.advance();
                    let block = self.block()?;
                    let mut param = None;
                    let mut handler = None;
                    let mut finalizer = None;
                    if self.eat_kw("catch") {
                        if self.eat("(") {
                            param = Some(self.binding_target()?);
                            self.expect(")")?;
                        }
                        handler = Some(self.block()?);
                    }
                    if self.eat_kw("finally") {
                        finalizer = Some(self.block()?);
                    }
                    if handler.is_none() && finalizer.is_none() {
                        return Err(self.err_here("Missing catch or finally after try"));
                    }
                    StmtKind::Try {
                        block,
                        param,
                        handler,
                        finalizer,
                    }
                }
                "switch" => {
                    self.advance();
                    self.expect("(")?;
                    let d = self.expression(false)?;
                    self.expect(")")?;
                    self.expect("{")?;
                    let mut cases = vec![];
                    let mut seen_default = false;
                    self.ctx.switch_depth += 1;
                    while !self.eat("}") {
                        let cpos = self.pos();
                        let test = if self.eat_kw("case") {
                            Some(self.expression(false)?)
                        } else if self.is_kw("default") {
                            if seen_default {
                                return Err(self
                                    .err_here("More than one default clause in switch statement"));
                            }
                            seen_default = true;
                            self.advance();
                            None
                        } else {
                            return Err(self.unexpected());
                        };
                        self.expect(":")?;
                        let mut body = vec![];
                        while !self.is_kw("case") && !self.is_kw("default") && !self.is_punct("}") {
                            if matches!(self.tok(), Tok::Eof) {
                                return Err(self.unexpected());
                            }
                            body.push(self.statement_list_item(false)?);
                        }
                        cases.push(Case {
                            test,
                            body,
                            pos: cpos,
                        });
                    }
                    self.ctx.switch_depth -= 1;
                    StmtKind::Switch(d, cases)
                }
                "debugger" => {
                    self.advance();
                    self.consume_semicolon()?;
                    StmtKind::Debugger
                }
                "with" => {
                    if self.ctx.strict {
                        return Err(
                            self.err_here("Strict mode code may not include a with statement")
                        );
                    }
                    self.advance();
                    self.expect("(")?;
                    let object = self.expression(false)?;
                    self.expect(")")?;
                    let body = self.sub_statement()?;
                    StmtKind::With(object, Box::new(body))
                }
                "function" => {
                    // Function declaration in statement position (sloppy mode).
                    let f = self.function(false, true, pos)?;
                    StmtKind::Func(f)
                }
                "class" | "const" => return Err(self.unexpected()),
                "let" if self.is_punct_at(1, "[") => {
                    return Err(self.err_here(
                        "Lexical declaration cannot appear in a single-statement context",
                    ))
                }
                _ => {
                    // Labeled statement?
                    if let Some(n) = self.ident_name_here() {
                        if self.is_punct_at(1, ":") {
                            if self.labels.iter().any(|(l, _)| **l == *n) {
                                return Err(
                                    self.err_here(format!("Label '{n}' has already been declared"))
                                );
                            }
                            self.advance();
                            self.advance();
                            let is_loop =
                                self.is_kw("for") || self.is_kw("while") || self.is_kw("do");
                            self.labels.push((n.clone(), is_loop));
                            let body = if self.is_kw("function") {
                                self.statement_list_item(false)?
                            } else {
                                // A labeled block may be exited with `break label`.
                                let saved = self.ctx.switch_depth;
                                if !is_loop {
                                    self.ctx.switch_depth += 1;
                                }
                                let s = self.statement();
                                self.ctx.switch_depth = saved;
                                s?
                            };
                            self.labels.pop();
                            return Ok(Stmt {
                                kind: StmtKind::Labeled(n, Box::new(body)),
                                pos,
                            });
                        }
                    }
                    let e = self.expression(false)?;
                    self.consume_semicolon()?;
                    StmtKind::Expr(e)
                }
            },
            _ => {
                let e = self.expression(false)?;
                self.consume_semicolon()?;
                StmtKind::Expr(e)
            }
        };
        Ok(Stmt { kind, pos })
    }

    fn sub_statement(&mut self) -> PResult<Stmt> {
        if self.is_kw("let") && self.is_punct_at(1, "[") {
            return Err(
                self.err_here("Lexical declaration cannot appear in a single-statement context")
            );
        }
        self.statement()
    }

    fn loop_body(&mut self) -> PResult<Stmt> {
        self.ctx.loop_depth += 1;
        // Labels directly on this loop were pushed by the labeled statement.
        let r = self.sub_statement();
        self.ctx.loop_depth -= 1;
        r
    }

    pub fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect("{")?;
        let mut body = vec![];
        while !self.eat("}") {
            if matches!(self.tok(), Tok::Eof) {
                return Err(self.unexpected());
            }
            body.push(self.statement_list_item(false)?);
        }
        Ok(body)
    }

    fn for_statement(&mut self) -> PResult<Stmt> {
        let pos = self.pos();
        self.advance();
        let mut is_await = false;
        if self.is_kw("await") {
            if !(self.ctx.is_async || (!self.ctx.in_function && self.top_level_await)) {
                if !self.ctx.in_function {
                    self.saw_module_syntax = true;
                } else {
                    return Err(self.unexpected());
                }
            }
            self.advance();
            is_await = true;
        }
        self.expect("(")?;
        let mut init = None;
        if self.is_punct(";") {
            // no init
        } else {
            let decl_kind = if self.is_kw("var") {
                Some(VarKind::Var)
            } else if self.is_kw("const") {
                Some(VarKind::Const)
            } else if self.is_kw("let") && self.let_is_decl() {
                Some(VarKind::Let)
            } else {
                None
            };
            if let Some(kind) = decl_kind {
                self.advance();
                let tpos = self.i;
                let target = self.binding_target()?;
                if self.is_kw("of") || self.is_kw("in") {
                    let is_of = self.is_kw("of");
                    self.advance();
                    let right = if is_of {
                        self.assign(false)?
                    } else {
                        self.expression(false)?
                    };
                    self.expect(")")?;
                    let body = self.loop_body()?;
                    let left = ForLeft::Var(kind, target);
                    let kind = if is_of {
                        StmtKind::ForOf {
                            left,
                            right,
                            body: Box::new(body),
                            is_await,
                        }
                    } else {
                        StmtKind::ForIn {
                            left,
                            right,
                            body: Box::new(body),
                        }
                    };
                    return Ok(Stmt { kind, pos });
                }
                // Regular for: rewind and parse declarators fully.
                self.i = tpos;
                let decls = self.declarators(kind, true)?;
                init = Some(ForInit::Var(kind, decls));
            } else {
                let start_tok = self.peek().clone();
                let e = self.expression(true)?;
                if self.is_kw("of") || self.is_kw("in") {
                    let is_of = self.is_kw("of");
                    let target = self.to_pattern(e, &start_tok)?;
                    self.advance();
                    let right = if is_of {
                        self.assign(false)?
                    } else {
                        self.expression(false)?
                    };
                    self.expect(")")?;
                    let body = self.loop_body()?;
                    let left = ForLeft::Pattern(target);
                    let kind = if is_of {
                        StmtKind::ForOf {
                            left,
                            right,
                            body: Box::new(body),
                            is_await,
                        }
                    } else {
                        StmtKind::ForIn {
                            left,
                            right,
                            body: Box::new(body),
                        }
                    };
                    return Ok(Stmt { kind, pos });
                }
                init = Some(ForInit::Expr(e));
            }
        }
        self.expect(";")?;
        let test = if self.is_punct(";") {
            None
        } else {
            Some(self.expression(false)?)
        };
        self.expect(";")?;
        let update = if self.is_punct(")") {
            None
        } else {
            Some(self.expression(false)?)
        };
        self.expect(")")?;
        let body = self.loop_body()?;
        Ok(Stmt {
            kind: StmtKind::For {
                init,
                test,
                update,
                body: Box::new(body),
            },
            pos,
        })
    }

    fn import_decl(&mut self) -> PResult<Stmt> {
        let pos = self.pos();
        self.advance();
        let mut names = vec![];
        if let Tok::Str(s) = self.tok().clone() {
            self.advance();
            self.consume_semicolon()?;
            return Ok(Stmt {
                kind: StmtKind::Import(names, Rc::from(s.as_str())),
                pos,
            });
        }
        if let Some(n) = self.ident_name_here() {
            self.advance();
            names.push(ImportName::Default(n));
            if !self.eat(",") {
                return self.import_from(names, pos);
            }
        }
        if self.eat("*") {
            if !self.eat_kw("as") {
                return Err(self.unexpected());
            }
            let n = self.binding_ident()?;
            names.push(ImportName::Namespace(n));
        } else if self.eat("{") {
            while !self.eat("}") {
                let imported = match self.tok().clone() {
                    Tok::Str(s) => {
                        self.advance();
                        Rc::from(s.as_str())
                    }
                    _ => self.ident_name_any()?,
                };
                let local = if self.eat_kw("as") {
                    self.binding_ident()?
                } else {
                    imported.clone()
                };
                names.push(ImportName::Named(imported, local));
                if !self.is_punct("}") {
                    self.expect(",")?;
                }
            }
        }
        self.import_from(names, pos)
    }

    fn import_from(&mut self, names: Vec<ImportName>, pos: Pos) -> PResult<Stmt> {
        if !self.eat_kw("from") {
            return Err(self.unexpected());
        }
        let Tok::Str(s) = self.tok().clone() else {
            return Err(self.unexpected());
        };
        self.advance();
        if self.is_kw("with") || self.is_kw("assert") {
            // Import attributes: skip `{ ... }`.
            self.advance();
            self.expect("{")?;
            while !self.eat("}") {
                self.advance();
            }
        }
        self.consume_semicolon()?;
        Ok(Stmt {
            kind: StmtKind::Import(names, Rc::from(s.as_str())),
            pos,
        })
    }

    fn export_decl(&mut self) -> PResult<Stmt> {
        let pos = self.pos();
        self.advance();
        let kind = if self.eat_kw("default") {
            let dpos = self.pos();
            if self.is_kw("function") || (self.is_kw("async") && self.is_kw_at(1, "function")) {
                let is_async = self.eat_kw("async");
                let f = self.function(is_async, false, dpos)?;
                if f.name.is_some() {
                    ExportKind::DefaultDecl(Box::new(Stmt {
                        kind: StmtKind::Func(f),
                        pos: dpos,
                    }))
                } else {
                    ExportKind::Default(Expr {
                        kind: ExprKind::Function(f),
                        pos: dpos,
                    })
                }
            } else if self.is_kw("class") {
                let c = self.class(false)?;
                if c.name.is_some() {
                    ExportKind::DefaultDecl(Box::new(Stmt {
                        kind: StmtKind::Class(c),
                        pos: dpos,
                    }))
                } else {
                    ExportKind::Default(Expr {
                        kind: ExprKind::Class(c),
                        pos: dpos,
                    })
                }
            } else {
                let e = self.assign(false)?;
                self.consume_semicolon()?;
                ExportKind::Default(e)
            }
        } else if self.eat("*") {
            let alias = if self.eat_kw("as") {
                Some(self.ident_name_any()?)
            } else {
                None
            };
            if !self.eat_kw("from") {
                return Err(self.unexpected());
            }
            let Tok::Str(s) = self.tok().clone() else {
                return Err(self.unexpected());
            };
            self.advance();
            self.consume_semicolon()?;
            ExportKind::All(alias, Rc::from(s.as_str()))
        } else if self.eat("{") {
            let mut names = vec![];
            while !self.eat("}") {
                let local = self.ident_name_any()?;
                let exported = if self.eat_kw("as") {
                    self.ident_name_any()?
                } else {
                    local.clone()
                };
                names.push((local, exported));
                if !self.is_punct("}") {
                    self.expect(",")?;
                }
            }
            let from = if self.eat_kw("from") {
                let Tok::Str(s) = self.tok().clone() else {
                    return Err(self.unexpected());
                };
                self.advance();
                Some(Rc::from(s.as_str()))
            } else {
                None
            };
            self.consume_semicolon()?;
            ExportKind::Names(names, from)
        } else {
            let s = self.statement_list_item(false)?;
            match s.kind {
                StmtKind::Var(..) | StmtKind::Func(_) | StmtKind::Class(_) => {}
                _ => return Err(self.err_pos(s.pos, 1, "Unexpected token 'export'")),
            }
            ExportKind::Decl(Box::new(s))
        };
        Ok(Stmt {
            kind: StmtKind::Export(kind),
            pos,
        })
    }

    // ----- functions -----
    /// Parses `function [*] name (params) { body }` starting at `function`.
    fn function(&mut self, is_async: bool, is_decl: bool, pos: Pos) -> PResult<Rc<Func>> {
        let start = if is_async {
            self.toks[self.i - 1].start
        } else {
            self.peek().start
        };
        self.expect_kw("function")?;
        let is_generator = self.eat("*");
        let name = if self.is_punct("(") {
            if is_decl {
                return Err(self.unexpected());
            }
            None
        } else {
            // The name of a function expression is bound inside it, with its
            // own async/generator-ness governing `await`/`yield`.
            let saved = self.ctx;
            if !is_decl {
                self.ctx.is_async = is_async;
                self.ctx.is_generator = is_generator;
            }
            let n = self.binding_ident();
            self.ctx = saved;
            Some(n?)
        };
        self.function_rest(name, FuncKind::Normal, is_async, is_generator, pos, start)
    }

    fn function_rest(
        &mut self,
        name: Option<Name>,
        kind: FuncKind,
        is_async: bool,
        is_generator: bool,
        pos: Pos,
        start: usize,
    ) -> PResult<Rc<Func>> {
        let saved = self.ctx;
        let saved_labels = std::mem::take(&mut self.labels);
        self.ctx.in_function = true;
        self.ctx.is_async = is_async;
        self.ctx.is_generator = is_generator;
        self.ctx.loop_depth = 0;
        self.ctx.switch_depth = 0;
        self.ctx.in_class_field = false;
        match kind {
            FuncKind::Normal => {
                self.ctx.super_call = false;
                self.ctx.super_prop = false;
            }
            FuncKind::Method
            | FuncKind::Getter
            | FuncKind::Setter
            | FuncKind::BaseConstructor
            | FuncKind::ClassInit => {
                self.ctx.super_call = false;
                self.ctx.super_prop = true;
            }
            FuncKind::DerivedConstructor => {
                self.ctx.super_call = true;
                self.ctx.super_prop = true;
            }
            FuncKind::Arrow => {}
        }
        let r = (|| {
            let (params, rest) = self.params()?;
            if kind == FuncKind::Getter && (!params.is_empty() || rest.is_some()) {
                return Err(self.err_here("Getter must not have any formal parameters."));
            }
            if kind == FuncKind::Setter && (params.len() != 1 || rest.is_some()) {
                return Err(self.err_here("Setter must have exactly one formal parameter."));
            }
            if !self.is_punct("{") {
                return Err(self.unexpected());
            }
            let body_strict = {
                let save = self.i;
                self.advance();
                let s = self.directive_strict();
                self.i = save;
                s
            };
            let strict = self.ctx.strict || body_strict;
            self.ctx.strict = strict;
            let simple = rest.is_none()
                && params
                    .iter()
                    .all(|p| p.default.is_none() && matches!(p.target, Pattern::Ident(..)));
            if strict || !simple || kind != FuncKind::Normal {
                let mut names = vec![];
                for p in &params {
                    crate::compiler::pattern_names(&p.target, &mut names);
                }
                if let Some(r) = &rest {
                    crate::compiler::pattern_names(r, &mut names);
                }
                for (i, (n, p)) in names.iter().enumerate() {
                    if names[..i].iter().any(|(m, _)| m == n) {
                        return Err(self.err_pos(
                            *p,
                            n.chars().count() as u32,
                            "Duplicate parameter name not allowed in this context",
                        ));
                    }
                }
            }
            let body = self.block()?;
            Ok(Rc::new(Func {
                name,
                params,
                rest,
                body: FuncBody::Block(body),
                kind,
                is_async,
                is_generator,
                strict,
                pos,
                src_start: start,
                src_end: self.prev_end(),
                fields: None,
            }))
        })();
        self.ctx = saved;
        self.labels = saved_labels;
        r
    }

    fn params(&mut self) -> PResult<(Vec<PatElem>, Option<Pattern>)> {
        self.expect("(")?;
        let mut params = vec![];
        let mut rest = None;
        while !self.eat(")") {
            if self.eat("...") {
                rest = Some(self.binding_target()?);
                if !self.is_punct(")") {
                    return Err(self.err_here("Rest parameter must be last formal parameter"));
                }
                self.advance();
                break;
            }
            let target = self.binding_target()?;
            let default = if self.eat("=") {
                Some(self.assign(false)?)
            } else {
                None
            };
            params.push(PatElem { target, default });
            if !self.is_punct(")") {
                self.expect(",")?;
            }
        }
        Ok((params, rest))
    }

    fn is_arrow_ahead(&self) -> Option<bool> {
        // Returns Some(is_async) when an arrow function starts here.
        let t0 = self.peek();
        let ident0 = matches!(&t0.tok, Tok::Ident(s) if !is_keyword(s))
            || matches!(t0.tok, Tok::EscapedIdent(_));
        if ident0 && self.is_punct_at(1, "=>") && !self.peek_at(1).nl_before {
            return Some(false);
        }
        if self.is_kw("async") && !self.peek_at(1).nl_before {
            let t1 = self.peek_at(1);
            let ident1 = matches!(&t1.tok, Tok::Ident(s) if !is_keyword(s));
            if ident1 && self.is_punct_at(2, "=>") && !self.peek_at(2).nl_before {
                return Some(true);
            }
            if self.is_punct_at(1, "(") {
                if let Some(j) = self.matching_paren(self.i + 1) {
                    if matches!(&self.toks[j + 1].tok, Tok::Punct("=>"))
                        && !self.toks[j + 1].nl_before
                    {
                        return Some(true);
                    }
                }
            }
        }
        if self.is_punct("(") {
            if let Some(j) = self.matching_paren(self.i) {
                if j + 1 < self.toks.len()
                    && matches!(&self.toks[j + 1].tok, Tok::Punct("=>"))
                    && !self.toks[j + 1].nl_before
                {
                    return Some(false);
                }
            }
        }
        None
    }

    fn matching_paren(&self, start: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut j = start;
        while j < self.toks.len() {
            match &self.toks[j].tok {
                Tok::Punct("(") | Tok::Punct("[") | Tok::Punct("{") | Tok::TemplateHead(..) => {
                    depth += 1
                }
                Tok::Punct(")") | Tok::Punct("]") | Tok::Punct("}") | Tok::TemplateTail(..) => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
                Tok::Eof => return None,
                _ => {}
            }
            j += 1;
        }
        None
    }

    fn arrow(&mut self, is_async: bool, no_in: bool) -> PResult<Expr> {
        let pos = self.pos();
        let start = self.peek().start;
        if is_async {
            self.advance();
        }
        let saved = self.ctx;
        let saved_labels = std::mem::take(&mut self.labels);
        self.ctx.is_async = is_async;
        self.ctx.is_generator = false;
        self.ctx.loop_depth = 0;
        self.ctx.switch_depth = 0;
        let r = (|| {
            let (params, rest) = if self.is_punct("(") {
                self.params()?
            } else {
                let p = self.pos();
                let n = self.binding_ident()?;
                (
                    vec![PatElem {
                        target: Pattern::Ident(n, p),
                        default: None,
                    }],
                    None,
                )
            };
            self.expect("=>")?;
            let was_in_function = self.ctx.in_function;
            self.ctx.in_function = true;
            let (body, strict) = if self.is_punct("{") {
                let body_strict = {
                    let save = self.i;
                    self.advance();
                    let s = self.directive_strict();
                    self.i = save;
                    s
                };
                self.ctx.strict = self.ctx.strict || body_strict;
                (FuncBody::Block(self.block()?), self.ctx.strict)
            } else {
                (
                    FuncBody::Expr(Box::new(self.assign(no_in)?)),
                    self.ctx.strict,
                )
            };
            self.ctx.in_function = was_in_function;
            Ok(Rc::new(Func {
                name: None,
                params,
                rest,
                body,
                kind: FuncKind::Arrow,
                is_async,
                is_generator: false,
                strict,
                pos,
                src_start: start,
                src_end: self.prev_end(),
                fields: None,
            }))
        })();
        self.ctx = saved;
        self.labels = saved_labels;
        Ok(Expr {
            kind: ExprKind::Function(r?),
            pos,
        })
    }

    // ----- classes -----
    fn class(&mut self, is_decl: bool) -> PResult<Rc<Class>> {
        let pos = self.pos();
        let start = self.peek().start;
        self.expect_kw("class")?;
        let saved = self.ctx;
        self.ctx.strict = true;
        let name = if self.ident_name_here().is_some() && !self.is_kw("extends") {
            Some(self.binding_ident()?)
        } else {
            if is_decl {
                self.ctx = saved;
                return Err(self.unexpected());
            }
            None
        };
        let extends = if self.eat_kw("extends") {
            Some(Box::new(self.lhs_expression()?))
        } else {
            None
        };
        self.expect("{")?;
        let mut ctor = None;
        let mut members = vec![];
        while !self.eat("}") {
            if self.eat(";") {
                continue;
            }
            let mpos = self.pos();
            let mstart = self.peek().start;
            let mut is_static = false;
            if self.is_kw("static")
                && !self.is_punct_at(1, "(")
                && !self.is_punct_at(1, "=")
                && !self.is_punct_at(1, ";")
                && !self.is_punct_at(1, "}")
            {
                self.advance();
                is_static = true;
                if self.is_punct("{") {
                    let saved2 = self.ctx;
                    self.ctx.in_function = true;
                    self.ctx.is_async = false;
                    self.ctx.is_generator = false;
                    self.ctx.super_prop = true;
                    self.ctx.super_call = false;
                    self.ctx.in_class_field = true;
                    let b = self.block();
                    self.ctx = saved2;
                    members.push(ClassMember::StaticBlock(b?));
                    continue;
                }
            }
            let mut is_async = false;
            let mut is_gen = false;
            let mut kind = MethodKind::Method;
            if self.is_kw("async")
                && !self.is_punct_at(1, "(")
                && !self.is_punct_at(1, "=")
                && !self.peek_at(1).nl_before
                && !self.is_punct_at(1, ";")
                && !self.is_punct_at(1, "}")
            {
                self.advance();
                is_async = true;
            }
            if self.eat("*") {
                is_gen = true;
            }
            if !is_async
                && !is_gen
                && (self.is_kw("get") || self.is_kw("set"))
                && !self.is_punct_at(1, "(")
                && !self.is_punct_at(1, "=")
                && !self.is_punct_at(1, ";")
                && !self.is_punct_at(1, "}")
            {
                kind = if self.is_kw("get") {
                    MethodKind::Get
                } else {
                    MethodKind::Set
                };
                self.advance();
            }
            let key_tok = self.peek().clone();
            let key = self.prop_key()?;
            if self.is_punct("(") {
                let is_ctor = !is_static
                    && matches!(&key, PropKey::Lit(k) if &**k == "constructor")
                    && matches!(key_tok.tok, Tok::Ident(_) | Tok::Str(_));
                if is_ctor {
                    if kind != MethodKind::Method || is_async || is_gen {
                        self.ctx = saved;
                        return Err(self.err_at(
                            &key_tok,
                            "Class constructor may not be a".to_string()
                                + if is_async {
                                    "n async method"
                                } else if is_gen {
                                    " generator"
                                } else {
                                    "n accessor"
                                },
                        ));
                    }
                    if ctor.is_some() {
                        self.ctx = saved;
                        return Err(self.err_at(&key_tok, "A class may only have one constructor"));
                    }
                    let fk = if extends.is_some() {
                        FuncKind::DerivedConstructor
                    } else {
                        FuncKind::BaseConstructor
                    };
                    let f = self.function_rest(name.clone(), fk, false, false, mpos, mstart);
                    let f = match f {
                        Ok(f) => f,
                        Err(e) => {
                            self.ctx = saved;
                            return Err(e);
                        }
                    };
                    ctor = Some(f);
                    continue;
                }
                if is_static && matches!(&key, PropKey::Lit(k) if &**k == "prototype") {
                    self.ctx = saved;
                    return Err(self.err_at(
                        &key_tok,
                        "Classes may not have a static property named 'prototype'",
                    ));
                }
                let fk = match kind {
                    MethodKind::Get => FuncKind::Getter,
                    MethodKind::Set => FuncKind::Setter,
                    MethodKind::Method => FuncKind::Method,
                };
                let fname = match &key {
                    PropKey::Lit(k) => Some(k.clone()),
                    PropKey::Private(k) => Some(Rc::from(format!("#{k}").as_str())),
                    _ => None,
                };
                let f = match self.function_rest(
                    fname,
                    fk,
                    is_async,
                    is_gen,
                    key_tok_pos(&key_tok),
                    mstart,
                ) {
                    Ok(f) => f,
                    Err(e) => {
                        self.ctx = saved;
                        return Err(e);
                    }
                };
                members.push(ClassMember::Method {
                    key,
                    func: f,
                    kind,
                    is_static,
                });
            } else {
                // Field.
                if matches!(&key, PropKey::Lit(k) if &**k == "constructor") {
                    self.ctx = saved;
                    return Err(
                        self.err_at(&key_tok, "Classes may not have a field named 'constructor'")
                    );
                }
                let value = if self.eat("=") {
                    let saved2 = self.ctx;
                    self.ctx.in_function = true;
                    self.ctx.is_async = false;
                    self.ctx.is_generator = false;
                    self.ctx.super_prop = true;
                    self.ctx.super_call = false;
                    self.ctx.in_class_field = true;
                    let v = self.assign(false);
                    self.ctx = saved2;
                    Some(match v {
                        Ok(v) => v,
                        Err(e) => {
                            self.ctx = saved;
                            return Err(e);
                        }
                    })
                } else {
                    None
                };
                if let Err(e) = self.consume_semicolon() {
                    self.ctx = saved;
                    return Err(e);
                }
                members.push(ClassMember::Field {
                    key,
                    value,
                    is_static,
                    pos: key_tok_pos(&key_tok),
                });
            }
        }
        self.ctx = saved;
        Ok(Rc::new(Class {
            name,
            extends,
            ctor,
            members,
            pos,
            src_start: start,
            src_end: self.prev_end(),
        }))
    }

    // ----- expressions -----
    pub fn expression(&mut self, no_in: bool) -> PResult<Expr> {
        let pos = self.pos();
        let first = self.assign(no_in)?;
        if !self.is_punct(",") {
            return Ok(first);
        }
        let mut v = vec![first];
        while self.eat(",") {
            v.push(self.assign(no_in)?);
        }
        Ok(Expr {
            kind: ExprKind::Seq(v),
            pos,
        })
    }

    fn assign(&mut self, no_in: bool) -> PResult<Expr> {
        if let Some(is_async) = self.is_arrow_ahead() {
            return self.arrow(is_async, no_in);
        }
        if self.is_kw("yield") && self.ctx.is_generator {
            return self.yield_expr(no_in);
        }
        let start_tok = self.peek().clone();
        let lhs = self.conditional(no_in)?;
        let op = match self.tok() {
            Tok::Punct(p) => match *p {
                "=" => Some(AssignOp::Assign),
                "+=" => Some(AssignOp::Op(BinOp::Add)),
                "-=" => Some(AssignOp::Op(BinOp::Sub)),
                "*=" => Some(AssignOp::Op(BinOp::Mul)),
                "/=" => Some(AssignOp::Op(BinOp::Div)),
                "%=" => Some(AssignOp::Op(BinOp::Mod)),
                "**=" => Some(AssignOp::Op(BinOp::Exp)),
                "<<=" => Some(AssignOp::Op(BinOp::Shl)),
                ">>=" => Some(AssignOp::Op(BinOp::Shr)),
                ">>>=" => Some(AssignOp::Op(BinOp::UShr)),
                "&=" => Some(AssignOp::Op(BinOp::BitAnd)),
                "|=" => Some(AssignOp::Op(BinOp::BitOr)),
                "^=" => Some(AssignOp::Op(BinOp::BitXor)),
                "&&=" => Some(AssignOp::Logical(LogOp::And)),
                "||=" => Some(AssignOp::Logical(LogOp::Or)),
                "??=" => Some(AssignOp::Logical(LogOp::Nullish)),
                _ => None,
            },
            _ => None,
        };
        let Some(op) = op else {
            if let ExprKind::CoverInit(..) = lhs.kind {
                return Err(self.err_pos(lhs.pos, 1, "Invalid shorthand property initializer"));
            }
            check_no_cover(&lhs)
                .map_err(|p| self.err_pos(p, 1, "Invalid shorthand property initializer"))?;
            return Ok(lhs);
        };
        // V8 reports assignment failures at the operator.
        let pos = self.pos();
        let target = if op == AssignOp::Assign {
            self.to_pattern(lhs, &start_tok)?
        } else {
            match lhs.kind {
                ExprKind::Ident(n) => {
                    self.check_assign_ident(&n, &start_tok)?;
                    Pattern::Ident(n, pos)
                }
                ExprKind::Member { .. } => Pattern::Expr(lhs),
                _ => return Err(self.span_err(&start_tok, "Invalid left-hand side in assignment")),
            }
        };
        self.advance();
        let value = self.assign(no_in)?;
        Ok(Expr {
            kind: ExprKind::Assign {
                op,
                target: Box::new(target),
                value: Box::new(value),
            },
            pos,
        })
    }

    fn check_assign_ident(&self, n: &str, t: &Token) -> PResult<()> {
        if self.ctx.strict && (n == "eval" || n == "arguments") {
            return Err(self.err_at(t, "Unexpected eval or arguments in strict mode"));
        }
        Ok(())
    }

    /// Converts an expression (cover grammar) to an assignment pattern.
    pub fn to_pattern(&self, e: Expr, start: &Token) -> PResult<Pattern> {
        let pos = e.pos;
        match e.kind {
            ExprKind::Ident(n) => {
                self.check_assign_ident(&n, start)?;
                Ok(Pattern::Ident(n, pos))
            }
            ExprKind::Member {
                optional: false, ..
            } => Ok(Pattern::Expr(e)),
            ExprKind::Object(props) => {
                let mut out = vec![];
                let mut rest = None;
                let n = props.len();
                for (i, p) in props.into_iter().enumerate() {
                    match p {
                        Prop::KeyValue(key, v) => {
                            let value = self.to_pat_elem(v, start)?;
                            out.push(PatProp { key, value });
                        }
                        Prop::Shorthand(name, p) => out.push(PatProp {
                            key: PropKey::Lit(name.clone()),
                            value: PatElem {
                                target: Pattern::Ident(name, p),
                                default: None,
                            },
                        }),
                        Prop::Spread(v) => {
                            if i != n - 1 {
                                return Err(self.err_pos(
                                    v.pos,
                                    1,
                                    "Rest element must be last element",
                                ));
                            }
                            rest = Some(Box::new(self.to_pattern(v, start)?));
                        }
                        Prop::Method { .. } => {
                            return Err(self.err_pos(
                                pos,
                                1,
                                "Invalid destructuring assignment target",
                            ))
                        }
                    }
                }
                Ok(Pattern::Object { props: out, rest })
            }
            ExprKind::Array(elems) => {
                let mut out = vec![];
                let mut rest = None;
                let n = elems.len();
                for (i, el) in elems.into_iter().enumerate() {
                    match el {
                        ArrElem::Hole => out.push(None),
                        ArrElem::Expr(v) => out.push(Some(self.to_pat_elem(v, start)?)),
                        ArrElem::Spread(v) => {
                            if i != n - 1 {
                                return Err(self.err_pos(
                                    v.pos,
                                    1,
                                    "Rest element must be last element",
                                ));
                            }
                            rest = Some(Box::new(self.to_pattern(v, start)?));
                        }
                    }
                }
                Ok(Pattern::Array { elems: out, rest })
            }
            ExprKind::CoverInit(..) => {
                Err(self.err_pos(pos, 1, "Invalid destructuring assignment target"))
            }
            _ => Err(self.span_err(start, "Invalid left-hand side in assignment")),
        }
    }

    fn to_pat_elem(&self, v: Expr, start: &Token) -> PResult<PatElem> {
        let vpos = v.pos;
        match v.kind {
            ExprKind::Assign {
                op: AssignOp::Assign,
                target,
                value,
            } => Ok(PatElem {
                target: *target,
                default: Some(*value),
            }),
            ExprKind::CoverInit(name, def) => Ok(PatElem {
                target: Pattern::Ident(name, vpos),
                default: Some(*def),
            }),
            ExprKind::Ident(_)
            | ExprKind::Member { .. }
            | ExprKind::Object(_)
            | ExprKind::Array(_) => Ok(PatElem {
                target: self.to_pattern(v, start)?,
                default: None,
            }),
            _ => Err(self.err_pos(vpos, 1, "Invalid destructuring assignment target")),
        }
    }

    fn yield_expr(&mut self, no_in: bool) -> PResult<Expr> {
        let pos = self.pos();
        self.advance();
        let delegate = !self.peek().nl_before && self.eat("*");
        let arg = if delegate
            || !(self.peek().nl_before
                || self.is_punct(")")
                || self.is_punct("]")
                || self.is_punct("}")
                || self.is_punct(",")
                || self.is_punct(";")
                || self.is_punct(":")
                || matches!(self.tok(), Tok::Eof)
                || self.is_kw("in") && no_in)
        {
            Some(Box::new(self.assign(no_in)?))
        } else {
            None
        };
        Ok(Expr {
            kind: ExprKind::Yield { arg, delegate },
            pos,
        })
    }

    fn conditional(&mut self, no_in: bool) -> PResult<Expr> {
        let test = self.binary(0, no_in)?;
        if !self.is_punct("?") {
            return Ok(test);
        }
        self.advance();
        let pos = test.pos;
        let a = self.assign(false)?;
        self.expect(":")?;
        let b = self.assign(no_in)?;
        Ok(Expr {
            kind: ExprKind::Cond(Box::new(test), Box::new(a), Box::new(b)),
            pos,
        })
    }

    fn binop_here(&self, no_in: bool) -> Option<(u8, Result<BinOp, LogOp>)> {
        let op = match self.tok() {
            Tok::Punct(p) => match *p {
                "??" => (1, Err(LogOp::Nullish)),
                "||" => (2, Err(LogOp::Or)),
                "&&" => (3, Err(LogOp::And)),
                "|" => (4, Ok(BinOp::BitOr)),
                "^" => (5, Ok(BinOp::BitXor)),
                "&" => (6, Ok(BinOp::BitAnd)),
                "==" => (7, Ok(BinOp::Eq)),
                "!=" => (7, Ok(BinOp::Ne)),
                "===" => (7, Ok(BinOp::StrictEq)),
                "!==" => (7, Ok(BinOp::StrictNe)),
                "<" => (8, Ok(BinOp::Lt)),
                ">" => (8, Ok(BinOp::Gt)),
                "<=" => (8, Ok(BinOp::Le)),
                ">=" => (8, Ok(BinOp::Ge)),
                "<<" => (9, Ok(BinOp::Shl)),
                ">>" => (9, Ok(BinOp::Shr)),
                ">>>" => (9, Ok(BinOp::UShr)),
                "+" => (10, Ok(BinOp::Add)),
                "-" => (10, Ok(BinOp::Sub)),
                "*" => (11, Ok(BinOp::Mul)),
                "/" => (11, Ok(BinOp::Div)),
                "%" => (11, Ok(BinOp::Mod)),
                "**" => (12, Ok(BinOp::Exp)),
                _ => return None,
            },
            Tok::Ident(s) if s == "instanceof" => (8, Ok(BinOp::InstanceOf)),
            Tok::Ident(s) if s == "in" && !no_in => (8, Ok(BinOp::In)),
            _ => return None,
        };
        Some(op)
    }

    fn binary(&mut self, min: u8, no_in: bool) -> PResult<Expr> {
        let mut left = if let Tok::PrivateName(n) = self.tok().clone() {
            // `#x in obj`
            if self.is_kw_at(1, "in") && !no_in {
                let pos = self.pos();
                self.advance();
                self.advance();
                let right = self.binary(9, no_in)?;
                Expr {
                    kind: ExprKind::PrivateIn(Rc::from(n.as_str()), Box::new(right)),
                    pos,
                }
            } else {
                return Err(self.unexpected());
            }
        } else {
            self.unary()?
        };
        while let Some((prec, op)) = self.binop_here(no_in) {
            if prec < min {
                break;
            }
            let op_pos = self.pos();
            self.advance();
            let right = if prec == 12 {
                self.binary(12, no_in)?
            } else {
                self.binary(prec + 1, no_in)?
            };
            // V8 reports binary-operator failures at the operator.
            let pos = if op.is_ok() { op_pos } else { left.pos };
            left = Expr {
                kind: match op {
                    Ok(b) => ExprKind::Binary(b, Box::new(left), Box::new(right)),
                    Err(l) => ExprKind::Logical(l, Box::new(left), Box::new(right)),
                },
                pos,
            };
        }
        Ok(left)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let pos = self.pos();
        let t = self.peek().clone();
        let op = match &t.tok {
            Tok::Punct("!") => Some(UnOp::Not),
            Tok::Punct("-") => Some(UnOp::Neg),
            Tok::Punct("+") => Some(UnOp::Plus),
            Tok::Punct("~") => Some(UnOp::BitNot),
            Tok::Ident(s) if s == "typeof" => Some(UnOp::Typeof),
            Tok::Ident(s) if s == "void" => Some(UnOp::Void),
            Tok::Ident(s) if s == "delete" => Some(UnOp::Delete),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let arg = self.unary()?;
            if op == UnOp::Delete && self.ctx.strict {
                if let ExprKind::Ident(_) = arg.kind {
                    return Err(
                        self.err_at(&t, "Delete of an unqualified identifier in strict mode.")
                    );
                }
            }
            if self.is_punct("**") {
                return Err(self.err_here("Unary operator used immediately before exponentiation expression. Parenthesis must be used to disambiguate operator precedence"));
            }
            return Ok(Expr {
                kind: ExprKind::Unary(op, Box::new(arg)),
                pos,
            });
        }
        if self.is_punct("++") || self.is_punct("--") {
            let inc = self.is_punct("++");
            self.advance();
            let tt = self.peek().clone();
            let target = self.unary()?;
            if !matches!(
                target.kind,
                ExprKind::Ident(_)
                    | ExprKind::Member {
                        optional: false,
                        ..
                    }
            ) {
                return Err(
                    self.err_at(&tt, "Invalid left-hand side expression in prefix operation")
                );
            }
            return Ok(Expr {
                kind: ExprKind::Update {
                    inc,
                    prefix: true,
                    target: Box::new(target),
                },
                pos,
            });
        }
        if self.is_kw("await")
            && (self.ctx.is_async || (!self.ctx.in_function && !self.ctx.is_async))
        {
            let top = !self.ctx.in_function;
            if top && !self.top_level_await {
                // Script goal: `await` is an identifier unless followed by an
                // expression start; treat as top-level await (module detection).
                let next = self.peek_at(1);
                let looks_like_await = !next.nl_before
                    && !matches!(&next.tok, Tok::Punct(p) if matches!(*p, ")" | "]" | "}" | ";" | "," | "=" | "." | "?." | ":" | "=>"))
                    && !matches!(next.tok, Tok::Eof);
                if !looks_like_await {
                    return self.postfix();
                }
                self.saw_module_syntax = true;
            }
            self.advance();
            let arg = self.unary()?;
            return Ok(Expr {
                kind: ExprKind::Await(Box::new(arg)),
                pos,
            });
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let t = self.peek().clone();
        let e = self.lhs_expression()?;
        if (self.is_punct("++") || self.is_punct("--")) && !self.peek().nl_before {
            if !matches!(
                e.kind,
                ExprKind::Ident(_)
                    | ExprKind::Member {
                        optional: false,
                        ..
                    }
            ) {
                return Err(
                    self.err_at(&t, "Invalid left-hand side expression in postfix operation")
                );
            }
            let inc = self.is_punct("++");
            self.advance();
            let pos = e.pos;
            return Ok(Expr {
                kind: ExprKind::Update {
                    inc,
                    prefix: false,
                    target: Box::new(e),
                },
                pos,
            });
        }
        Ok(e)
    }

    fn arguments(&mut self) -> PResult<Vec<ArrElem>> {
        self.expect("(")?;
        let mut args = vec![];
        loop {
            if self.eat(")") {
                break;
            }
            if self.eat("...") {
                args.push(ArrElem::Spread(self.assign(false)?));
            } else {
                args.push(ArrElem::Expr(self.assign(false)?));
            }
            if self.eat(")") {
                break;
            }
            if !self.eat(",") {
                // V8 points at the last token of the argument list.
                let prev = self.toks[self.i.saturating_sub(1)].clone();
                return Err(self.err_at(&prev, "missing ) after argument list"));
            }
        }
        Ok(args)
    }

    fn template_parts(&mut self, tagged: bool) -> PResult<TemplateParts> {
        let mut cooked = vec![];
        let mut raw = vec![];
        let mut exprs = vec![];
        let t = self.advance();
        let check = |c: &Option<String>, this: &Self| -> PResult<Option<Rc<str>>> {
            match c {
                Some(s) => Ok(Some(Rc::from(s.as_str()))),
                None if tagged => Ok(None),
                None => Err(this.err_at(&t, "Invalid escape sequence in template")),
            }
        };
        match &t.tok {
            Tok::Template(c, r) => {
                cooked.push(check(c, self)?);
                raw.push(Rc::from(r.as_str()));
            }
            Tok::TemplateHead(c, r) => {
                cooked.push(check(c, self)?);
                raw.push(Rc::from(r.as_str()));
                loop {
                    exprs.push(self.expression(false)?);
                    let t2 = self.peek().clone();
                    match &t2.tok {
                        Tok::TemplateMiddle(c, r) => {
                            self.advance();
                            cooked.push(check(c, self)?);
                            raw.push(Rc::from(r.as_str()));
                        }
                        Tok::TemplateTail(c, r) => {
                            self.advance();
                            cooked.push(check(c, self)?);
                            raw.push(Rc::from(r.as_str()));
                            break;
                        }
                        _ => return Err(self.unexpected()),
                    }
                }
            }
            _ => return Err(self.err_at(&t, "Unexpected token")),
        }
        Ok((cooked, raw, exprs))
    }

    fn is_template(&self) -> bool {
        matches!(self.tok(), Tok::Template(..) | Tok::TemplateHead(..))
    }

    pub fn lhs_expression(&mut self) -> PResult<Expr> {
        let pos = self.pos();
        let mut e = if self.is_kw("new") {
            self.new_expr()?
        } else if self.is_kw("super") {
            let t = self.advance();
            if self.is_punct("(") {
                if !self.ctx.super_call {
                    return Err(self.err_at(&t, "'super' keyword unexpected here"));
                }
                let args = self.arguments()?;
                Expr {
                    kind: ExprKind::SuperCall(args),
                    pos,
                }
            } else if self.eat(".") {
                if !self.ctx.super_prop {
                    return Err(self.err_at(&t, "'super' keyword unexpected here"));
                }
                let p = self.pos();
                let n = self.ident_name_any()?;
                Expr {
                    kind: ExprKind::SuperMember(MemberProp::Name(n, p)),
                    pos,
                }
            } else if self.eat("[") {
                if !self.ctx.super_prop {
                    return Err(self.err_at(&t, "'super' keyword unexpected here"));
                }
                let k = self.expression(false)?;
                self.expect("]")?;
                Expr {
                    kind: ExprKind::SuperMember(MemberProp::Computed(Box::new(k))),
                    pos,
                }
            } else {
                return Err(self.err_at(&t, "'super' keyword unexpected here"));
            }
        } else if self.is_kw("import") {
            let t = self.advance();
            if self.eat(".") {
                let n = self.ident_name_any()?;
                if &*n != "meta" || !self.is_module {
                    return Err(self.err_at(&t, "Cannot use 'import.meta' outside a module"));
                }
                Expr {
                    kind: ExprKind::ImportMeta,
                    pos,
                }
            } else if self.is_punct("(") {
                self.advance();
                let a = self.assign(false)?;
                self.eat(",");
                self.expect(")")?;
                Expr {
                    kind: ExprKind::Import(Box::new(a)),
                    pos,
                }
            } else {
                return Err(self.err_at(&t, "Cannot use import statement outside a module"));
            }
        } else {
            self.primary()?
        };
        let mut optional_chain = false;
        loop {
            if self.is_punct(".") {
                self.advance();
                let p = self.pos();
                let prop = if let Tok::PrivateName(n) = self.tok().clone() {
                    self.advance();
                    MemberProp::Private(Rc::from(n.as_str()), p)
                } else {
                    MemberProp::Name(self.ident_name_any()?, p)
                };
                e = Expr {
                    kind: ExprKind::Member {
                        obj: Box::new(e),
                        prop,
                        optional: false,
                    },
                    pos,
                };
            } else if self.is_punct("?.") {
                self.advance();
                optional_chain = true;
                if self.is_punct("(") {
                    let cpos = self.call_pos(&e);
                    let args = self.arguments()?;
                    e = Expr {
                        kind: ExprKind::Call {
                            callee: Box::new(e),
                            args,
                            optional: true,
                        },
                        pos: cpos,
                    };
                } else if self.is_punct("[") {
                    self.advance();
                    let k = self.expression(false)?;
                    self.expect("]")?;
                    e = Expr {
                        kind: ExprKind::Member {
                            obj: Box::new(e),
                            prop: MemberProp::Computed(Box::new(k)),
                            optional: true,
                        },
                        pos,
                    };
                } else {
                    let p = self.pos();
                    let prop = if let Tok::PrivateName(n) = self.tok().clone() {
                        self.advance();
                        MemberProp::Private(Rc::from(n.as_str()), p)
                    } else {
                        MemberProp::Name(self.ident_name_any()?, p)
                    };
                    e = Expr {
                        kind: ExprKind::Member {
                            obj: Box::new(e),
                            prop,
                            optional: true,
                        },
                        pos,
                    };
                }
            } else if self.is_punct("[") {
                self.advance();
                let k = self.expression(false)?;
                self.expect("]")?;
                e = Expr {
                    kind: ExprKind::Member {
                        obj: Box::new(e),
                        prop: MemberProp::Computed(Box::new(k)),
                        optional: false,
                    },
                    pos,
                };
            } else if self.is_punct("(") {
                let cpos = self.call_pos(&e);
                let args = self.arguments()?;
                e = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(e),
                        args,
                        optional: false,
                    },
                    pos: cpos,
                };
            } else if self.is_template() {
                if optional_chain {
                    return Err(self.err_here("Invalid tagged template on optional chain"));
                }
                let tpos = self.pos();
                let (cooked, raw, exprs) = self.template_parts(true)?;
                e = Expr {
                    kind: ExprKind::Tagged {
                        tag: Box::new(e),
                        cooked,
                        raw,
                        exprs,
                    },
                    pos: tpos,
                };
            } else {
                break;
            }
        }
        if optional_chain {
            e = Expr {
                kind: ExprKind::OptChain(Box::new(e)),
                pos,
            };
        }
        Ok(e)
    }

    /// Source position V8 reports for a call: the property name for method
    /// calls, else the start of the callee.
    fn call_pos(&self, callee: &Expr) -> Pos {
        match &callee.kind {
            ExprKind::Member {
                prop: MemberProp::Name(_, p) | MemberProp::Private(_, p),
                ..
            } => *p,
            ExprKind::Member {
                prop: MemberProp::Computed(k),
                ..
            } => k.pos,
            _ => callee.pos,
        }
    }

    fn new_expr(&mut self) -> PResult<Expr> {
        let pos = self.pos();
        let t = self.advance(); // new
        if self.eat(".") {
            let n = self.ident_name_any()?;
            // CommonJS code runs inside a function, so new.target is valid.
            if &*n != "target" || (!self.ctx.in_function && self.is_module) {
                return Err(self.err_at(&t, "new.target expression is not allowed here"));
            }
            return Ok(Expr {
                kind: ExprKind::NewTarget,
                pos,
            });
        }
        let mut callee = if self.is_kw("new") {
            self.new_expr()?
        } else {
            self.primary()?
        };
        loop {
            if self.eat(".") {
                let p = self.pos();
                let prop = if let Tok::PrivateName(n) = self.tok().clone() {
                    self.advance();
                    MemberProp::Private(Rc::from(n.as_str()), p)
                } else {
                    MemberProp::Name(self.ident_name_any()?, p)
                };
                let cp = callee.pos;
                callee = Expr {
                    kind: ExprKind::Member {
                        obj: Box::new(callee),
                        prop,
                        optional: false,
                    },
                    pos: cp,
                };
            } else if self.is_punct("[") {
                self.advance();
                let k = self.expression(false)?;
                self.expect("]")?;
                let cp = callee.pos;
                callee = Expr {
                    kind: ExprKind::Member {
                        obj: Box::new(callee),
                        prop: MemberProp::Computed(Box::new(k)),
                        optional: false,
                    },
                    pos: cp,
                };
            } else if self.is_template() {
                let (cooked, raw, exprs) = self.template_parts(true)?;
                let cp = callee.pos;
                callee = Expr {
                    kind: ExprKind::Tagged {
                        tag: Box::new(callee),
                        cooked,
                        raw,
                        exprs,
                    },
                    pos: cp,
                };
            } else {
                break;
            }
        }
        if self.is_punct("?.") {
            return Err(self.err_here("Invalid optional chain from new expression"));
        }
        let args = if self.is_punct("(") {
            self.arguments()?
        } else {
            vec![]
        };
        Ok(Expr {
            kind: ExprKind::New {
                callee: Box::new(callee),
                args,
            },
            pos,
        })
    }

    fn primary(&mut self) -> PResult<Expr> {
        let pos = self.pos();
        let t = self.peek().clone();
        let kind = match &t.tok {
            Tok::Num(n) => {
                if self.ctx.strict {
                    let text: String = self
                        ._src
                        .get(t.start..t.end)
                        .map(|c| c.iter().collect())
                        .unwrap_or_default();
                    let b = text.as_bytes();
                    if b.len() > 1 && b[0] == b'0' && b[1].is_ascii_digit() {
                        let msg = if text.bytes().all(|c| (b'0'..=b'7').contains(&c)) {
                            "Octal literals are not allowed in strict mode."
                        } else {
                            "Decimals with leading zeros are not allowed in strict mode."
                        };
                        return Err(self.err_at(&t, msg));
                    }
                }
                self.advance();
                ExprKind::Num(*n)
            }
            Tok::BigInt(s) => {
                self.advance();
                ExprKind::BigInt(Rc::from(s.as_str()))
            }
            Tok::Str(s) => {
                self.advance();
                ExprKind::Str(Rc::from(s.as_str()))
            }
            Tok::Template(..) | Tok::TemplateHead(..) => {
                let (cooked, _raw, exprs) = self.template_parts(false)?;
                ExprKind::Template {
                    cooked: cooked.into_iter().map(|c| c.unwrap()).collect(),
                    exprs,
                }
            }
            Tok::Regex(p, f) => {
                self.advance();
                if let Err(msg) = crate::regexp::validate(p, f) {
                    return Err(self.err_at(&t, msg));
                }
                ExprKind::Regex {
                    pattern: Rc::from(p.as_str()),
                    flags: Rc::from(f.as_str()),
                }
            }
            Tok::Punct("(") => {
                self.advance();
                let e = self.expression(false)?;
                self.expect(")")?;
                return Ok(Expr {
                    kind: e.kind,
                    pos: e.pos,
                });
            }
            Tok::Punct("[") => {
                self.advance();
                let mut elems = vec![];
                loop {
                    if self.eat("]") {
                        break;
                    }
                    if self.eat(",") {
                        elems.push(ArrElem::Hole);
                        continue;
                    }
                    if self.eat("...") {
                        elems.push(ArrElem::Spread(self.assign(false)?));
                    } else {
                        elems.push(ArrElem::Expr(self.assign(false)?));
                    }
                    if !self.is_punct("]") {
                        self.expect(",")?;
                    }
                }
                ExprKind::Array(elems)
            }
            Tok::Punct("{") => self.object_literal()?,
            Tok::Ident(k) => match k.as_str() {
                "this" => {
                    self.advance();
                    ExprKind::This
                }
                "null" => {
                    self.advance();
                    ExprKind::Null
                }
                "true" => {
                    self.advance();
                    ExprKind::Bool(true)
                }
                "false" => {
                    self.advance();
                    ExprKind::Bool(false)
                }
                "function" => {
                    let f = self.function(false, false, pos)?;
                    ExprKind::Function(f)
                }
                "async" if self.is_kw_at(1, "function") && !self.peek_at(1).nl_before => {
                    self.advance();
                    let f = self.function(true, false, pos)?;
                    ExprKind::Function(f)
                }
                "class" => ExprKind::Class(self.class(false)?),
                "arguments" if self.ctx.in_class_field => {
                    return Err(self.err_here("'arguments' is not allowed in class field initializer or static initialization block"));
                }
                _ => match self.ident_name_here() {
                    Some(n) => {
                        self.advance();
                        ExprKind::Ident(n)
                    }
                    None => return Err(self.unexpected()),
                },
            },
            Tok::EscapedIdent(s) => {
                self.advance();
                ExprKind::Ident(Rc::from(s.as_str()))
            }
            _ => return Err(self.unexpected()),
        };
        Ok(Expr { kind, pos })
    }

    fn object_literal(&mut self) -> PResult<ExprKind> {
        self.expect("{")?;
        let mut props = vec![];
        loop {
            if self.eat("}") {
                break;
            }
            if self.eat("...") {
                props.push(Prop::Spread(self.assign(false)?));
            } else {
                let mstart = self.peek().start;
                let mut is_async = false;
                let mut is_gen = false;
                let mut kind = MethodKind::Method;
                let not_key_follows = |p: &Self| {
                    p.is_punct_at(1, "(")
                        || p.is_punct_at(1, ",")
                        || p.is_punct_at(1, ":")
                        || p.is_punct_at(1, "}")
                        || p.is_punct_at(1, "=")
                };
                if self.is_kw("async") && !not_key_follows(self) && !self.peek_at(1).nl_before {
                    self.advance();
                    is_async = true;
                }
                if self.eat("*") {
                    is_gen = true;
                }
                if !is_async
                    && !is_gen
                    && (self.is_kw("get") || self.is_kw("set"))
                    && !not_key_follows(self)
                {
                    kind = if self.is_kw("get") {
                        MethodKind::Get
                    } else {
                        MethodKind::Set
                    };
                    self.advance();
                }
                let key_tok = self.peek().clone();
                let shorthand = self.ident_name_here();
                let is_ident_tok = matches!(key_tok.tok, Tok::Ident(_) | Tok::EscapedIdent(_));
                let key = self.prop_key()?;
                if let PropKey::Private(_) = key {
                    return Err(self.err_at(&key_tok, "Unexpected identifier"));
                }
                if self.is_punct("(") {
                    let fk = match kind {
                        MethodKind::Get => FuncKind::Getter,
                        MethodKind::Set => FuncKind::Setter,
                        MethodKind::Method => FuncKind::Method,
                    };
                    let fname = match &key {
                        PropKey::Lit(k) => Some(k.clone()),
                        _ => None,
                    };
                    let f = self.function_rest(
                        fname,
                        fk,
                        is_async,
                        is_gen,
                        key_tok_pos(&key_tok),
                        mstart,
                    )?;
                    props.push(Prop::Method { key, func: f, kind });
                } else if kind != MethodKind::Method || is_async || is_gen {
                    return Err(self.unexpected());
                } else if self.eat(":") {
                    let v = self.assign(false)?;
                    props.push(Prop::KeyValue(key, v));
                } else if is_ident_tok && self.is_punct("=") {
                    let Some(n) = shorthand else {
                        return Err(self.err_at(&key_tok, "Unexpected token"));
                    };
                    self.advance();
                    let d = self.assign(false)?;
                    props.push(Prop::KeyValue(
                        key,
                        Expr {
                            kind: ExprKind::CoverInit(n, Box::new(d)),
                            pos: key_tok_pos(&key_tok),
                        },
                    ));
                } else if is_ident_tok {
                    let Some(n) = shorthand else {
                        return Err(self.err_at(
                            &key_tok,
                            format!(
                                "Unexpected token '{}'",
                                match &key_tok.tok {
                                    Tok::Ident(s) => s.clone(),
                                    _ => String::new(),
                                }
                            ),
                        ));
                    };
                    props.push(Prop::Shorthand(n, key_tok_pos(&key_tok)));
                } else {
                    return Err(self.unexpected());
                }
            }
            if self.eat("}") {
                break;
            }
            self.expect(",")?;
        }
        Ok(ExprKind::Object(props))
    }
}

fn key_tok_pos(t: &Token) -> Pos {
    pos_of(t)
}

/// Finds a stray `{a = 1}` shorthand initialiser in an expression that is
/// not used as a pattern.
fn check_no_cover(e: &Expr) -> Result<(), Pos> {
    match &e.kind {
        ExprKind::Object(props) => {
            for p in props {
                match p {
                    Prop::KeyValue(_, v) => {
                        if let ExprKind::CoverInit(..) = v.kind {
                            return Err(v.pos);
                        }
                        check_no_cover(v)?;
                    }
                    Prop::Spread(v) => check_no_cover(v)?,
                    _ => {}
                }
            }
            Ok(())
        }
        ExprKind::Array(els) => {
            for el in els {
                if let ArrElem::Expr(v) | ArrElem::Spread(v) = el {
                    check_no_cover(v)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Parses source text. `module` selects the module goal; with `None`, the
/// script goal is tried and module syntax is reported via the flag.
pub fn parse(src: &str, is_module: bool) -> Result<(Program, bool), SyntaxErr> {
    let toks = crate::lexer::tokenize(src)?;
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser::new(toks, &chars, is_module);
    let prog = p.parse_program()?;
    Ok((prog, p.saw_module_syntax))
}
