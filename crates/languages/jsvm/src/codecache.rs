//! A parse-and-compile cache shared by every VM on a thread.
//!
//! Compiled `Code` is immutable once built, so realms that load the same source
//! (every browser realm's `web.js` prelude, the same framework bundle on two
//! pages, a page reloaded) can share it instead of tokenizing, parsing and
//! compiling it again. Entries are keyed by the SHA-256 of everything that shapes
//! the compiled result: how it was compiled (program with its wrapper
//! parameters, or global-scope eval), the file name it reports in stack traces
//! and errors, and the source text. Only successful compiles are cached; a
//! syntax error is recompiled every time so it is reported as before.
//!
//! What stays per realm: function objects (a closure is made from the shared
//! code by each realm), the "compiled once" charge on the virtual clock
//! (`Code::compiled_by` and `Vm::compiled`), and tagged-template objects
//! (`Vm::templates`). So sharing is invisible to programs: identity, source
//! positions, stack traces, `Function.prototype.toString` and timing all come
//! out the same as compiling afresh.
//!
//! The cache is per thread (the VM's values are `Rc`), bounded by the source
//! bytes it holds, and evicts the oldest entries first.

use crate::bytecode::Code;
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

/// Source bytes the cache may hold before it evicts.
pub const CAPACITY_BYTES: usize = 64 << 20;

pub type CacheKey = [u8; 32];

struct Entry {
    code: Rc<Code>,
    is_module: bool,
    bytes: usize,
}

#[derive(Default)]
struct Cache {
    map: HashMap<CacheKey, Entry>,
    order: VecDeque<CacheKey>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
    static ENABLED: Cell<bool> = const { Cell::new(true) };
}

/// The key for a compile: its kind and options, the file name and the source,
/// each length-prefixed so no two different inputs share a byte stream.
pub fn key(parts: &[&[u8]]) -> CacheKey {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
    }
    h.finalize().into()
}

pub fn get(k: &CacheKey) -> Option<(Rc<Code>, bool)> {
    if !enabled() {
        return None;
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        match c.map.get(k).map(|e| (e.code.clone(), e.is_module)) {
            Some(hit) => {
                c.hits += 1;
                Some(hit)
            }
            None => {
                c.misses += 1;
                None
            }
        }
    })
}

pub fn put(k: CacheKey, code: Rc<Code>, is_module: bool, bytes: usize) {
    if !enabled() || bytes > CAPACITY_BYTES {
        return;
    }
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.map.contains_key(&k) {
            return;
        }
        while c.bytes + bytes > CAPACITY_BYTES {
            let Some(old) = c.order.pop_front() else {
                break;
            };
            if let Some(e) = c.map.remove(&old) {
                c.bytes -= e.bytes;
            }
        }
        c.bytes += bytes;
        c.order.push_back(k);
        c.map.insert(
            k,
            Entry {
                code,
                is_module,
                bytes,
            },
        );
    })
}

pub fn enabled() -> bool {
    ENABLED.with(|e| e.get())
}

/// Turns the cache on or off for this thread (on by default); turning it off
/// also empties it.
pub fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
    if !on {
        clear();
    }
}

/// Drops every entry.
pub fn clear() {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.map.clear();
        c.order.clear();
        c.bytes = 0;
    });
}

/// (entries, source bytes held, hits, misses) on this thread.
pub fn stats() -> (usize, usize, u64, u64) {
    CACHE.with(|c| {
        let c = c.borrow();
        (c.map.len(), c.bytes, c.hits, c.misses)
    })
}

thread_local! {
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// A fresh identifier, unique on this thread: for `Code::uid` and `Vm::id`.
pub fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}
