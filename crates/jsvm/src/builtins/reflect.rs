//! `Reflect` and `Proxy`.

use super::*;
use crate::builtins::function::list_from_array_like;
use crate::value::*;
use crate::vm::Vm;

fn target(vm: &mut Vm, a: &Args, name: &str) -> JsResult<Obj> {
    match a.arg(0) {
        Value::Obj(o) => Ok(o),
        _ => Err(vm.type_error(format!("Reflect.{name} called on non-object"))),
    }
}

fn r_apply(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = a.arg(0);
    let args = list_from_array_like(vm, &a.arg(2))?;
    if !f.is_callable() {
        let d = vm.describe_for_error(&f);
        return Err(vm.type_error(format!(
            "Function.prototype.apply was called on {d}, which is {} and not a function",
            f.type_of()
        )));
    }
    vm.call(&f, a.arg(1), args)
}

fn r_construct(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let f = a.arg(0);
    if !vm.is_constructor(&f) {
        let d = vm.describe_for_error(&f);
        return Err(vm.type_error(format!("{d} is not a constructor")));
    }
    let args = list_from_array_like(vm, &a.arg(1))?;
    let nt = if a.args.len() > 2 {
        a.arg(2).as_obj().cloned()
    } else {
        None
    };
    vm.construct(&f, args, nt)
}

fn r_define_property(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "defineProperty")?;
    let k = vm.to_key(&a.arg(1))?;
    let existing = vm.get_own(&o, &k)?;
    let p = crate::builtins::object::to_prop(vm, &a.arg(2), existing)?;
    Ok(Value::Bool(vm.define_own(&o, k, p)?))
}

fn r_delete_property(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "deleteProperty")?;
    let k = vm.to_key(&a.arg(1))?;
    Ok(Value::Bool(vm.delete(&o, &k)?))
}

fn r_get(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "get")?;
    let k = vm.to_key(&a.arg(1))?;
    let recv = if a.args.len() > 2 {
        a.arg(2)
    } else {
        Value::Obj(o.clone())
    };
    vm.get_from(&o, &k, &recv)
}

fn r_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "set")?;
    let k = vm.to_key(&a.arg(1))?;
    let recv = if a.args.len() > 3 {
        a.arg(3)
    } else {
        Value::Obj(o.clone())
    };
    Ok(Value::Bool(vm.set_on(&o, k, a.arg(2), &recv)?))
}

fn r_has(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "has")?;
    let k = vm.to_key(&a.arg(1))?;
    Ok(Value::Bool(vm.has_property(&o, &k)?))
}

fn r_own_keys(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "ownKeys")?;
    let ks = vm.own_keys(&o)?;
    Ok(vm.arr(ks.into_iter().map(|k| k.to_value()).collect()))
}

fn r_gopd(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "getOwnPropertyDescriptor")?;
    let k = vm.to_key(&a.arg(1))?;
    match vm.get_own(&o, &k)? {
        Some(p) => Ok(crate::builtins::object::from_prop(vm, p)),
        None => Ok(Value::Undefined),
    }
}

fn r_get_proto(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "getPrototypeOf")?;
    Ok(o.proto().map(Value::Obj).unwrap_or(Value::Null))
}

fn r_set_proto(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "setPrototypeOf")?;
    match a.arg(1) {
        Value::Obj(p) => o.borrow_mut().proto = Some(p),
        Value::Null => o.borrow_mut().proto = None,
        _ => return Err(vm.type_error("Object prototype may only be an Object or null")),
    }
    Ok(Value::Bool(true))
}

fn r_is_extensible(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "isExtensible")?;
    let e = o.borrow().extensible;
    Ok(Value::Bool(e))
}

fn r_prevent_extensions(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let o = target(vm, a, "preventExtensions")?;
    o.borrow_mut().extensible = false;
    Ok(Value::Bool(true))
}

fn proxy_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if a.new_target.is_none() {
        return Err(vm.type_error("Constructor Proxy requires 'new'"));
    }
    let (Value::Obj(t), Value::Obj(h)) = (a.arg(0), a.arg(1)) else {
        return Err(vm.type_error("Cannot create proxy with a non-object as target or handler"));
    };
    let callable = t.is_callable();
    let proxy = vm.obj_with(
        t.proto(),
        Kind::Proxy {
            target: t.clone(),
            handler: h,
        },
    );
    let _ = callable;
    Ok(Value::Obj(proxy))
}

fn proxy_revocable(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut a2 = Args {
        this: Value::Undefined,
        args: a.args.clone(),
        new_target: Some(a.callee.clone()),
        callee: a.callee.clone(),
    };
    let p = proxy_ctor(vm, &mut a2)?;
    let revoke = vm.native_fn("", 0, |_vm, _a| Ok(Value::Undefined));
    let o = new_obj_from(vm, vec![("proxy", p), ("revoke", Value::Obj(revoke))]);
    Ok(Value::Obj(o))
}

pub fn install(vm: &mut Vm) {
    let r = vm.new_object();
    let tag = vm.syms.to_string_tag.clone();
    r.set_sym(&tag, Value::str("Reflect"), CONFIGURABLE);
    let fs: &[(&str, u32, NativeFn)] = &[
        ("apply", 3, r_apply),
        ("construct", 2, r_construct),
        ("defineProperty", 3, r_define_property),
        ("deleteProperty", 2, r_delete_property),
        ("get", 2, r_get),
        ("set", 3, r_set),
        ("has", 2, r_has),
        ("ownKeys", 1, r_own_keys),
        ("getOwnPropertyDescriptor", 2, r_gopd),
        ("getPrototypeOf", 1, r_get_proto),
        ("setPrototypeOf", 2, r_set_proto),
        ("isExtensible", 1, r_is_extensible),
        ("preventExtensions", 1, r_prevent_extensions),
    ];
    for (n, l, f) in fs {
        vm.method(&r, n, *l, *f);
    }
    vm.set_global("Reflect", Value::Obj(r));
    let pc = vm.native_fn("Proxy", 2, proxy_ctor);
    if let Kind::Function(fd) = &mut pc.borrow_mut().kind {
        fd.ctor = CtorKind::Base;
    }
    vm.method(&pc, "revocable", 2, proxy_revocable);
    vm.set_global("Proxy", Value::Obj(pc));
}
