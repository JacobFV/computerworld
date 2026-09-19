//! `Array`, `Array.prototype` and array iterators.

use super::*;
use crate::conv::cmp_utf16;
use crate::value::*;
use crate::vm::Vm;
use std::cmp::Ordering;

fn array_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let proto = match &a.new_target {
        Some(nt) => {
            let ap = vm.intr.array_proto.clone();
            vm.proto_from_ctor(nt, &ap)?
        }
        None => vm.intr.array_proto.clone(),
    };
    let items = if a.args.len() == 1 {
        match a.arg(0) {
            Value::Num(n) => {
                if n < 0.0
                    || n.fract() != 0.0
                    || n > 4294967295.0
                    || n as usize > crate::props::MAX_DENSE
                {
                    return Err(vm.range_error("Invalid array length"));
                }
                vec![Value::Empty; n as usize]
            }
            v => vec![v],
        }
    } else {
        a.args.clone()
    };
    Ok(Value::Obj(vm.obj_with(Some(proto), Kind::Array(items))))
}

fn is_array(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(
        matches!(a.arg(0), Value::Obj(o) if o.is_array()),
    ))
}

fn from(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let src = a.arg(0);
    let map = a.arg(1);
    if !map.is_undefined() && !map.is_callable() {
        let d = vm.describe_for_error(&map);
        return Err(vm.type_error(format!("{d} is not a function")));
    }
    let this_arg = a.arg(2);
    let items = if src.is_nullish() {
        let d = if src.is_undefined() {
            "undefined"
        } else {
            "object null"
        };
        return Err(vm.type_error(format!(
            "{d} is not iterable (cannot read property Symbol(Symbol.iterator))"
        )));
    } else {
        let iter = vm.get(&src, &Key::Sym(vm.syms.iterator.clone()))?;
        if !iter.is_nullish() {
            vm.iterable_to_vec(&src)?
        } else {
            let n = vm.length_of(&src)?;
            let mut out = Vec::with_capacity(n.min(1 << 20));
            for i in 0..n {
                out.push(vm.get_index(&src, i)?);
            }
            out
        }
    };
    let items = if map.is_callable() {
        let mut out = Vec::with_capacity(items.len());
        for (i, v) in items.into_iter().enumerate() {
            out.push(vm.call(&map, this_arg.clone(), vec![v, Value::Num(i as f64)])?);
        }
        out
    } else {
        items
    };
    // Subclasses: construct via `this`.
    if let Value::Obj(c) = &a.this {
        if !c.ptr_eq(&vm.intr.array_ctor) && vm.is_constructor(&a.this) {
            let o = vm.construct(&a.this, vec![], None)?;
            if let Value::Obj(oo) = &o {
                for (i, v) in items.into_iter().enumerate() {
                    vm.create_data_property(oo, Vm::index_key(i), v)?;
                }
            }
            return Ok(o);
        }
    }
    Ok(vm.arr(items))
}

fn of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(vm.arr(a.args.clone()))
}

/// Snapshot of elements for read-only algorithms (holes as Empty).
fn elements(vm: &mut Vm, this: &Value) -> JsResult<(Obj, Vec<Value>)> {
    let o = vm.to_object(this)?;
    if let Kind::Array(v) = &o.borrow().kind {
        return Ok((o.clone(), v.clone()));
    }
    let ov = Value::Obj(o.clone());
    let n = vm.length_of(&ov)?;
    let mut out = Vec::with_capacity(n.min(1 << 20));
    for i in 0..n {
        let k = Vm::index_key(i);
        if vm.has_property(&o, &k)? {
            out.push(vm.get(&ov, &k)?);
        } else {
            out.push(Value::Empty);
        }
    }
    Ok((o, out))
}

fn len_of(vm: &mut Vm, o: &Obj) -> JsResult<usize> {
    vm.length_of(&Value::Obj(o.clone()))
}

/// Element i, reading live (callbacks may mutate the array).
fn live_get(vm: &mut Vm, o: &Obj, i: usize) -> JsResult<Option<Value>> {
    if let Kind::Array(v) = &o.borrow().kind {
        return Ok(match v.get(i) {
            Some(Value::Empty) | None => None,
            Some(x) => Some(x.clone()),
        });
    }
    let k = Vm::index_key(i);
    if vm.has_property(o, &k)? {
        Ok(Some(vm.get(&Value::Obj(o.clone()), &k)?))
    } else {
        Ok(None)
    }
}

fn for_each(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let ov = Value::Obj(o.clone());
    for i in 0..len {
        if let Some(v) = live_get(vm, &o, i)? {
            vm.call(&f, t.clone(), vec![v, Value::Num(i as f64), ov.clone()])?;
        }
    }
    Ok(Value::Undefined)
}

fn map(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let ov = Value::Obj(o.clone());
    let mut out = vec![Value::Empty; len];
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(v) = live_get(vm, &o, i)? {
            *slot = vm.call(&f, t.clone(), vec![v, Value::Num(i as f64), ov.clone()])?;
        }
    }
    Ok(vm.arr(out))
}

fn filter(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let ov = Value::Obj(o.clone());
    let mut out = vec![];
    for i in 0..len {
        if let Some(v) = live_get(vm, &o, i)? {
            if vm
                .call(
                    &f,
                    t.clone(),
                    vec![v.clone(), Value::Num(i as f64), ov.clone()],
                )?
                .truthy()
            {
                out.push(v);
            }
        }
    }
    Ok(vm.arr(out))
}

fn some_every(vm: &mut Vm, a: &mut Args, every: bool) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let ov = Value::Obj(o.clone());
    for i in 0..len {
        if let Some(v) = live_get(vm, &o, i)? {
            let r = vm
                .call(&f, t.clone(), vec![v, Value::Num(i as f64), ov.clone()])?
                .truthy();
            if every && !r {
                return Ok(Value::Bool(false));
            }
            if !every && r {
                return Ok(Value::Bool(true));
            }
        }
    }
    Ok(Value::Bool(every))
}

fn some(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    some_every(vm, a, false)
}
fn every(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    some_every(vm, a, true)
}

/// kind: 0 find, 1 findIndex, 2 findLast, 3 findLastIndex
fn find_impl(vm: &mut Vm, a: &mut Args, kind: u8) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let ov = Value::Obj(o.clone());
    let idx: Box<dyn Iterator<Item = usize>> = if kind >= 2 {
        Box::new((0..len).rev())
    } else {
        Box::new(0..len)
    };
    for i in idx {
        let v = vm.get_index(&ov, i)?;
        if vm
            .call(
                &f,
                t.clone(),
                vec![v.clone(), Value::Num(i as f64), ov.clone()],
            )?
            .truthy()
        {
            return Ok(if kind.is_multiple_of(2) {
                v
            } else {
                Value::Num(i as f64)
            });
        }
    }
    Ok(if kind.is_multiple_of(2) {
        Value::Undefined
    } else {
        Value::Num(-1.0)
    })
}

fn find(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    find_impl(vm, a, 0)
}
fn find_index(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    find_impl(vm, a, 1)
}
fn find_last(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    find_impl(vm, a, 2)
}
fn find_last_index(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    find_impl(vm, a, 3)
}

fn reduce_impl(vm: &mut Vm, a: &mut Args, right: bool) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let ov = Value::Obj(o.clone());
    let order: Vec<usize> = if right {
        (0..len).rev().collect()
    } else {
        (0..len).collect()
    };
    let mut it = order.into_iter();
    let mut acc = if a.args.len() >= 2 {
        a.arg(1)
    } else {
        loop {
            match it.next() {
                Some(i) => {
                    if let Some(v) = live_get(vm, &o, i)? {
                        break v;
                    }
                }
                None => return Err(vm.type_error("Reduce of empty array with no initial value")),
            }
        }
    };
    for i in it {
        if let Some(v) = live_get(vm, &o, i)? {
            acc = vm.call(
                &f,
                Value::Undefined,
                vec![acc, v, Value::Num(i as f64), ov.clone()],
            )?;
        }
    }
    Ok(acc)
}

fn reduce(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    reduce_impl(vm, a, false)
}
fn reduce_right(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    reduce_impl(vm, a, true)
}

fn push(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    {
        let mut d = o.borrow_mut();
        let frozen = d.elems_frozen || !d.extensible;
        if let Kind::Array(v) = &mut d.kind {
            if !frozen {
                v.extend(a.args.iter().cloned());
                return Ok(Value::Num(v.len() as f64));
            }
        }
    }
    if o.is_array() {
        let n = len_of(vm, &o)?;
        return Err(vm.type_error(format!("Cannot add property {n}, object is not extensible")));
    }
    let ov = Value::Obj(o.clone());
    let mut n = len_of(vm, &o)?;
    for x in a.args.iter() {
        vm.set(&ov, Vm::index_key(n), x.clone())?;
        n += 1;
    }
    vm.set_str(&ov, "length", Value::Num(n as f64))?;
    Ok(Value::Num(n as f64))
}

fn pop(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    {
        let mut d = o.borrow_mut();
        let frozen = d.elems_frozen || d.elems_sealed;
        if let Kind::Array(v) = &mut d.kind {
            if !frozen {
                return Ok(match v.pop() {
                    Some(Value::Empty) => Value::Undefined,
                    Some(x) => x,
                    None => Value::Undefined,
                });
            }
        }
    }
    let ov = Value::Obj(o.clone());
    let n = len_of(vm, &o)?;
    if n == 0 {
        vm.set_str(&ov, "length", Value::Num(0.0))?;
        return Ok(Value::Undefined);
    }
    let v = vm.get_index(&ov, n - 1)?;
    vm.delete(&o, &Vm::index_key(n - 1))?;
    vm.set_str(&ov, "length", Value::Num((n - 1) as f64))?;
    Ok(v)
}

fn shift(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    {
        let mut d = o.borrow_mut();
        let frozen = d.elems_frozen || d.elems_sealed;
        if let Kind::Array(v) = &mut d.kind {
            if !frozen {
                if v.is_empty() {
                    return Ok(Value::Undefined);
                }
                return Ok(match v.remove(0) {
                    Value::Empty => Value::Undefined,
                    x => x,
                });
            }
        }
    }
    let (o, mut els) = elements(vm, &Value::Obj(o))?;
    if els.is_empty() {
        return Ok(Value::Undefined);
    }
    let first = els.remove(0);
    rewrite(vm, &o, els)?;
    Ok(if let Value::Empty = first {
        Value::Undefined
    } else {
        first
    })
}

fn rewrite(vm: &mut Vm, o: &Obj, els: Vec<Value>) -> JsResult<()> {
    let ov = Value::Obj(o.clone());
    let old = len_of(vm, o)?;
    for (i, v) in els.iter().enumerate() {
        match v {
            Value::Empty => {
                vm.delete(o, &Vm::index_key(i))?;
            }
            v => {
                vm.set(&ov, Vm::index_key(i), v.clone())?;
            }
        }
    }
    for i in els.len()..old {
        vm.delete(o, &Vm::index_key(i))?;
    }
    vm.set_str(&ov, "length", Value::Num(els.len() as f64))?;
    Ok(())
}

fn unshift(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    {
        let mut d = o.borrow_mut();
        let frozen = d.elems_frozen || !d.extensible;
        if let Kind::Array(v) = &mut d.kind {
            if !frozen {
                let mut nv = a.args.clone();
                nv.append(v);
                *v = nv;
                return Ok(Value::Num(v.len() as f64));
            }
        }
    }
    let (o, els) = elements(vm, &Value::Obj(o))?;
    let mut nv = a.args.clone();
    nv.extend(els);
    let n = nv.len();
    rewrite(vm, &o, nv)?;
    Ok(Value::Num(n as f64))
}

fn slice(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let s = vm.rel_index(&a.arg(0), len, 0)?;
    let e = vm.rel_index(&a.arg(1), len, len)?;
    if let Kind::Array(v) = &o.borrow().kind {
        let out: Vec<Value> = if s < e {
            v[s.min(v.len())..e.min(v.len())].to_vec()
        } else {
            vec![]
        };
        return Ok(Value::Obj(vm.new_array(out)));
    }
    let ov = Value::Obj(o.clone());
    let mut out = vec![];
    for i in s..e.max(s) {
        let k = Vm::index_key(i);
        if vm.has_property(&o, &k)? {
            out.push(vm.get(&ov, &k)?);
        } else {
            out.push(Value::Empty);
        }
    }
    Ok(vm.arr(out))
}

fn splice(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, mut els) = elements(vm, &a.this)?;
    let len = els.len();
    let start = vm.rel_index(&a.arg(0), len, 0)?;
    let del = if a.args.is_empty() {
        0
    } else if a.args.len() == 1 {
        len - start
    } else {
        let d = vm.to_integer(&a.arg(1))?;
        (d.max(0.0) as usize).min(len - start)
    };
    let items: Vec<Value> = a.args.iter().skip(2).cloned().collect();
    let removed: Vec<Value> = els.splice(start..start + del, items).collect();
    let is_arr = o.is_array();
    let frozen = o.borrow().elems_frozen;
    if frozen {
        return Err(vm.type_error("Cannot add/remove sealed array elements"));
    }
    if is_arr {
        if let Kind::Array(v) = &mut o.borrow_mut().kind {
            *v = els;
        }
    } else {
        rewrite(vm, &o, els)?;
    }
    Ok(vm.arr(removed))
}

fn to_spliced(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, mut els) = elements(vm, &a.this)?;
    let len = els.len();
    let start = vm.rel_index(&a.arg(0), len, 0)?;
    let del = if a.args.is_empty() {
        0
    } else if a.args.len() == 1 {
        len - start
    } else {
        let d = vm.to_integer(&a.arg(1))?;
        (d.max(0.0) as usize).min(len - start)
    };
    let items: Vec<Value> = a.args.iter().skip(2).cloned().collect();
    els.splice(start..start + del, items);
    let els = els
        .into_iter()
        .map(|x| {
            if let Value::Empty = x {
                Value::Undefined
            } else {
                x
            }
        })
        .collect();
    Ok(vm.arr(els))
}

fn concat(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let this = Value::Obj(vm.to_object(&a.this)?);
    let mut out = vec![];
    let mut all = vec![this];
    all.extend(a.args.iter().cloned());
    for item in all {
        let spreadable = match &item {
            Value::Obj(o) => {
                let s = vm.get(&item, &Key::Sym(vm.syms.is_concat_spreadable.clone()))?;
                if s.is_undefined() {
                    o.is_array()
                } else {
                    s.truthy()
                }
            }
            _ => false,
        };
        if spreadable {
            let Value::Obj(o) = &item else { unreachable!() };
            if let Kind::Array(v) = &o.borrow().kind {
                out.extend(v.iter().cloned());
                continue;
            }
            let n = vm.length_of(&item)?;
            for i in 0..n {
                let k = Vm::index_key(i);
                if vm.has_property(o, &k)? {
                    out.push(vm.get(&item, &k)?);
                } else {
                    out.push(Value::Empty);
                }
            }
        } else {
            out.push(item);
        }
    }
    Ok(vm.arr(out))
}

pub fn join_values(vm: &mut Vm, o: &Obj, sep: &str) -> JsResult<String> {
    if vm.inspect_seen.contains(&o.addr()) {
        return Ok(String::new());
    }
    vm.inspect_seen.push(o.addr());
    let r = (|| {
        let ov = Value::Obj(o.clone());
        let n = vm.length_of(&ov)?;
        let mut s = String::new();
        for i in 0..n {
            if i > 0 {
                s.push_str(sep);
            }
            let v = vm.get_index(&ov, i)?;
            if !v.is_nullish() {
                s.push_str(&vm.to_string(&v)?);
            }
        }
        Ok(s)
    })();
    vm.inspect_seen.pop();
    r
}

fn join(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let sep = if a.arg(0).is_undefined() {
        ",".to_string()
    } else {
        vm.to_str(&a.arg(0))?
    };
    Ok(Value::string(join_values(vm, &o, &sep)?))
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let j = vm.get_str(&Value::Obj(o.clone()), "join")?;
    if j.is_callable() {
        return vm.call(&j, Value::Obj(o), vec![]);
    }
    crate::builtins::object::object_to_string(vm, a)
}

fn to_locale_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let ov = Value::Obj(o);
    let n = vm.length_of(&ov)?;
    let mut parts = vec![];
    for i in 0..n {
        let v = vm.get_index(&ov, i)?;
        if v.is_nullish() {
            parts.push(String::new());
        } else {
            let f = vm.get_str(&v, "toLocaleString")?;
            // The locales and options reach every element.
            let r = vm.call(&f, v, vec![a.arg(0), a.arg(1)])?;
            parts.push(vm.to_str(&r)?);
        }
    }
    Ok(Value::string(parts.join(",")))
}

fn index_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, els) = elements(vm, &a.this)?;
    let len = els.len();
    let start = vm.rel_index(&a.arg(1), len, 0)?;
    let t = a.arg(0);
    for (i, x) in els.iter().enumerate().skip(start) {
        if strict_equals(x, &t) {
            return Ok(Value::Num(i as f64));
        }
    }
    Ok(Value::Num(-1.0))
}

fn last_index_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, els) = elements(vm, &a.this)?;
    let len = els.len();
    if len == 0 {
        return Ok(Value::Num(-1.0));
    }
    let start = if a.args.len() > 1 {
        let n = vm.to_integer(&a.arg(1))?;
        if n < 0.0 {
            let v = len as f64 + n;
            if v < 0.0 {
                return Ok(Value::Num(-1.0));
            }
            v as usize
        } else {
            (n as usize).min(len - 1)
        }
    } else {
        len - 1
    };
    let t = a.arg(0);
    for i in (0..=start).rev() {
        if strict_equals(&els[i], &t) {
            return Ok(Value::Num(i as f64));
        }
    }
    Ok(Value::Num(-1.0))
}

fn includes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, els) = elements(vm, &a.this)?;
    let len = els.len();
    let start = vm.rel_index(&a.arg(1), len, 0)?;
    let t = a.arg(0);
    for x in els.iter().skip(start) {
        let x = if let Value::Empty = x {
            &Value::Undefined
        } else {
            x
        };
        if same_value_zero(x, &t) {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

fn reverse(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    if o.borrow().elems_frozen {
        return Err(
            vm.type_error("Cannot assign to read only property '0' of object '[object Array]'")
        );
    }
    {
        let mut d = o.borrow_mut();
        if let Kind::Array(v) = &mut d.kind {
            v.reverse();
            return Ok(a.this.clone());
        }
    }
    let (o, mut els) = elements(vm, &Value::Obj(o))?;
    els.reverse();
    rewrite(vm, &o, els)?;
    Ok(Value::Obj(o))
}

fn to_reversed(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, mut els) = elements(vm, &a.this)?;
    els.reverse();
    let els = els
        .into_iter()
        .map(|x| {
            if let Value::Empty = x {
                Value::Undefined
            } else {
                x
            }
        })
        .collect();
    Ok(vm.arr(els))
}

pub fn sort_values(vm: &mut Vm, v: &mut Vec<Value>, cmp: &Value) -> JsResult<()> {
    // Holes and undefined go last.
    let mut vals: Vec<Value> = vec![];
    let mut undefs = 0;
    let mut holes = 0;
    for x in v.drain(..) {
        match x {
            Value::Empty => holes += 1,
            Value::Undefined => undefs += 1,
            x => vals.push(x),
        }
    }
    let sorted = if cmp.is_undefined() {
        // Pre-compute string keys.
        let mut keyed: Vec<(String, Value)> = Vec::with_capacity(vals.len());
        for x in vals {
            let k = vm.to_str(&x)?;
            keyed.push((k, x));
        }
        keyed.sort_by(|a, b| cmp_utf16(&a.0, &b.0));
        keyed.into_iter().map(|(_, x)| x).collect()
    } else {
        merge_sort(vm, vals, cmp)?
    };
    v.extend(sorted);
    v.extend(std::iter::repeat_n(Value::Undefined, undefs));
    v.extend(std::iter::repeat_n(Value::Empty, holes));
    Ok(())
}

fn compare_with(vm: &mut Vm, cmp: &Value, a: &Value, b: &Value) -> JsResult<Ordering> {
    let r = vm.call(cmp, Value::Undefined, vec![a.clone(), b.clone()])?;
    let n = match r {
        Value::Num(n) => n,
        other => vm.to_number(&other)?,
    };
    Ok(if n < 0.0 {
        Ordering::Less
    } else if n > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Equal
    })
}

fn merge_sort(vm: &mut Vm, v: Vec<Value>, cmp: &Value) -> JsResult<Vec<Value>> {
    let n = v.len();
    if n <= 1 {
        return Ok(v);
    }
    // Insertion sort for small runs, then bottom-up merges.
    let mut a = v;
    let run = 8;
    let mut i = 0;
    while i < n {
        let end = (i + run).min(n);
        for j in i + 1..end {
            let mut k = j;
            while k > i {
                if compare_with(vm, cmp, &a[k - 1], &a[k])? == Ordering::Greater {
                    a.swap(k - 1, k);
                    k -= 1;
                } else {
                    break;
                }
            }
        }
        i = end;
    }
    let mut width = run;
    let mut buf: Vec<Value> = Vec::with_capacity(n);
    while width < n {
        buf.clear();
        let mut lo = 0;
        while lo < n {
            let mid = (lo + width).min(n);
            let hi = (lo + 2 * width).min(n);
            let (mut l, mut r) = (lo, mid);
            while l < mid && r < hi {
                if compare_with(vm, cmp, &a[r], &a[l])? == Ordering::Less {
                    buf.push(a[r].clone());
                    r += 1;
                } else {
                    buf.push(a[l].clone());
                    l += 1;
                }
            }
            buf.extend_from_slice(&a[l..mid]);
            buf.extend_from_slice(&a[r..hi]);
            lo = hi;
        }
        std::mem::swap(&mut a, &mut buf);
        width *= 2;
    }
    Ok(a)
}

fn sort(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let cmp = a.arg(0);
    if !cmp.is_undefined() && !cmp.is_callable() {
        return Err(vm.type_error("The comparison function must be either a function or undefined"));
    }
    let (o, mut els) = elements(vm, &a.this)?;
    sort_values(vm, &mut els, &cmp)?;
    if o.is_array() {
        if o.borrow().elems_frozen {
            return Err(
                vm.type_error("Cannot assign to read only property '0' of object '[object Array]'")
            );
        }
        if let Kind::Array(v) = &mut o.borrow_mut().kind {
            *v = els;
        }
    } else {
        rewrite(vm, &o, els)?;
    }
    Ok(Value::Obj(o))
}

fn to_sorted(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let cmp = a.arg(0);
    if !cmp.is_undefined() && !cmp.is_callable() {
        return Err(vm.type_error("The comparison function must be either a function or undefined"));
    }
    let (_, els) = elements(vm, &a.this)?;
    let mut els: Vec<Value> = els
        .into_iter()
        .map(|x| {
            if let Value::Empty = x {
                Value::Undefined
            } else {
                x
            }
        })
        .collect();
    sort_values(vm, &mut els, &cmp)?;
    Ok(vm.arr(els))
}

fn fill(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let s = vm.rel_index(&a.arg(1), len, 0)?;
    let e = vm.rel_index(&a.arg(2), len, len)?;
    let v = a.arg(0);
    if o.borrow().elems_frozen {
        return Err(
            vm.type_error("Cannot assign to read only property '0' of object '[object Array]'")
        );
    }
    if let Kind::Array(arr) = &mut o.borrow_mut().kind {
        for x in arr.iter_mut().take(e).skip(s) {
            *x = v.clone();
        }
        return Ok(a.this.clone());
    }
    let ov = Value::Obj(o.clone());
    for i in s..e {
        vm.set(&ov, Vm::index_key(i), v.clone())?;
    }
    Ok(ov)
}

fn copy_within(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (o, mut els) = elements(vm, &a.this)?;
    let len = els.len();
    let target = vm.rel_index(&a.arg(0), len, 0)?;
    let s = vm.rel_index(&a.arg(1), len, 0)?;
    let e = vm.rel_index(&a.arg(2), len, len)?;
    let count = e.saturating_sub(s).min(len - target);
    let chunk: Vec<Value> = els[s..s + count].to_vec();
    for (i, v) in chunk.into_iter().enumerate() {
        els[target + i] = v;
    }
    if o.is_array() {
        if let Kind::Array(v) = &mut o.borrow_mut().kind {
            *v = els;
        }
    } else {
        rewrite(vm, &o, els)?;
    }
    Ok(Value::Obj(o))
}

fn flatten(vm: &mut Vm, out: &mut Vec<Value>, v: &Value, depth: f64) -> JsResult<()> {
    let n = vm.length_of(v)?;
    for i in 0..n {
        let Value::Obj(o) = v else { break };
        let k = Vm::index_key(i);
        if !vm.has_property(o, &k)? {
            continue;
        }
        let x = vm.get(v, &k)?;
        if depth > 0.0 && matches!(&x, Value::Obj(xo) if xo.is_array()) {
            flatten(vm, out, &x, depth - 1.0)?;
        } else {
            out.push(x);
        }
    }
    Ok(())
}

fn flat(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = Value::Obj(vm.to_object(&a.this)?);
    let depth = if a.arg(0).is_undefined() {
        1.0
    } else {
        vm.to_integer(&a.arg(0))?
    };
    let mut out = vec![];
    flatten(vm, &mut out, &o, depth)?;
    Ok(vm.arr(out))
}

fn flat_map(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let len = len_of(vm, &o)?;
    let f = callback(vm, &a.arg(0))?;
    let t = a.arg(1);
    let ov = Value::Obj(o.clone());
    let mut out = vec![];
    for i in 0..len {
        if let Some(v) = live_get(vm, &o, i)? {
            let r = vm.call(&f, t.clone(), vec![v, Value::Num(i as f64), ov.clone()])?;
            if matches!(&r, Value::Obj(ro) if ro.is_array()) {
                flatten(vm, &mut out, &r, 0.0)?;
            } else {
                out.push(r);
            }
        }
    }
    Ok(vm.arr(out))
}

fn at(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = Value::Obj(vm.to_object(&a.this)?);
    let len = vm.length_of(&o)? as f64;
    let i = vm.to_integer(&a.arg(0))?;
    let k = if i < 0.0 { len + i } else { i };
    if k < 0.0 || k >= len {
        return Ok(Value::Undefined);
    }
    vm.get_index(&o, k as usize)
}

fn with(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (_, els) = elements(vm, &a.this)?;
    let len = els.len() as f64;
    let i = vm.to_integer(&a.arg(0))?;
    let k = if i < 0.0 { len + i } else { i };
    if k < 0.0 || k >= len {
        return Err(
            vm.range_error("Invalid index : ".to_string() + &crate::numconv::number_to_string(i))
        );
    }
    let mut els: Vec<Value> = els
        .into_iter()
        .map(|x| {
            if let Value::Empty = x {
                Value::Undefined
            } else {
                x
            }
        })
        .collect();
    els[k as usize] = a.arg(1);
    Ok(vm.arr(els))
}

pub fn make_array_iter(vm: &mut Vm, target: Value, kind: IterKind) -> Value {
    Value::Obj(vm.obj_with(
        Some(vm.intr.array_iter_proto.clone()),
        Kind::ArrayIter {
            target,
            index: 0,
            kind,
            done: false,
        },
    ))
}

fn keys(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    Ok(make_array_iter(vm, Value::Obj(o), IterKind::Keys))
}
fn values(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    Ok(make_array_iter(vm, Value::Obj(o), IterKind::Values))
}
fn entries(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    Ok(make_array_iter(vm, Value::Obj(o), IterKind::Entries))
}

fn array_iter_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(io) = a.this.clone() else {
        return Err(vm.type_error("next method called on incompatible receiver"));
    };
    match vm.array_iter_fast(&io)? {
        Some(Some(v)) => Ok(vm.iter_result(v, false)),
        Some(None) => Ok(vm.iter_result(Value::Undefined, true)),
        None => {
            let d = vm.describe_for_error(&a.this);
            Err(vm.type_error(format!(
                "Method Array Iterator.prototype.next called on incompatible receiver {d}"
            )))
        }
    }
}

fn species_getter(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(a.this.clone())
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.array_proto.clone();
    let ctor = vm.make_ctor("Array", 1, array_ctor, &proto);
    vm.intr.array_ctor = ctor.clone();
    vm.set_global("Array", Value::Obj(ctor.clone()));
    vm.method(&ctor, "isArray", 1, is_array);
    vm.method(&ctor, "from", 1, from);
    vm.method(&ctor, "of", 0, of);
    let sp = vm.native_fn("get [Symbol.species]", 0, species_getter);
    ctor.borrow_mut().props.insert(
        Key::Sym(vm.syms.species.clone()),
        Prop {
            slot: Slot::Accessor(Some(sp), None),
            flags: CONFIGURABLE,
        },
    );
    let m: &[(&str, u32, NativeFn)] = &[
        ("at", 1, at),
        ("concat", 1, concat),
        ("copyWithin", 2, copy_within),
        ("entries", 0, entries),
        ("every", 1, every),
        ("fill", 1, fill),
        ("filter", 1, filter),
        ("find", 1, find),
        ("findIndex", 1, find_index),
        ("findLast", 1, find_last),
        ("findLastIndex", 1, find_last_index),
        ("flat", 0, flat),
        ("flatMap", 1, flat_map),
        ("forEach", 1, for_each),
        ("includes", 1, includes),
        ("indexOf", 1, index_of),
        ("join", 1, join),
        ("keys", 0, keys),
        ("lastIndexOf", 1, last_index_of),
        ("map", 1, map),
        ("pop", 0, pop),
        ("push", 1, push),
        ("reduce", 1, reduce),
        ("reduceRight", 1, reduce_right),
        ("reverse", 0, reverse),
        ("shift", 0, shift),
        ("slice", 2, slice),
        ("some", 1, some),
        ("sort", 1, sort),
        ("splice", 2, splice),
        ("toLocaleString", 0, to_locale_string),
        ("toReversed", 0, to_reversed),
        ("toSorted", 1, to_sorted),
        ("toSpliced", 2, to_spliced),
        ("toString", 0, to_string),
        ("unshift", 1, unshift),
        ("with", 2, with),
    ];
    for (n, l, f) in m {
        vm.method(&proto, n, *l, *f);
    }
    let values_fn = vm.method(&proto, "values", 0, values);
    let it = vm.syms.iterator.clone();
    proto.set_sym(&it, Value::Obj(values_fn.clone()), HIDDEN);
    vm.intr.array_values = values_fn;
    let aip = vm.intr.array_iter_proto.clone();
    let next = vm.method(&aip, "next", 0, array_iter_next);
    vm.intr.array_iter_next = next;
    let tag = vm.syms.to_string_tag.clone();
    aip.set_sym(&tag, Value::str("Array Iterator"), CONFIGURABLE);
}
