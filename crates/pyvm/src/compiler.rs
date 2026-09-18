//! AST to bytecode: symbol-table analysis (locals, cells, free variables, globals)
//! followed by code generation with CPython-style block unwinding.
use crate::ast::*;
use crate::lexer::SyntaxErr;
use crate::value::{Code, Op, Pos, PyStr, Value};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// ---------------- symbol tables ----------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ScopeKind {
    Module,
    Function,
    Class,
    Comprehension,
}

#[derive(Debug)]
struct Scope {
    kind: ScopeKind,
    params: Vec<String>,
    bound: Vec<String>,
    bound_set: HashSet<String>,
    used: HashSet<String>,
    globals: HashSet<String>,
    nonlocals: HashSet<String>,
    parent: Option<usize>,
    cells: Vec<String>,
    frees: Vec<String>,
    is_generator: bool,
    is_coroutine: bool,
    /// Function uses `super()` or `__class__` and needs the class cell.
    needs_class: bool,
    /// Class body must create a `__class__` cell for its methods.
    class_cell: bool,
}
impl Scope {
    fn new(kind: ScopeKind, parent: Option<usize>) -> Self {
        Scope {
            kind,
            params: vec![],
            bound: vec![],
            bound_set: HashSet::new(),
            used: HashSet::new(),
            globals: HashSet::new(),
            nonlocals: HashSet::new(),
            parent,
            cells: vec![],
            frees: vec![],
            is_generator: false,
            is_coroutine: false,
            needs_class: false,
            class_cell: false,
        }
    }
    fn bind(&mut self, name: &str) {
        if self.bound_set.insert(name.to_string()) {
            self.bound.push(name.to_string());
        }
    }
    fn is_local(&self, name: &str) -> bool {
        !self.globals.contains(name)
            && !self.nonlocals.contains(name)
            && (self.bound_set.contains(name) || self.params.iter().any(|p| p == name))
    }
}

struct SymBuilder {
    scopes: Vec<Scope>,
    by_node: HashMap<usize, usize>,
    stack: Vec<usize>,
    filename: Rc<str>,
}

fn node_id<T>(r: &T) -> usize {
    r as *const T as usize
}

type CResult<T> = Result<T, SyntaxErr>;

impl SymBuilder {
    fn cur(&mut self) -> &mut Scope {
        let i = *self.stack.last().unwrap();
        &mut self.scopes[i]
    }
    fn push(&mut self, kind: ScopeKind, node: usize) -> usize {
        let parent = self.stack.last().copied();
        let idx = self.scopes.len();
        self.scopes.push(Scope::new(kind, parent));
        self.by_node.insert(node, idx);
        self.stack.push(idx);
        idx
    }
    fn pop(&mut self) {
        self.stack.pop();
    }
    fn bind(&mut self, name: &str, line: u32) -> CResult<()> {
        let _ = line;
        self.cur().bind(name);
        Ok(())
    }
    fn use_name(&mut self, name: &str) {
        self.cur().used.insert(name.to_string());
    }

    fn stmts(&mut self, body: &[Stmt]) -> CResult<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }
    fn declare(&mut self, names: &[String], global: bool, line: u32) -> CResult<()> {
        let kind = self.cur().kind;
        for n in names {
            let scope = self.cur();
            if scope.params.contains(n) {
                return Err(SyntaxErr::new(
                    format!(
                        "name '{n}' is parameter and {}",
                        if global { "global" } else { "nonlocal" }
                    ),
                    line,
                    0,
                ));
            }
            if scope.used.contains(n) || scope.bound_set.contains(n) {
                let what = if scope.bound_set.contains(n) {
                    "assigned to"
                } else {
                    "used"
                };
                return Err(SyntaxErr::new(
                    format!(
                        "name '{n}' is {what} prior to {} declaration",
                        if global { "global" } else { "nonlocal" }
                    ),
                    line,
                    0,
                ));
            }
            if global {
                scope.globals.insert(n.clone());
            } else {
                if kind == ScopeKind::Module {
                    return Err(SyntaxErr::new(
                        "nonlocal declaration not allowed at module level",
                        line,
                        0,
                    ));
                }
                scope.nonlocals.insert(n.clone());
            }
        }
        Ok(())
    }
    fn stmt(&mut self, s: &Stmt) -> CResult<()> {
        match &s.kind {
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Assign(targets, v) => {
                self.expr(v)?;
                for t in targets {
                    self.target(t)?;
                }
                Ok(())
            }
            StmtKind::AugAssign(t, _, v) => {
                self.expr(v)?;
                if let ExprKind::Name(n) = &t.kind {
                    self.use_name(n);
                }
                self.target(t)
            }
            StmtKind::AnnAssign(t, ann, v, simple) => {
                if let Some(v) = v {
                    self.expr(v)?;
                }
                if !(*simple && self.cur().kind == ScopeKind::Function) {
                    self.expr(ann)?;
                }
                if v.is_some() || !*simple {
                    self.target(t)?;
                } else if let ExprKind::Name(n) = &t.kind {
                    // Annotation alone makes the name local in a function.
                    if self.cur().kind == ScopeKind::Function {
                        self.bind(n, s.line)?;
                    }
                }
                Ok(())
            }
            StmtKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v)?;
                }
                Ok(())
            }
            StmtKind::Pass | StmtKind::Break | StmtKind::Continue => Ok(()),
            StmtKind::If(c, a, b) | StmtKind::While(c, a, b) => {
                self.expr(c)?;
                self.stmts(a)?;
                self.stmts(b)
            }
            StmtKind::For(t, it, body, orelse, _) => {
                self.expr(it)?;
                self.target(t)?;
                self.stmts(body)?;
                self.stmts(orelse)
            }
            StmtKind::Try(body, handlers, orelse, fin) => {
                self.stmts(body)?;
                for h in handlers {
                    if let Some(t) = &h.typ {
                        self.expr(t)?;
                    }
                    if let Some(n) = &h.name {
                        self.bind(n, h.line)?;
                    }
                    self.stmts(&h.body)?;
                }
                self.stmts(orelse)?;
                self.stmts(fin)
            }
            StmtKind::Raise(a, b) => {
                if let Some(a) = a {
                    self.expr(a)?;
                }
                if let Some(b) = b {
                    self.expr(b)?;
                }
                Ok(())
            }
            StmtKind::With(items, body, _) => {
                for (e, t) in items {
                    self.expr(e)?;
                    if let Some(t) = t {
                        self.target(t)?;
                    }
                }
                self.stmts(body)
            }
            StmtKind::FunctionDef(f) => {
                for d in &f.decorators {
                    self.expr(d)?;
                }
                self.func_defaults(&f.params)?;
                if let Some(r) = &f.returns {
                    self.expr(r)?;
                }
                self.annotations(&f.params)?;
                self.bind(&f.name, s.line)?;
                self.function(f)
            }
            StmtKind::ClassDef(c) => {
                for d in &c.decorators {
                    self.expr(d)?;
                }
                for b in &c.bases {
                    self.expr(b)?;
                }
                for k in &c.keywords {
                    self.expr(&k.value)?;
                }
                self.bind(&c.name, s.line)?;
                self.push(ScopeKind::Class, node_id(&**c));
                self.stmts(&c.body)?;
                self.pop();
                Ok(())
            }
            StmtKind::Import(names) => {
                for a in names {
                    let n = a
                        .asname
                        .clone()
                        .unwrap_or_else(|| a.name.split('.').next().unwrap().to_string());
                    self.bind(&n, s.line)?;
                }
                Ok(())
            }
            StmtKind::ImportFrom(_, names, _) => {
                for a in names {
                    if a.name == "*" {
                        if self.cur().kind != ScopeKind::Module {
                            return Err(SyntaxErr::new(
                                "import * only allowed at module level",
                                s.line,
                                0,
                            ));
                        }
                        continue;
                    }
                    let n = a.asname.clone().unwrap_or_else(|| a.name.clone());
                    self.bind(&n, s.line)?;
                }
                Ok(())
            }
            StmtKind::Global(names) => self.declare(names, true, s.line),
            StmtKind::Nonlocal(names) => self.declare(names, false, s.line),
            StmtKind::Assert(t, m) => {
                self.expr(t)?;
                if let Some(m) = m {
                    self.expr(m)?;
                }
                Ok(())
            }
            StmtKind::Delete(ts) => {
                for t in ts {
                    self.target(t)?;
                }
                Ok(())
            }
            StmtKind::Match(subject, cases) => {
                self.expr(subject)?;
                for c in cases {
                    self.pattern(&c.pattern)?;
                    if let Some(g) = &c.guard {
                        self.expr(g)?;
                    }
                    self.stmts(&c.body)?;
                }
                Ok(())
            }
        }
    }
    fn pattern(&mut self, p: &Pattern) -> CResult<()> {
        match p {
            Pattern::Capture(Some(n)) | Pattern::Star(Some(n)) => self.bind(n, 0),
            Pattern::Capture(None) | Pattern::Star(None) | Pattern::Singleton(_) => Ok(()),
            Pattern::Value(e) => self.expr(e),
            Pattern::Sequence(items) | Pattern::Or(items) => {
                for i in items {
                    self.pattern(i)?;
                }
                Ok(())
            }
            Pattern::Mapping {
                keys,
                patterns,
                rest,
            } => {
                for k in keys {
                    self.expr(k)?;
                }
                for p in patterns {
                    self.pattern(p)?;
                }
                if let Some(r) = rest {
                    self.bind(r, 0)?;
                }
                Ok(())
            }
            Pattern::Class { cls, args, kwargs } => {
                self.expr(cls)?;
                for a in args {
                    self.pattern(a)?;
                }
                for (_, p) in kwargs {
                    self.pattern(p)?;
                }
                Ok(())
            }
            Pattern::As(p, n) => {
                self.pattern(p)?;
                self.bind(n, 0)
            }
        }
    }
    fn func_defaults(&mut self, p: &Params) -> CResult<()> {
        for d in &p.defaults {
            self.expr(d)?;
        }
        for d in p.kw_defaults.iter().flatten() {
            self.expr(d)?;
        }
        Ok(())
    }
    fn annotations(&mut self, p: &Params) -> CResult<()> {
        for a in p
            .posonly
            .iter()
            .chain(&p.args)
            .chain(&p.vararg)
            .chain(&p.kwonly)
            .chain(&p.kwarg)
        {
            if let Some(an) = &a.annotation {
                self.expr(an)?;
            }
        }
        Ok(())
    }
    fn function(&mut self, f: &FuncDef) -> CResult<()> {
        let idx = self.push(ScopeKind::Function, node_id(f));
        let p = &f.params;
        let mut params: Vec<String> = vec![];
        for a in p.posonly.iter().chain(&p.args) {
            params.push(a.name.clone());
        }
        for a in &p.kwonly {
            params.push(a.name.clone());
        }
        if let Some(a) = &p.vararg {
            params.push(a.name.clone());
        }
        if let Some(a) = &p.kwarg {
            params.push(a.name.clone());
        }
        self.scopes[idx].params = params;
        self.scopes[idx].is_coroutine = f.is_async;
        self.stmts(&f.body)?;
        self.pop();
        Ok(())
    }
    fn target(&mut self, t: &Expr) -> CResult<()> {
        match &t.kind {
            ExprKind::Name(n) => self.bind(n, t.line),
            ExprKind::Tuple(items) | ExprKind::List(items) => {
                for i in items {
                    self.target(i)?;
                }
                Ok(())
            }
            ExprKind::Starred(i) => self.target(i),
            ExprKind::Attribute(v, _) => self.expr(v),
            ExprKind::Subscript(v, i) => {
                self.expr(v)?;
                self.expr(i)
            }
            _ => self.expr(t),
        }
    }
    fn comprehension(&mut self, e: &Expr, gens: &[Comprehension], elts: &[&Expr]) -> CResult<()> {
        // The first iterator is evaluated in the enclosing scope.
        self.expr(&gens[0].iter)?;
        let idx = self.push(ScopeKind::Comprehension, node_id(e));
        self.scopes[idx].params = vec![".0".into()];
        if matches!(e.kind, ExprKind::GenExp(..)) {
            self.scopes[idx].is_generator = true;
        }
        for (i, g) in gens.iter().enumerate() {
            if i > 0 {
                self.expr(&g.iter)?;
            }
            self.target(&g.target)?;
            for c in &g.ifs {
                self.expr(c)?;
            }
        }
        for x in elts {
            self.expr(x)?;
        }
        self.pop();
        Ok(())
    }
    fn expr(&mut self, e: &Expr) -> CResult<()> {
        match &e.kind {
            ExprKind::Name(n) => {
                if n == "super" {
                    self.mark_super();
                }
                if n == "__class__" {
                    self.mark_super();
                }
                self.use_name(n);
                Ok(())
            }
            ExprKind::Const(_) => Ok(()),
            ExprKind::JoinedStr(parts) => {
                for p in parts {
                    self.fpart(p)?;
                }
                Ok(())
            }
            ExprKind::List(v) | ExprKind::Tuple(v) | ExprKind::Set(v) => {
                for x in v {
                    self.expr(x)?;
                }
                Ok(())
            }
            ExprKind::Dict(items) => {
                for (k, v) in items {
                    if let Some(k) = k {
                        self.expr(k)?;
                    }
                    self.expr(v)?;
                }
                Ok(())
            }
            ExprKind::Starred(x) | ExprKind::Attribute(x, _) | ExprKind::UnaryOp(_, x) => {
                self.expr(x)
            }
            ExprKind::Await(x) => self.expr(x),
            ExprKind::Subscript(a, b) | ExprKind::BinOp(_, a, b) => {
                self.expr(a)?;
                self.expr(b)
            }
            ExprKind::Slice(a, b, c) => {
                for x in [a, b, c].into_iter().flatten() {
                    self.expr(x)?;
                }
                Ok(())
            }
            ExprKind::Call {
                func,
                args,
                keywords,
            } => {
                self.expr(func)?;
                for a in args {
                    self.expr(a)?;
                }
                for k in keywords {
                    self.expr(&k.value)?;
                }
                Ok(())
            }
            ExprKind::BoolOp(_, v) => {
                for x in v {
                    self.expr(x)?;
                }
                Ok(())
            }
            ExprKind::Compare(l, ops) => {
                self.expr(l)?;
                for (_, x) in ops {
                    self.expr(x)?;
                }
                Ok(())
            }
            ExprKind::IfExp(a, b, c) => {
                self.expr(a)?;
                self.expr(b)?;
                self.expr(c)
            }
            ExprKind::Lambda(f) => {
                self.func_defaults(&f.params)?;
                self.function(f)
            }
            ExprKind::ListComp(elt, gens)
            | ExprKind::SetComp(elt, gens)
            | ExprKind::GenExp(elt, gens) => self.comprehension(e, gens, &[elt]),
            ExprKind::DictComp(k, v, gens) => self.comprehension(e, gens, &[k, v]),
            ExprKind::Yield(v) => {
                self.mark_generator(e)?;
                if let Some(v) = v {
                    self.expr(v)?;
                }
                Ok(())
            }
            ExprKind::YieldFrom(v) => {
                self.mark_generator(e)?;
                self.expr(v)
            }
            ExprKind::NamedExpr(t, v) => {
                self.expr(v)?;
                if let ExprKind::Name(n) = &t.kind {
                    // Walrus in a comprehension binds in the enclosing function.
                    let mut i = self.stack.len() - 1;
                    while self.scopes[self.stack[i]].kind == ScopeKind::Comprehension && i > 0 {
                        let si = self.stack[i];
                        i -= 1;
                        let target = self.stack[i];
                        if self.scopes[target].kind == ScopeKind::Module
                            || self.scopes[target].globals.contains(n)
                        {
                            self.scopes[si].globals.insert(n.clone());
                        } else if self.scopes[target].kind != ScopeKind::Comprehension {
                            self.scopes[si].nonlocals.insert(n.clone());
                        } else {
                            self.scopes[si].nonlocals.insert(n.clone());
                        }
                    }
                    let target = self.stack[i];
                    self.scopes[target].bind(n);
                    if i == self.stack.len() - 1 {
                        return Ok(());
                    }
                    return Ok(());
                }
                Ok(())
            }
        }
    }
    fn fpart(&mut self, p: &FStrPart) -> CResult<()> {
        if let FStrPart::Expr { value, spec, .. } = p {
            self.expr(value)?;
            if let Some(spec) = spec {
                for s in spec {
                    self.fpart(s)?;
                }
            }
        }
        Ok(())
    }
    fn mark_generator(&mut self, e: &Expr) -> CResult<()> {
        let s = self.cur();
        match s.kind {
            ScopeKind::Function | ScopeKind::Comprehension => {
                if s.kind == ScopeKind::Comprehension {
                    return Err(SyntaxErr::new(
                        "'yield' inside list comprehension",
                        e.line,
                        e.col,
                    ));
                }
                s.is_generator = true;
                Ok(())
            }
            _ => Err(SyntaxErr::new("'yield' outside function", e.line, e.col)),
        }
    }
    fn mark_super(&mut self) {
        // The innermost enclosing function directly inside a class needs __class__.
        for &i in self.stack.iter().rev() {
            if self.scopes[i].kind == ScopeKind::Function {
                self.scopes[i].needs_class = true;
                if let Some(p) = self.scopes[i].parent {
                    if self.scopes[p].kind == ScopeKind::Class {
                        self.scopes[p].class_cell = true;
                        return;
                    }
                }
            }
        }
    }

    /// Resolves free variables: a name used but not bound in a function-like scope
    /// is free if an enclosing function binds it; that scope then makes it a cell.
    fn resolve(&mut self) -> CResult<()> {
        for idx in 0..self.scopes.len() {
            let kind = self.scopes[idx].kind;
            if kind == ScopeKind::Module {
                continue;
            }
            let mut names: Vec<String> = self.scopes[idx].used.iter().cloned().collect();
            names.sort();
            let mut nl: Vec<String> = self.scopes[idx].nonlocals.iter().cloned().collect();
            nl.sort();
            for n in &nl {
                if !self.find_enclosing(idx, n, true)? {
                    return Err(SyntaxErr::new(
                        format!("no binding for nonlocal '{n}' found"),
                        0,
                        0,
                    ));
                }
            }
            if self.scopes[idx].needs_class {
                self.add_free(idx, "__class__");
            }
            for n in names {
                let s = &self.scopes[idx];
                if s.globals.contains(&n) || s.nonlocals.contains(&n) {
                    continue;
                }
                if kind != ScopeKind::Class && s.is_local(&n) {
                    continue;
                }
                if kind == ScopeKind::Class && s.bound_set.contains(&n) {
                    continue;
                }
                self.find_enclosing(idx, &n, false)?;
            }
        }
        Ok(())
    }
    fn add_free(&mut self, idx: usize, name: &str) {
        let s = &mut self.scopes[idx];
        if !s.frees.iter().any(|f| f == name) {
            s.frees.push(name.to_string());
        }
    }
    /// Walks outward from `idx`. Returns whether a binding was found.
    fn find_enclosing(&mut self, idx: usize, name: &str, nonlocal: bool) -> CResult<bool> {
        let mut path = vec![idx];
        let mut cur = self.scopes[idx].parent;
        while let Some(p) = cur {
            let s = &self.scopes[p];
            match s.kind {
                ScopeKind::Module => return Ok(false),
                ScopeKind::Class => {
                    if name == "__class__" && s.class_cell {
                        // The class body owns the __class__ cell.
                        let s = &mut self.scopes[p];
                        if !s.cells.iter().any(|c| c == "__class__") {
                            s.cells.push("__class__".into());
                        }
                        for &q in &path {
                            self.add_free(q, name);
                        }
                        return Ok(true);
                    }
                }
                _ => {
                    if s.globals.contains(name) {
                        return Ok(false);
                    }
                    if s.nonlocals.contains(name) || s.frees.iter().any(|f| f == name) {
                        for &q in &path {
                            self.add_free(q, name);
                        }
                        self.add_free(p, name);
                        return Ok(true);
                    }
                    if s.is_local(name) {
                        let s = &mut self.scopes[p];
                        if !s.cells.iter().any(|c| c == name) {
                            s.cells.push(name.to_string());
                        }
                        for &q in &path {
                            self.add_free(q, name);
                        }
                        return Ok(true);
                    }
                    path.push(p);
                }
            }
            cur = self.scopes[p].parent;
        }
        let _ = nonlocal;
        Ok(false)
    }
}

// ---------------- code generation ----------------

#[derive(Clone, Copy, PartialEq, Eq)]
struct Label(u32);

enum FBlock {
    While { start: Label, end: Label },
    For { start: Label, end: Label },
    TryExcept,
    Finally(*const [Stmt]),
    FinallyEnd,
    HandlerCleanup(Option<String>),
    With,
    PopValue,
}

struct Unit {
    scope: usize,
    ops: Vec<Op>,
    pos: Vec<Pos>,
    consts: Vec<Value>,
    names: Vec<Rc<PyStr>>,
    name_map: HashMap<String, u32>,
    varnames: Vec<Rc<str>>,
    var_map: HashMap<String, u32>,
    cellvars: Vec<Rc<str>>,
    freevars: Vec<Rc<str>>,
    labels: Vec<u32>,
    fblocks: Vec<FBlock>,
    cur: Pos,
    name: Rc<str>,
    qualname: Rc<str>,
    kind: ScopeKind,
    in_async: bool,
}

pub struct Compiler<'a> {
    syms: SymBuilder,
    units: Vec<Unit>,
    filename: Rc<str>,
    _src: &'a str,
    interactive: bool,
}

pub fn compile_module(body: &[Stmt], filename: &str, src: &str, mode: &str) -> CResult<Rc<Code>> {
    let interactive = mode == "single";
    let eval = mode == "eval";
    let filename: Rc<str> = filename.into();
    let mut syms = SymBuilder {
        scopes: vec![],
        by_node: HashMap::new(),
        stack: vec![],
        filename: filename.clone(),
    };
    syms.push(ScopeKind::Module, 0);
    syms.stmts(body)?;
    syms.pop();
    syms.resolve()?;
    let _ = &syms.filename;
    let mut c = Compiler {
        syms,
        units: vec![],
        filename,
        _src: src,
        interactive,
    };
    c.enter(0, "<module>".into(), "<module>".into());
    if eval {
        // eval(): the single expression statement is the result.
        if let Some(Stmt {
            kind: StmtKind::Expr(e),
            line,
        }) = body.first()
        {
            c.u().cur = Compiler::stmt_pos(*line);
            c.expr(e)?;
            c.emit(Op::Return);
            return Ok(c.leave(0, None));
        }
    }
    let doc = docstring(body);
    if let Some(d) = &doc {
        c.load_const(Value::str(d));
        c.store_name("__doc__");
    }
    let has_ann = body
        .iter()
        .any(|s| matches!(s.kind, StmtKind::AnnAssign(_, _, _, true)));
    if has_ann {
        c.emit(Op::SetupAnnotations);
    }
    c.stmts(body)?;
    c.load_const(Value::None);
    c.emit(Op::Return);
    Ok(c.leave(0, doc))
}

fn docstring(body: &[Stmt]) -> Option<String> {
    match body.first().map(|s| &s.kind) {
        Some(StmtKind::Expr(Expr {
            kind: ExprKind::Const(Const::Str(s)),
            ..
        })) => Some(s.clone()),
        _ => None,
    }
}

fn is_jump(op: &Op) -> bool {
    matches!(
        op,
        Op::Jump(_)
            | Op::PopJumpIfFalse(_)
            | Op::PopJumpIfTrue(_)
            | Op::JumpIfFalseOrPop(_)
            | Op::JumpIfTrueOrPop(_)
            | Op::ForIter(_)
            | Op::SetupFinally(_)
            | Op::JumpIfNotExcMatch(_)
            | Op::SetupWith(_)
    )
}

fn patch(op: Op, labels: &[u32]) -> Op {
    let f = |l: u32| labels[l as usize];
    match op {
        Op::Jump(l) => Op::Jump(f(l)),
        Op::PopJumpIfFalse(l) => Op::PopJumpIfFalse(f(l)),
        Op::PopJumpIfTrue(l) => Op::PopJumpIfTrue(f(l)),
        Op::JumpIfFalseOrPop(l) => Op::JumpIfFalseOrPop(f(l)),
        Op::JumpIfTrueOrPop(l) => Op::JumpIfTrueOrPop(f(l)),
        Op::ForIter(l) => Op::ForIter(f(l)),
        Op::SetupFinally(l) => Op::SetupFinally(f(l)),
        Op::JumpIfNotExcMatch(l) => Op::JumpIfNotExcMatch(f(l)),
        Op::SetupWith(l) => Op::SetupWith(f(l)),
        other => other,
    }
}

enum NameOp {
    Fast(u32),
    Deref(u32),
    ClassDeref(u32),
    Global(u32),
    Name(u32),
}

fn expr_pos(e: &Expr) -> Pos {
    let mut p = Pos {
        line: e.line,
        end_line: e.line,
        col: e.col,
        end_col: e.end_col.max(e.col + 1),
        anchor: (0, 0, 0),
    };
    match &e.kind {
        ExprKind::BinOp(_, l, r) => {
            p.anchor = (l.end_col, r.col, 1);
            if r.line != l.line || l.line != e.line {
                p.end_line = r.line;
            }
        }
        ExprKind::Subscript(v, s) => {
            p.anchor = (v.end_col, s.end_col, 2);
        }
        _ => {}
    }
    p
}

impl<'a> Compiler<'a> {
    fn u(&mut self) -> &mut Unit {
        self.units.last_mut().unwrap()
    }
    fn enter(&mut self, scope: usize, name: Rc<str>, qualname: Rc<str>) {
        let s = &self.syms.scopes[scope];
        let kind = s.kind;
        let mut varnames: Vec<Rc<str>> = vec![];
        let mut var_map = HashMap::new();
        if matches!(kind, ScopeKind::Function | ScopeKind::Comprehension) {
            for p in &s.params {
                var_map.insert(p.clone(), varnames.len() as u32);
                varnames.push(p.as_str().into());
            }
            for b in &s.bound {
                if !var_map.contains_key(b) && s.is_local(b) {
                    var_map.insert(b.clone(), varnames.len() as u32);
                    varnames.push(b.as_str().into());
                }
            }
        }
        let cellvars: Vec<Rc<str>> = s.cells.iter().map(|c| c.as_str().into()).collect();
        let freevars: Vec<Rc<str>> = s.frees.iter().map(|c| c.as_str().into()).collect();
        let in_async = s.is_coroutine;
        self.units.push(Unit {
            scope,
            ops: vec![],
            pos: vec![],
            consts: vec![],
            names: vec![],
            name_map: HashMap::new(),
            varnames,
            var_map,
            cellvars,
            freevars,
            labels: vec![],
            fblocks: vec![],
            cur: Pos::default(),
            name,
            qualname,
            kind,
            in_async,
        });
    }
    fn leave(&mut self, argspec: u32, doc: Option<String>) -> Rc<Code> {
        let _ = argspec;
        let u = self.units.pop().unwrap();
        let s = &self.syms.scopes[u.scope];
        let ops: Vec<Op> = u
            .ops
            .iter()
            .map(|o| if is_jump(o) { patch(*o, &u.labels) } else { *o })
            .collect();
        let cell2arg = u
            .cellvars
            .iter()
            .map(|c| s.params.iter().position(|p| **p == **c).map(|i| i as u32))
            .collect();
        Rc::new(Code {
            name: u.name.clone(),
            qualname: u.qualname.clone(),
            filename: self.filename.clone(),
            ops,
            pos: u.pos,
            consts: u.consts,
            names: u.names,
            varnames: u.varnames,
            cellvars: u.cellvars,
            freevars: u.freevars,
            cell2arg,
            argcount: 0,
            posonlyargcount: 0,
            kwonlyargcount: 0,
            varargs: false,
            varkw: false,
            is_generator: s.is_generator && !s.is_coroutine,
            is_coroutine: s.is_coroutine,
            firstlineno: 1,
            uses_name_ops: matches!(u.kind, ScopeKind::Module | ScopeKind::Class),
            docstring: doc,
        })
    }
    fn emit(&mut self, op: Op) -> usize {
        let u = self.u();
        u.ops.push(op);
        let p = u.cur;
        u.pos.push(p);
        u.ops.len() - 1
    }
    fn new_label(&mut self) -> Label {
        let u = self.u();
        u.labels.push(u32::MAX);
        Label(u.labels.len() as u32 - 1)
    }
    fn bind(&mut self, l: Label) {
        let u = self.u();
        u.labels[l.0 as usize] = u.ops.len() as u32;
    }
    fn jump(&mut self, ctor: fn(u32) -> Op, l: Label) {
        self.emit(ctor(l.0));
    }
    fn const_index(&mut self, v: Value) -> u32 {
        let u = self.u();
        // Reuse identical simple constants.
        for (i, c) in u.consts.iter().enumerate() {
            let same = match (c, &v) {
                (Value::None, Value::None) => true,
                (Value::Bool(a), Value::Bool(b)) => a == b,
                (Value::Int(a), Value::Int(b)) => a == b,
                (Value::Str(a), Value::Str(b)) => a.s == b.s,
                _ => false,
            };
            if same {
                return i as u32;
            }
        }
        u.consts.push(v);
        u.consts.len() as u32 - 1
    }
    fn load_const(&mut self, v: Value) {
        let i = self.const_index(v);
        self.emit(Op::LoadConst(i));
    }
    fn name_index(&mut self, n: &str) -> u32 {
        let u = self.u();
        if let Some(i) = u.name_map.get(n) {
            return *i;
        }
        u.names.push(Rc::new(PyStr::new(n.to_string())));
        let i = u.names.len() as u32 - 1;
        u.name_map.insert(n.to_string(), i);
        i
    }
    fn set_pos(&mut self, p: Pos) -> Pos {
        std::mem::replace(&mut self.u().cur, p)
    }
    fn stmt_pos(line: u32) -> Pos {
        Pos {
            line,
            end_line: line,
            col: 0,
            end_col: u32::MAX,
            anchor: (0, 0, 0),
        }
    }

    fn resolve_name(&mut self, name: &str) -> NameOp {
        let scope = self.u().scope;
        let s = &self.syms.scopes[scope];
        let kind = s.kind;
        let u = self.units.last().unwrap();
        if s.globals.contains(name) {
            let i = self.name_index(name);
            return NameOp::Global(i);
        }
        if let Some(i) = u.cellvars.iter().position(|c| &**c == name) {
            if kind != ScopeKind::Class || name == "__class__" {
                if kind == ScopeKind::Class {
                    // Class bodies store __class__ via the cell only at the end.
                }
                return NameOp::Deref(i as u32);
            }
        }
        if let Some(i) = u.freevars.iter().position(|c| &**c == name) {
            let idx = (u.cellvars.len() + i) as u32;
            if kind == ScopeKind::Class {
                return NameOp::ClassDeref(idx);
            }
            return NameOp::Deref(idx);
        }
        match kind {
            ScopeKind::Module | ScopeKind::Class => {
                let i = self.name_index(name);
                NameOp::Name(i)
            }
            _ => {
                if let Some(i) = u.var_map.get(name) {
                    return NameOp::Fast(*i);
                }
                let i = self.name_index(name);
                NameOp::Global(i)
            }
        }
    }
    fn load_name(&mut self, name: &str) {
        let op = match self.resolve_name(name) {
            NameOp::Fast(i) => Op::LoadFast(i),
            NameOp::Deref(i) => Op::LoadDeref(i),
            NameOp::ClassDeref(i) => Op::LoadClassDeref(i),
            NameOp::Global(i) => Op::LoadGlobal(i),
            NameOp::Name(i) => Op::LoadName(i),
        };
        self.emit(op);
    }
    fn store_name(&mut self, name: &str) {
        let op = match self.resolve_name(name) {
            NameOp::Fast(i) => Op::StoreFast(i),
            NameOp::Deref(i) | NameOp::ClassDeref(i) => Op::StoreDeref(i),
            NameOp::Global(i) => Op::StoreGlobal(i),
            NameOp::Name(i) => Op::StoreName(i),
        };
        self.emit(op);
    }
    fn delete_name(&mut self, name: &str) {
        let op = match self.resolve_name(name) {
            NameOp::Fast(i) => Op::DeleteFast(i),
            NameOp::Deref(i) | NameOp::ClassDeref(i) => Op::DeleteDeref(i),
            NameOp::Global(i) => Op::DeleteGlobal(i),
            NameOp::Name(i) => Op::DeleteName(i),
        };
        self.emit(op);
    }

    fn stmts(&mut self, body: &[Stmt]) -> CResult<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn in_function(&self) -> bool {
        self.units
            .last()
            .is_some_and(|u| matches!(u.kind, ScopeKind::Function | ScopeKind::Comprehension))
    }

    fn stmt(&mut self, s: &Stmt) -> CResult<()> {
        let saved = self.set_pos(Self::stmt_pos(s.line));
        let r = self.stmt_inner(s);
        self.set_pos(saved);
        r
    }

    fn stmt_inner(&mut self, s: &Stmt) -> CResult<()> {
        match &s.kind {
            StmtKind::Expr(e) => {
                if self.interactive && self.units.len() == 1 {
                    self.expr(e)?;
                    self.emit(Op::PrintExpr);
                    return Ok(());
                }
                if let ExprKind::Const(_) = e.kind {
                    return Ok(());
                }
                self.expr(e)?;
                self.emit(Op::Pop);
            }
            StmtKind::Assign(targets, value) => {
                self.expr(value)?;
                for (i, t) in targets.iter().enumerate() {
                    if i + 1 < targets.len() {
                        self.emit(Op::Dup);
                    }
                    self.assign(t)?;
                }
            }
            StmtKind::AugAssign(target, op, value) => match &target.kind {
                ExprKind::Name(n) => {
                    self.load_name(n);
                    self.expr(value)?;
                    let p = self.set_pos(Pos {
                        anchor: (target.end_col, value.col, 1),
                        ..expr_pos(target)
                    });
                    self.emit(Op::Inplace(*op));
                    self.set_pos(p);
                    self.store_name(n);
                }
                ExprKind::Attribute(obj, attr) => {
                    self.expr(obj)?;
                    self.emit(Op::Dup);
                    let i = self.name_index(attr);
                    let p = self.set_pos(expr_pos(target));
                    self.emit(Op::LoadAttr(i));
                    self.set_pos(p);
                    self.expr(value)?;
                    self.emit(Op::Inplace(*op));
                    self.emit(Op::Rot2);
                    self.emit(Op::StoreAttr(i));
                }
                ExprKind::Subscript(obj, idx) => {
                    self.expr(obj)?;
                    self.expr(idx)?;
                    self.emit(Op::DupTwo);
                    let p = self.set_pos(expr_pos(target));
                    self.emit(Op::BinarySubscr);
                    self.set_pos(p);
                    self.expr(value)?;
                    self.emit(Op::Inplace(*op));
                    self.emit(Op::Rot3);
                    self.emit(Op::StoreSubscr);
                }
                _ => unreachable!(),
            },
            StmtKind::AnnAssign(target, ann, value, simple) => {
                if let Some(v) = value {
                    self.expr(v)?;
                    self.assign(target)?;
                }
                let kind = self.u().kind;
                if *simple && matches!(kind, ScopeKind::Module | ScopeKind::Class) {
                    if let ExprKind::Name(n) = &target.kind {
                        // __annotations__[name] = ann
                        self.expr(ann)?;
                        self.load_name("__annotations__");
                        self.load_const(Value::str(n));
                        self.emit(Op::StoreSubscr);
                    }
                }
            }
            StmtKind::Return(v) => {
                if !self.in_function() {
                    return Err(SyntaxErr::new("'return' outside function", s.line, 0));
                }
                let scope = self.u().scope;
                if self.syms.scopes[scope].is_generator
                    && self.syms.scopes[scope].is_coroutine
                    && v.is_some()
                {
                    return Err(SyntaxErr::new(
                        "'return' with value in async generator",
                        s.line,
                        0,
                    ));
                }
                match v {
                    Some(v) => self.expr(v)?,
                    None => self.load_const(Value::None),
                }
                self.unwind_all(true)?;
                self.emit(Op::Return);
            }
            StmtKind::Pass => {}
            StmtKind::Break => {
                let end = self.unwind_loop(false, s.line)?;
                self.jump(Op::Jump, end);
            }
            StmtKind::Continue => {
                let start = self.unwind_loop(true, s.line)?;
                self.jump(Op::Jump, start);
            }
            StmtKind::If(cond, body, orelse) => {
                let else_l = self.new_label();
                let end = self.new_label();
                self.cond_jump(cond, false, else_l)?;
                self.stmts(body)?;
                if !orelse.is_empty() {
                    self.jump(Op::Jump, end);
                }
                self.bind(else_l);
                self.stmts(orelse)?;
                self.bind(end);
            }
            StmtKind::While(cond, body, orelse) => {
                let start = self.new_label();
                let else_l = self.new_label();
                let end = self.new_label();
                self.bind(start);
                let infinite = matches!(cond.kind, ExprKind::Const(Const::True));
                if !infinite {
                    self.cond_jump(cond, false, else_l)?;
                }
                self.u().fblocks.push(FBlock::While { start, end });
                self.stmts(body)?;
                self.u().fblocks.pop();
                self.jump(Op::Jump, start);
                self.bind(else_l);
                self.stmts(orelse)?;
                self.bind(end);
            }
            StmtKind::For(target, iter, body, orelse, is_async) => {
                if *is_async {
                    return Err(SyntaxErr::new(
                        "'async for' is not supported in this interpreter",
                        s.line,
                        0,
                    ));
                }
                let start = self.new_label();
                let cleanup = self.new_label();
                let end = self.new_label();
                self.expr(iter)?;
                self.emit(Op::GetIter);
                self.bind(start);
                let p = self.set_pos(expr_pos(iter));
                self.jump(Op::ForIter, cleanup);
                self.set_pos(p);
                self.assign(target)?;
                self.u().fblocks.push(FBlock::For { start, end });
                self.stmts(body)?;
                self.u().fblocks.pop();
                self.jump(Op::Jump, start);
                self.bind(cleanup);
                self.stmts(orelse)?;
                self.bind(end);
            }
            StmtKind::Try(body, handlers, orelse, finalbody) => {
                if finalbody.is_empty() {
                    self.try_except(body, handlers, orelse)?;
                } else {
                    self.try_finally(body, handlers, orelse, finalbody)?;
                }
            }
            StmtKind::Raise(exc, cause) => {
                let mut n = 0;
                if let Some(e) = exc {
                    self.expr(e)?;
                    n = 1;
                    if let Some(c) = cause {
                        self.expr(c)?;
                        n = 2;
                    }
                }
                self.emit(Op::Raise(n));
            }
            StmtKind::With(items, body, is_async) => {
                if *is_async {
                    return Err(SyntaxErr::new(
                        "'async with' is not supported in this interpreter",
                        s.line,
                        0,
                    ));
                }
                self.with(items, body)?;
            }
            StmtKind::FunctionDef(f) => {
                for d in &f.decorators {
                    self.expr(d)?;
                }
                self.make_function(f)?;
                for _ in &f.decorators {
                    self.emit(Op::Call(1));
                }
                self.store_name(&f.name);
            }
            StmtKind::ClassDef(c) => self.class_def(c)?,
            StmtKind::Import(names) => {
                for a in names {
                    self.load_const(Value::Int(0));
                    self.load_const(Value::None);
                    let i = self.name_index(&a.name);
                    self.emit(Op::ImportName(i));
                    match &a.asname {
                        Some(asname) => {
                            // `import a.b as c` binds the submodule.
                            for part in a.name.split('.').skip(1) {
                                let j = self.name_index(part);
                                self.emit(Op::ImportFrom(j));
                                self.emit(Op::Rot2);
                                self.emit(Op::Pop);
                            }
                            self.store_name(asname);
                        }
                        None => {
                            let top = a.name.split('.').next().unwrap().to_string();
                            self.store_name(&top);
                        }
                    }
                }
            }
            StmtKind::ImportFrom(module, names, level) => {
                self.load_const(Value::Int(*level as i64));
                let fromlist: Vec<Value> = names.iter().map(|a| Value::str(&a.name)).collect();
                self.load_const(Value::tuple(fromlist));
                let i = self.name_index(module.as_deref().unwrap_or(""));
                self.emit(Op::ImportName(i));
                if names.len() == 1 && names[0].name == "*" {
                    self.emit(Op::ImportStar);
                    return Ok(());
                }
                for a in names {
                    let j = self.name_index(&a.name);
                    self.emit(Op::ImportFrom(j));
                    self.store_name(a.asname.as_ref().unwrap_or(&a.name));
                }
                self.emit(Op::Pop);
            }
            StmtKind::Global(_) | StmtKind::Nonlocal(_) => {}
            StmtKind::Assert(test, msg) => {
                let end = self.new_label();
                self.cond_jump(test, true, end)?;
                self.emit(Op::LoadAssertionError);
                if let Some(m) = msg {
                    self.expr(m)?;
                    self.emit(Op::Call(1));
                }
                self.emit(Op::Raise(1));
                self.bind(end);
            }
            StmtKind::Delete(targets) => {
                for t in targets {
                    self.delete(t)?;
                }
            }
            StmtKind::Match(subject, cases) => self.match_stmt(subject, cases)?,
        }
        Ok(())
    }

    fn delete(&mut self, t: &Expr) -> CResult<()> {
        match &t.kind {
            ExprKind::Name(n) => {
                let p = self.set_pos(expr_pos(t));
                self.delete_name(n);
                self.set_pos(p);
            }
            ExprKind::Attribute(obj, attr) => {
                self.expr(obj)?;
                let i = self.name_index(attr);
                self.emit(Op::DeleteAttr(i));
            }
            ExprKind::Subscript(obj, idx) => {
                self.expr(obj)?;
                self.expr(idx)?;
                let p = self.set_pos(expr_pos(t));
                self.emit(Op::DeleteSubscr);
                self.set_pos(p);
            }
            ExprKind::Tuple(items) | ExprKind::List(items) => {
                for i in items {
                    self.delete(i)?;
                }
            }
            _ => return Err(SyntaxErr::new("cannot delete expression", t.line, t.col)),
        }
        Ok(())
    }

    /// Emits cleanup for every enclosing block before a `return`.
    fn unwind_all(&mut self, preserve_tos: bool) -> CResult<()> {
        let n = self.u().fblocks.len();
        for i in (0..n).rev() {
            self.unwind_block(i, preserve_tos)?;
        }
        Ok(())
    }
    /// Emits cleanup up to the innermost loop; returns the loop's continue/break label.
    fn unwind_loop(&mut self, is_continue: bool, line: u32) -> CResult<Label> {
        let n = self.u().fblocks.len();
        for i in (0..n).rev() {
            match &self.u().fblocks[i] {
                FBlock::While { start, end } | FBlock::For { start, end } => {
                    let (start, end) = (*start, *end);
                    let is_for = matches!(self.u().fblocks[i], FBlock::For { .. });
                    if is_continue {
                        return Ok(start);
                    }
                    if is_for {
                        self.emit(Op::Pop);
                    }
                    return Ok(end);
                }
                _ => self.unwind_block(i, false)?,
            }
        }
        Err(SyntaxErr::new(
            if is_continue {
                "'continue' not properly in loop"
            } else {
                "'break' outside loop"
            },
            line,
            0,
        ))
    }
    fn unwind_block(&mut self, i: usize, preserve_tos: bool) -> CResult<()> {
        let block = match &self.u().fblocks[i] {
            FBlock::While { .. } => return Ok(()),
            FBlock::For { .. } => {
                if preserve_tos {
                    self.emit(Op::Rot2);
                }
                self.emit(Op::Pop);
                return Ok(());
            }
            FBlock::TryExcept => {
                self.emit(Op::PopBlock);
                return Ok(());
            }
            FBlock::FinallyEnd => {
                self.emit(Op::PopExcept);
                return Ok(());
            }
            FBlock::HandlerCleanup(name) => {
                let name = name.clone();
                if name.is_some() {
                    self.emit(Op::PopBlock);
                }
                self.emit(Op::PopExcept);
                if let Some(n) = name {
                    self.load_const(Value::None);
                    self.store_name(&n);
                    self.delete_name(&n);
                }
                return Ok(());
            }
            FBlock::With => {
                self.emit(Op::PopBlock);
                if preserve_tos {
                    self.emit(Op::Rot2);
                }
                self.emit(Op::WithExit);
                return Ok(());
            }
            FBlock::PopValue => {
                if preserve_tos {
                    self.emit(Op::Rot2);
                }
                self.emit(Op::Pop);
                return Ok(());
            }
            FBlock::Finally(body) => *body,
        };
        self.emit(Op::PopBlock);
        // Compile the finally body with this block (and inner ones) removed, so a
        // `return` inside it does not re-run it.
        let saved: Vec<FBlock> = self.u().fblocks.drain(i..).collect();
        // SAFETY: the pointer refers to AST owned by the caller for the whole compile.
        let body: &[Stmt] = unsafe { &*block };
        if preserve_tos {
            self.u().fblocks.push(FBlock::PopValue);
        }
        let r = self.stmts(body);
        if preserve_tos {
            self.u().fblocks.pop();
        }
        self.u().fblocks.extend(saved);
        r
    }

    fn try_except(&mut self, body: &[Stmt], handlers: &[Handler], orelse: &[Stmt]) -> CResult<()> {
        let handler = self.new_label();
        let else_l = self.new_label();
        let end = self.new_label();
        self.jump(Op::SetupFinally, handler);
        self.u().fblocks.push(FBlock::TryExcept);
        self.stmts(body)?;
        self.u().fblocks.pop();
        self.emit(Op::PopBlock);
        self.jump(Op::Jump, else_l);
        self.bind(handler);
        // Stack: [exc]; an ExceptHandler block is active.
        for (i, h) in handlers.iter().enumerate() {
            let saved = self.set_pos(Self::stmt_pos(h.line));
            let next = self.new_label();
            if let Some(t) = &h.typ {
                self.emit(Op::Dup);
                self.expr(t)?;
                self.jump(Op::JumpIfNotExcMatch, next);
            } else if i + 1 < handlers.len() {
                return Err(SyntaxErr::new("default 'except:' must be last", h.line, 0));
            }
            match &h.name {
                Some(n) => {
                    self.emit(Op::Dup);
                    self.store_name(n);
                    // try: body finally: n = None; del n
                    let cleanup = self.new_label();
                    self.jump(Op::SetupFinally, cleanup);
                    self.u()
                        .fblocks
                        .push(FBlock::HandlerCleanup(Some(n.clone())));
                    self.stmts(&h.body)?;
                    self.u().fblocks.pop();
                    self.emit(Op::PopBlock);
                    self.emit(Op::PopExcept);
                    self.load_const(Value::None);
                    self.store_name(n);
                    self.delete_name(n);
                    self.jump(Op::Jump, end);
                    // Exception inside the handler body: clear the name, re-raise.
                    self.bind(cleanup);
                    self.load_const(Value::None);
                    self.store_name(n);
                    self.delete_name(n);
                    self.emit(Op::Reraise);
                }
                None => {
                    self.u().fblocks.push(FBlock::HandlerCleanup(None));
                    self.stmts(&h.body)?;
                    self.u().fblocks.pop();
                    self.emit(Op::PopExcept);
                    self.jump(Op::Jump, end);
                }
            }
            self.bind(next);
            self.set_pos(saved);
        }
        self.emit(Op::Reraise);
        self.bind(else_l);
        self.stmts(orelse)?;
        self.bind(end);
        Ok(())
    }

    fn try_finally(
        &mut self,
        body: &[Stmt],
        handlers: &[Handler],
        orelse: &[Stmt],
        finalbody: &[Stmt],
    ) -> CResult<()> {
        let handler = self.new_label();
        let end = self.new_label();
        self.jump(Op::SetupFinally, handler);
        self.u()
            .fblocks
            .push(FBlock::Finally(finalbody as *const [Stmt]));
        if handlers.is_empty() {
            self.stmts(body)?;
        } else {
            self.try_except(body, handlers, orelse)?;
        }
        self.u().fblocks.pop();
        self.emit(Op::PopBlock);
        self.stmts(finalbody)?;
        self.jump(Op::Jump, end);
        self.bind(handler);
        self.u().fblocks.push(FBlock::FinallyEnd);
        self.stmts(finalbody)?;
        self.u().fblocks.pop();
        self.emit(Op::Reraise);
        self.bind(end);
        Ok(())
    }

    fn with(&mut self, items: &[(Expr, Option<Expr>)], body: &[Stmt]) -> CResult<()> {
        let Some(((ctx, target), rest)) = items.split_first() else {
            return self.stmts(body);
        };
        let handler = self.new_label();
        let end = self.new_label();
        self.expr(ctx)?;
        let p = self.set_pos(expr_pos(ctx));
        self.jump(Op::SetupWith, handler);
        self.set_pos(p);
        match target {
            Some(t) => self.assign(t)?,
            None => {
                self.emit(Op::Pop);
            }
        }
        self.u().fblocks.push(FBlock::With);
        self.with(rest, body)?;
        self.u().fblocks.pop();
        self.emit(Op::PopBlock);
        self.emit(Op::WithExit);
        self.jump(Op::Jump, end);
        self.bind(handler);
        // Stack: [__exit__, exc]
        let suppress = self.new_label();
        self.emit(Op::WithExceptStart);
        self.jump(Op::PopJumpIfTrue, suppress);
        self.emit(Op::Reraise);
        self.bind(suppress);
        self.emit(Op::PopExcept);
        self.emit(Op::Pop);
        self.bind(end);
        Ok(())
    }

    fn make_function(&mut self, f: &FuncDef) -> CResult<()> {
        let mut flags = 0;
        if !f.params.defaults.is_empty() {
            for d in &f.params.defaults {
                self.expr(d)?;
            }
            self.emit(Op::BuildTuple(f.params.defaults.len() as u32));
            flags |= 1;
        }
        let kw: Vec<(&Arg, &Expr)> = f
            .params
            .kwonly
            .iter()
            .zip(&f.params.kw_defaults)
            .filter_map(|(a, d)| d.as_ref().map(|d| (a, d)))
            .collect();
        if !kw.is_empty() {
            for (a, d) in &kw {
                self.load_const(Value::str(&a.name));
                self.expr(d)?;
            }
            self.emit(Op::BuildMap(kw.len() as u32));
            flags |= 2;
        }
        // Annotations dict (evaluated eagerly, as in CPython without PEP 563).
        let mut anns = 0;
        let p = &f.params;
        for a in p
            .posonly
            .iter()
            .chain(&p.args)
            .chain(&p.vararg)
            .chain(&p.kwonly)
            .chain(&p.kwarg)
        {
            if let Some(an) = &a.annotation {
                self.load_const(Value::str(&a.name));
                self.expr(an)?;
                anns += 1;
            }
        }
        if let Some(r) = &f.returns {
            self.load_const(Value::str("return"));
            self.expr(r)?;
            anns += 1;
        }
        if anns > 0 {
            self.emit(Op::BuildMap(anns));
            flags |= 4;
        }
        let scope = self.syms.by_node[&node_id(f)];
        let parent_q = self.u().qualname.clone();
        let parent_kind = self.u().kind;
        let qualname: Rc<str> = match parent_kind {
            ScopeKind::Module => f.name.as_str().into(),
            ScopeKind::Class => format!("{parent_q}.{}", f.name).into(),
            _ => format!("{parent_q}.<locals>.{}", f.name).into(),
        };
        let code = self.function_code(f, scope, qualname.clone())?;
        if !code.freevars.is_empty() {
            for fv in code.freevars.clone().iter() {
                self.load_closure(fv);
            }
            self.emit(Op::BuildTuple(code.freevars.len() as u32));
            flags |= 8;
        }
        self.load_const(Value::Code(code));
        self.load_const(Value::Str(Rc::new(PyStr::new(qualname.to_string()))));
        self.emit(Op::MakeFunction(flags));
        Ok(())
    }
    fn load_closure(&mut self, name: &str) {
        let u = self.units.last().unwrap();
        let idx = if let Some(i) = u.cellvars.iter().position(|c| &**c == name) {
            i
        } else if let Some(i) = u.freevars.iter().position(|c| &**c == name) {
            u.cellvars.len() + i
        } else {
            0
        };
        self.emit(Op::LoadClosure(idx as u32));
    }
    fn function_code(&mut self, f: &FuncDef, scope: usize, qualname: Rc<str>) -> CResult<Rc<Code>> {
        self.enter(scope, f.name.as_str().into(), qualname);
        let doc = if f.is_lambda {
            None
        } else {
            docstring(&f.body)
        };
        let body: &[Stmt] = if doc.is_some() { &f.body[1..] } else { &f.body };
        self.u().cur = Self::stmt_pos(f.line);
        self.stmts(body)?;
        let last_return = matches!(body.last().map(|s| &s.kind), Some(StmtKind::Return(_)));
        if !last_return || f.is_lambda {
            self.load_const(Value::None);
            self.emit(Op::Return);
        }
        let code = self.leave(0, doc);
        let p = &f.params;
        let mut code = Rc::try_unwrap(code).ok().unwrap();
        code.argcount = (p.posonly.len() + p.args.len()) as u32;
        code.posonlyargcount = p.posonly.len() as u32;
        code.kwonlyargcount = p.kwonly.len() as u32;
        code.varargs = p.vararg.is_some();
        code.varkw = p.kwarg.is_some();
        code.firstlineno = f.line;
        Ok(Rc::new(code))
    }

    fn class_def(&mut self, c: &ClassDef) -> CResult<()> {
        for d in &c.decorators {
            self.expr(d)?;
        }
        self.emit(Op::LoadBuildClass);
        let scope = self.syms.by_node[&node_id(c)];
        let parent_q = self.u().qualname.clone();
        let qualname: Rc<str> = match self.u().kind {
            ScopeKind::Module => c.name.as_str().into(),
            ScopeKind::Class => format!("{parent_q}.{}", c.name).into(),
            _ => format!("{parent_q}.<locals>.{}", c.name).into(),
        };
        self.enter(scope, c.name.as_str().into(), qualname.clone());
        self.u().cur = Self::stmt_pos(c.line);
        self.load_name("__name__");
        self.store_name("__module__");
        self.load_const(Value::Str(Rc::new(PyStr::new(qualname.to_string()))));
        self.store_name("__qualname__");
        let doc = docstring(&c.body);
        if let Some(d) = &doc {
            self.load_const(Value::str(d));
            self.store_name("__doc__");
        }
        if c.body
            .iter()
            .any(|s| matches!(s.kind, StmtKind::AnnAssign(_, _, _, true)))
        {
            self.emit(Op::SetupAnnotations);
        }
        let body: &[Stmt] = if doc.is_some() { &c.body[1..] } else { &c.body };
        self.stmts(body)?;
        // Return the __class__ cell (or None) so __build_class__ can fill it.
        let has_cell = self.u().cellvars.iter().any(|c| &**c == "__class__");
        if has_cell {
            let i = self
                .u()
                .cellvars
                .iter()
                .position(|c| &**c == "__class__")
                .unwrap();
            self.emit(Op::LoadClosure(i as u32));
        } else {
            self.load_const(Value::None);
        }
        self.emit(Op::Return);
        let code = self.leave(0, None);
        let mut code = Rc::try_unwrap(code).ok().unwrap();
        code.firstlineno = c.line;
        let code = Rc::new(code);
        let mut flags = 0;
        if !code.freevars.is_empty() {
            for fv in code.freevars.clone().iter() {
                self.load_closure(fv);
            }
            self.emit(Op::BuildTuple(code.freevars.len() as u32));
            flags |= 8;
        }
        self.load_const(Value::Code(code));
        self.load_const(Value::Str(Rc::new(PyStr::new(qualname.to_string()))));
        self.emit(Op::MakeFunction(flags));
        self.load_const(Value::str(&c.name));
        self.call_with(&c.bases, &c.keywords, 2)?;
        for _ in &c.decorators {
            self.emit(Op::Call(1));
        }
        self.store_name(&c.name);
        Ok(())
    }

    /// Emits a call whose callable (and `extra` leading args) are already pushed.
    fn call_with(&mut self, args: &[Expr], keywords: &[Keyword], extra: u32) -> CResult<()> {
        let has_star = args.iter().any(|a| matches!(a.kind, ExprKind::Starred(_)));
        let has_dstar = keywords.iter().any(|k| k.name.is_none());
        if !has_star && !has_dstar {
            for a in args {
                self.expr(a)?;
            }
            if keywords.is_empty() {
                self.emit(Op::Call(args.len() as u32 + extra));
            } else {
                for k in keywords {
                    self.expr(&k.value)?;
                }
                let names: Vec<Value> = keywords
                    .iter()
                    .map(|k| Value::str(k.name.as_ref().unwrap()))
                    .collect();
                self.load_const(Value::tuple(names));
                self.emit(Op::CallKw((args.len() + keywords.len()) as u32 + extra));
            }
            return Ok(());
        }
        // General form: build a positional tuple and a keyword dict.
        self.emit(Op::BuildList(extra));
        for a in args {
            if let ExprKind::Starred(inner) = &a.kind {
                self.expr(inner)?;
                self.emit(Op::ListExtend(1));
            } else {
                self.expr(a)?;
                self.emit(Op::ListAppend(1));
            }
        }
        self.emit(Op::ListToTuple);
        if keywords.is_empty() {
            self.emit(Op::CallEx(false));
            return Ok(());
        }
        self.emit(Op::BuildMap(0));
        for k in keywords {
            match &k.name {
                Some(n) => {
                    self.load_const(Value::str(n));
                    self.expr(&k.value)?;
                    self.emit(Op::BuildMap(1));
                    self.emit(Op::DictMerge(1));
                }
                None => {
                    self.expr(&k.value)?;
                    self.emit(Op::DictMerge(1));
                }
            }
        }
        self.emit(Op::CallEx(true));
        Ok(())
    }

    fn assign(&mut self, t: &Expr) -> CResult<()> {
        match &t.kind {
            ExprKind::Name(n) => self.store_name(n),
            ExprKind::Attribute(obj, attr) => {
                self.expr(obj)?;
                let i = self.name_index(attr);
                let p = self.set_pos(expr_pos(t));
                self.emit(Op::StoreAttr(i));
                self.set_pos(p);
            }
            ExprKind::Subscript(obj, idx) => {
                self.expr(obj)?;
                self.expr(idx)?;
                let p = self.set_pos(expr_pos(t));
                self.emit(Op::StoreSubscr);
                self.set_pos(p);
            }
            ExprKind::Tuple(items) | ExprKind::List(items) => {
                let star = items
                    .iter()
                    .position(|i| matches!(i.kind, ExprKind::Starred(_)));
                let p = self.set_pos(expr_pos(t));
                match star {
                    None => {
                        self.emit(Op::UnpackSequence(items.len() as u32));
                    }
                    Some(s) => {
                        let after = items.len() - s - 1;
                        self.emit(Op::UnpackEx((s as u32) | ((after as u32) << 8)));
                    }
                }
                self.set_pos(p);
                for i in items {
                    match &i.kind {
                        ExprKind::Starred(inner) => self.assign(inner)?,
                        _ => self.assign(i)?,
                    }
                }
            }
            ExprKind::Starred(_) => {
                return Err(SyntaxErr::new(
                    "starred assignment target must be in a list or tuple",
                    t.line,
                    t.col,
                ))
            }
            _ => {
                return Err(SyntaxErr::new(
                    format!("cannot assign to {}", crate::parser::expr_desc(t)),
                    t.line,
                    t.col,
                ))
            }
        }
        Ok(())
    }

    /// Jumps to `target` when `expr` is truthy (`when == true`) or falsy.
    fn cond_jump(&mut self, e: &Expr, when: bool, target: Label) -> CResult<()> {
        match &e.kind {
            ExprKind::UnaryOp(UnaryOp::Not, inner) => return self.cond_jump(inner, !when, target),
            ExprKind::BoolOp(is_and, items) => {
                // and: all must be true.
                if *is_and != when {
                    // jump if any item fails (and/false) or passes (or/true)
                    for it in items {
                        self.cond_jump(it, when, target)?;
                    }
                } else {
                    let skip = self.new_label();
                    for (i, it) in items.iter().enumerate() {
                        if i + 1 < items.len() {
                            self.cond_jump(it, !when, skip)?;
                        } else {
                            self.cond_jump(it, when, target)?;
                        }
                    }
                    self.bind(skip);
                }
                return Ok(());
            }
            _ => {}
        }
        self.expr(e)?;
        if when {
            self.jump(Op::PopJumpIfTrue, target);
        } else {
            self.jump(Op::PopJumpIfFalse, target);
        }
        Ok(())
    }

    fn int_const(digits: &str, radix: u32) -> Value {
        let (neg, d) = match digits.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, digits),
        };
        if let Ok(v) = i64::from_str_radix(d, radix) {
            return Value::Int(if neg { -v } else { v });
        }
        let b = crate::bigint::BigInt::parse_digits(d, radix).unwrap_or_default();
        Value::big(if neg { b.neg() } else { b })
    }

    fn expr(&mut self, e: &Expr) -> CResult<()> {
        let saved = self.set_pos(expr_pos(e));
        let r = self.expr_inner(e);
        self.set_pos(saved);
        r
    }

    fn expr_inner(&mut self, e: &Expr) -> CResult<()> {
        match &e.kind {
            ExprKind::Name(n) => self.load_name(n),
            ExprKind::Const(c) => {
                let v = match c {
                    Const::None => Value::None,
                    Const::True => Value::Bool(true),
                    Const::False => Value::Bool(false),
                    Const::Ellipsis => Value::Ellipsis,
                    Const::Int(d, r) => Self::int_const(d, *r),
                    Const::Float(f) => Value::Float(*f),
                    Const::Imag(f) => Value::Complex(0.0, *f),
                    Const::Str(s) => Value::str(s),
                    Const::Bytes(b) => Value::Bytes(Rc::new(b.clone())),
                };
                self.load_const(v);
            }
            ExprKind::JoinedStr(parts) => {
                let n = self.fstring(parts)?;
                if n != 1 || !matches!(parts.first(), Some(FStrPart::Expr { .. })) {
                    self.emit(Op::BuildString(n));
                } else {
                    self.emit(Op::BuildString(1));
                }
            }
            ExprKind::Tuple(items) => {
                if items.iter().any(|i| matches!(i.kind, ExprKind::Starred(_))) {
                    self.starred_seq(items)?;
                    self.emit(Op::ListToTuple);
                } else {
                    for i in items {
                        self.expr(i)?;
                    }
                    self.emit(Op::BuildTuple(items.len() as u32));
                }
            }
            ExprKind::List(items) => {
                if items.iter().any(|i| matches!(i.kind, ExprKind::Starred(_))) {
                    self.starred_seq(items)?;
                } else {
                    for i in items {
                        self.expr(i)?;
                    }
                    self.emit(Op::BuildList(items.len() as u32));
                }
            }
            ExprKind::Set(items) => {
                if items.iter().any(|i| matches!(i.kind, ExprKind::Starred(_))) {
                    self.emit(Op::BuildSet(0));
                    for i in items {
                        if let ExprKind::Starred(inner) = &i.kind {
                            self.expr(inner)?;
                            self.emit(Op::SetUpdate(1));
                        } else {
                            self.expr(i)?;
                            self.emit(Op::SetAdd(1));
                        }
                    }
                } else {
                    for i in items {
                        self.expr(i)?;
                    }
                    self.emit(Op::BuildSet(items.len() as u32));
                }
            }
            ExprKind::Dict(items) => {
                if items.iter().any(|(k, _)| k.is_none()) {
                    self.emit(Op::BuildMap(0));
                    for (k, v) in items {
                        match k {
                            Some(k) => {
                                self.expr(k)?;
                                self.expr(v)?;
                                self.emit(Op::MapAdd(1));
                            }
                            None => {
                                self.expr(v)?;
                                self.emit(Op::DictUpdate(1));
                            }
                        }
                    }
                } else {
                    for (k, v) in items {
                        self.expr(k.as_ref().unwrap())?;
                        self.expr(v)?;
                    }
                    self.emit(Op::BuildMap(items.len() as u32));
                }
            }
            ExprKind::Starred(_) => {
                return Err(SyntaxErr::new(
                    "can't use starred expression here",
                    e.line,
                    e.col,
                ))
            }
            ExprKind::Attribute(obj, attr) => {
                self.expr(obj)?;
                let i = self.name_index(attr);
                self.emit(Op::LoadAttr(i));
            }
            ExprKind::Subscript(obj, idx) => {
                self.expr(obj)?;
                self.expr(idx)?;
                self.emit(Op::BinarySubscr);
            }
            ExprKind::Slice(a, b, c) => {
                for x in [a, b] {
                    match x {
                        Some(x) => self.expr(x)?,
                        None => self.load_const(Value::None),
                    }
                }
                if let Some(c) = c {
                    self.expr(c)?;
                    self.emit(Op::BuildSlice(3));
                } else {
                    self.emit(Op::BuildSlice(2));
                }
            }
            ExprKind::Call {
                func,
                args,
                keywords,
            } => {
                let simple = !args.iter().any(|a| matches!(a.kind, ExprKind::Starred(_)))
                    && keywords.is_empty();
                if let (ExprKind::Attribute(obj, attr), true) = (&func.kind, simple) {
                    self.expr(obj)?;
                    let i = self.name_index(attr);
                    let p = self.set_pos(expr_pos(func));
                    self.emit(Op::LoadMethod(i));
                    self.set_pos(p);
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::CallMethod(args.len() as u32));
                } else {
                    self.expr(func)?;
                    self.call_with(args, keywords, 0)?;
                }
            }
            ExprKind::BinOp(op, l, r) => {
                self.expr(l)?;
                self.expr(r)?;
                self.emit(Op::Binary(*op));
            }
            ExprKind::UnaryOp(op, x) => {
                self.expr(x)?;
                self.emit(Op::Unary(*op));
            }
            ExprKind::BoolOp(is_and, items) => {
                let end = self.new_label();
                for (i, it) in items.iter().enumerate() {
                    self.expr(it)?;
                    if i + 1 < items.len() {
                        if *is_and {
                            self.jump(Op::JumpIfFalseOrPop, end);
                        } else {
                            self.jump(Op::JumpIfTrueOrPop, end);
                        }
                    }
                }
                self.bind(end);
            }
            ExprKind::Compare(left, ops) => {
                self.expr(left)?;
                if ops.len() == 1 {
                    self.expr(&ops[0].1)?;
                    self.emit(Op::Compare(ops[0].0));
                } else {
                    let cleanup = self.new_label();
                    let end = self.new_label();
                    for (i, (op, x)) in ops.iter().enumerate() {
                        self.expr(x)?;
                        if i + 1 < ops.len() {
                            self.emit(Op::Dup);
                            self.emit(Op::Rot3);
                            self.emit(Op::Compare(*op));
                            self.jump(Op::JumpIfFalseOrPop, cleanup);
                        } else {
                            self.emit(Op::Compare(*op));
                        }
                    }
                    self.jump(Op::Jump, end);
                    self.bind(cleanup);
                    self.emit(Op::Rot2);
                    self.emit(Op::Pop);
                    self.bind(end);
                }
            }
            ExprKind::IfExp(cond, body, orelse) => {
                let else_l = self.new_label();
                let end = self.new_label();
                self.cond_jump(cond, false, else_l)?;
                self.expr(body)?;
                self.jump(Op::Jump, end);
                self.bind(else_l);
                self.expr(orelse)?;
                self.bind(end);
            }
            ExprKind::Lambda(f) => self.make_function(f)?,
            ExprKind::ListComp(elt, gens) => self.comprehension(e, gens, CompKind::List(elt))?,
            ExprKind::SetComp(elt, gens) => self.comprehension(e, gens, CompKind::Set(elt))?,
            ExprKind::GenExp(elt, gens) => self.comprehension(e, gens, CompKind::Gen(elt))?,
            ExprKind::DictComp(k, v, gens) => self.comprehension(e, gens, CompKind::Dict(k, v))?,
            ExprKind::Yield(v) => {
                match v {
                    Some(v) => self.expr(v)?,
                    None => self.load_const(Value::None),
                }
                self.emit(Op::Yield);
            }
            ExprKind::YieldFrom(v) => {
                self.expr(v)?;
                self.emit(Op::GetYieldFromIter);
                self.load_const(Value::None);
                self.emit(Op::YieldFrom);
            }
            ExprKind::Await(v) => {
                if !self.u().in_async {
                    return Err(SyntaxErr::new(
                        "'await' outside async function",
                        e.line,
                        e.col,
                    ));
                }
                self.expr(v)?;
                self.emit(Op::GetAwaitable);
                self.load_const(Value::None);
                self.emit(Op::YieldFrom);
            }
            ExprKind::NamedExpr(t, v) => {
                self.expr(v)?;
                self.emit(Op::Dup);
                if let ExprKind::Name(n) = &t.kind {
                    self.store_name(n);
                }
            }
        }
        Ok(())
    }

    fn starred_seq(&mut self, items: &[Expr]) -> CResult<()> {
        self.emit(Op::BuildList(0));
        for i in items {
            if let ExprKind::Starred(inner) = &i.kind {
                self.expr(inner)?;
                self.emit(Op::ListExtend(1));
            } else {
                self.expr(i)?;
                self.emit(Op::ListAppend(1));
            }
        }
        Ok(())
    }

    /// Pushes each f-string part; returns how many values to join.
    fn fstring(&mut self, parts: &[FStrPart]) -> CResult<u32> {
        let mut n = 0;
        for p in parts {
            match p {
                FStrPart::Lit(s) => {
                    self.load_const(Value::str(s));
                }
                FStrPart::Expr {
                    value,
                    conversion,
                    spec,
                } => {
                    self.expr(value)?;
                    let mut flags = match conversion {
                        Some('s') => 1,
                        Some('r') => 2,
                        Some('a') => 3,
                        _ => 0,
                    };
                    if let Some(spec) = spec {
                        let k = self.fstring(spec)?;
                        self.emit(Op::BuildString(k));
                        flags |= 4;
                    }
                    self.emit(Op::FormatValue(flags));
                }
            }
            n += 1;
        }
        if n == 0 {
            self.load_const(Value::str(""));
            n = 1;
        }
        Ok(n)
    }

    fn comprehension(&mut self, e: &Expr, gens: &[Comprehension], kind: CompKind) -> CResult<()> {
        let scope = self.syms.by_node[&node_id(e)];
        let (name, is_gen) = match kind {
            CompKind::List(_) => ("<listcomp>", false),
            CompKind::Set(_) => ("<setcomp>", false),
            CompKind::Dict(..) => ("<dictcomp>", false),
            CompKind::Gen(_) => ("<genexpr>", true),
        };
        let parent_q = self.u().qualname.clone();
        let qualname: Rc<str> = if self.u().kind == ScopeKind::Module {
            name.into()
        } else {
            format!("{parent_q}.<locals>.{name}").into()
        };
        // The outermost iterator is evaluated here and passed as `.0`.
        self.expr(&gens[0].iter)?;
        self.emit(Op::GetIter);
        self.enter(scope, name.into(), qualname.clone());
        self.u().cur = expr_pos(e);
        self.u().in_async = false;
        if !is_gen {
            match kind {
                CompKind::List(_) => self.emit(Op::BuildList(0)),
                CompKind::Set(_) => self.emit(Op::BuildSet(0)),
                _ => self.emit(Op::BuildMap(0)),
            };
        }
        self.comp_loop(gens, 0, &kind, is_gen)?;
        if is_gen {
            self.load_const(Value::None);
        }
        self.emit(Op::Return);
        let code = self.leave(0, None);
        let mut code = Rc::try_unwrap(code).ok().unwrap();
        code.argcount = 1;
        code.firstlineno = e.line;
        code.is_generator = is_gen;
        let code = Rc::new(code);
        let mut flags = 0;
        if !code.freevars.is_empty() {
            for fv in code.freevars.clone().iter() {
                self.load_closure(fv);
            }
            self.emit(Op::BuildTuple(code.freevars.len() as u32));
            flags |= 8;
        }
        self.load_const(Value::Code(code));
        self.load_const(Value::Str(Rc::new(PyStr::new(qualname.to_string()))));
        // Flag 16: an inlined comprehension (PEP 709), hidden from tracebacks.
        self.emit(Op::MakeFunction(flags | if is_gen { 0 } else { 16 }));
        // Stack: iter, func -> func(iter)
        self.emit(Op::Rot2);
        self.emit(Op::Call(1));
        Ok(())
    }
    fn comp_loop(
        &mut self,
        gens: &[Comprehension],
        i: usize,
        kind: &CompKind,
        is_gen: bool,
    ) -> CResult<()> {
        let g = &gens[i];
        let start = self.new_label();
        let end = self.new_label();
        if i == 0 {
            self.emit(Op::LoadFast(0));
        } else {
            self.expr(&g.iter)?;
            self.emit(Op::GetIter);
        }
        self.bind(start);
        self.jump(Op::ForIter, end);
        self.assign(&g.target)?;
        for cond in &g.ifs {
            self.cond_jump(cond, false, start)?;
        }
        if i + 1 < gens.len() {
            self.comp_loop(gens, i + 1, kind, is_gen)?;
        } else {
            let depth = gens.len() as u32 + 1;
            match kind {
                CompKind::List(elt) => {
                    self.expr(elt)?;
                    self.emit(Op::ListAppend(depth));
                }
                CompKind::Set(elt) => {
                    self.expr(elt)?;
                    self.emit(Op::SetAdd(depth));
                }
                CompKind::Dict(k, v) => {
                    self.expr(k)?;
                    self.expr(v)?;
                    self.emit(Op::MapAdd(depth));
                }
                CompKind::Gen(elt) => {
                    self.expr(elt)?;
                    self.emit(Op::Yield);
                    self.emit(Op::Pop);
                }
            }
        }
        self.jump(Op::Jump, start);
        self.bind(end);
        Ok(())
    }

    // ---------- match statements ----------

    fn match_stmt(&mut self, subject: &Expr, cases: &[MatchCase]) -> CResult<()> {
        self.expr(subject)?;
        let end = self.new_label();
        for case in cases {
            let next = self.new_label();
            let mut fails: Vec<(Label, u32)> = vec![];
            self.emit(Op::Dup);
            self.pattern(&case.pattern, 0, &mut fails)?;
            if let Some(g) = &case.guard {
                let f = self.new_label();
                fails.push((f, 0));
                self.cond_jump(g, false, f)?;
            }
            self.emit(Op::Pop);
            self.stmts(&case.body)?;
            self.jump(Op::Jump, end);
            // Failure trampolines: pop leftover items, then try the next case.
            let mut by_depth: Vec<(Label, u32)> = fails;
            by_depth.sort_by_key(|(_, d)| std::cmp::Reverse(*d));
            for (l, d) in by_depth {
                self.bind(l);
                for _ in 0..d {
                    self.emit(Op::Pop);
                }
                self.jump(Op::Jump, next);
            }
            self.bind(next);
        }
        self.emit(Op::Pop);
        self.bind(end);
        Ok(())
    }

    /// Consumes the subject on TOS; on mismatch jumps to a trampoline that pops
    /// `extra` further items. `extra` counts items above the case's subject copy.
    fn pattern(&mut self, p: &Pattern, extra: u32, fails: &mut Vec<(Label, u32)>) -> CResult<()> {
        let fail = |c: &mut Self, fails: &mut Vec<(Label, u32)>, depth: u32| -> Label {
            let l = c.new_label();
            fails.push((l, depth));
            l
        };
        match p {
            Pattern::Capture(None) => {
                self.emit(Op::Pop);
            }
            Pattern::Capture(Some(n)) => self.store_name(n),
            Pattern::Star(_) => unreachable!(),
            Pattern::Value(e) => {
                self.expr(e)?;
                self.emit(Op::Compare(CmpOp::Eq));
                let l = fail(self, fails, extra);
                self.jump(Op::PopJumpIfFalse, l);
            }
            Pattern::Singleton(c) => {
                let v = match c {
                    Const::None => Value::None,
                    Const::True => Value::Bool(true),
                    _ => Value::Bool(false),
                };
                self.load_const(v);
                self.emit(Op::Compare(CmpOp::Is));
                let l = fail(self, fails, extra);
                self.jump(Op::PopJumpIfFalse, l);
            }
            Pattern::Sequence(items) => {
                let star = items.iter().position(|i| matches!(i, Pattern::Star(_)));
                let n = items.len() as u32;
                let spec = match star {
                    None => n,
                    Some(_) => (n - 1) | 0x8000_0000,
                };
                self.emit(Op::MatchSequence(spec));
                let l = fail(self, fails, extra + 1);
                self.jump(Op::PopJumpIfFalse, l);
                match star {
                    None => {
                        self.emit(Op::UnpackSequence(n));
                    }
                    Some(s) => {
                        let after = items.len() - s - 1;
                        self.emit(Op::UnpackEx((s as u32) | ((after as u32) << 8)));
                    }
                }
                for (k, item) in items.iter().enumerate() {
                    let remaining = n - k as u32 - 1;
                    match item {
                        Pattern::Star(Some(name)) => self.store_name(name),
                        Pattern::Star(None) => {
                            self.emit(Op::Pop);
                        }
                        other => self.pattern(other, extra + remaining, fails)?,
                    }
                }
            }
            Pattern::Mapping {
                keys,
                patterns,
                rest,
            } => {
                self.emit(Op::MatchMapping);
                let l = fail(self, fails, extra + 1);
                self.jump(Op::PopJumpIfFalse, l);
                for k in keys {
                    self.expr(k)?;
                }
                self.emit(Op::BuildTuple(keys.len() as u32));
                self.emit(Op::MatchKeys(u32::from(rest.is_some())));
                // Stack: [rest?] values_or_None
                let has_rest = u32::from(rest.is_some());
                self.emit(Op::Dup);
                self.load_const(Value::None);
                self.emit(Op::Compare(CmpOp::Is));
                let l = fail(self, fails, extra + 1 + has_rest);
                self.jump(Op::PopJumpIfTrue, l);
                self.emit(Op::UnpackSequence(keys.len() as u32));
                let n = patterns.len() as u32;
                for (k, item) in patterns.iter().enumerate() {
                    let remaining = n - k as u32 - 1;
                    self.pattern(item, extra + remaining + has_rest, fails)?;
                }
                if let Some(r) = rest {
                    self.store_name(r);
                }
            }
            Pattern::Class { cls, args, kwargs } => {
                self.expr(cls)?;
                let names: Vec<Value> = kwargs.iter().map(|(k, _)| Value::str(k)).collect();
                self.load_const(Value::tuple(names));
                self.emit(Op::MatchClass(args.len() as u32));
                self.emit(Op::Dup);
                self.load_const(Value::None);
                self.emit(Op::Compare(CmpOp::Is));
                let l = fail(self, fails, extra + 1);
                self.jump(Op::PopJumpIfTrue, l);
                let total = (args.len() + kwargs.len()) as u32;
                self.emit(Op::UnpackSequence(total));
                let all: Vec<&Pattern> = args.iter().chain(kwargs.iter().map(|(_, p)| p)).collect();
                for (k, item) in all.iter().enumerate() {
                    let remaining = total - k as u32 - 1;
                    self.pattern(item, extra + remaining, fails)?;
                }
            }
            Pattern::Or(alts) => {
                let success = self.new_label();
                for (i, alt) in alts.iter().enumerate() {
                    if i + 1 < alts.len() {
                        let mut local: Vec<(Label, u32)> = vec![];
                        self.emit(Op::Dup);
                        self.pattern(alt, 0, &mut local)?;
                        self.emit(Op::Pop);
                        self.jump(Op::Jump, success);
                        for (l, d) in local {
                            self.bind(l);
                            for _ in 0..d {
                                self.emit(Op::Pop);
                            }
                        }
                    } else {
                        self.pattern(alt, extra, fails)?;
                    }
                }
                self.bind(success);
            }
            Pattern::As(inner, name) => {
                self.emit(Op::Dup);
                self.pattern(inner, extra + 1, fails)?;
                self.store_name(name);
            }
        }
        Ok(())
    }
}

enum CompKind<'e> {
    List(&'e Expr),
    Set(&'e Expr),
    Gen(&'e Expr),
    Dict(&'e Expr, &'e Expr),
}
