//! AST -> bytecode compiler with lexical scope resolution.
//!
//! Every function gets a frame of local slots. Bindings captured by inner
//! functions live in shared cells (decided per slot when the function is
//! finished); inner functions reach them through their capture list.

use crate::ast::*;
use crate::bytecode::*;
use crate::lexer::SyntaxErr;
use crate::numconv::number_to_string;
use crate::value::{JsStr, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BKind {
    Var,
    Let,
    Const,
    /// Name of a named function expression (read-only, silently).
    FnName,
    Hidden,
}

struct Binding {
    name: Name,
    slot: u32,
    kind: BKind,
}

#[derive(Default)]
struct Scope {
    binds: Vec<Binding>,
}

impl Scope {
    fn find(&self, n: &str) -> Option<&Binding> {
        self.binds.iter().rev().find(|b| &*b.name == n)
    }
}

type Label = u32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopKind {
    Plain,
    ForOf,
    ForIn,
}

enum FBlock {
    Loop {
        brk: Label,
        cont: Label,
        labels: Vec<Name>,
        kind: LoopKind,
    },
    Switch {
        brk: Label,
        labels: Vec<Name>,
    },
    Label {
        brk: Label,
        labels: Vec<Name>,
    },
    Try,
    Finally {
        entry: Label,
    },
}

enum Res {
    Local(u32, BKind),
    Free(u32, BKind),
    Global,
}

struct FState {
    ops: Vec<Op>,
    pos: Vec<Pos>,
    consts: Vec<Value>,
    str_consts: HashMap<String, u32>,
    codes: Vec<Rc<Code>>,
    local_names: Vec<JsStr>,
    is_cell: Vec<bool>,
    captures: Vec<Capture>,
    free: Vec<(Name, BKind)>,
    scopes: Vec<Scope>,
    fblocks: Vec<FBlock>,
    labels: Vec<Option<u32>>,
    patches: Vec<usize>,
    kind: FuncKind,
    is_async: bool,
    is_generator: bool,
    strict: bool,
    this_slot: Option<u32>,
    newtarget_slot: Option<u32>,
    home_slot: Option<u32>,
    fn_slot: Option<u32>,
    args_slot: Option<u32>,
    self_name: Option<Name>,
    pending_stmt: Option<Pos>,
    cur_pos: Pos,
    opt_labels: Vec<Label>,
    run_fields: bool,
    is_top: bool,
    templates: Vec<(Vec<Option<JsStr>>, Vec<JsStr>)>,
}

impl FState {
    fn new(kind: FuncKind, strict: bool) -> FState {
        FState {
            ops: vec![],
            pos: vec![],
            consts: vec![],
            str_consts: HashMap::new(),
            codes: vec![],
            local_names: vec![],
            is_cell: vec![],
            captures: vec![],
            free: vec![],
            scopes: vec![Scope::default()],
            fblocks: vec![],
            labels: vec![],
            patches: vec![],
            kind,
            is_async: false,
            is_generator: false,
            strict,
            this_slot: None,
            newtarget_slot: None,
            home_slot: None,
            fn_slot: None,
            args_slot: None,
            self_name: None,
            pending_stmt: None,
            cur_pos: Pos::default(),
            opt_labels: vec![],
            run_fields: false,
            is_top: false,
            templates: vec![],
        }
    }
    fn is_arrow(&self) -> bool {
        self.kind == FuncKind::Arrow
    }
    fn alloc(&mut self, name: &str) -> u32 {
        self.local_names.push(JsStr::new(name));
        self.is_cell.push(false);
        (self.local_names.len() - 1) as u32
    }
}

pub struct Compiler<'a> {
    file: Rc<str>,
    src: &'a [char],
    funcs: Vec<FState>,
    /// Exported bindings of an ES module: (local, exported).
    exports: Vec<(Name, Name)>,
    /// Record top-level expression values (eval, `node -p`).
    pub completion: bool,
    /// Outermost destructuring context: (pattern position, source text).
    pat_ctx: Option<(Pos, Option<String>)>,
}

type CResult<T> = Result<T, SyntaxErr>;

fn serr(pos: Pos, len: u32, msg: impl Into<String>) -> SyntaxErr {
    SyntaxErr {
        msg: msg.into(),
        line: pos.line,
        col: pos.col,
        len,
    }
}

pub fn pattern_names(p: &Pattern, out: &mut Vec<(Name, Pos)>) {
    match p {
        Pattern::Ident(n, pos) => out.push((n.clone(), *pos)),
        Pattern::Expr(_) => {}
        Pattern::Object { props, rest } => {
            for pp in props {
                pattern_names(&pp.value.target, out);
            }
            if let Some(r) = rest {
                pattern_names(r, out);
            }
        }
        Pattern::Array { elems, rest } => {
            for e in elems.iter().flatten() {
                pattern_names(&e.target, out);
            }
            if let Some(r) = rest {
                pattern_names(r, out);
            }
        }
    }
}

/// Callee text V8 prints in "x is not a function" messages.
pub fn expr_text(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Ident(n) => n.to_string(),
        ExprKind::This => "this".into(),
        ExprKind::Num(n) => number_to_string(*n),
        ExprKind::Str(s) => format!("\"{s}\""),
        ExprKind::Null => "null".into(),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Object(_) => "{}".into(),
        ExprKind::Array(_) => "[]".into(),
        ExprKind::Member { obj, prop, .. } => {
            let o = expr_text(obj);
            match prop {
                MemberProp::Name(n, _) => format!("{o}.{n}"),
                MemberProp::Private(n, _) => format!("{o}.#{n}"),
                MemberProp::Computed(k) => match &k.kind {
                    ExprKind::Str(s) => format!("{o}.{s}"),
                    _ => format!("{o}[{}]", expr_text(k)),
                },
            }
        }
        ExprKind::SuperMember(prop) => match prop {
            MemberProp::Name(n, _) => format!("super.{n}"),
            _ => "super[...]".into(),
        },
        ExprKind::Call { callee, .. } => format!("{}(...)", expr_text(callee)),
        ExprKind::OptChain(inner) => expr_text(inner),
        ExprKind::Template { .. } => "(intermediate value)".into(),
        _ => "(intermediate value)".into(),
    }
}

fn set_target(op: &mut Op, pc: u32) {
    match op {
        Op::Jump(t)
        | Op::JumpIfFalse(t)
        | Op::JumpIfTrue(t)
        | Op::JumpIfFalseKeep(t)
        | Op::JumpIfTrueKeep(t)
        | Op::JumpIfNotNullishKeep(t)
        | Op::JumpIfNotUndefKeep(t)
        | Op::OptCheck(t, _)
        | Op::EnterTry(t, _)
        | Op::PushPc(t)
        | Op::IterNext(t)
        | Op::ForInNext(t)
        | Op::IterResult(t) => *t = pc,
        _ => panic!("not a jump: {op:?}"),
    }
}

fn is_anon_fn(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Function(f) => f.name.is_none(),
        ExprKind::Class(c) => c.name.is_none(),
        _ => false,
    }
}

impl<'a> Compiler<'a> {
    pub fn new(file: Rc<str>, src: &'a [char], _is_module: bool) -> Self {
        Compiler {
            file,
            src,
            funcs: vec![],
            exports: vec![],
            completion: false,
            pat_ctx: None,
        }
    }

    fn f(&mut self) -> &mut FState {
        self.funcs.last_mut().unwrap()
    }

    // ------------------------------------------------------------ emission
    fn emit(&mut self, op: Op) -> usize {
        let f = self.funcs.last_mut().unwrap();
        let p = f.pending_stmt.take().unwrap_or(f.cur_pos);
        f.ops.push(op);
        f.pos.push(p);
        f.ops.len() - 1
    }
    fn emit_at(&mut self, op: Op, pos: Pos) -> usize {
        let f = self.funcs.last_mut().unwrap();
        let p = f.pending_stmt.take().unwrap_or(pos);
        f.cur_pos = pos;
        f.ops.push(op);
        f.pos.push(p);
        f.ops.len() - 1
    }
    fn stmt_pos(&mut self, pos: Pos) {
        let f = self.f();
        f.pending_stmt = Some(pos);
        f.cur_pos = pos;
    }
    fn new_label(&mut self) -> Label {
        let f = self.f();
        f.labels.push(None);
        (f.labels.len() - 1) as u32
    }
    fn bind(&mut self, l: Label) {
        let f = self.f();
        f.labels[l as usize] = Some(f.ops.len() as u32);
    }
    fn emit_jump(&mut self, op: Op) -> usize {
        let i = self.emit(op);
        self.f().patches.push(i);
        i
    }
    fn str_const(&mut self, s: &str) -> u32 {
        let f = self.f();
        if let Some(&i) = f.str_consts.get(s) {
            return i;
        }
        f.consts.push(Value::Str(JsStr::new(s)));
        let i = (f.consts.len() - 1) as u32;
        f.str_consts.insert(s.to_string(), i);
        i
    }
    fn val_const(&mut self, v: Value) -> u32 {
        let f = self.f();
        f.consts.push(v);
        (f.consts.len() - 1) as u32
    }

    // ------------------------------------------------------------ scopes
    fn push_scope(&mut self) {
        self.f().scopes.push(Scope::default());
    }
    fn pop_scope(&mut self) {
        self.f().scopes.pop();
    }

    fn declare(&mut self, name: &Name, kind: BKind, pos: Pos) -> CResult<u32> {
        let f = self.funcs.last_mut().unwrap();
        let top = f.scopes.len() == 1;
        let scope = f.scopes.last().unwrap();
        if let Some(b) = scope.find(name) {
            let conflict = !matches!(
                (b.kind, kind),
                (BKind::Var, BKind::Var)
                    | (BKind::Hidden, _)
                    | (_, BKind::Hidden)
                    | (BKind::FnName, _)
            );
            if conflict {
                return Err(serr(
                    pos,
                    name.chars().count() as u32,
                    format!("Identifier '{name}' has already been declared"),
                ));
            }
            if kind == BKind::Var {
                return Ok(b.slot);
            }
            // Function redeclaration at top level (var-like) reuses the slot.
            return Ok(b.slot);
        }
        let _ = top;
        let slot = f.alloc(name);
        f.scopes.last_mut().unwrap().binds.push(Binding {
            name: name.clone(),
            slot,
            kind,
        });
        Ok(slot)
    }

    /// Declares a `var` in the function scope (scope 0).
    fn declare_var(&mut self, name: &Name, pos: Pos) -> CResult<u32> {
        let f = self.funcs.last_mut().unwrap();
        if let Some(b) = f.scopes[0].find(name) {
            if matches!(b.kind, BKind::Let | BKind::Const) {
                return Err(serr(
                    pos,
                    name.chars().count() as u32,
                    format!("Identifier '{name}' has already been declared"),
                ));
            }
            return Ok(b.slot);
        }
        let slot = f.alloc(name);
        f.scopes[0].binds.push(Binding {
            name: name.clone(),
            slot,
            kind: BKind::Var,
        });
        Ok(slot)
    }

    fn lookup_local(&mut self, fi: usize, name: &str) -> Option<(u32, BKind)> {
        let f = &mut self.funcs[fi];
        for s in f.scopes.iter().rev() {
            if let Some(b) = s.find(name) {
                return Some((b.slot, b.kind));
            }
        }
        if !f.is_arrow() {
            let special = match name {
                "this" => Some(&mut f.this_slot as *mut Option<u32>),
                "new.target" => Some(&mut f.newtarget_slot as *mut Option<u32>),
                "%home" => Some(&mut f.home_slot as *mut Option<u32>),
                "%fn" => Some(&mut f.fn_slot as *mut Option<u32>),
                "arguments" if f.kind != FuncKind::ClassInit => {
                    Some(&mut f.args_slot as *mut Option<u32>)
                }
                _ => None,
            };
            if let Some(ptr) = special {
                // SAFETY: the pointer targets a field of `f`, which is live and
                // exclusively borrowed for the duration of this block.
                let cur = unsafe { *ptr };
                let slot = match cur {
                    Some(s) => s,
                    None => {
                        let s = f.alloc(name);
                        unsafe { *ptr = Some(s) };
                        s
                    }
                };
                return Some((slot, BKind::Hidden));
            }
        }
        if f.self_name.as_deref() == Some(name) {
            let slot = match f.fn_slot {
                Some(s) => s,
                None => {
                    let s = f.alloc("%fn");
                    f.fn_slot = Some(s);
                    s
                }
            };
            return Some((slot, BKind::FnName));
        }
        None
    }

    fn resolve_in(&mut self, fi: usize, name: &str) -> Res {
        if let Some((slot, kind)) = self.lookup_local(fi, name) {
            return Res::Local(slot, kind);
        }
        if let Some(i) = self.funcs[fi].free.iter().position(|(n, _)| &**n == name) {
            return Res::Free(i as u32, self.funcs[fi].free[i].1);
        }
        if fi == 0 {
            return Res::Global;
        }
        let (cap, kind) = match self.resolve_in(fi - 1, name) {
            Res::Global => return Res::Global,
            Res::Local(slot, kind) => {
                self.funcs[fi - 1].is_cell[slot as usize] = true;
                (Capture::Local(slot), kind)
            }
            Res::Free(i, kind) => (Capture::Free(i), kind),
        };
        let f = &mut self.funcs[fi];
        f.captures.push(cap);
        f.free.push((Rc::from(name), kind));
        Res::Free((f.free.len() - 1) as u32, kind)
    }

    fn resolve(&mut self, name: &str) -> Res {
        let fi = self.funcs.len() - 1;
        self.resolve_in(fi, name)
    }

    fn load_name(&mut self, name: &str, pos: Pos) {
        match self.resolve(name) {
            Res::Local(s, _) => {
                self.emit_at(Op::Load(s), pos);
            }
            Res::Free(i, _) => {
                self.emit_at(Op::LoadFree(i), pos);
            }
            Res::Global => match name {
                "undefined" => {
                    self.emit_at(Op::Undef, pos);
                }
                "NaN" => {
                    self.emit_at(Op::Num(f64::NAN), pos);
                }
                "Infinity" => {
                    self.emit_at(Op::Num(f64::INFINITY), pos);
                }
                _ => {
                    let c = self.str_const(name);
                    self.emit_at(Op::LoadGlobal(c), pos);
                }
            },
        }
    }

    /// Stores TOS into a name (consumes it). `init` for declarations.
    fn store_name(&mut self, name: &str, pos: Pos, init: bool) {
        match self.resolve(name) {
            Res::Local(s, kind) => {
                if !init && kind == BKind::Const {
                    self.emit_at(Op::ConstAssign, pos);
                } else if !init && kind == BKind::FnName {
                    if self.f().strict {
                        self.emit_at(Op::ConstAssign, pos);
                    } else {
                        self.emit_at(Op::Pop, pos);
                    }
                } else if init {
                    self.emit_at(Op::Init(s), pos);
                } else {
                    self.emit_at(Op::Store(s), pos);
                }
            }
            Res::Free(i, kind) => {
                if !init && kind == BKind::Const {
                    self.emit_at(Op::ConstAssign, pos);
                } else if !init && kind == BKind::FnName {
                    if self.f().strict {
                        self.emit_at(Op::ConstAssign, pos);
                    } else {
                        self.emit_at(Op::Pop, pos);
                    }
                } else if init {
                    self.emit_at(Op::InitFree(i), pos);
                } else {
                    self.emit_at(Op::StoreFree(i), pos);
                }
            }
            Res::Global => {
                let c = self.str_const(name);
                self.emit_at(Op::StoreGlobal(c), pos);
            }
        }
    }

    // ------------------------------------------------------------ declarations scan
    fn collect_vars(&self, stmts: &[Stmt], out: &mut Vec<(Name, Pos)>, strict: bool, top: bool) {
        for s in stmts {
            self.collect_vars_stmt(s, out, strict, top);
        }
    }
    fn collect_vars_stmt(&self, s: &Stmt, out: &mut Vec<(Name, Pos)>, strict: bool, top: bool) {
        match &s.kind {
            StmtKind::Var(VarKind::Var, decls) => {
                for d in decls {
                    pattern_names(&d.target, out);
                }
            }
            StmtKind::If(_, a, b) => {
                self.collect_vars_stmt(a, out, strict, false);
                if let Some(b) = b {
                    self.collect_vars_stmt(b, out, strict, false);
                }
            }
            StmtKind::For { init, body, .. } => {
                if let Some(ForInit::Var(VarKind::Var, decls)) = init {
                    for d in decls {
                        pattern_names(&d.target, out);
                    }
                }
                self.collect_vars_stmt(body, out, strict, false);
            }
            StmtKind::ForIn { left, body, .. } | StmtKind::ForOf { left, body, .. } => {
                if let ForLeft::Var(VarKind::Var, p) = left {
                    pattern_names(p, out);
                }
                self.collect_vars_stmt(body, out, strict, false);
            }
            StmtKind::While(_, b) | StmtKind::DoWhile(b, _) | StmtKind::Labeled(_, b) => {
                self.collect_vars_stmt(b, out, strict, false)
            }
            StmtKind::Block(b) => self.collect_vars(b, out, strict, false),
            StmtKind::Try {
                block,
                handler,
                finalizer,
                ..
            } => {
                self.collect_vars(block, out, strict, false);
                if let Some(h) = handler {
                    self.collect_vars(h, out, strict, false);
                }
                if let Some(f) = finalizer {
                    self.collect_vars(f, out, strict, false);
                }
            }
            StmtKind::Switch(_, cases) => {
                for c in cases {
                    self.collect_vars(&c.body, out, strict, false);
                }
            }
            StmtKind::Func(f) if !top && !strict => {
                // Annex B: block-level functions also get a var binding.
                if let Some(n) = &f.name {
                    out.push((n.clone(), f.pos));
                }
            }
            StmtKind::Export(ExportKind::Decl(d) | ExportKind::DefaultDecl(d)) => {
                self.collect_vars_stmt(d, out, strict, top)
            }
            _ => {}
        }
    }

    /// Declares the lexical bindings of a block and hoists its functions.
    fn hoist_block(&mut self, stmts: &[Stmt], fn_top: bool) -> CResult<()> {
        let mut lex: Vec<(Name, Pos, BKind)> = vec![];
        let mut funcs: Vec<&Rc<Func>> = vec![];
        for s in stmts {
            let s = match &s.kind {
                StmtKind::Export(ExportKind::Decl(d) | ExportKind::DefaultDecl(d)) => d,
                _ => s,
            };
            match &s.kind {
                StmtKind::Var(k @ (VarKind::Let | VarKind::Const), decls) => {
                    let mut names = vec![];
                    for d in decls {
                        pattern_names(&d.target, &mut names);
                    }
                    for (n, p) in names {
                        lex.push((
                            n,
                            p,
                            if *k == VarKind::Const {
                                BKind::Const
                            } else {
                                BKind::Let
                            },
                        ));
                    }
                }
                StmtKind::Class(c) => {
                    if let Some(n) = &c.name {
                        lex.push((n.clone(), c.pos, BKind::Let));
                    }
                }
                StmtKind::Func(f) => funcs.push(f),
                _ => {}
            }
        }
        for (n, p, k) in lex {
            if fn_top {
                // Conflict with params / vars in the function scope.
                if let Some(b) = self.f().scopes.last().unwrap().find(&n) {
                    if b.kind == BKind::Var {
                        return Err(serr(
                            p,
                            n.chars().count() as u32,
                            format!("Identifier '{n}' has already been declared"),
                        ));
                    }
                }
            }
            let slot = self.declare(&n, k, p)?;
            self.emit(Op::DeclLet(slot));
        }
        // Declare every function binding before compiling any body, so
        // functions can reference ones declared later in the block.
        let mut slots = vec![];
        for f in &funcs {
            let name = f.name.clone().unwrap();
            let slot = if fn_top {
                self.declare_var(&name, f.pos)?
            } else {
                let existing = self.f().scopes.last().unwrap().find(&name).map(|b| b.slot);
                match existing {
                    Some(s) => s,
                    None => self.declare(&name, BKind::Let, f.pos)?,
                }
            };
            slots.push(slot);
        }
        for (f, slot) in funcs.into_iter().zip(slots) {
            let name = f.name.clone().unwrap();
            let idx = self.compile_function(f, Some(name.as_ref()))?;
            self.emit(Op::Closure(idx));
            if !fn_top && !self.f().strict {
                // Annex B var binding.
                if let Some(b) = self.f().scopes[0].find(&name) {
                    if b.kind == BKind::Var {
                        let vs = b.slot;
                        self.emit(Op::Dup);
                        self.emit(Op::Init(vs));
                    }
                }
            }
            self.emit(Op::Init(slot));
        }
        Ok(())
    }

    // ------------------------------------------------------------ program
    pub fn compile_program(
        &mut self,
        prog: &Program,
        params: &[&str],
        is_async: bool,
    ) -> CResult<Rc<Code>> {
        let mut fs = FState::new(FuncKind::Normal, prog.strict || prog.is_module);
        fs.is_top = true;
        fs.is_async = is_async;
        self.funcs.push(fs);
        for p in params {
            let n: Name = Rc::from(*p);
            self.declare(&n, BKind::Var, Pos::default())?;
        }
        let nparams = params.len() as u32;
        // ES module imports are hoisted.
        if prog.is_module {
            self.compile_imports(&prog.body)?;
        }
        self.compile_body(&prog.body)?;
        if prog.is_module {
            self.compile_exports(&prog.body)?;
        }
        if self.completion {
            self.emit(Op::TakeCompletion);
        } else {
            self.emit(Op::Undef);
        }
        self.emit(Op::Return);
        let fs = self.funcs.pop().unwrap();
        let src: String = self.src.iter().collect();
        Ok(self.finish(fs, JsStr::new(""), Some(nparams), 0, Rc::from(src.as_str())))
    }

    /// Indirect eval / `new Function` bodies: global code returning its
    /// completion value.
    pub fn compile_eval(&mut self, prog: &Program) -> CResult<Rc<Code>> {
        self.completion = true;
        self.compile_program(prog, &[], false)
    }

    fn compile_body(&mut self, body: &[Stmt]) -> CResult<()> {
        let strict = self.f().strict;
        let mut vars = vec![];
        self.collect_vars(body, &mut vars, strict, true);
        for (n, p) in vars {
            self.declare_var(&n, p)?;
        }
        self.hoist_block(body, true)?;
        for s in body {
            self.compile_stmt(s)?;
        }
        Ok(())
    }

    fn finish(
        &mut self,
        mut fs: FState,
        name: JsStr,
        simple: Option<u32>,
        length: u32,
        source: Rc<str>,
    ) -> Rc<Code> {
        // Resolve labels.
        for &i in &fs.patches {
            let l = match fs.ops[i] {
                Op::Jump(t)
                | Op::JumpIfFalse(t)
                | Op::JumpIfTrue(t)
                | Op::JumpIfFalseKeep(t)
                | Op::JumpIfTrueKeep(t)
                | Op::JumpIfNotNullishKeep(t)
                | Op::JumpIfNotUndefKeep(t)
                | Op::OptCheck(t, _)
                | Op::EnterTry(t, _)
                | Op::PushPc(t)
                | Op::IterNext(t)
                | Op::ForInNext(t)
                | Op::IterResult(t) => t,
                _ => unreachable!(),
            };
            let pc = fs.labels[l as usize].expect("unbound label");
            set_target(&mut fs.ops[i], pc);
        }
        let ntemplates = fs.templates.len();
        Rc::new(Code {
            name,
            ops: fs.ops,
            pos: fs.pos,
            consts: fs.consts,
            codes: fs.codes,
            nlocals: fs.local_names.len() as u32,
            local_names: fs.local_names,
            is_cell: fs.is_cell,
            captures: fs.captures,
            free_names: fs.free.iter().map(|(n, _)| JsStr::new(&**n)).collect(),
            simple_params: simple,
            length,
            this_slot: fs.this_slot,
            newtarget_slot: fs.newtarget_slot,
            home_slot: fs.home_slot,
            fn_slot: fs.fn_slot,
            args_slot: fs.args_slot,
            kind: fs.kind,
            is_async: fs.is_async,
            is_generator: fs.is_generator,
            strict: fs.strict,
            file: self.file.clone(),
            source,
            template_cache: RefCell::new(vec![None; ntemplates]),
            templates: fs.templates,
            is_top: fs.is_top,
            needs_args: false,
        })
    }

    fn source_of(&self, start: usize, end: usize) -> Rc<str> {
        let end = end.min(self.src.len());
        let start = start.min(end);
        let s: String = self.src[start..end].iter().collect();
        Rc::from(s.as_str())
    }

    // ------------------------------------------------------------ functions
    /// Compiles a function; returns its index in the current code's table.
    fn compile_function(&mut self, f: &Func, name: Option<&str>) -> CResult<u32> {
        let code = self.compile_function_code(f, name, false)?;
        let fs = self.f();
        fs.codes.push(code);
        Ok((fs.codes.len() - 1) as u32)
    }

    fn compile_function_code(
        &mut self,
        f: &Func,
        name: Option<&str>,
        run_fields: bool,
    ) -> CResult<Rc<Code>> {
        let mut fs = FState::new(f.kind, f.strict);
        fs.is_async = f.is_async;
        fs.is_generator = f.is_generator;
        fs.run_fields = run_fields;
        if f.kind == FuncKind::Normal {
            fs.self_name = f.name.clone();
        }
        let fname = f.name.as_deref().or(name).unwrap_or("");
        self.funcs.push(fs);
        let r = self.compile_function_inner(f);
        let fs = self.funcs.pop().unwrap();
        let (simple, length) = r?;
        let source = self.source_of(f.src_start, f.src_end);
        Ok(self.finish(fs, JsStr::new(fname), simple, length, source))
    }

    fn compile_function_inner(&mut self, f: &Func) -> CResult<(Option<u32>, u32)> {
        let simple = f.rest.is_none()
            && f.params
                .iter()
                .all(|p| p.default.is_none() && matches!(p.target, Pattern::Ident(..)));
        let mut length = 0u32;
        for p in &f.params {
            if p.default.is_some() {
                break;
            }
            length += 1;
        }
        let simple_n = if simple {
            for p in &f.params {
                if let Pattern::Ident(n, pos) = &p.target {
                    // Duplicate simple params (sloppy) share the later slot.
                    let slot = self.f().alloc(n);
                    let f2 = self.f();
                    f2.scopes[0].binds.push(Binding {
                        name: n.clone(),
                        slot,
                        kind: BKind::Var,
                    });
                    let _ = pos;
                }
            }
            Some(f.params.len() as u32)
        } else {
            // Declare all parameter names first.
            let mut names = vec![];
            for p in &f.params {
                pattern_names(&p.target, &mut names);
            }
            if let Some(r) = &f.rest {
                pattern_names(r, &mut names);
            }
            for (n, p) in &names {
                self.declare_var(n, *p)?;
            }
            for (i, p) in f.params.iter().enumerate() {
                self.stmt_pos(f.pos);
                self.emit(Op::Arg(i as u32));
                if let Some(d) = &p.default {
                    let skip = self.new_label();
                    self.emit_jump(Op::JumpIfNotUndefKeep(skip));
                    let hint = if let Pattern::Ident(n, _) = &p.target {
                        Some(n.clone())
                    } else {
                        None
                    };
                    self.compile_expr_named(d, hint.as_deref())?;
                    self.bind(skip);
                }
                let ppos = pattern_pos(&p.target).unwrap_or(f.pos);
                self.bind_top(&p.target, true, ppos, None)?;
            }
            if let Some(r) = &f.rest {
                self.emit(Op::RestParam(f.params.len() as u32));
                self.bind_pattern(r, true)?;
            }
            None
        };
        if f.kind == FuncKind::BaseConstructor && self.f().run_fields {
            self.emit_run_fields();
        }
        match &f.body {
            FuncBody::Block(body) => {
                self.compile_body(body)?;
                self.emit(Op::Undef);
                self.emit(Op::Return);
            }
            FuncBody::Expr(e) => {
                self.stmt_pos(e.pos);
                self.compile_expr(e)?;
                self.emit(Op::Return);
            }
        }
        Ok((simple_n, length))
    }

    fn emit_run_fields(&mut self) {
        let p = self.f().cur_pos;
        self.load_name("%fn", p);
        self.load_name("this", p);
        self.emit(Op::RunFields);
    }

    // ------------------------------------------------------------ statements
    fn compile_stmts(&mut self, stmts: &[Stmt]) -> CResult<()> {
        for s in stmts {
            self.compile_stmt(s)?;
        }
        Ok(())
    }

    fn compile_block(&mut self, stmts: &[Stmt]) -> CResult<()> {
        let needs_scope = stmts.iter().any(|s| {
            matches!(
                s.kind,
                StmtKind::Var(VarKind::Let | VarKind::Const, _)
                    | StmtKind::Class(_)
                    | StmtKind::Func(_)
            )
        });
        if needs_scope {
            self.push_scope();
            self.hoist_block(stmts, false)?;
        }
        let r = self.compile_stmts(stmts);
        if needs_scope {
            self.pop_scope();
        }
        r
    }

    fn compile_stmt(&mut self, s: &Stmt) -> CResult<()> {
        self.stmt_pos(s.pos);
        match &s.kind {
            StmtKind::Expr(e) => {
                if self.completion && self.funcs.len() == 1 {
                    self.compile_expr(e)?;
                    self.emit(Op::SetCompletion);
                } else {
                    self.compile_expr_stmt(e)?
                }
            }
            StmtKind::Var(kind, decls) => {
                for d in decls {
                    let init = *kind != VarKind::Var;
                    match &d.init {
                        Some(e) => {
                            let hint = if let Pattern::Ident(n, _) = &d.target {
                                Some(n.clone())
                            } else {
                                None
                            };
                            self.compile_expr_named(e, hint.as_deref())?;
                            self.pat_ctx = Some((d.pos, Some(expr_text(e))));
                            self.bind_pattern(&d.target, true)?;
                        }
                        None => {
                            if init {
                                self.emit(Op::Undef);
                                self.pat_ctx = Some((d.pos, None));
                                self.bind_pattern(&d.target, true)?;
                            }
                        }
                    }
                }
            }
            StmtKind::Func(_) => {} // hoisted
            StmtKind::Class(c) => {
                self.compile_class(c, None)?;
                let n = c.name.clone().unwrap();
                self.store_name(&n, c.pos, true);
            }
            StmtKind::Return(arg) => {
                match arg {
                    Some(e) => self.compile_expr(e)?,
                    None => {
                        self.emit(Op::Undef);
                    }
                }
                let unwind = self.f().fblocks.iter().any(|b| {
                    matches!(
                        b,
                        FBlock::Finally { .. }
                            | FBlock::Loop {
                                kind: LoopKind::ForOf,
                                ..
                            }
                    )
                });
                self.emit(if unwind { Op::ReturnUnwind } else { Op::Return });
            }
            StmtKind::If(test, cons, alt) => {
                self.compile_expr(test)?;
                let else_l = self.new_label();
                self.emit_jump(Op::JumpIfFalse(else_l));
                self.compile_stmt(cons)?;
                match alt {
                    Some(a) => {
                        let end = self.new_label();
                        self.emit_jump(Op::Jump(end));
                        self.bind(else_l);
                        self.compile_stmt(a)?;
                        self.bind(end);
                    }
                    None => self.bind(else_l),
                }
            }
            StmtKind::Block(b) => self.compile_block(b)?,
            StmtKind::Empty | StmtKind::Debugger => {}
            StmtKind::Throw(e) => {
                self.compile_expr(e)?;
                self.emit_at(Op::Throw, s.pos);
            }
            StmtKind::While(test, body) => self.compile_while(test, body, vec![])?,
            StmtKind::DoWhile(body, test) => self.compile_do_while(body, test, vec![])?,
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => self.compile_for(init, test, update, body, vec![])?,
            StmtKind::ForOf {
                left,
                right,
                body,
                is_await,
            } => self.compile_for_of(left, right, body, *is_await, vec![])?,
            StmtKind::ForIn { left, right, body } => {
                self.compile_for_in(left, right, body, vec![])?
            }
            StmtKind::Labeled(..) => {
                let mut labels = vec![];
                let mut cur = s;
                while let StmtKind::Labeled(l, inner) = &cur.kind {
                    labels.push(l.clone());
                    cur = inner;
                }
                self.stmt_pos(cur.pos);
                match &cur.kind {
                    StmtKind::While(t, b) => self.compile_while(t, b, labels)?,
                    StmtKind::DoWhile(b, t) => self.compile_do_while(b, t, labels)?,
                    StmtKind::For {
                        init,
                        test,
                        update,
                        body,
                    } => self.compile_for(init, test, update, body, labels)?,
                    StmtKind::ForOf {
                        left,
                        right,
                        body,
                        is_await,
                    } => self.compile_for_of(left, right, body, *is_await, labels)?,
                    StmtKind::ForIn { left, right, body } => {
                        self.compile_for_in(left, right, body, labels)?
                    }
                    _ => {
                        let brk = self.new_label();
                        self.f().fblocks.push(FBlock::Label { brk, labels });
                        self.compile_stmt(cur)?;
                        self.f().fblocks.pop();
                        self.bind(brk);
                    }
                }
            }
            StmtKind::Break(label) => self.compile_break(label.as_deref(), false, s.pos)?,
            StmtKind::Continue(label) => self.compile_break(label.as_deref(), true, s.pos)?,
            StmtKind::Try {
                block,
                param,
                handler,
                finalizer,
            } => self.compile_try(
                block,
                param.as_ref(),
                handler.as_deref(),
                finalizer.as_deref(),
            )?,
            StmtKind::Switch(d, cases) => self.compile_switch(d, cases)?,
            StmtKind::Import(..) => {}
            StmtKind::Export(k) => match k {
                ExportKind::Decl(d) | ExportKind::DefaultDecl(d) => self.compile_stmt(d)?,
                ExportKind::Default(e) => {
                    self.compile_expr_named(e, Some("default"))?;
                    let n: Name = Rc::from("*default*");
                    self.store_name(&n, s.pos, true);
                }
                _ => {}
            },
        }
        Ok(())
    }

    fn compile_expr_stmt(&mut self, e: &Expr) -> CResult<()> {
        // Assignment / update statements without keeping the value.
        match &e.kind {
            ExprKind::Assign {
                op: AssignOp::Assign,
                target,
                value,
            } if matches!(**target, Pattern::Ident(..)) => {
                let Pattern::Ident(n, _) = &**target else {
                    unreachable!()
                };
                self.compile_expr_named(value, Some(n))?;
                self.store_name(n, e.pos, false);
                return Ok(());
            }
            ExprKind::Update { inc, target, .. } if matches!(target.kind, ExprKind::Ident(_)) => {
                let ExprKind::Ident(n) = &target.kind else {
                    unreachable!()
                };
                self.load_name(n, target.pos);
                self.emit(if *inc { Op::Inc } else { Op::Dec });
                self.store_name(n, target.pos, false);
                return Ok(());
            }
            _ => {}
        }
        self.compile_expr(e)?;
        self.emit(Op::Pop);
        Ok(())
    }

    fn compile_while(&mut self, test: &Expr, body: &Stmt, labels: Vec<Name>) -> CResult<()> {
        let head = self.new_label();
        let brk = self.new_label();
        self.bind(head);
        self.compile_expr(test)?;
        self.emit_jump(Op::JumpIfFalse(brk));
        self.f().fblocks.push(FBlock::Loop {
            brk,
            cont: head,
            labels,
            kind: LoopKind::Plain,
        });
        self.compile_stmt(body)?;
        self.f().fblocks.pop();
        self.emit_jump(Op::Jump(head));
        self.bind(brk);
        Ok(())
    }

    fn compile_do_while(&mut self, body: &Stmt, test: &Expr, labels: Vec<Name>) -> CResult<()> {
        let head = self.new_label();
        let cont = self.new_label();
        let brk = self.new_label();
        self.bind(head);
        self.f().fblocks.push(FBlock::Loop {
            brk,
            cont,
            labels,
            kind: LoopKind::Plain,
        });
        self.compile_stmt(body)?;
        self.f().fblocks.pop();
        self.bind(cont);
        self.stmt_pos(test.pos);
        self.compile_expr(test)?;
        self.emit_jump(Op::JumpIfTrue(head));
        self.bind(brk);
        Ok(())
    }

    fn compile_for(
        &mut self,
        init: &Option<ForInit>,
        test: &Option<Expr>,
        update: &Option<Expr>,
        body: &Stmt,
        labels: Vec<Name>,
    ) -> CResult<()> {
        let mut scoped = false;
        let mut let_slots = vec![];
        match init {
            Some(ForInit::Var(kind, decls)) if *kind != VarKind::Var => {
                scoped = true;
                self.push_scope();
                let mut names = vec![];
                for d in decls {
                    pattern_names(&d.target, &mut names);
                }
                for (n, p) in names {
                    let slot = self.declare(
                        &n,
                        if *kind == VarKind::Const {
                            BKind::Const
                        } else {
                            BKind::Let
                        },
                        p,
                    )?;
                    self.emit(Op::DeclLet(slot));
                    let_slots.push(slot);
                }
                for d in decls {
                    match &d.init {
                        Some(e) => {
                            let hint = if let Pattern::Ident(n, _) = &d.target {
                                Some(n.clone())
                            } else {
                                None
                            };
                            self.compile_expr_named(e, hint.as_deref())?;
                        }
                        None => {
                            self.emit(Op::Undef);
                        }
                    }
                    self.bind_pattern(&d.target, true)?;
                }
            }
            Some(ForInit::Var(_, decls)) => {
                for d in decls {
                    if let Some(e) = &d.init {
                        let hint = if let Pattern::Ident(n, _) = &d.target {
                            Some(n.clone())
                        } else {
                            None
                        };
                        self.compile_expr_named(e, hint.as_deref())?;
                        self.bind_pattern(&d.target, true)?;
                    }
                }
            }
            Some(ForInit::Expr(e)) => self.compile_expr_stmt(e)?,
            None => {}
        }
        let head = self.new_label();
        let cont = self.new_label();
        let brk = self.new_label();
        self.bind(head);
        if let Some(t) = test {
            self.stmt_pos(t.pos);
            self.compile_expr(t)?;
            self.emit_jump(Op::JumpIfFalse(brk));
        }
        self.f().fblocks.push(FBlock::Loop {
            brk,
            cont,
            labels,
            kind: LoopKind::Plain,
        });
        self.compile_stmt(body)?;
        self.f().fblocks.pop();
        self.bind(cont);
        for s in &let_slots {
            self.emit(Op::CopyCell(*s));
        }
        if let Some(u) = update {
            self.stmt_pos(u.pos);
            self.compile_expr_stmt(u)?;
        }
        self.emit_jump(Op::Jump(head));
        self.bind(brk);
        if scoped {
            self.pop_scope();
        }
        Ok(())
    }

    /// Binds the loop variable of for-in/of from TOS (consumed).
    fn bind_for_left(&mut self, left: &ForLeft) -> CResult<bool> {
        let cp = self.f().cur_pos;
        match left {
            ForLeft::Var(VarKind::Var, p) => {
                self.bind_top(p, true, cp, None)?;
                Ok(false)
            }
            ForLeft::Var(kind, p) => {
                self.push_scope();
                let mut names = vec![];
                pattern_names(p, &mut names);
                // Declare, then initialise (value is on the stack).
                for (n, pos) in names {
                    let slot = self.declare(
                        &n,
                        if *kind == VarKind::Const {
                            BKind::Const
                        } else {
                            BKind::Let
                        },
                        pos,
                    )?;
                    self.emit(Op::DeclLet(slot));
                }
                self.bind_top(p, true, cp, None)?;
                Ok(true)
            }
            ForLeft::Pattern(p) => {
                self.bind_top(p, false, cp, None)?;
                Ok(false)
            }
        }
    }

    fn compile_for_of(
        &mut self,
        left: &ForLeft,
        right: &Expr,
        body: &Stmt,
        is_await: bool,
        labels: Vec<Name>,
    ) -> CResult<()> {
        self.compile_expr(right)?;
        let text = self.str_const(&expr_text(right));
        self.emit_at(
            if is_await {
                Op::GetAsyncIter(text)
            } else {
                Op::GetIter(text)
            },
            right.pos,
        );
        let handler = self.new_label();
        let head = self.new_label();
        let cont = self.new_label();
        let brk = self.new_label();
        let done = self.new_label();
        let end = self.new_label();
        self.emit_jump(Op::EnterTry(handler, true));
        self.bind(head);
        if is_await {
            self.emit(Op::AsyncIterNext);
            self.emit(Op::Await);
            self.emit_jump(Op::IterResult(done));
            self.emit(Op::Await);
        } else {
            self.emit_jump(Op::IterNext(done));
        }
        self.f().fblocks.push(FBlock::Loop {
            brk,
            cont,
            labels,
            kind: LoopKind::ForOf,
        });
        let scoped = self.bind_for_left(left)?;
        self.compile_stmt(body)?;
        if scoped {
            self.pop_scope();
        }
        self.f().fblocks.pop();
        self.bind(cont);
        self.emit_jump(Op::Jump(head));
        self.bind(brk);
        self.emit(Op::ExitTry);
        self.emit(Op::IterClose);
        self.emit_jump(Op::Jump(end));
        self.bind(done);
        self.emit(Op::ExitTry);
        self.emit(Op::Pop);
        self.emit(Op::Pop);
        self.emit_jump(Op::Jump(end));
        self.bind(handler);
        self.emit(Op::IterCloseCompletion);
        self.bind(end);
        Ok(())
    }

    fn compile_for_in(
        &mut self,
        left: &ForLeft,
        right: &Expr,
        body: &Stmt,
        labels: Vec<Name>,
    ) -> CResult<()> {
        self.compile_expr(right)?;
        self.emit(Op::ForInPrep);
        let head = self.new_label();
        let brk = self.new_label();
        let done = self.new_label();
        self.bind(head);
        self.emit_jump(Op::ForInNext(done));
        self.f().fblocks.push(FBlock::Loop {
            brk,
            cont: head,
            labels,
            kind: LoopKind::ForIn,
        });
        let scoped = self.bind_for_left(left)?;
        self.compile_stmt(body)?;
        if scoped {
            self.pop_scope();
        }
        self.f().fblocks.pop();
        self.emit_jump(Op::Jump(head));
        self.bind(brk);
        self.bind(done);
        self.emit(Op::Pop);
        Ok(())
    }

    fn compile_break(&mut self, label: Option<&str>, is_continue: bool, pos: Pos) -> CResult<()> {
        // Find the target fblock.
        let n = self.f().fblocks.len();
        let mut target = None;
        for i in (0..n).rev() {
            let hit = match &self.f().fblocks[i] {
                FBlock::Loop { labels, .. } => match label {
                    None => true,
                    Some(l) => labels.iter().any(|x| &**x == l),
                },
                FBlock::Switch { labels, .. } => match label {
                    None => !is_continue,
                    Some(l) => !is_continue && labels.iter().any(|x| &**x == l),
                },
                FBlock::Label { labels, .. } => match label {
                    None => false,
                    Some(l) => !is_continue && labels.iter().any(|x| &**x == l),
                },
                _ => false,
            };
            if hit {
                target = Some(i);
                break;
            }
        }
        let Some(t) = target else {
            return Err(serr(
                pos,
                1,
                if is_continue {
                    "Illegal continue statement"
                } else {
                    "Illegal break statement"
                },
            ));
        };
        for i in (t + 1..n).rev() {
            enum Act {
                ExitTry,
                Finally(Label),
                ForOf,
                Pop,
                Nothing,
            }
            let act = match &self.f().fblocks[i] {
                FBlock::Try => Act::ExitTry,
                FBlock::Finally { entry } => Act::Finally(*entry),
                FBlock::Loop {
                    kind: LoopKind::ForOf,
                    ..
                } => Act::ForOf,
                FBlock::Loop {
                    kind: LoopKind::ForIn,
                    ..
                }
                | FBlock::Switch { .. } => Act::Pop,
                _ => Act::Nothing,
            };
            match act {
                Act::ExitTry => {
                    self.emit(Op::ExitTry);
                }
                Act::Finally(entry) => {
                    self.emit(Op::ExitTry);
                    let resume = self.new_label();
                    self.emit_jump(Op::PushPc(resume));
                    self.emit(Op::Num(3.0));
                    self.emit_jump(Op::Jump(entry));
                    self.bind(resume);
                }
                Act::ForOf => {
                    self.emit(Op::ExitTry);
                    self.emit(Op::IterClose);
                }
                Act::Pop => {
                    self.emit(Op::Pop);
                }
                Act::Nothing => {}
            }
        }
        let dest = match &self.f().fblocks[t] {
            FBlock::Loop { brk, cont, .. } => {
                if is_continue {
                    *cont
                } else {
                    *brk
                }
            }
            FBlock::Switch { brk, .. } | FBlock::Label { brk, .. } => *brk,
            _ => unreachable!(),
        };
        self.emit_jump(Op::Jump(dest));
        Ok(())
    }

    fn compile_try(
        &mut self,
        block: &[Stmt],
        param: Option<&Pattern>,
        handler: Option<&[Stmt]>,
        finalizer: Option<&[Stmt]>,
    ) -> CResult<()> {
        let mut fin = None;
        if finalizer.is_some() {
            let kind_slot = self.f().alloc("%kind");
            let val_slot = self.f().alloc("%val");
            let entry = self.new_label();
            self.emit_jump(Op::EnterTry(entry, true));
            self.f().fblocks.push(FBlock::Finally { entry });
            fin = Some((kind_slot, val_slot, entry));
        }
        if let Some(h) = handler {
            let catch_l = self.new_label();
            let after = self.new_label();
            self.emit_jump(Op::EnterTry(catch_l, false));
            self.f().fblocks.push(FBlock::Try);
            self.compile_block(block)?;
            self.f().fblocks.pop();
            self.emit(Op::ExitTry);
            self.emit_jump(Op::Jump(after));
            self.bind(catch_l);
            self.push_scope();
            match param {
                Some(p) => {
                    let mut names = vec![];
                    pattern_names(p, &mut names);
                    for (n, pos) in names {
                        let slot = self.declare(&n, BKind::Let, pos)?;
                        self.emit(Op::DeclLet(slot));
                    }
                    let cp = self.f().cur_pos;
                    self.bind_top(p, true, cp, None)?;
                }
                None => {
                    self.emit(Op::Pop);
                }
            }
            self.compile_block(h)?;
            self.pop_scope();
            self.bind(after);
        } else {
            self.compile_block(block)?;
        }
        if let Some((kind_slot, val_slot, entry)) = fin {
            self.f().fblocks.pop();
            self.emit(Op::ExitTry);
            self.emit(Op::Undef);
            self.emit(Op::Num(0.0));
            self.bind(entry);
            self.emit(Op::Init(kind_slot));
            self.emit(Op::Init(val_slot));
            self.compile_block(finalizer.unwrap())?;
            self.emit(Op::EndFinally(kind_slot, val_slot));
        }
        Ok(())
    }

    fn compile_switch(&mut self, d: &Expr, cases: &[Case]) -> CResult<()> {
        self.compile_expr(d)?;
        let brk = self.new_label();
        self.push_scope();
        let all: Vec<&Stmt> = cases.iter().flat_map(|c| c.body.iter()).collect();
        // Hoist lexical declarations of all cases.
        let mut lex_stmts: Vec<&Stmt> = vec![];
        for s in &all {
            if matches!(
                s.kind,
                StmtKind::Var(VarKind::Let | VarKind::Const, _)
                    | StmtKind::Class(_)
                    | StmtKind::Func(_)
            ) {
                lex_stmts.push(s);
            }
        }
        self.hoist_refs(&lex_stmts)?;
        let labels: Vec<Label> = cases.iter().map(|_| self.new_label()).collect();
        let mut default = None;
        for (i, c) in cases.iter().enumerate() {
            match &c.test {
                Some(t) => {
                    self.emit(Op::Dup);
                    self.stmt_pos(t.pos);
                    self.compile_expr(t)?;
                    self.emit(Op::StrictEq);
                    self.emit_jump(Op::JumpIfTrue(labels[i]));
                }
                None => default = Some(i),
            }
        }
        match default {
            Some(i) => {
                self.emit_jump(Op::Jump(labels[i]));
            }
            None => {
                self.emit_jump(Op::Jump(brk));
            }
        }
        self.f().fblocks.push(FBlock::Switch {
            brk,
            labels: vec![],
        });
        for (i, c) in cases.iter().enumerate() {
            self.bind(labels[i]);
            self.compile_stmts(&c.body)?;
        }
        self.f().fblocks.pop();
        self.bind(brk);
        self.emit(Op::Pop);
        self.pop_scope();
        Ok(())
    }

    fn hoist_refs(&mut self, stmts: &[&Stmt]) -> CResult<()> {
        // Same as hoist_block for a list of references.
        for s in stmts {
            match &s.kind {
                StmtKind::Var(k @ (VarKind::Let | VarKind::Const), decls) => {
                    let mut names = vec![];
                    for d in decls {
                        pattern_names(&d.target, &mut names);
                    }
                    for (n, p) in names {
                        let slot = self.declare(
                            &n,
                            if *k == VarKind::Const {
                                BKind::Const
                            } else {
                                BKind::Let
                            },
                            p,
                        )?;
                        self.emit(Op::DeclLet(slot));
                    }
                }
                StmtKind::Class(c) => {
                    if let Some(n) = &c.name {
                        let slot = self.declare(n, BKind::Let, c.pos)?;
                        self.emit(Op::DeclLet(slot));
                    }
                }
                _ => {}
            }
        }
        for s in stmts {
            if let StmtKind::Func(f) = &s.kind {
                let name = f.name.clone().unwrap();
                let slot = self.declare(&name, BKind::Let, f.pos)?;
                let idx = self.compile_function(f, Some(&name))?;
                self.emit(Op::Closure(idx));
                self.emit(Op::Init(slot));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ patterns
    /// Binds TOS to a pattern (consumes it).
    /// Binds TOS to the outermost pattern of a declaration / parameter /
    /// assignment (enables V8's "Cannot destructure" checks).
    fn bind_top(&mut self, p: &Pattern, init: bool, pos: Pos, text: Option<String>) -> CResult<()> {
        self.pat_ctx = Some((pattern_pos(p).map(|_| pos).unwrap_or(pos), text));
        self.bind_pattern(p, init)
    }

    fn bind_pattern(&mut self, p: &Pattern, init: bool) -> CResult<()> {
        match p {
            Pattern::Ident(n, pos) => {
                self.pat_ctx = None;
                self.store_name(n, *pos, init);
            }
            Pattern::Expr(e) => match &e.kind {
                ExprKind::Member { obj, prop, .. } => {
                    self.compile_expr(obj)?;
                    match prop {
                        MemberProp::Name(n, p) => {
                            self.emit(Op::Swap);
                            let c = self.str_const(n);
                            self.emit_at(Op::SetProp(c), *p);
                        }
                        MemberProp::Computed(k) => {
                            self.compile_expr(k)?;
                            self.emit(Op::Rot3);
                            self.emit(Op::Rot3);
                            self.emit_at(Op::SetElem, k.pos);
                        }
                        MemberProp::Private(n, p) => {
                            self.load_name(&format!("#{n}"), *p);
                            self.emit(Op::Rot3);
                            self.emit(Op::Rot3);
                            self.emit_at(Op::SetPrivate, *p);
                        }
                    }
                    self.emit(Op::Pop);
                }
                ExprKind::SuperMember(prop) => {
                    let p = e.pos;
                    self.load_name("this", p);
                    self.load_name("%home", p);
                    match prop {
                        MemberProp::Name(n, _) => {
                            self.emit(Op::Rot3);
                            self.emit(Op::Rot3);
                            let c = self.str_const(n);
                            self.emit(Op::SuperSet(c));
                        }
                        MemberProp::Computed(k) => {
                            self.compile_expr(k)?;
                            self.emit(Op::Rot4);
                            self.emit(Op::Rot4);
                            self.emit(Op::Rot4);
                            self.emit(Op::SuperSetElem);
                        }
                        MemberProp::Private(..) => {}
                    }
                    self.emit(Op::Pop);
                }
                _ => return Err(serr(e.pos, 1, "Invalid left-hand side in assignment")),
            },
            Pattern::Object { props, rest } => {
                // Only the outermost pattern reports "Cannot destructure";
                // nested ones fail on the property read like V8.
                if let Some((top_pos, _)) = self.pat_ctx.take() {
                    let first = match props.first().map(|p| &p.key) {
                        Some(PropKey::Lit(k)) => self.str_const(k),
                        _ => u32::MAX,
                    };
                    let first_pos = match props.first() {
                        Some(pp) => pattern_pos(&pp.value.target).unwrap_or(top_pos),
                        None => top_pos,
                    };
                    self.emit_at(Op::RequireCoercible(first), first_pos);
                }
                let mut key_slots: Vec<Result<u32, u32>> = vec![]; // Ok(const) / Err(slot)
                for pp in props {
                    self.emit(Op::Dup);
                    let tpos = pattern_pos(&pp.value.target).unwrap_or(self.f().cur_pos);
                    match &pp.key {
                        PropKey::Lit(k) => {
                            let c = self.str_const(k);
                            self.emit_at(Op::GetProp(c), tpos);
                            key_slots.push(Ok(c));
                        }
                        PropKey::Computed(k) => {
                            self.compile_expr(k)?;
                            self.emit(Op::ToPropertyKey);
                            if rest.is_some() {
                                let slot = self.f().alloc("%key");
                                self.emit(Op::Dup);
                                self.emit(Op::Init(slot));
                                key_slots.push(Err(slot));
                            }
                            self.emit_at(Op::GetElem, tpos);
                        }
                        PropKey::Private(_) => return Err(serr(tpos, 1, "Unexpected identifier")),
                    }
                    self.bind_elem(&pp.value, init)?;
                }
                match rest {
                    Some(r) => {
                        self.emit(Op::NewObject);
                        self.emit(Op::Swap);
                        let n = key_slots.len() as u32;
                        for k in key_slots {
                            match k {
                                Ok(c) => {
                                    self.emit(Op::Const(c));
                                }
                                Err(s) => {
                                    self.emit(Op::Load(s));
                                }
                            }
                        }
                        self.emit(Op::CopyDataExcluding(n));
                        self.bind_pattern(r, init)?;
                    }
                    None => {
                        self.emit(Op::Pop);
                    }
                }
            }
            Pattern::Array { elems, rest } => {
                let pos = self.f().cur_pos;
                let text = match self.pat_ctx.take() {
                    Some((_, Some(t))) => self.str_const(&t),
                    _ => u32::MAX - 1,
                };
                self.emit_at(Op::GetIter(text), pos);
                for el in elems {
                    self.emit(Op::IterStep);
                    match el {
                        None => {
                            self.emit(Op::Pop);
                        }
                        Some(el) => self.bind_elem(el, init)?,
                    }
                }
                if let Some(r) = rest {
                    self.emit(Op::IterRest);
                    self.bind_pattern(r, init)?;
                }
                self.emit(Op::IterClose);
            }
        }
        Ok(())
    }

    fn bind_elem(&mut self, el: &PatElem, init: bool) -> CResult<()> {
        if let Some(d) = &el.default {
            let skip = self.new_label();
            self.emit_jump(Op::JumpIfNotUndefKeep(skip));
            let hint = if let Pattern::Ident(n, _) = &el.target {
                Some(n.clone())
            } else {
                None
            };
            self.compile_expr_named(d, hint.as_deref())?;
            self.bind(skip);
        }
        self.bind_pattern(&el.target, init)
    }

    // ------------------------------------------------------------ expressions
    fn compile_expr_named(&mut self, e: &Expr, name: Option<&str>) -> CResult<()> {
        if let (Some(n), true) = (name, is_anon_fn(e)) {
            match &e.kind {
                ExprKind::Function(f) => {
                    let idx = self.compile_function(f, Some(n))?;
                    self.emit_at(Op::Closure(idx), e.pos);
                    return Ok(());
                }
                ExprKind::Class(c) => return self.compile_class(c, Some(n)),
                _ => {}
            }
        }
        self.compile_expr(e)
    }

    pub fn compile_expr(&mut self, e: &Expr) -> CResult<()> {
        let pos = e.pos;
        match &e.kind {
            ExprKind::Num(n) => {
                self.emit_at(Op::Num(*n), pos);
            }
            ExprKind::Str(s) => {
                let c = self.str_const(s);
                self.emit_at(Op::Const(c), pos);
            }
            ExprKind::BigInt(s) => {
                let b = crate::bigint::BigInt::parse_digits(s, 10).unwrap_or_default();
                let c = self.val_const(Value::BigInt(Rc::new(b)));
                self.emit_at(Op::Const(c), pos);
            }
            ExprKind::Bool(b) => {
                self.emit_at(if *b { Op::True } else { Op::False }, pos);
            }
            ExprKind::Null => {
                self.emit_at(Op::Null, pos);
            }
            ExprKind::Template { cooked, exprs } => {
                let mut n = 0;
                for (i, c) in cooked.iter().enumerate() {
                    if !c.is_empty() || (i == 0 && exprs.is_empty()) {
                        let k = self.str_const(c);
                        self.emit_at(Op::Const(k), pos);
                        n += 1;
                    }
                    if let Some(x) = exprs.get(i) {
                        self.compile_expr(x)?;
                        self.emit_at(Op::ToStr, x.pos);
                        n += 1;
                    }
                }
                if n != 1 {
                    self.emit(Op::Concat(n));
                } else if exprs.is_empty() {
                    // single literal string, already pushed
                }
            }
            ExprKind::Tagged {
                tag,
                cooked,
                raw,
                exprs,
            } => {
                let text = self.str_const(&expr_text(tag));
                let is_method = self.compile_callee(tag)?;
                let site = {
                    let f = self.f();
                    f.templates.push((
                        cooked
                            .iter()
                            .map(|c| c.as_ref().map(|s| JsStr::new(&**s)))
                            .collect(),
                        raw.iter().map(|s| JsStr::new(&**s)).collect(),
                    ));
                    (f.templates.len() - 1) as u32
                };
                self.emit(Op::TemplateObj(site));
                for x in exprs {
                    self.compile_expr(x)?;
                }
                let argc = exprs.len() as u32 + 1;
                self.emit_at(
                    if is_method {
                        Op::CallMethod(argc, text)
                    } else {
                        Op::Call(argc, text)
                    },
                    pos,
                );
            }
            ExprKind::Regex { pattern, flags } => {
                let p = self.str_const(pattern);
                let f = self.str_const(flags);
                self.emit_at(Op::RegExp(p, f), pos);
            }
            ExprKind::Ident(n) => self.load_name(n, pos),
            ExprKind::This => self.load_name("this", pos),
            ExprKind::NewTarget => self.load_name("new.target", pos),
            ExprKind::ImportMeta => {
                self.emit_at(Op::ImportMeta, pos);
            }
            ExprKind::Import(arg) => {
                self.compile_expr(arg)?;
                self.emit_at(Op::DynImport, pos);
            }
            ExprKind::Array(elems) => {
                let simple = elems.iter().all(|x| matches!(x, ArrElem::Expr(_)));
                if simple {
                    for x in elems {
                        if let ArrElem::Expr(x) = x {
                            self.compile_expr(x)?;
                        }
                    }
                    self.emit_at(Op::NewArray(elems.len() as u32), pos);
                } else {
                    self.emit_at(Op::NewArray(0), pos);
                    for x in elems {
                        match x {
                            ArrElem::Expr(x) => {
                                self.compile_expr(x)?;
                                self.emit(Op::ArrayPush);
                            }
                            ArrElem::Hole => {
                                self.emit(Op::ArrayHole);
                            }
                            ArrElem::Spread(x) => {
                                self.compile_expr(x)?;
                                let t = self.str_const(&expr_text(x));
                                self.emit_at(Op::ArraySpread(t), x.pos);
                            }
                        }
                    }
                }
            }
            ExprKind::Object(props) => self.compile_object(props, pos)?,
            ExprKind::Function(f) => {
                let idx = self.compile_function(f, None)?;
                self.emit_at(Op::Closure(idx), pos);
            }
            ExprKind::Class(c) => self.compile_class(c, None)?,
            ExprKind::Unary(op, arg) => self.compile_unary(*op, arg, pos)?,
            ExprKind::Update {
                inc,
                prefix,
                target,
            } => self.compile_update(*inc, *prefix, target)?,
            ExprKind::Binary(op, l, r) => {
                self.compile_expr(l)?;
                self.compile_expr(r)?;
                let o = match op {
                    BinOp::Add => Op::Add,
                    BinOp::Sub => Op::Sub,
                    BinOp::Mul => Op::Mul,
                    BinOp::Div => Op::Div,
                    BinOp::Mod => Op::Mod,
                    BinOp::Exp => Op::Exp,
                    BinOp::Shl => Op::Shl,
                    BinOp::Shr => Op::Shr,
                    BinOp::UShr => Op::UShr,
                    BinOp::BitAnd => Op::BitAnd,
                    BinOp::BitOr => Op::BitOr,
                    BinOp::BitXor => Op::BitXor,
                    BinOp::Eq => Op::Eq,
                    BinOp::Ne => Op::Ne,
                    BinOp::StrictEq => Op::StrictEq,
                    BinOp::StrictNe => Op::StrictNe,
                    BinOp::Lt => Op::Lt,
                    BinOp::Gt => Op::Gt,
                    BinOp::Le => Op::Le,
                    BinOp::Ge => Op::Ge,
                    BinOp::In => Op::In,
                    BinOp::InstanceOf => Op::InstanceOf,
                };
                let _ = r;
                self.emit_at(o, pos);
            }
            ExprKind::Logical(op, l, r) => {
                self.compile_expr(l)?;
                let end = self.new_label();
                match op {
                    LogOp::And => self.emit_jump(Op::JumpIfFalseKeep(end)),
                    LogOp::Or => self.emit_jump(Op::JumpIfTrueKeep(end)),
                    LogOp::Nullish => self.emit_jump(Op::JumpIfNotNullishKeep(end)),
                };
                self.compile_expr(r)?;
                self.bind(end);
            }
            ExprKind::Cond(t, a, b) => {
                self.compile_expr(t)?;
                let else_l = self.new_label();
                let end = self.new_label();
                self.emit_jump(Op::JumpIfFalse(else_l));
                self.compile_expr(a)?;
                self.emit_jump(Op::Jump(end));
                self.bind(else_l);
                self.compile_expr(b)?;
                self.bind(end);
            }
            ExprKind::Assign { op, target, value } => {
                self.compile_assign(*op, target, value, pos)?
            }
            ExprKind::Seq(v) => {
                for (i, x) in v.iter().enumerate() {
                    self.compile_expr(x)?;
                    if i + 1 < v.len() {
                        self.emit(Op::Pop);
                    }
                }
            }
            ExprKind::Call {
                callee,
                args,
                optional,
            } => self.compile_call(e, callee, args, *optional)?,
            ExprKind::New { callee, args } => {
                let text = self.str_const(&expr_text(callee));
                self.compile_expr(callee)?;
                if args.iter().any(|a| matches!(a, ArrElem::Spread(_))) {
                    self.compile_spread_args(args, pos)?;
                    self.emit_at(Op::NewSpread(text), pos);
                } else {
                    for a in args {
                        if let ArrElem::Expr(x) = a {
                            self.compile_expr(x)?;
                        }
                    }
                    self.emit_at(Op::New(args.len() as u32, text), pos);
                }
            }
            ExprKind::Member {
                obj,
                prop,
                optional,
            } => {
                self.compile_expr(obj)?;
                if *optional {
                    let l = *self.f().opt_labels.last().unwrap();
                    self.emit_jump(Op::OptCheck(l, 1));
                }
                match prop {
                    MemberProp::Name(n, p) => {
                        let c = self.str_const(n);
                        self.emit_at(Op::GetProp(c), *p);
                    }
                    MemberProp::Computed(k) => {
                        self.compile_expr(k)?;
                        // V8 reports keyed loads at the `[` (just before the key).
                        let bracket = Pos {
                            line: k.pos.line,
                            col: k.pos.col.saturating_sub(1).max(1),
                        };
                        self.emit_at(Op::GetElem, bracket);
                    }
                    MemberProp::Private(n, p) => {
                        self.load_name(&format!("#{n}"), *p);
                        self.emit_at(Op::GetPrivate, *p);
                    }
                }
            }
            ExprKind::OptChain(inner) => {
                let l = self.new_label();
                self.f().opt_labels.push(l);
                self.compile_expr(inner)?;
                self.f().opt_labels.pop();
                let end = self.new_label();
                self.emit_jump(Op::Jump(end));
                self.bind(l);
                self.emit(Op::Undef);
                self.bind(end);
            }
            ExprKind::Yield { arg, delegate } => {
                match arg {
                    Some(a) => self.compile_expr(a)?,
                    None => {
                        self.emit(Op::Undef);
                    }
                }
                if *delegate {
                    let text = match arg {
                        Some(a) => self.str_const(&expr_text(a)),
                        None => u32::MAX,
                    };
                    let op = if self.f().is_async {
                        Op::GetAsyncIter(text)
                    } else {
                        Op::GetIter(text)
                    };
                    self.emit_at(op, pos);
                    self.emit(Op::Undef);
                    self.emit_at(Op::YieldStar(0), pos);
                } else {
                    self.emit_at(Op::Yield, pos);
                }
            }
            ExprKind::Await(a) => {
                self.compile_expr(a)?;
                self.emit_at(Op::Await, pos);
            }
            ExprKind::SuperMember(prop) => {
                self.load_name("this", pos);
                self.load_name("%home", pos);
                match prop {
                    MemberProp::Name(n, p) => {
                        let c = self.str_const(n);
                        self.emit_at(Op::SuperGet(c), *p);
                    }
                    MemberProp::Computed(k) => {
                        self.compile_expr(k)?;
                        self.emit_at(Op::SuperGetElem, k.pos);
                    }
                    MemberProp::Private(..) => {}
                }
            }
            ExprKind::SuperCall(args) => {
                self.load_name("%fn", pos);
                self.load_name("new.target", pos);
                if args.iter().any(|a| matches!(a, ArrElem::Spread(_))) {
                    self.compile_spread_args(args, pos)?;
                    self.emit_at(Op::SuperCallSpread, pos);
                } else {
                    for a in args {
                        if let ArrElem::Expr(x) = a {
                            self.compile_expr(x)?;
                        }
                    }
                    self.emit_at(Op::SuperCall(args.len() as u32), pos);
                }
                match self.resolve("this") {
                    Res::Local(s, _) => self.emit_at(Op::BindThis(s, false), pos),
                    Res::Free(i, _) => self.emit_at(Op::BindThis(i, true), pos),
                    Res::Global => self.emit(Op::Nop),
                };
                if self.nearest_run_fields() {
                    self.emit_run_fields();
                }
            }
            ExprKind::PrivateIn(n, obj) => {
                self.load_name(&format!("#{n}"), pos);
                self.compile_expr(obj)?;
                self.emit_at(Op::HasPrivate, pos);
            }
            ExprKind::CoverInit(..) => {
                return Err(serr(pos, 1, "Invalid shorthand property initializer"))
            }
        }
        Ok(())
    }

    fn nearest_run_fields(&self) -> bool {
        for f in self.funcs.iter().rev() {
            if !f.is_arrow() {
                return f.run_fields;
            }
        }
        false
    }

    /// Pushes callee (and receiver for member callees). Returns true when a
    /// receiver was pushed.
    fn compile_callee(&mut self, callee: &Expr) -> CResult<bool> {
        match &callee.kind {
            ExprKind::Member {
                obj,
                prop,
                optional,
            } => {
                self.compile_expr(obj)?;
                if *optional {
                    let l = *self.f().opt_labels.last().unwrap();
                    self.emit_jump(Op::OptCheck(l, 1));
                }
                match prop {
                    MemberProp::Name(n, p) => {
                        let c = self.str_const(n);
                        self.emit_at(Op::GetPropKeep(c), *p);
                    }
                    MemberProp::Computed(k) => {
                        self.compile_expr(k)?;
                        self.emit_at(Op::GetElemKeep, k.pos);
                    }
                    MemberProp::Private(n, p) => {
                        self.load_name(&format!("#{n}"), *p);
                        self.emit_at(Op::GetPrivateKeep, *p);
                    }
                }
                Ok(true)
            }
            ExprKind::SuperMember(prop) => {
                let pos = callee.pos;
                self.load_name("this", pos);
                self.load_name("%home", pos);
                match prop {
                    MemberProp::Name(n, p) => {
                        let c = self.str_const(n);
                        self.emit_at(Op::SuperGetKeep(c), *p);
                    }
                    MemberProp::Computed(k) => {
                        self.compile_expr(k)?;
                        self.emit_at(Op::SuperGetElemKeep, k.pos);
                    }
                    MemberProp::Private(..) => {}
                }
                Ok(true)
            }
            _ => {
                self.compile_expr(callee)?;
                Ok(false)
            }
        }
    }

    fn compile_spread_args(&mut self, args: &[ArrElem], call_pos: Pos) -> CResult<()> {
        self.emit(Op::NewArray(0));
        for a in args {
            match a {
                ArrElem::Expr(x) => {
                    self.compile_expr(x)?;
                    self.emit(Op::ArrayPush);
                }
                ArrElem::Spread(x) => {
                    self.compile_expr(x)?;
                    self.emit_at(Op::ArraySpread(u32::MAX), call_pos);
                }
                ArrElem::Hole => {
                    self.emit(Op::ArrayHole);
                }
            }
        }
        Ok(())
    }

    fn compile_call(
        &mut self,
        e: &Expr,
        callee: &Expr,
        args: &[ArrElem],
        optional: bool,
    ) -> CResult<()> {
        let text = self.str_const(&expr_text(callee));
        let is_method = self.compile_callee(callee)?;
        if optional {
            let l = *self.f().opt_labels.last().unwrap();
            self.emit_jump(Op::OptCheck(l, if is_method { 2 } else { 1 }));
        }
        if args.iter().any(|a| matches!(a, ArrElem::Spread(_))) {
            self.compile_spread_args(args, e.pos)?;
            self.emit_at(
                if is_method {
                    Op::CallMethodSpread(text)
                } else {
                    Op::CallSpread(text)
                },
                e.pos,
            );
        } else {
            for a in args {
                if let ArrElem::Expr(x) = a {
                    self.compile_expr(x)?;
                }
            }
            let n = args.len() as u32;
            self.emit_at(
                if is_method {
                    Op::CallMethod(n, text)
                } else {
                    Op::Call(n, text)
                },
                e.pos,
            );
        }
        Ok(())
    }

    fn compile_unary(&mut self, op: UnOp, arg: &Expr, pos: Pos) -> CResult<()> {
        match op {
            UnOp::Typeof => {
                if let ExprKind::Ident(n) = &arg.kind {
                    if let Res::Global = self.resolve(n) {
                        let c = self.str_const(n);
                        self.emit_at(Op::TypeofGlobal(c), pos);
                        return Ok(());
                    }
                }
                self.compile_expr(arg)?;
                self.emit_at(Op::Typeof, pos);
            }
            UnOp::Delete => match &arg.kind {
                ExprKind::Member { obj, prop, .. } => {
                    self.compile_expr(obj)?;
                    match prop {
                        MemberProp::Name(n, _) => {
                            let c = self.str_const(n);
                            self.emit_at(Op::DeleteProp(c), pos);
                        }
                        MemberProp::Computed(k) => {
                            self.compile_expr(k)?;
                            self.emit_at(Op::DeleteElem, pos);
                        }
                        MemberProp::Private(..) => {
                            return Err(serr(pos, 1, "Private fields can not be deleted"))
                        }
                    }
                }
                ExprKind::OptChain(inner) => {
                    let l = self.new_label();
                    self.f().opt_labels.push(l);
                    if let ExprKind::Member {
                        obj,
                        prop,
                        optional,
                    } = &inner.kind
                    {
                        self.compile_expr(obj)?;
                        if *optional {
                            self.emit_jump(Op::OptCheck(l, 1));
                        }
                        match prop {
                            MemberProp::Name(n, _) => {
                                let c = self.str_const(n);
                                self.emit_at(Op::DeleteProp(c), pos);
                            }
                            MemberProp::Computed(k) => {
                                self.compile_expr(k)?;
                                self.emit_at(Op::DeleteElem, pos);
                            }
                            _ => {}
                        }
                    } else {
                        self.compile_expr(inner)?;
                        self.emit(Op::Pop);
                        self.emit(Op::True);
                    }
                    self.f().opt_labels.pop();
                    let end = self.new_label();
                    self.emit_jump(Op::Jump(end));
                    self.bind(l);
                    self.emit(Op::True);
                    self.bind(end);
                }
                ExprKind::Ident(n) => match self.resolve(n) {
                    Res::Global => {
                        // `delete x` on a global: remove the global property.
                        let c = self.str_const(n);
                        let g = self.str_const("globalThis");
                        self.emit(Op::LoadGlobal(g));
                        self.emit(Op::DeleteProp(c));
                    }
                    _ => {
                        self.emit(Op::False);
                    }
                },
                _ => {
                    self.compile_expr(arg)?;
                    self.emit(Op::Pop);
                    self.emit(Op::True);
                }
            },
            _ => {
                self.compile_expr(arg)?;
                let o = match op {
                    UnOp::Neg => Op::Neg,
                    UnOp::Plus => Op::Plus,
                    UnOp::Not => Op::Not,
                    UnOp::BitNot => Op::BitNot,
                    UnOp::Void => {
                        self.emit(Op::Pop);
                        Op::Undef
                    }
                    _ => unreachable!(),
                };
                self.emit_at(o, pos);
            }
        }
        Ok(())
    }

    fn compile_update(&mut self, inc: bool, prefix: bool, target: &Expr) -> CResult<()> {
        let op = if inc { Op::Inc } else { Op::Dec };
        match &target.kind {
            ExprKind::Ident(n) => {
                self.load_name(n, target.pos);
                if prefix {
                    self.emit(op);
                    self.emit(Op::Dup);
                } else {
                    self.emit(Op::ToNumeric);
                    self.emit(Op::Dup);
                    self.emit(op);
                }
                self.store_name(n, target.pos, false);
            }
            ExprKind::Member { obj, prop, .. } => {
                self.compile_expr(obj)?;
                match prop {
                    MemberProp::Name(n, p) => {
                        let c = self.str_const(n);
                        self.emit(Op::Dup);
                        self.emit_at(Op::GetProp(c), *p);
                        if prefix {
                            self.emit(op);
                        } else {
                            self.emit(Op::ToNumeric);
                            self.emit(Op::Dup);
                            self.emit(Op::Rot3);
                            self.emit(op);
                        }
                        self.emit_at(Op::SetProp(c), *p);
                        if !prefix {
                            self.emit(Op::Pop);
                        }
                    }
                    MemberProp::Computed(k) => {
                        self.compile_expr(k)?;
                        self.emit(Op::ToPropertyKey);
                        self.emit(Op::Dup2);
                        self.emit_at(Op::GetElem, k.pos);
                        if prefix {
                            self.emit(op);
                        } else {
                            self.emit(Op::ToNumeric);
                            self.emit(Op::Dup);
                            self.emit(Op::Rot4);
                            self.emit(op);
                        }
                        self.emit_at(Op::SetElem, k.pos);
                        if !prefix {
                            self.emit(Op::Pop);
                        }
                    }
                    MemberProp::Private(n, p) => {
                        self.load_name(&format!("#{n}"), *p);
                        self.emit(Op::Dup2);
                        self.emit_at(Op::GetPrivate, *p);
                        if prefix {
                            self.emit(op);
                        } else {
                            self.emit(Op::ToNumeric);
                            self.emit(Op::Dup);
                            self.emit(Op::Rot4);
                            self.emit(op);
                        }
                        self.emit_at(Op::SetPrivate, *p);
                        if !prefix {
                            self.emit(Op::Pop);
                        }
                    }
                }
            }
            _ => {
                return Err(serr(
                    target.pos,
                    1,
                    "Invalid left-hand side expression in postfix operation",
                ))
            }
        }
        Ok(())
    }

    fn compile_assign(
        &mut self,
        op: AssignOp,
        target: &Pattern,
        value: &Expr,
        pos: Pos,
    ) -> CResult<()> {
        let binop = |b: BinOp| match b {
            BinOp::Add => Op::Add,
            BinOp::Sub => Op::Sub,
            BinOp::Mul => Op::Mul,
            BinOp::Div => Op::Div,
            BinOp::Mod => Op::Mod,
            BinOp::Exp => Op::Exp,
            BinOp::Shl => Op::Shl,
            BinOp::Shr => Op::Shr,
            BinOp::UShr => Op::UShr,
            BinOp::BitAnd => Op::BitAnd,
            BinOp::BitOr => Op::BitOr,
            BinOp::BitXor => Op::BitXor,
            _ => Op::Nop,
        };
        match target {
            Pattern::Ident(n, npos) => match op {
                AssignOp::Assign => {
                    self.compile_expr_named(value, Some(n))?;
                    self.emit(Op::Dup);
                    self.store_name(n, pos, false);
                }
                AssignOp::Op(b) => {
                    self.load_name(n, *npos);
                    self.compile_expr(value)?;
                    self.emit_at(binop(b), pos);
                    self.emit(Op::Dup);
                    self.store_name(n, pos, false);
                }
                AssignOp::Logical(l) => {
                    self.load_name(n, *npos);
                    let end = self.new_label();
                    match l {
                        LogOp::And => self.emit_jump(Op::JumpIfFalseKeep(end)),
                        LogOp::Or => self.emit_jump(Op::JumpIfTrueKeep(end)),
                        LogOp::Nullish => self.emit_jump(Op::JumpIfNotNullishKeep(end)),
                    };
                    self.compile_expr_named(value, Some(n))?;
                    self.emit(Op::Dup);
                    self.store_name(n, pos, false);
                    self.bind(end);
                }
            },
            Pattern::Expr(t) => match &t.kind {
                ExprKind::Member { obj, prop, .. } => {
                    self.compile_expr(obj)?;
                    match prop {
                        MemberProp::Name(name, p) => {
                            let c = self.str_const(name);
                            match op {
                                AssignOp::Assign => {
                                    self.compile_expr(value)?;
                                    self.emit_at(Op::SetProp(c), pos);
                                }
                                AssignOp::Op(b) => {
                                    self.emit(Op::Dup);
                                    self.emit_at(Op::GetProp(c), *p);
                                    self.compile_expr(value)?;
                                    self.emit_at(binop(b), pos);
                                    self.emit_at(Op::SetProp(c), pos);
                                }
                                AssignOp::Logical(l) => {
                                    self.emit(Op::Dup);
                                    self.emit_at(Op::GetProp(c), *p);
                                    let skip = self.new_label();
                                    let end = self.new_label();
                                    match l {
                                        LogOp::And => self.emit_jump(Op::JumpIfFalseKeep(skip)),
                                        LogOp::Or => self.emit_jump(Op::JumpIfTrueKeep(skip)),
                                        LogOp::Nullish => {
                                            self.emit_jump(Op::JumpIfNotNullishKeep(skip))
                                        }
                                    };
                                    self.compile_expr(value)?;
                                    self.emit_at(Op::SetProp(c), pos);
                                    self.emit_jump(Op::Jump(end));
                                    self.bind(skip);
                                    // [obj value] -> [value]
                                    self.emit(Op::Swap);
                                    self.emit(Op::Pop);
                                    self.bind(end);
                                }
                            }
                        }
                        MemberProp::Computed(k) => {
                            self.compile_expr(k)?;
                            match op {
                                AssignOp::Assign => {
                                    self.compile_expr(value)?;
                                    self.emit_at(Op::SetElem, pos);
                                }
                                AssignOp::Op(b) => {
                                    self.emit(Op::ToPropertyKey);
                                    self.emit(Op::Dup2);
                                    self.emit_at(Op::GetElem, k.pos);
                                    self.compile_expr(value)?;
                                    self.emit_at(binop(b), pos);
                                    self.emit_at(Op::SetElem, pos);
                                }
                                AssignOp::Logical(l) => {
                                    self.emit(Op::ToPropertyKey);
                                    self.emit(Op::Dup2);
                                    self.emit_at(Op::GetElem, k.pos);
                                    let skip = self.new_label();
                                    let end = self.new_label();
                                    match l {
                                        LogOp::And => self.emit_jump(Op::JumpIfFalseKeep(skip)),
                                        LogOp::Or => self.emit_jump(Op::JumpIfTrueKeep(skip)),
                                        LogOp::Nullish => {
                                            self.emit_jump(Op::JumpIfNotNullishKeep(skip))
                                        }
                                    };
                                    self.compile_expr(value)?;
                                    self.emit_at(Op::SetElem, pos);
                                    self.emit_jump(Op::Jump(end));
                                    self.bind(skip);
                                    // [obj key value] -> [value]
                                    self.emit(Op::Rot3);
                                    self.emit(Op::Pop);
                                    self.emit(Op::Pop);
                                    self.bind(end);
                                }
                            }
                        }
                        MemberProp::Private(name, p) => {
                            self.load_name(&format!("#{name}"), *p);
                            match op {
                                AssignOp::Assign => {
                                    self.compile_expr(value)?;
                                    self.emit_at(Op::SetPrivate, pos);
                                }
                                AssignOp::Op(b) => {
                                    self.emit(Op::Dup2);
                                    self.emit_at(Op::GetPrivate, *p);
                                    self.compile_expr(value)?;
                                    self.emit_at(binop(b), pos);
                                    self.emit_at(Op::SetPrivate, pos);
                                }
                                AssignOp::Logical(l) => {
                                    self.emit(Op::Dup2);
                                    self.emit_at(Op::GetPrivate, *p);
                                    let skip = self.new_label();
                                    let end = self.new_label();
                                    match l {
                                        LogOp::And => self.emit_jump(Op::JumpIfFalseKeep(skip)),
                                        LogOp::Or => self.emit_jump(Op::JumpIfTrueKeep(skip)),
                                        LogOp::Nullish => {
                                            self.emit_jump(Op::JumpIfNotNullishKeep(skip))
                                        }
                                    };
                                    self.compile_expr(value)?;
                                    self.emit_at(Op::SetPrivate, pos);
                                    self.emit_jump(Op::Jump(end));
                                    self.bind(skip);
                                    self.emit(Op::Rot3);
                                    self.emit(Op::Pop);
                                    self.emit(Op::Pop);
                                    self.bind(end);
                                }
                            }
                        }
                    }
                }
                ExprKind::SuperMember(prop) => {
                    self.load_name("this", pos);
                    self.load_name("%home", pos);
                    match prop {
                        MemberProp::Name(name, _) => {
                            let c = self.str_const(name);
                            if let AssignOp::Op(b) = op {
                                self.emit(Op::Dup2);
                                self.emit(Op::SuperGet(c));
                                self.compile_expr(value)?;
                                self.emit(binop(b));
                            } else {
                                self.compile_expr(value)?;
                            }
                            self.emit_at(Op::SuperSet(c), pos);
                        }
                        MemberProp::Computed(k) => {
                            self.compile_expr(k)?;
                            self.compile_expr(value)?;
                            self.emit_at(Op::SuperSetElem, pos);
                        }
                        MemberProp::Private(..) => {}
                    }
                }
                _ => return Err(serr(t.pos, 1, "Invalid left-hand side in assignment")),
            },
            _ => {
                // Destructuring assignment: value remains as the result.
                self.compile_expr(value)?;
                self.emit(Op::Dup);
                self.bind_top(target, false, pos, Some(expr_text(value)))?;
            }
        }
        Ok(())
    }

    fn compile_object(&mut self, props: &[Prop], pos: Pos) -> CResult<()> {
        self.emit_at(Op::NewObject, pos);
        for p in props {
            match p {
                Prop::KeyValue(key, v) => match key {
                    PropKey::Lit(k) => {
                        if &**k == "__proto__" {
                            self.compile_expr(v)?;
                            self.emit(Op::SetProtoLit);
                        } else {
                            self.compile_expr_named(v, Some(k))?;
                            let c = self.str_const(k);
                            self.emit(Op::DefineField(c));
                        }
                    }
                    PropKey::Computed(k) => {
                        self.compile_expr(k)?;
                        self.emit(Op::ToPropertyKey);
                        self.compile_expr(v)?;
                        if is_anon_fn(v) {
                            self.emit(Op::SetFnNameElem(0));
                        }
                        self.emit(Op::DefineElem);
                    }
                    PropKey::Private(_) => {}
                },
                Prop::Shorthand(n, p) => {
                    self.load_name(n, *p);
                    let c = self.str_const(n);
                    self.emit(Op::DefineField(c));
                }
                Prop::Spread(v) => {
                    self.compile_expr(v)?;
                    self.emit(Op::CopyData);
                }
                Prop::Method { key, func, kind } => {
                    let k = match kind {
                        MethodKind::Method => 0,
                        MethodKind::Get => 1,
                        MethodKind::Set => 2,
                    };
                    match key {
                        PropKey::Lit(name) => {
                            let idx = self.compile_function(func, Some(name))?;
                            self.emit(Op::Closure(idx));
                            let c = self.str_const(name);
                            self.emit(Op::DefineMethod(c, k, true));
                        }
                        PropKey::Computed(e) => {
                            self.compile_expr(e)?;
                            self.emit(Op::ToPropertyKey);
                            let idx = self.compile_function(func, None)?;
                            self.emit(Op::Closure(idx));
                            self.emit(Op::DefineMethodElem(k, true));
                        }
                        PropKey::Private(_) => {}
                    }
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ classes
    fn compile_class(&mut self, c: &Class, name_hint: Option<&str>) -> CResult<()> {
        let cname: Option<Name> = c.name.clone();
        let display: String = cname.as_deref().or(name_hint).unwrap_or("").to_string();
        self.push_scope();
        let inner_slot = match &cname {
            Some(n) => {
                let s = self.declare(n, BKind::Const, c.pos)?;
                self.emit(Op::DeclLet(s));
                Some(s)
            }
            None => None,
        };
        // Private names.
        let mut privs: Vec<Name> = vec![];
        for m in &c.members {
            let key = match m {
                ClassMember::Method { key, .. } | ClassMember::Field { key, .. } => key,
                _ => continue,
            };
            if let PropKey::Private(n) = key {
                if !privs.contains(n) {
                    privs.push(n.clone());
                }
            }
        }
        for n in &privs {
            let hn: Name = Rc::from(format!("#{n}").as_str());
            let slot = self.declare(&hn, BKind::Const, c.pos)?;
            let d = self.str_const(&hn);
            self.emit(Op::NewPrivateName(d));
            self.emit(Op::Init(slot));
        }
        let has_super = c.extends.is_some();
        if let Some(sup) = &c.extends {
            self.compile_expr(sup)?;
        }
        let has_instance = c.members.iter().any(|m| match m {
            ClassMember::Field { is_static, .. } => !is_static,
            ClassMember::Method {
                key: PropKey::Private(_),
                is_static,
                ..
            } => !is_static,
            _ => false,
        });
        // Constructor.
        let default_ctor;
        let ctor: &Func = match &c.ctor {
            Some(f) => f,
            None => {
                default_ctor = synth_ctor(c, has_super);
                &default_ctor
            }
        };
        let code = self.compile_function_code(ctor, Some(&display), has_instance)?;
        let code = {
            // Rename to the display name (class expressions get inferred names).
            let mut code = Rc::try_unwrap(code).ok().expect("fresh code");
            code.name = JsStr::new(display.as_str());
            code.source = self.source_of(c.src_start, c.src_end);
            Rc::new(code)
        };
        let idx = {
            let f = self.f();
            f.codes.push(code);
            (f.codes.len() - 1) as u32
        };
        self.emit_at(Op::Class(idx, has_super), c.pos);
        if let Some(s) = inner_slot {
            self.emit(Op::Swap);
            self.emit(Op::Dup);
            self.emit(Op::Init(s));
            self.emit(Op::Swap);
        }
        // Methods and computed field keys, in order.
        let mut field_keys: Vec<Option<u32>> = vec![];
        for m in &c.members {
            match m {
                ClassMember::Method {
                    key,
                    func,
                    kind,
                    is_static,
                } => {
                    let k = match kind {
                        MethodKind::Method => 0,
                        MethodKind::Get => 1,
                        MethodKind::Set => 2,
                    };
                    match key {
                        PropKey::Lit(n) => {
                            let fidx = self.compile_function(func, Some(n))?;
                            self.emit(Op::Closure(fidx));
                            let cn = self.str_const(n);
                            self.emit(Op::ClassMethod(cn, k, *is_static));
                        }
                        PropKey::Computed(e) => {
                            self.compile_expr(e)?;
                            self.emit(Op::ToPropertyKey);
                            let fidx = self.compile_function(func, None)?;
                            self.emit(Op::Closure(fidx));
                            self.emit(Op::ClassMethodElem(k, *is_static));
                        }
                        PropKey::Private(_) => {
                            if *is_static {
                                // Static private methods live on the constructor.
                                let PropKey::Private(n) = key else {
                                    unreachable!()
                                };
                                self.emit(Op::Swap);
                                self.load_name(&format!("#{n}"), c.pos);
                                let fidx = self.compile_function(func, Some(&format!("#{n}")))?;
                                self.emit(Op::Closure(fidx));
                                // [proto ctor key fn] -> define on ctor
                                self.emit(Op::DefineMethodElem(k, false));
                                self.emit(Op::Swap);
                            }
                        }
                    }
                }
                ClassMember::Field {
                    key: PropKey::Computed(e),
                    ..
                } => {
                    self.compile_expr(e)?;
                    self.emit(Op::ToPropertyKey);
                    let slot = self.f().alloc("%fieldkey");
                    let hn: Name = Rc::from(format!("%fieldkey{}", slot).as_str());
                    self.f().scopes.last_mut().unwrap().binds.push(Binding {
                        name: hn,
                        slot,
                        kind: BKind::Hidden,
                    });
                    self.emit(Op::Init(slot));
                    field_keys.push(Some(slot));
                }
                ClassMember::Field { .. } => field_keys.push(None),
                ClassMember::StaticBlock(_) => {}
            }
        }
        // Instance field initialiser.
        if has_instance {
            let code = self.compile_class_init(c, false)?;
            let f = self.f();
            f.codes.push(code);
            let i = (f.codes.len() - 1) as u32;
            self.emit(Op::Closure(i));
            self.emit(Op::SetFieldInit);
        }
        // Static elements.
        let has_static = c.members.iter().any(|m| match m {
            ClassMember::Field { is_static, .. } => *is_static,
            ClassMember::StaticBlock(_) => true,
            _ => false,
        });
        if has_static {
            let code = self.compile_class_init(c, true)?;
            let f = self.f();
            f.codes.push(code);
            let i = (f.codes.len() - 1) as u32;
            self.emit(Op::Swap);
            self.emit(Op::Dup);
            self.emit(Op::Closure(i));
            let t = self.str_const("static initializer");
            self.emit(Op::CallMethod(0, t));
            self.emit(Op::Pop);
            self.emit(Op::Swap);
        }
        self.emit(Op::Pop);
        self.pop_scope();
        Ok(())
    }

    fn compile_class_init(&mut self, c: &Class, is_static: bool) -> CResult<Rc<Code>> {
        let mut fs = FState::new(FuncKind::ClassInit, true);
        fs.kind = FuncKind::ClassInit;
        self.funcs.push(fs);
        let r = (|| -> CResult<()> {
            let mut field_i = 0usize;
            let mut computed_slots: Vec<String> = vec![];
            // Collect hidden computed-key names in declaration order.
            {
                let parent = &self.funcs[self.funcs.len() - 2];
                let scope = parent.scopes.last().unwrap();
                for b in &scope.binds {
                    if b.name.starts_with("%fieldkey") {
                        computed_slots.push(b.name.to_string());
                    }
                }
            }
            let mut comp_i = 0usize;
            // Private methods are installed before fields.
            if !is_static {
                for m in &c.members {
                    if let ClassMember::Method {
                        key: PropKey::Private(n),
                        func,
                        kind,
                        is_static: false,
                    } = m
                    {
                        let p = func.pos;
                        self.load_name("this", p);
                        self.load_name(&format!("#{n}"), p);
                        let fidx = self.compile_function(func, Some(&format!("#{n}")))?;
                        self.emit(Op::Closure(fidx));
                        let k = match kind {
                            MethodKind::Method => 0,
                            MethodKind::Get => 1,
                            MethodKind::Set => 2,
                        };
                        self.emit(Op::DefineMethodElem(k, false));
                        self.emit(Op::Pop);
                    }
                }
            }
            for m in &c.members {
                match m {
                    ClassMember::Field {
                        key,
                        value,
                        is_static: st,
                        pos,
                    } => {
                        let is_computed = matches!(key, PropKey::Computed(_));
                        let my_comp = if is_computed {
                            comp_i += 1;
                            Some(computed_slots[comp_i - 1].clone())
                        } else {
                            None
                        };
                        field_i += 1;
                        if *st != is_static {
                            continue;
                        }
                        self.stmt_pos(*pos);
                        self.load_name("this", *pos);
                        match key {
                            PropKey::Lit(k) => {
                                match value {
                                    Some(v) => self.compile_expr_named(v, Some(k))?,
                                    None => {
                                        self.emit(Op::Undef);
                                    }
                                }
                                let cst = self.str_const(k);
                                self.emit(Op::DefineField(cst));
                            }
                            PropKey::Computed(_) => {
                                self.load_name(my_comp.as_deref().unwrap(), *pos);
                                match value {
                                    Some(v) => self.compile_expr(v)?,
                                    None => {
                                        self.emit(Op::Undef);
                                    }
                                }
                                self.emit(Op::DefineElem);
                            }
                            PropKey::Private(n) => {
                                self.load_name(&format!("#{n}"), *pos);
                                match value {
                                    Some(v) => {
                                        self.compile_expr_named(v, Some(&format!("#{n}")))?
                                    }
                                    None => {
                                        self.emit(Op::Undef);
                                    }
                                }
                                self.emit(Op::DefinePrivate);
                            }
                        }
                        self.emit(Op::Pop);
                    }
                    ClassMember::StaticBlock(body) if is_static => {
                        self.push_scope();
                        let mut vars = vec![];
                        self.collect_vars(body, &mut vars, true, true);
                        for (n, p) in vars {
                            self.declare(&n, BKind::Var, p)?;
                        }
                        self.hoist_block(body, false)?;
                        self.compile_stmts(body)?;
                        self.pop_scope();
                    }
                    _ => {}
                }
            }
            let _ = field_i;
            self.emit(Op::Undef);
            self.emit(Op::Return);
            Ok(())
        })();
        let fs = self.funcs.pop().unwrap();
        r?;
        let name = if is_static {
            "<static_initializer>"
        } else {
            "<instance_members_initializer>"
        };
        let src = self.source_of(c.src_start, c.src_end);
        Ok(self.finish(fs, JsStr::new(name), Some(0), 0, src))
    }

    // ------------------------------------------------------------ modules
    fn compile_imports(&mut self, body: &[Stmt]) -> CResult<()> {
        for s in body {
            if let StmtKind::Import(names, spec) = &s.kind {
                self.stmt_pos(s.pos);
                // %import(spec) -> namespace
                self.load_name("%import", s.pos);
                let c = self.str_const(spec);
                self.emit(Op::Const(c));
                let t = self.str_const("import");
                self.emit_at(Op::Call(1, t), s.pos);
                // The loader returns the namespace; bind names.
                if names.is_empty() {
                    self.emit(Op::Pop);
                    continue;
                }
                for (i, n) in names.iter().enumerate() {
                    if i + 1 < names.len() {
                        self.emit(Op::Dup);
                    }
                    match n {
                        ImportName::Namespace(local) => {
                            let slot = self.declare(local, BKind::Const, s.pos)?;
                            self.emit(Op::Init(slot));
                        }
                        ImportName::Default(local) => {
                            let c = self.str_const("default");
                            self.emit(Op::GetProp(c));
                            let slot = self.declare(local, BKind::Const, s.pos)?;
                            self.emit(Op::Init(slot));
                        }
                        ImportName::Named(imported, local) => {
                            let c = self.str_const(imported);
                            self.emit(Op::GetProp(c));
                            let slot = self.declare(local, BKind::Const, s.pos)?;
                            self.emit(Op::Init(slot));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn compile_exports(&mut self, body: &[Stmt]) -> CResult<()> {
        // Exported names become live getters on the namespace (param 0).
        let mut pairs: Vec<(Name, Name)> = vec![];
        for s in body {
            if let StmtKind::Export(k) = &s.kind {
                match k {
                    ExportKind::Decl(d) => {
                        let mut names = vec![];
                        match &d.kind {
                            StmtKind::Var(_, decls) => {
                                for dd in decls {
                                    pattern_names(&dd.target, &mut names);
                                }
                            }
                            StmtKind::Func(f) => names.push((f.name.clone().unwrap(), f.pos)),
                            StmtKind::Class(c) => names.push((c.name.clone().unwrap(), c.pos)),
                            _ => {}
                        }
                        for (n, _) in names {
                            pairs.push((n.clone(), n));
                        }
                    }
                    ExportKind::Default(_) => {
                        pairs.push((Rc::from("*default*"), Rc::from("default")))
                    }
                    ExportKind::DefaultDecl(d) => match &d.kind {
                        StmtKind::Func(f) => {
                            pairs.push((f.name.clone().unwrap(), Rc::from("default")))
                        }
                        StmtKind::Class(c) => {
                            pairs.push((c.name.clone().unwrap(), Rc::from("default")))
                        }
                        _ => {}
                    },
                    ExportKind::Names(list, None) => {
                        for (l, e) in list {
                            pairs.push((l.clone(), e.clone()));
                        }
                    }
                    ExportKind::Names(list, Some(from)) => {
                        // Re-export: namespace property copies via getters.
                        for (l, e) in list {
                            self.load_name("%ns", s.pos);
                            self.load_name("%import", s.pos);
                            let c = self.str_const(from);
                            self.emit(Op::Const(c));
                            let t = self.str_const("import");
                            self.emit(Op::Call(1, t));
                            let lc = self.str_const(l);
                            self.emit(Op::GetProp(lc));
                            let ec = self.str_const(e);
                            self.emit(Op::DefineField(ec));
                            self.emit(Op::Pop);
                        }
                    }
                    ExportKind::All(alias, from) => {
                        self.load_name("%ns", s.pos);
                        self.load_name("%import", s.pos);
                        let c = self.str_const(from);
                        self.emit(Op::Const(c));
                        let t = self.str_const("import");
                        self.emit(Op::Call(1, t));
                        match alias {
                            Some(a) => {
                                let ac = self.str_const(a);
                                self.emit(Op::DefineField(ac));
                            }
                            None => {
                                self.emit(Op::CopyData);
                            }
                        }
                        self.emit(Op::Pop);
                    }
                }
            }
        }
        if pairs.is_empty() {
            return Ok(());
        }
        // Getters must exist before the body runs (hoisting): emit them at
        // the very start by compiling here and relocating isn't possible, so
        // we define them now; the body already ran by this point, but the
        // getters read live bindings for later access (and cycles are rare).
        // Module namespace keys are sorted.
        pairs.sort_by(|a, b| a.1.cmp(&b.1));
        for (local, exported) in pairs {
            self.exports.push((local.clone(), exported.clone()));
            self.load_name("%ns", Pos::default());
            let getter = Func {
                name: None,
                params: vec![],
                rest: None,
                body: FuncBody::Expr(Box::new(Expr {
                    kind: ExprKind::Ident(local.clone()),
                    pos: Pos::default(),
                })),
                kind: FuncKind::Arrow,
                is_async: false,
                is_generator: false,
                strict: true,
                pos: Pos::default(),
                src_start: 0,
                src_end: 0,
                fields: None,
            };
            let idx = self.compile_function(&getter, Some(&exported))?;
            self.emit(Op::Closure(idx));
            let c = self.str_const(&exported);
            self.emit(Op::ExportGetter(c));
            self.emit(Op::Pop);
        }
        Ok(())
    }
}

fn pattern_pos(p: &Pattern) -> Option<Pos> {
    match p {
        Pattern::Ident(_, pos) => Some(*pos),
        Pattern::Expr(e) => Some(e.pos),
        Pattern::Object { props, .. } => props.first().and_then(|pp| pattern_pos(&pp.value.target)),
        Pattern::Array { elems, .. } => elems
            .iter()
            .flatten()
            .next()
            .and_then(|e| pattern_pos(&e.target)),
    }
}

/// `constructor() {}` / `constructor(...args) { super(...args); }`
fn synth_ctor(c: &Class, derived: bool) -> Func {
    let pos = c.pos;
    let (rest, body) = if derived {
        let args: Name = Rc::from("args");
        (
            Some(Pattern::Ident(args.clone(), pos)),
            vec![Stmt {
                kind: StmtKind::Expr(Expr {
                    kind: ExprKind::SuperCall(vec![ArrElem::Spread(Expr {
                        kind: ExprKind::Ident(args),
                        pos,
                    })]),
                    pos,
                }),
                pos,
            }],
        )
    } else {
        (None, vec![])
    };
    Func {
        name: c.name.clone(),
        params: vec![],
        rest,
        body: FuncBody::Block(body),
        kind: if derived {
            FuncKind::DerivedConstructor
        } else {
            FuncKind::BaseConstructor
        },
        is_async: false,
        is_generator: false,
        strict: true,
        pos,
        src_start: c.src_start,
        src_end: c.src_end,
        fields: None,
    }
}
