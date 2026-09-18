//! Builtin types, the `builtins` module, class creation and special attributes.
use crate::ast::{BinOp, CmpOp};
use crate::bigint::BigInt;
use crate::value::*;
use crate::vm::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub struct Types {
    pub object: Rc<Class>,
    pub type_: Rc<Class>,
    pub int: Rc<Class>,
    pub bool_: Rc<Class>,
    pub float: Rc<Class>,
    pub complex: Rc<Class>,
    pub str_: Rc<Class>,
    pub bytes: Rc<Class>,
    pub bytearray: Rc<Class>,
    pub list: Rc<Class>,
    pub tuple: Rc<Class>,
    pub dict: Rc<Class>,
    pub set: Rc<Class>,
    pub frozenset: Rc<Class>,
    pub range: Rc<Class>,
    pub slice: Rc<Class>,
    pub none_type: Rc<Class>,
    pub notimpl_type: Rc<Class>,
    pub ellipsis_type: Rc<Class>,
    pub function: Rc<Class>,
    pub builtin_function: Rc<Class>,
    pub method: Rc<Class>,
    pub module: Rc<Class>,
    pub generator: Rc<Class>,
    pub coroutine: Rc<Class>,
    pub list_iterator: Rc<Class>,
    pub tuple_iterator: Rc<Class>,
    pub str_iterator: Rc<Class>,
    pub range_iterator: Rc<Class>,
    pub set_iterator: Rc<Class>,
    pub dict_keyiterator: Rc<Class>,
    pub dict_valueiterator: Rc<Class>,
    pub dict_itemiterator: Rc<Class>,
    pub enumerate: Rc<Class>,
    pub zip: Rc<Class>,
    pub map: Rc<Class>,
    pub filter: Rc<Class>,
    pub reversed: Rc<Class>,
    pub dict_keys: Rc<Class>,
    pub dict_values: Rc<Class>,
    pub dict_items: Rc<Class>,
    pub property: Rc<Class>,
    pub staticmethod: Rc<Class>,
    pub classmethod: Rc<Class>,
    pub super_: Rc<Class>,
    pub textio: Rc<Class>,
    pub cell: Rc<Class>,
    pub code: Rc<Class>,
    pub exceptions: HashMap<&'static str, Rc<Class>>,
}

impl Types {
    pub fn exc(&self, name: &str) -> Rc<Class> {
        self.exceptions
            .get(name)
            .cloned()
            .unwrap_or_else(|| self.exceptions["Exception"].clone())
    }
}

pub fn new_class(name: &str, bases: Vec<Rc<Class>>, kind: Kind, builtin: bool) -> Rc<Class> {
    let cls = Rc::new(Class {
        name: RefCell::new(name.into()),
        qualname: RefCell::new(name.into()),
        bases: RefCell::new(bases.clone()),
        mro: RefCell::new(vec![]),
        dict: new_ref(Dict::new()),
        kind,
        builtin,
        metaclass: RefCell::new(None),
        abstract_methods: RefCell::new(vec![]),
    });
    let mro = c3(&cls, &bases).unwrap_or_else(|_| vec![cls.clone()]);
    *cls.mro.borrow_mut() = mro;
    if builtin {
        cls.dict
            .borrow_mut()
            .set_str("__module__", Value::str("builtins"));
    }
    cls
}

/// C3 linearization.
pub fn c3(cls: &Rc<Class>, bases: &[Rc<Class>]) -> Result<Vec<Rc<Class>>, String> {
    let mut seqs: Vec<Vec<Rc<Class>>> = bases.iter().map(|b| b.mro.borrow().clone()).collect();
    seqs.push(bases.to_vec());
    let mut out = vec![cls.clone()];
    loop {
        seqs.retain(|s| !s.is_empty());
        if seqs.is_empty() {
            return Ok(out);
        }
        let mut chosen = None;
        for s in &seqs {
            let cand = &s[0];
            let in_tail = seqs
                .iter()
                .any(|t| t[1..].iter().any(|c| Rc::ptr_eq(c, cand)));
            if !in_tail {
                chosen = Some(cand.clone());
                break;
            }
        }
        let Some(c) = chosen else {
            let names: Vec<String> = bases.iter().map(|b| b.name().to_string()).collect();
            return Err(format!(
                "Cannot create a consistent method resolution order (MRO) for bases {}",
                names.join(", ")
            ));
        };
        for s in seqs.iter_mut() {
            if Rc::ptr_eq(&s[0], &c) {
                s.remove(0);
            }
        }
        out.push(c);
    }
}

pub fn add_fn(cls: &Rc<Class>, name: &str, f: NativeFn) {
    let owner: &'static str = Box::leak(cls.name().to_string().into_boxed_str());
    let b = Builtin {
        name: format!("{}.{}", cls.name(), name).into(),
        func: f,
        data: Value::None,
        owner: Some(owner),
    };
    cls.dict
        .borrow_mut()
        .set_str(name, Value::Builtin(Rc::new(b)));
}
pub fn add_static(cls: &Rc<Class>, name: &str, f: NativeFn) {
    let b = Builtin {
        name: format!("{}.{}", cls.name(), name).into(),
        func: f,
        data: Value::None,
        owner: None,
    };
    cls.dict.borrow_mut().set_str(
        name,
        Value::StaticMethod(Rc::new(Value::Builtin(Rc::new(b)))),
    );
}
pub fn add_classmethod(cls: &Rc<Class>, name: &str, f: NativeFn) {
    let b = Builtin {
        name: format!("{}.{}", cls.name(), name).into(),
        func: f,
        data: Value::None,
        owner: None,
    };
    cls.dict.borrow_mut().set_str(
        name,
        Value::ClassMethod(Rc::new(Value::Builtin(Rc::new(b)))),
    );
}
pub fn native_fn(name: &str, f: NativeFn) -> Value {
    Value::Builtin(Rc::new(Builtin {
        name: name.into(),
        func: f,
        data: Value::None,
        owner: None,
    }))
}
pub fn native_closure(name: &str, f: NativeFn, data: Value) -> Value {
    Value::Builtin(Rc::new(Builtin {
        name: name.into(),
        func: f,
        data,
        owner: None,
    }))
}

const EXCEPTIONS: &[(&str, &str)] = &[
    ("BaseException", ""),
    ("SystemExit", "BaseException"),
    ("KeyboardInterrupt", "BaseException"),
    ("GeneratorExit", "BaseException"),
    ("Exception", "BaseException"),
    ("StopIteration", "Exception"),
    ("StopAsyncIteration", "Exception"),
    ("ArithmeticError", "Exception"),
    ("FloatingPointError", "ArithmeticError"),
    ("OverflowError", "ArithmeticError"),
    ("ZeroDivisionError", "ArithmeticError"),
    ("AssertionError", "Exception"),
    ("AttributeError", "Exception"),
    ("BufferError", "Exception"),
    ("EOFError", "Exception"),
    ("ImportError", "Exception"),
    ("ModuleNotFoundError", "ImportError"),
    ("LookupError", "Exception"),
    ("IndexError", "LookupError"),
    ("KeyError", "LookupError"),
    ("MemoryError", "Exception"),
    ("NameError", "Exception"),
    ("UnboundLocalError", "NameError"),
    ("OSError", "Exception"),
    ("BlockingIOError", "OSError"),
    ("ChildProcessError", "OSError"),
    ("ConnectionError", "OSError"),
    ("BrokenPipeError", "ConnectionError"),
    ("ConnectionAbortedError", "ConnectionError"),
    ("ConnectionRefusedError", "ConnectionError"),
    ("ConnectionResetError", "ConnectionError"),
    ("FileExistsError", "OSError"),
    ("FileNotFoundError", "OSError"),
    ("InterruptedError", "OSError"),
    ("IsADirectoryError", "OSError"),
    ("NotADirectoryError", "OSError"),
    ("PermissionError", "OSError"),
    ("ProcessLookupError", "OSError"),
    ("TimeoutError", "OSError"),
    ("ReferenceError", "Exception"),
    ("RuntimeError", "Exception"),
    ("NotImplementedError", "RuntimeError"),
    ("RecursionError", "RuntimeError"),
    ("SyntaxError", "Exception"),
    ("IndentationError", "SyntaxError"),
    ("TabError", "IndentationError"),
    ("SystemError", "Exception"),
    ("TypeError", "Exception"),
    ("ValueError", "Exception"),
    ("UnicodeError", "ValueError"),
    ("UnicodeDecodeError", "UnicodeError"),
    ("UnicodeEncodeError", "UnicodeError"),
    ("UnicodeTranslateError", "UnicodeError"),
    ("Warning", "Exception"),
    ("DeprecationWarning", "Warning"),
    ("PendingDeprecationWarning", "Warning"),
    ("RuntimeWarning", "Warning"),
    ("SyntaxWarning", "Warning"),
    ("UserWarning", "Warning"),
    ("FutureWarning", "Warning"),
    ("ImportWarning", "Warning"),
    ("UnicodeWarning", "Warning"),
    ("BytesWarning", "Warning"),
    ("ResourceWarning", "Warning"),
    ("EncodingWarning", "Warning"),
];

pub fn make_types() -> Types {
    let object = new_class("object", vec![], Kind::Object, true);
    let c = |name: &str, kind: Kind| new_class(name, vec![object.clone()], kind, true);
    let type_ = c("type", Kind::Type);
    let int = c("int", Kind::Int);
    let bool_ = new_class("bool", vec![int.clone()], Kind::Bool, true);
    let mut exceptions: HashMap<&'static str, Rc<Class>> = HashMap::new();
    for (name, base) in EXCEPTIONS {
        let base = if base.is_empty() {
            object.clone()
        } else {
            exceptions[base].clone()
        };
        exceptions.insert(name, new_class(name, vec![base], Kind::Exception, true));
    }
    Types {
        type_,
        int,
        bool_,
        float: c("float", Kind::Float),
        complex: c("complex", Kind::Complex),
        str_: c("str", Kind::Str),
        bytes: c("bytes", Kind::Bytes),
        bytearray: c("bytearray", Kind::ByteArray),
        list: c("list", Kind::List),
        tuple: c("tuple", Kind::Tuple),
        dict: c("dict", Kind::Dict),
        set: c("set", Kind::Set),
        frozenset: c("frozenset", Kind::FrozenSet),
        range: c("range", Kind::Other),
        slice: c("slice", Kind::Other),
        none_type: c("NoneType", Kind::NoneType),
        notimpl_type: c("NotImplementedType", Kind::Other),
        ellipsis_type: c("ellipsis", Kind::Other),
        function: c("function", Kind::Other),
        builtin_function: c("builtin_function_or_method", Kind::Other),
        method: c("method", Kind::Other),
        module: c("module", Kind::Other),
        generator: c("generator", Kind::Other),
        coroutine: c("coroutine", Kind::Other),
        list_iterator: c("list_iterator", Kind::Other),
        tuple_iterator: c("tuple_iterator", Kind::Other),
        str_iterator: c("str_ascii_iterator", Kind::Other),
        range_iterator: c("range_iterator", Kind::Other),
        set_iterator: c("set_iterator", Kind::Other),
        dict_keyiterator: c("dict_keyiterator", Kind::Other),
        dict_valueiterator: c("dict_valueiterator", Kind::Other),
        dict_itemiterator: c("dict_itemiterator", Kind::Other),
        enumerate: c("enumerate", Kind::Other),
        zip: c("zip", Kind::Other),
        map: c("map", Kind::Other),
        filter: c("filter", Kind::Other),
        reversed: c("reversed", Kind::Other),
        dict_keys: c("dict_keys", Kind::Other),
        dict_values: c("dict_values", Kind::Other),
        dict_items: c("dict_items", Kind::Other),
        property: c("property", Kind::Other),
        staticmethod: c("staticmethod", Kind::Other),
        classmethod: c("classmethod", Kind::Other),
        super_: c("super", Kind::Other),
        textio: c("TextIOWrapper", Kind::Other),
        cell: c("cell", Kind::Other),
        code: c("code", Kind::Other),
        exceptions,
        object,
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

pub fn arity(a: &Args, name: &str, min: usize, max: usize) -> PyResult<()> {
    if !a.kwargs.is_empty() {
        return Err(type_err(format!("{name}() takes no keyword arguments")));
    }
    let n = a.args.len();
    if n < min || n > max {
        if min == max {
            if min == 1 {
                return Err(type_err(format!(
                    "{name}() takes exactly one argument ({n} given)"
                )));
            }
            if min == 0 {
                return Err(type_err(format!("{name}() takes no arguments ({n} given)")));
            }
            return Err(type_err(format!(
                "{name} expected {min} arguments, got {n}"
            )));
        }
        if n < min {
            return Err(type_err(format!(
                "{name} expected at least {min} argument{}, got {n}",
                if min == 1 { "" } else { "s" }
            )));
        }
        return Err(type_err(format!(
            "{name} expected at most {max} argument{}, got {n}",
            if max == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

/// Takes positional-or-keyword parameters by name, in order.
pub fn take_params(
    a: &mut Args,
    fname: &str,
    names: &[&str],
    required: usize,
) -> PyResult<Vec<Option<Value>>> {
    let mut out: Vec<Option<Value>> = vec![None; names.len()];
    if a.args.len() > names.len() {
        return Err(type_err(format!(
            "{fname}() takes at most {} argument{} ({} given)",
            names.len(),
            if names.len() == 1 { "" } else { "s" },
            a.args.len()
        )));
    }
    for (i, v) in a.args.drain(..).enumerate() {
        out[i] = Some(v);
    }
    for (k, v) in a.kwargs.drain(..) {
        match names.iter().position(|n| **n == *k) {
            Some(i) => {
                if out[i].is_some() {
                    return Err(type_err(format!(
                        "argument for {fname}() given by name ('{k}') and position ({})",
                        i + 1
                    )));
                }
                out[i] = Some(v);
            }
            None => {
                return Err(type_err(format!(
                    "{fname}() got an unexpected keyword argument '{k}'"
                )))
            }
        }
    }
    for (i, o) in out.iter().enumerate().take(required) {
        if o.is_none() {
            return Err(type_err(format!(
                "{fname}() missing required argument '{}' (pos {})",
                names[i],
                i + 1
            )));
        }
    }
    Ok(out)
}

pub fn to_str_arg(vm: &Vm, v: &Value, what: &str) -> PyResult<Rc<PyStr>> {
    match vm.base_value(v) {
        Value::Str(s) => Ok(s),
        other => Err(type_err(format!(
            "{what} must be str, not {}",
            vm.type_name(&other)
        ))),
    }
}

pub fn to_int_arg(vm: &mut Vm, v: &Value) -> PyResult<i64> {
    match v {
        Value::Int(i) => Ok(*i),
        Value::Bool(b) => Ok(*b as i64),
        Value::Float(_) => Err(type_err(
            "'float' object cannot be interpreted as an integer",
        )),
        _ => vm.index_of(v),
    }
}

// ---------------------------------------------------------------------------
// Sets
// ---------------------------------------------------------------------------

pub fn set_items(v: &Value) -> Vec<Value> {
    match v {
        Value::Set(s) => s.borrow().items(),
        Value::FrozenSet(s) => s.items(),
        Value::DictView(dv) => match dv.1 {
            ViewKind::Keys => dv.0.borrow().keys(),
            ViewKind::Items => {
                dv.0.borrow()
                    .items()
                    .into_iter()
                    .map(|(k, v)| Value::tuple(vec![k, v]))
                    .collect()
            }
            ViewKind::Values => dv.0.borrow().values(),
        },
        Value::Instance(i) => match &*i.native.borrow() {
            NativeData::Base(b) => set_items(b),
            _ => vec![],
        },
        _ => vec![],
    }
}

pub fn set_data_of(v: &Value) -> Option<SetData> {
    match v {
        Value::Set(s) => Some(s.borrow().clone()),
        Value::FrozenSet(s) => Some((**s).clone()),
        Value::Instance(i) => match &*i.native.borrow() {
            NativeData::Base(b) => set_data_of(b),
            _ => None,
        },
        _ => None,
    }
}

/// `set(iterable)` construction following setobject.c's update paths.
pub fn build_set(vm: &mut Vm, it: &Value) -> PyResult<SetData> {
    if let Some(other) = set_data_of(it) {
        return Ok(SetData::copy_from(&other));
    }
    let mut s = SetData::new();
    if let Value::Dict(d) = it {
        let n = d.borrow().len();
        if n * 5 >= 7 * 3 {
            s.resize(n * 2);
        }
    }
    for x in vm.iterate(it)? {
        vm.set_add_data(&mut s, x)?;
    }
    Ok(s)
}

fn wrap_set(a: &Value, data: SetData) -> Value {
    match a {
        Value::FrozenSet(_) => Value::FrozenSet(Rc::new(data)),
        _ => Value::Set(new_ref(data)),
    }
}

pub fn set_binop(vm: &mut Vm, a: &Value, b: &Value, op: BinOp) -> PyResult<Value> {
    let da = match a {
        Value::DictView(_) => build_set(vm, &Value::list(set_items(a)))?,
        _ => set_data_of(a).unwrap_or_default(),
    };
    let db = match b {
        Value::DictView(_) => build_set(vm, &Value::list(set_items(b)))?,
        _ => set_data_of(b).unwrap_or_default(),
    };
    let result_kind = if matches!(a, Value::DictView(_)) {
        &Value::None
    } else {
        a
    };
    match op {
        BinOp::BitOr => {
            let mut r = SetData::copy_from(&da);
            for x in db.items() {
                vm.set_add_data(&mut r, x)?;
            }
            Ok(wrap_set(result_kind, r))
        }
        BinOp::BitAnd => {
            // Iterate the smaller set, as setobject.c does.
            let (small, big) = if db.len() > da.len() {
                (&da, &db)
            } else {
                (&db, &da)
            };
            let mut r = SetData::new();
            for x in small.items() {
                if vm.set_contains_data(big, &x)? {
                    vm.set_add_data(&mut r, x)?;
                }
            }
            Ok(wrap_set(result_kind, r))
        }
        BinOp::Sub => {
            if (da.len() >> 2) > db.len() {
                let mut r = SetData::copy_from(&da);
                let rr = new_ref(std::mem::take(&mut r));
                for x in db.items() {
                    vm.set_discard(&rr, &x)?;
                }
                let r = rr.borrow().clone();
                return Ok(wrap_set(result_kind, r));
            }
            let mut r = SetData::new();
            for x in da.items() {
                if !vm.set_contains_data(&db, &x)? {
                    vm.set_add_data(&mut r, x)?;
                }
            }
            Ok(wrap_set(result_kind, r))
        }
        BinOp::BitXor => {
            let mut r = SetData::copy_from(&da);
            let rr = new_ref(std::mem::take(&mut r));
            for x in db.items() {
                if !vm.set_discard(&rr, &x)? {
                    vm.set_add(&rr, x)?;
                }
            }
            let r = rr.borrow().clone();
            Ok(wrap_set(result_kind, r))
        }
        _ => Ok(Value::NotImplemented),
    }
}

pub fn set_compare(vm: &mut Vm, a: &Value, b: &Value, op: CmpOp) -> PyResult<Option<bool>> {
    let (Some(da), Some(db)) = (set_data_of(a), set_data_of(b)) else {
        return Ok(None);
    };
    let subset = |vm: &mut Vm, x: &SetData, y: &SetData| -> PyResult<bool> {
        if x.len() > y.len() {
            return Ok(false);
        }
        for it in x.items() {
            if !vm.set_contains_data(y, &it)? {
                return Ok(false);
            }
        }
        Ok(true)
    };
    Ok(Some(match op {
        CmpOp::LtE => subset(vm, &da, &db)?,
        CmpOp::Lt => da.len() < db.len() && subset(vm, &da, &db)?,
        CmpOp::GtE => subset(vm, &db, &da)?,
        CmpOp::Gt => db.len() < da.len() && subset(vm, &db, &da)?,
        _ => return Ok(None),
    }))
}

// ---------------------------------------------------------------------------
// Sequences: indexing and slicing
// ---------------------------------------------------------------------------

/// Normalizes slice bounds like PySlice_AdjustIndices; returns (start, stop, step, len).
pub fn slice_indices(vm: &mut Vm, s: &[Value; 3], len: i64) -> PyResult<(i64, i64, i64, i64)> {
    let get = |vm: &mut Vm, v: &Value| -> PyResult<Option<i64>> {
        match v {
            Value::None => Ok(None),
            Value::Big(b) => Ok(Some(if b.is_negative() {
                i64::MIN / 2
            } else {
                i64::MAX / 2
            })),
            other => vm.index_of(other).map(Some).map_err(|_| {
                type_err("slice indices must be integers or None or have an __index__ method")
            }),
        }
    };
    let step = get(vm, &s[2])?.unwrap_or(1);
    if step == 0 {
        return Err(value_err("slice step cannot be zero"));
    }
    let (lower, upper) = if step < 0 { (-1, len - 1) } else { (0, len) };
    let norm = |v: Option<i64>, default: i64| -> i64 {
        match v {
            None => default,
            Some(mut x) => {
                if x < 0 {
                    x += len;
                    if x < lower {
                        x = lower;
                    }
                } else if x > upper {
                    x = upper;
                }
                x
            }
        }
    };
    let start = norm(get(vm, &s[0])?, if step < 0 { upper } else { lower });
    let stop = norm(get(vm, &s[1])?, if step < 0 { lower } else { upper });
    let n = if step < 0 {
        if stop < start {
            (start - stop - 1) / (-step) + 1
        } else {
            0
        }
    } else if start < stop {
        (stop - start - 1) / step + 1
    } else {
        0
    };
    Ok((start, stop, step, n))
}

fn norm_index(i: i64, len: usize) -> Option<usize> {
    let n = len as i64;
    let j = if i < 0 { i + n } else { i };
    if j >= 0 && j < n {
        Some(j as usize)
    } else {
        None
    }
}

pub fn seq_getitem(vm: &mut Vm, obj: &Value, idx: &Value) -> PyResult<Value> {
    if let Value::Slice(s) = idx {
        let s = s.clone();
        return match obj {
            Value::List(l) => {
                let items = l.borrow().clone();
                let (start, _, step, n) = slice_indices(vm, &s, items.len() as i64)?;
                Ok(Value::list(
                    (0..n)
                        .map(|k| items[(start + k * step) as usize].clone())
                        .collect(),
                ))
            }
            Value::Tuple(t) => {
                let (start, _, step, n) = slice_indices(vm, &s, t.len() as i64)?;
                Ok(Value::tuple(
                    (0..n)
                        .map(|k| t[(start + k * step) as usize].clone())
                        .collect(),
                ))
            }
            Value::Str(st) => {
                let (start, stop, step, n) = slice_indices(vm, &s, st.nchars as i64)?;
                if step == 1 {
                    return Ok(Value::str(
                        st.substr(start as usize, stop.max(start) as usize),
                    ));
                }
                let chars: Vec<char> = st.s.chars().collect();
                Ok(Value::string(
                    (0..n).map(|k| chars[(start + k * step) as usize]).collect(),
                ))
            }
            Value::Bytes(b) => {
                let (start, _, step, n) = slice_indices(vm, &s, b.len() as i64)?;
                Ok(Value::Bytes(Rc::new(
                    (0..n).map(|k| b[(start + k * step) as usize]).collect(),
                )))
            }
            Value::ByteArray(b) => {
                let b = b.borrow().clone();
                let (start, _, step, n) = slice_indices(vm, &s, b.len() as i64)?;
                Ok(Value::ByteArray(new_ref(
                    (0..n).map(|k| b[(start + k * step) as usize]).collect(),
                )))
            }
            Value::Range(r) => {
                let (start, stop, step, _) = slice_indices(vm, &s, r.len())?;
                Ok(Value::Range(Rc::new(RangeObj {
                    start: r.get(start),
                    stop: r.get(stop),
                    step: r.step * step,
                })))
            }
            _ => Err(type_err(format!(
                "'{}' object is not subscriptable",
                vm.type_name(obj)
            ))),
        };
    }
    let (kind, len) = match obj {
        Value::List(l) => ("list", l.borrow().len()),
        Value::Tuple(t) => ("tuple", t.len()),
        Value::Str(s) => ("string", s.nchars),
        Value::Bytes(b) => ("index", b.len()),
        Value::ByteArray(b) => ("bytearray index", b.borrow().len()),
        Value::Range(r) => ("range object index", r.len() as usize),
        Value::Native(n) => {
            if let NativeKind::Deque(d, _) = &*n.data.borrow() {
                let i = match idx {
                    Value::Int(i) => *i,
                    _ => return Err(type_err("sequence index must be integer, not 'str'")),
                };
                return norm_index(i, d.len())
                    .map(|j| d[j].clone())
                    .ok_or_else(|| err("IndexError", "deque index out of range"));
            }
            return crate::modules::native_getitem(vm, obj, idx);
        }
        Value::DictView(_) => {
            return Err(type_err(format!(
                "'{}' object is not subscriptable",
                vm.type_name(obj)
            )))
        }
        _ => {
            return Err(type_err(format!(
                "'{}' object is not subscriptable",
                vm.type_name(obj)
            )))
        }
    };
    let i = match idx {
        Value::Int(i) => *i,
        Value::Bool(b) => *b as i64,
        Value::Big(_) => {
            return Err(err(
                "IndexError",
                format!("cannot fit 'int' into an index-sized integer"),
            ))
        }
        Value::Instance(_) if vm.lookup_special(idx, "__index__").is_some() => vm.index_of(idx)?,
        _ => {
            let tn = vm.type_name(idx);
            return Err(type_err(match obj {
                Value::Str(_) => format!("string indices must be integers, not '{tn}'"),
                Value::List(_) => format!("list indices must be integers or slices, not {tn}"),
                Value::Tuple(_) => format!("tuple indices must be integers or slices, not {tn}"),
                Value::Range(_) => format!("range indices must be integers or slices, not {tn}"),
                _ => format!("byte indices must be integers or slices, not {tn}"),
            }));
        }
    };
    let Some(j) = norm_index(i, len) else {
        return Err(err("IndexError", format!("{kind} out of range")));
    };
    Ok(match obj {
        Value::List(l) => l.borrow()[j].clone(),
        Value::Tuple(t) => t[j].clone(),
        Value::Str(s) => Value::string(s.char_at(j).unwrap_or(' ').to_string()),
        Value::Bytes(b) => Value::Int(b[j] as i64),
        Value::ByteArray(b) => Value::Int(b.borrow()[j] as i64),
        Value::Range(r) => Value::Int(r.get(j as i64)),
        _ => Value::None,
    })
}

pub fn list_setitem(vm: &mut Vm, obj: &Value, idx: &Value, v: Value) -> PyResult<()> {
    if let Value::ByteArray(b) = obj {
        let i = vm.index_of(idx)?;
        let len = b.borrow().len();
        let j =
            norm_index(i, len).ok_or_else(|| err("IndexError", "bytearray index out of range"))?;
        let byte = to_int_arg(vm, &v)?;
        if !(0..256).contains(&byte) {
            return Err(value_err("byte must be in range(0, 256)"));
        }
        b.borrow_mut()[j] = byte as u8;
        return Ok(());
    }
    let Value::List(l) = obj else { unreachable!() };
    if let Value::Slice(s) = idx {
        let items = vm
            .iterate(&v)
            .map_err(|_| type_err("must assign iterable to extended slice"))?;
        let len = l.borrow().len() as i64;
        let (start, stop, step, n) = slice_indices(vm, s, len)?;
        let mut list = l.borrow_mut();
        if step == 1 {
            let stop = stop.max(start);
            list.splice(start as usize..stop as usize, items);
            return Ok(());
        }
        if items.len() as i64 != n {
            return Err(value_err(format!(
                "attempt to assign sequence of size {} to extended slice of size {}",
                items.len(),
                n
            )));
        }
        for (k, it) in items.into_iter().enumerate() {
            list[(start + k as i64 * step) as usize] = it;
        }
        return Ok(());
    }
    let i = match idx {
        Value::Int(i) => *i,
        Value::Bool(b) => *b as i64,
        _ => {
            if vm.lookup_special(idx, "__index__").is_some() {
                vm.index_of(idx)?
            } else {
                return Err(type_err(format!(
                    "list indices must be integers or slices, not {}",
                    vm.type_name(idx)
                )));
            }
        }
    };
    let len = l.borrow().len();
    let j = norm_index(i, len)
        .ok_or_else(|| err("IndexError", "list assignment index out of range"))?;
    l.borrow_mut()[j] = v;
    Ok(())
}

pub fn list_delitem(vm: &mut Vm, obj: &Value, idx: &Value) -> PyResult<()> {
    let Value::List(l) = obj else {
        if let Value::ByteArray(b) = obj {
            let i = vm.index_of(idx)?;
            let len = b.borrow().len();
            let j = norm_index(i, len)
                .ok_or_else(|| err("IndexError", "bytearray index out of range"))?;
            b.borrow_mut().remove(j);
        }
        return Ok(());
    };
    if let Value::Slice(s) = idx {
        let len = l.borrow().len() as i64;
        let (start, _, step, n) = slice_indices(vm, s, len)?;
        let mut idxs: Vec<usize> = (0..n).map(|k| (start + k * step) as usize).collect();
        idxs.sort_unstable();
        let mut list = l.borrow_mut();
        for i in idxs.into_iter().rev() {
            list.remove(i);
        }
        return Ok(());
    }
    let i = match idx {
        Value::Int(i) => *i,
        _ => {
            return Err(type_err(format!(
                "list indices must be integers or slices, not {}",
                vm.type_name(idx)
            )))
        }
    };
    let len = l.borrow().len();
    let j = norm_index(i, len)
        .ok_or_else(|| err("IndexError", "list assignment index out of range"))?;
    l.borrow_mut().remove(j);
    Ok(())
}

pub fn native_setitem(vm: &mut Vm, obj: &Value, idx: &Value, v: Value) -> PyResult<()> {
    if let Value::Native(n) = obj {
        if let NativeKind::Deque(d, _) = &mut *n.data.borrow_mut() {
            let i = match idx {
                Value::Int(i) => *i,
                _ => return Err(type_err("sequence index must be integer")),
            };
            let j = norm_index(i, d.len())
                .ok_or_else(|| err("IndexError", "deque index out of range"))?;
            d[j] = v;
            return Ok(());
        }
    }
    Err(type_err(format!(
        "'{}' object does not support item assignment",
        vm.type_name(obj)
    )))
}

pub fn native_repr(vm: &mut Vm, v: &Value) -> PyResult<String> {
    let Value::Native(n) = v else {
        return Ok(String::new());
    };
    let items = match &*n.data.borrow() {
        NativeKind::Deque(d, maxlen) => Some((d.iter().cloned().collect::<Vec<_>>(), *maxlen)),
        _ => None,
    };
    if let Some((items, maxlen)) = items {
        let mut parts = vec![];
        for it in &items {
            parts.push(vm.repr(it)?);
        }
        return Ok(match maxlen {
            Some(m) => format!("deque([{}], maxlen={m})", parts.join(", ")),
            None => format!("deque([{}])", parts.join(", ")),
        });
    }
    crate::modules::native_repr(vm, v)
}

// ---------------------------------------------------------------------------
// Special attributes
// ---------------------------------------------------------------------------

pub fn exc_args(vm: &Vm, v: &Value) -> Vec<Value> {
    vm.exc_data(v, |d| match &d.args {
        Value::Tuple(t) => (**t).clone(),
        _ => vec![],
    })
    .unwrap_or_default()
}

pub fn exc_str(vm: &mut Vm, v: &Value) -> PyResult<String> {
    let args = exc_args(vm, v);
    let cls = vm.type_of(v);
    let is = |name: &str| cls.is_subclass(&vm.t.exc(name));
    if is("KeyError") && args.len() == 1 {
        return vm.repr(&args[0]);
    }
    if is("OSError") && args.len() >= 2 {
        if let Value::Int(no) = &args[0] {
            let strerror = vm.str_of(&args[1])?;
            if let Some(Value::Str(_)) | Some(Value::Bytes(_)) = args.get(2).map(|a| a) {
                let fname = vm.repr(&args[2])?;
                return Ok(format!("[Errno {no}] {strerror}: {fname}"));
            }
            return Ok(format!("[Errno {no}] {strerror}"));
        }
    }
    if is("SyntaxError") {
        if let Some(msg) = args.first() {
            return vm.str_of(msg);
        }
    }
    if is("UnicodeDecodeError") && args.len() == 5 {
        let enc = vm.str_of(&args[0])?;
        let reason = vm.str_of(&args[4])?;
        let start = vm.str_of(&args[2])?;
        let byte = match &args[1] {
            Value::Bytes(b) => b
                .get(start.parse::<usize>().unwrap_or(0))
                .copied()
                .unwrap_or(0),
            _ => 0,
        };
        return Ok(format!(
            "'{enc}' codec can't decode byte 0x{byte:02x} in position {start}: {reason}"
        ));
    }
    match args.len() {
        0 => Ok(String::new()),
        1 => vm.str_of(&args[0]),
        _ => vm.repr(&Value::tuple(args)),
    }
}

pub fn instance_special_attr(
    vm: &mut Vm,
    obj: &Value,
    inst: &Rc<Instance>,
    name: &str,
) -> PyResult<Option<Value>> {
    match name {
        "__class__" => return Ok(Some(Value::Class(inst.class()))),
        "__dict__" => return Ok(Some(Value::Dict(inst.dict.clone()))),
        _ => {}
    }
    let base = match &*inst.native.borrow() {
        NativeData::Base(b) => Some(b.clone()),
        _ => None,
    };
    if let Some(b) = base {
        return special_attr(vm, &b, name).map(|o| o.filter(|_| name != "__class__"));
    }
    let is_exc = matches!(&*inst.native.borrow(), NativeData::Exc(_));
    if !is_exc {
        return Ok(None);
    }
    let cls = inst.class();
    let is = |n: &str| cls.is_subclass(&vm.t.exc(n));
    let args = exc_args(vm, obj);
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::None);
    Ok(match name {
        "args" => Some(Value::tuple(args.clone())),
        "__traceback__" => Some(Value::None),
        "__cause__" => Some(
            vm.exc_data(obj, |d| d.cause.clone())
                .flatten()
                .unwrap_or(Value::None),
        ),
        "__context__" => Some(
            vm.exc_data(obj, |d| d.context.clone())
                .flatten()
                .unwrap_or(Value::None),
        ),
        "__suppress_context__" => Some(Value::Bool(
            vm.exc_data(obj, |d| d.suppress_context).unwrap_or(false),
        )),
        "__notes__" => None,
        "code" if is("SystemExit") => Some(match args.len() {
            0 => Value::None,
            1 => arg(0),
            _ => Value::tuple(args.clone()),
        }),
        "value" if is("StopIteration") => Some(arg(0)),
        "errno" if is("OSError") => Some(if args.len() >= 2 { arg(0) } else { Value::None }),
        "strerror" if is("OSError") => Some(if args.len() >= 2 { arg(1) } else { Value::None }),
        "filename" if is("OSError") => Some(arg(2)),
        "filename2" if is("OSError") => Some(Value::None),
        "msg" if is("SyntaxError") => Some(arg(0)),
        "filename" | "lineno" | "offset" | "text" | "end_lineno" | "end_offset"
            if is("SyntaxError") =>
        {
            let details = match arg(1) {
                Value::Tuple(t) => (*t).clone(),
                _ => vec![],
            };
            let i = match name {
                "filename" => 0,
                "lineno" => 1,
                "offset" => 2,
                "text" => 3,
                "end_lineno" => 4,
                _ => 5,
            };
            Some(details.get(i).cloned().unwrap_or(Value::None))
        }
        "name" if is("NameError") || is("ImportError") || is("AttributeError") => Some(Value::None),
        "obj" if is("AttributeError") => Some(Value::None),
        "path" if is("ImportError") => Some(Value::None),
        "encoding" if is("UnicodeError") => Some(arg(0)),
        "object" if is("UnicodeError") => Some(arg(1)),
        "start" if is("UnicodeError") => Some(arg(2)),
        "end" if is("UnicodeError") => Some(arg(3)),
        "reason" if is("UnicodeError") => Some(arg(4)),
        "add_note" => None,
        "with_traceback" => None,
        _ => None,
    })
}

pub fn instance_set_special(
    vm: &mut Vm,
    inst: &Rc<Instance>,
    name: &str,
    v: &Value,
) -> PyResult<Option<()>> {
    if name == "__class__" {
        if let Value::Class(c) = v {
            *inst.class.borrow_mut() = c.clone();
            return Ok(Some(()));
        }
        return Err(type_err("__class__ must be set to a class"));
    }
    let is_exc = matches!(&*inst.native.borrow(), NativeData::Exc(_));
    if !is_exc {
        return Ok(None);
    }
    let obj = Value::Instance(inst.clone());
    match name {
        "args" => {
            let items = vm.iterate(v)?;
            vm.exc_data(&obj, |d| d.args = Value::tuple(items));
        }
        "__cause__" => {
            let c = if v.is_none() { None } else { Some(v.clone()) };
            vm.exc_data(&obj, |d| {
                d.cause = c;
                d.suppress_context = true;
            });
        }
        "__context__" => {
            let c = if v.is_none() { None } else { Some(v.clone()) };
            vm.exc_data(&obj, |d| d.context = c);
        }
        "__suppress_context__" => {
            let b = vm.truthy(v)?;
            vm.exc_data(&obj, |d| d.suppress_context = b);
        }
        "__traceback__" => {}
        _ => return Ok(None),
    }
    Ok(Some(()))
}

pub fn class_special_attr(vm: &mut Vm, cls: &Rc<Class>, name: &str) -> PyResult<Option<Value>> {
    Ok(Some(match name {
        "__name__" => Value::str(&cls.name()),
        "__qualname__" => Value::str(&cls.qualname.borrow()),
        "__mro__" => Value::tuple(
            cls.mro
                .borrow()
                .iter()
                .map(|c| Value::Class(c.clone()))
                .collect(),
        ),
        "__bases__" => Value::tuple(
            cls.bases
                .borrow()
                .iter()
                .map(|c| Value::Class(c.clone()))
                .collect(),
        ),
        "__base__" => cls
            .bases
            .borrow()
            .first()
            .map(|c| Value::Class(c.clone()))
            .unwrap_or(Value::None),
        "__dict__" => Value::Dict(new_ref(cls.dict.borrow().clone())),
        "__doc__" => Value::None,
        "__class__" => Value::Class(vm.type_of(&Value::Class(cls.clone()))),
        "__module__" => Value::str("builtins"),
        "__abstractmethods__" => Value::FrozenSet(Rc::new({
            let mut s = SetData::new();
            for m in cls.abstract_methods.borrow().iter() {
                vm.set_add_data(&mut s, Value::str(m))?;
            }
            s
        })),
        "__annotations__" => {
            let d = Value::dict(Dict::new());
            cls.dict.borrow_mut().set_str("__annotations__", d.clone());
            d
        }
        _ => return Ok(None),
    }))
}

pub fn special_attr(vm: &mut Vm, obj: &Value, name: &str) -> PyResult<Option<Value>> {
    if name == "__class__" {
        return Ok(Some(Value::Class(vm.type_of(obj))));
    }
    Ok(match (obj, name) {
        (Value::Int(_) | Value::Bool(_) | Value::Big(_), "real" | "numerator") => Some(match obj {
            Value::Bool(b) => Value::Int(*b as i64),
            _ => obj.clone(),
        }),
        (Value::Int(_) | Value::Bool(_) | Value::Big(_), "imag") => Some(Value::Int(0)),
        (Value::Int(_) | Value::Bool(_) | Value::Big(_), "denominator") => Some(Value::Int(1)),
        (Value::Float(f), "real") => Some(Value::Float(*f)),
        (Value::Float(_), "imag") => Some(Value::Float(0.0)),
        (Value::Complex(r, _), "real") => Some(Value::Float(*r)),
        (Value::Complex(_, i), "imag") => Some(Value::Float(*i)),
        (Value::Range(r), "start") => Some(Value::Int(r.start)),
        (Value::Range(r), "stop") => Some(Value::Int(r.stop)),
        (Value::Range(r), "step") => Some(Value::Int(r.step)),
        (Value::Slice(s), "start") => Some(s[0].clone()),
        (Value::Slice(s), "stop") => Some(s[1].clone()),
        (Value::Slice(s), "step") => Some(s[2].clone()),
        (Value::Func(f), _) => match name {
            "__name__" => Some(Value::str(&f.name.borrow())),
            "__qualname__" => Some(Value::str(&f.qualname.borrow())),
            "__doc__" => Some(f.doc.borrow().clone()),
            "__module__" => Some(f.module.clone()),
            "__defaults__" => {
                let d = f.defaults.borrow();
                Some(if d.is_empty() {
                    Value::None
                } else {
                    Value::tuple(d.clone())
                })
            }
            "__kwdefaults__" => {
                let d = f.kwdefaults.borrow();
                if d.is_empty() {
                    Some(Value::None)
                } else {
                    let mut dict = Dict::new();
                    for (k, v) in d.iter() {
                        dict.set_str(k, v.clone());
                    }
                    Some(Value::dict(dict))
                }
            }
            "__dict__" => Some(Value::Dict(f.dict.clone())),
            "__code__" => Some(Value::Code(f.code.clone())),
            "__globals__" => Some(Value::Dict(f.globals.clone())),
            "__annotations__" => {
                let a = f.annotations.borrow().clone();
                Some(if a.is_none() {
                    let d = Value::dict(Dict::new());
                    *f.annotations.borrow_mut() = d.clone();
                    d
                } else {
                    a
                })
            }
            "__closure__" => Some(if f.closure.is_empty() {
                Value::None
            } else {
                Value::tuple(f.closure.iter().map(|c| Value::Cell(c.clone())).collect())
            }),
            _ => f.dict.borrow().get_str(name),
        },
        (Value::Builtin(b), "__name__") => {
            Some(Value::str(b.name.rsplit('.').next().unwrap_or(&b.name)))
        }
        (Value::Builtin(b), "__qualname__") => Some(Value::str(&b.name)),
        (Value::Builtin(_), "__doc__") => Some(Value::None),
        (Value::Builtin(_), "__module__") => Some(Value::str("builtins")),
        (Value::Builtin(_), "__self__") => Some(Value::None),
        (Value::Method(m), "__self__") => Some(m.0.clone()),
        (Value::Method(m), "__func__") => Some(m.1.clone()),
        (Value::Method(m), _) if name.starts_with("__") || !matches!(m.1, Value::Func(_)) => {
            let f = m.1.clone();
            return special_attr(vm, &f, name);
        }
        (Value::Method(m), _) => {
            let f = m.1.clone();
            return special_attr(vm, &f, name);
        }
        (Value::StaticMethod(f) | Value::ClassMethod(f), "__func__") => Some((**f).clone()),
        (Value::StaticMethod(f) | Value::ClassMethod(f), "__isabstractmethod__") => {
            let f = (**f).clone();
            return special_attr(vm, &f, name).map(|o| Some(o.unwrap_or(Value::Bool(false))));
        }
        (Value::Property(p), _) => match name {
            "fget" => Some(p.fget.clone()),
            "fset" => Some(p.fset.clone()),
            "fdel" => Some(p.fdel.clone()),
            "__doc__" => Some(p.doc.clone()),
            "__isabstractmethod__" => {
                let fget = p.fget.clone();
                match special_attr(vm, &fget, name)? {
                    Some(v) => Some(v),
                    None => Some(Value::Bool(false)),
                }
            }
            _ => None,
        },
        (Value::File(f), _) => {
            let f = f.borrow();
            match name {
                "name" => Some(f.name.clone()),
                "mode" => Some(Value::str(&f.mode)),
                "closed" => Some(Value::Bool(f.closed)),
                "encoding" => Some(Value::str("utf-8")),
                "buffer" => None,
                _ => None,
            }
        }
        (Value::Gen(g), _) => {
            let g = g.borrow();
            match name {
                "__name__" => Some(Value::str(&g.name)),
                "__qualname__" => Some(Value::str(&g.qualname)),
                "gi_running" => Some(Value::Bool(matches!(g.state, GenState::Running))),
                _ => None,
            }
        }
        (Value::Code(c), _) => match name {
            "co_name" => Some(Value::str(&c.name)),
            "co_qualname" => Some(Value::str(&c.qualname)),
            "co_filename" => Some(Value::str(&c.filename)),
            "co_firstlineno" => Some(Value::Int(c.firstlineno as i64)),
            "co_argcount" => Some(Value::Int(c.argcount as i64)),
            "co_varnames" => Some(Value::tuple(
                c.varnames.iter().map(|v| Value::str(v)).collect(),
            )),
            _ => None,
        },
        (Value::Cell(c), "cell_contents") => Some(c.borrow().clone()),
        (Value::Native(_), _) => crate::modules::native_attr(vm, obj, name)?,
        (_, "__doc__") => Some(Value::None),
        _ => None,
    })
}

/// Truncating numeric conversion used by int() and friends.
pub fn float_to_int(f: f64) -> PyResult<Value> {
    if f.is_nan() {
        return Err(value_err("cannot convert float NaN to integer"));
    }
    if f.is_infinite() {
        return Err(err(
            "OverflowError",
            "cannot convert float infinity to integer",
        ));
    }
    if f.abs() < 9.2e18 {
        return Ok(Value::Int(f.trunc() as i64));
    }
    Ok(Value::big(BigInt::from_f64(f)))
}

pub fn file_readline(vm: &mut Vm, f: &Ref<FileObj>, limit: i64) -> PyResult<Value> {
    crate::io::readline(vm, f, limit)
}
