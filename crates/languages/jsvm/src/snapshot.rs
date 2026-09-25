//! Heap snapshots: a VM between two entry points, written to bytes and read
//! back into a VM that behaves identically from there on.
//!
//! What is written. Every object reachable from the VM's roots (its
//! intrinsics, global, job queues, timers, module table and the rest of its
//! fields) and from the embedder's roots, with each object's prototype,
//! properties in order, internal kind and slots; closure cells, symbols,
//! array buffers and strings, each once, so sharing and identity survive; the
//! suspended frames of generators and async functions; and the VM's own
//! state (clock, step count, entropy, console bookkeeping). Output is
//! deterministic: objects are numbered in the order a breadth-first walk
//! from the roots first reaches them, so the same heap writes the same bytes
//! whatever its addresses, and a restored VM snapshots to the same bytes as
//! the VM it came from.
//!
//! Compiled code is not written. A closure refers to its code as a
//! compilation unit (the source, its file name and how it was compiled, the
//! inputs of the compile cache's key) and the position of the body in that
//! unit's nesting; a restore compiles each unit again, or takes it from the
//! compile cache, exactly as loading the source would. What the realm keeps
//! per body travels with the unit: which bodies it has been charged the
//! compile cost for (the virtual clock) and its tagged-template objects. Units
//! no live function or frame refers to are left out, since nothing can run
//! them again.
//!
//! Native functions are written as their distance from a function of this
//! crate, so a snapshot is readable only by the program image that wrote it
//! (the header's fingerprint checks this); host hooks are written as their
//! position in the list the embedder passes. Snapshots are for handing a
//! heap to another VM of the same process (a fork, a restore, a pre-booted
//! template), not for storage.
//!
//! What cannot be written, and is refused: a VM with frames on its stack (a
//! snapshot is taken between entry points), a debugger, workers, pending
//! messages or native handles, or an object that is borrowed.

use crate::bigint::BigInt;
use crate::bytecode::Code;
use crate::codecache::CacheKey;
use crate::value::*;
use crate::vm::*;
use cw_script_host::ScriptHost;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::BuildHasherDefault;
use std::rc::{Rc, Weak};

const MAGIC: &[u8; 8] = b"CWJSHEAP";
const VERSION: u32 = 1;

/// Why a VM could not be written or read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotError(pub String);

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SnapshotError {}

type R<T> = Result<T, SnapshotError>;

fn err<T>(m: impl Into<String>) -> R<T> {
    Err(SnapshotError(m.into()))
}

/// How a compilation unit was compiled: the parts of its compile-cache key
/// other than the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// `Vm::compile_source`.
    Program {
        force_module: Option<bool>,
        params: String,
        file: String,
    },
    /// `Vm::eval_source_with`.
    Eval { global: bool, file: String },
}

/// One compile a VM made: enough to compile it again.
pub struct Unit {
    pub kind: UnitKind,
    pub src: Rc<str>,
    pub code: Weak<Code>,
}

/// What the embedder passes to both sides.
#[derive(Clone, Copy)]
pub struct Options<'a> {
    /// Every `HostHooks` the embedder's objects can have.
    pub hooks: &'a [&'static HostHooks],
    /// Mixed into the header's fingerprint: something that changes when the
    /// embedder's native functions move (see `fingerprint_of`).
    pub fingerprint: u64,
}

fn anchor(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Undefined)
}

fn fn_offset(f: NativeFn) -> i64 {
    (f as usize as i64).wrapping_sub(anchor as NativeFn as usize as i64)
}

/// A fingerprint of the program image: the distances between a few
/// functions, which move when the code does. An embedder passes one made
/// from functions of its own as `Options::fingerprint`.
pub fn fingerprint_of(fns: &[NativeFn]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in fns {
        h ^= fn_offset(*f) as u64;
        h = h.wrapping_mul(0x0100_0000_01b3).rotate_left(17);
    }
    h
}

fn own_fingerprint() -> u64 {
    let base = anchor as NativeFn as usize as u64;
    let mut h = fingerprint_of(&[probe]);
    for f in [
        crate::gc::collect as fn() -> crate::gc::Stats as usize,
        crate::codecache::clear as fn() as usize,
        crate::regexp::compile_for_restore as fn(&str, &str) -> Result<cw_regex::Regex, String>
            as usize,
    ] {
        h = (h ^ (f as u64).wrapping_sub(base)).wrapping_mul(0x0100_0000_01b3);
    }
    for b in env!("CARGO_PKG_VERSION").bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn probe(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Null)
}

// ---------------------------------------------------------------- bytes

#[derive(Default)]
struct Out {
    b: Vec<u8>,
}

impl Out {
    #[inline]
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    #[inline]
    fn uv(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.b.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.b.push(v as u8);
    }
    fn iv(&mut self, v: i64) {
        self.uv(((v << 1) ^ (v >> 63)) as u64);
    }
    fn f64(&mut self, v: f64) {
        self.b.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fn bool(&mut self, v: bool) {
        self.b.push(v as u8);
    }
    fn bytes(&mut self, v: &[u8]) {
        self.uv(v.len() as u64);
        self.b.extend_from_slice(v);
    }
    fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
}

struct In<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> In<'a> {
    #[inline]
    fn u8(&mut self) -> R<u8> {
        match self.b.get(self.p) {
            Some(v) => {
                self.p += 1;
                Ok(*v)
            }
            None => err("snapshot truncated"),
        }
    }
    #[inline]
    fn uv(&mut self) -> R<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            if shift >= 64 {
                return err("bad varint");
            }
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
        }
    }
    fn us(&mut self) -> R<usize> {
        Ok(self.uv()? as usize)
    }
    fn u32(&mut self) -> R<u32> {
        let v = self.uv()?;
        u32::try_from(v).map_err(|_| SnapshotError("bad u32".into()))
    }
    fn iv(&mut self) -> R<i64> {
        let v = self.uv()?;
        Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
    }
    fn f64(&mut self) -> R<f64> {
        let s = self.raw(8)?;
        Ok(f64::from_bits(u64::from_le_bytes(s.try_into().unwrap())))
    }
    fn bool(&mut self) -> R<bool> {
        Ok(self.u8()? != 0)
    }
    fn raw(&mut self, n: usize) -> R<&'a [u8]> {
        if self.p + n > self.b.len() {
            return err("snapshot truncated");
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn bytes(&mut self) -> R<&'a [u8]> {
        let n = self.us()?;
        self.raw(n)
    }
    fn str(&mut self) -> R<&'a str> {
        std::str::from_utf8(self.bytes()?).map_err(|_| SnapshotError("bad utf-8".into()))
    }
    fn string(&mut self) -> R<String> {
        Ok(self.str()?.to_owned())
    }
}

// ---------------------------------------------------------------- tags

const V_UNDEF: u8 = 0;
const V_NULL: u8 = 1;
const V_FALSE: u8 = 2;
const V_TRUE: u8 = 3;
const V_NUM: u8 = 4;
const V_INT: u8 = 5;
const V_STR: u8 = 6;
const V_BIG: u8 = 7;
const V_SYM: u8 = 8;
const V_OBJ: u8 = 9;
const V_EMPTY: u8 = 10;

const REC_OBJ: u8 = 1;
const REC_CELL: u8 = 2;
const REC_END: u8 = 3;

const TAGS: &[&str] = &["Module"];

type Fast<K, V> = HashMap<K, V, BuildHasherDefault<FastHash>>;

// ---------------------------------------------------------------- writer

enum Node {
    Obj(Obj),
    Cell(CellRef),
}

struct W<'v, 'h> {
    o: Out,
    vm: &'v Vm<'h>,
    hooks: &'v [&'static HostHooks],
    objs: Fast<usize, u32>,
    cells: Fast<usize, u32>,
    queue: VecDeque<Node>,
    strs: Fast<JsStr, u32>,
    str_at: Fast<usize, u32>,
    rcstrs: Fast<Rc<str>, u32>,
    syms: Fast<usize, u32>,
    bufs: Fast<usize, u32>,
    /// Code body address -> (index in `vm.units`, position in the unit).
    code_at: Fast<usize, (usize, u32)>,
    /// `vm.units` index -> the unit's number in this snapshot.
    unit_ids: Fast<usize, u32>,
}

fn preorder(root: &Rc<Code>, out: &mut Vec<Rc<Code>>) {
    out.push(root.clone());
    for c in &root.codes {
        preorder(c, out);
    }
}

impl<'v, 'h> W<'v, 'h> {
    fn obj(&mut self, o: &Obj) {
        let n = self.objs.len() as u32;
        let id = *self.objs.entry(o.addr()).or_insert(n);
        if id == n {
            self.queue.push_back(Node::Obj(o.clone()));
        }
        self.o.uv(id as u64);
    }
    fn opt_obj(&mut self, o: &Option<Obj>) {
        match o {
            None => self.o.u8(0),
            Some(o) => {
                self.o.u8(1);
                self.obj(o);
            }
        }
    }
    fn cell(&mut self, c: &CellRef) {
        let n = self.cells.len() as u32;
        let id = *self
            .cells
            .entry(Rc::as_ptr(c) as *const u8 as usize)
            .or_insert(n);
        if id == n {
            self.queue.push_back(Node::Cell(c.clone()));
        }
        self.o.uv(id as u64);
    }
    fn jstr(&mut self, s: &JsStr) {
        let at = Rc::as_ptr(&s.0) as *const u8 as usize;
        if let Some(&id) = self.str_at.get(&at) {
            self.o.uv(id as u64 + 1);
            return;
        }
        let n = self.strs.len() as u32;
        let id = *self.strs.entry(s.clone()).or_insert(n);
        // Keyed by address too: nothing is freed while a snapshot is written
        // (the heap is not touched), so an address names one string throughout.
        self.str_at.insert(at, id);
        if id == n {
            self.o.uv(0);
            self.o.str(s.as_str());
        } else {
            self.o.uv(id as u64 + 1);
        }
    }
    fn rcstr(&mut self, s: &Rc<str>) {
        let n = self.rcstrs.len() as u32;
        let id = *self.rcstrs.entry(s.clone()).or_insert(n);
        if id == n {
            self.o.uv(0);
            self.o.str(s);
        } else {
            self.o.uv(id as u64 + 1);
        }
    }
    fn sym(&mut self, s: &Rc<Symbol>) {
        let n = self.syms.len() as u32;
        let id = *self
            .syms
            .entry(Rc::as_ptr(s) as *const u8 as usize)
            .or_insert(n);
        if id == n {
            self.o.uv(0);
            match &s.desc {
                None => self.o.u8(0),
                Some(d) => {
                    self.o.u8(1);
                    self.jstr(d);
                }
            }
            self.o.bool(s.private);
            self.o.bool(s.registered);
        } else {
            self.o.uv(id as u64 + 1);
        }
    }
    fn buf(&mut self, b: &Rc<RefCell<Vec<u8>>>) -> R<()> {
        let n = self.bufs.len() as u32;
        let id = *self
            .bufs
            .entry(Rc::as_ptr(b) as *const u8 as usize)
            .or_insert(n);
        if id == n {
            self.o.uv(0);
            let Ok(v) = b.try_borrow() else {
                return err("an array buffer is borrowed");
            };
            self.o.bytes(&v);
        } else {
            self.o.uv(id as u64 + 1);
        }
        Ok(())
    }
    fn native(&mut self, f: NativeFn) {
        self.o.iv(fn_offset(f));
    }
    fn big(&mut self, b: &BigInt) {
        self.o.str(&b.to_str_radix(16));
    }
    fn value(&mut self, v: &Value) {
        match v {
            Value::Undefined => self.o.u8(V_UNDEF),
            Value::Null => self.o.u8(V_NULL),
            Value::Bool(false) => self.o.u8(V_FALSE),
            Value::Bool(true) => self.o.u8(V_TRUE),
            Value::Num(n) => {
                let i = *n as i32;
                if i as f64 == *n && !(i == 0 && n.is_sign_negative()) {
                    self.o.u8(V_INT);
                    self.o.iv(i as i64);
                } else {
                    self.o.u8(V_NUM);
                    self.o.f64(*n);
                }
            }
            Value::Str(s) => {
                self.o.u8(V_STR);
                self.jstr(s);
            }
            Value::BigInt(b) => {
                self.o.u8(V_BIG);
                self.big(b);
            }
            Value::Sym(s) => {
                self.o.u8(V_SYM);
                self.sym(s);
            }
            Value::Obj(o) => {
                self.o.u8(V_OBJ);
                self.obj(o);
            }
            Value::Empty => self.o.u8(V_EMPTY),
        }
    }
    fn values(&mut self, vs: &[Value]) {
        self.o.uv(vs.len() as u64);
        for v in vs {
            self.value(v);
        }
    }
    fn opt_value(&mut self, v: &Option<Value>) {
        match v {
            None => self.o.u8(0),
            Some(v) => {
                self.o.u8(1);
                self.value(v);
            }
        }
    }
    fn key(&mut self, k: &Key) {
        match k {
            Key::Str(s) => {
                self.o.u8(0);
                self.jstr(s);
            }
            Key::Sym(s) => {
                self.o.u8(1);
                self.sym(s);
            }
        }
    }
    fn site(&mut self, s: &Option<Site>) {
        match s {
            None => self.o.u8(0),
            Some(s) => {
                self.o.u8(1);
                self.rcstr(&s.file);
                self.o.uv(s.line as u64);
                self.o.uv(s.col as u64);
            }
        }
    }

    /// A reference to a code body: its unit (defined on first reference) and
    /// its position there.
    fn code(&mut self, c: &Rc<Code>) -> R<()> {
        if self.code_at.is_empty() {
            self.index_units();
        }
        let Some(&(unit, at)) = self.code_at.get(&(Rc::as_ptr(c) as usize)) else {
            return err(format!(
                "code of {} ({}) belongs to no compilation unit",
                c.name.as_str(),
                c.file
            ));
        };
        let n = self.unit_ids.len() as u32;
        let id = *self.unit_ids.entry(unit).or_insert(n);
        if id == n {
            self.o.uv(0);
            self.unit_def(unit)?;
        } else {
            self.o.uv(id as u64 + 1);
        }
        self.o.uv(at as u64);
        Ok(())
    }

    fn index_units(&mut self) {
        // A sentinel so an empty table is not indexed again.
        self.code_at.insert(0, (usize::MAX, 0));
        for (i, u) in self.vm.units.iter().enumerate() {
            let Some(root) = u.code.upgrade() else {
                continue;
            };
            let mut all = Vec::new();
            preorder(&root, &mut all);
            for (at, c) in all.iter().enumerate() {
                self.code_at
                    .entry(Rc::as_ptr(c) as usize)
                    .or_insert((i, at as u32));
            }
        }
    }

    fn unit_def(&mut self, unit: usize) -> R<()> {
        let u = &self.vm.units[unit];
        let root = u.code.upgrade().expect("indexed units are live");
        match &u.kind {
            UnitKind::Program {
                force_module,
                params,
                file,
            } => {
                self.o.u8(0);
                self.o.u8(match force_module {
                    None => 0,
                    Some(false) => 1,
                    Some(true) => 2,
                });
                self.o.str(params);
                self.o.str(file);
            }
            UnitKind::Eval { global, file } => {
                self.o.u8(1);
                self.o.bool(*global);
                self.o.str(file);
            }
        }
        let src = u.src.clone();
        self.rcstr(&src);
        let mut all = Vec::new();
        preorder(&root, &mut all);
        self.o.uv(all.len() as u64);
        // Which bodies this realm has been charged for, eight to a byte.
        let mut byte = 0u8;
        for (i, c) in all.iter().enumerate() {
            let by = c.compiled_by.get();
            let charged = by == self.vm.id || (by != 0 && self.vm.compiled.contains(&c.uid));
            if charged {
                byte |= 1 << (i % 8);
            }
            if i % 8 == 7 {
                self.o.u8(byte);
                byte = 0;
            }
        }
        if all.len() % 8 != 0 {
            self.o.u8(byte);
        }
        // Tagged-template objects, by body and site.
        let mut tpl: Vec<(u32, u32, Obj)> = Vec::new();
        for (i, c) in all.iter().enumerate() {
            for site in 0..c.templates.len() as u32 {
                if let Some(o) = self.vm.templates.get(&(c.uid, site)) {
                    tpl.push((i as u32, site, o.clone()));
                }
            }
        }
        self.o.uv(tpl.len() as u64);
        for (i, site, o) in tpl {
            self.o.uv(i as u64);
            self.o.uv(site as u64);
            self.obj(&o);
        }
        Ok(())
    }

    fn frame(&mut self, f: &Frame) -> R<()> {
        self.code(&f.code)?;
        self.o.uv(f.pc as u64);
        self.values(&f.stack);
        self.o.uv(f.locals.len() as u64);
        for l in &f.locals {
            match l {
                Local::V(v) => {
                    self.o.u8(0);
                    self.value(v);
                }
                Local::C(c) => {
                    self.o.u8(1);
                    self.cell(c);
                }
            }
        }
        self.o.uv(f.captures.len() as u64);
        for c in f.captures.iter() {
            self.cell(c);
        }
        self.o.uv(f.handlers.len() as u64);
        for h in &f.handlers {
            self.o.uv(h.pc as u64);
            self.o.uv(h.depth as u64);
            self.o.bool(h.finally);
        }
        self.values(&f.args);
        self.opt_obj(&f.func);
        self.value(&f.recv);
        match &f.kind {
            FrameKind::Normal => self.o.u8(0),
            FrameKind::Construct(v) => {
                self.o.u8(1);
                self.value(v);
            }
            FrameKind::Generator(o) => {
                self.o.u8(2);
                self.obj(o);
            }
            FrameKind::Async { promise, co, first } => {
                self.o.u8(3);
                self.obj(promise);
                self.obj(co);
                self.o.bool(*first);
            }
        }
        match &f.resume {
            None => self.o.u8(0),
            Some(Resume::Throw(v)) => {
                self.o.u8(1);
                self.value(v);
            }
            Some(Resume::Return(v)) => {
                self.o.u8(2);
                self.value(v);
            }
        }
        self.o.u8(f.ystar_mode);
        self.o.bool(f.resumed);
        self.o.u8(f.timer);
        Ok(())
    }

    fn opt_frame(&mut self, f: &Option<Box<Frame>>) -> R<()> {
        match f {
            None => self.o.u8(0),
            Some(f) => {
                self.o.u8(1);
                self.frame(f)?;
            }
        }
        Ok(())
    }

    fn reaction(&mut self, r: &Reaction) {
        match r {
            Reaction::Then {
                handler,
                derived,
                cap,
            } => {
                self.o.u8(0);
                self.opt_value(handler);
                self.opt_obj(derived);
                match cap {
                    None => self.o.u8(0),
                    Some((a, b)) => {
                        self.o.u8(1);
                        self.value(a);
                        self.value(b);
                    }
                }
            }
            Reaction::Resume(o) => {
                self.o.u8(1);
                self.obj(o);
            }
            Reaction::Native(f, vs) => {
                self.o.u8(2);
                self.native(*f);
                self.values(vs);
            }
        }
    }

    fn map(&mut self, m: &MapData) {
        self.o.uv(m.entries.len() as u64);
        for e in &m.entries {
            match e {
                None => self.o.u8(0),
                Some((k, v)) => {
                    self.o.u8(1);
                    self.value(k);
                    self.value(v);
                }
            }
        }
    }

    fn record(&mut self, o: &Obj) -> R<()> {
        let Ok(d) = o.0.try_borrow() else {
            return err("an object is borrowed");
        };
        self.opt_obj(&d.proto);
        self.o
            .u8(d.extensible as u8 | (d.elems_frozen as u8) << 1 | (d.elems_sealed as u8) << 2);
        match d.tag {
            None => self.o.u8(0),
            Some(t) => match TAGS.iter().position(|x| *x == t) {
                Some(i) => self.o.u8(i as u8 + 1),
                None => return err(format!("unknown object tag {t}")),
            },
        }
        self.o.uv(d.props.entries.len() as u64);
        for (k, p) in &d.props.entries {
            self.key(k);
            self.o.u8(p.flags);
            match &p.slot {
                Slot::Data(v) => {
                    self.o.u8(0);
                    self.value(v);
                }
                Slot::Accessor(g, s) => {
                    self.o.u8(1);
                    self.opt_obj(g);
                    self.opt_obj(s);
                }
            }
        }
        match &d.kind {
            Kind::Ordinary => self.o.u8(0),
            Kind::Array(vs) => {
                self.o.u8(1);
                self.values(vs);
            }
            Kind::Function(fd) => {
                self.o.u8(2);
                match &fd.imp {
                    FuncImpl::Closure { code, captures } => {
                        self.o.u8(0);
                        self.code(code)?;
                        self.o.uv(captures.len() as u64);
                        for c in captures.iter() {
                            self.cell(c);
                        }
                    }
                    FuncImpl::Native { f, slots } => {
                        self.o.u8(1);
                        self.native(*f);
                        self.values(slots);
                    }
                    FuncImpl::Bound { target, this, args } => {
                        self.o.u8(2);
                        self.obj(target);
                        self.value(this);
                        self.values(args);
                    }
                }
                self.o.u8(match fd.ctor {
                    CtorKind::None => 0,
                    CtorKind::Base => 1,
                    CtorKind::Derived => 2,
                });
                self.o.bool(fd.class_ctor);
                self.opt_obj(&fd.home);
                self.opt_obj(&fd.fields);
            }
            Kind::Error(e) => {
                self.o.u8(3);
                self.o.uv(e.frames.len() as u64);
                for f in &e.frames {
                    self.o.str(f);
                }
                self.site(&e.site);
                match &e.arrow {
                    None => self.o.u8(0),
                    Some(a) => {
                        self.o.u8(1);
                        self.o.str(a);
                    }
                }
                self.o.bool(e.from_async);
            }
            Kind::Boolean(b) => {
                self.o.u8(4);
                self.o.bool(*b);
            }
            Kind::Number(n) => {
                self.o.u8(5);
                self.o.f64(*n);
            }
            Kind::String(s) => {
                self.o.u8(6);
                self.jstr(s);
            }
            Kind::Symbol(s) => {
                self.o.u8(7);
                self.sym(s);
            }
            Kind::BigInt(b) => {
                self.o.u8(8);
                self.big(b);
            }
            Kind::Date(t) => {
                self.o.u8(9);
                self.o.f64(*t);
            }
            Kind::RegExp(r) => {
                self.o.u8(10);
                self.jstr(&r.source);
                self.jstr(&r.flags);
                self.o.u8(r.global as u8
                    | (r.sticky as u8) << 1
                    | (r.unicode as u8) << 2
                    | (r.has_indices as u8) << 3);
            }
            Kind::Map(m) => {
                self.o.u8(11);
                self.map(m);
            }
            Kind::Set(m) => {
                self.o.u8(12);
                self.map(m);
            }
            Kind::WeakMap(m) => {
                self.o.u8(13);
                self.map(m);
            }
            Kind::WeakSet(m) => {
                self.o.u8(14);
                self.map(m);
            }
            Kind::WeakRef(v) => {
                self.o.u8(15);
                self.value(v);
            }
            Kind::Promise(p) => {
                self.o.u8(16);
                self.o.u8(match p.state {
                    PromiseState::Pending => 0,
                    PromiseState::Fulfilled => 1,
                    PromiseState::Rejected => 2,
                });
                self.value(&p.value);
                self.o.uv(p.fulfill.len() as u64);
                for r in &p.fulfill {
                    self.reaction(r);
                }
                self.o.uv(p.reject.len() as u64);
                for r in &p.reject {
                    self.reaction(r);
                }
                self.o.bool(p.handled);
                self.o.bool(p.resolving);
            }
            Kind::Generator(g) => {
                self.o.u8(17);
                self.o.u8(match g.state {
                    GenState::SuspendedStart => 0,
                    GenState::SuspendedYield => 1,
                    GenState::Running => 2,
                    GenState::Completed => 3,
                });
                self.opt_frame(&g.frame)?;
                self.o.uv(g.queue.len() as u64);
                for (k, v, o) in &g.queue {
                    self.o.u8(*k);
                    self.value(v);
                    self.obj(o);
                }
                self.o.bool(g.is_async);
            }
            Kind::Coroutine(f) => {
                self.o.u8(18);
                self.opt_frame(f)?;
            }
            Kind::ArrayIter {
                target,
                index,
                kind,
                done,
            } => {
                self.o.u8(19);
                self.value(target);
                self.o.uv(*index as u64);
                self.o.u8(iter_kind(*kind));
                self.o.bool(*done);
            }
            Kind::MapIter {
                target,
                index,
                kind,
                done,
            } => {
                self.o.u8(20);
                self.obj(target);
                self.o.uv(*index as u64);
                self.o.u8(iter_kind(*kind));
                self.o.bool(*done);
            }
            Kind::StringIter { s, pos, done } => {
                self.o.u8(21);
                self.jstr(s);
                self.o.uv(*pos as u64);
                self.o.bool(*done);
            }
            Kind::RegExpStringIter {
                re,
                s,
                global,
                unicode,
                done,
            } => {
                self.o.u8(22);
                self.obj(re);
                self.jstr(s);
                self.o.bool(*global);
                self.o.bool(*unicode);
                self.o.bool(*done);
            }
            Kind::ForIn { keys, index, obj } => {
                self.o.u8(23);
                self.o.uv(keys.len() as u64);
                for k in keys {
                    self.jstr(k);
                }
                self.o.uv(*index as u64);
                self.obj(obj);
            }
            Kind::Arguments => self.o.u8(24),
            Kind::ArrayBuffer(b) => {
                self.o.u8(25);
                self.buf(b)?;
            }
            Kind::TypedArray {
                kind,
                buf,
                offset,
                len,
                buf_obj,
            } => {
                self.o.u8(26);
                self.o.u8(typed_kind(*kind));
                self.buf(buf)?;
                self.o.uv(*offset as u64);
                self.o.uv(*len as u64);
                self.opt_obj(buf_obj);
            }
            Kind::Proxy { target, handler } => {
                self.o.u8(27);
                self.obj(target);
                self.obj(handler);
            }
            Kind::Internal(vs) => {
                self.o.u8(28);
                self.values(vs);
            }
            Kind::Host(h) => {
                self.o.u8(29);
                let Some(i) = self.hooks.iter().position(|x| std::ptr::eq(*x, h.hooks)) else {
                    return err(format!(
                        "host hooks of class {} are not registered",
                        h.hooks.class
                    ));
                };
                self.o.uv(i as u64);
                self.values(&h.data);
            }
        }
        Ok(())
    }

    fn job(&mut self, j: &Job) {
        match j {
            Job::Reaction {
                reaction,
                arg,
                rejected,
            } => {
                self.o.u8(0);
                self.reaction(reaction);
                self.value(arg);
                self.o.bool(*rejected);
            }
            Job::Thenable {
                promise,
                thenable,
                then,
            } => {
                self.o.u8(1);
                self.obj(promise);
                self.value(thenable);
                self.value(then);
            }
            Job::Callback(f, args) => {
                self.o.u8(2);
                self.value(f);
                self.values(args);
            }
        }
    }

    fn strings(&mut self, v: &[String]) {
        self.o.uv(v.len() as u64);
        for s in v {
            self.o.str(s);
        }
    }

    /// The VM's own fields, in declaration order.
    fn vm_fields(&mut self, roots: &[Value]) -> R<()> {
        let vm = self.vm;
        macro_rules! intr {
            ($($f:ident),*) => { $( self.obj(&vm.intr.$f); )* };
        }
        intr!(
            object_proto,
            function_proto,
            array_proto,
            string_proto,
            number_proto,
            boolean_proto,
            symbol_proto,
            bigint_proto
        );
        self.o.uv(vm.intr.error_protos.len() as u64);
        for o in &vm.intr.error_protos {
            self.obj(o);
        }
        self.o.uv(vm.intr.error_ctors.len() as u64);
        for o in &vm.intr.error_ctors {
            self.obj(o);
        }
        intr!(
            iterator_proto,
            async_iterator_proto,
            array_iter_proto,
            map_iter_proto,
            set_iter_proto,
            string_iter_proto,
            regexp_str_iter_proto,
            generator_proto,
            async_generator_proto,
            generator_function_proto,
            async_generator_function_proto,
            async_function_proto,
            promise_proto,
            promise_ctor,
            regexp_proto,
            date_proto,
            map_proto,
            set_proto,
            weakmap_proto,
            weakset_proto,
            weakref_proto,
            arraybuffer_proto
        );
        self.o.uv(vm.intr.typed_protos.len() as u64);
        for o in &vm.intr.typed_protos {
            self.obj(o);
        }
        intr!(
            array_iter_next,
            array_values,
            object_ctor,
            array_ctor,
            function_ctor,
            buffer_proto
        );
        macro_rules! syms {
            ($($f:ident),*) => { $( self.sym(&vm.syms.$f); )* };
        }
        syms!(
            iterator,
            async_iterator,
            has_instance,
            to_primitive,
            to_string_tag,
            species,
            is_concat_spreadable,
            unscopables,
            match_,
            match_all,
            replace,
            search,
            split,
            inspect_custom
        );
        self.obj(&vm.global);
        self.o.str(&vm.stdout);
        self.o.str(&vm.stderr);
        match &vm.stdin {
            None => self.o.u8(0),
            Some(s) => {
                self.o.u8(1);
                self.o.str(s);
            }
        }
        self.o.bool(vm.stdin_consumed);
        self.o.uv(vm.stdin_pos as u64);
        self.o.bool(vm.interactive);
        self.o.bool(vm.stdin_eof);
        self.o.bool(vm.awaiting_input);
        self.o.uv(vm.steps);
        self.o.uv(vm.budget);
        self.site(&vm.throw_site);
        self.o.u8(match vm.exit {
            Exit::Return => 0,
            Exit::Yield => 1,
            Exit::Await => 2,
        });
        self.o.uv(vm.microtasks.len() as u64);
        for j in &vm.microtasks {
            self.job(j);
        }
        self.o.uv(vm.ticks.len() as u64);
        for (f, a) in &vm.ticks {
            self.value(f);
            self.values(a);
        }
        self.o.uv(vm.timers.len() as u64);
        for t in &vm.timers {
            self.o.uv(t.id);
            self.o.f64(t.when);
            self.o.uv(t.seq);
            self.value(&t.callback);
            self.values(&t.args);
            match t.interval {
                None => self.o.u8(0),
                Some(i) => {
                    self.o.u8(1);
                    self.o.f64(i);
                }
            }
            self.obj(&t.obj);
            self.o.bool(t.immediate);
            self.o.bool(t.io);
            self.o.f64(t.dur);
        }
        self.o.uv(vm.timer_seq);
        self.o.uv(vm.timer_id);
        self.o.f64(vm.elapsed_ms);
        self.opt_value(&vm.loading_parent);
        self.o.uv(vm.clock_steps);
        self.o.iv(vm.start_micros);
        self.o.uv(vm.pending_rejections.len() as u64);
        for o in &vm.pending_rejections {
            self.obj(o);
        }
        self.o.uv(vm.modules.len() as u64);
        for (k, v) in &vm.modules {
            self.o.str(k);
            self.value(v);
        }
        self.strings(&vm.argv);
        self.o.uv(vm.env.len() as u64);
        for (k, v) in &vm.env {
            self.o.str(k);
            self.o.str(v);
        }
        self.o.iv(vm.exit_code as i64);
        self.o.uv(vm.sources.len() as u64);
        for (f, s) in &vm.sources {
            self.rcstr(f);
            self.rcstr(s);
        }
        self.o.uv(vm.symbol_registry.len() as u64);
        for (k, s) in &vm.symbol_registry {
            self.o.str(k);
            self.sym(s);
        }
        tail(&mut self.o, vm.tail);
        self.o.str(&vm.main_file);
        self.opt_obj(&vm.process);
        self.o.uv(vm.console_indent as u64);
        self.o.uv(vm.console_counts.len() as u64);
        for (k, n) in &vm.console_counts {
            self.o.str(k);
            self.o.uv(*n);
        }
        self.o.uv(vm.console_timers.len() as u64);
        for (k, t) in &vm.console_timers {
            self.o.str(k);
            self.o.f64(*t);
        }
        self.values(&vm.exit_handlers);
        self.o.uv(vm.listeners.len() as u64);
        for (k, v, once) in &vm.listeners {
            self.o.str(k);
            self.value(v);
            self.o.bool(*once);
        }
        self.o.uv(vm.stdin_listeners.len() as u64);
        for (k, v) in &vm.stdin_listeners {
            self.o.str(k);
            self.value(v);
        }
        self.o.bool(vm.stdin_flowing);
        self.o.uv(vm.readline_ifaces.len() as u64);
        for o in &vm.readline_ifaces {
            self.obj(o);
        }
        self.o.uv(vm.stack_limit as u64);
        self.o.uv(vm.rng_state);
        self.o.bool(vm.is_esm_main);
        self.opt_obj(&vm.import_meta);
        self.o.uv(vm.trace_funcs.len() as u64);
        for o in &vm.trace_funcs {
            self.opt_obj(o);
        }
        self.o.bool(vm.exit_code_set);
        self.o.uv(vm.esm_promises.len() as u64);
        for o in &vm.esm_promises {
            self.obj(o);
        }
        self.o.u8(vm.timer_frame);
        batch_opt(&mut self.o, vm.drain.between);
        self.o.bool(vm.drain.tick);
        self.o.uv(vm.out_mark as u64);
        self.o.uv(vm.open_fds.len() as u64);
        for f in &vm.open_fds {
            match f {
                None => self.o.u8(0),
                Some((p, n)) => {
                    self.o.u8(1);
                    self.o.str(p);
                    self.o.uv(*n as u64);
                }
            }
        }
        self.value(&vm.completion);
        self.o.uv(vm.handles.len() as u64);
        let mut seen: Vec<&CacheKey> = vm.cache_seen.iter().collect();
        seen.sort();
        self.o.uv(seen.len() as u64);
        for k in seen {
            self.o.b.extend_from_slice(k);
        }
        self.values(roots);
        Ok(())
    }
}

fn iter_kind(k: IterKind) -> u8 {
    match k {
        IterKind::Keys => 0,
        IterKind::Values => 1,
        IterKind::Entries => 2,
    }
}

fn iter_kind_of(b: u8) -> R<IterKind> {
    Ok(match b {
        0 => IterKind::Keys,
        1 => IterKind::Values,
        2 => IterKind::Entries,
        _ => return err("bad iterator kind"),
    })
}

const TYPED: [TypedKind; 11] = [
    TypedKind::Int8,
    TypedKind::Uint8,
    TypedKind::Uint8Clamped,
    TypedKind::Int16,
    TypedKind::Uint16,
    TypedKind::Int32,
    TypedKind::Uint32,
    TypedKind::Float32,
    TypedKind::Float64,
    TypedKind::BigInt64,
    TypedKind::BigUint64,
];

fn typed_kind(k: TypedKind) -> u8 {
    TYPED.iter().position(|x| *x == k).unwrap() as u8
}

fn batch(b: Batch) -> u8 {
    match b {
        Batch::Immediates => 0,
        Batch::List => 1,
        Batch::Lists => 2,
    }
}

fn batch_opt(o: &mut Out, b: Option<Batch>) {
    match b {
        None => o.u8(0),
        Some(b) => o.u8(1 + batch(b)),
    }
}

fn batch_of(b: u8) -> R<Option<Batch>> {
    Ok(match b {
        0 => None,
        1 => Some(Batch::Immediates),
        2 => Some(Batch::List),
        3 => Some(Batch::Lists),
        _ => return err("bad batch"),
    })
}

fn tail(o: &mut Out, t: Tail) {
    match t {
        Tail::Main => o.u8(0),
        Tail::Esm => o.u8(1),
        Tail::Timer => o.u8(2),
        Tail::Immediate => o.u8(3),
        Tail::Tick(b) => {
            o.u8(4);
            batch_opt(o, b);
        }
        Tail::Microtask(b, t) => {
            o.u8(5);
            batch_opt(o, b);
            o.bool(t);
        }
        Tail::Eval => o.u8(6),
        Tail::Check => o.u8(7),
        Tail::None => o.u8(8),
    }
}

impl<'h> Vm<'h> {
    /// Writes this VM's heap and state, and the embedder's `roots` (the JS
    /// values it holds outside the VM), to bytes. See the module
    /// documentation. Call between entry points.
    pub fn heap_snapshot(&self, roots: &[Value], opts: Options) -> Result<Vec<u8>, SnapshotError> {
        if !self.frames.is_empty() || !self.natives.is_empty() || self.native_depth != 0 {
            return err("the VM is running");
        }
        if self.debug.is_some() {
            return err("a debugger is attached");
        }
        if self.workers.is_some() || !self.inbox.is_empty() || !self.worker_errors.is_empty() {
            return err("the VM has workers");
        }
        if self.handles.iter().any(|h| h.is_some()) {
            return err("the VM holds native handles");
        }
        let mut w = W {
            o: Out::default(),
            vm: self,
            hooks: opts.hooks,
            objs: Fast::default(),
            cells: Fast::default(),
            queue: VecDeque::new(),
            strs: Fast::default(),
            str_at: Fast::default(),
            rcstrs: Fast::default(),
            syms: Fast::default(),
            bufs: Fast::default(),
            code_at: Fast::default(),
            unit_ids: Fast::default(),
        };
        w.o.b.reserve(1 << 16);
        w.vm_fields(roots)?;
        while let Some(n) = w.queue.pop_front() {
            match n {
                Node::Obj(o) => {
                    w.o.u8(REC_OBJ);
                    w.record(&o)?;
                }
                Node::Cell(c) => {
                    w.o.u8(REC_CELL);
                    let Ok(v) = c.try_borrow() else {
                        return err("a cell is borrowed");
                    };
                    w.value(&v);
                }
            }
        }
        w.o.u8(REC_END);
        let mut out = Out::default();
        out.b.reserve(w.o.b.len() + 64);
        out.b.extend_from_slice(MAGIC);
        out.uv(VERSION as u64);
        out.b
            .extend_from_slice(&(own_fingerprint() ^ opts.fingerprint).to_le_bytes());
        out.uv(w.objs.len() as u64);
        out.uv(w.cells.len() as u64);
        out.b.extend_from_slice(&w.o.b);
        Ok(out.b)
    }

    /// Records a compile for `heap_snapshot` (which refers to code by the
    /// compile that made it).
    pub(crate) fn note_unit(&mut self, kind: UnitKind, src: &str, code: &Rc<Code>) {
        let src = match self.sources.last() {
            Some((_, s)) if &**s == src => s.clone(),
            _ => Rc::from(src),
        };
        self.units.push(Unit {
            kind,
            src,
            code: Rc::downgrade(code),
        });
        if self.units.len() >= self.units_pruned_at.max(64) * 2 {
            self.units.retain(|u| u.code.strong_count() > 0);
            self.units_pruned_at = self.units.len();
        }
    }
}

// ---------------------------------------------------------------- reader

struct Rd<'a> {
    i: In<'a>,
    hooks: &'a [&'static HostHooks],
    objs: Vec<Obj>,
    cells: Vec<CellRef>,
    strs: Vec<JsStr>,
    rcstrs: Vec<Rc<str>>,
    syms: Vec<Rc<Symbol>>,
    bufs: Vec<Rc<RefCell<Vec<u8>>>>,
    units: Vec<Vec<Rc<Code>>>,
    unit_list: Vec<Unit>,
    /// Bodies this realm has been charged for.
    charged: Vec<Rc<Code>>,
    templates: Vec<(Rc<Code>, u32, Obj)>,
    seen: HashSet<CacheKey>,
    regexps: HashMap<(String, String), Rc<cw_regex::Regex>>,
    objs_filled: usize,
    cells_filled: usize,
}

impl<'a> Rd<'a> {
    fn obj(&mut self) -> R<Obj> {
        let i = self.i.us()?;
        match self.objs.get(i) {
            Some(o) => Ok(o.clone()),
            None => err("bad object reference"),
        }
    }
    fn opt_obj(&mut self) -> R<Option<Obj>> {
        Ok(match self.i.u8()? {
            0 => None,
            _ => Some(self.obj()?),
        })
    }
    fn cell(&mut self) -> R<CellRef> {
        let i = self.i.us()?;
        match self.cells.get(i) {
            Some(c) => Ok(c.clone()),
            None => err("bad cell reference"),
        }
    }
    fn jstr(&mut self) -> R<JsStr> {
        let r = self.i.us()?;
        if r == 0 {
            let s = JsStr::new(self.i.str()?);
            self.strs.push(s.clone());
            Ok(s)
        } else {
            match self.strs.get(r - 1) {
                Some(s) => Ok(s.clone()),
                None => err("bad string reference"),
            }
        }
    }
    fn rcstr(&mut self) -> R<Rc<str>> {
        let r = self.i.us()?;
        if r == 0 {
            let s: Rc<str> = Rc::from(self.i.str()?);
            self.rcstrs.push(s.clone());
            Ok(s)
        } else {
            match self.rcstrs.get(r - 1) {
                Some(s) => Ok(s.clone()),
                None => err("bad source reference"),
            }
        }
    }
    fn sym(&mut self) -> R<Rc<Symbol>> {
        let r = self.i.us()?;
        if r == 0 {
            let desc = match self.i.u8()? {
                0 => None,
                _ => Some(self.jstr()?),
            };
            let private = self.i.bool()?;
            let registered = self.i.bool()?;
            let s = Rc::new(Symbol {
                desc,
                private,
                registered,
            });
            self.syms.push(s.clone());
            Ok(s)
        } else {
            match self.syms.get(r - 1) {
                Some(s) => Ok(s.clone()),
                None => err("bad symbol reference"),
            }
        }
    }
    fn buf(&mut self) -> R<Rc<RefCell<Vec<u8>>>> {
        let r = self.i.us()?;
        if r == 0 {
            let b = Rc::new(RefCell::new(self.i.bytes()?.to_vec()));
            self.bufs.push(b.clone());
            Ok(b)
        } else {
            match self.bufs.get(r - 1) {
                Some(b) => Ok(b.clone()),
                None => err("bad buffer reference"),
            }
        }
    }
    fn native(&mut self) -> R<NativeFn> {
        let off = self.i.iv()?;
        let addr = (anchor as NativeFn as usize as i64).wrapping_add(off) as usize;
        // SAFETY: the header's fingerprint matched, so this is the program
        // image that wrote the offset, in which it is a `NativeFn`.
        Ok(unsafe { std::mem::transmute::<usize, NativeFn>(addr) })
    }
    fn big(&mut self) -> R<Rc<BigInt>> {
        let s = self.i.str()?;
        let (neg, digits) = match s.strip_prefix('-') {
            Some(d) => (true, d),
            None => (false, s),
        };
        let Some(b) = BigInt::parse_digits(digits, 16) else {
            return err("bad bigint");
        };
        Ok(Rc::new(if neg { b.neg() } else { b }))
    }
    fn value(&mut self) -> R<Value> {
        Ok(match self.i.u8()? {
            V_UNDEF => Value::Undefined,
            V_NULL => Value::Null,
            V_FALSE => Value::Bool(false),
            V_TRUE => Value::Bool(true),
            V_NUM => Value::Num(self.i.f64()?),
            V_INT => Value::Num(self.i.iv()? as f64),
            V_STR => Value::Str(self.jstr()?),
            V_BIG => Value::BigInt(self.big()?),
            V_SYM => Value::Sym(self.sym()?),
            V_OBJ => Value::Obj(self.obj()?),
            V_EMPTY => Value::Empty,
            _ => return err("bad value tag"),
        })
    }
    fn values(&mut self) -> R<Vec<Value>> {
        let n = self.i.us()?;
        let mut v = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            v.push(self.value()?);
        }
        Ok(v)
    }
    fn opt_value(&mut self) -> R<Option<Value>> {
        Ok(match self.i.u8()? {
            0 => None,
            _ => Some(self.value()?),
        })
    }
    /// A property key: its string made canonical once, in the table, so later
    /// uses of the same name find it by address.
    fn key(&mut self) -> R<Key> {
        Ok(match self.i.u8()? {
            0 => {
                let r = self.i.us()?;
                let at = if r == 0 {
                    let s = JsStr::new(self.i.str()?).canonical();
                    self.strs.push(s);
                    self.strs.len() - 1
                } else {
                    r - 1
                };
                let Some(s) = self.strs.get_mut(at) else {
                    return err("bad string reference");
                };
                if !s.is_canon() {
                    *s = s.canonical();
                }
                Key::Str(s.clone())
            }
            _ => Key::Sym(self.sym()?),
        })
    }
    fn site(&mut self) -> R<Option<Site>> {
        Ok(match self.i.u8()? {
            0 => None,
            _ => Some(Site {
                file: self.rcstr()?,
                line: self.i.u32()?,
                col: self.i.u32()?,
            }),
        })
    }
    fn strings(&mut self) -> R<Vec<String>> {
        let n = self.i.us()?;
        let mut v = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            v.push(self.i.string()?);
        }
        Ok(v)
    }

    fn code(&mut self, vm: &mut Vm) -> R<Rc<Code>> {
        let r = self.i.us()?;
        let unit = if r == 0 { self.unit_def(vm)? } else { r - 1 };
        let at = self.i.us()?;
        match self.units.get(unit).and_then(|u| u.get(at)) {
            Some(c) => Ok(c.clone()),
            None => err("bad code reference"),
        }
    }

    fn unit_def(&mut self, vm: &mut Vm) -> R<usize> {
        let kind = match self.i.u8()? {
            0 => {
                let force_module = match self.i.u8()? {
                    0 => None,
                    1 => Some(false),
                    _ => Some(true),
                };
                let params = self.i.string()?;
                let file = self.i.string()?;
                UnitKind::Program {
                    force_module,
                    params,
                    file,
                }
            }
            _ => {
                let global = self.i.bool()?;
                let file = self.i.string()?;
                UnitKind::Eval { global, file }
            }
        };
        let src = self.rcstr()?;
        let root = vm
            .restore_unit(&kind, &src, &mut self.seen)
            .map_err(SnapshotError)?;
        let mut all = Vec::new();
        preorder(&root, &mut all);
        let n = self.i.us()?;
        if n != all.len() {
            return err(format!(
                "a unit of {} compiled to {} bodies, not {n}",
                match &kind {
                    UnitKind::Program { file, .. } | UnitKind::Eval { file, .. } => file,
                },
                all.len()
            ));
        }
        let mut byte = 0u8;
        for (i, c) in all.iter().enumerate() {
            if i % 8 == 0 {
                byte = self.i.u8()?;
            }
            if byte & (1 << (i % 8)) != 0 {
                self.charged.push(c.clone());
            }
        }
        let nt = self.i.us()?;
        for _ in 0..nt {
            let at = self.i.us()?;
            let site = self.i.u32()?;
            let o = self.obj()?;
            let Some(c) = all.get(at) else {
                return err("bad template reference");
            };
            self.templates.push((c.clone(), site, o));
        }
        self.unit_list.push(Unit {
            kind,
            src,
            code: Rc::downgrade(&root),
        });
        self.units.push(all);
        Ok(self.units.len() - 1)
    }

    fn frame(&mut self, vm: &mut Vm) -> R<Frame> {
        let code = self.code(vm)?;
        let pc = self.i.us()?;
        let stack = self.values()?;
        let nl = self.i.us()?;
        let mut locals = Vec::with_capacity(nl);
        for _ in 0..nl {
            locals.push(match self.i.u8()? {
                0 => Local::V(self.value()?),
                _ => Local::C(self.cell()?),
            });
        }
        let nc = self.i.us()?;
        let mut caps = Vec::with_capacity(nc);
        for _ in 0..nc {
            caps.push(self.cell()?);
        }
        let nh = self.i.us()?;
        let mut handlers = Vec::with_capacity(nh);
        for _ in 0..nh {
            handlers.push(Handler {
                pc: self.i.u32()?,
                depth: self.i.u32()?,
                finally: self.i.bool()?,
            });
        }
        let args = self.values()?;
        let func = self.opt_obj()?;
        let recv = self.value()?;
        let kind = match self.i.u8()? {
            0 => FrameKind::Normal,
            1 => FrameKind::Construct(self.value()?),
            2 => FrameKind::Generator(self.obj()?),
            _ => FrameKind::Async {
                promise: self.obj()?,
                co: self.obj()?,
                first: self.i.bool()?,
            },
        };
        let resume = match self.i.u8()? {
            0 => None,
            1 => Some(Resume::Throw(self.value()?)),
            _ => Some(Resume::Return(self.value()?)),
        };
        Ok(Frame {
            code,
            pc,
            stack,
            locals,
            captures: Rc::from(caps),
            handlers,
            args,
            func,
            recv,
            kind,
            resume,
            ystar_mode: self.i.u8()?,
            resumed: self.i.bool()?,
            timer: self.i.u8()?,
        })
    }

    fn opt_frame(&mut self, vm: &mut Vm) -> R<Option<Box<Frame>>> {
        Ok(match self.i.u8()? {
            0 => None,
            _ => Some(Box::new(self.frame(vm)?)),
        })
    }

    fn reaction(&mut self) -> R<Reaction> {
        Ok(match self.i.u8()? {
            0 => {
                let handler = self.opt_value()?;
                let derived = self.opt_obj()?;
                let cap = match self.i.u8()? {
                    0 => None,
                    _ => Some((self.value()?, self.value()?)),
                };
                Reaction::Then {
                    handler,
                    derived,
                    cap,
                }
            }
            1 => Reaction::Resume(self.obj()?),
            _ => Reaction::Native(self.native()?, self.values()?),
        })
    }

    fn map(&mut self) -> R<Box<MapData>> {
        let n = self.i.us()?;
        let mut m = MapData::default();
        m.entries.reserve(n);
        for i in 0..n {
            match self.i.u8()? {
                0 => m.entries.push(None),
                _ => {
                    let k = self.value()?;
                    let v = self.value()?;
                    m.index.insert(hkey(&k), i);
                    m.entries.push(Some((k, v)));
                    m.live += 1;
                }
            }
        }
        Ok(Box::new(m))
    }

    fn regexp(&mut self, source: &JsStr, flags: &JsStr) -> R<Rc<cw_regex::Regex>> {
        let k = (source.as_str().to_owned(), flags.as_str().to_owned());
        if let Some(r) = self.regexps.get(&k) {
            return Ok(r.clone());
        }
        let re = crate::regexp::compile_for_restore(source, flags).map_err(SnapshotError)?;
        let re = Rc::new(re);
        self.regexps.insert(k, re.clone());
        Ok(re)
    }

    fn record(&mut self, vm: &mut Vm, o: &Obj) -> R<()> {
        let proto = self.opt_obj()?;
        let flags = self.i.u8()?;
        let tag = match self.i.u8()? {
            0 => None,
            t => match TAGS.get(t as usize - 1) {
                Some(t) => Some(*t),
                None => return err("bad object tag"),
            },
        };
        let np = self.i.us()?;
        let mut entries = Vec::with_capacity(np.min(1 << 16));
        for _ in 0..np {
            let k = self.key()?;
            let f = self.i.u8()?;
            let slot = match self.i.u8()? {
                0 => Slot::Data(self.value()?),
                _ => Slot::Accessor(self.opt_obj()?, self.opt_obj()?),
            };
            entries.push((k, Prop { slot, flags: f }));
        }
        let props = PropMap::from_entries(entries);
        let kind = match self.i.u8()? {
            0 => Kind::Ordinary,
            1 => Kind::Array(self.values()?),
            2 => {
                let imp = match self.i.u8()? {
                    0 => {
                        let code = self.code(vm)?;
                        let n = self.i.us()?;
                        let mut caps = Vec::with_capacity(n);
                        for _ in 0..n {
                            caps.push(self.cell()?);
                        }
                        FuncImpl::Closure {
                            code,
                            captures: Rc::from(caps),
                        }
                    }
                    1 => FuncImpl::Native {
                        f: self.native()?,
                        slots: self.values()?,
                    },
                    _ => FuncImpl::Bound {
                        target: self.obj()?,
                        this: self.value()?,
                        args: self.values()?,
                    },
                };
                let ctor = match self.i.u8()? {
                    0 => CtorKind::None,
                    1 => CtorKind::Base,
                    _ => CtorKind::Derived,
                };
                Kind::Function(Box::new(FuncData {
                    imp,
                    ctor,
                    class_ctor: self.i.bool()?,
                    home: self.opt_obj()?,
                    fields: self.opt_obj()?,
                }))
            }
            3 => {
                let n = self.i.us()?;
                let mut frames = Vec::with_capacity(n);
                for _ in 0..n {
                    frames.push(self.i.string()?);
                }
                let site = self.site()?;
                let arrow = match self.i.u8()? {
                    0 => None,
                    _ => Some(self.i.string()?),
                };
                Kind::Error(Box::new(ErrorData {
                    frames,
                    site,
                    arrow,
                    from_async: self.i.bool()?,
                }))
            }
            4 => Kind::Boolean(self.i.bool()?),
            5 => Kind::Number(self.i.f64()?),
            6 => Kind::String(self.jstr()?),
            7 => Kind::Symbol(self.sym()?),
            8 => Kind::BigInt(self.big()?),
            9 => Kind::Date(self.i.f64()?),
            10 => {
                let source = self.jstr()?;
                let flags = self.jstr()?;
                let b = self.i.u8()?;
                let re = self.regexp(&source, &flags)?;
                Kind::RegExp(Box::new(RegExpData {
                    source,
                    flags,
                    re,
                    global: b & 1 != 0,
                    sticky: b & 2 != 0,
                    unicode: b & 4 != 0,
                    has_indices: b & 8 != 0,
                }))
            }
            11 => Kind::Map(self.map()?),
            12 => Kind::Set(self.map()?),
            13 => Kind::WeakMap(self.map()?),
            14 => Kind::WeakSet(self.map()?),
            15 => Kind::WeakRef(self.value()?),
            16 => {
                let state = match self.i.u8()? {
                    0 => PromiseState::Pending,
                    1 => PromiseState::Fulfilled,
                    _ => PromiseState::Rejected,
                };
                let value = self.value()?;
                let n = self.i.us()?;
                let mut fulfill = Vec::with_capacity(n);
                for _ in 0..n {
                    fulfill.push(self.reaction()?);
                }
                let n = self.i.us()?;
                let mut reject = Vec::with_capacity(n);
                for _ in 0..n {
                    reject.push(self.reaction()?);
                }
                Kind::Promise(Box::new(PromiseData {
                    state,
                    value,
                    fulfill,
                    reject,
                    handled: self.i.bool()?,
                    resolving: self.i.bool()?,
                }))
            }
            17 => {
                let state = match self.i.u8()? {
                    0 => GenState::SuspendedStart,
                    1 => GenState::SuspendedYield,
                    2 => GenState::Running,
                    _ => GenState::Completed,
                };
                let frame = self.opt_frame(vm)?;
                let n = self.i.us()?;
                let mut queue = VecDeque::with_capacity(n);
                for _ in 0..n {
                    queue.push_back((self.i.u8()?, self.value()?, self.obj()?));
                }
                Kind::Generator(Box::new(GenData {
                    state,
                    frame,
                    queue,
                    is_async: self.i.bool()?,
                }))
            }
            18 => Kind::Coroutine(self.opt_frame(vm)?),
            19 => Kind::ArrayIter {
                target: self.value()?,
                index: self.i.us()?,
                kind: iter_kind_of(self.i.u8()?)?,
                done: self.i.bool()?,
            },
            20 => Kind::MapIter {
                target: self.obj()?,
                index: self.i.us()?,
                kind: iter_kind_of(self.i.u8()?)?,
                done: self.i.bool()?,
            },
            21 => Kind::StringIter {
                s: self.jstr()?,
                pos: self.i.us()?,
                done: self.i.bool()?,
            },
            22 => Kind::RegExpStringIter {
                re: self.obj()?,
                s: self.jstr()?,
                global: self.i.bool()?,
                unicode: self.i.bool()?,
                done: self.i.bool()?,
            },
            23 => {
                let n = self.i.us()?;
                let mut keys = Vec::with_capacity(n);
                for _ in 0..n {
                    keys.push(self.jstr()?);
                }
                Kind::ForIn {
                    keys,
                    index: self.i.us()?,
                    obj: self.obj()?,
                }
            }
            24 => Kind::Arguments,
            25 => Kind::ArrayBuffer(self.buf()?),
            26 => {
                let k = self.i.u8()? as usize;
                let Some(kind) = TYPED.get(k).copied() else {
                    return err("bad typed array kind");
                };
                Kind::TypedArray {
                    kind,
                    buf: self.buf()?,
                    offset: self.i.us()?,
                    len: self.i.us()?,
                    buf_obj: self.opt_obj()?,
                }
            }
            27 => Kind::Proxy {
                target: self.obj()?,
                handler: self.obj()?,
            },
            28 => Kind::Internal(self.values()?),
            29 => {
                let i = self.i.us()?;
                let Some(hooks) = self.hooks.get(i).copied() else {
                    return err("bad host hooks reference");
                };
                Kind::Host(Box::new(HostData {
                    hooks,
                    data: self.values()?,
                }))
            }
            _ => return err("bad object kind"),
        };
        // Filled in place: an object's data never leaves it (see `gc`).
        let mut d = o.borrow_mut();
        d.proto = proto;
        d.props = props;
        d.kind = kind;
        d.extensible = flags & 1 != 0;
        d.elems_frozen = flags & 2 != 0;
        d.elems_sealed = flags & 4 != 0;
        d.tag = tag;
        Ok(())
    }

    fn job(&mut self) -> R<Job> {
        Ok(match self.i.u8()? {
            0 => Job::Reaction {
                reaction: self.reaction()?,
                arg: self.value()?,
                rejected: self.i.bool()?,
            },
            1 => Job::Thenable {
                promise: self.obj()?,
                thenable: self.value()?,
                then: self.value()?,
            },
            _ => {
                let f = self.value()?;
                Job::Callback(f, self.values()?)
            }
        })
    }

    fn tail(&mut self) -> R<Tail> {
        Ok(match self.i.u8()? {
            0 => Tail::Main,
            1 => Tail::Esm,
            2 => Tail::Timer,
            3 => Tail::Immediate,
            4 => Tail::Tick(batch_of(self.i.u8()?)?),
            5 => Tail::Microtask(batch_of(self.i.u8()?)?, self.i.bool()?),
            6 => Tail::Eval,
            7 => Tail::Check,
            _ => Tail::None,
        })
    }
}

type VmParts<'h> = (
    Vm<'h>,
    Vec<Value>,
    Vec<(Rc<str>, Rc<str>)>,
    HashSet<CacheKey>,
);

/// The VM's own fields, as `W::vm_fields` wrote them.
fn read_vm<'h>(r: &mut Rd, host: &'h mut dyn ScriptHost) -> R<VmParts<'h>> {
    let objs = |r: &mut Rd| -> R<Vec<Obj>> {
        let n = r.i.us()?;
        let mut v = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            v.push(r.obj()?);
        }
        Ok(v)
    };
    let object_proto = r.obj()?;
    let function_proto = r.obj()?;
    let array_proto = r.obj()?;
    let string_proto = r.obj()?;
    let number_proto = r.obj()?;
    let boolean_proto = r.obj()?;
    let symbol_proto = r.obj()?;
    let bigint_proto = r.obj()?;
    let error_protos = objs(r)?;
    let error_ctors = objs(r)?;
    let iterator_proto = r.obj()?;
    let async_iterator_proto = r.obj()?;
    let array_iter_proto = r.obj()?;
    let map_iter_proto = r.obj()?;
    let set_iter_proto = r.obj()?;
    let string_iter_proto = r.obj()?;
    let regexp_str_iter_proto = r.obj()?;
    let generator_proto = r.obj()?;
    let async_generator_proto = r.obj()?;
    let generator_function_proto = r.obj()?;
    let async_generator_function_proto = r.obj()?;
    let async_function_proto = r.obj()?;
    let promise_proto = r.obj()?;
    let promise_ctor = r.obj()?;
    let regexp_proto = r.obj()?;
    let date_proto = r.obj()?;
    let map_proto = r.obj()?;
    let set_proto = r.obj()?;
    let weakmap_proto = r.obj()?;
    let weakset_proto = r.obj()?;
    let weakref_proto = r.obj()?;
    let arraybuffer_proto = r.obj()?;
    let typed_protos = objs(r)?;
    let intr = Intrinsics {
        object_proto,
        function_proto,
        array_proto,
        string_proto,
        number_proto,
        boolean_proto,
        symbol_proto,
        bigint_proto,
        error_protos,
        error_ctors,
        iterator_proto,
        async_iterator_proto,
        array_iter_proto,
        map_iter_proto,
        set_iter_proto,
        string_iter_proto,
        regexp_str_iter_proto,
        generator_proto,
        async_generator_proto,
        generator_function_proto,
        async_generator_function_proto,
        async_function_proto,
        promise_proto,
        promise_ctor,
        regexp_proto,
        date_proto,
        map_proto,
        set_proto,
        weakmap_proto,
        weakset_proto,
        weakref_proto,
        arraybuffer_proto,
        typed_protos,
        array_iter_next: r.obj()?,
        array_values: r.obj()?,
        object_ctor: r.obj()?,
        array_ctor: r.obj()?,
        function_ctor: r.obj()?,
        buffer_proto: r.obj()?,
    };
    let syms = Syms {
        iterator: r.sym()?,
        async_iterator: r.sym()?,
        has_instance: r.sym()?,
        to_primitive: r.sym()?,
        to_string_tag: r.sym()?,
        species: r.sym()?,
        is_concat_spreadable: r.sym()?,
        unscopables: r.sym()?,
        match_: r.sym()?,
        match_all: r.sym()?,
        replace: r.sym()?,
        search: r.sym()?,
        split: r.sym()?,
        inspect_custom: r.sym()?,
    };
    let global = r.obj()?;
    let stdout = r.i.string()?;
    let stderr = r.i.string()?;
    let stdin = match r.i.u8()? {
        0 => None,
        _ => Some(r.i.string()?),
    };
    let stdin_consumed = r.i.bool()?;
    let stdin_pos = r.i.us()?;
    let interactive = r.i.bool()?;
    let stdin_eof = r.i.bool()?;
    let awaiting_input = r.i.bool()?;
    let steps = r.i.uv()?;
    let budget = r.i.uv()?;
    let throw_site = r.site()?;
    let exit = match r.i.u8()? {
        0 => Exit::Return,
        1 => Exit::Yield,
        _ => Exit::Await,
    };
    let n = r.i.us()?;
    let mut microtasks = VecDeque::with_capacity(n);
    for _ in 0..n {
        microtasks.push_back(r.job()?);
    }
    let n = r.i.us()?;
    let mut ticks = VecDeque::with_capacity(n);
    for _ in 0..n {
        let f = r.value()?;
        ticks.push_back((f, r.values()?));
    }
    let n = r.i.us()?;
    let mut timers = Vec::with_capacity(n);
    for _ in 0..n {
        timers.push(Timer {
            id: r.i.uv()?,
            when: r.i.f64()?,
            seq: r.i.uv()?,
            callback: r.value()?,
            args: r.values()?,
            interval: match r.i.u8()? {
                0 => None,
                _ => Some(r.i.f64()?),
            },
            obj: r.obj()?,
            immediate: r.i.bool()?,
            io: r.i.bool()?,
            dur: r.i.f64()?,
        });
    }
    let timer_seq = r.i.uv()?;
    let timer_id = r.i.uv()?;
    let elapsed_ms = r.i.f64()?;
    let loading_parent = r.opt_value()?;
    let clock_steps = r.i.uv()?;
    let start_micros = r.i.iv()?;
    let pending_rejections = objs(r)?;
    let n = r.i.us()?;
    let mut modules = Vec::with_capacity(n);
    for _ in 0..n {
        let k = r.i.string()?;
        modules.push((k, r.value()?));
    }
    let argv = r.strings()?;
    let n = r.i.us()?;
    let mut env = Vec::with_capacity(n);
    for _ in 0..n {
        env.push((r.i.string()?, r.i.string()?));
    }
    let exit_code = r.i.iv()? as i32;
    let n = r.i.us()?;
    let mut sources = Vec::with_capacity(n);
    for _ in 0..n {
        sources.push((r.rcstr()?, r.rcstr()?));
    }
    let n = r.i.us()?;
    let mut symbol_registry = Vec::with_capacity(n);
    for _ in 0..n {
        let k = r.i.string()?;
        symbol_registry.push((k, r.sym()?));
    }
    let tail = r.tail()?;
    let main_file = r.i.string()?;
    let process = r.opt_obj()?;
    let console_indent = r.i.us()?;
    let n = r.i.us()?;
    let mut console_counts = Vec::with_capacity(n);
    for _ in 0..n {
        console_counts.push((r.i.string()?, r.i.uv()?));
    }
    let n = r.i.us()?;
    let mut console_timers = Vec::with_capacity(n);
    for _ in 0..n {
        console_timers.push((r.i.string()?, r.i.f64()?));
    }
    let exit_handlers = r.values()?;
    let n = r.i.us()?;
    let mut listeners = Vec::with_capacity(n);
    for _ in 0..n {
        listeners.push((r.i.string()?, r.value()?, r.i.bool()?));
    }
    let n = r.i.us()?;
    let mut stdin_listeners = Vec::with_capacity(n);
    for _ in 0..n {
        stdin_listeners.push((r.i.string()?, r.value()?));
    }
    let stdin_flowing = r.i.bool()?;
    let readline_ifaces = objs(r)?;
    let stack_limit = r.i.us()?;
    let rng_state = r.i.uv()?;
    let is_esm_main = r.i.bool()?;
    let import_meta = r.opt_obj()?;
    let n = r.i.us()?;
    let mut trace_funcs = Vec::with_capacity(n);
    for _ in 0..n {
        trace_funcs.push(r.opt_obj()?);
    }
    let exit_code_set = r.i.bool()?;
    let esm_promises = objs(r)?;
    let timer_frame = r.i.u8()?;
    let drain = Drain {
        between: batch_of(r.i.u8()?)?,
        tick: r.i.bool()?,
    };
    let out_mark = r.i.us()?;
    let n = r.i.us()?;
    let mut open_fds = Vec::with_capacity(n);
    for _ in 0..n {
        open_fds.push(match r.i.u8()? {
            0 => None,
            _ => Some((r.i.string()?, r.i.us()?)),
        });
    }
    let completion = r.value()?;
    let nhandles = r.i.us()?;
    let n = r.i.us()?;
    let mut seen = HashSet::with_capacity(n);
    for _ in 0..n {
        seen.insert(<CacheKey>::try_from(r.i.raw(32)?).unwrap());
    }
    let roots = r.values()?;
    let vm = Vm {
        host,
        frames: vec![],
        natives: vec![],
        intr,
        syms,
        global,
        stdout,
        stderr,
        stdin,
        stdin_consumed,
        stdin_pos,
        interactive,
        stdin_eof,
        awaiting_input,
        debug: None,
        workers: None,
        inbox: VecDeque::new(),
        worker_errors: vec![],
        steps,
        budget,
        native_depth: 0,
        throw_site,
        exit,
        microtasks,
        ticks,
        timers,
        timer_seq,
        timer_id,
        elapsed_ms,
        loading_parent,
        clock_steps,
        start_micros,
        pending_rejections,
        modules,
        argv,
        env,
        exit_code,
        sources: sources.clone(),
        symbol_registry,
        tail,
        main_file,
        inspect_seen: vec![],
        process,
        console_indent,
        console_counts,
        console_timers,
        exit_handlers,
        listeners,
        stdin_listeners,
        stdin_flowing,
        readline_ifaces,
        stack_limit,
        rng_state,
        is_esm_main,
        import_meta,
        trace_funcs,
        exit_code_set,
        esm_promises,
        timer_frame,
        drain,
        out_mark,
        open_fds,
        completion,
        handles: (0..nhandles).map(|_| None).collect(),
        embedder: None,
        id: crate::codecache::next_id(),
        compiled: Default::default(),
        cache_seen: Default::default(),
        templates: Default::default(),
        units: Vec::new(),
        units_pruned_at: 0,
        pool: Default::default(),
        prof: None,
        reclaim: Some(crate::gc::Reclaim),
    };
    Ok((vm, roots, sources, seen))
}

/// Placeholder objects for the VM's fields until the records fill them.
fn shells(n: usize) -> Vec<Obj> {
    (0..n)
        .map(|_| Obj::new(ObjData::new(None, Kind::Ordinary)))
        .collect()
}

impl<'h> Vm<'h> {
    /// Reads a VM written by `heap_snapshot` (by this program image, with the
    /// same `opts`), running on `host`; returns it with the embedder's roots.
    pub fn from_heap_snapshot(
        host: &'h mut dyn ScriptHost,
        bytes: &[u8],
        opts: Options,
    ) -> Result<(Vm<'h>, Vec<Value>), SnapshotError> {
        let mut i = In { b: bytes, p: 0 };
        if i.raw(8)? != MAGIC {
            return err("not a heap snapshot");
        }
        if i.u32()? != VERSION {
            return err("heap snapshot version differs");
        }
        let fp = u64::from_le_bytes(i.raw(8)?.try_into().unwrap());
        if fp != own_fingerprint() ^ opts.fingerprint {
            return err("heap snapshot written by another program image");
        }
        let nobj = i.us()?;
        let ncell = i.us()?;
        if nobj > bytes.len() || ncell > bytes.len() {
            return err("bad heap snapshot counts");
        }
        let mut r = Rd {
            i,
            hooks: opts.hooks,
            objs: shells(nobj),
            cells: (0..ncell)
                .map(|_| Rc::new(RefCell::new(Value::Undefined)))
                .collect(),
            strs: Vec::new(),
            rcstrs: Vec::new(),
            syms: Vec::new(),
            bufs: Vec::new(),
            units: Vec::new(),
            unit_list: Vec::new(),
            charged: Vec::new(),
            templates: Vec::new(),
            seen: HashSet::new(),
            regexps: HashMap::new(),
            objs_filled: 0,
            cells_filled: 0,
        };
        // The VM's fields come first; units compile into it as the records
        // reach them, which registers their sources again, so the list read
        // from the snapshot is put back afterwards.
        let (mut vm, roots, sources, seen) = read_vm(&mut r, host)?;
        loop {
            match r.i.u8()? {
                REC_OBJ => {
                    let at = r.objs_filled;
                    let o = match r.objs.get(at) {
                        Some(o) => o.clone(),
                        None => return err("more object records than objects"),
                    };
                    r.objs_filled += 1;
                    r.record(&mut vm, &o)?;
                }
                REC_CELL => {
                    let at = r.cells_filled;
                    let c = match r.cells.get(at) {
                        Some(c) => c.clone(),
                        None => return err("more cell records than cells"),
                    };
                    r.cells_filled += 1;
                    let v = r.value()?;
                    *c.borrow_mut() = v;
                }
                REC_END => break,
                _ => return err("bad record tag"),
            }
        }
        if r.objs_filled != r.objs.len() || r.cells_filled != r.cells.len() {
            return err("records missing");
        }
        for c in std::mem::take(&mut r.charged) {
            if c.compiled_by.get() == 0 {
                c.compiled_by.set(vm.id);
            } else {
                vm.compiled.insert(c.uid);
            }
        }
        for (c, site, o) in std::mem::take(&mut r.templates) {
            vm.templates.insert((c.uid, site), o);
        }
        vm.units = std::mem::take(&mut r.unit_list);
        vm.sources = sources;
        vm.cache_seen = seen;
        Ok((vm, roots))
    }
}
