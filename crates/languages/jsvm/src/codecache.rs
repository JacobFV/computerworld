//! A parse-and-compile cache shared by every VM on a thread.
//!
//! Compiled `Code` is immutable once built, so realms that load the same source
//! (every browser realm's `web.js` prelude, the same framework bundle on two
//! pages, a page reloaded) can share it instead of tokenizing, parsing and
//! compiling it again. Entries are keyed by the SHA-256 of everything that shapes
//! the compiled result: how it was compiled (program with its wrapper
//! parameters, or global-scope eval), the file name it reports in stack traces
//! and errors, and the source text. A lookup finds its entry by a fast 64-bit
//! hash of the same inputs and confirms it by comparing them byte for byte, so
//! loading a cached source costs a pass over it at memory speed rather than a
//! SHA-256 (which is computed once, when the entry is made). Only successful
//! compiles are cached; a syntax error is recompiled every time so it is
//! reported as before.
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
    /// The SHA-256 of the length-prefixed parts: the entry's identity.
    key: CacheKey,
    /// The parts themselves, compared on lookup.
    parts: Vec<Box<[u8]>>,
    code: Rc<Code>,
    is_module: bool,
    bytes: usize,
}

#[derive(Default)]
struct Cache {
    /// Entries by a fast 64-bit hash of their parts (a lookup hashes the
    /// source once at memory speed and confirms by comparing it, rather than
    /// running SHA-256 over every source every time it is loaded).
    map: HashMap<u64, Vec<Entry>>,
    order: VecDeque<(u64, CacheKey)>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
    static ENABLED: Cell<bool> = const { Cell::new(true) };
}

/// The SHA-256 key of a compile: its kind and options, the file name and the
/// source, each length-prefixed so no two different inputs share a byte
/// stream.
pub fn key(parts: &[&[u8]]) -> CacheKey {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_le_bytes());
        h.update(p);
    }
    h.finalize().into()
}

fn fast_hash(parts: &[&[u8]]) -> u64 {
    use std::hash::Hasher;
    let mut h = crate::value::FastHash::default();
    for p in parts {
        h.write_usize(p.len());
        h.write(p);
    }
    h.finish()
}

/// The cached compile of `parts`, with its key.
pub fn get(parts: &[&[u8]]) -> Option<(CacheKey, Rc<Code>, bool)> {
    if !enabled() {
        return None;
    }
    let fast = fast_hash(parts);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let hit = c.map.get(&fast).and_then(|list| {
            list.iter()
                .find(|e| {
                    e.parts.len() == parts.len()
                        && e.parts.iter().zip(parts).all(|(a, b)| &a[..] == *b)
                })
                .map(|e| (e.key, e.code.clone(), e.is_module))
        });
        match hit {
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

/// Keeps a successful compile of `parts`; returns its key.
pub fn put(parts: &[&[u8]], code: Rc<Code>, is_module: bool) -> CacheKey {
    let k = key(parts);
    let bytes: usize = parts.iter().map(|p| p.len()).sum();
    if !enabled() || bytes > CAPACITY_BYTES {
        return k;
    }
    let fast = fast_hash(parts);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.map
            .get(&fast)
            .is_some_and(|l| l.iter().any(|e| e.key == k))
        {
            return;
        }
        while c.bytes + bytes > CAPACITY_BYTES {
            let Some((of, ok)) = c.order.pop_front() else {
                break;
            };
            let mut freed = 0;
            if let Some(list) = c.map.get_mut(&of) {
                if let Some(i) = list.iter().position(|e| e.key == ok) {
                    freed = list.remove(i).bytes;
                }
                if list.is_empty() {
                    c.map.remove(&of);
                }
            }
            c.bytes -= freed;
        }
        c.bytes += bytes;
        c.order.push_back((fast, k));
        c.map.entry(fast).or_default().push(Entry {
            key: k,
            parts: parts.iter().map(|p| Box::<[u8]>::from(*p)).collect(),
            code,
            is_module,
            bytes,
        });
    });
    k
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
        (
            c.map.values().map(Vec::len).sum(),
            c.bytes,
            c.hits,
            c.misses,
        )
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
