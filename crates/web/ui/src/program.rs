//! What a runtime runs: a [`Program`].
//!
//! An app's code reaches the runtime in one of two forms, behind one trait:
//!
//! * [`IrProgram`]: the UI IR (`ir::Module`) as `cw-tsx` emits it, walked by the
//!   interpreter. What a page or a world loads at run time: nothing is compiled
//!   there (no rustc in the world or in Wasm).
//! * [`GenProgram`]: the same IR translated ahead of time into Rust by `cw-tsx build
//!   --emit rust` (see `cw_tsx::emit_rust`), for apps built into the binary. Every
//!   function is a Rust `fn`, templates are `static` data, hole dependencies are
//!   baked into the code, and nothing is parsed at boot.
//!
//! Both keep the IR's numbering (function `n`, template `t`, global `g`, hook order),
//! and both run on the same reconciler, hooks, events and commit, so a document,
//! a log or a snapshot does not depend on which one ran. A snapshot of an
//! interpreted app carries its IR; one of a generated app carries only
//! [`ProgramId`] (the app's name and a hash of the IR it was generated from). Either
//! restores on either form of the same IR (`UiApp::restore_with`), since the hash
//! says whether the two are the same program.
//!
//! Async functions are the one part a generated program does not translate: their
//! bodies stay IR (a few hundred bytes of JSON in the generated module, parsed the
//! first time one is called) and run on the interpreter's suspendable walk, whose
//! continuations are positions in those bodies; that keeps a suspended task in a
//! snapshot restorable by either form. Everything they call is generated code.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::interp::Frame;
use crate::ir::{self, Capture, Function, Module, TAttr, TNode};
use crate::runtime::{Runtime, R};
use crate::value::{Closure, Value};

/// Which program a snapshot belongs to: the app's name and a hash of its IR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramId {
    pub name: String,
    /// FNV-1a 64 of the IR's canonical JSON (`serde_json::to_string(&module)`), hex.
    pub hash: String,
}

/// The hash a [`ProgramId`] carries: FNV-1a 64 over the IR's canonical JSON.
pub fn ir_hash(module: &Module) -> u64 {
    let text = serde_json::to_string(module).expect("IR serialises");
    fnv1a(text.as_bytes())
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

pub fn hash_hex(h: u64) -> String {
    format!("{h:016x}")
}

/// A host-element template as the runtime reads it: the IR's, or a generated
/// program's `static` copy.
#[derive(Clone, Copy, Debug)]
pub enum TemplateRef<'a> {
    Ir(&'a ir::Template),
    Static(&'static STemplate),
}

impl TemplateRef<'_> {
    pub fn n_holes(&self) -> usize {
        match self {
            TemplateRef::Ir(t) => t.holes.len(),
            TemplateRef::Static(t) => t.n_holes as usize,
        }
    }
}

/// A template in a generated program: `ir::Template`'s skeleton as `static` data.
/// Hole dependencies are not here; the generated code checks them itself.
#[derive(Debug)]
pub struct STemplate {
    pub root: STNode,
    pub n_holes: u32,
}

#[derive(Debug)]
pub enum STNode {
    Element {
        tag: &'static str,
        attrs: &'static [STAttr],
        children: &'static [STNode],
    },
    Text(&'static str),
    Hole(u32),
}

#[derive(Debug)]
pub enum STAttr {
    Static(&'static str, &'static str),
    Dynamic(&'static str, u32),
    Spread(u32),
    Ref(u32),
}

/// One template node, from either representation (see `dom::build`).
pub(crate) enum TView<'a, N> {
    Element {
        tag: &'a str,
        attrs: AttrsView<'a>,
        children: &'a [N],
    },
    Text(&'a str),
    Hole(u32),
}

#[derive(Clone, Copy)]
pub(crate) enum AttrsView<'a> {
    Ir(&'a [TAttr]),
    Static(&'static [STAttr]),
}

#[derive(Clone, Copy)]
pub(crate) enum AttrView<'a> {
    Static(&'a str, &'a str),
    Dynamic(&'a str, u32),
    Spread(u32),
    Ref(u32),
}

impl<'a> AttrsView<'a> {
    pub fn len(&self) -> usize {
        match self {
            AttrsView::Ir(a) => a.len(),
            AttrsView::Static(a) => a.len(),
        }
    }
    pub fn get(&self, i: usize) -> AttrView<'a> {
        match self {
            AttrsView::Ir(a) => match &a[i] {
                TAttr::Static(n, v) => AttrView::Static(n, v),
                TAttr::Dynamic(n, h) => AttrView::Dynamic(n, *h),
                TAttr::Spread(h) => AttrView::Spread(*h),
                TAttr::Ref(h) => AttrView::Ref(*h),
            },
            AttrsView::Static(a) => match &a[i] {
                STAttr::Static(n, v) => AttrView::Static(n, v),
                STAttr::Dynamic(n, h) => AttrView::Dynamic(n, *h),
                STAttr::Spread(h) => AttrView::Spread(*h),
                STAttr::Ref(h) => AttrView::Ref(*h),
            },
        }
    }
}

pub(crate) trait TNodeView: Sized {
    fn view(&self) -> TView<'_, Self>;
}

impl TNodeView for TNode {
    fn view(&self) -> TView<'_, TNode> {
        match self {
            TNode::Element {
                tag,
                attrs,
                children,
            } => TView::Element {
                tag,
                attrs: AttrsView::Ir(attrs),
                children,
            },
            TNode::Text(s) => TView::Text(s),
            TNode::Hole(h) => TView::Hole(*h),
        }
    }
}

impl TNodeView for STNode {
    fn view(&self) -> TView<'_, STNode> {
        match self {
            STNode::Element {
                tag,
                attrs,
                children,
            } => TView::Element {
                tag,
                attrs: AttrsView::Static(attrs),
                children,
            },
            STNode::Text(s) => TView::Text(s),
            STNode::Hole(h) => TView::Hole(*h),
        }
    }
}

/// An app's code, interpreted or generated. See the module documentation.
pub trait Program {
    /// The app's name (the IR's `source`).
    fn name(&self) -> &str;
    /// [`ir_hash`] of the IR this program is.
    fn ir_hash(&self) -> u64;
    /// The IR, when this program interprets it (a snapshot then carries it).
    fn module(&self) -> Option<&Rc<Module>> {
        None
    }
    fn globals_len(&self) -> usize;
    /// The `id` of the element the app renders into.
    fn container_id(&self) -> Option<&str>;
    /// The function returning the root element.
    fn root_element(&self) -> u32;
    /// Whether hole skipping and element reuse are sound (`render::is_pure`).
    fn pure_render(&self) -> bool;
    /// How many parameters function `f` declares.
    fn arity(&self, f: u32) -> usize;
    /// Whether function `f` is a `forwardRef` render function.
    fn forward_ref(&self, f: u32) -> bool;
    /// What a closure of function `f` copies when created.
    fn captures(&self, f: u32) -> &[Capture];
    /// Frame slots of function `f` that live in a shared cell.
    fn boxed(&self, f: u32) -> &[u32];
    fn templates_len(&self) -> usize;
    fn template(&self, t: u32) -> TemplateRef<'_>;
    /// Function `f`'s IR, where the interpreter runs it (every function of an
    /// [`IrProgram`], the async ones of a [`GenProgram`]).
    fn function_ir(&self, f: u32) -> Option<&Function>;
    /// Initialises the module's globals, in declaration order.
    fn boot_globals(&self, rt: &mut Runtime) -> R<()>;
    /// The program's island script, when it has code on the JS VM.
    fn island_script(&self) -> Option<&str> {
        None
    }
    /// Calls a closure of this program; `inst` marks a component's render.
    fn call(
        &self,
        rt: &mut Runtime,
        c: &Rc<Closure>,
        args: Vec<Value>,
        inst: Option<u32>,
    ) -> R<Value>;
    /// The program's identity for a snapshot.
    fn id(&self) -> ProgramId {
        ProgramId {
            name: self.name().to_owned(),
            hash: hash_hex(self.ir_hash()),
        }
    }
}

impl std::fmt::Debug for dyn Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Program({})", self.name())
    }
}

/// Whether hole skipping and element reuse are sound for `module` (what a generated
/// program bakes in as `pure_render`).
pub fn pure_render(module: &Module) -> bool {
    crate::render::is_pure(module)
}

// ---------------------------------------------------------------- interpreted

/// The IR, run by the interpreter.
pub struct IrProgram {
    module: Rc<Module>,
    pure: bool,
    hash: OnceCell<u64>,
}

impl IrProgram {
    pub fn new(module: Module) -> IrProgram {
        Self::from_rc(Rc::new(module))
    }

    pub fn from_rc(module: Rc<Module>) -> IrProgram {
        let pure = crate::render::is_pure(&module);
        IrProgram {
            module,
            pure,
            hash: OnceCell::new(),
        }
    }
}

impl Program for IrProgram {
    fn name(&self) -> &str {
        &self.module.source
    }
    fn ir_hash(&self) -> u64 {
        *self.hash.get_or_init(|| ir_hash(&self.module))
    }
    fn module(&self) -> Option<&Rc<Module>> {
        Some(&self.module)
    }
    fn island_script(&self) -> Option<&str> {
        self.module.island.as_ref().map(|i| i.script.as_str())
    }
    fn globals_len(&self) -> usize {
        self.module.globals.len()
    }
    fn container_id(&self) -> Option<&str> {
        self.module.root.as_ref().map(|r| r.container_id.as_str())
    }
    fn root_element(&self) -> u32 {
        self.module.root.as_ref().map(|r| r.element).unwrap_or(0)
    }
    fn pure_render(&self) -> bool {
        self.pure
    }
    fn arity(&self, f: u32) -> usize {
        self.module.functions[f as usize].arity()
    }
    fn forward_ref(&self, f: u32) -> bool {
        self.module.functions[f as usize].forward_ref
    }
    fn captures(&self, f: u32) -> &[Capture] {
        &self.module.functions[f as usize].captures
    }
    fn boxed(&self, f: u32) -> &[u32] {
        &self.module.functions[f as usize].boxed
    }
    fn templates_len(&self) -> usize {
        self.module.templates.len()
    }
    fn template(&self, t: u32) -> TemplateRef<'_> {
        TemplateRef::Ir(&self.module.templates[t as usize])
    }
    fn function_ir(&self, f: u32) -> Option<&Function> {
        self.module.functions.get(f as usize)
    }
    fn boot_globals(&self, rt: &mut Runtime) -> R<()> {
        let module = &self.module;
        for (i, g) in module.globals.iter().enumerate() {
            match &g.init {
                ir::GlobalInit::Function(f) => {
                    rt.globals[i] = Value::Func(Rc::new(Closure {
                        func: *f,
                        captures: Vec::new(),
                    }));
                }
                ir::GlobalInit::Context(_) => rt.globals[i] = Value::Context(i as u32),
                _ => {}
            }
        }
        let mut frame = Frame::bare();
        for (i, g) in module.globals.iter().enumerate() {
            match &g.init {
                ir::GlobalInit::Expr(e) => {
                    let v = rt.eval(&mut frame, e)?;
                    rt.globals[i] = v;
                }
                ir::GlobalInit::Context(e) => {
                    let v = rt.eval(&mut frame, e)?;
                    rt.ctx_defaults.insert(i as u32, v);
                }
                ir::GlobalInit::Island(k) => {
                    rt.globals[i] = rt.island_export(*k)?;
                }
                ir::GlobalInit::Run(f) => {
                    rt.call_closure(
                        &Rc::new(Closure {
                            func: *f,
                            captures: Vec::new(),
                        }),
                        Vec::new(),
                        None,
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn call(
        &self,
        rt: &mut Runtime,
        c: &Rc<Closure>,
        args: Vec<Value>,
        inst: Option<u32>,
    ) -> R<Value> {
        let f = &self.module.functions[c.func as usize];
        rt.interpret_call(f, c, args, inst)
    }
}

// ---------------------------------------------------------------- generated

/// A generated function: the program's runtime, the closure's captures, the
/// arguments, and the instance when it runs as a component's render.
pub type GenFn = fn(&mut Runtime, &[Value], Vec<Value>, Option<u32>) -> R<Value>;

/// One function of a generated program.
#[derive(Debug)]
pub struct GenFunc {
    pub name: &'static str,
    pub arity: u32,
    pub captures: &'static [Capture],
    pub boxed: &'static [u32],
    /// `None` for an async function, which runs on the interpreter over its IR.
    pub code: Option<GenFn>,
    /// A `forwardRef` render function (see `ir::Function::forward_ref`).
    pub forward_ref: bool,
}

/// A program generated ahead of time (`cw-tsx build --emit rust`): static tables
/// and functions. A generated module defines one as a `static`.
pub struct GenProgram {
    pub name: &'static str,
    pub hash: u64,
    pub container_id: Option<&'static str>,
    pub root_element: u32,
    pub pure_render: bool,
    pub n_globals: u32,
    pub funcs: &'static [GenFunc],
    pub templates: &'static [STemplate],
    /// Initialises the globals in declaration order.
    pub init: fn(&mut Runtime) -> R<()>,
    /// The async functions' IR as JSON (`[[index, Function], ...]`), parsed the first
    /// time one is called; empty when there are none.
    pub async_ir: &'static str,
    pub async_cache: OnceLock<BTreeMap<u32, Function>>,
    /// The island's script (`ir::Island::script`); empty when there is none.
    pub island: &'static str,
}

impl GenProgram {
    fn async_functions(&self) -> &BTreeMap<u32, Function> {
        self.async_cache.get_or_init(|| {
            if self.async_ir.is_empty() {
                return BTreeMap::new();
            }
            let list: Vec<(u32, Function)> =
                serde_json::from_str(self.async_ir).expect("generated async IR parses");
            list.into_iter().collect()
        })
    }
}

impl std::fmt::Debug for GenProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GenProgram({} {:016x})", self.name, self.hash)
    }
}

impl Program for GenProgram {
    fn name(&self) -> &str {
        self.name
    }
    fn ir_hash(&self) -> u64 {
        self.hash
    }
    fn globals_len(&self) -> usize {
        self.n_globals as usize
    }
    fn container_id(&self) -> Option<&str> {
        self.container_id
    }
    fn root_element(&self) -> u32 {
        self.root_element
    }
    fn pure_render(&self) -> bool {
        self.pure_render
    }
    fn arity(&self, f: u32) -> usize {
        self.funcs[f as usize].arity as usize
    }
    fn forward_ref(&self, f: u32) -> bool {
        self.funcs[f as usize].forward_ref
    }
    fn captures(&self, f: u32) -> &[Capture] {
        self.funcs[f as usize].captures
    }
    fn boxed(&self, f: u32) -> &[u32] {
        self.funcs[f as usize].boxed
    }
    fn templates_len(&self) -> usize {
        self.templates.len()
    }
    fn template(&self, t: u32) -> TemplateRef<'_> {
        TemplateRef::Static(&self.templates[t as usize])
    }
    fn function_ir(&self, f: u32) -> Option<&Function> {
        if self.funcs.get(f as usize)?.code.is_some() {
            return None;
        }
        self.async_functions().get(&f)
    }
    fn boot_globals(&self, rt: &mut Runtime) -> R<()> {
        (self.init)(rt)
    }
    fn island_script(&self) -> Option<&str> {
        (!self.island.is_empty()).then_some(self.island)
    }
    fn call(
        &self,
        rt: &mut Runtime,
        c: &Rc<Closure>,
        args: Vec<Value>,
        inst: Option<u32>,
    ) -> R<Value> {
        match self.funcs[c.func as usize].code {
            Some(code) => code(rt, &c.captures, args, inst),
            None => {
                let f = self
                    .async_functions()
                    .get(&c.func)
                    .expect("an async function's IR is in its generated program");
                rt.interpret_call(f, c, args, inst)
            }
        }
    }
}

/// A generated program as a runtime holds it.
pub struct StaticProgram(pub &'static GenProgram);

impl Program for StaticProgram {
    fn name(&self) -> &str {
        self.0.name()
    }
    fn ir_hash(&self) -> u64 {
        self.0.ir_hash()
    }
    fn globals_len(&self) -> usize {
        self.0.globals_len()
    }
    fn container_id(&self) -> Option<&str> {
        self.0.container_id()
    }
    fn root_element(&self) -> u32 {
        self.0.root_element()
    }
    fn pure_render(&self) -> bool {
        self.0.pure_render()
    }
    fn arity(&self, f: u32) -> usize {
        self.0.arity(f)
    }
    fn forward_ref(&self, f: u32) -> bool {
        self.0.forward_ref(f)
    }
    fn captures(&self, f: u32) -> &[Capture] {
        self.0.captures(f)
    }
    fn boxed(&self, f: u32) -> &[u32] {
        self.0.boxed(f)
    }
    fn templates_len(&self) -> usize {
        self.0.templates_len()
    }
    fn template(&self, t: u32) -> TemplateRef<'_> {
        self.0.template(t)
    }
    fn function_ir(&self, f: u32) -> Option<&Function> {
        self.0.function_ir(f)
    }
    fn boot_globals(&self, rt: &mut Runtime) -> R<()> {
        self.0.boot_globals(rt)
    }
    fn island_script(&self) -> Option<&str> {
        self.0.island_script()
    }
    fn call(
        &self,
        rt: &mut Runtime,
        c: &Rc<Closure>,
        args: Vec<Value>,
        inst: Option<u32>,
    ) -> R<Value> {
        self.0.call(rt, c, args, inst)
    }
}

fn registry() -> std::sync::MutexGuard<'static, Vec<&'static GenProgram>> {
    static REGISTRY: std::sync::Mutex<Vec<&'static GenProgram>> = std::sync::Mutex::new(Vec::new());
    match REGISTRY.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Makes a generated program findable by [`find`] (and so by `UiApp::restore` for
/// a snapshot naming it), in every thread. Registering twice is harmless.
pub fn register(p: &'static GenProgram) {
    let mut r = registry();
    if !r.iter().any(|x| std::ptr::eq(*x, p)) {
        r.push(p);
    }
}

/// The registered generated program with this identity.
pub fn find(id: &ProgramId) -> Option<&'static GenProgram> {
    registry()
        .iter()
        .copied()
        .find(|p| p.name == id.name && hash_hex(p.hash) == id.hash)
}
