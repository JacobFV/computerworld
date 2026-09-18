//! %IteratorPrototype%, generators, async generators and iterator helpers.

use super::*;
use crate::value::*;
use crate::vm::Vm;

fn return_this(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(a.this.clone())
}

fn this_gen(vm: &mut Vm, a: &Args, method: &str) -> JsResult<Obj> {
    if let Value::Obj(o) = &a.this {
        if let Kind::Generator(gd) = &o.borrow().kind {
            if !gd.is_async {
                return Ok(o.clone());
            }
        }
    }
    let d = vm.describe_for_error(&a.this);
    Err(vm.type_error(format!(
        "{method} method called on incompatible receiver {d}"
    )))
}

fn gen_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let g = this_gen(vm, a, "next")?;
    let (v, done) = vm.gen_resume(&g, 0, a.arg(0))?;
    Ok(vm.iter_result(v, done))
}

fn gen_return(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let g = this_gen(vm, a, "return")?;
    let (v, done) = vm.gen_resume(&g, 2, a.arg(0))?;
    Ok(vm.iter_result(v, done))
}

fn gen_throw(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let g = this_gen(vm, a, "throw")?;
    let (v, done) = vm.gen_resume(&g, 1, a.arg(0))?;
    Ok(vm.iter_result(v, done))
}

fn agen_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(g) = a.this.clone() else {
        return Err(vm.type_error("next method called on incompatible receiver"));
    };
    vm.async_gen_enqueue(&g, 0, a.arg(0))
}
fn agen_return(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(g) = a.this.clone() else {
        return Err(vm.type_error("return method called on incompatible receiver"));
    };
    vm.async_gen_enqueue(&g, 2, a.arg(0))
}
fn agen_throw(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(g) = a.this.clone() else {
        return Err(vm.type_error("throw method called on incompatible receiver"));
    };
    vm.async_gen_enqueue(&g, 1, a.arg(0))
}

// ---------------------------------------------------------------- helpers

fn iter_record(vm: &mut Vm, this: &Value) -> JsResult<(Value, Value)> {
    if !matches!(this, Value::Obj(_)) {
        let d = vm.describe_for_error(this);
        return Err(vm.type_error(format!("{d} is not an object")));
    }
    let next = vm.get_str(this, "next")?;
    Ok((this.clone(), next))
}

/// Internal slots: [iter, next, fn, kind, counter, limit, inner_iter, inner_next]
fn make_helper(vm: &mut Vm, a: &Args, kind: u8) -> JsResult<Value> {
    let (it, next) = iter_record(vm, &a.this)?;
    let (f, limit) = match kind {
        2 | 3 => {
            let n = vm.to_number(&a.arg(0))?;
            if n.is_nan() || n < 0.0 {
                let d = vm.describe_for_error(&a.arg(0));
                return Err(vm.range_error(format!("{d} must be positive")));
            }
            (Value::Undefined, n)
        }
        _ => (callback(vm, &a.arg(0))?, 0.0),
    };
    let proto = helper_proto(vm);
    let o = vm.obj_with(
        Some(proto),
        Kind::Internal(vec![
            it,
            next,
            f,
            Value::Num(kind as f64),
            Value::Num(0.0),
            Value::Num(limit),
            Value::Undefined,
            Value::Undefined,
        ]),
    );
    Ok(Value::Obj(o))
}

fn helper_proto(vm: &mut Vm) -> Obj {
    let ip = vm.intr.iterator_proto.clone();
    if let Some(Value::Obj(p)) = ip.own_value("%helper") {
        return p;
    }
    let p = vm.obj_with(Some(ip.clone()), Kind::Ordinary);
    vm.method(&p, "next", 0, helper_next);
    vm.method(&p, "return", 0, helper_return);
    let tag = vm.syms.to_string_tag.clone();
    p.set_sym(&tag, Value::str("Iterator Helper"), CONFIGURABLE);
    ip.borrow_mut()
        .props
        .insert(Key::str("%helper"), Prop::data(Value::Obj(p.clone()), 0));
    p
}

fn helper_slots(o: &Obj) -> Vec<Value> {
    match &o.borrow().kind {
        Kind::Internal(v) => v.clone(),
        _ => vec![],
    }
}

fn set_slot(o: &Obj, i: usize, v: Value) {
    if let Kind::Internal(s) = &mut o.borrow_mut().kind {
        s[i] = v;
    }
}

fn helper_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.this.clone() else {
        return Err(vm.type_error("next method called on incompatible receiver"));
    };
    let s = helper_slots(&o);
    if s.len() < 8 {
        return Err(vm.type_error("next method called on incompatible receiver"));
    }
    let (it, next, f) = (s[0].clone(), s[1].clone(), s[2].clone());
    let kind = match s[3] {
        Value::Num(n) => n as u8,
        _ => 0,
    };
    let mut counter = match s[4] {
        Value::Num(n) => n,
        _ => 0.0,
    };
    let limit = match s[5] {
        Value::Num(n) => n,
        _ => 0.0,
    };
    loop {
        match kind {
            0 | 1 => {
                let Some(v) = vm.iter_step(&it, &next)? else {
                    return Ok(vm.iter_result(Value::Undefined, true));
                };
                let r = vm.call(&f, Value::Undefined, vec![v.clone(), Value::Num(counter)])?;
                counter += 1.0;
                set_slot(&o, 4, Value::Num(counter));
                if kind == 0 {
                    return Ok(vm.iter_result(r, false));
                }
                if r.truthy() {
                    return Ok(vm.iter_result(v, false));
                }
            }
            2 => {
                if counter >= limit {
                    vm.iter_close(&it)?;
                    return Ok(vm.iter_result(Value::Undefined, true));
                }
                counter += 1.0;
                set_slot(&o, 4, Value::Num(counter));
                return match vm.iter_step(&it, &next)? {
                    Some(v) => Ok(vm.iter_result(v, false)),
                    None => Ok(vm.iter_result(Value::Undefined, true)),
                };
            }
            3 => {
                while counter < limit {
                    counter += 1.0;
                    set_slot(&o, 4, Value::Num(counter));
                    if vm.iter_step(&it, &next)?.is_none() {
                        return Ok(vm.iter_result(Value::Undefined, true));
                    }
                }
                return match vm.iter_step(&it, &next)? {
                    Some(v) => Ok(vm.iter_result(v, false)),
                    None => Ok(vm.iter_result(Value::Undefined, true)),
                };
            }
            _ => {
                // flatMap
                let s = helper_slots(&o);
                if !s[6].is_undefined() {
                    if let Some(v) = vm.iter_step(&s[6], &s[7])? {
                        return Ok(vm.iter_result(v, false));
                    }
                    set_slot(&o, 6, Value::Undefined);
                }
                let Some(v) = vm.iter_step(&it, &next)? else {
                    return Ok(vm.iter_result(Value::Undefined, true));
                };
                let r = vm.call(&f, Value::Undefined, vec![v, Value::Num(counter)])?;
                counter += 1.0;
                set_slot(&o, 4, Value::Num(counter));
                let (ii, inext) = vm.get_iterator(&r)?;
                set_slot(&o, 6, ii);
                set_slot(&o, 7, inext);
            }
        }
    }
}

fn helper_return(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        let s = helper_slots(o);
        if !s.is_empty() {
            vm.iter_close(&s[0])?;
        }
    }
    Ok(vm.iter_result(Value::Undefined, true))
}

fn it_map(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    make_helper(vm, a, 0)
}
fn it_filter(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    make_helper(vm, a, 1)
}
fn it_take(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    make_helper(vm, a, 2)
}
fn it_drop(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    make_helper(vm, a, 3)
}
fn it_flat_map(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    make_helper(vm, a, 4)
}

fn it_to_array(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (it, next) = iter_record(vm, &a.this)?;
    let mut out = vec![];
    while let Some(v) = vm.iter_step(&it, &next)? {
        out.push(v);
    }
    Ok(vm.arr(out))
}

/// kind: 0 forEach, 1 some, 2 every, 3 find
fn it_consume(vm: &mut Vm, a: &mut Args, kind: u8) -> JsResult<Value> {
    let (it, next) = iter_record(vm, &a.this)?;
    let f = callback(vm, &a.arg(0))?;
    let mut i = 0.0;
    while let Some(v) = vm.iter_step(&it, &next)? {
        let r = vm.call(&f, Value::Undefined, vec![v.clone(), Value::Num(i)])?;
        i += 1.0;
        match kind {
            1 if r.truthy() => {
                vm.iter_close(&it)?;
                return Ok(Value::Bool(true));
            }
            2 if !r.truthy() => {
                vm.iter_close(&it)?;
                return Ok(Value::Bool(false));
            }
            3 if r.truthy() => {
                vm.iter_close(&it)?;
                return Ok(v);
            }
            _ => {}
        }
    }
    Ok(match kind {
        1 => Value::Bool(false),
        2 => Value::Bool(true),
        _ => Value::Undefined,
    })
}

fn it_for_each(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    it_consume(vm, a, 0)
}
fn it_some(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    it_consume(vm, a, 1)
}
fn it_every(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    it_consume(vm, a, 2)
}
fn it_find(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    it_consume(vm, a, 3)
}

fn it_reduce(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let (it, next) = iter_record(vm, &a.this)?;
    let f = callback(vm, &a.arg(0))?;
    let mut acc = if a.args.len() >= 2 {
        a.arg(1)
    } else {
        match vm.iter_step(&it, &next)? {
            Some(v) => v,
            None => return Err(vm.type_error("Reduce of empty iterator with no initial value")),
        }
    };
    let mut i = 0.0;
    while let Some(v) = vm.iter_step(&it, &next)? {
        acc = vm.call(&f, Value::Undefined, vec![acc, v, Value::Num(i)])?;
        i += 1.0;
    }
    Ok(acc)
}

fn iterator_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    match &a.new_target {
        Some(nt) if !nt.ptr_eq(&a.callee) => {
            let ip = vm.intr.iterator_proto.clone();
            let p = vm.proto_from_ctor(nt, &ip)?;
            Ok(Value::Obj(vm.obj_with(Some(p), Kind::Ordinary)))
        }
        _ => Err(vm.type_error("Abstract class Iterator not directly constructable")),
    }
}

fn iterator_from(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let v = a.arg(0);
    let m = vm.get(&v, &Key::Sym(vm.syms.iterator.clone()))?;
    if m.is_callable() {
        return vm.call(&m, v, vec![]);
    }
    Ok(v)
}

pub fn install(vm: &mut Vm) {
    let ip = vm.intr.iterator_proto.clone();
    let it = vm.syms.iterator.clone();
    vm.method_sym(&ip, &it, "[Symbol.iterator]", 0, return_this);
    let fs: &[(&str, u32, NativeFn)] = &[
        ("map", 1, it_map),
        ("filter", 1, it_filter),
        ("take", 1, it_take),
        ("drop", 1, it_drop),
        ("flatMap", 1, it_flat_map),
        ("toArray", 0, it_to_array),
        ("forEach", 1, it_for_each),
        ("some", 1, it_some),
        ("every", 1, it_every),
        ("find", 1, it_find),
        ("reduce", 1, it_reduce),
    ];
    for (n, l, f) in fs {
        vm.method(&ip, n, *l, *f);
    }
    let ictor = vm.make_ctor("Iterator", 0, iterator_ctor, &ip);
    vm.method(&ictor, "from", 1, iterator_from);
    // Iterator.prototype.constructor is an accessor (iterator helpers).
    let cg = vm.native_fn_slots(
        "get constructor",
        0,
        |_vm, a| Ok(crate::promise::slots(a)[0].clone()),
        vec![Value::Obj(ictor.clone())],
    );
    ip.borrow_mut().props.insert(
        Key::str("constructor"),
        Prop {
            slot: Slot::Accessor(Some(cg), None),
            flags: CONFIGURABLE,
        },
    );
    vm.set_global("Iterator", Value::Obj(ictor));
    // %AsyncFunction%, %GeneratorFunction%, %AsyncGeneratorFunction%.
    for (name, proto) in [
        ("AsyncFunction", vm.intr.async_function_proto.clone()),
        (
            "GeneratorFunction",
            vm.intr.generator_function_proto.clone(),
        ),
        (
            "AsyncGeneratorFunction",
            vm.intr.async_generator_function_proto.clone(),
        ),
    ] {
        let f: NativeFn = match name {
            "AsyncFunction" => |vm, a| {
                crate::builtins::function::compile_function_from_args(
                    vm,
                    a,
                    "async function",
                    "anonymous",
                )
            },
            "GeneratorFunction" => |vm, a| {
                crate::builtins::function::compile_function_from_args(
                    vm,
                    a,
                    "function*",
                    "anonymous",
                )
            },
            _ => |vm, a| {
                crate::builtins::function::compile_function_from_args(
                    vm,
                    a,
                    "async function*",
                    "anonymous",
                )
            },
        };
        let c = vm.make_ctor(name, 1, f, &proto);
        c.borrow_mut().proto = Some(vm.intr.function_ctor.clone());
    }

    let aip = vm.intr.async_iterator_proto.clone();
    let ai = vm.syms.async_iterator.clone();
    vm.method_sym(&aip, &ai, "[Symbol.asyncIterator]", 0, return_this);

    let tag = vm.syms.to_string_tag.clone();
    let gp = vm.intr.generator_proto.clone();
    vm.method(&gp, "next", 1, gen_next);
    vm.method(&gp, "return", 1, gen_return);
    vm.method(&gp, "throw", 1, gen_throw);
    gp.set_sym(&tag, Value::str("Generator"), CONFIGURABLE);
    let gfp = vm.intr.generator_function_proto.clone();
    gfp.set_prop("prototype", Value::Obj(gp.clone()), CONFIGURABLE);
    gp.set_prop("constructor", Value::Obj(gfp.clone()), CONFIGURABLE);
    gfp.set_sym(&tag, Value::str("GeneratorFunction"), CONFIGURABLE);
    let _ = &gp;

    let agp = vm.intr.async_generator_proto.clone();
    vm.method(&agp, "next", 1, agen_next);
    vm.method(&agp, "return", 1, agen_return);
    vm.method(&agp, "throw", 1, agen_throw);
    agp.set_sym(&tag, Value::str("AsyncGenerator"), CONFIGURABLE);
    let agfp = vm.intr.async_generator_function_proto.clone();
    agfp.set_prop("prototype", Value::Obj(agp), CONFIGURABLE);
    agfp.set_sym(&tag, Value::str("AsyncGeneratorFunction"), CONFIGURABLE);
    let afp = vm.intr.async_function_proto.clone();
    afp.set_sym(&tag, Value::str("AsyncFunction"), CONFIGURABLE);
}
