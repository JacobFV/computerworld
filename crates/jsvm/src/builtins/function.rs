//! `Function` and `Function.prototype`.

use crate::value::*;
use crate::vm::Vm;

fn function_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    compile_function_from_args(vm, a, "function", "anonymous")
}

pub fn compile_function_from_args(vm: &mut Vm, a: &Args, kw: &str, name: &str) -> JsResult<Value> {
    let n = a.args.len();
    let mut params = vec![];
    for i in 0..n.saturating_sub(1) {
        params.push(vm.to_str(&a.args[i])?);
    }
    let body = if n > 0 {
        vm.to_str(&a.args[n - 1])?
    } else {
        String::new()
    };
    let src = format!("({kw} {name}({}\n) {{\n{body}\n}})", params.join(","));
    let f = vm.eval_source(&src, "anonymous", false)?;
    Ok(f)
}

pub fn func_to_string(vm: &mut Vm, f: &Obj) -> String {
    let d = f.borrow();
    match &d.kind {
        Kind::Function(fd) => match &fd.imp {
            FuncImpl::Closure { code, .. } => {
                if code.source.is_empty() {
                    format!("function {}() {{ [native code] }}", code.name.as_str())
                } else {
                    code.source.to_string()
                }
            }
            FuncImpl::Native { .. } => {
                drop(d);
                let n = Vm::func_name(f);
                let _ = vm;
                format!("function {n}() {{ [native code] }}")
            }
            FuncImpl::Bound { .. } => "function () { [native code] }".to_string(),
        },
        _ => "function () { [native code] }".to_string(),
    }
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    match &a.this {
        Value::Obj(o) if o.is_callable() => {
            let s = func_to_string(vm, o);
            Ok(Value::string(s))
        }
        _ => Err(vm.type_error("Function.prototype.toString requires that 'this' be a Function")),
    }
}

fn call(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if !a.this.is_callable() {
        let d = vm.describe_for_error(&a.this);
        return Err(vm.type_error(format!(
            "Function.prototype.call called on {d}, which is not a function"
        )));
    }
    let this = a.arg(0);
    let rest = if a.args.len() > 1 {
        a.args[1..].to_vec()
    } else {
        vec![]
    };
    let f = a.this.clone();
    vm.call(&f, this, rest)
}

pub fn list_from_array_like(vm: &mut Vm, v: &Value) -> JsResult<Vec<Value>> {
    match v {
        Value::Undefined | Value::Null => Ok(vec![]),
        Value::Obj(o) => {
            if let Kind::Array(arr) = &o.borrow().kind {
                return Ok(arr
                    .iter()
                    .map(|x| {
                        if let Value::Empty = x {
                            Value::Undefined
                        } else {
                            x.clone()
                        }
                    })
                    .collect());
            }
            let n = vm.length_of(v)?;
            let mut out = Vec::with_capacity(n.min(1 << 20));
            for i in 0..n {
                out.push(vm.get_index(v, i)?);
            }
            Ok(out)
        }
        _ => Err(vm.type_error("CreateListFromArrayLike called on non-object")),
    }
}

fn apply(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if !a.this.is_callable() {
        let d = vm.describe_for_error(&a.this);
        return Err(vm.type_error(format!(
            "Function.prototype.apply was called on {d}, which is {} and not a function",
            a.this.type_of()
        )));
    }
    let args = list_from_array_like(vm, &a.arg(1))?;
    let f = a.this.clone();
    vm.call(&f, a.arg(0), args)
}

fn bind(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(target) = a.this.clone() else {
        return Err(vm.type_error("Bind must be called on a function"));
    };
    if !target.is_callable() {
        return Err(vm.type_error("Bind must be called on a function"));
    }
    let bargs: Vec<Value> = a.args.iter().skip(1).cloned().collect();
    let tlen = match target.own_value("length") {
        Some(Value::Num(n)) => n,
        _ => 0.0,
    };
    let len = (tlen - bargs.len() as f64).max(0.0);
    let name = format!("bound {}", Vm::func_name(&target));
    let ctor = match &target.borrow().kind {
        Kind::Function(fd) => fd.ctor,
        _ => CtorKind::None,
    };
    let proto = target.proto();
    let f = vm.obj_with(
        proto,
        Kind::Function(Box::new(FuncData {
            imp: FuncImpl::Bound {
                target: target.clone(),
                this: a.arg(0),
                args: bargs,
            },
            ctor: if ctor == CtorKind::None {
                CtorKind::None
            } else {
                CtorKind::Base
            },
            class_ctor: false,
            home: None,
            fields: None,
        })),
    );
    f.set_prop("length", Value::Num(len), CONFIGURABLE);
    f.set_prop("name", Value::string(name), CONFIGURABLE);
    Ok(Value::Obj(f))
}

fn has_instance(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(f) = a.this.clone() else {
        return Ok(Value::Bool(false));
    };
    if !f.is_callable() {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(vm.ordinary_has_instance(&f, &a.arg(0))?))
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.function_proto.clone();
    proto.set_prop("length", Value::Num(0.0), CONFIGURABLE);
    proto.set_prop("name", Value::str(""), CONFIGURABLE);
    let ctor = vm.make_ctor("Function", 1, function_ctor, &proto);
    vm.intr.function_ctor = ctor.clone();
    vm.set_global("Function", Value::Obj(ctor));
    vm.method(&proto, "toString", 0, to_string);
    vm.method(&proto, "call", 1, call);
    vm.method(&proto, "apply", 2, apply);
    vm.method(&proto, "bind", 1, bind);
    let hi = vm.syms.has_instance.clone();
    let f = vm.native_fn("[Symbol.hasInstance]", 1, has_instance);
    proto.set_sym(&hi, Value::Obj(f), 0);
}
