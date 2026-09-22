//! `_collections`: a native `deque` (O(1) at both ends) used by `collections`.
use super::{new_module, set_val};
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

fn deque_of(vm: &Vm, v: &Value) -> PyResult<Rc<Native>> {
    match vm.base_value(v) {
        Value::Native(n) if matches!(&*n.data.borrow(), NativeKind::Deque(..)) => Ok(n),
        _ => Err(type_err("descriptor requires a 'collections.deque' object")),
    }
}
fn with<R>(n: &Rc<Native>, f: impl FnOnce(&mut VecDeque<Value>, Option<usize>) -> R) -> R {
    match &mut *n.data.borrow_mut() {
        NativeKind::Deque(d, m) => f(d, *m),
        _ => unreachable!(),
    }
}
fn items(n: &Rc<Native>) -> Vec<Value> {
    with(n, |d, _| d.iter().cloned().collect())
}
fn push_back(d: &mut VecDeque<Value>, max: Option<usize>, v: Value) {
    if max == Some(0) {
        return;
    }
    d.push_back(v);
    if let Some(m) = max {
        while d.len() > m {
            d.pop_front();
        }
    }
}
fn push_front(d: &mut VecDeque<Value>, max: Option<usize>, v: Value) {
    if max == Some(0) {
        return;
    }
    d.push_front(v);
    if let Some(m) = max {
        while d.len() > m {
            d.pop_back();
        }
    }
}

fn parse_args(vm: &mut Vm, a: &mut Args) -> PyResult<(Vec<Value>, Option<usize>)> {
    let p = take_params(a, "deque", &["iterable", "maxlen"], 0)?;
    let maxlen = match &p[1] {
        None | Some(Value::None) => None,
        Some(v) => {
            let n = to_int_arg(vm, v)?;
            if n < 0 {
                return Err(value_err("maxlen must be non-negative"));
            }
            Some(n as usize)
        }
    };
    let items = match &p[0] {
        Some(v) => vm.iterate(v)?,
        None => vec![],
    };
    Ok((items, maxlen))
}

pub fn new_deque(cls: &Rc<Class>, items: Vec<Value>, maxlen: Option<usize>) -> Value {
    let mut d = VecDeque::new();
    for it in items {
        push_back(&mut d, maxlen, it);
    }
    Value::Native(Rc::new(Native {
        class: cls.clone(),
        data: RefCell::new(NativeKind::Deque(d, maxlen)),
    }))
}

fn dq_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = match a.args.remove(0) {
        Value::Class(c) => c,
        _ => return Err(type_err("expected class")),
    };
    if cls.builtin {
        let (items, maxlen) = parse_args(vm, &mut a)?;
        return Ok(new_deque(&cls, items, maxlen));
    }
    let base = new_deque(&cls, vec![], None);
    let inst = vm.new_instance(&cls);
    if let Value::Instance(i) = &inst {
        *i.native.borrow_mut() = NativeData::Base(base);
    }
    Ok(inst)
}
fn dq_init(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args.remove(0))?;
    let (items, maxlen) = parse_args(vm, &mut a)?;
    *n.data.borrow_mut() = NativeKind::Deque(VecDeque::new(), maxlen);
    with(&n, |d, m| {
        for it in items {
            push_back(d, m, it);
        }
    });
    Ok(Value::None)
}
fn dq_append(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    with(&n, |d, m| push_back(d, m, a.args[1].clone()));
    Ok(Value::None)
}
fn dq_appendleft(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    with(&n, |d, m| push_front(d, m, a.args[1].clone()));
    Ok(Value::None)
}
fn dq_pop(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    with(&n, |d, _| d.pop_back()).ok_or_else(|| err("IndexError", "pop from an empty deque"))
}
fn dq_popleft(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    with(&n, |d, _| d.pop_front()).ok_or_else(|| err("IndexError", "pop from an empty deque"))
}
fn dq_extend(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let its = vm.iterate(&a.args[1])?;
    with(&n, |d, m| {
        for it in its {
            push_back(d, m, it);
        }
    });
    Ok(Value::None)
}
fn dq_extendleft(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let its = vm.iterate(&a.args[1])?;
    with(&n, |d, m| {
        for it in its {
            push_front(d, m, it);
        }
    });
    Ok(Value::None)
}
fn dq_clear(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    with(&n, |d, _| d.clear());
    Ok(Value::None)
}
fn dq_copy(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let (its, m) = with(&n, |d, m| (d.iter().cloned().collect::<Vec<_>>(), m));
    Ok(new_deque(&n.class, its, m))
}
fn dq_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let mut c = 0;
    for x in items(&n) {
        if vm.eq(&x, &a.args[1])? {
            c += 1;
        }
    }
    Ok(Value::Int(c))
}
fn dq_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    for (i, x) in items(&n).iter().enumerate() {
        if vm.eq(x, &a.args[1])? {
            return Ok(Value::Int(i as i64));
        }
    }
    let r = vm.repr(&a.args[1])?;
    Err(value_err(format!("{r} is not in deque")))
}
fn dq_insert(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let i = to_int_arg(vm, &a.args[1])?;
    with(&n, |d, m| {
        if m.is_some_and(|m| d.len() >= m) {
            return Err(err("IndexError", "deque already at its maximum size"));
        }
        let len = d.len() as i64;
        let pos = if i < 0 { (i + len).max(0) } else { i.min(len) };
        d.insert(pos as usize, a.args[2].clone());
        Ok(Value::None)
    })
}
fn dq_remove(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    for (i, x) in items(&n).iter().enumerate() {
        if vm.eq(x, &a.args[1])? {
            with(&n, |d, _| d.remove(i));
            return Ok(Value::None);
        }
    }
    let r = vm.repr(&a.args[1])?;
    Err(value_err(format!("{r} is not in deque")))
}
fn dq_reverse(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    with(&n, |d, _| {
        let v: Vec<Value> = d.drain(..).rev().collect();
        d.extend(v);
    });
    Ok(Value::None)
}
fn dq_rotate(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let k = match a.args.get(1) {
        Some(v) => to_int_arg(vm, v)?,
        None => 1,
    };
    with(&n, |d, _| {
        let len = d.len() as i64;
        if len == 0 {
            return;
        }
        let k = k.rem_euclid(len) as usize;
        d.rotate_right(k);
    });
    Ok(Value::None)
}
fn dq_len(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    Ok(Value::Int(with(&n, |d, _| d.len()) as i64))
}
fn dq_bool(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    Ok(Value::Bool(with(&n, |d, _| !d.is_empty())))
}
fn dq_iter(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    Ok(Value::Iter(new_ref(IterObj::List {
        items: items(&n),
        idx: 0,
    })))
}
fn dq_reversed(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let mut v = items(&n);
    v.reverse();
    Ok(Value::Iter(new_ref(IterObj::List { items: v, idx: 0 })))
}
fn dq_contains(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    for x in items(&n) {
        if x.is(&a.args[1]) || vm.eq(&x, &a.args[1])? {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}
fn dq_getitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    crate::builtins::seq_getitem(vm, &Value::Native(n), &a.args[1])
}
fn dq_setitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    crate::builtins::native_setitem(vm, &Value::Native(n), &a.args[1], a.args[2].clone())?;
    Ok(Value::None)
}
fn dq_delitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let i = to_int_arg(vm, &a.args[1])?;
    with(&n, |d, _| {
        let len = d.len() as i64;
        let j = if i < 0 { i + len } else { i };
        if j < 0 || j >= len {
            return Err(err("IndexError", "deque index out of range"));
        }
        d.remove(j as usize);
        Ok(Value::None)
    })
}
fn dq_repr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let s = crate::builtins::native_repr(vm, &Value::Native(n))?;
    if let Value::Instance(_) = &a.args[0] {
        let name = vm.type_name(&a.args[0]);
        return Ok(Value::string(s.replacen("deque", &name, 1)));
    }
    Ok(Value::string(s))
}
fn dq_eq(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let Ok(o) = deque_of(vm, &a.args[1]) else {
        return Ok(Value::NotImplemented);
    };
    let (x, y) = (items(&n), items(&o));
    Ok(Value::Bool(vm.eq(&Value::list(x), &Value::list(y))?))
}
fn dq_add(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    let Ok(o) = deque_of(vm, &a.args[1]) else {
        return Ok(Value::NotImplemented);
    };
    let mut x = items(&n);
    x.extend(items(&o));
    let m = with(&n, |_, m| m);
    Ok(new_deque(&n.class, x, m))
}
fn dq_iadd(vm: &mut Vm, a: Args) -> PyResult<Value> {
    dq_extend(vm, Args::new(vec![a.args[0].clone(), a.args[1].clone()]))?;
    Ok(a.args[0].clone())
}
fn dq_maxlen(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let n = deque_of(vm, &a.args[0])?;
    Ok(match with(&n, |_, m| m) {
        Some(m) => Value::Int(m as i64),
        None => Value::None,
    })
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("_collections");
    let cls = new_class("deque", vec![vm.t.object.clone()], Kind::Object, true);
    cls.dict
        .borrow_mut()
        .set_str("__module__", Value::str("collections"));
    let b = Builtin {
        name: "deque.__new__".into(),
        func: dq_new,
        data: Value::None,
        owner: Some("type"),
    };
    cls.dict.borrow_mut().set_str(
        "__new__",
        Value::StaticMethod(Rc::new(Value::Builtin(Rc::new(b)))),
    );
    for (name, f) in [
        ("__init__", dq_init as NativeFn),
        ("append", dq_append),
        ("appendleft", dq_appendleft),
        ("pop", dq_pop),
        ("popleft", dq_popleft),
        ("extend", dq_extend),
        ("extendleft", dq_extendleft),
        ("clear", dq_clear),
        ("copy", dq_copy),
        ("__copy__", dq_copy),
        ("count", dq_count),
        ("index", dq_index),
        ("insert", dq_insert),
        ("remove", dq_remove),
        ("reverse", dq_reverse),
        ("rotate", dq_rotate),
        ("__len__", dq_len),
        ("__bool__", dq_bool),
        ("__iter__", dq_iter),
        ("__reversed__", dq_reversed),
        ("__contains__", dq_contains),
        ("__getitem__", dq_getitem),
        ("__setitem__", dq_setitem),
        ("__delitem__", dq_delitem),
        ("__repr__", dq_repr),
        ("__eq__", dq_eq),
        ("__add__", dq_add),
        ("__iadd__", dq_iadd),
    ] {
        add_fn(&cls, name, f);
    }
    let getter = crate::builtins::native_fn("maxlen", dq_maxlen);
    cls.dict.borrow_mut().set_str(
        "maxlen",
        Value::Property(Rc::new(Property {
            fget: getter,
            fset: Value::None,
            fdel: Value::None,
            doc: Value::None,
        })),
    );
    cls.dict.borrow_mut().set_str("__hash__", Value::None);
    set_val(&m, "deque", Value::Class(cls));
    Value::Module(m)
}
