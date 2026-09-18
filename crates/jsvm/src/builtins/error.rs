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
    vm.capture_stack(&e, None);
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
    for t in vm.tail_frames() {
        frames.push(t.to_string());
    }
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
    let header = vm.error_header(&o)?;
    let mut s = header;
    for f in frames {
        s.push_str("\n    at ");
        s.push_str(&f);
    }
    o.set_hidden("stack", Value::string(s));
    Ok(Value::Undefined)
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
