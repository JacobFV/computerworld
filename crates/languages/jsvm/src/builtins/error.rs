//! Error constructors, `Error.captureStackTrace`, `Error.prototype.toString`.

use crate::value::*;
use crate::vm::{ErrKind, Vm};

fn kind_of(a: &Args) -> ErrKind {
    let n = Vm::func_name(&a.callee);
    ErrKind::ALL
        .iter()
        .copied()
        .find(|k| k.name() == n)
        .unwrap_or(ErrKind::Error)
}

fn error_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let kind = kind_of(a);
    let default = vm.intr.error_protos[kind as usize].clone();
    let nt = a.new_target.clone().unwrap_or_else(|| a.callee.clone());
    let proto = vm.proto_from_ctor(&nt, &default)?;
    let e = vm.obj_with(
        Some(proto),
        Kind::Error(Box::new(ErrorData {
            frames: vec![],
            site: None,
            arrow: None,
            from_async: false,
        })),
    );
    let (msg_i, opt_i) = if kind == ErrKind::AggregateError {
        (1, 2)
    } else {
        (0, 1)
    };
    let msg = a.arg(msg_i);
    if !msg.is_undefined() {
        let m = vm.to_string(&msg)?;
        e.set_hidden("message", Value::Str(m));
    }
    let opts = a.arg(opt_i);
    if let Value::Obj(o) = &opts {
        if vm.has_property(o, &Key::str("cause"))? {
            let c = vm.get_str(&opts, "cause")?;
            e.set_hidden("cause", c);
        }
    }
    if kind == ErrKind::AggregateError {
        let errs = vm.iterable_to_vec(&a.arg(0))?;
        let arr = vm.arr(errs);
        e.set_hidden("errors", arr);
    }
    let skip = (!nt.ptr_eq(&a.callee)).then_some(nt);
    vm.capture_stack(&e, skip.as_ref());
    Ok(Value::Obj(e))
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = &a.this else {
        return Err(vm.type_error("Error.prototype.toString requires that 'this' be an Object"));
    };
    let h = vm.error_header(o)?;
    Ok(Value::string(h))
}

fn capture_stack_trace(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Err(vm.type_error("Invalid argument"));
    };
    let (mut frames, _) = vm.stack_frames(true);
    let funcs = std::mem::take(&mut vm.trace_funcs);
    // Drop frames above (and including) the given function.
    if let Value::Obj(f) = a.arg(1) {
        if let Some(i) = funcs
            .iter()
            .position(|x| matches!(x, Some(g) if g.ptr_eq(&f)))
        {
            frames.drain(..=i.min(frames.len() - 1));
        }
    }
    vm.append_tail(&mut frames);
    frames.truncate(vm.stack_limit);
    let is_error = matches!(o.borrow().kind, Kind::Error(_));
    if is_error {
        // Formatted lazily, like V8 (the header uses the name at first access).
        if let Kind::Error(ed) = &mut o.borrow_mut().kind {
            ed.frames = frames;
        }
        o.borrow_mut()
            .props
            .insert(Key::str("stack"), Prop::data(Value::Empty, HIDDEN));
        return Ok(Value::Undefined);
    }
    if let Some(v) = prepared_stack(vm, &o, &frames)? {
        o.set_hidden("stack", v);
        return Ok(Value::Undefined);
    }
    let header = vm.error_header(&o)?;
    let mut s = header;
    for f in frames {
        s.push_str("\n    at ");
        s.push_str(&f);
    }
    o.set_hidden("stack", Value::string(s));
    Ok(Value::Undefined)
}

/// `Error.prepareStackTrace(error, callSites)`, when a program installed one:
/// what V8 hands it is an array of `CallSite` objects, which `depd` (every
/// Express app loads it) and `callsites`-style libraries read. `None` when no
/// hook is installed and the stack is formatted as text.
pub(crate) fn prepared_stack(vm: &mut Vm, e: &Obj, frames: &[String]) -> JsResult<Option<Value>> {
    let Some(ctor) = vm.intr.error_ctors.first().cloned() else {
        return Ok(None);
    };
    let hook = vm.get_str(&Value::Obj(ctor), "prepareStackTrace")?;
    if !matches!(&hook, Value::Obj(o) if o.is_callable()) {
        return Ok(None);
    }
    thread_local! {
        /// A hook that reads `error.stack` itself gets the plain text, as in V8.
        static PREPARING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if PREPARING.with(|p| p.replace(true)) {
        return Ok(None);
    }
    let sites = frames.iter().map(|f| call_site(vm, f)).collect();
    let sites = vm.arr(sites);
    let r = vm.call(&hook, Value::Undefined, vec![Value::Obj(e.clone()), sites]);
    PREPARING.with(|p| p.set(false));
    r.map(Some)
}

/// One `CallSite`, from a formatted frame (`[async ][new ]name (file:line:col)`
/// or a bare `file:line:col`).
fn call_site(vm: &mut Vm, frame: &str) -> Value {
    let mut rest = frame;
    let is_async = rest.starts_with("async ");
    if is_async {
        rest = &rest[6..];
    }
    let is_ctor = rest.starts_with("new ");
    if is_ctor {
        rest = &rest[4..];
    }
    let (name, loc) = match rest.strip_suffix(')').and_then(|r| r.rsplit_once(" (")) {
        Some((n, l)) => (Some(n.to_string()), l.to_string()),
        None => (None, rest.to_string()),
    };
    let native = loc == "native";
    let mut parts = loc.rsplitn(3, ':');
    let (col, line, file) = match (parts.next(), parts.next(), parts.next()) {
        (Some(c), Some(l), Some(f)) if c.parse::<u32>().is_ok() && l.parse::<u32>().is_ok() => (
            Value::Num(c.parse::<f64>().unwrap()),
            Value::Num(l.parse::<f64>().unwrap()),
            Value::string(f.to_string()),
        ),
        _ => (
            Value::Null,
            Value::Null,
            if native {
                Value::Undefined
            } else {
                Value::string(loc.clone())
            },
        ),
    };
    let (type_name, method) = match name.as_deref().and_then(|n| n.split_once('.')) {
        Some((t, m)) => (Value::string(t.to_string()), Value::string(m.to_string())),
        None => (Value::Null, Value::Null),
    };
    let fn_name = match &name {
        Some(n) if n != "<anonymous>" => Value::string(n.clone()),
        _ => Value::Null,
    };
    let site = vm.new_object();
    let fields: [(&str, Value); 12] = [
        ("getFileName", file.clone()),
        ("getScriptNameOrSourceURL", file),
        ("getLineNumber", line),
        ("getColumnNumber", col),
        ("getFunctionName", fn_name),
        ("getTypeName", type_name.clone()),
        ("getMethodName", method),
        ("isNative", Value::Bool(native)),
        ("isConstructor", Value::Bool(is_ctor)),
        ("isAsync", Value::Bool(is_async)),
        (
            "isToplevel",
            Value::Bool(
                matches!(&type_name, Value::Null)
                    || matches!(&type_name, Value::Str(s) if s.to_string() == "Object"),
            ),
        ),
        ("toString", Value::string(frame.to_string())),
    ];
    for (method, v) in fields {
        let f = vm.native_fn_slots(method, 0, call_site_field, vec![v]);
        site.set_hidden(method, Value::Obj(f));
    }
    for method in ["isEval", "isPromiseAll"] {
        let f = vm.native_fn_slots(method, 0, call_site_field, vec![Value::Bool(false)]);
        site.set_hidden(method, Value::Obj(f));
    }
    for method in ["getThis", "getFunction", "getEvalOrigin", "getPromiseIndex"] {
        let f = vm.native_fn_slots(method, 0, call_site_field, vec![Value::Undefined]);
        site.set_hidden(method, Value::Obj(f));
    }
    Value::Obj(site)
}

fn call_site_field(_vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    Ok(crate::promise::slots(a)[0].clone())
}

fn stack_limit_get(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Num(vm.stack_limit as f64))
}

fn stack_limit_set(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = vm.to_number(&a.arg(0))?;
    vm.stack_limit = if n.is_finite() && n >= 0.0 {
        n as usize
    } else if n == f64::INFINITY {
        usize::MAX
    } else {
        0
    };
    Ok(Value::Undefined)
}

pub fn install(vm: &mut Vm) {
    let mut ctors = vec![];
    for kind in ErrKind::ALL {
        let proto = vm.intr.error_protos[kind as usize].clone();
        let len = if kind == ErrKind::AggregateError {
            2
        } else {
            1
        };
        let c = vm.make_ctor(kind.name(), len, error_ctor, &proto);
        proto.set_hidden("name", Value::str(kind.name()));
        proto.set_hidden("message", Value::str(""));
        vm.set_global(kind.name(), Value::Obj(c.clone()));
        ctors.push(c);
    }
    let error_ctor_obj = ctors[0].clone();
    for c in ctors.iter().skip(1) {
        c.borrow_mut().proto = Some(error_ctor_obj.clone());
    }
    let ep = vm.intr.error_protos[0].clone();
    vm.method(&ep, "toString", 0, to_string);
    vm.method(&error_ctor_obj, "captureStackTrace", 2, capture_stack_trace);
    let g = vm.native_fn("get stackTraceLimit", 0, stack_limit_get);
    let s = vm.native_fn("set stackTraceLimit", 1, stack_limit_set);
    error_ctor_obj.borrow_mut().props.insert(
        Key::str("stackTraceLimit"),
        Prop {
            slot: Slot::Accessor(Some(g), Some(s)),
            flags: ENUMERABLE | CONFIGURABLE,
        },
    );
    vm.intr.error_ctors = ctors;
}
