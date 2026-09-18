//! Runtime values: strings with UTF-16 semantics, objects, property maps and
//! the internal kinds of exotic and built-in objects.

use crate::bigint::BigInt;
use crate::bytecode::Code;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::rc::Rc;

/// FNV-1a: deterministic hashing for lookup tables.
#[derive(Default)]
pub struct Fnv(u64);
impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 {
            0xcbf29ce484222325
        } else {
            self.0
        };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.0 = h;
    }
}
pub type FnvMap<K, V> = HashMap<K, V, BuildHasherDefault<Fnv>>;

// ---------------------------------------------------------------- strings

pub struct StrInner {
    pub s: String,
    pub ascii: bool,
    pub len16: usize,
}

#[derive(Clone)]
pub struct JsStr(pub Rc<StrInner>);

impl JsStr {
    pub fn new(s: impl Into<String>) -> JsStr {
        let s: String = s.into();
        let ascii = s.is_ascii();
        let len16 = if ascii {
            s.len()
        } else {
            s.encode_utf16().count()
        };
        JsStr(Rc::new(StrInner { s, ascii, len16 }))
    }
    pub fn as_str(&self) -> &str {
        &self.0.s
    }
    pub fn len16(&self) -> usize {
        self.0.len16
    }
    pub fn is_ascii(&self) -> bool {
        self.0.ascii
    }
    pub fn utf16(&self) -> Vec<u16> {
        self.0.s.encode_utf16().collect()
    }
    pub fn from_utf16(u: &[u16]) -> JsStr {
        JsStr::new(String::from_utf16_lossy(u))
    }
    pub fn code_unit(&self, i: usize) -> Option<u16> {
        if i >= self.len16() {
            return None;
        }
        if self.is_ascii() {
            return Some(self.0.s.as_bytes()[i] as u16);
        }
        self.0.s.encode_utf16().nth(i)
    }
    /// Substring by UTF-16 indices (clamped).
    pub fn slice16(&self, start: usize, end: usize) -> JsStr {
        let len = self.len16();
        let end = end.min(len);
        let start = start.min(end);
        if start == 0 && end == len {
            return self.clone();
        }
        if self.is_ascii() {
            return JsStr::new(&self.0.s[start..end]);
        }
        let u: Vec<u16> = self
            .0
            .s
            .encode_utf16()
            .skip(start)
            .take(end - start)
            .collect();
        JsStr::from_utf16(&u)
    }
    /// Appends in place when uniquely owned, else copies.
    pub fn append(this: &mut JsStr, other: &str) {
        if let Some(inner) = Rc::get_mut(&mut this.0) {
            inner.s.push_str(other);
            inner.ascii = inner.ascii && other.is_ascii();
            inner.len16 += if other.is_ascii() {
                other.len()
            } else {
                other.encode_utf16().count()
            };
            return;
        }
        let mut s = String::with_capacity(this.0.s.len() + other.len());
        s.push_str(&this.0.s);
        s.push_str(other);
        *this = JsStr::new(s);
    }
    /// UTF-16 index -> byte offset.
    pub fn byte_offset(&self, idx16: usize) -> usize {
        if self.is_ascii() {
            return idx16.min(self.0.s.len());
        }
        let mut n = 0;
        for (b, c) in self.0.s.char_indices() {
            if n >= idx16 {
                return b;
            }
            n += c.len_utf16();
        }
        self.0.s.len()
    }
    /// Byte offset -> UTF-16 index.
    pub fn index16(&self, byte: usize) -> usize {
        if self.is_ascii() {
            return byte;
        }
        self.0.s[..byte].encode_utf16().count()
    }
}

impl std::ops::Deref for JsStr {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0.s
    }
}
impl PartialEq for JsStr {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0) || self.0.s == o.0.s
    }
}
impl Eq for JsStr {}
impl Hash for JsStr {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.0.s.as_str().hash(h)
    }
}
impl std::borrow::Borrow<str> for JsStr {
    fn borrow(&self) -> &str {
        &self.0.s
    }
}
impl From<&str> for JsStr {
    fn from(s: &str) -> JsStr {
        JsStr::new(s)
    }
}
impl From<String> for JsStr {
    fn from(s: String) -> JsStr {
        JsStr::new(s)
    }
}
impl std::fmt::Display for JsStr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::fmt::Debug for JsStr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

// ---------------------------------------------------------------- symbols

pub struct Symbol {
    pub desc: Option<JsStr>,
    /// Private names (`#x`) are symbols hidden from reflection.
    pub private: bool,
    /// Registered via `Symbol.for`.
    pub registered: bool,
}

// ---------------------------------------------------------------- values

#[derive(Clone)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(JsStr),
    BigInt(Rc<BigInt>),
    Sym(Rc<Symbol>),
    Obj(Obj),
    /// Internal marker: array holes and uninitialised bindings.
    Empty,
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(JsStr::new(s))
    }
    pub fn string(s: String) -> Value {
        Value::Str(JsStr::new(s))
    }
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }
    pub fn as_obj(&self) -> Option<&Obj> {
        match self {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null | Value::Empty => false,
            Value::Bool(b) => *b,
            Value::Num(n) => !(*n == 0.0 || n.is_nan()),
            Value::Str(s) => !s.is_empty(),
            Value::BigInt(b) => !b.is_zero(),
            Value::Sym(_) | Value::Obj(_) => true,
        }
    }
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined | Value::Empty => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::BigInt(_) => "bigint",
            Value::Sym(_) => "symbol",
            Value::Obj(o) => {
                if o.is_callable() {
                    "function"
                } else {
                    "object"
                }
            }
        }
    }
    pub fn is_callable(&self) -> bool {
        matches!(self, Value::Obj(o) if o.is_callable())
    }
}

pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => strict_equals(a, b),
    }
}

pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else {
                x == y && x.is_sign_negative() == y.is_sign_negative()
            }
        }
        _ => strict_equals(a, b),
    }
}

pub fn strict_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::BigInt(x), Value::BigInt(y)) => x == y,
        (Value::Sym(x), Value::Sym(y)) => Rc::ptr_eq(x, y),
        (Value::Obj(x), Value::Obj(y)) => x.ptr_eq(y),
        _ => false,
    }
}

// ---------------------------------------------------------------- keys & properties

#[derive(Clone)]
pub enum Key {
    Str(JsStr),
    Sym(Rc<Symbol>),
}

impl Key {
    pub fn str(s: &str) -> Key {
        Key::Str(JsStr::new(s))
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Key::Str(s) => Some(s.as_str()),
            Key::Sym(_) => None,
        }
    }
    pub fn to_value(&self) -> Value {
        match self {
            Key::Str(s) => Value::Str(s.clone()),
            Key::Sym(s) => Value::Sym(s.clone()),
        }
    }
    pub fn array_index(&self) -> Option<u32> {
        match self {
            Key::Str(s) => crate::numconv::array_index(s),
            Key::Sym(_) => None,
        }
    }
    pub fn same(&self, o: &Key) -> bool {
        match (self, o) {
            (Key::Str(a), Key::Str(b)) => a == b,
            (Key::Sym(a), Key::Sym(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

pub const WRITABLE: u8 = 1;
pub const ENUMERABLE: u8 = 2;
pub const CONFIGURABLE: u8 = 4;
pub const ALL: u8 = 7;
/// Non-enumerable, writable, configurable (built-in methods).
pub const HIDDEN: u8 = WRITABLE | CONFIGURABLE;

#[derive(Clone)]
pub enum Slot {
    Data(Value),
    Accessor(Option<Obj>, Option<Obj>),
}

#[derive(Clone)]
pub struct Prop {
    pub slot: Slot,
    pub flags: u8,
}

impl Prop {
    pub fn data(v: Value, flags: u8) -> Prop {
        Prop {
            slot: Slot::Data(v),
            flags,
        }
    }
    pub fn enumerable(&self) -> bool {
        self.flags & ENUMERABLE != 0
    }
    pub fn writable(&self) -> bool {
        self.flags & WRITABLE != 0
    }
    pub fn configurable(&self) -> bool {
        self.flags & CONFIGURABLE != 0
    }
}

#[derive(Default)]
pub struct PropMap {
    pub entries: Vec<(Key, Prop)>,
    index: Option<FnvMap<JsStr, usize>>,
}

const INDEX_AT: usize = 10;

impl PropMap {
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn find_str(&self, k: &str) -> Option<usize> {
        if let Some(ix) = &self.index {
            return ix.get(k).copied();
        }
        self.entries
            .iter()
            .position(|(key, _)| matches!(key, Key::Str(s) if s.as_str() == k))
    }
    pub fn find(&self, k: &Key) -> Option<usize> {
        match k {
            Key::Str(s) => self.find_str(s),
            Key::Sym(sym) => self
                .entries
                .iter()
                .position(|(key, _)| matches!(key, Key::Sym(x) if Rc::ptr_eq(x, sym))),
        }
    }
    pub fn get(&self, k: &Key) -> Option<&Prop> {
        self.find(k).map(|i| &self.entries[i].1)
    }
    pub fn get_str(&self, k: &str) -> Option<&Prop> {
        self.find_str(k).map(|i| &self.entries[i].1)
    }
    pub fn get_mut(&mut self, k: &Key) -> Option<&mut Prop> {
        self.find(k).map(move |i| &mut self.entries[i].1)
    }
    pub fn insert(&mut self, k: Key, p: Prop) {
        if let Some(i) = self.find(&k) {
            self.entries[i].1 = p;
            return;
        }
        if let (Some(ix), Key::Str(s)) = (&mut self.index, &k) {
            ix.insert(s.clone(), self.entries.len());
        }
        self.entries.push((k, p));
        if self.index.is_none() && self.entries.len() > INDEX_AT {
            self.rebuild();
        }
    }
    fn rebuild(&mut self) {
        let mut ix = FnvMap::default();
        for (i, (k, _)) in self.entries.iter().enumerate() {
            if let Key::Str(s) = k {
                ix.insert(s.clone(), i);
            }
        }
        self.index = Some(ix);
    }
    pub fn remove(&mut self, k: &Key) -> Option<Prop> {
        let i = self.find(k)?;
        let (_, p) = self.entries.remove(i);
        if self.index.is_some() {
            self.rebuild();
        }
        Some(p)
    }
    pub fn set_value(&mut self, k: &str, v: Value) {
        match self.find_str(k) {
            Some(i) => self.entries[i].1.slot = Slot::Data(v),
            None => self.insert(Key::str(k), Prop::data(v, ALL)),
        }
    }
}

// ---------------------------------------------------------------- objects

#[derive(Clone)]
pub struct Obj(pub Rc<RefCell<ObjData>>);

impl Obj {
    pub fn new(data: ObjData) -> Obj {
        Obj(Rc::new(RefCell::new(data)))
    }
    pub fn borrow(&self) -> std::cell::Ref<'_, ObjData> {
        self.0.borrow()
    }
    pub fn borrow_mut(&self) -> std::cell::RefMut<'_, ObjData> {
        self.0.borrow_mut()
    }
    pub fn ptr_eq(&self, o: &Obj) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
    pub fn addr(&self) -> usize {
        Rc::as_ptr(&self.0) as *const u8 as usize
    }
    pub fn is_callable(&self) -> bool {
        match &self.borrow().kind {
            Kind::Function(_) => true,
            Kind::Proxy { target, .. } => target.is_callable(),
            _ => false,
        }
    }
    pub fn is_array(&self) -> bool {
        matches!(self.borrow().kind, Kind::Array(_))
    }
    pub fn proto(&self) -> Option<Obj> {
        self.borrow().proto.clone()
    }
    /// Own data property value (no getters, no proto walk).
    pub fn own_value(&self, k: &str) -> Option<Value> {
        match &self.borrow().props.get_str(k)?.slot {
            Slot::Data(v) => Some(v.clone()),
            _ => None,
        }
    }
    pub fn set_hidden(&self, k: &str, v: Value) {
        self.borrow_mut()
            .props
            .insert(Key::str(k), Prop::data(v, HIDDEN));
    }
    pub fn set_prop(&self, k: &str, v: Value, flags: u8) {
        self.borrow_mut()
            .props
            .insert(Key::str(k), Prop::data(v, flags));
    }
    pub fn set_sym(&self, k: &Rc<Symbol>, v: Value, flags: u8) {
        self.borrow_mut()
            .props
            .insert(Key::Sym(k.clone()), Prop::data(v, flags));
    }
}

pub struct ObjData {
    pub proto: Option<Obj>,
    pub props: PropMap,
    pub kind: Kind,
    pub extensible: bool,
    /// Array elements are frozen (Object.freeze) / sealed.
    pub elems_frozen: bool,
    pub elems_sealed: bool,
    /// Class name for objects built by class constructors is resolved via
    /// the prototype chain; this marks module namespaces, arguments, etc.
    pub tag: Option<&'static str>,
}

impl ObjData {
    pub fn new(proto: Option<Obj>, kind: Kind) -> ObjData {
        ObjData {
            proto,
            props: PropMap::default(),
            kind,
            extensible: true,
            elems_frozen: false,
            elems_sealed: false,
            tag: None,
        }
    }
}

pub type CellRef = Rc<RefCell<Value>>;

pub struct Args {
    pub this: Value,
    pub args: Vec<Value>,
    pub new_target: Option<Obj>,
    pub callee: Obj,
}

impl Args {
    pub fn arg(&self, i: usize) -> Value {
        self.args.get(i).cloned().unwrap_or(Value::Undefined)
    }
    pub fn len(&self) -> usize {
        self.args.len()
    }
    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }
}

pub enum Ctl {
    Throw(Value),
    /// `process.exit(code)`.
    Exit(i32),
    /// Uncatchable termination (step budget); carries the error to report.
    Fatal(Value),
}
pub type JsResult<T> = Result<T, Ctl>;

pub type NativeFn = fn(&mut crate::vm::Vm, &mut Args) -> JsResult<Value>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtorKind {
    /// Not a constructor (arrows, methods, most built-ins).
    None,
    /// Ordinary function / base class.
    Base,
    Derived,
}

pub enum FuncImpl {
    Closure {
        code: Rc<Code>,
        captures: Rc<[CellRef]>,
    },
    Native {
        f: NativeFn,
        slots: Vec<Value>,
    },
    Bound {
        target: Obj,
        this: Value,
        args: Vec<Value>,
    },
}

pub struct FuncData {
    pub imp: FuncImpl,
    pub ctor: CtorKind,
    /// Class constructors throw when called without `new`.
    pub class_ctor: bool,
    pub home: Option<Obj>,
    /// Instance field initialiser (class constructors).
    pub fields: Option<Obj>,
}

impl FuncData {
    pub fn code(&self) -> Option<&Rc<Code>> {
        match &self.imp {
            FuncImpl::Closure { code, .. } => Some(code),
            _ => None,
        }
    }
}

pub struct ErrorData {
    /// Captured stack frames, formatted ("    at f (file:1:2)").
    pub frames: Vec<String>,
    /// Location of the construction site, for the uncaught-error header.
    pub site: Option<Site>,
    /// Precomputed uncaught-error header ("file:line\nsource\n  ^\n").
    pub arrow: Option<String>,
    /// Escaped from a microtask / rejection: the header points at the
    /// construction site instead of the throw.
    pub from_async: bool,
}

#[derive(Clone, Debug)]
pub struct Site {
    pub file: Rc<str>,
    pub line: u32,
    pub col: u32,
}

pub struct RegExpData {
    pub source: JsStr,
    pub flags: JsStr,
    pub re: Rc<cw_regex::Regex>,
    pub global: bool,
    pub sticky: bool,
    pub unicode: bool,
    pub has_indices: bool,
}

/// Hashable identity of a Map/Set key under SameValueZero.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum HKey {
    Undef,
    Null,
    Bool(bool),
    Num(u64),
    Str(JsStr),
    Big(String),
    Ptr(usize),
}

pub fn hkey(v: &Value) -> HKey {
    match v {
        Value::Undefined | Value::Empty => HKey::Undef,
        Value::Null => HKey::Null,
        Value::Bool(b) => HKey::Bool(*b),
        Value::Num(n) => {
            let n = if *n == 0.0 {
                0.0
            } else if n.is_nan() {
                f64::NAN
            } else {
                *n
            };
            HKey::Num(n.to_bits())
        }
        Value::Str(s) => HKey::Str(s.clone()),
        Value::BigInt(b) => HKey::Big(b.to_str_radix(10)),
        Value::Sym(s) => HKey::Ptr(Rc::as_ptr(s) as *const u8 as usize),
        Value::Obj(o) => HKey::Ptr(o.addr()),
    }
}

#[derive(Default)]
pub struct MapData {
    pub entries: Vec<Option<(Value, Value)>>,
    pub index: FnvMap<HKey, usize>,
    pub live: usize,
}

impl MapData {
    pub fn get(&self, k: &Value) -> Option<&Value> {
        let i = *self.index.get(&hkey(k))?;
        self.entries[i].as_ref().map(|(_, v)| v)
    }
    pub fn has(&self, k: &Value) -> bool {
        self.index.contains_key(&hkey(k))
    }
    pub fn set(&mut self, k: Value, v: Value) {
        let h = hkey(&k);
        if let Some(&i) = self.index.get(&h) {
            if let Some(e) = &mut self.entries[i] {
                e.1 = v;
            }
            return;
        }
        // Normalise -0 to +0 as the spec requires.
        let k = if matches!(k, Value::Num(n) if n == 0.0) {
            Value::Num(0.0)
        } else {
            k
        };
        self.index.insert(h, self.entries.len());
        self.entries.push(Some((k, v)));
        self.live += 1;
    }
    pub fn delete(&mut self, k: &Value) -> bool {
        match self.index.remove(&hkey(k)) {
            Some(i) => {
                self.entries[i] = None;
                self.live -= 1;
                true
            }
            None => false,
        }
    }
    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = None;
        }
        self.index.clear();
        self.live = 0;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Clone)]
pub enum Reaction {
    /// JS handler (or None = pass through), derived promise capability.
    Then {
        handler: Option<Value>,
        derived: Option<Obj>,
        /// Resolve/reject functions of a non-native capability.
        cap: Option<(Value, Value)>,
    },
    /// Resume a suspended async function / await.
    Resume(Obj),
    /// Internal: async-from-sync iterator / Promise combinator steps.
    Native(NativeFn, Vec<Value>),
}

pub struct PromiseData {
    pub state: PromiseState,
    pub value: Value,
    pub fulfill: Vec<Reaction>,
    pub reject: Vec<Reaction>,
    pub handled: bool,
    /// Resolution already started (resolve functions called once).
    pub resolving: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenState {
    SuspendedStart,
    SuspendedYield,
    Running,
    Completed,
}

pub struct GenData {
    pub state: GenState,
    pub frame: Option<Box<crate::vm::Frame>>,
    /// Async generators: queued requests (kind, value, promise).
    pub queue: std::collections::VecDeque<(u8, Value, Obj)>,
    pub is_async: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IterKind {
    Keys,
    Values,
    Entries,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypedKind {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

pub enum Kind {
    Ordinary,
    Array(Vec<Value>),
    Function(Box<FuncData>),
    Error(Box<ErrorData>),
    Boolean(bool),
    Number(f64),
    String(JsStr),
    Symbol(Rc<Symbol>),
    BigInt(Rc<BigInt>),
    Date(f64),
    RegExp(Box<RegExpData>),
    Map(Box<MapData>),
    Set(Box<MapData>),
    WeakMap(Box<MapData>),
    WeakSet(Box<MapData>),
    WeakRef(Value),
    Promise(Box<PromiseData>),
    Generator(Box<GenData>),
    /// Suspended async function activation.
    Coroutine(Option<Box<crate::vm::Frame>>),
    ArrayIter {
        target: Value,
        index: usize,
        kind: IterKind,
        done: bool,
    },
    MapIter {
        target: Obj,
        index: usize,
        kind: IterKind,
        done: bool,
    },
    StringIter {
        s: JsStr,
        pos: usize,
        done: bool,
    },
    RegExpStringIter {
        re: Obj,
        s: JsStr,
        global: bool,
        unicode: bool,
        done: bool,
    },
    ForIn {
        keys: Vec<JsStr>,
        index: usize,
        obj: Obj,
    },
    Arguments,
    ArrayBuffer(Rc<RefCell<Vec<u8>>>),
    TypedArray {
        kind: TypedKind,
        buf: Rc<RefCell<Vec<u8>>>,
        offset: usize,
        len: usize,
        buf_obj: Option<Obj>,
    },
    Proxy {
        target: Obj,
        handler: Obj,
    },
    /// Iterator helper / generic wrapper objects created by natives.
    Internal(Vec<Value>),
}
