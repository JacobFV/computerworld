//! `Map`, `Set`, `WeakMap`, `WeakSet`, `WeakRef`, `FinalizationRegistry`.

use super::*;
use crate::value::*;
use crate::vm::Vm;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Map,
    Set,
    WeakMap,
    WeakSet,
}

fn which_name(w: Which) -> &'static str {
    match w {
        Which::Map => "Map",
        Which::Set => "Set",
        Which::WeakMap => "WeakMap",
        Which::WeakSet => "WeakSet",
    }
}

fn check(vm: &mut Vm, this: &Value, w: Which, method: &str) -> JsResult<Obj> {
    if let Value::Obj(o) = this {
        let ok = matches!(
            (&o.borrow().kind, w),
            (Kind::Map(_), Which::Map)
                | (Kind::Set(_), Which::Set)
                | (Kind::WeakMap(_), Which::WeakMap)
                | (Kind::WeakSet(_), Which::WeakSet)
        );
        if ok {
            return Ok(o.clone());
        }
    }
    let d = vm.describe_for_error(this);
    Err(vm.type_error(format!(
        "Method {}.prototype.{method} called on incompatible receiver {d}",
        which_name(w)
    )))
}

fn with_data<R>(o: &Obj, f: impl FnOnce(&mut MapData) -> R) -> R {
    let mut d = o.borrow_mut();
    match &mut d.kind {
        Kind::Map(m) | Kind::Set(m) | Kind::WeakMap(m) | Kind::WeakSet(m) => f(m),
        _ => unreachable!(),
    }
}

fn ctor_common(vm: &mut Vm, a: &mut Args, w: Which) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error(format!("Constructor {} requires 'new'", which_name(w))));
    };
    let default = match w {
        Which::Map => vm.intr.map_proto.clone(),
        Which::Set => vm.intr.set_proto.clone(),
        Which::WeakMap => vm.intr.weakmap_proto.clone(),
        Which::WeakSet => vm.intr.weakset_proto.clone(),
    };
    let proto = vm.proto_from_ctor(&nt, &default)?;
    let kind = match w {
        Which::Map => Kind::Map(Box::default()),
        Which::Set => Kind::Set(Box::default()),
        Which::WeakMap => Kind::WeakMap(Box::default()),
        Which::WeakSet => Kind::WeakSet(Box::default()),
    };
    let o = vm.obj_with(Some(proto), kind);
    let init = a.arg(0);
    if !init.is_nullish() {
        let items = vm.iterable_to_vec(&init)?;
        let ov = Value::Obj(o.clone());
        let adder_name = if matches!(w, Which::Map | Which::WeakMap) {
            "set"
        } else {
            "add"
        };
        let adder = vm.get_str(&ov, adder_name)?;
        for it in items {
            if matches!(w, Which::Map | Which::WeakMap) {
                if !matches!(it, Value::Obj(_)) {
                    let d = vm.describe_for_error(&it);
                    return Err(vm.type_error(format!("Iterator value {d} is not an entry object")));
                }
                let k = vm.get_index(&it, 0)?;
                let v = vm.get_index(&it, 1)?;
                vm.call(&adder, ov.clone(), vec![k, v])?;
            } else {
                vm.call(&adder, ov.clone(), vec![it])?;
            }
        }
    }
    Ok(Value::Obj(o))
}

fn map_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    ctor_common(vm, a, Which::Map)
}
fn set_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    ctor_common(vm, a, Which::Set)
}
fn weakmap_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    ctor_common(vm, a, Which::WeakMap)
}
fn weakset_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    ctor_common(vm, a, Which::WeakSet)
}

fn weak_key_check(vm: &mut Vm, k: &Value, what: &str) -> JsResult<()> {
    match k {
        Value::Obj(_) => Ok(()),
        Value::Sym(s) if !s.registered => Ok(()),
        _ => {
            let d = vm.describe_for_error(k);
            Err(vm.type_error(format!(
                "Invalid value used {what}: {d}",
                d = d,
                what = what
            )))
        }
    }
}

fn map_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "get")?;
    let k = a.arg(0);
    Ok(with_data(&o, |m| m.get(&k).cloned()).unwrap_or(Value::Undefined))
}
fn map_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "set")?;
    let (k, v) = (a.arg(0), a.arg(1));
    with_data(&o, |m| m.set(k, v));
    Ok(a.this.clone())
}
fn map_has(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "has")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.has(&k))))
}
fn map_delete(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "delete")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.delete(&k))))
}
fn map_clear(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "clear")?;
    with_data(&o, |m| m.clear());
    Ok(Value::Undefined)
}
fn map_size(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "size")?;
    Ok(Value::Num(with_data(&o, |m| m.live) as f64))
}

fn for_each_common(vm: &mut Vm, a: &mut Args, w: Which) -> JsResult<Value> {
    let o = check(vm, &a.this, w, "forEach")?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let mut i = 0;
    loop {
        let e = with_data(&o, |m| {
            while i < m.entries.len() {
                if let Some(e) = &m.entries[i] {
                    return Some(e.clone());
                }
                i += 1;
            }
            None
        });
        let Some((k, v)) = e else { break };
        i += 1;
        let (a1, a2) = if w == Which::Set {
            (k.clone(), k)
        } else {
            (v, k)
        };
        vm.call(&f, t.clone(), vec![a1, a2, a.this.clone()])?;
    }
    Ok(Value::Undefined)
}

fn map_for_each(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    for_each_common(vm, a, Which::Map)
}
fn set_for_each(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    for_each_common(vm, a, Which::Set)
}

fn make_iter(vm: &mut Vm, o: Obj, kind: IterKind, set: bool) -> Value {
    let proto = if set {
        vm.intr.set_iter_proto.clone()
    } else {
        vm.intr.map_iter_proto.clone()
    };
    Value::Obj(vm.obj_with(
        Some(proto),
        Kind::MapIter {
            target: o,
            index: 0,
            kind,
            done: false,
        },
    ))
}

fn map_keys(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "keys")?;
    Ok(make_iter(vm, o, IterKind::Keys, false))
}
fn map_values(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "values")?;
    Ok(make_iter(vm, o, IterKind::Values, false))
}
fn map_entries(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Map, "entries")?;
    Ok(make_iter(vm, o, IterKind::Entries, false))
}
fn set_values(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "values")?;
    Ok(make_iter(vm, o, IterKind::Values, true))
}
fn set_entries(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "entries")?;
    Ok(make_iter(vm, o, IterKind::Entries, true))
}

fn map_iter_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(it) = a.this.clone() else {
        return Err(vm.type_error("next method called on incompatible receiver"));
    };
    let (target, index, kind, done) = match &it.borrow().kind {
        Kind::MapIter {
            target,
            index,
            kind,
            done,
        } => (target.clone(), *index, *kind, *done),
        _ => return Err(vm.type_error("next method called on incompatible receiver")),
    };
    if done {
        return Ok(vm.iter_result(Value::Undefined, true));
    }
    let is_set = matches!(target.borrow().kind, Kind::Set(_));
    let mut i = index;
    let e = with_data(&target, |m| {
        while i < m.entries.len() {
            if let Some(e) = &m.entries[i] {
                return Some(e.clone());
            }
            i += 1;
        }
        None
    });
    let set_state = |idx: usize, d: bool| {
        if let Kind::MapIter { index, done, .. } = &mut it.borrow_mut().kind {
            *index = idx;
            *done = d;
        }
    };
    match e {
        None => {
            set_state(i, true);
            Ok(vm.iter_result(Value::Undefined, true))
        }
        Some((k, v)) => {
            set_state(i + 1, false);
            let v = if is_set { k.clone() } else { v };
            let r = match kind {
                IterKind::Keys => k,
                IterKind::Values => v,
                IterKind::Entries => vm.arr(vec![k, v]),
            };
            Ok(vm.iter_result(r, false))
        }
    }
}

fn set_add(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "add")?;
    let k = a.arg(0);
    with_data(&o, |m| {
        if !m.has(&k) {
            m.set(k, Value::Undefined)
        }
    });
    Ok(a.this.clone())
}
fn set_has(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "has")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.has(&k))))
}
fn set_delete(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "delete")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.delete(&k))))
}
fn set_clear(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "clear")?;
    with_data(&o, |m| m.clear());
    Ok(Value::Undefined)
}
fn set_size(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "size")?;
    Ok(Value::Num(with_data(&o, |m| m.live) as f64))
}

fn set_items(o: &Obj) -> Vec<Value> {
    with_data(o, |m| {
        m.entries.iter().flatten().map(|(k, _)| k.clone()).collect()
    })
}

/// Items of a set-like argument (has/keys protocol; Sets fast).
fn other_items(vm: &mut Vm, v: &Value) -> JsResult<(Vec<Value>, Value)> {
    if let Value::Obj(o) = v {
        let (is_set, is_map) = {
            let d = o.borrow();
            (
                matches!(d.kind, Kind::Set(_)),
                matches!(d.kind, Kind::Map(_)),
            )
        };
        if is_set {
            return Ok((set_items(o), v.clone()));
        }
        if is_map {
            let ks = with_data(o, |m| {
                m.entries.iter().flatten().map(|(k, _)| k.clone()).collect()
            });
            return Ok((ks, v.clone()));
        }
    }
    let keys = vm.get_str(v, "keys")?;
    if !keys.is_callable() {
        return Err(vm.type_error("The 'keys' property must be a function"));
    }
    let it = vm.call(&keys, v.clone(), vec![])?;
    let next = vm.get_str(&it, "next")?;
    let mut out = vec![];
    while let Some(x) = vm.iter_step(&it, &next)? {
        out.push(x);
    }
    Ok((out, v.clone()))
}

fn new_set(vm: &mut Vm, items: Vec<Value>) -> Value {
    let s = vm.obj_with(Some(vm.intr.set_proto.clone()), Kind::Set(Box::default()));
    with_data(&s, |m| {
        for it in items {
            if !m.has(&it) {
                m.set(it, Value::Undefined);
            }
        }
    });
    Value::Obj(s)
}

/// op: 0 union, 1 intersection, 2 difference, 3 symmetricDifference,
/// 4 isSubsetOf, 5 isSupersetOf, 6 isDisjointFrom
fn set_algebra(vm: &mut Vm, a: &mut Args, op: u8) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::Set, "union")?;
    let mine = set_items(&o);
    let (theirs, _) = other_items(vm, &a.arg(0))?;
    let has = |xs: &[Value], v: &Value| xs.iter().any(|x| same_value_zero(x, v));
    Ok(match op {
        0 => {
            let mut v = mine;
            v.extend(theirs);
            new_set(vm, v)
        }
        1 => {
            let v = mine.into_iter().filter(|x| has(&theirs, x)).collect();
            new_set(vm, v)
        }
        2 => {
            let v = mine.into_iter().filter(|x| !has(&theirs, x)).collect();
            new_set(vm, v)
        }
        3 => {
            let mut v: Vec<Value> = mine.iter().filter(|x| !has(&theirs, x)).cloned().collect();
            v.extend(theirs.iter().filter(|x| !has(&mine, x)).cloned());
            new_set(vm, v)
        }
        4 => Value::Bool(mine.iter().all(|x| has(&theirs, x))),
        5 => Value::Bool(theirs.iter().all(|x| has(&mine, x))),
        _ => Value::Bool(!mine.iter().any(|x| has(&theirs, x))),
    })
}

fn set_union(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 0)
}
fn set_intersection(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 1)
}
fn set_difference(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 2)
}
fn set_sym_diff(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 3)
}
fn set_is_subset(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 4)
}
fn set_is_superset(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 5)
}
fn set_is_disjoint(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    set_algebra(vm, a, 6)
}

fn weakmap_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakMap, "get")?;
    let k = a.arg(0);
    Ok(with_data(&o, |m| m.get(&k).cloned()).unwrap_or(Value::Undefined))
}
fn weakmap_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakMap, "set")?;
    let k = a.arg(0);
    weak_key_check(vm, &k, "as weak map key")?;
    let v = a.arg(1);
    with_data(&o, |m| m.set(k, v));
    Ok(a.this.clone())
}
fn weakmap_has(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakMap, "has")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.has(&k))))
}
fn weakmap_delete(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakMap, "delete")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.delete(&k))))
}
fn weakset_add(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakSet, "add")?;
    let k = a.arg(0);
    weak_key_check(vm, &k, "in weak set")?;
    with_data(&o, |m| m.set(k, Value::Undefined));
    Ok(a.this.clone())
}
fn weakset_has(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakSet, "has")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.has(&k))))
}
fn weakset_delete(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = check(vm, &a.this, Which::WeakSet, "delete")?;
    let k = a.arg(0);
    Ok(Value::Bool(with_data(&o, |m| m.delete(&k))))
}

fn weakref_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error("Constructor WeakRef requires 'new'"));
    };
    let t = a.arg(0);
    weak_key_check(vm, &t, "as WeakRef target")?;
    let wp = vm.intr.weakref_proto.clone();
    let proto = vm.proto_from_ctor(&nt, &wp)?;
    Ok(Value::Obj(vm.obj_with(Some(proto), Kind::WeakRef(t))))
}

fn weakref_deref(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        if let Kind::WeakRef(v) = &o.borrow().kind {
            return Ok(v.clone());
        }
    }
    Ok(Value::Undefined)
}

fn finreg_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Some(nt) = a.new_target.clone() else {
        return Err(vm.type_error("Constructor FinalizationRegistry requires 'new'"));
    };
    let op = vm.intr.object_proto.clone();
    let proto = vm.proto_from_ctor(&nt, &op)?;
    Ok(Value::Obj(vm.obj_with(Some(proto), Kind::Ordinary)))
}

fn noop(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Undefined)
}

fn map_group_by(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let items = vm.iterable_to_vec(&a.arg(0))?;
    let f = callback(vm, &a.arg(1))?;
    let m = vm.obj_with(Some(vm.intr.map_proto.clone()), Kind::Map(Box::default()));
    for (i, it) in items.into_iter().enumerate() {
        let k = vm.call(&f, Value::Undefined, vec![it.clone(), Value::Num(i as f64)])?;
        let existing = with_data(&m, |md| md.get(&k).cloned());
        match existing {
            Some(Value::Obj(arr)) => {
                if let Kind::Array(v) = &mut arr.borrow_mut().kind {
                    v.push(it);
                }
            }
            _ => {
                let arr = vm.arr(vec![it]);
                with_data(&m, |md| md.set(k, arr));
            }
        }
    }
    Ok(Value::Obj(m))
}

pub fn install(vm: &mut Vm) {
    let tag = vm.syms.to_string_tag.clone();
    let it = vm.syms.iterator.clone();
    // Map
    let mp = vm.intr.map_proto.clone();
    let mc = vm.make_ctor("Map", 0, map_ctor, &mp);
    vm.method(&mc, "groupBy", 2, map_group_by);
    vm.set_global("Map", Value::Obj(mc));
    for (n, l, f) in [
        ("get", 1, map_get as NativeFn),
        ("set", 2, map_set),
        ("has", 1, map_has),
        ("delete", 1, map_delete),
        ("clear", 0, map_clear),
        ("forEach", 1, map_for_each),
        ("keys", 0, map_keys),
        ("values", 0, map_values),
    ] {
        vm.method(&mp, n, l, f);
    }
    let me = vm.method(&mp, "entries", 0, map_entries);
    mp.set_sym(&it, Value::Obj(me), HIDDEN);
    vm.getter(&mp, "size", map_size);
    mp.set_sym(&tag, Value::str("Map"), CONFIGURABLE);
    let mip = vm.intr.map_iter_proto.clone();
    vm.method(&mip, "next", 0, map_iter_next);
    mip.set_sym(&tag, Value::str("Map Iterator"), CONFIGURABLE);
    // Set
    let sp = vm.intr.set_proto.clone();
    let sc = vm.make_ctor("Set", 0, set_ctor, &sp);
    vm.set_global("Set", Value::Obj(sc));
    for (n, l, f) in [
        ("add", 1, set_add as NativeFn),
        ("has", 1, set_has),
        ("delete", 1, set_delete),
        ("clear", 0, set_clear),
        ("forEach", 1, set_for_each),
        ("entries", 0, set_entries),
        ("union", 1, set_union),
        ("intersection", 1, set_intersection),
        ("difference", 1, set_difference),
        ("symmetricDifference", 1, set_sym_diff),
        ("isSubsetOf", 1, set_is_subset),
        ("isSupersetOf", 1, set_is_superset),
        ("isDisjointFrom", 1, set_is_disjoint),
    ] {
        vm.method(&sp, n, l, f);
    }
    let sv = vm.method(&sp, "values", 0, set_values);
    sp.set_hidden("keys", Value::Obj(sv.clone()));
    sp.set_sym(&it, Value::Obj(sv), HIDDEN);
    vm.getter(&sp, "size", set_size);
    sp.set_sym(&tag, Value::str("Set"), CONFIGURABLE);
    let sip = vm.intr.set_iter_proto.clone();
    vm.method(&sip, "next", 0, map_iter_next);
    sip.set_sym(&tag, Value::str("Set Iterator"), CONFIGURABLE);
    // WeakMap / WeakSet
    let wmp = vm.intr.weakmap_proto.clone();
    let wmc = vm.make_ctor("WeakMap", 0, weakmap_ctor, &wmp);
    vm.set_global("WeakMap", Value::Obj(wmc));
    vm.method(&wmp, "get", 1, weakmap_get);
    vm.method(&wmp, "set", 2, weakmap_set);
    vm.method(&wmp, "has", 1, weakmap_has);
    vm.method(&wmp, "delete", 1, weakmap_delete);
    wmp.set_sym(&tag, Value::str("WeakMap"), CONFIGURABLE);
    let wsp = vm.intr.weakset_proto.clone();
    let wsc = vm.make_ctor("WeakSet", 0, weakset_ctor, &wsp);
    vm.set_global("WeakSet", Value::Obj(wsc));
    vm.method(&wsp, "add", 1, weakset_add);
    vm.method(&wsp, "has", 1, weakset_has);
    vm.method(&wsp, "delete", 1, weakset_delete);
    wsp.set_sym(&tag, Value::str("WeakSet"), CONFIGURABLE);
    // WeakRef / FinalizationRegistry
    let wrp = vm.intr.weakref_proto.clone();
    let wrc = vm.make_ctor("WeakRef", 1, weakref_ctor, &wrp);
    vm.set_global("WeakRef", Value::Obj(wrc));
    vm.method(&wrp, "deref", 0, weakref_deref);
    wrp.set_sym(&tag, Value::str("WeakRef"), CONFIGURABLE);
    let frp = vm.new_object();
    let frc = vm.make_ctor("FinalizationRegistry", 1, finreg_ctor, &frp);
    vm.method(&frp, "register", 2, noop);
    vm.method(&frp, "unregister", 1, noop);
    frp.set_sym(&tag, Value::str("FinalizationRegistry"), CONFIGURABLE);
    vm.set_global("FinalizationRegistry", Value::Obj(frc));
}
