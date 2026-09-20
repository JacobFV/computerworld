//! `Object` and `Object.prototype`.

use super::*;
use crate::value::*;
use crate::vm::Vm;

fn object_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Some(nt) = &a.new_target {
        if !nt.ptr_eq(&a.callee) {
            let op = vm.intr.object_proto.clone();
            let proto = vm.proto_from_ctor(nt, &op)?;
            return Ok(Value::Obj(vm.obj_with(Some(proto), Kind::Ordinary)));
        }
    }
    let v = a.arg(0);
    if v.is_nullish() {
        return Ok(Value::Obj(vm.new_object()));
    }
    Ok(Value::Obj(vm.to_object(&v)?))
}

fn keys(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let ks = vm.own_enum_keys(&o)?;
    Ok(vm.arr(ks.into_iter().map(Value::Str).collect()))
}

fn values(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let ks = vm.own_enum_keys(&o)?;
    let ov = Value::Obj(o);
    let mut out = vec![];
    for k in ks {
        out.push(vm.get(&ov, &Key::Str(k))?);
    }
    Ok(vm.arr(out))
}

fn entries(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let ks = vm.own_enum_keys(&o)?;
    let ov = Value::Obj(o);
    let mut out = vec![];
    for k in ks {
        let v = vm.get(&ov, &Key::Str(k.clone()))?;
        out.push(vm.arr(vec![Value::Str(k), v]));
    }
    Ok(vm.arr(out))
}

fn assign(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = vm.to_object(&a.arg(0))?;
    let tv = Value::Obj(t.clone());
    for src in a.args.iter().skip(1) {
        if src.is_nullish() {
            continue;
        }
        let s = vm.to_object(src)?;
        let keys = vm.own_keys(&s)?;
        for k in keys {
            if let Some(p) = vm.get_own(&s, &k)? {
                if p.enumerable() {
                    let v = vm.get(&Value::Obj(s.clone()), &k)?;
                    if !vm.set(&tv, k.clone(), v)? {
                        let kd = vm.key_display(&k);
                        return Err(vm.type_error(format!(
                            "Cannot assign to read only property '{kd}' of object '#<Object>'"
                        )));
                    }
                }
            }
        }
    }
    Ok(tv)
}

pub fn freeze_obj(o: &Obj, level: u8) {
    // level 1 = seal, 2 = freeze
    let mut d = o.borrow_mut();
    d.extensible = false;
    if level == 2 {
        d.elems_frozen = true;
    }
    d.elems_sealed = true;
    for (_, p) in d.props.entries.iter_mut() {
        p.flags &= !CONFIGURABLE;
        if level == 2 {
            if let Slot::Data(_) = p.slot {
                p.flags &= !WRITABLE;
            }
        }
    }
}

fn freeze(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = a.arg(0) {
        freeze_obj(&o, 2);
    }
    Ok(a.arg(0))
}

fn seal(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = a.arg(0) {
        freeze_obj(&o, 1);
    }
    Ok(a.arg(0))
}

fn prevent_extensions(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = a.arg(0) {
        o.borrow_mut().extensible = false;
    }
    Ok(a.arg(0))
}

fn is_frozen(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(match a.arg(0) {
        Value::Obj(o) => {
            let d = o.borrow();
            let elems_ok = match &d.kind {
                Kind::Array(v) => v.is_empty() || d.elems_frozen,
                _ => true,
            };
            !d.extensible
                && elems_ok
                && d.props.entries.iter().all(|(_, p)| {
                    !p.configurable() && (matches!(p.slot, Slot::Accessor(..)) || !p.writable())
                })
        }
        _ => true,
    }))
}

fn is_sealed(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(match a.arg(0) {
        Value::Obj(o) => {
            let d = o.borrow();
            let elems_ok = match &d.kind {
                Kind::Array(v) => v.is_empty() || d.elems_sealed,
                _ => true,
            };
            !d.extensible && elems_ok && d.props.entries.iter().all(|(_, p)| !p.configurable())
        }
        _ => true,
    }))
}

fn is_extensible(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(match a.arg(0) {
        Value::Obj(o) => o.borrow().extensible,
        _ => false,
    }))
}

fn create(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let proto = match a.arg(0) {
        Value::Obj(p) => Some(p),
        Value::Null => None,
        other => {
            let d = vm.describe_for_error(&other);
            return Err(vm.type_error(format!(
                "Object prototype may only be an Object or null: {d}"
            )));
        }
    };
    let o = vm.obj_with(proto, Kind::Ordinary);
    let props = a.arg(1);
    if !props.is_undefined() {
        define_properties_on(vm, &o, &props)?;
    }
    Ok(Value::Obj(o))
}

fn get_prototype_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    Ok(o.proto().map(Value::Obj).unwrap_or(Value::Null))
}

fn set_prototype_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = a.arg(0);
    if v.is_nullish() {
        return Err(vm.type_error("Object.setPrototypeOf called on null or undefined"));
    }
    let p = match a.arg(1) {
        Value::Obj(p) => Some(p),
        Value::Null => None,
        other => {
            let d = vm.describe_for_error(&other);
            return Err(vm.type_error(format!(
                "Object prototype may only be an Object or null: {d}"
            )));
        }
    };
    if let Value::Obj(o) = &v {
        // Cycle check.
        let mut cur = p.clone();
        while let Some(c) = cur {
            if c.ptr_eq(o) {
                return Err(vm.type_error("Cyclic __proto__ value"));
            }
            cur = c.proto();
        }
        o.borrow_mut().proto = p;
    }
    Ok(v)
}

/// ToPropertyDescriptor + validation into a Prop, merged with `existing`.
pub fn to_prop(vm: &mut Vm, desc: &Value, existing: Option<Prop>) -> JsResult<Prop> {
    let Value::Obj(d) = desc else {
        let s = vm.describe_for_error(desc);
        return Err(vm.type_error(format!("Property description must be an object: {s}")));
    };
    let dv = Value::Obj(d.clone());
    let has = |vm: &mut Vm, k: &str| vm.has_property(d, &Key::str(k));
    let has_get = has(vm, "get")?;
    let has_set = has(vm, "set")?;
    let has_value = has(vm, "value")?;
    let has_writable = has(vm, "writable")?;
    if (has_get || has_set) && (has_value || has_writable) {
        return Err(vm.type_error("Invalid property descriptor. Cannot both specify accessors and a value or writable attribute"));
    }
    let mut flags = existing.as_ref().map(|p| p.flags).unwrap_or(0);
    if has(vm, "enumerable")? {
        let e = vm.get_str(&dv, "enumerable")?.truthy();
        flags = if e {
            flags | ENUMERABLE
        } else {
            flags & !ENUMERABLE
        };
    }
    if has(vm, "configurable")? {
        let c = vm.get_str(&dv, "configurable")?.truthy();
        flags = if c {
            flags | CONFIGURABLE
        } else {
            flags & !CONFIGURABLE
        };
    }
    if has_get || has_set {
        let (mut g, mut s) = match existing.map(|p| p.slot) {
            Some(Slot::Accessor(g, s)) => (g, s),
            _ => (None, None),
        };
        if has_get {
            let gv = vm.get_str(&dv, "get")?;
            g = match gv {
                Value::Undefined => None,
                Value::Obj(o) if o.is_callable() => Some(o),
                other => {
                    let s = vm.describe_for_error(&other);
                    return Err(vm.type_error(format!("Getter must be a function: {s}")));
                }
            };
        }
        if has_set {
            let sv = vm.get_str(&dv, "set")?;
            s = match sv {
                Value::Undefined => None,
                Value::Obj(o) if o.is_callable() => Some(o),
                other => {
                    let d = vm.describe_for_error(&other);
                    return Err(vm.type_error(format!("Setter must be a function: {d}")));
                }
            };
        }
        flags &= !WRITABLE;
        return Ok(Prop {
            slot: Slot::Accessor(g, s),
            flags,
        });
    }
    let mut value = match existing.map(|p| p.slot) {
        Some(Slot::Data(v)) => v,
        _ => Value::Undefined,
    };
    if has_value {
        value = vm.get_str(&dv, "value")?;
    }
    if has_writable {
        let w = vm.get_str(&dv, "writable")?.truthy();
        flags = if w {
            flags | WRITABLE
        } else {
            flags & !WRITABLE
        };
    }
    Ok(Prop::data(value, flags))
}

pub fn define_property_desc(vm: &mut Vm, o: &Obj, key: Key, desc: &Value) -> JsResult<()> {
    let existing = vm.get_own(o, &key)?;
    if let Some(e) = &existing {
        if !e.configurable() {
            // Allowed: writable data value change / writable -> false.
            let p = to_prop(vm, desc, existing.clone())?;
            let ok = match (&e.slot, &p.slot) {
                (Slot::Data(_), Slot::Data(_)) => {
                    e.writable()
                        && p.flags & (ENUMERABLE | CONFIGURABLE)
                            == e.flags & (ENUMERABLE | CONFIGURABLE)
                }
                _ => false,
            };
            let same = match (&e.slot, &p.slot) {
                (Slot::Data(a), Slot::Data(b)) => same_value(a, b) && e.flags == p.flags,
                _ => false,
            };
            if !ok && !same {
                let k = vm.key_display(&key);
                return Err(vm.type_error(format!("Cannot redefine property: {k}")));
            }
            vm.define_own(o, key, p)?;
            return Ok(());
        }
    }
    let p = to_prop(vm, desc, existing)?;
    if !vm.define_own(o, key.clone(), p)? {
        let k = vm.key_display(&key);
        return Err(vm.type_error(format!(
            "Cannot define property {k}, object is not extensible"
        )));
    }
    Ok(())
}

fn define_property(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Err(vm.type_error("Object.defineProperty called on non-object"));
    };
    let key = vm.to_key(&a.arg(1))?;
    define_property_desc(vm, &o, key, &a.arg(2))?;
    Ok(a.arg(0))
}

fn define_properties_on(vm: &mut Vm, o: &Obj, props: &Value) -> JsResult<()> {
    let p = vm.to_object(props)?;
    let keys = vm.own_keys(&p)?;
    for k in keys {
        if let Some(pr) = vm.get_own(&p, &k)? {
            if pr.enumerable() {
                let d = vm.get(&Value::Obj(p.clone()), &k)?;
                define_property_desc(vm, o, k, &d)?;
            }
        }
    }
    Ok(())
}

fn define_properties(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Err(vm.type_error("Object.defineProperties called on non-object"));
    };
    define_properties_on(vm, &o, &a.arg(1))?;
    Ok(a.arg(0))
}

pub fn from_prop(vm: &mut Vm, p: Prop) -> Value {
    let o = vm.new_object();
    let (writable, enumerable, configurable) = (p.writable(), p.enumerable(), p.configurable());
    match p.slot {
        Slot::Data(v) => {
            o.set_prop("value", v, ALL);
            o.set_prop("writable", Value::Bool(writable), ALL);
        }
        Slot::Accessor(g, s) => {
            o.set_prop("get", g.map(Value::Obj).unwrap_or(Value::Undefined), ALL);
            o.set_prop("set", s.map(Value::Obj).unwrap_or(Value::Undefined), ALL);
        }
    }
    o.set_prop("enumerable", Value::Bool(enumerable), ALL);
    o.set_prop("configurable", Value::Bool(configurable), ALL);
    Value::Obj(o)
}

fn get_own_property_descriptor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let k = vm.to_key(&a.arg(1))?;
    match vm.get_own(&o, &k)? {
        Some(p) => Ok(from_prop(vm, p)),
        None => Ok(Value::Undefined),
    }
}

fn get_own_property_descriptors(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let out = vm.new_object();
    for k in vm.own_keys(&o)? {
        if let Some(p) = vm.get_own(&o, &k)? {
            let d = from_prop(vm, p);
            out.borrow_mut().props.insert(k, Prop::data(d, ALL));
        }
    }
    Ok(Value::Obj(out))
}

fn get_own_property_names(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let ks = vm.own_keys(&o)?;
    let v: Vec<Value> = ks
        .into_iter()
        .filter_map(|k| match k {
            Key::Str(s) => Some(Value::Str(s)),
            _ => None,
        })
        .collect();
    Ok(vm.arr(v))
}

fn get_own_property_symbols(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let ks = vm.own_keys(&o)?;
    let v: Vec<Value> = ks
        .into_iter()
        .filter_map(|k| match k {
            Key::Sym(s) => Some(Value::Sym(s)),
            _ => None,
        })
        .collect();
    Ok(vm.arr(v))
}

fn from_entries(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let items = vm.iterable_to_vec(&a.arg(0))?;
    let o = vm.new_object();
    for it in items {
        if !matches!(it, Value::Obj(_)) {
            let d = vm.describe_for_error(&it);
            return Err(vm.type_error(format!("Iterator value {d} is not an entry object")));
        }
        let k = vm.get_index(&it, 0)?;
        let v = vm.get_index(&it, 1)?;
        let key = vm.to_key(&k)?;
        vm.create_data_property(&o, key, v)?;
    }
    Ok(Value::Obj(o))
}

fn is(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Bool(same_value(&a.arg(0), &a.arg(1))))
}

fn has_own(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.arg(0))?;
    let k = vm.to_key(&a.arg(1))?;
    Ok(Value::Bool(vm.has_own(&o, &k)?))
}

fn group_by(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let items = vm.iterable_to_vec(&a.arg(0))?;
    let f = callback(vm, &a.arg(1))?;
    let o = vm.obj_with(None, Kind::Ordinary);
    for (i, it) in items.into_iter().enumerate() {
        let k = vm.call(&f, Value::Undefined, vec![it.clone(), Value::Num(i as f64)])?;
        let key = vm.to_key(&k)?;
        let existing = o.borrow().props.get(&key).cloned();
        match existing {
            Some(Prop {
                slot: Slot::Data(Value::Obj(arr)),
                ..
            }) => {
                if let Kind::Array(v) = &mut arr.borrow_mut().kind {
                    v.push(it);
                }
            }
            _ => {
                let arr = vm.arr(vec![it]);
                o.borrow_mut().props.insert(key, Prop::data(arr, ALL));
            }
        }
    }
    Ok(Value::Obj(o))
}

// ---------------------------------------------------------------- prototype

fn has_own_property(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let k = vm.to_key(&a.arg(0))?;
    let o = vm.to_object(&a.this)?;
    Ok(Value::Bool(vm.has_own(&o, &k)?))
}

fn is_prototype_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(v) = a.arg(0) else {
        return Ok(Value::Bool(false));
    };
    let o = vm.to_object(&a.this)?;
    let mut cur = v.proto();
    while let Some(c) = cur {
        if c.ptr_eq(&o) {
            return Ok(Value::Bool(true));
        }
        cur = c.proto();
    }
    Ok(Value::Bool(false))
}

fn property_is_enumerable(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let k = vm.to_key(&a.arg(0))?;
    let o = vm.to_object(&a.this)?;
    Ok(Value::Bool(
        vm.get_own(&o, &k)?.map(|p| p.enumerable()).unwrap_or(false),
    ))
}

pub fn object_to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let t = match &a.this {
        Value::Undefined => return Ok(Value::str("[object Undefined]")),
        Value::Null => return Ok(Value::str("[object Null]")),
        v => vm.to_object(v)?,
    };
    let builtin = {
        let d = t.borrow();
        match &d.kind {
            Kind::Array(_) => "Array",
            Kind::Function(_) => "Function",
            Kind::Error(_) => "Error",
            Kind::Boolean(_) => "Boolean",
            Kind::Number(_) => "Number",
            Kind::String(_) => "String",
            Kind::Date(_) => "Date",
            Kind::RegExp(_) => "RegExp",
            Kind::Arguments => "Arguments",
            Kind::Host(h) => h.hooks.class,
            _ => "Object",
        }
    };
    let tag = vm.get(
        &Value::Obj(t.clone()),
        &Key::Sym(vm.syms.to_string_tag.clone()),
    )?;
    let name = match tag {
        Value::Str(s) => s.to_string(),
        _ => builtin.to_string(),
    };
    Ok(Value::string(format!("[object {name}]")))
}

fn to_locale_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = vm.get_str(&a.this, "toString")?;
    vm.call(&f, a.this.clone(), vec![])
}

fn value_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Obj(vm.to_object(&a.this)?))
}

fn proto_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    Ok(o.proto().map(Value::Obj).unwrap_or(Value::Null))
}

fn proto_set(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        match a.arg(0) {
            Value::Obj(p) => o.borrow_mut().proto = Some(p),
            Value::Null => o.borrow_mut().proto = None,
            _ => {}
        }
    }
    Ok(Value::Undefined)
}

fn define_getter_legacy(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = vm.to_object(&a.this)?;
    let k = vm.to_key(&a.arg(0))?;
    let f = a.arg(1).as_obj().cloned();
    let existing = o.borrow().props.get(&k).cloned();
    let s = match existing {
        Some(Prop {
            slot: Slot::Accessor(_, s),
            ..
        }) => s,
        _ => None,
    };
    o.borrow_mut().props.insert(
        k,
        Prop {
            slot: Slot::Accessor(f, s),
            flags: ENUMERABLE | CONFIGURABLE,
        },
    );
    Ok(Value::Undefined)
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.object_proto.clone();
    let ctor = vm.make_ctor("Object", 1, object_ctor, &proto);
    vm.intr.object_ctor = ctor.clone();
    vm.set_global("Object", Value::Obj(ctor.clone()));
    vm.method(&ctor, "keys", 1, keys);
    vm.method(&ctor, "values", 1, values);
    vm.method(&ctor, "entries", 1, entries);
    vm.method(&ctor, "assign", 2, assign);
    vm.method(&ctor, "freeze", 1, freeze);
    vm.method(&ctor, "seal", 1, seal);
    vm.method(&ctor, "preventExtensions", 1, prevent_extensions);
    vm.method(&ctor, "isFrozen", 1, is_frozen);
    vm.method(&ctor, "isSealed", 1, is_sealed);
    vm.method(&ctor, "isExtensible", 1, is_extensible);
    vm.method(&ctor, "create", 2, create);
    vm.method(&ctor, "getPrototypeOf", 1, get_prototype_of);
    vm.method(&ctor, "setPrototypeOf", 2, set_prototype_of);
    vm.method(&ctor, "defineProperty", 3, define_property);
    vm.method(&ctor, "defineProperties", 2, define_properties);
    vm.method(
        &ctor,
        "getOwnPropertyDescriptor",
        2,
        get_own_property_descriptor,
    );
    vm.method(
        &ctor,
        "getOwnPropertyDescriptors",
        1,
        get_own_property_descriptors,
    );
    vm.method(&ctor, "getOwnPropertyNames", 1, get_own_property_names);
    vm.method(&ctor, "getOwnPropertySymbols", 1, get_own_property_symbols);
    vm.method(&ctor, "fromEntries", 1, from_entries);
    vm.method(&ctor, "is", 2, is);
    vm.method(&ctor, "hasOwn", 2, has_own);
    vm.method(&ctor, "groupBy", 2, group_by);
    vm.method(&proto, "hasOwnProperty", 1, has_own_property);
    vm.method(&proto, "isPrototypeOf", 1, is_prototype_of);
    vm.method(&proto, "propertyIsEnumerable", 1, property_is_enumerable);
    vm.method(&proto, "toString", 0, object_to_string);
    vm.method(&proto, "toLocaleString", 0, to_locale_string);
    vm.method(&proto, "valueOf", 0, value_of);
    vm.method(&proto, "__defineGetter__", 2, define_getter_legacy);
    let g = vm.native_fn("get __proto__", 0, proto_get);
    let s = vm.native_fn("set __proto__", 1, proto_set);
    proto.borrow_mut().props.insert(
        Key::str("__proto__"),
        Prop {
            slot: Slot::Accessor(Some(g), Some(s)),
            flags: CONFIGURABLE,
        },
    );
}
