//! A cycle collector for the VM's reference-counted heap.
//!
//! Objects and closure cells are `Rc`s: what nothing references is freed at
//! once, but garbage that references itself in a cycle (a function and its
//! `prototype` object, React's circular update queues, a closure stored in a
//! variable it captures, a whole realm once its embedder drops it) never
//! reaches a count of zero and was never freed, so a long-lived page grew
//! without bound.
//!
//! Every object has an entry in a table here (a raw pointer, cleared when the
//! object's data drops). A collection counts, for each object, the references
//! it receives from other objects, directly or through closure cells; an
//! object or cell with more references than that is held from outside the
//! heap (a VM stack, a global, the embedder, a Rust local), so it is a root.
//! Everything reachable from the roots is live; the rest is garbage held only
//! by other garbage, and its contents are dropped, which breaks the cycles and
//! frees it. Nothing needs to enumerate the roots, and an edge the traversal
//! does not know about only makes its target look externally held (kept,
//! never wrongly freed).
//!
//! Cells are not registered: a collection finds them through the capture
//! lists of the functions that hold them (a list held only by its function is
//! part of that function; one shared with a frame is treated as external,
//! which keeps what it reaches).
//!
//! Collection frees only unreachable objects and runs no JavaScript (there are
//! no finalizers, and `WeakRef` holds its target strongly), so when it runs is
//! unobservable: it happens at safe points (between event-loop tasks, after an
//! embedder's entry point) once enough has been allocated since the last one.

use crate::value::*;
use crate::vm::{Frame, FrameKind, Local, Resume};
use std::cell::RefCell;
use std::rc::Rc;

/// `ObjData::gc_slot` of data not (or no longer) in the table.
pub(crate) const UNREGISTERED: u32 = u32::MAX;

struct Entry {
    cell: *const RefCell<ObjData>,
    /// The data's address, to recognise the entry's own data when it drops.
    data: *const ObjData,
}

#[derive(Default)]
struct Registry {
    /// Every registered object; null where one was dropped.
    table: Vec<Entry>,
    free: Vec<u32>,
    /// Live objects after the last collection.
    survivors: i64,
    /// Collections run, and nodes they freed (for tests and reports).
    runs: u64,
    freed: u64,
}

thread_local! {
    static REG: RefCell<Registry> = RefCell::new(Registry::default());
}

/// A collection is due once the live object count has grown by this much
/// since the last one, or by as many as survived it if that is more: garbage
/// that reference counting frees never triggers one, and the work stays
/// proportional to what accumulates.
const MIN_GROWTH: i64 = 20_000;

pub(crate) fn register(o: &Obj) {
    let e = Entry {
        cell: Rc::as_ptr(&o.0),
        data: o.0.as_ptr() as *const ObjData,
    };
    let slot = REG.with(|r| {
        let mut r = r.borrow_mut();
        match r.free.pop() {
            Some(s) => {
                r.table[s as usize] = e;
                s
            }
            None => {
                r.table.push(e);
                (r.table.len() - 1) as u32
            }
        }
    });
    o.0.borrow_mut().gc_slot = slot;
}

/// Called as a registered object's data drops.
pub(crate) fn unregister(slot: u32, data: *const ObjData) {
    let _ = REG.try_with(|r| {
        let mut r = r.borrow_mut();
        let e = &mut r.table[slot as usize];
        debug_assert!(e.data == data, "an object's data moved out of it");
        if e.data == data {
            e.cell = std::ptr::null();
            e.data = std::ptr::null();
            r.free.push(slot);
        }
    });
}

/// Runs a collection when dropped: a VM holds one as its last field, so that
/// what it made is freed with it (see `Vm::reclaim`).
pub struct Reclaim;

impl Drop for Reclaim {
    fn drop(&mut self) {
        // Not while the thread itself is being torn down.
        if REG.try_with(|_| ()).is_ok() {
            collect();
        }
    }
}

/// What a collection did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Nodes (objects, and the cells found through them) alive before.
    pub live: usize,
    /// Nodes found unreachable and freed.
    pub freed: usize,
}

/// Collects when enough has been allocated since the last collection. Call at
/// a safe point: no object borrowed, no raw pointer into the heap in use.
pub fn maybe_collect() -> Option<Stats> {
    let due = REG.with(|r| {
        let r = r.borrow();
        live_objects() - r.survivors >= MIN_GROWTH.max(r.survivors)
    });
    if due {
        Some(collect())
    } else {
        None
    }
}

/// (collections run, nodes freed) on this thread so far.
pub fn totals() -> (u64, u64) {
    REG.with(|r| {
        let r = r.borrow();
        (r.runs, r.freed)
    })
}

/// A strong reference one node holds to another.
enum Edge<'a> {
    Obj(&'a Obj),
    Cell(&'a CellRef),
    Captures(&'a Rc<[CellRef]>),
}

/// One collection's view of the heap: objects by table slot, then cells in the
/// order they were found.
struct Heap {
    objs: Vec<Option<Obj>>,
    cells: Vec<CellRef>,
    cell_ix: FastMap<usize, u32>,
    /// Set when a node is borrowed: not a safe point.
    busy: bool,
}

impl Heap {
    /// The node an object edge points at, if it is in the table.
    fn obj_node(&mut self, o: &Obj) -> Option<usize> {
        // SAFETY: only the slot is read, and a collection runs no code that
        // could mutate an object; one mutably borrowed means the caller is not
        // at a safe point, and the collection is abandoned.
        match unsafe { o.0.try_borrow_unguarded() } {
            Ok(d) => {
                let s = d.gc_slot as usize;
                match self.objs.get(s) {
                    Some(Some(h)) if Rc::ptr_eq(&h.0, &o.0) => Some(s),
                    _ => None,
                }
            }
            Err(_) => {
                self.busy = true;
                None
            }
        }
    }

    /// The node for a cell, added on first sight.
    fn cell_node(&mut self, c: &CellRef) -> usize {
        let next = self.cells.len() as u32;
        let ix = *self
            .cell_ix
            .entry(Rc::as_ptr(c) as *const u8 as usize)
            .or_insert(next);
        if ix == next {
            self.cells.push(c.clone());
        }
        self.objs.len() + ix as usize
    }

    /// Calls `f` with every node `e` leads to.
    fn targets(&mut self, e: Edge, f: &mut impl FnMut(usize)) {
        match e {
            Edge::Obj(o) => {
                if let Some(n) = self.obj_node(o) {
                    f(n);
                }
            }
            Edge::Cell(c) => f(self.cell_node(c)),
            Edge::Captures(s) => {
                // Part of its one holder; a list shared with a frame is opaque.
                if Rc::strong_count(s) == 1 {
                    for c in s.iter() {
                        f(self.cell_node(c));
                    }
                }
            }
        }
    }

    /// Calls `f` with every node that node `n` references.
    fn edges(&mut self, n: usize, f: &mut impl FnMut(usize)) {
        if n < self.objs.len() {
            let Some(o) = self.objs[n].clone() else {
                return;
            };
            let Ok(d) = o.0.try_borrow() else {
                self.busy = true;
                return;
            };
            obj_edges(&d, &mut |e| self.targets(e, f));
        } else {
            let c = self.cells[n - self.objs.len()].clone();
            let Ok(v) = c.try_borrow() else {
                self.busy = true;
                return;
            };
            if let Value::Obj(o) = &*v {
                self.targets(Edge::Obj(o), f);
            }
        }
    }

    fn strong(&self, n: usize) -> usize {
        if n < self.objs.len() {
            self.objs[n].as_ref().map_or(0, |o| Rc::strong_count(&o.0))
        } else {
            Rc::strong_count(&self.cells[n - self.objs.len()])
        }
    }
}

/// Runs a collection now (see the module documentation).
pub fn collect() -> Stats {
    // One handle to every registered object (the handle is one of its strong
    // references).
    let objs: Vec<Option<Obj>> = REG.with(|r| {
        r.borrow()
            .table
            .iter()
            .map(|e| {
                (!e.cell.is_null()).then(|| {
                    // SAFETY: an entry is cleared as its object's data drops,
                    // so a non-null entry is a live `Rc` made by `Obj::new`.
                    unsafe {
                        Rc::increment_strong_count(e.cell);
                        Obj(Rc::from_raw(e.cell))
                    }
                })
            })
            .collect()
    });
    let mut heap = Heap {
        objs,
        cells: Vec::new(),
        cell_ix: FastMap::default(),
        busy: false,
    };

    // Internal reference counts: references from other nodes. Cells are
    // counted as they are found (the node list grows as it is walked).
    let mut internal: Vec<u32> = vec![0; heap.objs.len()];
    let mut n = 0;
    while n < heap.objs.len() + heap.cells.len() && !heap.busy {
        heap.edges(n, &mut |t| {
            if t >= internal.len() {
                internal.resize(t + 1, 0);
            }
            internal[t] += 1;
        });
        n += 1;
    }
    let total = heap.objs.len() + heap.cells.len();
    internal.resize(total, 0);
    let live = heap.objs.iter().flatten().count() + heap.cells.len();
    if heap.busy {
        REG.with(|r| r.borrow_mut().survivors = live_objects());
        return Stats { live, freed: 0 };
    }

    // Roots: nodes with references from outside (besides this collection's
    // own handle). Everything reachable from a root is live.
    let mut marked = vec![false; total];
    let mut stack: Vec<usize> = Vec::new();
    for (n, m) in marked.iter_mut().enumerate() {
        if heap.strong(n) > 1 + internal[n] as usize {
            *m = true;
            stack.push(n);
        }
    }
    while let Some(n) = stack.pop() {
        heap.edges(n, &mut |t| {
            if !marked[t] {
                marked[t] = true;
                stack.push(t);
            }
        });
    }

    // The rest is garbage held only by garbage: empty it (in place, so the
    // table's record of each object stays current), which drops the
    // references that keep the cycles alive.
    let nobj = heap.objs.len();
    let mut dropped_objs: Vec<ObjData> = Vec::new();
    let mut dropped_values: Vec<Value> = Vec::new();
    for (n, o) in heap.objs.iter().enumerate() {
        if let (Some(o), false) = (o, marked[n]) {
            let mut d = o.0.borrow_mut();
            let mut empty = ObjData::new(None, Kind::Ordinary);
            empty.gc_slot = d.gc_slot;
            let mut old = std::mem::replace(&mut *d, empty);
            old.gc_slot = UNREGISTERED;
            dropped_objs.push(old);
        }
    }
    for (i, c) in heap.cells.iter().enumerate() {
        if !marked[nobj + i] {
            dropped_values.push(std::mem::replace(&mut *c.borrow_mut(), Value::Undefined));
        }
    }
    let freed = dropped_objs.len() + dropped_values.len();
    drop(dropped_objs);
    drop(dropped_values);
    drop(heap);
    REG.with(|r| {
        let mut r = r.borrow_mut();
        r.survivors = live_objects();
        r.runs += 1;
        r.freed += freed as u64;
    });
    Stats { live, freed }
}

// ---------------------------------------------------------------- edges
//
// Each function reports every strong reference to an object, cell or capture
// list that the given node holds: exactly once each, since a reference
// reported that the node does not hold would make its target look less
// referenced from outside than it is.

fn value_edges(v: &Value, f: &mut impl FnMut(Edge)) {
    if let Value::Obj(o) = v {
        f(Edge::Obj(o));
    }
}

fn values_edges<'a>(vs: impl IntoIterator<Item = &'a Value>, f: &mut impl FnMut(Edge)) {
    for v in vs {
        value_edges(v, f);
    }
}

fn obj_edges(d: &ObjData, f: &mut impl FnMut(Edge)) {
    if let Some(p) = &d.proto {
        f(Edge::Obj(p));
    }
    for (_, p) in &d.props.entries {
        match &p.slot {
            Slot::Data(v) => value_edges(v, f),
            Slot::Accessor(g, s) => {
                if let Some(g) = g {
                    f(Edge::Obj(g));
                }
                if let Some(s) = s {
                    f(Edge::Obj(s));
                }
            }
        }
    }
    match &d.kind {
        Kind::Ordinary
        | Kind::Error(_)
        | Kind::Boolean(_)
        | Kind::Number(_)
        | Kind::String(_)
        | Kind::Symbol(_)
        | Kind::BigInt(_)
        | Kind::Date(_)
        | Kind::RegExp(_)
        | Kind::StringIter { .. }
        | Kind::Arguments
        | Kind::ArrayBuffer(_) => {}
        Kind::Array(vs) | Kind::Internal(vs) => values_edges(vs, f),
        Kind::Function(fd) => {
            match &fd.imp {
                FuncImpl::Closure { captures, .. } => {
                    if !captures.is_empty() {
                        f(Edge::Captures(captures));
                    }
                }
                FuncImpl::Native { slots, .. } => values_edges(slots, f),
                FuncImpl::Bound { target, this, args } => {
                    f(Edge::Obj(target));
                    value_edges(this, f);
                    values_edges(args, f);
                }
            }
            if let Some(h) = &fd.home {
                f(Edge::Obj(h));
            }
            if let Some(x) = &fd.fields {
                f(Edge::Obj(x));
            }
        }
        Kind::Map(m) | Kind::Set(m) | Kind::WeakMap(m) | Kind::WeakSet(m) => {
            for (k, v) in m.entries.iter().flatten() {
                value_edges(k, f);
                value_edges(v, f);
            }
        }
        Kind::WeakRef(v) => value_edges(v, f),
        Kind::Promise(p) => {
            value_edges(&p.value, f);
            for r in p.fulfill.iter().chain(p.reject.iter()) {
                match r {
                    Reaction::Then {
                        handler,
                        derived,
                        cap,
                    } => {
                        if let Some(h) = handler {
                            value_edges(h, f);
                        }
                        if let Some(d) = derived {
                            f(Edge::Obj(d));
                        }
                        if let Some((a, b)) = cap {
                            value_edges(a, f);
                            value_edges(b, f);
                        }
                    }
                    Reaction::Resume(o) => f(Edge::Obj(o)),
                    Reaction::Native(_, vs) => values_edges(vs, f),
                }
            }
        }
        Kind::Generator(g) => {
            if let Some(fr) = &g.frame {
                frame_edges(fr, f);
            }
            for (_, v, o) in &g.queue {
                value_edges(v, f);
                f(Edge::Obj(o));
            }
        }
        Kind::Coroutine(fr) => {
            if let Some(fr) = fr {
                frame_edges(fr, f);
            }
        }
        Kind::ArrayIter { target, .. } => value_edges(target, f),
        Kind::MapIter { target, .. } => f(Edge::Obj(target)),
        Kind::RegExpStringIter { re, .. } => f(Edge::Obj(re)),
        Kind::ForIn { obj, .. } => f(Edge::Obj(obj)),
        Kind::TypedArray { buf_obj, .. } => {
            if let Some(b) = buf_obj {
                f(Edge::Obj(b));
            }
        }
        Kind::Proxy { target, handler } => {
            f(Edge::Obj(target));
            f(Edge::Obj(handler));
        }
        Kind::Host(h) => values_edges(&h.data, f),
    }
}

fn frame_edges(fr: &Frame, f: &mut impl FnMut(Edge)) {
    values_edges(&fr.stack, f);
    for l in &fr.locals {
        match l {
            Local::V(v) => value_edges(v, f),
            Local::C(c) => f(Edge::Cell(c)),
        }
    }
    if !fr.captures.is_empty() {
        f(Edge::Captures(&fr.captures));
    }
    values_edges(&fr.args, f);
    if let Some(o) = &fr.func {
        f(Edge::Obj(o));
    }
    value_edges(&fr.recv, f);
    match &fr.kind {
        FrameKind::Normal => {}
        FrameKind::Construct(v) => value_edges(v, f),
        FrameKind::Generator(o) => f(Edge::Obj(o)),
        FrameKind::Async { promise, co, .. } => {
            f(Edge::Obj(promise));
            f(Edge::Obj(co));
        }
    }
    if let Some(r) = &fr.resume {
        match r {
            Resume::Throw(v) | Resume::Return(v) => value_edges(v, f),
        }
    }
}
