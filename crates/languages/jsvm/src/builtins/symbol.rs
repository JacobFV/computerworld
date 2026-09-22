//! `Symbol`.

use crate::value::*;
use crate::vm::Vm;
use std::rc::Rc;

fn symbol_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if a.new_target.is_some() {
        return Err(vm.type_error("Symbol is not a constructor"));
    }
    let desc = if a.arg(0).is_undefined() {
        None
    } else {
        Some(vm.to_string(&a.arg(0))?)
    };
    Ok(Value::Sym(Rc::new(Symbol {
        desc,
        private: false,
        registered: false,
    })))
}

fn symbol_for(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let k = vm.to_str(&a.arg(0))?;
    if let Some((_, s)) = vm.symbol_registry.iter().find(|(n, _)| *n == k) {
        return Ok(Value::Sym(s.clone()));
    }
    let s = Rc::new(Symbol {
        desc: Some(JsStr::new(k.clone())),
        private: false,
        registered: true,
    });
    vm.symbol_registry.push((k, s.clone()));
    Ok(Value::Sym(s))
}

fn key_for(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Sym(s) = a.arg(0) else {
        let d = vm.describe_for_error(&a.arg(0));
        return Err(vm.type_error(format!("{d} is not a symbol")));
    };
    if let Some((k, _)) = vm.symbol_registry.iter().find(|(_, x)| Rc::ptr_eq(x, &s)) {
        return Ok(Value::string(k.clone()));
    }
    Ok(Value::Undefined)
}

fn this_sym(vm: &mut Vm, a: &Args) -> JsResult<Rc<Symbol>> {
    match &a.this {
        Value::Sym(s) => Ok(s.clone()),
        Value::Obj(o) => match &o.borrow().kind {
            Kind::Symbol(s) => Ok(s.clone()),
            _ => Err(vm.type_error("Symbol.prototype.toString requires that 'this' be a Symbol")),
        },
        _ => Err(vm.type_error("Symbol.prototype.toString requires that 'this' be a Symbol")),
    }
}

pub fn symbol_descriptive(s: &Symbol) -> String {
    format!(
        "Symbol({})",
        s.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
    )
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_sym(vm, a)?;
    Ok(Value::string(symbol_descriptive(&s)))
}

fn value_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(Value::Sym(this_sym(vm, a)?))
}

fn description(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_sym(vm, a)?;
    Ok(s.desc.clone().map(Value::Str).unwrap_or(Value::Undefined))
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.symbol_proto.clone();
    let ctor = vm.make_ctor("Symbol", 0, symbol_ctor, &proto);
    vm.set_global("Symbol", Value::Obj(ctor.clone()));
    vm.method(&ctor, "for", 1, symbol_for);
    vm.method(&ctor, "keyFor", 1, key_for);
    let wk = [
        ("iterator", vm.syms.iterator.clone()),
        ("asyncIterator", vm.syms.async_iterator.clone()),
        ("hasInstance", vm.syms.has_instance.clone()),
        ("toPrimitive", vm.syms.to_primitive.clone()),
        ("toStringTag", vm.syms.to_string_tag.clone()),
        ("species", vm.syms.species.clone()),
        ("isConcatSpreadable", vm.syms.is_concat_spreadable.clone()),
        ("unscopables", vm.syms.unscopables.clone()),
        ("match", vm.syms.match_.clone()),
        ("matchAll", vm.syms.match_all.clone()),
        ("replace", vm.syms.replace.clone()),
        ("search", vm.syms.search.clone()),
        ("split", vm.syms.split.clone()),
    ];
    for (n, s) in wk {
        vm.constant(&ctor, n, Value::Sym(s));
    }
    vm.method(&proto, "toString", 0, to_string);
    vm.method(&proto, "valueOf", 0, value_of);
    vm.getter(&proto, "description", description);
    let tp = vm.syms.to_primitive.clone();
    let f = vm.native_fn("[Symbol.toPrimitive]", 1, value_of);
    proto.set_sym(&tp, Value::Obj(f), CONFIGURABLE);
    let tag = vm.syms.to_string_tag.clone();
    proto.set_sym(&tag, Value::str("Symbol"), CONFIGURABLE);
}
