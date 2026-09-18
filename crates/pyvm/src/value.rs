//! Runtime object model: values, strings, dicts, sets, classes, functions, code.
use crate::bigint::BigInt;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

pub type Ref<T> = Rc<RefCell<T>>;

pub fn new_ref<T>(v: T) -> Ref<T> {
    Rc::new(RefCell::new(v))
}

#[derive(Clone)]
pub enum Value {
    /// Internal marker: an unbound local, or "no self" in a method call pair.
    Undefined,
    None,
    NotImplemented,
    Ellipsis,
    Bool(bool),
    Int(i64),
    Big(Rc<BigInt>),
    Float(f64),
    Complex(f64, f64),
    Str(Rc<PyStr>),
    Bytes(Rc<Vec<u8>>),
    ByteArray(Ref<Vec<u8>>),
    Tuple(Rc<Vec<Value>>),
    List(Ref<Vec<Value>>),
    Dict(Ref<Dict>),
    Set(Ref<SetData>),
    FrozenSet(Rc<SetData>),
    Range(Rc<RangeObj>),
    Slice(Rc<[Value; 3]>),
    Func(Rc<Function>),
    Builtin(Rc<Builtin>),
    Method(Rc<(Value, Value)>),
    Class(Rc<Class>),
    Instance(Rc<Instance>),
    Module(Rc<Module>),
    Gen(Ref<Generator>),
    Iter(Ref<IterObj>),
    DictView(Rc<(Ref<Dict>, ViewKind)>),
    Property(Rc<Property>),
    StaticMethod(Rc<Value>),
    ClassMethod(Rc<Value>),
    Super(Rc<(Rc<Class>, Value)>),
    File(Ref<FileObj>),
    Cell(Ref<Value>),
    Code(Rc<Code>),
    Native(Rc<Native>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Keys,
    Values,
    Items,
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::new(PyStr::new(s.to_string())))
    }
    pub fn string(s: String) -> Value {
        Value::Str(Rc::new(PyStr::new(s)))
    }
    pub fn tuple(v: Vec<Value>) -> Value {
        Value::Tuple(Rc::new(v))
    }
    pub fn list(v: Vec<Value>) -> Value {
        Value::List(new_ref(v))
    }
    pub fn dict(d: Dict) -> Value {
        Value::Dict(new_ref(d))
    }
    pub fn big(b: BigInt) -> Value {
        match b.to_i64() {
            Some(i) => Value::Int(i),
            None => Value::Big(Rc::new(b)),
        }
    }
    pub fn is_none(&self) -> bool {
        matches!(self, Value::None)
    }
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }
    /// Pointer identity (`is`).
    pub fn is(&self, o: &Value) -> bool {
        use Value::*;
        match (self, o) {
            (Undefined, Undefined) | (None, None) | (NotImplemented, NotImplemented) => true,
            (Ellipsis, Ellipsis) => true,
            (Bool(a), Bool(b)) => a == b,
            // Small ints are cached in CPython; every i64 compares by value here.
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a.to_bits() == b.to_bits(),
            (Big(a), Big(b)) => Rc::ptr_eq(a, b),
            (Str(a), Str(b)) => Rc::ptr_eq(a, b) || (a.s.len() <= 1 && a.s == b.s),
            (Bytes(a), Bytes(b)) => Rc::ptr_eq(a, b),
            (ByteArray(a), ByteArray(b)) => Rc::ptr_eq(a, b),
            (Tuple(a), Tuple(b)) => Rc::ptr_eq(a, b) || (a.is_empty() && b.is_empty()),
            (List(a), List(b)) => Rc::ptr_eq(a, b),
            (Dict(a), Dict(b)) => Rc::ptr_eq(a, b),
            (Set(a), Set(b)) => Rc::ptr_eq(a, b),
            (FrozenSet(a), FrozenSet(b)) => Rc::ptr_eq(a, b),
            (Range(a), Range(b)) => Rc::ptr_eq(a, b),
            (Slice(a), Slice(b)) => Rc::ptr_eq(a, b),
            (Func(a), Func(b)) => Rc::ptr_eq(a, b),
            (Builtin(a), Builtin(b)) => Rc::ptr_eq(a, b),
            (Method(a), Method(b)) => Rc::ptr_eq(a, b),
            (Class(a), Class(b)) => Rc::ptr_eq(a, b),
            (Instance(a), Instance(b)) => Rc::ptr_eq(a, b),
            (Module(a), Module(b)) => Rc::ptr_eq(a, b),
            (Gen(a), Gen(b)) => Rc::ptr_eq(a, b),
            (Iter(a), Iter(b)) => Rc::ptr_eq(a, b),
            (DictView(a), DictView(b)) => Rc::ptr_eq(a, b),
            (Property(a), Property(b)) => Rc::ptr_eq(a, b),
            (StaticMethod(a), StaticMethod(b)) => Rc::ptr_eq(a, b),
            (ClassMethod(a), ClassMethod(b)) => Rc::ptr_eq(a, b),
            (Super(a), Super(b)) => Rc::ptr_eq(a, b),
            (File(a), File(b)) => Rc::ptr_eq(a, b),
            (Cell(a), Cell(b)) => Rc::ptr_eq(a, b),
            (Code(a), Code(b)) => Rc::ptr_eq(a, b),
            (Native(a), Native(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
    /// A stable identity number (`id()`), derived from the allocation address
    /// for heap objects. Only used for `id()` and default reprs.
    pub fn id(&self) -> usize {
        use Value::*;
        let p: usize = match self {
            // Identity keys only; the Python-visible id() is Vm::object_id.
            Undefined => 1,
            None => 2,
            NotImplemented => 3,
            Ellipsis => 4,
            Bool(b) => 5 + *b as usize,
            Int(i) => *i as usize,
            Float(f) => f.to_bits() as usize,
            Complex(a, _) => a.to_bits() as usize ^ 0x55,
            Big(r) => Rc::as_ptr(r) as usize,
            Str(r) => Rc::as_ptr(r) as usize,
            Bytes(r) => Rc::as_ptr(r) as usize,
            ByteArray(r) => Rc::as_ptr(r) as *const u8 as usize,
            Tuple(r) => Rc::as_ptr(r) as usize,
            List(r) => Rc::as_ptr(r) as *const u8 as usize,
            Dict(r) => Rc::as_ptr(r) as *const u8 as usize,
            Set(r) => Rc::as_ptr(r) as *const u8 as usize,
            FrozenSet(r) => Rc::as_ptr(r) as usize,
            Range(r) => Rc::as_ptr(r) as usize,
            Slice(r) => Rc::as_ptr(r) as *const u8 as usize,
            Func(r) => Rc::as_ptr(r) as usize,
            Builtin(r) => Rc::as_ptr(r) as usize,
            Method(r) => Rc::as_ptr(r) as *const u8 as usize,
            Class(r) => Rc::as_ptr(r) as usize,
            Instance(r) => Rc::as_ptr(r) as usize,
            Module(r) => Rc::as_ptr(r) as usize,
            Gen(r) => Rc::as_ptr(r) as *const u8 as usize,
            Iter(r) => Rc::as_ptr(r) as *const u8 as usize,
            DictView(r) => Rc::as_ptr(r) as *const u8 as usize,
            Property(r) => Rc::as_ptr(r) as usize,
            StaticMethod(r) => Rc::as_ptr(r) as usize,
            ClassMethod(r) => Rc::as_ptr(r) as usize,
            Super(r) => Rc::as_ptr(r) as *const u8 as usize,
            File(r) => Rc::as_ptr(r) as *const u8 as usize,
            Cell(r) => Rc::as_ptr(r) as *const u8 as usize,
            Code(r) => Rc::as_ptr(r) as usize,
            Native(r) => Rc::as_ptr(r) as usize,
        };
        p
    }
    pub fn as_pystr(&self) -> Option<&Rc<PyStr>> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// An immutable Python `str` with its character count and a cached hash.
pub struct PyStr {
    pub s: String,
    pub nchars: usize,
    hash: Cell<i64>,
}
impl PyStr {
    pub fn new(s: String) -> Self {
        let nchars = if s.is_ascii() {
            s.len()
        } else {
            s.chars().count()
        };
        Self {
            s,
            nchars,
            hash: Cell::new(0),
        }
    }
    pub fn as_str(&self) -> &str {
        &self.s
    }
    pub fn is_ascii(&self) -> bool {
        self.nchars == self.s.len()
    }
    pub fn hash(&self) -> i64 {
        let h = self.hash.get();
        if h != 0 {
            return h;
        }
        // FNV-1a over UTF-8: deterministic across runs (CPython randomizes per
        // process, so no particular value is promised to programs).
        let mut x: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.s.bytes() {
            x ^= b as u64;
            x = x.wrapping_mul(0x0100_0000_01b3);
        }
        let mut h = (x >> 1) as i64;
        if h == 0 || h == -1 {
            h = 2;
        }
        self.hash.set(h);
        h
    }
    /// Byte offset of the `i`-th character.
    pub fn byte_index(&self, i: usize) -> usize {
        if self.is_ascii() {
            return i.min(self.s.len());
        }
        self.s
            .char_indices()
            .nth(i)
            .map(|(b, _)| b)
            .unwrap_or(self.s.len())
    }
    pub fn char_at(&self, i: usize) -> Option<char> {
        if self.is_ascii() {
            return self.s.as_bytes().get(i).map(|b| *b as char);
        }
        self.s.chars().nth(i)
    }
    /// Characters `[a, b)`.
    pub fn substr(&self, a: usize, b: usize) -> &str {
        if b <= a {
            return "";
        }
        let x = self.byte_index(a);
        let y = self.byte_index(b);
        &self.s[x..y]
    }
}

// ---------- hashing ----------

#[derive(Default)]
pub struct IdHasher(u64);
impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_i64(&mut self, i: i64) {
        // Mix so sequential ints spread across buckets.
        let mut z = i as u64;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        self.0 = z ^ (z >> 31);
    }
}
pub type IdMap<K, V> = HashMap<K, V, BuildHasherDefault<IdHasher>>;

pub const HASH_MODULUS: i64 = (1 << 61) - 1;

pub fn hash_i64(i: i64) -> i64 {
    // CPython: sign * (|i| mod (2^61 - 1)), with -1 mapped to -2.
    let m = (i.unsigned_abs() % HASH_MODULUS as u64) as i64;
    let h = if i < 0 { -m } else { m };
    if h == -1 {
        -2
    } else {
        h
    }
}
pub fn hash_big(b: &BigInt) -> i64 {
    // Reduce the magnitude modulo 2^61 - 1 limb by limb.
    let mut acc: u128 = 0;
    for &limb in b.limbs().iter().rev() {
        acc = ((acc << 32) | limb as u128) % HASH_MODULUS as u128;
    }
    let m = acc as i64;
    let h = if b.is_negative() { -m } else { m };
    if h == -1 {
        -2
    } else {
        h
    }
}
pub fn hash_f64(f: f64) -> i64 {
    if f.is_nan() {
        return 0;
    }
    if f.is_infinite() {
        return if f > 0.0 { 314159 } else { -314159 };
    }
    if f.fract() == 0.0 && f.abs() < 9.2e18 {
        return hash_i64(f as i64);
    }
    // CPython's _Py_HashDouble.
    let (mut m, mut e) = frexp(f.abs());
    let sign = if f < 0.0 { -1i64 } else { 1 };
    let mut x: u64 = 0;
    const BITS: i32 = 61;
    while m != 0.0 {
        x = ((x << 28) & HASH_MODULUS as u64) | (x >> (BITS - 28));
        m *= 268435456.0;
        e -= 28;
        let y = m as u64;
        m -= y as f64;
        x += y;
        if x >= HASH_MODULUS as u64 {
            x -= HASH_MODULUS as u64;
        }
    }
    let e = if e >= 0 {
        e % BITS
    } else {
        BITS - 1 - ((-1 - e) % BITS)
    };
    x = ((x << e) & HASH_MODULUS as u64) | (x >> (BITS - e));
    let h = x as i64 * sign;
    if h == -1 {
        -2
    } else {
        h
    }
}
pub fn frexp(f: f64) -> (f64, i32) {
    if f == 0.0 || !f.is_finite() {
        return (f, 0);
    }
    let bits = f.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 {
        // Subnormal: scale up first.
        let (m, e) = frexp(f * 2f64.powi(54));
        return (m, e - 54);
    }
    let e = exp - 1022;
    let m = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (m, e)
}
pub fn hash_tuple(hashes: impl ExactSizeIterator<Item = i64>) -> i64 {
    const P1: u64 = 11400714785074694791;
    const P2: u64 = 14029467366897019727;
    const P5: u64 = 2870177450012600261;
    let len = hashes.len() as u64;
    let mut acc: u64 = P5;
    for h in hashes {
        acc = acc.wrapping_add((h as u64).wrapping_mul(P2));
        acc = acc.rotate_left(31);
        acc = acc.wrapping_mul(P1);
    }
    acc = acc.wrapping_add(len ^ (P5 ^ 3527539));
    if acc == u64::MAX {
        return 1546275796;
    }
    acc as i64
}

// ---------- dict ----------

#[derive(Clone)]
enum Slots {
    One(u32),
    Many(Vec<u32>),
}

/// Insertion-ordered hash map keyed by Python values. Key equality beyond the
/// primitive fast path is resolved by the VM, which calls `candidates` first.
#[derive(Clone, Default)]
pub struct Dict {
    entries: Vec<Option<(Value, Value, i64)>>,
    index: IdMap<i64, Slots>,
    live: usize,
    /// Bumped on every insertion or deletion so iterators can detect resizing.
    pub version: u64,
}

impl Dict {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.live
    }
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }
    /// Entry slots whose hash matches, with their keys.
    pub fn candidates(&self, hash: i64) -> Vec<(usize, Value)> {
        match self.index.get(&hash) {
            None => vec![],
            Some(Slots::One(i)) => {
                let e = self.entries[*i as usize].as_ref().unwrap();
                vec![(*i as usize, e.0.clone())]
            }
            Some(Slots::Many(v)) => v
                .iter()
                .map(|i| {
                    (
                        *i as usize,
                        self.entries[*i as usize].as_ref().unwrap().0.clone(),
                    )
                })
                .collect(),
        }
    }
    /// Fast lookup when keys compare primitively (the common case).
    #[allow(clippy::result_unit_err)]
    pub fn find_fast(&self, hash: i64, key: &Value) -> Result<Option<usize>, ()> {
        let check = |i: u32| -> Result<bool, ()> {
            let e = self.entries[i as usize].as_ref().unwrap();
            match fast_eq(&e.0, key) {
                Some(b) => Ok(b),
                None => Err(()),
            }
        };
        match self.index.get(&hash) {
            None => Ok(None),
            Some(Slots::One(i)) => Ok(if check(*i)? { Some(*i as usize) } else { None }),
            Some(Slots::Many(v)) => {
                for i in v {
                    if check(*i)? {
                        return Ok(Some(*i as usize));
                    }
                }
                Ok(None)
            }
        }
    }
    pub fn entry_value(&self, idx: usize) -> Option<&Value> {
        self.entries.get(idx)?.as_ref().map(|e| &e.1)
    }
    pub fn entry(&self, idx: usize) -> Option<&(Value, Value, i64)> {
        self.entries.get(idx)?.as_ref()
    }
    pub fn set_at(&mut self, idx: usize, v: Value) {
        if let Some(Some(e)) = self.entries.get_mut(idx) {
            e.1 = v;
        }
    }
    /// Appends a key known to be absent.
    pub fn push_new(&mut self, key: Value, value: Value, hash: i64) {
        let idx = self.entries.len() as u32;
        self.entries.push(Some((key, value, hash)));
        match self.index.get_mut(&hash) {
            None => {
                self.index.insert(hash, Slots::One(idx));
            }
            Some(s) => match s {
                Slots::One(i) => *s = Slots::Many(vec![*i, idx]),
                Slots::Many(v) => v.push(idx),
            },
        }
        self.live += 1;
        self.version += 1;
    }
    pub fn remove_at(&mut self, idx: usize) -> Option<(Value, Value)> {
        let (k, v, h) = self.entries.get_mut(idx)?.take()?;
        let remove_key = match self.index.get_mut(&h) {
            Some(Slots::One(_)) => true,
            Some(Slots::Many(list)) => {
                list.retain(|i| *i as usize != idx);
                list.is_empty()
            }
            None => false,
        };
        if remove_key {
            self.index.remove(&h);
        }
        self.live -= 1;
        self.version += 1;
        if self.entries.len() > 16 && self.live * 2 < self.entries.len() {
            self.compact();
        }
        Some((k, v))
    }
    fn compact(&mut self) {
        let old = std::mem::take(&mut self.entries);
        self.index.clear();
        self.live = 0;
        let version = self.version;
        for (k, v, h) in old.into_iter().flatten() {
            self.push_new(k, v, h);
        }
        self.version = version + 1;
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.live = 0;
        self.version += 1;
    }
    /// Live entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&Value, &Value)> {
        self.entries.iter().flatten().map(|(k, v, _)| (k, v))
    }
    pub fn keys(&self) -> Vec<Value> {
        self.iter().map(|(k, _)| k.clone()).collect()
    }
    pub fn values(&self) -> Vec<Value> {
        self.iter().map(|(_, v)| v.clone()).collect()
    }
    pub fn items(&self) -> Vec<(Value, Value)> {
        self.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    /// Raw slot count, for iterators that walk by position.
    pub fn slots(&self) -> usize {
        self.entries.len()
    }
    pub fn pop_last(&mut self) -> Option<(Value, Value)> {
        let idx = self.entries.iter().rposition(|e| e.is_some())?;
        self.remove_at(idx)
    }
    // String-keyed conveniences for namespaces (hash computed from the key).
    pub fn get_str(&self, key: &str) -> Option<Value> {
        let h = PyStr::new(key.to_string()).hash();
        self.get_str_hashed(key, h)
    }
    pub fn get_str_hashed(&self, key: &str, h: i64) -> Option<Value> {
        match self.index.get(&h)? {
            Slots::One(i) => {
                let e = self.entries[*i as usize].as_ref()?;
                match &e.0 {
                    Value::Str(s) if s.s == key => Some(e.1.clone()),
                    _ => None,
                }
            }
            Slots::Many(v) => v.iter().find_map(|i| {
                let e = self.entries[*i as usize].as_ref()?;
                match &e.0 {
                    Value::Str(s) if s.s == key => Some(e.1.clone()),
                    _ => None,
                }
            }),
        }
    }
    pub fn get_pystr(&self, key: &PyStr) -> Option<Value> {
        self.get_str_hashed(&key.s, key.hash())
    }
    pub fn set_str(&mut self, key: &str, value: Value) {
        let k = Rc::new(PyStr::new(key.to_string()));
        self.set_pystr(k, value);
    }
    pub fn set_pystr(&mut self, key: Rc<PyStr>, value: Value) {
        let h = key.hash();
        let kv = Value::Str(key);
        match self.find_fast(h, &kv) {
            Ok(Some(i)) => self.set_at(i, value),
            _ => self.push_new(kv, value, h),
        }
    }
    pub fn del_str(&mut self, key: &str) -> Option<Value> {
        let k = Value::str(key);
        let h = PyStr::new(key.to_string()).hash();
        let i = self.find_fast(h, &k).ok()??;
        self.remove_at(i).map(|(_, v)| v)
    }
    pub fn contains_str(&self, key: &str) -> bool {
        self.get_str(key).is_some()
    }
}

/// Equality that needs no user code: `Some(answer)` or `None` when a
/// user-defined `__eq__` could be involved.
pub fn fast_eq(a: &Value, b: &Value) -> Option<bool> {
    use Value::*;
    Some(match (a, b) {
        (Str(x), Str(y)) => Rc::ptr_eq(x, y) || x.s == y.s,
        (Int(x), Int(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Bool(y)) | (Bool(y), Int(x)) => *x == *y as i64,
        (Float(x), Bool(y)) | (Bool(y), Float(x)) => *x == *y as i64 as f64,
        (Big(_), Float(_)) | (Float(_), Big(_)) => return Option::None,
        (Float(x), Float(y)) => x == y,
        (Int(x), Float(y)) | (Float(y), Int(x)) => int_float_eq(*x, *y),
        (Big(x), Big(y)) => x == y,
        (Big(_), Int(_)) | (Int(_), Big(_)) => false,
        (None, None) => true,
        (Bytes(x), Bytes(y)) => x == y,
        (Tuple(x), Tuple(y)) => {
            if x.len() != y.len() {
                return Some(false);
            }
            for (p, q) in x.iter().zip(y.iter()) {
                if p.is(q) {
                    continue;
                }
                if !fast_eq(p, q)? {
                    return Some(false);
                }
            }
            true
        }
        (FrozenSet(_), _) | (_, FrozenSet(_)) => return Option::None,
        (Instance(_), _) | (_, Instance(_)) => {
            if a.is(b) {
                return Some(true);
            }
            return Option::None;
        }
        (Str(_), _) | (_, Str(_)) => false,
        (Int(_) | Float(_) | Bool(_) | Big(_), Tuple(_) | None | Bytes(_))
        | (Tuple(_) | None | Bytes(_), Int(_) | Float(_) | Bool(_) | Big(_)) => false,
        (None, _) | (_, None) => a.is(b),
        _ => {
            if a.is(b) {
                return Some(true);
            }
            return Option::None;
        }
    })
}

pub fn int_float_eq(i: i64, f: f64) -> bool {
    f.fract() == 0.0 && f.abs() < 9.2e18 && f as i64 == i
}

// ---------- set (CPython's open-addressing table, so iteration order matches) ----------

#[derive(Clone)]
enum SetSlot {
    Empty,
    Dummy,
    Full(Value, i64),
}

#[derive(Clone)]
pub struct SetData {
    table: Vec<SetSlot>,
    fill: usize,
    used: usize,
    pub version: u64,
}
impl Default for SetData {
    fn default() -> Self {
        Self::new()
    }
}

const LINEAR_PROBES: usize = 9;
const PERTURB_SHIFT: u32 = 5;

pub enum Probe {
    Found(usize),
    /// Slot to insert into (a dummy or an empty slot), and whether it was empty.
    Vacant(usize, bool),
    /// A slot whose key needs a user-level equality check.
    Check(usize, Value),
}

impl SetData {
    pub fn new() -> Self {
        Self {
            table: vec![SetSlot::Empty; 8],
            fill: 0,
            used: 0,
            version: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.used
    }
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }
    fn mask(&self) -> usize {
        self.table.len() - 1
    }
    /// Walks the probe sequence for `hash`; `skip` lists slots already compared
    /// unequal by the VM. Mirrors setobject.c's set_add_entry/set_lookkey.
    pub fn probe(&self, key: &Value, hash: i64, skip: &[usize]) -> Probe {
        let mask = self.mask();
        let mut i = (hash as u64 as usize) & mask;
        let mut perturb = hash as u64;
        let mut freeslot: Option<usize> = None;
        loop {
            let mut probes = if i + LINEAR_PROBES <= mask {
                LINEAR_PROBES
            } else {
                0
            };
            let mut j = i;
            loop {
                match &self.table[j] {
                    SetSlot::Empty => {
                        return match freeslot {
                            Some(f) => Probe::Vacant(f, false),
                            None => Probe::Vacant(j, true),
                        };
                    }
                    SetSlot::Full(k, h) if *h == hash && !skip.contains(&j) => {
                        match fast_eq(k, key) {
                            Some(true) => return Probe::Found(j),
                            Some(false) => {}
                            None => return Probe::Check(j, k.clone()),
                        }
                    }
                    SetSlot::Dummy if freeslot.is_none() => {
                        freeslot = Some(j);
                    }
                    _ => {}
                }
                if probes == 0 {
                    break;
                }
                probes -= 1;
                j += 1;
            }
            perturb >>= PERTURB_SHIFT;
            i = (i
                .wrapping_mul(5)
                .wrapping_add(1)
                .wrapping_add(perturb as usize))
                & mask;
        }
    }
    /// Inserts at a vacant slot returned by `probe`.
    pub fn insert_at(&mut self, slot: usize, was_empty: bool, key: Value, hash: i64) {
        self.table[slot] = SetSlot::Full(key, hash);
        self.used += 1;
        self.version += 1;
        if was_empty {
            self.fill += 1;
            if self.fill * 5 >= self.mask() * 3 {
                let minused = if self.used > 50000 {
                    self.used * 2
                } else {
                    self.used * 4
                };
                self.resize(minused);
            }
        }
    }
    pub fn remove_slot(&mut self, slot: usize) -> Value {
        let old = std::mem::replace(&mut self.table[slot], SetSlot::Dummy);
        self.used -= 1;
        self.version += 1;
        match old {
            SetSlot::Full(k, _) => k,
            _ => Value::None,
        }
    }
    pub fn resize(&mut self, minused: usize) {
        let mut newsize = 8;
        while newsize <= minused {
            newsize <<= 1;
        }
        let old = std::mem::replace(&mut self.table, vec![SetSlot::Empty; newsize]);
        self.fill = self.used;
        for slot in old {
            if let SetSlot::Full(k, h) = slot {
                self.insert_clean(k, h);
            }
        }
    }
    fn insert_clean(&mut self, key: Value, hash: i64) {
        let mask = self.mask();
        let mut perturb = hash as u64;
        let mut i = (hash as u64 as usize) & mask;
        loop {
            if matches!(self.table[i], SetSlot::Empty) {
                self.table[i] = SetSlot::Full(key, hash);
                return;
            }
            if i + LINEAR_PROBES <= mask {
                for j in 1..=LINEAR_PROBES {
                    if matches!(self.table[i + j], SetSlot::Empty) {
                        self.table[i + j] = SetSlot::Full(key, hash);
                        return;
                    }
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i
                .wrapping_mul(5)
                .wrapping_add(1)
                .wrapping_add(perturb as usize))
                & mask;
        }
    }
    /// `set(other_set)`: setobject.c's set_merge into an empty set.
    pub fn copy_from(other: &SetData) -> SetData {
        let mut s = SetData::new();
        if other.used * 5 >= s.mask() * 3 {
            s.resize(other.used * 2);
        }
        if s.table.len() == other.table.len() && other.fill == other.used {
            s.table = other.table.clone();
        } else {
            for slot in &other.table {
                if let SetSlot::Full(k, h) = slot {
                    s.insert_clean(k.clone(), *h);
                }
            }
        }
        s.fill = other.used;
        s.used = other.used;
        s
    }
    pub fn table_len(&self) -> usize {
        self.table.len()
    }
    pub fn fill(&self) -> usize {
        self.fill
    }
    pub fn clear(&mut self) {
        *self = SetData {
            version: self.version + 1,
            ..SetData::new()
        };
    }
    pub fn items(&self) -> Vec<Value> {
        self.table
            .iter()
            .filter_map(|s| match s {
                SetSlot::Full(k, _) => Some(k.clone()),
                _ => None,
            })
            .collect()
    }
    pub fn items_hashed(&self) -> Vec<(Value, i64)> {
        self.table
            .iter()
            .filter_map(|s| match s {
                SetSlot::Full(k, h) => Some((k.clone(), *h)),
                _ => None,
            })
            .collect()
    }
    /// set.pop() takes from a finger that advances through the table.
    pub fn pop_any(&mut self, finger: &mut usize) -> Option<Value> {
        if self.used == 0 {
            return None;
        }
        let mask = self.mask();
        let mut i = *finger & mask;
        loop {
            if let SetSlot::Full(..) = self.table[i] {
                let v = self.remove_slot(i);
                *finger = i + 1;
                return Some(v);
            }
            i = (i + 1) & mask;
        }
    }
}

// ---------- ranges ----------

pub struct RangeObj {
    pub start: i64,
    pub stop: i64,
    pub step: i64,
}
impl RangeObj {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn len(&self) -> i64 {
        let (lo, hi, step) = (self.start as i128, self.stop as i128, self.step as i128);
        let n = if step > 0 && lo < hi {
            (hi - lo - 1) / step + 1
        } else if step < 0 && lo > hi {
            (lo - hi - 1) / (-step) + 1
        } else {
            0
        };
        n.min(i64::MAX as i128) as i64
    }
    pub fn get(&self, i: i64) -> i64 {
        self.start + i * self.step
    }
}

// ---------- code and functions ----------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Nop,
    LoadConst(u32),
    LoadFast(u32),
    StoreFast(u32),
    DeleteFast(u32),
    LoadDeref(u32),
    StoreDeref(u32),
    DeleteDeref(u32),
    LoadClassDeref(u32),
    LoadClosure(u32),
    LoadGlobal(u32),
    StoreGlobal(u32),
    DeleteGlobal(u32),
    LoadName(u32),
    StoreName(u32),
    DeleteName(u32),
    LoadAttr(u32),
    StoreAttr(u32),
    DeleteAttr(u32),
    LoadMethod(u32),
    CallMethod(u32),
    Call(u32),
    /// argc includes keyword values; TOS is a tuple of keyword names.
    CallKw(u32),
    /// Stack: callable, args tuple[, kwargs dict].
    CallEx(bool),
    BinarySubscr,
    StoreSubscr,
    DeleteSubscr,
    Binary(crate::ast::BinOp),
    Inplace(crate::ast::BinOp),
    Unary(crate::ast::UnaryOp),
    Compare(crate::ast::CmpOp),
    Pop,
    Dup,
    DupTwo,
    Rot2,
    Rot3,
    Rot4,
    BuildTuple(u32),
    BuildList(u32),
    BuildSet(u32),
    BuildMap(u32),
    BuildString(u32),
    BuildSlice(u32),
    ListAppend(u32),
    SetAdd(u32),
    MapAdd(u32),
    ListExtend(u32),
    SetUpdate(u32),
    DictUpdate(u32),
    DictMerge(u32),
    ListToTuple,
    UnpackSequence(u32),
    UnpackEx(u32),
    Jump(u32),
    PopJumpIfFalse(u32),
    PopJumpIfTrue(u32),
    JumpIfFalseOrPop(u32),
    JumpIfTrueOrPop(u32),
    GetIter,
    ForIter(u32),
    Return,
    SetupFinally(u32),
    PopBlock,
    PopExcept,
    Reraise,
    Raise(u32),
    JumpIfNotExcMatch(u32),
    SetupWith(u32),
    WithExit,
    WithExceptStart,
    Yield,
    YieldFrom,
    GetYieldFromIter,
    GetAwaitable,
    MakeFunction(u32),
    LoadBuildClass,
    ImportName(u32),
    ImportFrom(u32),
    ImportStar,
    FormatValue(u32),
    LoadAssertionError,
    SetupAnnotations,
    PrintExpr,
    // Structural pattern matching helpers.
    MatchSequence(u32),
    MatchStar(u32, u32),
    MatchMapping,
    MatchKeys(u32),
    MatchClass(u32),
    GetLen,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Pos {
    pub line: u32,
    pub end_line: u32,
    pub col: u32,
    pub end_col: u32,
    /// Caret anchors: BinOp (left.end_col, right.col, 1) or Subscript (value.end_col, slice.end_col, 2).
    pub anchor: (u32, u32, u8),
}

pub struct Code {
    pub name: Rc<str>,
    pub qualname: Rc<str>,
    pub filename: Rc<str>,
    pub ops: Vec<Op>,
    pub pos: Vec<Pos>,
    pub consts: Vec<Value>,
    pub names: Vec<Rc<PyStr>>,
    pub varnames: Vec<Rc<str>>,
    pub cellvars: Vec<Rc<str>>,
    pub freevars: Vec<Rc<str>>,
    /// For each cellvar that is also an argument, its argument index.
    pub cell2arg: Vec<Option<u32>>,
    pub argcount: u32,
    pub posonlyargcount: u32,
    pub kwonlyargcount: u32,
    pub varargs: bool,
    pub varkw: bool,
    pub is_generator: bool,
    pub is_coroutine: bool,
    pub firstlineno: u32,
    /// Module or class body: names live in a dict, not fast locals.
    pub uses_name_ops: bool,
    pub docstring: Option<String>,
}

pub struct Function {
    pub code: Rc<Code>,
    pub globals: Ref<Dict>,
    pub defaults: RefCell<Vec<Value>>,
    pub kwdefaults: RefCell<Vec<(Rc<str>, Value)>>,
    pub closure: Vec<Ref<Value>>,
    pub name: RefCell<Rc<str>>,
    pub qualname: RefCell<Rc<str>>,
    pub dict: Ref<Dict>,
    pub module: Value,
    pub doc: RefCell<Value>,
    pub annotations: RefCell<Value>,
}

pub struct Args {
    pub args: Vec<Value>,
    pub kwargs: Vec<(Rc<str>, Value)>,
}
impl Args {
    pub fn new(args: Vec<Value>) -> Self {
        Self {
            args,
            kwargs: vec![],
        }
    }
    pub fn kw(&mut self, name: &str) -> Option<Value> {
        let i = self.kwargs.iter().position(|(k, _)| &**k == name)?;
        Some(self.kwargs.remove(i).1)
    }
}

pub type NativeFn = fn(&mut crate::vm::Vm, Args) -> crate::vm::PyResult<Value>;

pub struct Builtin {
    pub name: Rc<str>,
    pub func: NativeFn,
    /// Captured state for natively built callables (bound data, partials).
    pub data: Value,
    /// For methods of builtin types: the owning type's name, used in messages.
    pub owner: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Object,
    Type,
    Int,
    Bool,
    Float,
    Complex,
    Str,
    Bytes,
    ByteArray,
    Tuple,
    List,
    Dict,
    Set,
    FrozenSet,
    Exception,
    NoneType,
    Other,
}

pub struct Class {
    pub name: RefCell<Rc<str>>,
    pub qualname: RefCell<Rc<str>>,
    pub bases: RefCell<Vec<Rc<Class>>>,
    pub mro: RefCell<Vec<Rc<Class>>>,
    pub dict: Ref<Dict>,
    /// The builtin layout this class (or its nearest builtin base) provides.
    pub kind: Kind,
    pub builtin: bool,
    pub metaclass: RefCell<Option<Rc<Class>>>,
    /// Class-level `__slots__` or other flags could go here.
    pub abstract_methods: RefCell<Vec<Rc<str>>>,
}
impl Class {
    pub fn name(&self) -> Rc<str> {
        self.name.borrow().clone()
    }
    pub fn lookup(&self, name: &str) -> Option<Value> {
        for c in self.mro.borrow().iter() {
            if let Some(v) = c.dict.borrow().get_str(name) {
                return Some(v);
            }
        }
        None
    }
    pub fn lookup_pystr(&self, name: &PyStr) -> Option<Value> {
        for c in self.mro.borrow().iter() {
            if let Some(v) = c.dict.borrow().get_pystr(name) {
                return Some(v);
            }
        }
        None
    }
    pub fn is_subclass(&self, other: &Rc<Class>) -> bool {
        self.mro.borrow().iter().any(|c| Rc::ptr_eq(c, other))
    }
}

pub struct ExcData {
    pub args: Value,
    /// (filename, line, function name, position) innermost first.
    pub traceback: Vec<TbEntry>,
    pub cause: Option<Value>,
    pub context: Option<Value>,
    pub suppress_context: bool,
}

#[derive(Clone)]
pub struct TbEntry {
    pub filename: Rc<str>,
    pub name: Rc<str>,
    pub pos: Pos,
}

pub enum NativeData {
    None,
    /// The builtin value a subclass of a builtin type wraps.
    Base(Value),
    Exc(Box<ExcData>),
}

pub struct Instance {
    pub class: RefCell<Rc<Class>>,
    pub dict: Ref<Dict>,
    pub native: RefCell<NativeData>,
}
impl Instance {
    pub fn class(&self) -> Rc<Class> {
        self.class.borrow().clone()
    }
}

pub struct Module {
    pub name: Rc<str>,
    pub dict: Ref<Dict>,
}

pub struct Property {
    pub fget: Value,
    pub fset: Value,
    pub fdel: Value,
    pub doc: Value,
}

// ---------- generators and iterators ----------

pub enum GenState {
    Created,
    Suspended,
    Running,
    Done,
}

pub struct Generator {
    pub frame: Option<Box<crate::vm::Frame>>,
    pub state: GenState,
    pub name: Rc<str>,
    pub qualname: Rc<str>,
    pub is_coroutine: bool,
}

pub enum IterObj {
    Seq {
        seq: Value,
        idx: usize,
    },
    Str {
        s: Rc<PyStr>,
        byte: usize,
    },
    Range {
        cur: i64,
        step: i64,
        remaining: i64,
    },
    Dict {
        dict: Ref<Dict>,
        slot: usize,
        kind: ViewKind,
        version_len: usize,
    },
    List {
        items: Vec<Value>,
        idx: usize,
    },
    Reversed {
        seq: Value,
        idx: isize,
    },
    Enumerate {
        it: Value,
        count: Value,
    },
    Zip {
        its: Vec<Value>,
        strict: bool,
    },
    Map {
        func: Value,
        its: Vec<Value>,
    },
    Filter {
        func: Value,
        it: Value,
    },
    Callable {
        func: Value,
        sentinel: Value,
    },
    GetItem {
        obj: Value,
        idx: i64,
    },
    Done,
}

// ---------- files ----------

pub struct FileObj {
    pub path: String,
    pub name: Value,
    pub mode: String,
    pub binary: bool,
    pub readable: bool,
    pub writable: bool,
    pub append: bool,
    pub closed: bool,
    /// Whole contents for reading; pending bytes for writing.
    pub data: Vec<u8>,
    pub pos: usize,
    /// Standard streams: 0 stdin, 1 stdout, 2 stderr.
    pub std: Option<u8>,
    pub dirty: bool,
}

/// Host-backed objects implemented in Rust modules (regex patterns, deques…).
pub struct Native {
    pub class: Rc<Class>,
    pub data: RefCell<NativeKind>,
}

pub enum NativeKind {
    Pattern(Rc<crate::modules::re::PatternObj>),
    Match(Rc<crate::modules::re::MatchObj>),
    Random(Box<crate::modules::random::Mt>),
    Deque(std::collections::VecDeque<Value>, Option<usize>),
    Other,
}
