//! The functions of the `builtins` module and the constructors of builtin types.
use crate::ast::BinOp;
use crate::bigint::BigInt;
use crate::builtins::*;
use crate::ops::Num;
use crate::value::*;
use crate::vm::*;
use std::cell::RefCell;
use std::rc::Rc;

pub fn install(vm: &mut Vm) {
    let b = vm.builtins.clone();
    let mut d = b.borrow_mut();
    let fns: &[(&str, NativeFn)] = &[
        ("print", b_print),
        ("input", b_input),
        ("len", b_len),
        ("repr", b_repr),
        ("ascii", b_ascii),
        ("abs", b_abs),
        ("all", b_all),
        ("any", b_any),
        ("sum", b_sum),
        ("min", b_min),
        ("max", b_max),
        ("sorted", b_sorted),
        ("isinstance", b_isinstance),
        ("issubclass", b_issubclass),
        ("callable", b_callable),
        ("chr", b_chr),
        ("ord", b_ord),
        ("hex", b_hex),
        ("bin", b_bin),
        ("oct", b_oct),
        ("divmod", b_divmod),
        ("pow", b_pow),
        ("round", b_round),
        ("id", b_id),
        ("hash", b_hash),
        ("iter", b_iter),
        ("next", b_next),
        ("getattr", b_getattr),
        ("setattr", b_setattr),
        ("hasattr", b_hasattr),
        ("delattr", b_delattr),
        ("vars", b_vars),
        ("dir", b_dir),
        ("globals", b_globals),
        ("locals", b_locals),
        ("format", b_format),
        ("open", crate::io::b_open),
        ("exec", b_exec),
        ("eval", b_eval),
        ("compile", b_compile),
        ("__import__", b_import),
        ("__build_class__", b_build_class),
        ("exit", b_exit),
        ("quit", b_exit),
        ("help", b_help),
        ("breakpoint", b_breakpoint),
        ("aiter", b_iter),
    ];
    for (name, f) in fns {
        d.set_str(name, native_fn(name, *f));
    }
    let t = &vm.t;
    for (name, cls) in [
        ("object", &t.object),
        ("type", &t.type_),
        ("int", &t.int),
        ("bool", &t.bool_),
        ("float", &t.float),
        ("complex", &t.complex),
        ("str", &t.str_),
        ("bytes", &t.bytes),
        ("bytearray", &t.bytearray),
        ("list", &t.list),
        ("tuple", &t.tuple),
        ("dict", &t.dict),
        ("set", &t.set),
        ("frozenset", &t.frozenset),
        ("range", &t.range),
        ("slice", &t.slice),
        ("enumerate", &t.enumerate),
        ("zip", &t.zip),
        ("map", &t.map),
        ("filter", &t.filter),
        ("reversed", &t.reversed),
        ("property", &t.property),
        ("staticmethod", &t.staticmethod),
        ("classmethod", &t.classmethod),
        ("super", &t.super_),
    ] {
        d.set_str(name, Value::Class(cls.clone()));
    }
    for (name, cls) in &t.exceptions {
        d.set_str(name, Value::Class(cls.clone()));
    }
    d.set_str("EnvironmentError", Value::Class(t.exc("OSError")));
    d.set_str("IOError", Value::Class(t.exc("OSError")));
    d.set_str("None", Value::None);
    d.set_str("True", Value::Bool(true));
    d.set_str("False", Value::Bool(false));
    d.set_str("NotImplemented", Value::NotImplemented);
    d.set_str("Ellipsis", Value::Ellipsis);
    d.set_str("__name__", Value::str("builtins"));
    d.set_str("__debug__", Value::Bool(true));
    drop(d);
    install_constructors(vm);
}

fn install_constructors(vm: &mut Vm) {
    let t = &vm.t;
    let news: Vec<(&Rc<Class>, NativeFn)> = vec![
        (&t.object, object_new),
        (&t.type_, type_new_builtin),
        (&t.int, int_new),
        (&t.bool_, bool_new),
        (&t.float, float_new),
        (&t.complex, complex_new),
        (&t.str_, str_new),
        (&t.bytes, bytes_new),
        (&t.bytearray, bytearray_new),
        (&t.list, list_new),
        (&t.tuple, tuple_new),
        (&t.dict, dict_new),
        (&t.set, set_new),
        (&t.frozenset, frozenset_new),
        (&t.range, range_new),
        (&t.slice, slice_new),
        (&t.enumerate, enumerate_new),
        (&t.zip, zip_new),
        (&t.map, map_new),
        (&t.filter, filter_new),
        (&t.reversed, reversed_new),
        (&t.property, property_new),
        (&t.staticmethod, staticmethod_new),
        (&t.classmethod, classmethod_new),
        (&t.super_, super_new),
        (&t.none_type, none_new),
    ];
    for (cls, f) in news {
        let b = Builtin {
            name: format!("{}.__new__", cls.name()).into(),
            func: f,
            data: Value::None,
            owner: Some("type"),
        };
        cls.dict.borrow_mut().set_str(
            "__new__",
            Value::StaticMethod(Rc::new(Value::Builtin(Rc::new(b)))),
        );
    }
    // object.__new__ is special-cased by name, so it is stored unwrapped too.
    let on = Builtin {
        name: "object.__new__".into(),
        func: object_new,
        data: Value::None,
        owner: None,
    };
    t.object
        .dict
        .borrow_mut()
        .set_str("__new__", Value::Builtin(Rc::new(on)));
    let exc_new = Builtin {
        name: "BaseException.__new__".into(),
        func: exception_new,
        data: Value::None,
        owner: None,
    };
    t.exc("BaseException")
        .dict
        .borrow_mut()
        .set_str("__new__", Value::Builtin(Rc::new(exc_new)));
}

// ---------------------------------------------------------------------------
// I/O builtins
// ---------------------------------------------------------------------------

fn b_print(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let sep = a.kw("sep").unwrap_or(Value::None);
    let end = a.kw("end").unwrap_or(Value::None);
    let file = a.kw("file").unwrap_or(Value::None);
    let flush = a.kw("flush");
    let _ = flush;
    if let Some((k, _)) = a.kwargs.first() {
        return Err(type_err(format!(
            "'{k}' is an invalid keyword argument for print()"
        )));
    }
    let sep = match &sep {
        Value::None => " ".to_string(),
        Value::Str(s) => s.s.clone(),
        other => {
            return Err(type_err(format!(
                "sep must be None or a string, not {}",
                vm.type_name(other)
            )))
        }
    };
    let end = match &end {
        Value::None => "\n".to_string(),
        Value::Str(s) => s.s.clone(),
        other => {
            return Err(type_err(format!(
                "end must be None or a string, not {}",
                vm.type_name(other)
            )))
        }
    };
    let mut out = String::new();
    for (i, v) in a.args.iter().enumerate() {
        if i > 0 {
            out.push_str(&sep);
        }
        out.push_str(&vm.str_of(v)?);
    }
    out.push_str(&end);
    let file = if file.is_none() {
        crate::io::sys_stream(vm, "stdout")
    } else {
        file
    };
    crate::io::write_to(vm, &file, &out)?;
    Ok(Value::None)
}

fn b_input(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "input", 0, 1)?;
    if let Some(p) = a.args.first() {
        let s = vm.str_of(p)?;
        let out = crate::io::sys_stream(vm, "stdout");
        crate::io::write_to(vm, &out, &s)?;
    }
    let stdin = crate::io::sys_stream(vm, "stdin");
    let line = match &stdin {
        Value::File(f) => crate::io::readline(vm, f, -1)?,
        other => {
            let rl = vm.getattr_str(other, "readline")?;
            vm.call(&rl, vec![])?
        }
    };
    match line {
        Value::Str(s) if s.s.is_empty() => Err(err("EOFError", "EOF when reading a line")),
        Value::Str(s) => Ok(Value::str(s.s.strip_suffix('\n').unwrap_or(&s.s))),
        other => Ok(other),
    }
}

// ---------------------------------------------------------------------------
// Basic builtins
// ---------------------------------------------------------------------------

fn b_len(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "len", 1, 1)?;
    Ok(Value::Int(vm.len(&a.args[0])? as i64))
}
fn b_repr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "repr", 1, 1)?;
    Ok(Value::string(vm.repr(&a.args[0])?))
}
fn b_ascii(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "ascii", 1, 1)?;
    Ok(Value::string(crate::format::ascii_escape(
        &vm.repr(&a.args[0])?,
    )))
}
fn b_abs(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "abs", 1, 1)?;
    let v = &a.args[0];
    if !matches!(v, Value::Instance(_)) {
        if let Some(n) = vm.as_num(v) {
            return Ok(match n {
                Num::I(i) => match i.checked_abs() {
                    Some(x) => Value::Int(x),
                    None => Value::big(BigInt::from_i64(i).abs()),
                },
                Num::B(b) => Value::big(b.abs()),
                Num::F(f) => Value::Float(f.abs()),
                Num::C(r, i) => Value::Float(r.hypot(i)),
            });
        }
    }
    if let Some(r) = vm.call_special(v, "__abs__", vec![])? {
        return Ok(r);
    }
    Err(type_err(format!(
        "bad operand type for abs(): '{}'",
        vm.type_name(v)
    )))
}
fn b_all(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "all", 1, 1)?;
    let it = vm.get_iter(&a.args[0])?;
    while let Some(x) = vm.next(&it)? {
        if !vm.truthy(&x)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}
fn b_any(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "any", 1, 1)?;
    let it = vm.get_iter(&a.args[0])?;
    while let Some(x) = vm.next(&it)? {
        if vm.truthy(&x)? {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}

fn b_sum(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(&mut a, "sum", &["iterable", "start"], 1)?;
    let start = p[1].clone().unwrap_or(Value::Int(0));
    if matches!(start, Value::Str(_)) {
        return Err(type_err(
            "sum() can't sum strings [use ''.join(seq) instead]",
        ));
    }
    if matches!(start, Value::Bytes(_)) {
        return Err(type_err(
            "sum() can't sum bytes [use b''.join(seq) instead]",
        ));
    }
    let it = vm.get_iter(p[0].as_ref().unwrap())?;
    let mut result = start;
    // Integer phase.
    if let Value::Int(mut acc) = result {
        loop {
            let Some(x) = vm.next(&it)? else {
                return Ok(Value::Int(acc));
            };
            match x {
                Value::Int(i) => match acc.checked_add(i) {
                    Some(v) => acc = v,
                    None => {
                        result = vm.binary_op(&Value::Int(acc), &x, BinOp::Add)?;
                        break;
                    }
                },
                Value::Bool(b) => acc += b as i64,
                other => {
                    result = vm.binary_op(&Value::Int(acc), &other, BinOp::Add)?;
                    break;
                }
            }
        }
    }
    // Float phase with Neumaier compensation (CPython 3.12).
    if let Value::Float(mut f) = result {
        let mut c = 0.0f64;
        loop {
            let Some(x) = vm.next(&it)? else {
                if c != 0.0 && c.is_finite() {
                    f += c;
                }
                return Ok(Value::Float(f));
            };
            match x {
                Value::Float(xf) => {
                    let t = f + xf;
                    if f.abs() >= xf.abs() {
                        c += (f - t) + xf;
                    } else {
                        c += (xf - t) + f;
                    }
                    f = t;
                }
                Value::Int(i) => f += i as f64,
                Value::Bool(b) => f += b as i64 as f64,
                other => {
                    if c != 0.0 && c.is_finite() {
                        f += c;
                    }
                    result = vm.binary_op(&Value::Float(f), &other, BinOp::Add)?;
                    break;
                }
            }
        }
    }
    while let Some(x) = vm.next(&it)? {
        result = vm.binary_op(&result, &x, BinOp::Add)?;
    }
    Ok(result)
}

fn minmax(vm: &mut Vm, mut a: Args, is_max: bool) -> PyResult<Value> {
    let name = if is_max { "max" } else { "min" };
    let key = a.kw("key").filter(|k| !k.is_none());
    let default = a.kw("default");
    if let Some((k, _)) = a.kwargs.first() {
        return Err(type_err(format!(
            "'{k}' is an invalid keyword argument for {name}()"
        )));
    }
    let items = match a.args.len() {
        0 => {
            return Err(type_err(format!(
                "{name} expected at least 1 argument, got 0"
            )))
        }
        1 => {
            let it = a.args.pop().unwrap();
            vm.iterate(&it)?
        }
        _ => {
            if default.is_some() {
                return Err(type_err(format!(
                    "Cannot specify a default for {name}() with multiple positional arguments"
                )));
            }
            std::mem::take(&mut a.args)
        }
    };
    if items.is_empty() {
        return match default {
            Some(d) => Ok(d),
            None => Err(value_err(format!("{name}() iterable argument is empty"))),
        };
    }
    let mut best = items[0].clone();
    let mut best_key = match &key {
        Some(k) => vm.call(k, vec![best.clone()])?,
        None => best.clone(),
    };
    for x in items.into_iter().skip(1) {
        let k = match &key {
            Some(kf) => vm.call(kf, vec![x.clone()])?,
            None => x.clone(),
        };
        let better = if is_max {
            vm.less_than(&best_key, &k)?
        } else {
            vm.less_than(&k, &best_key)?
        };
        if better {
            best = x;
            best_key = k;
        }
    }
    Ok(best)
}
fn b_min(vm: &mut Vm, a: Args) -> PyResult<Value> {
    minmax(vm, a, false)
}
fn b_max(vm: &mut Vm, a: Args) -> PyResult<Value> {
    minmax(vm, a, true)
}

/// Stable merge sort using only `<`, like CPython's list.sort.
pub fn sort_values(
    vm: &mut Vm,
    items: Vec<Value>,
    key: Option<&Value>,
    reverse: bool,
) -> PyResult<Vec<Value>> {
    let keys: Vec<Value> = match key {
        Some(k) => {
            let mut out = Vec::with_capacity(items.len());
            for it in &items {
                out.push(vm.call(k, vec![it.clone()])?);
            }
            out
        }
        None => items.clone(),
    };
    let mut idx: Vec<usize> = (0..items.len()).collect();
    if reverse {
        idx.reverse();
    }
    // Fast path: all ints or all strs.
    let all_int = keys.iter().all(|k| matches!(k, Value::Int(_)));
    let all_str = keys.iter().all(|k| matches!(k, Value::Str(_)));
    if all_int || all_str {
        idx.sort_by(|&x, &y| match (&keys[x], &keys[y]) {
            (Value::Int(p), Value::Int(q)) => p.cmp(q),
            (Value::Str(p), Value::Str(q)) => p.s.cmp(&q.s),
            _ => std::cmp::Ordering::Equal,
        });
    } else {
        let mut buf = idx.clone();
        merge_sort(vm, &keys, &mut idx, &mut buf)?;
    }
    if reverse {
        idx.reverse();
    }
    Ok(idx.into_iter().map(|i| items[i].clone()).collect())
}
fn merge_sort(vm: &mut Vm, keys: &[Value], idx: &mut [usize], buf: &mut [usize]) -> PyResult<()> {
    let n = idx.len();
    if n <= 1 {
        return Ok(());
    }
    if n <= 8 {
        // Binary-insertion-free simple insertion sort (stable).
        for i in 1..n {
            let mut j = i;
            while j > 0 && vm.less_than(&keys[idx[j]], &keys[idx[j - 1]])? {
                idx.swap(j, j - 1);
                j -= 1;
            }
        }
        return Ok(());
    }
    let mid = n / 2;
    {
        let (l, r) = idx.split_at_mut(mid);
        let (bl, br) = buf.split_at_mut(mid);
        merge_sort(vm, keys, l, bl)?;
        merge_sort(vm, keys, r, br)?;
    }
    buf[..n].copy_from_slice(idx);
    let (mut i, mut j, mut k) = (0, mid, 0);
    while i < mid && j < n {
        if vm.less_than(&keys[buf[j]], &keys[buf[i]])? {
            idx[k] = buf[j];
            j += 1;
        } else {
            idx[k] = buf[i];
            i += 1;
        }
        k += 1;
    }
    while i < mid {
        idx[k] = buf[i];
        i += 1;
        k += 1;
    }
    while j < n {
        idx[k] = buf[j];
        j += 1;
        k += 1;
    }
    Ok(())
}

fn b_sorted(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let key = a.kw("key").filter(|k| !k.is_none());
    let reverse = match a.kw("reverse") {
        Some(r) => vm.truthy(&r)?,
        None => false,
    };
    if let Some((k, _)) = a.kwargs.first() {
        return Err(type_err(format!(
            "'{k}' is an invalid keyword argument for sort()"
        )));
    }
    if a.args.len() != 1 {
        return Err(type_err(format!(
            "sorted expected 1 argument, got {}",
            a.args.len()
        )));
    }
    let items = vm.iterate(&a.args[0])?;
    let out = sort_values(vm, items, key.as_ref(), reverse)?;
    Ok(Value::list(out))
}

pub fn class_matches(vm: &mut Vm, v: &Value, cls: &Value, fname: &str) -> PyResult<bool> {
    match cls {
        Value::Class(c) => {
            if let Some(meta) = c.metaclass.borrow().clone() {
                if let Some(check) = meta.lookup("__instancecheck__") {
                    if matches!(check, Value::Func(_)) {
                        let r = vm.call(&check, vec![cls.clone(), v.clone()])?;
                        return vm.truthy(&r);
                    }
                }
            }
            Ok(vm.isinstance(v, c))
        }
        Value::Tuple(t) => {
            for c in t.iter() {
                if class_matches(vm, v, c, fname)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err(type_err(format!(
            "{fname}() arg 2 must be a type, a tuple of types, or a union"
        ))),
    }
}

fn b_isinstance(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "isinstance", 2, 2)?;
    Ok(Value::Bool(class_matches(
        vm,
        &a.args[0],
        &a.args[1],
        "isinstance",
    )?))
}
fn b_issubclass(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "issubclass", 2, 2)?;
    let Value::Class(c) = &a.args[0] else {
        return Err(type_err("issubclass() arg 1 must be a class"));
    };
    fn check(vm: &mut Vm, c: &Rc<Class>, target: &Value) -> PyResult<bool> {
        match target {
            Value::Class(t) => {
                if let Some(meta) = t.metaclass.borrow().clone() {
                    if let Some(sc) = meta.lookup("__subclasscheck__") {
                        if matches!(sc, Value::Func(_)) {
                            let r = vm.call(&sc, vec![target.clone(), Value::Class(c.clone())])?;
                            return vm.truthy(&r);
                        }
                    }
                }
                Ok(c.is_subclass(t))
            }
            Value::Tuple(ts) => {
                for t in ts.iter() {
                    if check(vm, c, t)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Err(type_err(
                "issubclass() arg 2 must be a class, a tuple of classes, or a union",
            )),
        }
    }
    Ok(Value::Bool(check(vm, c, &a.args[1])?))
}
fn b_callable(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "callable", 1, 1)?;
    let v = &a.args[0];
    Ok(Value::Bool(match v {
        Value::Func(_) | Value::Builtin(_) | Value::Method(_) | Value::Class(_) => true,
        _ => vm.lookup_special(v, "__call__").is_some(),
    }))
}
fn b_chr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "chr", 1, 1)?;
    let i = to_int_arg(vm, &a.args[0])?;
    let c = u32::try_from(i)
        .ok()
        .filter(|u| *u <= 0x10ffff)
        .ok_or_else(|| value_err("chr() arg not in range(0x110000)"))?;
    match char::from_u32(c) {
        Some(ch) => Ok(Value::string(ch.to_string())),
        None => Ok(Value::string('\u{fffd}'.to_string())),
    }
}
fn b_ord(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "ord", 1, 1)?;
    match &a.args[0] {
        Value::Str(s) => {
            if s.nchars != 1 {
                return Err(type_err(format!(
                    "ord() expected a character, but string of length {} found",
                    s.nchars
                )));
            }
            Ok(Value::Int(s.s.chars().next().unwrap() as i64))
        }
        Value::Bytes(b) => {
            if b.len() != 1 {
                return Err(type_err(format!(
                    "ord() expected a character, but string of length {} found",
                    b.len()
                )));
            }
            Ok(Value::Int(b[0] as i64))
        }
        other => Err(type_err(format!(
            "ord() expected string of length 1, but {} found",
            vm.type_name(other)
        ))),
    }
}
fn radix_str(vm: &mut Vm, v: &Value, radix: u32, prefix: &str) -> PyResult<Value> {
    let big = match v {
        Value::Int(i) => BigInt::from_i64(*i),
        Value::Bool(b) => BigInt::from_i64(*b as i64),
        Value::Big(b) => (**b).clone(),
        _ => BigInt::from_i64(vm.index_of(v)?),
    };
    let digits = big.abs().to_str_radix(radix);
    Ok(Value::string(format!(
        "{}{prefix}{digits}",
        if big.is_negative() { "-" } else { "" }
    )))
}
fn b_hex(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "hex", 1, 1)?;
    radix_str(vm, &a.args[0], 16, "0x")
}
fn b_bin(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "bin", 1, 1)?;
    radix_str(vm, &a.args[0], 2, "0b")
}
fn b_oct(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "oct", 1, 1)?;
    radix_str(vm, &a.args[0], 8, "0o")
}
fn b_divmod(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "divmod", 2, 2)?;
    let (x, y) = (&a.args[0], &a.args[1]);
    if matches!(x, Value::Instance(_)) || matches!(y, Value::Instance(_)) {
        if let Some(r) = vm.call_special(x, "__divmod__", vec![y.clone()])? {
            if !matches!(r, Value::NotImplemented) {
                return Ok(r);
            }
        }
        if let Some(r) = vm.call_special(y, "__rdivmod__", vec![x.clone()])? {
            if !matches!(r, Value::NotImplemented) {
                return Ok(r);
            }
        }
    }
    let q = vm.binary_op(x, y, BinOp::FloorDiv).map_err(|e| {
        if vm.err_matches(&e, "TypeError") {
            type_err(format!(
                "unsupported operand type(s) for divmod(): '{}' and '{}'",
                vm.type_name(x),
                vm.type_name(y)
            ))
        } else {
            e
        }
    })?;
    let r = vm.binary_op(x, y, BinOp::Mod)?;
    Ok(Value::tuple(vec![q, r]))
}
fn b_pow(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(&mut a, "pow", &["base", "exp", "mod"], 2)?;
    let (base, exp) = (p[0].clone().unwrap(), p[1].clone().unwrap());
    match &p[2] {
        None | Some(Value::None) => vm.binary_op(&base, &exp, BinOp::Pow),
        Some(m) => {
            let to_big = |v: &Value| -> Option<BigInt> {
                match v {
                    Value::Int(i) => Some(BigInt::from_i64(*i)),
                    Value::Bool(b) => Some(BigInt::from_i64(*b as i64)),
                    Value::Big(b) => Some((**b).clone()),
                    _ => None,
                }
            };
            let (Some(b), Some(e), Some(m)) = (to_big(&base), to_big(&exp), to_big(m)) else {
                return Err(type_err(
                    "pow() 3rd argument not allowed unless all arguments are integers",
                ));
            };
            if m.is_zero() {
                return Err(value_err("pow() 3rd argument cannot be 0"));
            }
            let mut b = b.divmod_floor(&m).1;
            let mut e = e;
            if e.is_negative() {
                // Modular inverse.
                b = mod_inverse(&b, &m)
                    .ok_or_else(|| value_err("base is not invertible for the given modulus"))?;
                e = e.neg();
            }
            let mut result = BigInt::from_i64(1).divmod_floor(&m).1;
            let bits = e.bit_length();
            for i in (0..bits).rev() {
                result = result.mul(&result).divmod_floor(&m).1;
                if e.shr(i).is_odd() {
                    result = result.mul(&b).divmod_floor(&m).1;
                }
            }
            Ok(Value::big(result))
        }
    }
}
fn mod_inverse(a: &BigInt, m: &BigInt) -> Option<BigInt> {
    let (mut old_r, mut r) = (a.clone(), m.abs());
    let (mut old_s, mut s) = (BigInt::from_i64(1), BigInt::zero());
    while !r.is_zero() {
        let (q, _) = old_r.divmod_floor(&r);
        let nr = old_r.sub(&q.mul(&r));
        old_r = std::mem::replace(&mut r, nr);
        let ns = old_s.sub(&q.mul(&s));
        old_s = std::mem::replace(&mut s, ns);
    }
    if old_r != BigInt::from_i64(1) {
        return None;
    }
    Some(old_s.divmod_floor(m).1)
}

pub fn round_float(f: f64, nd: i64) -> f64 {
    if !f.is_finite() {
        return f;
    }
    if nd > 300 {
        return f;
    }
    if nd >= 0 {
        let s = format!("{:.*}", nd as usize, f);
        return s.parse().unwrap_or(f);
    }
    // Negative ndigits: round to a power of ten, half-even on the exact value.
    let p = 10f64.powi((-nd) as i32);
    if p.is_infinite() {
        return 0.0f64.copysign(f);
    }
    let y = f / p;
    let r = y.round_ties_even();
    let r = if (y - y.trunc()).abs() == 0.5 {
        r
    } else {
        y.round()
    };
    r * p
}

fn b_round(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(&mut a, "round", &["number", "ndigits"], 1)?;
    let x = p[0].clone().unwrap();
    let nd = p[1].clone().filter(|v| !v.is_none());
    match (&x, &nd) {
        (Value::Float(f), None) => {
            let r = f.round_ties_even();
            float_to_int(r)
        }
        (Value::Float(f), Some(n)) => {
            let n = to_int_arg(vm, n)?;
            Ok(Value::Float(round_float(*f, n)))
        }
        (Value::Int(_) | Value::Big(_) | Value::Bool(_), None) => Ok(match x {
            Value::Bool(b) => Value::Int(b as i64),
            other => other,
        }),
        (Value::Int(_) | Value::Big(_) | Value::Bool(_), Some(n)) => {
            let n = to_int_arg(vm, n)?;
            if n >= 0 {
                return Ok(match x {
                    Value::Bool(b) => Value::Int(b as i64),
                    other => other,
                });
            }
            let big = match &x {
                Value::Int(i) => BigInt::from_i64(*i),
                Value::Bool(b) => BigInt::from_i64(*b as i64),
                Value::Big(b) => (**b).clone(),
                _ => unreachable!(),
            };
            let p = BigInt::from_i64(10).pow((-n) as u64);
            let (q, r) = big.divmod_floor(&p);
            // Round half to even on the exact remainder.
            let twice = r.shl(1);
            let q = match twice.cmp(&p) {
                std::cmp::Ordering::Greater => q.add(&BigInt::from_i64(1)),
                std::cmp::Ordering::Equal if q.is_odd() => q.add(&BigInt::from_i64(1)),
                _ => q,
            };
            Ok(Value::big(q.mul(&p)))
        }
        _ => {
            let mut args = vec![];
            if let Some(n) = nd {
                args.push(n.clone());
            }
            if let Some(r) = vm.call_special(&x, "__round__", args)? {
                return Ok(r);
            }
            Err(type_err(format!(
                "type {} doesn't define __round__ method",
                vm.type_name(&x)
            )))
        }
    }
}
fn b_id(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "id", 1, 1)?;
    Ok(Value::Int(vm.object_id(&a.args[0]) as i64))
}
fn b_hash(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "hash", 1, 1)?;
    Ok(Value::Int(vm.hash(&a.args[0])?))
}
fn b_iter(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "iter", 1, 2)?;
    if a.args.len() == 2 {
        return Ok(Value::Iter(new_ref(IterObj::Callable {
            func: a.args[0].clone(),
            sentinel: a.args[1].clone(),
        })));
    }
    vm.get_iter(&a.args[0])
}
fn b_next(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "next", 1, 2)?;
    let it = &a.args[0];
    let is_iter = matches!(it, Value::Iter(_) | Value::Gen(_) | Value::File(_))
        || vm.lookup_special(it, "__next__").is_some();
    if !is_iter {
        return Err(type_err(format!(
            "'{}' object is not an iterator",
            vm.type_name(it)
        )));
    }
    // A generator's return value travels in StopIteration.value.
    if let Value::Gen(g) = it {
        return match vm.gen_resume(g, Value::None, None)? {
            GenResult::Yielded(v) => Ok(v),
            GenResult::Returned(r) => match a.args.get(1) {
                Some(d) => Ok(d.clone()),
                None => Err(err_args(
                    "StopIteration",
                    if r.is_none() { vec![] } else { vec![r] },
                )),
            },
        };
    }
    match vm.next(it)? {
        Some(v) => Ok(v),
        None => match a.args.get(1) {
            Some(d) => Ok(d.clone()),
            None => Err(err_args("StopIteration", vec![])),
        },
    }
}
fn b_getattr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "getattr", 2, 3)?;
    let name = match &a.args[1] {
        Value::Str(s) => s.clone(),
        other => {
            return Err(type_err(format!(
                "attribute name must be string, not '{}'",
                vm.type_name(other)
            )))
        }
    };
    match vm.getattr(&a.args[0], &name) {
        Ok(v) => Ok(v),
        Err(e) if a.args.len() == 3 && vm.err_matches(&e, "AttributeError") => {
            Ok(a.args[2].clone())
        }
        Err(e) => Err(e),
    }
}
fn b_setattr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "setattr", 3, 3)?;
    let name = to_str_arg(vm, &a.args[1], "attribute name")?;
    vm.setattr(&a.args[0], &name, a.args[2].clone())?;
    Ok(Value::None)
}
fn b_hasattr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "hasattr", 2, 2)?;
    let name = to_str_arg(vm, &a.args[1], "attribute name")?;
    match vm.getattr(&a.args[0], &name) {
        Ok(_) => Ok(Value::Bool(true)),
        Err(e) if vm.err_matches(&e, "AttributeError") => Ok(Value::Bool(false)),
        Err(e) => Err(e),
    }
}
fn b_delattr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "delattr", 2, 2)?;
    let name = to_str_arg(vm, &a.args[1], "attribute name")?;
    vm.delattr(&a.args[0], &name)?;
    Ok(Value::None)
}
fn b_vars(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "vars", 1, 1)?;
    match vm.getattr_str(&a.args[0], "__dict__") {
        Ok(d) => Ok(d),
        Err(_) => Err(type_err("vars() argument must have __dict__ attribute")),
    }
}
pub fn dir_of(vm: &mut Vm, v: &Value) -> PyResult<Vec<String>> {
    let mut names: Vec<String> = vec![];
    match v {
        Value::Module(m) => {
            names.extend(
                m.dict
                    .borrow()
                    .keys()
                    .iter()
                    .filter_map(|k| k.as_pystr().map(|s| s.s.clone())),
            );
        }
        Value::Class(c) => {
            for k in c.mro.borrow().iter() {
                names.extend(
                    k.dict
                        .borrow()
                        .keys()
                        .iter()
                        .filter_map(|k| k.as_pystr().map(|s| s.s.clone())),
                );
            }
        }
        _ => {
            if let Value::Instance(i) = v {
                names.extend(
                    i.dict
                        .borrow()
                        .keys()
                        .iter()
                        .filter_map(|k| k.as_pystr().map(|s| s.s.clone())),
                );
            }
            let cls = vm.type_of(v);
            for k in cls.mro.borrow().iter() {
                names.extend(
                    k.dict
                        .borrow()
                        .keys()
                        .iter()
                        .filter_map(|k| k.as_pystr().map(|s| s.s.clone())),
                );
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}
fn b_dir(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "dir", 0, 1)?;
    let Some(v) = a.args.first() else {
        return Err(err("SystemError", "dir() without arguments needs a frame"));
    };
    if let Some(r) = vm.call_special(v, "__dir__", vec![])? {
        if !matches!(v, Value::Class(_) | Value::Module(_)) {
            let items = vm.iterate(&r)?;
            let out = sort_values(vm, items, None, false)?;
            return Ok(Value::list(out));
        }
    }
    let names = dir_of(vm, v)?;
    Ok(Value::list(names.iter().map(|n| Value::str(n)).collect()))
}
fn b_globals(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Err(err("SystemError", "globals() needs a frame"))
}
fn b_locals(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Err(err("SystemError", "locals() needs a frame"))
}
fn b_format(vm: &mut Vm, a: Args) -> PyResult<Value> {
    arity(&a, "format", 1, 2)?;
    let spec = match a.args.get(1) {
        Some(Value::Str(s)) => s.s.clone(),
        Some(other) => {
            return Err(type_err(format!(
                "format() argument 2 must be str, not {}",
                vm.type_name(other)
            )))
        }
        None => String::new(),
    };
    Ok(Value::string(vm.format(&a.args[0], &spec)?))
}
fn b_exec(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Err(err("SystemError", "exec() needs a frame"))
}
fn b_eval(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Err(err("SystemError", "eval() needs a frame"))
}
fn b_compile(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(
        &mut a,
        "compile",
        &[
            "source",
            "filename",
            "mode",
            "flags",
            "dont_inherit",
            "optimize",
        ],
        3,
    )?;
    let src = to_str_arg(vm, p[0].as_ref().unwrap(), "compile() arg 1")?;
    let filename = to_str_arg(vm, p[1].as_ref().unwrap(), "compile() arg 2")?;
    let mode = to_str_arg(vm, p[2].as_ref().unwrap(), "compile() arg 3")?;
    let code = crate::compile_source(vm, &src.s, &filename.s, &mode.s)?;
    Ok(Value::Code(code))
}
fn b_import(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(
        &mut a,
        "__import__",
        &["name", "globals", "locals", "fromlist", "level"],
        1,
    )?;
    let name = to_str_arg(vm, p[0].as_ref().unwrap(), "__import__() argument 1")?;
    let fromlist = p[3].clone().unwrap_or(Value::None);
    let level = match &p[4] {
        Some(Value::Int(l)) => *l as usize,
        _ => 0,
    };
    let globals = match &p[1] {
        Some(Value::Dict(d)) => d.clone(),
        _ => new_ref(Dict::new()),
    };
    vm.import(&name.s, &fromlist, level, &globals)
}
fn b_exit(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let _ = vm;
    Err(err_args("SystemExit", a.args))
}
fn b_help(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let text = match a.args.first() {
        Some(v) => format!("Help on {}:\n\n", vm.repr(v)?),
        None => "Type help() for interactive help, or help(object) for help about object.\n".into(),
    };
    vm.write_stdout(&text);
    Ok(Value::None)
}
fn b_breakpoint(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::None)
}

// ---------------------------------------------------------------------------
// Class creation
// ---------------------------------------------------------------------------

fn b_build_class(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    if a.args.len() < 2 {
        return Err(type_err("__build_class__: not enough arguments"));
    }
    let func = a.args.remove(0);
    let name = a.args.remove(0);
    let Value::Func(func) = func else {
        return Err(type_err("__build_class__: func must be a function"));
    };
    let name = to_str_arg(vm, &name, "__build_class__: name")?;
    let mut bases = vec![];
    for b in a.args.drain(..) {
        match &b {
            Value::Class(_) => bases.push(b),
            _ => {
                // __mro_entries__ (typing.Generic[T], NamedTuple…)
                if let Some(me) = vm.getattr_opt(&b, "__mro_entries__")? {
                    let r = vm.call(&me, vec![Value::tuple(vec![])])?;
                    bases.extend(vm.iterate(&r)?);
                } else {
                    return Err(type_err(format!(
                        "bases must be types, not {}",
                        vm.type_name(&b)
                    )));
                }
            }
        }
    }
    let explicit_meta = a.kw("metaclass");
    let kwds = std::mem::take(&mut a.kwargs);
    // Most derived metaclass among bases.
    let mut meta: Option<Rc<Class>> = match &explicit_meta {
        Some(Value::Class(c)) => Some(c.clone()),
        _ => None,
    };
    for b in &bases {
        if let Value::Class(bc) = b {
            let bm = vm.type_of(b);
            let _ = bc;
            match &meta {
                None => meta = Some(bm),
                Some(m) => {
                    if bm.is_subclass(m) {
                        meta = Some(bm);
                    }
                }
            }
        }
    }
    let meta = meta.unwrap_or_else(|| vm.t.type_.clone());
    let ns = match meta.lookup("__prepare__") {
        Some(p) if !Rc::ptr_eq(&meta, &vm.t.type_) => {
            let pf = match p {
                Value::ClassMethod(f) => (*f).clone(),
                other => other,
            };
            let r = vm.call_kw(
                &pf,
                vec![
                    Value::Class(meta.clone()),
                    Value::Str(name.clone()),
                    Value::tuple(bases.clone()),
                ],
                kwds.clone(),
            )?;
            match r {
                Value::Dict(d) => d,
                _ => new_ref(Dict::new()),
            }
        }
        _ => new_ref(Dict::new()),
    };
    // Run the class body with the namespace as its locals.
    let mut frame = vm.new_frame(func.code.clone(), func.globals.clone(), Some(ns.clone()));
    frame.cells.extend(func.closure.iter().cloned());
    vm.charge_depth()?;
    let cell = match vm.execute(frame, None)? {
        Exit::Return(v) => v,
        Exit::Yield(..) => Value::None,
    };
    let cls = if Rc::ptr_eq(&meta, &vm.t.type_) {
        type_new(vm, &meta, &name.s, bases, &ns, &kwds)?
    } else {
        let v = vm.call_kw(
            &Value::Class(meta.clone()),
            vec![
                Value::Str(name.clone()),
                Value::tuple(bases),
                Value::Dict(ns),
            ],
            kwds,
        )?;
        v
    };
    if let Value::Cell(c) = cell {
        *c.borrow_mut() = cls.clone();
    }
    Ok(cls)
}

/// `type.__new__(meta, name, bases, ns)`.
pub fn type_new(
    vm: &mut Vm,
    meta: &Rc<Class>,
    name: &str,
    bases: Vec<Value>,
    ns: &Ref<Dict>,
    kwds: &[(Rc<str>, Value)],
) -> PyResult<Value> {
    let mut base_classes: Vec<Rc<Class>> = vec![];
    for b in bases {
        match b {
            Value::Class(c) => {
                let final_type = matches!(
                    &*c.name(),
                    "bool"
                        | "NoneType"
                        | "range"
                        | "slice"
                        | "function"
                        | "builtin_function_or_method"
                        | "method"
                        | "generator"
                        | "coroutine"
                        | "ellipsis"
                        | "NotImplementedType"
                        | "cell"
                        | "code"
                );
                if c.builtin && final_type {
                    return Err(type_err(format!(
                        "type '{}' is not an acceptable base type",
                        c.name()
                    )));
                }
                base_classes.push(c)
            }
            other => {
                return Err(type_err(format!(
                    "bases must be types, not {}",
                    vm.type_name(&other)
                )))
            }
        }
    }
    if base_classes.is_empty() {
        base_classes.push(vm.t.object.clone());
    }
    let kind = base_classes
        .iter()
        .map(|b| b.kind)
        .find(|k| *k != Kind::Object)
        .unwrap_or(Kind::Object);
    let mut dict = ns.borrow().clone();
    let qualname = dict
        .del_str("__qualname__")
        .and_then(|q| q.as_pystr().map(|s| s.s.clone()))
        .unwrap_or_else(|| name.to_string());
    if dict.contains_str("__eq__") && !dict.contains_str("__hash__") {
        dict.set_str("__hash__", Value::None);
    }
    for special in ["__init_subclass__", "__class_getitem__"] {
        if let Some(f @ Value::Func(_)) = dict.get_str(special) {
            dict.set_str(special, Value::ClassMethod(Rc::new(f)));
        }
    }
    if let Some(f @ Value::Func(_)) = dict.get_str("__new__") {
        dict.set_str("__new__", Value::StaticMethod(Rc::new(f)));
    }
    let cls = Rc::new(Class {
        name: RefCell::new(name.into()),
        qualname: RefCell::new(qualname.into()),
        bases: RefCell::new(base_classes.clone()),
        mro: RefCell::new(vec![]),
        dict: new_ref(dict),
        kind,
        builtin: false,
        metaclass: RefCell::new(if Rc::ptr_eq(meta, &vm.t.type_) {
            None
        } else {
            Some(meta.clone())
        }),
        abstract_methods: RefCell::new(vec![]),
    });
    let mro = c3(&cls, &base_classes).map_err(type_err)?;
    *cls.mro.borrow_mut() = mro;
    vm.classes.push(Rc::downgrade(&cls));
    // Abstract methods (enforced for ABCMeta-derived metaclasses).
    let is_abc = meta.mro.borrow().iter().any(|m| &*m.name() == "ABCMeta");
    if is_abc {
        let mut abstract_names: Vec<Rc<str>> = vec![];
        let own: Vec<(Value, Value)> = cls.dict.borrow().items();
        for (k, v) in &own {
            if is_abstract(vm, v)? {
                if let Value::Str(s) = k {
                    abstract_names.push(s.s.as_str().into());
                }
            }
        }
        for b in &base_classes {
            for m in b.abstract_methods.borrow().iter() {
                if let Some(v) = cls.lookup(m) {
                    if is_abstract(vm, &v)? && !abstract_names.contains(m) {
                        abstract_names.push(m.clone());
                    }
                }
            }
        }
        *cls.abstract_methods.borrow_mut() = abstract_names;
    }
    let cls_v = Value::Class(cls.clone());
    // __set_name__ on descriptors.
    let items = cls.dict.borrow().items();
    for (k, v) in items {
        if let Value::Instance(_) = &v {
            if let Some(sn) = vm.lookup_special(&v, "__set_name__") {
                vm.call(&sn, vec![v.clone(), cls_v.clone(), k.clone()])?;
            }
        }
    }
    // __init_subclass__ of the parent.
    let mro = cls.mro.borrow().clone();
    for c in &mro[1..] {
        let found = c.dict.borrow().get_str("__init_subclass__");
        if let Some(isc) = found {
            let f = match isc {
                Value::ClassMethod(f) => (*f).clone(),
                other => other,
            };
            if !matches!(f, Value::Builtin(_)) {
                vm.call_kw(&f, vec![cls_v.clone()], kwds.to_vec())?;
            } else if !kwds.is_empty() {
                return Err(type_err(format!(
                    "{}.__init_subclass__() takes no keyword arguments",
                    c.name()
                )));
            }
            break;
        }
    }
    Ok(cls_v)
}

fn is_abstract(vm: &mut Vm, v: &Value) -> PyResult<bool> {
    let r = match v {
        Value::Func(f) => f.dict.borrow().get_str("__isabstractmethod__"),
        Value::StaticMethod(f) | Value::ClassMethod(f) => match &**f {
            Value::Func(f) => f.dict.borrow().get_str("__isabstractmethod__"),
            _ => None,
        },
        Value::Property(p) => match &p.fget {
            Value::Func(f) => f.dict.borrow().get_str("__isabstractmethod__"),
            _ => None,
        },
        _ => None,
    };
    match r {
        Some(v) => vm.truthy(&v),
        None => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Constructors (`__new__` of builtin types)
// ---------------------------------------------------------------------------

fn cls_arg(a: &mut Args) -> Rc<Class> {
    match a.args.remove(0) {
        Value::Class(c) => c,
        _ => unreachable!(),
    }
}

/// Wraps a builtin value for a subclass instance.
pub fn wrap_for(vm: &mut Vm, cls: &Rc<Class>, builtin: &Rc<Class>, v: Value) -> Value {
    if Rc::ptr_eq(cls, builtin) {
        return v;
    }
    let inst = vm.new_instance(cls);
    if let Value::Instance(i) = &inst {
        *i.native.borrow_mut() = NativeData::Base(v);
    }
    inst
}

fn object_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    if a.args.is_empty() {
        return Err(type_err("object.__new__(): not enough arguments"));
    }
    let cls = match a.args.remove(0) {
        Value::Class(c) => c,
        other => {
            return Err(type_err(format!(
                "object.__new__(X): X is not a type object ({})",
                vm.type_name(&other)
            )))
        }
    };
    if cls.builtin && cls.kind != Kind::Object {
        return Err(type_err(format!(
            "object.__new__({}) is not safe, use {}.__new__()",
            cls.name(),
            cls.name()
        )));
    }
    if !cls.abstract_methods.borrow().is_empty() {
        let mut names: Vec<String> = cls
            .abstract_methods
            .borrow()
            .iter()
            .map(|s| format!("'{s}'"))
            .collect();
        names.sort();
        return Err(type_err(format!(
            "Can't instantiate abstract class {} with abstract method{} {}",
            cls.name(),
            if names.len() == 1 { "" } else { "s" },
            names.join(", ")
        )));
    }
    Ok(vm.new_instance(&cls))
}

fn exception_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    let v = vm.new_instance(&cls);
    let args = std::mem::take(&mut a.args);
    vm.exc_data(&v, |d| d.args = Value::tuple(args));
    Ok(v)
}

fn type_new_builtin(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let meta = cls_arg(&mut a);
    if a.args.len() == 1 {
        return Ok(Value::Class(vm.type_of(&a.args[0])));
    }
    if a.args.len() != 3 {
        return Err(type_err("type() takes 1 or 3 arguments"));
    }
    let name = to_str_arg(vm, &a.args[0], "type.__new__() argument 1")?;
    let bases = vm.iterate(&a.args[1])?;
    let ns = match &a.args[2] {
        Value::Dict(d) => new_ref(d.borrow().clone()),
        other => {
            return Err(type_err(format!(
                "type.__new__() argument 3 must be dict, not {}",
                vm.type_name(other)
            )))
        }
    };
    let kwds = std::mem::take(&mut a.kwargs);
    type_new(vm, &meta, &name.s, bases, &ns, &kwds)
}

pub fn parse_int_str(vm: &Vm, s: &str, base: u32) -> Option<Value> {
    let t = s.trim_matches(|c: char| c.is_whitespace());
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let mut base = base;
    let mut body = body.to_string();
    let lower = body.to_ascii_lowercase();
    let prefixed = |p: &str| lower.starts_with(p);
    if (base == 16 && prefixed("0x"))
        || (base == 8 && prefixed("0o"))
        || (base == 2 && prefixed("0b"))
    {
        body = body[2..].to_string();
        if body.starts_with('_') {
            body.remove(0);
        }
    } else if base == 0 {
        if prefixed("0x") {
            base = 16;
            body = body[2..].to_string();
        } else if prefixed("0o") {
            base = 8;
            body = body[2..].to_string();
        } else if prefixed("0b") {
            base = 2;
            body = body[2..].to_string();
        } else {
            base = 10;
            if body.len() > 1 && body.starts_with('0') && body.chars().any(|c| c != '0' && c != '_')
            {
                return None;
            }
        }
        if body.starts_with('_') {
            body.remove(0);
        }
    }
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    let digits: String = body.chars().filter(|c| *c != '_').collect();
    if !digits.chars().all(|c| c.is_digit(base)) {
        return None;
    }
    let _ = vm;
    let v = BigInt::parse_digits(&digits, base)?;
    Some(Value::big(if neg { v.neg() } else { v }))
}

pub fn int_from(vm: &mut Vm, x: &Value, base: Option<i64>) -> PyResult<Value> {
    if let Some(b) = base {
        let s = match x {
            Value::Str(s) => s.s.clone(),
            Value::Bytes(b) => String::from_utf8_lossy(b).into_owned(),
            Value::ByteArray(b) => String::from_utf8_lossy(&b.borrow()).into_owned(),
            _ => {
                return Err(type_err(
                    "int() can't convert non-string with explicit base",
                ))
            }
        };
        if b != 0 && !(2..=36).contains(&b) {
            return Err(value_err("int() base must be >= 2 and <= 36, or 0"));
        }
        return parse_int_str(vm, &s, b as u32).ok_or_else(|| {
            value_err(format!(
                "invalid literal for int() with base {b}: {}",
                crate::format::str_repr(&s)
            ))
        });
    }
    match x {
        Value::Int(_) | Value::Big(_) => Ok(x.clone()),
        Value::Bool(b) => Ok(Value::Int(*b as i64)),
        Value::Float(f) => float_to_int(*f),
        Value::Str(s) => {
            let digits = s.s.chars().filter(|c| c.is_ascii_digit()).count();
            if vm.int_max_str_digits > 0 && digits > vm.int_max_str_digits {
                return Err(value_err(format!(
                    "Exceeds the limit ({}) for integer string conversion: value has {} digits; use sys.set_int_max_str_digits() to increase the limit",
                    vm.int_max_str_digits, digits
                )));
            }
            parse_int_str(vm, &s.s, 10).ok_or_else(|| {
                value_err(format!(
                    "invalid literal for int() with base 10: {}",
                    crate::format::str_repr(&s.s)
                ))
            })
        }
        Value::Bytes(b) => {
            let s = String::from_utf8_lossy(b).into_owned();
            parse_int_str(vm, &s, 10).ok_or_else(|| {
                value_err(format!(
                    "invalid literal for int() with base 10: {}",
                    crate::format::bytes_repr(b)
                ))
            })
        }
        _ => {
            for m in ["__int__", "__index__", "__trunc__"] {
                if let Some(r) = vm.call_special(x, m, vec![])? {
                    return Ok(match r {
                        Value::Bool(b) => Value::Int(b as i64),
                        other => other,
                    });
                }
            }
            if let Value::Instance(i) = x {
                if let NativeData::Base(b) = &*i.native.borrow() {
                    let b = b.clone();
                    return int_from(vm, &b, None);
                }
            }
            Err(type_err(format!(
                "int() argument must be a string, a bytes-like object or a real number, not '{}'",
                vm.type_name(x)
            )))
        }
    }
}

fn int_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    let p = take_params(&mut a, "int", &["x", "base"], 0)?;
    let base = match &p[1] {
        Some(b) => Some(to_int_arg(vm, b)?),
        None => None,
    };
    let v = match &p[0] {
        None => {
            if base.is_some() {
                return Err(type_err("int() missing string argument"));
            }
            Value::Int(0)
        }
        Some(x) => int_from(vm, x, base)?,
    };
    let int = vm.t.int.clone();
    Ok(wrap_for(vm, &cls, &int, v))
}
fn bool_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    arity(&a, "bool", 0, 1)?;
    Ok(Value::Bool(match a.args.first() {
        Some(v) => vm.truthy(v)?,
        None => false,
    }))
}
pub fn parse_float_str(s: &str) -> Option<f64> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let (sign, body) = match lower.strip_prefix('-') {
        Some(r) => (-1.0, r),
        None => (1.0, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    match body {
        "inf" | "infinity" => return Some(sign * f64::INFINITY),
        "nan" => return Some(f64::NAN),
        _ => {}
    }
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    if !body
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | '+' | '-' | '_'))
    {
        return None;
    }
    // Underscores only between digits.
    let chars: Vec<char> = body.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if *c == '_'
            && !(i > 0
                && chars[i - 1].is_ascii_digit()
                && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()))
        {
            return None;
        }
    }
    let clean: String = body.chars().filter(|c| *c != '_').collect();
    if clean == "." || clean.starts_with('e') {
        return None;
    }
    clean.parse::<f64>().ok().map(|f| sign * f)
}
pub fn float_from(vm: &mut Vm, x: &Value) -> PyResult<f64> {
    match x {
        Value::Float(f) => Ok(*f),
        Value::Int(i) => Ok(*i as f64),
        Value::Bool(b) => Ok(*b as i64 as f64),
        Value::Big(b) => b
            .to_f64()
            .ok_or_else(|| err("OverflowError", "int too large to convert to float")),
        Value::Str(s) => parse_float_str(&s.s).ok_or_else(|| {
            value_err(format!(
                "could not convert string to float: {}",
                crate::format::str_repr(&s.s)
            ))
        }),
        Value::Bytes(b) => parse_float_str(&String::from_utf8_lossy(b)).ok_or_else(|| {
            value_err(format!(
                "could not convert string to float: {}",
                crate::format::bytes_repr(b)
            ))
        }),
        _ => {
            if let Some(r) = vm.call_special(x, "__float__", vec![])? {
                return match r {
                    Value::Float(f) => Ok(f),
                    _ => Err(type_err("__float__ returned non-float")),
                };
            }
            if let Some(r) = vm.call_special(x, "__index__", vec![])? {
                return float_from(vm, &r);
            }
            if let Value::Instance(i) = x {
                if let NativeData::Base(b) = &*i.native.borrow() {
                    let b = b.clone();
                    return float_from(vm, &b);
                }
            }
            Err(type_err(format!(
                "float() argument must be a string or a real number, not '{}'",
                vm.type_name(x)
            )))
        }
    }
}
fn float_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    arity(&a, "float", 0, 1)?;
    let f = match a.args.first() {
        Some(x) => float_from(vm, x)?,
        None => 0.0,
    };
    let float = vm.t.float.clone();
    Ok(wrap_for(vm, &cls, &float, Value::Float(f)))
}
fn complex_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let p = take_params(&mut a, "complex", &["real", "imag"], 0)?;
    let part = |vm: &mut Vm, v: &Option<Value>| -> PyResult<(f64, f64)> {
        match v {
            None => Ok((0.0, 0.0)),
            Some(Value::Complex(r, i)) => Ok((*r, *i)),
            Some(Value::Str(s)) => {
                let t = s.s.trim().trim_start_matches('(').trim_end_matches(')');
                if let Some(body) = t.strip_suffix('j').or_else(|| t.strip_suffix('J')) {
                    // a+bj or bj
                    let split = body
                        .char_indices()
                        .skip(1)
                        .filter(|(i, c)| {
                            (*c == '+' || *c == '-') && !body[..*i].ends_with(['e', 'E'])
                        })
                        .map(|(i, _)| i)
                        .last();
                    let (re, im) = match split {
                        Some(i) => (&body[..i], &body[i..]),
                        None => ("0", body),
                    };
                    let im = match im {
                        "" | "+" => "1",
                        "-" => "-1",
                        x => x,
                    };
                    let r = parse_float_str(re)
                        .ok_or_else(|| value_err("complex() arg is a malformed string"))?;
                    let i = parse_float_str(im)
                        .ok_or_else(|| value_err("complex() arg is a malformed string"))?;
                    return Ok((r, i));
                }
                let r = parse_float_str(t)
                    .ok_or_else(|| value_err("complex() arg is a malformed string"))?;
                Ok((r, 0.0))
            }
            Some(x) => Ok((float_from(vm, x)?, 0.0)),
        }
    };
    let (r1, i1) = part(vm, &p[0])?;
    let (r2, i2) = part(vm, &p[1])?;
    Ok(Value::Complex(r1 - i2, i1 + r2))
}
pub fn decode_bytes(b: &[u8], encoding: &str, errors: &str) -> PyResult<String> {
    let enc = encoding.to_ascii_lowercase().replace('_', "-");
    match enc.as_str() {
        "utf-8" | "utf8" => match std::str::from_utf8(b) {
            Ok(s) => Ok(s.to_string()),
            Err(e) => {
                if errors == "replace" || errors == "ignore" {
                    let s = String::from_utf8_lossy(b).into_owned();
                    return Ok(if errors == "ignore" {
                        s.replace('\u{fffd}', "")
                    } else {
                        s
                    });
                }
                let pos = e.valid_up_to();
                Err(err_args(
                    "UnicodeDecodeError",
                    vec![
                        Value::str("utf-8"),
                        Value::Bytes(Rc::new(b.to_vec())),
                        Value::Int(pos as i64),
                        Value::Int(pos as i64 + 1),
                        Value::str(if b[pos] >= 0x80 && b[pos] < 0xc0 {
                            "invalid start byte"
                        } else {
                            "invalid continuation byte"
                        }),
                    ],
                ))
            }
        },
        "ascii" | "us-ascii" => {
            if let Some(pos) = b.iter().position(|c| *c >= 0x80) {
                if errors == "ignore" {
                    return Ok(b
                        .iter()
                        .filter(|c| **c < 0x80)
                        .map(|c| *c as char)
                        .collect());
                }
                if errors == "replace" {
                    return Ok(b
                        .iter()
                        .map(|c| if *c < 0x80 { *c as char } else { '\u{fffd}' })
                        .collect());
                }
                return Err(err_args(
                    "UnicodeDecodeError",
                    vec![
                        Value::str("ascii"),
                        Value::Bytes(Rc::new(b.to_vec())),
                        Value::Int(pos as i64),
                        Value::Int(pos as i64 + 1),
                        Value::str("ordinal not in range(128)"),
                    ],
                ));
            }
            Ok(b.iter().map(|c| *c as char).collect())
        }
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => {
            Ok(b.iter().map(|c| *c as char).collect())
        }
        _ => Err(err("LookupError", format!("unknown encoding: {encoding}"))),
    }
}
pub fn encode_str(s: &str, encoding: &str, errors: &str) -> PyResult<Vec<u8>> {
    let enc = encoding.to_ascii_lowercase().replace('_', "-");
    match enc.as_str() {
        "utf-8" | "utf8" => Ok(s.as_bytes().to_vec()),
        "ascii" | "us-ascii" | "latin-1" | "latin1" | "iso-8859-1" => {
            let limit = if enc.contains("ascii") { 0x80 } else { 0x100 };
            let mut out = vec![];
            for (i, c) in s.chars().enumerate() {
                if (c as u32) < limit {
                    out.push(c as u32 as u8);
                } else if errors == "ignore" {
                } else if errors == "replace" {
                    out.push(b'?');
                } else {
                    return Err(err(
                        "UnicodeEncodeError",
                        format!(
                            "'{}' codec can't encode character '\\u{:04x}' in position {i}: ordinal not in range({limit})",
                            if limit == 0x80 { "ascii" } else { "latin-1" },
                            c as u32
                        ),
                    ));
                }
            }
            Ok(out)
        }
        _ => Err(err("LookupError", format!("unknown encoding: {encoding}"))),
    }
}
fn str_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    let p = take_params(&mut a, "str", &["object", "encoding", "errors"], 0)?;
    let s = match (&p[0], &p[1]) {
        (None, _) => String::new(),
        (Some(Value::Bytes(b)), _) if p[1].is_some() || p[2].is_some() => {
            let enc = enc_opt(&p[1]).unwrap_or_else(|| "utf-8".into());
            let errors = enc_opt(&p[2]).unwrap_or_else(|| "strict".into());
            decode_bytes(b, &enc, &errors)?
        }
        (Some(v), _) => vm.str_of(v)?,
    };
    let str_ = vm.t.str_.clone();
    Ok(wrap_for(vm, &cls, &str_, Value::string(s)))
}
fn enc_opt(v: &Option<Value>) -> Option<String> {
    match v {
        Some(Value::Str(s)) => Some(s.s.clone()),
        _ => None,
    }
}
pub fn bytes_from(
    vm: &mut Vm,
    x: Option<&Value>,
    enc: Option<String>,
    errors: Option<String>,
) -> PyResult<Vec<u8>> {
    Ok(match x {
        None => vec![],
        Some(Value::Str(s)) => {
            let enc = enc.ok_or_else(|| type_err("string argument without an encoding"))?;
            encode_str(&s.s, &enc, &errors.unwrap_or_else(|| "strict".into()))?
        }
        Some(Value::Int(n)) => {
            if *n < 0 {
                return Err(value_err("negative count"));
            }
            vec![0; *n as usize]
        }
        Some(Value::Bytes(b)) => (**b).clone(),
        Some(Value::ByteArray(b)) => b.borrow().clone(),
        Some(other) => {
            let items = vm.iterate(other).map_err(|_| {
                type_err(format!(
                    "cannot convert '{}' object to bytes",
                    vm.type_name(other)
                ))
            })?;
            let mut out = vec![];
            for it in items {
                let i = to_int_arg(vm, &it)?;
                if !(0..256).contains(&i) {
                    return Err(value_err("bytes must be in range(0, 256)"));
                }
                out.push(i as u8);
            }
            out
        }
    })
}
fn bytes_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let p = take_params(&mut a, "bytes", &["source", "encoding", "errors"], 0)?;
    let b = bytes_from(vm, p[0].as_ref(), enc_opt(&p[1]), enc_opt(&p[2]))?;
    Ok(Value::Bytes(Rc::new(b)))
}
fn bytearray_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let p = take_params(&mut a, "bytearray", &["source", "encoding", "errors"], 0)?;
    let b = bytes_from(vm, p[0].as_ref(), enc_opt(&p[1]), enc_opt(&p[2]))?;
    Ok(Value::ByteArray(new_ref(b)))
}
fn list_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    let list = vm.t.list.clone();
    if Rc::ptr_eq(&cls, &list) {
        arity(&a, "list", 0, 1)?;
        let items = match a.args.first() {
            Some(v) => vm.iterate(v)?,
            None => vec![],
        };
        return Ok(Value::list(items));
    }
    Ok(wrap_for(vm, &cls, &list, Value::list(vec![])))
}
fn tuple_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    arity(&a, "tuple", 0, 1)?;
    let items = match a.args.first() {
        Some(Value::Tuple(t)) => return Ok(wrap_tuple(vm, &cls, (**t).clone())),
        Some(v) => vm.iterate(v)?,
        None => vec![],
    };
    Ok(wrap_tuple(vm, &cls, items))
}
fn wrap_tuple(vm: &mut Vm, cls: &Rc<Class>, items: Vec<Value>) -> Value {
    let tuple = vm.t.tuple.clone();
    wrap_for(vm, cls, &tuple, Value::tuple(items))
}
pub fn dict_update(
    vm: &mut Vm,
    d: &Ref<Dict>,
    src: Option<&Value>,
    kwargs: Vec<(Rc<str>, Value)>,
) -> PyResult<()> {
    if let Some(src) = src {
        let is_mapping = matches!(src, Value::Dict(_))
            || (!matches!(src, Value::List(_) | Value::Tuple(_)) && vm.hasattr(src, "keys"));
        if is_mapping {
            for (k, v) in vm.mapping_items(src)? {
                vm.dict_set(d, k, v)?;
            }
        } else {
            let it = vm.get_iter(src)?;
            let mut i = 0;
            while let Some(item) = vm.next(&it)? {
                let pair = vm.iterate(&item).map_err(|_| {
                    type_err(format!(
                        "cannot convert dictionary update sequence element #{i} to a sequence"
                    ))
                })?;
                if pair.len() != 2 {
                    return Err(value_err(format!(
                        "dictionary update sequence element #{i} has length {}; 2 is required",
                        pair.len()
                    )));
                }
                let mut p = pair.into_iter();
                let (k, v) = (p.next().unwrap(), p.next().unwrap());
                vm.dict_set(d, k, v)?;
                i += 1;
            }
        }
    }
    for (k, v) in kwargs {
        vm.dict_set(d, Value::str(&k), v)?;
    }
    Ok(())
}
fn dict_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    let dict = vm.t.dict.clone();
    if Rc::ptr_eq(&cls, &dict) {
        if a.args.len() > 1 {
            return Err(type_err(format!(
                "dict expected at most 1 argument, got {}",
                a.args.len()
            )));
        }
        let d = new_ref(Dict::new());
        let kwargs = std::mem::take(&mut a.kwargs);
        dict_update(vm, &d, a.args.first(), kwargs)?;
        return Ok(Value::Dict(d));
    }
    Ok(wrap_for(vm, &cls, &dict, Value::dict(Dict::new())))
}
fn set_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    let set = vm.t.set.clone();
    if Rc::ptr_eq(&cls, &set) {
        arity(&a, "set", 0, 1)?;
        let data = match a.args.first() {
            Some(v) => build_set(vm, v)?,
            None => SetData::new(),
        };
        return Ok(Value::Set(new_ref(data)));
    }
    Ok(wrap_for(
        vm,
        &cls,
        &set,
        Value::Set(new_ref(SetData::new())),
    ))
}
fn frozenset_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = cls_arg(&mut a);
    arity(&a, "frozenset", 0, 1)?;
    let data = match a.args.first() {
        Some(v) => build_set(vm, v)?,
        None => SetData::new(),
    };
    let fs = vm.t.frozenset.clone();
    Ok(wrap_for(vm, &cls, &fs, Value::FrozenSet(Rc::new(data))))
}
fn range_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    if !a.kwargs.is_empty() {
        return Err(type_err("range() takes no keyword arguments"));
    }
    let mut ints = vec![];
    for v in &a.args {
        ints.push(match v {
            Value::Int(i) => *i,
            Value::Bool(b) => *b as i64,
            Value::Float(_) => {
                return Err(type_err(
                    "'float' object cannot be interpreted as an integer",
                ))
            }
            other => vm.index_of(other)?,
        });
    }
    let (start, stop, step) = match ints.len() {
        1 => (0, ints[0], 1),
        2 => (ints[0], ints[1], 1),
        3 => (ints[0], ints[1], ints[2]),
        0 => return Err(type_err("range expected at least 1 argument, got 0")),
        n => {
            return Err(type_err(format!(
                "range expected at most 3 arguments, got {n}"
            )))
        }
    };
    if step == 0 {
        return Err(value_err("range() arg 3 must not be zero"));
    }
    Ok(Value::Range(Rc::new(RangeObj { start, stop, step })))
}
fn slice_new(_vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let v = match a.args.len() {
        1 => [Value::None, a.args[0].clone(), Value::None],
        2 => [a.args[0].clone(), a.args[1].clone(), Value::None],
        3 => [a.args[0].clone(), a.args[1].clone(), a.args[2].clone()],
        n => {
            return Err(type_err(format!(
                "slice expected at least 1 argument, got {n}"
            )))
        }
    };
    Ok(Value::Slice(Rc::new(v)))
}
fn enumerate_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let p = take_params(&mut a, "enumerate", &["iterable", "start"], 1)?;
    let it = vm.get_iter(p[0].as_ref().unwrap())?;
    let start = p[1].clone().unwrap_or(Value::Int(0));
    Ok(Value::Iter(new_ref(IterObj::Enumerate {
        it,
        count: start,
    })))
}
fn zip_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let strict = match a.kw("strict") {
        Some(v) => vm.truthy(&v)?,
        None => false,
    };
    let mut its = vec![];
    for (i, v) in a.args.iter().enumerate() {
        its.push(
            vm.get_iter(v)
                .map_err(|_| type_err(format!("zip argument #{} must support iteration", i + 1)))?,
        );
    }
    Ok(Value::Iter(new_ref(IterObj::Zip { its, strict })))
}
fn map_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    if a.args.len() < 2 {
        return Err(type_err("map() must have at least two arguments."));
    }
    let func = a.args.remove(0);
    let mut its = vec![];
    for v in &a.args {
        its.push(vm.get_iter(v)?);
    }
    Ok(Value::Iter(new_ref(IterObj::Map { func, its })))
}
fn filter_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    arity(&a, "filter", 2, 2)?;
    let it = vm.get_iter(&a.args[1])?;
    Ok(Value::Iter(new_ref(IterObj::Filter {
        func: a.args[0].clone(),
        it,
    })))
}
fn reversed_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    arity(&a, "reversed", 1, 1)?;
    let v = a.args[0].clone();
    if let Some(r) = vm.call_special(&v, "__reversed__", vec![])? {
        return Ok(r);
    }
    let n = match &v {
        Value::List(_) | Value::Tuple(_) | Value::Str(_) | Value::Range(_) => vm.len(&v)?,
        Value::Dict(d) => {
            let mut keys = d.borrow().keys();
            keys.reverse();
            return Ok(Value::Iter(new_ref(IterObj::List {
                items: keys,
                idx: 0,
            })));
        }
        Value::DictView(_) => {
            let mut items = vm.iterate(&v)?;
            items.reverse();
            return Ok(Value::Iter(new_ref(IterObj::List { items, idx: 0 })));
        }
        Value::Native(_) => {
            let mut items = vm.iterate(&v)?;
            items.reverse();
            return Ok(Value::Iter(new_ref(IterObj::List { items, idx: 0 })));
        }
        _ => {
            if vm.lookup_special(&v, "__getitem__").is_some()
                && vm.lookup_special(&v, "__len__").is_some()
            {
                vm.len(&v)?
            } else {
                return Err(type_err(format!(
                    "'{}' object is not reversible",
                    vm.type_name(&v)
                )));
            }
        }
    };
    Ok(Value::Iter(new_ref(IterObj::Reversed {
        seq: v,
        idx: n as isize - 1,
    })))
}
fn property_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    let p = take_params(&mut a, "property", &["fget", "fset", "fdel", "doc"], 0)?;
    let fget = p[0].clone().unwrap_or(Value::None);
    let doc = match &p[3] {
        Some(d) => d.clone(),
        None => match &fget {
            Value::Func(f) => f.doc.borrow().clone(),
            _ => Value::None,
        },
    };
    let _ = vm;
    Ok(Value::Property(Rc::new(Property {
        fget,
        fset: p[1].clone().unwrap_or(Value::None),
        fdel: p[2].clone().unwrap_or(Value::None),
        doc,
    })))
}
fn staticmethod_new(_vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    arity(&a, "staticmethod", 1, 1)?;
    Ok(Value::StaticMethod(Rc::new(a.args[0].clone())))
}
fn classmethod_new(_vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    arity(&a, "classmethod", 1, 1)?;
    Ok(Value::ClassMethod(Rc::new(a.args[0].clone())))
}
fn super_new(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let _ = cls_arg(&mut a);
    match a.args.len() {
        0 => Err(err("RuntimeError", "super(): no arguments")),
        1 | 2 => {
            let Value::Class(c) = &a.args[0] else {
                return Err(type_err(format!(
                    "super() argument 1 must be a type, not {}",
                    vm.type_name(&a.args[0])
                )));
            };
            let obj = a.args.get(1).cloned().unwrap_or(Value::None);
            if a.args.len() == 2 {
                let ok = match &obj {
                    Value::Class(oc) => oc.is_subclass(c),
                    other => vm.isinstance(other, c),
                };
                if !ok {
                    return Err(type_err(
                        "super(type, obj): obj must be an instance or subtype of type",
                    ));
                }
            }
            Ok(Value::Super(Rc::new((c.clone(), obj))))
        }
        n => Err(type_err(format!(
            "super() takes at most 2 arguments ({n} given)"
        ))),
    }
}
fn none_new(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::None)
}
