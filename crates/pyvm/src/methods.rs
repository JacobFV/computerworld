//! Methods of the builtin types (str, list, dict, set, int, float, …).
use crate::ast::{BinOp, CmpOp};
use crate::bfuncs::*;
use crate::bigint::BigInt;
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

fn this(vm: &Vm, a: &Args) -> Value {
    vm.base_value(&a.args[0])
}
fn arg(a: &Args, i: usize) -> Option<&Value> {
    a.args.get(i)
}
fn nargs(a: &Args, name: &str, min: usize, max: usize) -> PyResult<()> {
    let n = a.args.len().saturating_sub(1);
    if !a.kwargs.is_empty() {
        return Err(type_err(format!("{name}() takes no keyword arguments")));
    }
    if n < min || n > max {
        if min == max {
            return Err(type_err(match min {
                0 => format!("{name}() takes no arguments ({n} given)"),
                1 => format!("{name}() takes exactly one argument ({n} given)"),
                _ => format!("{name}() takes exactly {min} arguments ({n} given)"),
            }));
        }
        if n < min {
            return Err(type_err(format!(
                "{name}() takes at least {min} argument{} ({n} given)",
                if min == 1 { "" } else { "s" }
            )));
        }
        return Err(type_err(format!(
            "{name}() takes at most {max} argument{} ({n} given)",
            if max == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

pub fn as_list(vm: &Vm, v: &Value) -> PyResult<Ref<Vec<Value>>> {
    match vm.base_value(v) {
        Value::List(l) => Ok(l),
        other => Err(type_err(format!(
            "descriptor requires a 'list' object but received a '{}'",
            vm.type_name(&other)
        ))),
    }
}
pub fn as_dict(vm: &Vm, v: &Value) -> PyResult<Ref<Dict>> {
    match vm.base_value(v) {
        Value::Dict(d) => Ok(d),
        other => Err(type_err(format!(
            "descriptor requires a 'dict' object but received a '{}'",
            vm.type_name(&other)
        ))),
    }
}
fn as_str(vm: &Vm, v: &Value) -> PyResult<Rc<PyStr>> {
    match vm.base_value(v) {
        Value::Str(s) => Ok(s),
        other => Err(type_err(format!(
            "descriptor requires a 'str' object but received a '{}'",
            vm.type_name(&other)
        ))),
    }
}
fn str_arg(vm: &Vm, v: &Value, fname: &str, pos: usize) -> PyResult<Rc<PyStr>> {
    match vm.base_value(v) {
        Value::Str(s) => Ok(s),
        other => Err(type_err(format!(
            "{fname}() argument {pos} must be str, not {}",
            vm.type_name(&other)
        ))),
    }
}

/// Both operands unwrapped to builtin values, or None if either is a plain user object.
fn unwrap2(vm: &Vm, a: &Args) -> Option<(Value, Value)> {
    let x = vm.base_value(&a.args[0]);
    let y = vm.base_value(a.args.get(1)?);
    if matches!(x, Value::Instance(_)) || matches!(y, Value::Instance(_)) {
        return None;
    }
    Some((x, y))
}

macro_rules! binop_dunders {
    ($($f:ident, $rf:ident, $op:expr;)*) => {
        $(
            fn $f(vm: &mut Vm, a: Args) -> PyResult<Value> {
                match unwrap2(vm, &a) {
                    Some((x, y)) => vm.native_binop(&x, &y, $op),
                    None => Ok(Value::NotImplemented),
                }
            }
            fn $rf(vm: &mut Vm, a: Args) -> PyResult<Value> {
                match unwrap2(vm, &a) {
                    Some((x, y)) => vm.native_binop(&y, &x, $op),
                    None => Ok(Value::NotImplemented),
                }
            }
        )*
    };
}
binop_dunders! {
    d_add, d_radd, BinOp::Add;
    d_sub, d_rsub, BinOp::Sub;
    d_mul, d_rmul, BinOp::Mul;
    d_truediv, d_rtruediv, BinOp::Div;
    d_floordiv, d_rfloordiv, BinOp::FloorDiv;
    d_mod, d_rmod, BinOp::Mod;
    d_pow, d_rpow, BinOp::Pow;
    d_lshift, d_rlshift, BinOp::LShift;
    d_rshift, d_rrshift, BinOp::RShift;
    d_and, d_rand, BinOp::BitAnd;
    d_or, d_ror, BinOp::BitOr;
    d_xor, d_rxor, BinOp::BitXor;
}

macro_rules! cmp_dunders {
    ($($f:ident, $op:expr;)*) => {
        $(
            fn $f(vm: &mut Vm, a: Args) -> PyResult<Value> {
                match unwrap2(vm, &a) {
                    Some((x, y)) => {
                        let same_kind = std::mem::discriminant(&x) == std::mem::discriminant(&y)
                            || (vm.as_num(&x).is_some() && vm.as_num(&y).is_some());
                        if !same_kind && !matches!($op, CmpOp::Eq | CmpOp::NotEq) {
                            let sets = crate::builtins::set_data_of(&x).is_some() && crate::builtins::set_data_of(&y).is_some();
                            if !sets {
                                return Ok(Value::NotImplemented);
                            }
                        }
                        vm.rich_compare(&x, &y, $op)
                    }
                    None => Ok(Value::NotImplemented),
                }
            }
        )*
    };
}
cmp_dunders! {
    d_eq, CmpOp::Eq;
    d_ne, CmpOp::NotEq;
    d_lt, CmpOp::Lt;
    d_le, CmpOp::LtE;
    d_gt, CmpOp::Gt;
    d_ge, CmpOp::GtE;
}

fn d_len(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::Int(vm.len(&v)? as i64))
}
fn d_hash(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::Int(vm.hash(&v)?))
}
fn d_repr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    if let Value::Instance(_) = v {
        return Ok(Value::string(vm.default_repr(&v)));
    }
    Ok(Value::string(vm.repr(&v)?))
}
fn d_str(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::string(vm.str_of(&v)?))
}
fn d_iter(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    vm.get_iter(&v)
}
fn d_contains(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::Bool(vm.contains(&v, &a.args[1])?))
}
fn d_getitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "__getitem__", 1, 1)?;
    let v = this(vm, &a);
    if let Value::Dict(d) = &v {
        if let Some(r) = vm.dict_get(d, &a.args[1])? {
            return Ok(r);
        }
        // dict subclasses may define __missing__.
        if let Value::Instance(_) = &a.args[0] {
            if let Some(m) = vm.lookup_special(&a.args[0], "__missing__") {
                return vm.call(&m, vec![a.args[0].clone(), a.args[1].clone()]);
            }
        }
        return Err(err_args("KeyError", vec![a.args[1].clone()]));
    }
    vm.getitem(&v, &a.args[1])
}
fn d_setitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "__setitem__", 2, 2)?;
    let v = this(vm, &a);
    vm.setitem(&v, &a.args[1], a.args[2].clone())?;
    Ok(Value::None)
}
fn d_delitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "__delitem__", 1, 1)?;
    let v = this(vm, &a);
    vm.delitem(&v, &a.args[1])?;
    Ok(Value::None)
}
fn d_bool(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::Bool(vm.truthy(&v)?))
}
fn d_neg(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    vm.unary_op(&v, crate::ast::UnaryOp::Neg)
}
fn d_pos(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    vm.unary_op(&v, crate::ast::UnaryOp::Pos)
}
fn d_invert(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    vm.unary_op(&v, crate::ast::UnaryOp::Invert)
}
fn d_abs(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    let abs = vm.builtins.borrow().get_str("abs").unwrap();
    vm.call(&abs, vec![v])
}
fn d_int(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    int_from(vm, &v, None)
}
fn d_float(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::Float(float_from(vm, &v)?))
}
fn d_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(match this(vm, &a) {
        Value::Bool(b) => Value::Int(b as i64),
        other => other,
    })
}
fn d_format(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    let spec = match a.args.get(1) {
        Some(Value::Str(s)) => s.s.clone(),
        _ => String::new(),
    };
    if let Value::Instance(_) = v {
        if !spec.is_empty() {
            return Err(type_err(format!(
                "unsupported format string passed to {}.__format__",
                vm.type_name(&v)
            )));
        }
        return Ok(Value::string(vm.str_of(&v)?));
    }
    Ok(Value::string(crate::format::format_value(vm, &v, &spec)?))
}
fn d_round(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    let round = vm.builtins.borrow().get_str("round").unwrap();
    let mut args = vec![v];
    args.extend(a.args[1..].iter().cloned());
    vm.call(&round, args)
}
fn d_trunc(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    int_from(vm, &v, None)
}
fn d_floor(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match this(vm, &a) {
        Value::Float(f) => float_to_int(f.floor()),
        other => int_from(vm, &other, None),
    }
}
fn d_ceil(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match this(vm, &a) {
        Value::Float(f) => float_to_int(f.ceil()),
        other => int_from(vm, &other, None),
    }
}
fn d_divmod(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match unwrap2(vm, &a) {
        Some((x, y)) => {
            let q = vm.native_binop(&x, &y, BinOp::FloorDiv)?;
            if matches!(q, Value::NotImplemented) {
                return Ok(q);
            }
            let r = vm.native_binop(&x, &y, BinOp::Mod)?;
            Ok(Value::tuple(vec![q, r]))
        }
        None => Ok(Value::NotImplemented),
    }
}

fn add_common_dunders(c: &Rc<Class>, numeric: bool, container: bool) {
    for (n, f) in [
        ("__eq__", d_eq as NativeFn),
        ("__ne__", d_ne),
        ("__lt__", d_lt),
        ("__le__", d_le),
        ("__gt__", d_gt),
        ("__ge__", d_ge),
        ("__hash__", d_hash),
        ("__repr__", d_repr),
        ("__format__", d_format),
    ] {
        add_fn(c, n, f);
    }
    if numeric {
        for (n, f) in [
            ("__add__", d_add as NativeFn),
            ("__radd__", d_radd),
            ("__sub__", d_sub),
            ("__rsub__", d_rsub),
            ("__mul__", d_mul),
            ("__rmul__", d_rmul),
            ("__truediv__", d_truediv),
            ("__rtruediv__", d_rtruediv),
            ("__floordiv__", d_floordiv),
            ("__rfloordiv__", d_rfloordiv),
            ("__mod__", d_mod),
            ("__rmod__", d_rmod),
            ("__pow__", d_pow),
            ("__rpow__", d_rpow),
            ("__neg__", d_neg),
            ("__pos__", d_pos),
            ("__abs__", d_abs),
            ("__bool__", d_bool),
            ("__int__", d_int),
            ("__float__", d_float),
            ("__round__", d_round),
            ("__trunc__", d_trunc),
            ("__floor__", d_floor),
            ("__ceil__", d_ceil),
            ("__divmod__", d_divmod),
        ] {
            add_fn(c, n, f);
        }
    }
    if container {
        for (n, f) in [
            ("__len__", d_len as NativeFn),
            ("__iter__", d_iter),
            ("__contains__", d_contains),
            ("__getitem__", d_getitem),
        ] {
            add_fn(c, n, f);
        }
    }
}

pub fn install(vm: &mut Vm) {
    let t = &vm.t;
    // object
    let o = &t.object;
    add_fn(o, "__init__", object_init);
    add_fn(o, "__str__", object_str);
    add_fn(o, "__repr__", object_repr);
    add_fn(o, "__eq__", object_eq);
    add_fn(o, "__ne__", object_ne);
    add_fn(o, "__lt__", object_notimpl);
    add_fn(o, "__le__", object_notimpl);
    add_fn(o, "__gt__", object_notimpl);
    add_fn(o, "__ge__", object_notimpl);
    add_fn(o, "__hash__", object_hash);
    add_fn(o, "__setattr__", object_setattr_fn);
    add_fn(o, "__getattribute__", object_getattribute);
    add_fn(o, "__delattr__", object_delattr);
    add_fn(o, "__format__", object_format);
    add_fn(o, "__dir__", object_dir);
    add_fn(o, "__reduce_ex__", object_notimpl);
    add_fn(o, "__sizeof__", object_sizeof);
    add_classmethod(o, "__init_subclass__", object_init_subclass);
    // Methods are named "object.__str__" etc.; the VM recognizes these by name.
    let tt = &t.type_;
    add_fn(tt, "mro", type_mro);
    add_fn(tt, "__subclasses__", type_subclasses);
    add_fn(tt, "__instancecheck__", type_instancecheck);
    add_fn(tt, "__subclasscheck__", type_subclasscheck);
    add_fn(tt, "__call__", type_call);
    add_fn(tt, "__repr__", d_repr);
    add_fn(tt, "__or__", type_or);
    add_fn(tt, "__ror__", type_or);
    add_fn(tt, "__init__", object_init_any);

    // numbers
    for c in [&t.int, &t.bool_, &t.float, &t.complex] {
        add_common_dunders(c, true, false);
    }
    for c in [&t.int, &t.bool_] {
        for (n, f) in [
            ("__lshift__", d_lshift as NativeFn),
            ("__rlshift__", d_rlshift),
            ("__rshift__", d_rshift),
            ("__rrshift__", d_rrshift),
            ("__and__", d_and),
            ("__rand__", d_rand),
            ("__or__", d_or),
            ("__ror__", d_ror),
            ("__xor__", d_xor),
            ("__rxor__", d_rxor),
            ("__invert__", d_invert),
            ("__index__", d_index),
            ("bit_length", int_bit_length),
            ("bit_count", int_bit_count),
            ("to_bytes", int_to_bytes),
            ("as_integer_ratio", int_ratio),
            ("conjugate", d_pos),
            ("is_integer", int_is_integer),
        ] {
            add_fn(c, n, f);
        }
        add_classmethod(c, "from_bytes", int_from_bytes);
    }
    let f = &t.float;
    add_fn(f, "is_integer", float_is_integer);
    add_fn(f, "as_integer_ratio", float_ratio);
    add_fn(f, "hex", float_hex);
    add_fn(f, "conjugate", d_pos);
    add_classmethod(f, "fromhex", float_fromhex);
    add_fn(&t.complex, "conjugate", complex_conjugate);

    // str
    let s = &t.str_;
    add_common_dunders(s, false, true);
    for (n, fnc) in [
        ("__add__", d_add as NativeFn),
        ("__mul__", d_mul),
        ("__rmul__", d_rmul),
        ("__mod__", d_mod),
        ("__str__", str_str),
        ("capitalize", str_capitalize),
        ("casefold", str_casefold),
        ("center", str_center),
        ("count", str_count),
        ("encode", str_encode),
        ("endswith", str_endswith),
        ("expandtabs", str_expandtabs),
        ("find", str_find),
        ("format", str_format_m),
        ("format_map", str_format_map),
        ("index", str_index),
        ("isalnum", str_isalnum),
        ("isalpha", str_isalpha),
        ("isascii", str_isascii),
        ("isdecimal", str_isdecimal),
        ("isdigit", str_isdigit),
        ("isidentifier", str_isidentifier),
        ("islower", str_islower),
        ("isnumeric", str_isnumeric),
        ("isprintable", str_isprintable),
        ("isspace", str_isspace),
        ("istitle", str_istitle),
        ("isupper", str_isupper),
        ("join", str_join),
        ("ljust", str_ljust),
        ("lower", str_lower),
        ("lstrip", str_lstrip),
        ("partition", str_partition),
        ("removeprefix", str_removeprefix),
        ("removesuffix", str_removesuffix),
        ("replace", str_replace),
        ("rfind", str_rfind),
        ("rindex", str_rindex),
        ("rjust", str_rjust),
        ("rpartition", str_rpartition),
        ("rsplit", str_rsplit),
        ("rstrip", str_rstrip),
        ("split", str_split),
        ("splitlines", str_splitlines),
        ("startswith", str_startswith),
        ("strip", str_strip),
        ("swapcase", str_swapcase),
        ("title", str_title),
        ("translate", str_translate),
        ("upper", str_upper),
        ("zfill", str_zfill),
    ] {
        add_fn(s, n, fnc);
    }
    add_static(s, "maketrans", str_maketrans);

    // bytes / bytearray
    for c in [&t.bytes, &t.bytearray] {
        add_common_dunders(c, false, true);
        for (n, fnc) in [
            ("__add__", d_add as NativeFn),
            ("__mul__", d_mul),
            ("decode", bytes_decode),
            ("hex", bytes_hex),
            ("startswith", bytes_startswith),
            ("endswith", bytes_endswith),
            ("find", bytes_find),
            ("count", bytes_count),
            ("join", bytes_join),
            ("split", bytes_split),
            ("strip", bytes_strip),
            ("replace", bytes_replace),
            ("upper", bytes_upper),
            ("lower", bytes_lower),
            ("index", bytes_index),
        ] {
            add_fn(c, n, fnc);
        }
        add_classmethod(c, "fromhex", bytes_fromhex);
    }
    for (n, fnc) in [
        ("append", bytearray_append as NativeFn),
        ("extend", bytearray_extend),
        ("pop", bytearray_pop),
        ("__setitem__", d_setitem),
        ("__delitem__", d_delitem),
    ] {
        add_fn(&t.bytearray, n, fnc);
    }

    // list
    let l = &t.list;
    add_common_dunders(l, false, true);
    for (n, fnc) in [
        ("__init__", list_init as NativeFn),
        ("__add__", d_add),
        ("__mul__", d_mul),
        ("__rmul__", d_rmul),
        ("__iadd__", list_iadd),
        ("__setitem__", d_setitem),
        ("__delitem__", d_delitem),
        ("__reversed__", list_reversed),
        ("append", list_append),
        ("extend", list_extend),
        ("insert", list_insert),
        ("remove", list_remove),
        ("pop", list_pop),
        ("clear", list_clear),
        ("index", list_index),
        ("count", list_count),
        ("sort", list_sort),
        ("reverse", list_reverse),
        ("copy", list_copy),
    ] {
        add_fn(l, n, fnc);
    }
    l.dict.borrow_mut().set_str("__hash__", Value::None);

    // tuple
    let tu = &t.tuple;
    add_common_dunders(tu, false, true);
    for (n, fnc) in [
        ("__add__", d_add as NativeFn),
        ("__mul__", d_mul),
        ("__rmul__", d_rmul),
        ("index", tuple_index),
        ("count", tuple_count),
    ] {
        add_fn(tu, n, fnc);
    }

    // dict
    let d = &t.dict;
    add_common_dunders(d, false, true);
    for (n, fnc) in [
        ("__init__", dict_init as NativeFn),
        ("__setitem__", d_setitem),
        ("__delitem__", d_delitem),
        ("__or__", d_or),
        ("__ror__", d_ror),
        ("__ior__", dict_ior),
        ("__reversed__", dict_reversed),
        ("keys", dict_keys),
        ("values", dict_values),
        ("items", dict_items),
        ("get", dict_get),
        ("setdefault", dict_setdefault),
        ("pop", dict_pop),
        ("popitem", dict_popitem),
        ("update", dict_update_m),
        ("clear", dict_clear),
        ("copy", dict_copy),
    ] {
        add_fn(d, n, fnc);
    }
    add_classmethod(d, "fromkeys", dict_fromkeys);
    d.dict.borrow_mut().set_str("__hash__", Value::None);

    // set / frozenset
    for c in [&t.set, &t.frozenset] {
        add_common_dunders(c, false, true);
        for (n, fnc) in [
            ("__or__", d_or as NativeFn),
            ("__and__", d_and),
            ("__sub__", d_sub),
            ("__xor__", d_xor),
            ("__ror__", d_ror),
            ("__rand__", d_rand),
            ("__rsub__", d_rsub),
            ("__rxor__", d_rxor),
            ("union", set_union),
            ("intersection", set_intersection),
            ("difference", set_difference),
            ("symmetric_difference", set_symmetric_difference),
            ("issubset", set_issubset),
            ("issuperset", set_issuperset),
            ("isdisjoint", set_isdisjoint),
            ("copy", set_copy),
        ] {
            add_fn(c, n, fnc);
        }
    }
    let st = &t.set;
    for (n, fnc) in [
        ("__init__", set_init as NativeFn),
        ("add", set_add_m),
        ("remove", set_remove),
        ("discard", set_discard_m),
        ("pop", set_pop),
        ("clear", set_clear),
        ("update", set_update),
        ("intersection_update", set_intersection_update),
        ("difference_update", set_difference_update),
        (
            "symmetric_difference_update",
            set_symmetric_difference_update,
        ),
    ] {
        add_fn(st, n, fnc);
    }
    st.dict.borrow_mut().set_str("__hash__", Value::None);

    // range
    let r = &t.range;
    add_common_dunders(r, false, true);
    add_fn(r, "index", range_index);
    add_fn(r, "count", range_count);
    add_fn(r, "__reversed__", range_reversed);

    // iterators and generators
    for c in [
        &t.list_iterator,
        &t.tuple_iterator,
        &t.str_iterator,
        &t.range_iterator,
        &t.set_iterator,
        &t.dict_keyiterator,
        &t.dict_valueiterator,
        &t.dict_itemiterator,
        &t.enumerate,
        &t.zip,
        &t.map,
        &t.filter,
        &t.reversed,
        &t.generator,
    ] {
        add_fn(c, "__next__", iter_next);
        add_fn(c, "__iter__", iter_self);
    }
    for c in [&t.generator, &t.coroutine] {
        add_fn(c, "send", gen_send);
        add_fn(c, "throw", gen_throw);
        add_fn(c, "close", gen_close);
    }
    add_fn(&t.coroutine, "__await__", iter_self);
    for c in [&t.dict_keys, &t.dict_values, &t.dict_items] {
        add_fn(c, "__len__", d_len);
        add_fn(c, "__iter__", d_iter);
        add_fn(c, "__contains__", d_contains);
        add_fn(c, "__repr__", d_repr);
        add_fn(c, "__eq__", d_eq);
        add_fn(c, "isdisjoint", set_isdisjoint);
        add_fn(c, "__reversed__", dictview_reversed);
    }
    for c in [&t.dict_keys, &t.dict_items] {
        for (n, fnc) in [
            ("__or__", d_or as NativeFn),
            ("__and__", d_and),
            ("__sub__", d_sub),
            ("__xor__", d_xor),
        ] {
            add_fn(c, n, fnc);
        }
    }
    // property
    let p = &t.property;
    add_fn(p, "getter", property_getter);
    add_fn(p, "setter", property_setter);
    add_fn(p, "deleter", property_deleter);
    add_fn(p, "__get__", property_get);
    add_fn(p, "__set__", property_set);
    // functions
    add_fn(&t.function, "__get__", function_get);
    add_fn(&t.function, "__call__", callable_call);
    add_fn(&t.method, "__call__", callable_call);
    add_fn(&t.builtin_function, "__call__", callable_call);
    add_fn(&t.staticmethod, "__get__", staticmethod_get);
    add_fn(&t.classmethod, "__get__", classmethod_get);
    // exceptions
    let be = t.exc("BaseException");
    add_fn(&be, "__init__", exc_init);
    add_fn(&be, "__str__", exc_str_m);
    add_fn(&be, "__repr__", exc_repr);
    add_fn(&be, "with_traceback", exc_with_traceback);
    add_fn(&be, "add_note", exc_add_note);
    add_fn(&t.none_type, "__bool__", none_bool);
    add_fn(&t.none_type, "__repr__", d_repr);
    // files
    crate::io::install(vm);
}

// ---------------------------------------------------------------------------
// object / type
// ---------------------------------------------------------------------------

fn object_init(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() > 1 || !a.kwargs.is_empty() {
        let cls = vm.type_of(&a.args[0]);
        let new_overridden = !matches!(cls.lookup("__new__"), Some(Value::Builtin(b)) if &*b.name == "object.__new__");
        if !new_overridden {
            return Err(type_err(
                "object.__init__() takes exactly one argument (the instance to initialize)",
            ));
        }
    }
    Ok(Value::None)
}
fn object_init_any(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::None)
}
fn object_str(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::string(vm.repr(&a.args[0])?))
}
fn object_repr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::string(vm.default_repr(&a.args[0])))
}
fn object_eq(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(if a.args[0].is(&a.args[1]) {
        Value::Bool(true)
    } else {
        Value::NotImplemented
    })
}
fn object_ne(vm: &mut Vm, a: Args) -> PyResult<Value> {
    // Delegates to __eq__ and inverts, as object.__ne__ does.
    if let Some(eq) = vm.lookup_special(&a.args[0], "__eq__") {
        if !matches!(&eq, Value::Builtin(b) if &*b.name == "object.__eq__") {
            let r = vm.call(&eq, vec![a.args[0].clone(), a.args[1].clone()])?;
            if matches!(r, Value::NotImplemented) {
                return Ok(r);
            }
            return Ok(Value::Bool(!vm.truthy(&r)?));
        }
    }
    Ok(if a.args[0].is(&a.args[1]) {
        Value::Bool(false)
    } else {
        Value::NotImplemented
    })
}
fn object_notimpl(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::NotImplemented)
}
fn object_hash(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Int((vm.object_id(&a.args[0]) >> 4) as i64))
}
fn object_setattr_fn(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() != 3 {
        return Err(type_err("expected 2 arguments"));
    }
    let name = to_str_arg(vm, &a.args[1], "attribute name")?;
    match &a.args[0] {
        Value::Instance(inst) => {
            let inst = inst.clone();
            vm.object_setattr(&a.args[0], &inst, &name, a.args[2].clone())?;
        }
        other => vm.setattr(other, &name, a.args[2].clone())?,
    }
    Ok(Value::None)
}
fn object_getattribute(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let name = to_str_arg(vm, &a.args[1], "attribute name")?;
    vm.getattr(&a.args[0], &name)
}
fn object_delattr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let name = to_str_arg(vm, &a.args[1], "attribute name")?;
    if let Value::Instance(inst) = &a.args[0] {
        if inst.dict.borrow_mut().del_str(&name.s).is_none() {
            return Err(vm.attr_error(&a.args[0], &name.s));
        }
        return Ok(Value::None);
    }
    vm.delattr(&a.args[0], &name)?;
    Ok(Value::None)
}
fn object_format(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let spec = match a.args.get(1) {
        Some(Value::Str(s)) => s.s.clone(),
        _ => String::new(),
    };
    if !spec.is_empty() {
        return Err(type_err(format!(
            "unsupported format string passed to {}.__format__",
            vm.type_name(&a.args[0])
        )));
    }
    Ok(Value::string(vm.str_of(&a.args[0])?))
}
fn object_dir(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let names = dir_of(vm, &a.args[0])?;
    Ok(Value::list(names.iter().map(|n| Value::str(n)).collect()))
}
fn object_sizeof(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::Int(56))
}
fn object_init_subclass(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::None)
}
fn type_mro(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    match &a.args[0] {
        Value::Class(c) => Ok(Value::list(
            c.mro
                .borrow()
                .iter()
                .map(|c| Value::Class(c.clone()))
                .collect(),
        )),
        _ => Err(type_err("mro() requires a type")),
    }
}
fn type_subclasses(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Class(c) = &a.args[0] else {
        return Ok(Value::list(vec![]));
    };
    let mut out = vec![];
    for w in &vm.classes {
        if let Some(sub) = w.upgrade() {
            if sub.bases.borrow().iter().any(|b| Rc::ptr_eq(b, c)) {
                out.push(Value::Class(sub));
            }
        }
    }
    Ok(Value::list(out))
}
fn type_instancecheck(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match &a.args[0] {
        Value::Class(c) => Ok(Value::Bool(vm.isinstance(&a.args[1], c))),
        _ => Ok(Value::Bool(false)),
    }
}
fn type_subclasscheck(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    match (&a.args[0], &a.args[1]) {
        (Value::Class(c), Value::Class(s)) => Ok(Value::Bool(s.is_subclass(c))),
        _ => Ok(Value::Bool(false)),
    }
}
fn type_call(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let cls = a.args.remove(0);
    match &cls {
        Value::Class(c) => {
            // Bypass a metaclass __call__ (this *is* type.__call__).
            let meta = c.metaclass.borrow_mut().take();
            let r = vm.call_class(c, a.args, a.kwargs);
            *c.metaclass.borrow_mut() = meta;
            r
        }
        _ => Err(type_err("descriptor '__call__' requires a 'type' object")),
    }
}
fn type_or(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    // `int | str`: represented as a tuple, which isinstance() accepts.
    let mut items = vec![];
    for v in &a.args[..2] {
        match v {
            Value::Tuple(t) => items.extend(t.iter().cloned()),
            Value::None => items.push(Value::None),
            other => items.push(other.clone()),
        }
    }
    Ok(Value::tuple(items))
}

// ---------------------------------------------------------------------------
// int / float
// ---------------------------------------------------------------------------

fn to_bigint(v: &Value) -> BigInt {
    match v {
        Value::Int(i) => BigInt::from_i64(*i),
        Value::Bool(b) => BigInt::from_i64(*b as i64),
        Value::Big(b) => (**b).clone(),
        _ => BigInt::zero(),
    }
}
fn int_bit_length(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Int(to_bigint(&this(vm, &a)).bit_length() as i64))
}
fn int_bit_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = to_bigint(&this(vm, &a)).abs();
    Ok(Value::Int(
        b.limbs().iter().map(|l| l.count_ones() as i64).sum(),
    ))
}
fn int_is_integer(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::Bool(true))
}
fn int_ratio(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::tuple(vec![
        match v {
            Value::Bool(b) => Value::Int(b as i64),
            o => o,
        },
        Value::Int(1),
    ]))
}
fn int_to_bytes(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let v = to_bigint(&this(vm, &a));
    a.args.remove(0);
    let p = take_params(&mut a, "to_bytes", &["length", "byteorder", "signed"], 0)?;
    let length = match &p[0] {
        Some(l) => to_int_arg(vm, l)? as usize,
        None => 1,
    };
    let order = match &p[1] {
        Some(Value::Str(s)) => s.s.clone(),
        _ => "big".into(),
    };
    let signed = match &p[2] {
        Some(s) => vm.truthy(s)?,
        None => false,
    };
    if v.is_negative() && !signed {
        return Err(err(
            "OverflowError",
            "can't convert negative int to unsigned",
        ));
    }
    let mut bytes = v
        .to_bytes_le(length, signed)
        .ok_or_else(|| err("OverflowError", "int too big to convert"))?;
    if order == "big" {
        bytes.reverse();
    } else if order != "little" {
        return Err(value_err("byteorder must be either 'little' or 'big'"));
    }
    Ok(Value::Bytes(Rc::new(bytes)))
}
fn int_from_bytes(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    a.args.remove(0);
    let p = take_params(&mut a, "from_bytes", &["bytes", "byteorder", "signed"], 1)?;
    let mut bytes = bytes_from(vm, p[0].as_ref(), None, None)?;
    let order = match &p[1] {
        Some(Value::Str(s)) => s.s.clone(),
        _ => "big".into(),
    };
    let signed = match &p[2] {
        Some(s) => vm.truthy(s)?,
        None => false,
    };
    if order == "big" {
        bytes.reverse();
    }
    Ok(Value::big(BigInt::from_bytes_le(&bytes, signed)))
}
fn float_is_integer(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match this(vm, &a) {
        Value::Float(f) => Ok(Value::Bool(f.is_finite() && f.fract() == 0.0)),
        _ => Ok(Value::Bool(false)),
    }
}
fn float_ratio(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Float(f) = this(vm, &a) else {
        return Err(type_err("expected float"));
    };
    if f.is_infinite() {
        return Err(err(
            "OverflowError",
            "cannot convert Infinity to integer ratio",
        ));
    }
    if f.is_nan() {
        return Err(value_err("cannot convert NaN to integer ratio"));
    }
    let (m, e) = frexp(f);
    let mut mant = m;
    let mut exp = e;
    for _ in 0..300 {
        if mant.fract() == 0.0 {
            break;
        }
        mant *= 2.0;
        exp -= 1;
    }
    let num = BigInt::from_f64(mant);
    let (num, den) = if exp > 0 {
        (num.shl(exp as u64), BigInt::from_i64(1))
    } else {
        (num, BigInt::from_i64(1).shl((-exp) as u64))
    };
    Ok(Value::tuple(vec![Value::big(num), Value::big(den)]))
}
fn float_hex(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Float(f) = this(vm, &a) else {
        return Err(type_err("expected float"));
    };
    if f == 0.0 {
        return Ok(Value::str(if f.is_sign_negative() {
            "-0x0.0p+0"
        } else {
            "0x0.0p+0"
        }));
    }
    if !f.is_finite() {
        return Ok(Value::string(crate::format::float_repr(f)));
    }
    let bits = f.to_bits();
    let neg = f < 0.0;
    let exp = ((bits >> 52) & 0x7ff) as i64;
    let mant = bits & ((1 << 52) - 1);
    let (lead, e) = if exp == 0 {
        (0, -1022)
    } else {
        (1, exp - 1023)
    };
    let mut hex = format!("{mant:013x}");
    while hex.len() > 1 && hex.ends_with('0') {
        hex.pop();
    }
    Ok(Value::string(format!(
        "{}0x{lead}.{hex}p{}{}",
        if neg { "-" } else { "" },
        if e >= 0 { "+" } else { "-" },
        e.abs()
    )))
}
fn float_fromhex(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = str_arg(vm, &a.args[1], "fromhex", 1)?;
    let t = s.s.trim().to_ascii_lowercase();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, t.trim_start_matches('+').to_string()),
    };
    let t = t.trim_start_matches("0x");
    let (mant, exp) = match t.split_once('p') {
        Some((m, e)) => (
            m,
            e.parse::<i32>()
                .map_err(|_| value_err("invalid hexadecimal floating-point string"))?,
        ),
        None => (t, 0),
    };
    let (ip, fp) = mant.split_once('.').unwrap_or((mant, ""));
    let mut v = 0f64;
    for c in ip.chars() {
        v = v * 16.0
            + c.to_digit(16)
                .ok_or_else(|| value_err("invalid hexadecimal floating-point string"))?
                as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in fp.chars() {
        v += c
            .to_digit(16)
            .ok_or_else(|| value_err("invalid hexadecimal floating-point string"))?
            as f64
            * scale;
        scale /= 16.0;
    }
    let r = v * 2f64.powi(exp);
    Ok(Value::Float(if neg { -r } else { r }))
}
fn complex_conjugate(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match this(vm, &a) {
        Value::Complex(r, i) => Ok(Value::Complex(r, -i)),
        other => Ok(other),
    }
}

// ---------------------------------------------------------------------------
// str
// ---------------------------------------------------------------------------

fn str_str(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Str(as_str(vm, &a.args[0])?))
}
fn s_ret(s: String) -> PyResult<Value> {
    Ok(Value::string(s))
}
fn str_lower(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "str.lower", 0, 0)?;
    s_ret(as_str(vm, &a.args[0])?.s.to_lowercase())
}
fn str_upper(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "str.upper", 0, 0)?;
    s_ret(as_str(vm, &a.args[0])?.s.to_uppercase())
}
fn str_casefold(vm: &mut Vm, a: Args) -> PyResult<Value> {
    s_ret(as_str(vm, &a.args[0])?.s.to_lowercase().replace('ß', "ss"))
}
fn str_swapcase(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    s_ret(
        s.s.chars()
            .flat_map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().collect::<Vec<_>>()
                } else {
                    c.to_uppercase().collect::<Vec<_>>()
                }
            })
            .collect(),
    )
}
fn str_capitalize(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let mut chars = s.s.chars();
    let out = match chars.next() {
        Some(c) => {
            let first: String = if c.is_lowercase() || c.is_uppercase() {
                title_char(c)
            } else {
                c.to_string()
            };
            format!("{first}{}", chars.as_str().to_lowercase())
        }
        None => String::new(),
    };
    s_ret(out)
}
fn title_char(c: char) -> String {
    match c {
        'ǆ' | 'ǅ' | 'Ǆ' => "ǅ".into(),
        _ => c.to_uppercase().collect(),
    }
}
fn str_title(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let mut out = String::new();
    let mut prev_cased = false;
    for c in s.s.chars() {
        if c.is_alphabetic() {
            if prev_cased {
                out.extend(c.to_lowercase());
            } else {
                out.push_str(&title_char(c));
            }
            prev_cased = true;
        } else {
            out.push(c);
            prev_cased = false;
        }
    }
    s_ret(out)
}
fn str_istitle(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let mut prev_cased = false;
    let mut any = false;
    for c in s.s.chars() {
        if c.is_uppercase() {
            if prev_cased {
                return Ok(Value::Bool(false));
            }
            prev_cased = true;
            any = true;
        } else if c.is_lowercase() {
            if !prev_cased {
                return Ok(Value::Bool(false));
            }
            prev_cased = true;
            any = true;
        } else {
            prev_cased = false;
        }
    }
    Ok(Value::Bool(any))
}
fn pad_str(s: &PyStr, width: i64, fill: char, align: char) -> String {
    let len = s.nchars as i64;
    if width <= len {
        return s.s.clone();
    }
    let marg = (width - len) as usize;
    let (left, right) = match align {
        '<' => (0, marg),
        '>' => (marg, 0),
        _ => {
            let left = marg / 2 + (marg & width as usize & 1);
            (left, marg - left)
        }
    };
    let f = fill.to_string();
    format!("{}{}{}", f.repeat(left), s.s, f.repeat(right))
}
fn fill_arg(vm: &Vm, a: &Args, name: &str) -> PyResult<char> {
    match a.args.get(2) {
        None => Ok(' '),
        Some(v) => match vm.base_value(v) {
            Value::Str(s) if s.nchars == 1 => Ok(s.s.chars().next().unwrap()),
            Value::Str(_) => Err(type_err(
                "The fill character must be exactly one character long",
            )),
            other => Err(type_err(format!(
                "{name}() argument 2 must be str, not {}",
                vm.type_name(&other)
            ))),
        },
    }
}
fn str_center(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "center", 1, 2)?;
    let s = as_str(vm, &a.args[0])?;
    let w = to_int_arg(vm, &a.args[1])?;
    let f = fill_arg(vm, &a, "center")?;
    s_ret(pad_str(&s, w, f, '^'))
}
fn str_ljust(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "ljust", 1, 2)?;
    let s = as_str(vm, &a.args[0])?;
    let w = to_int_arg(vm, &a.args[1])?;
    let f = fill_arg(vm, &a, "ljust")?;
    s_ret(pad_str(&s, w, f, '<'))
}
fn str_rjust(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "rjust", 1, 2)?;
    let s = as_str(vm, &a.args[0])?;
    let w = to_int_arg(vm, &a.args[1])?;
    let f = fill_arg(vm, &a, "rjust")?;
    s_ret(pad_str(&s, w, f, '>'))
}
fn str_zfill(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "zfill", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let w = to_int_arg(vm, &a.args[1])?;
    let len = s.nchars as i64;
    if w <= len {
        return Ok(Value::Str(s));
    }
    let fill = "0".repeat((w - len) as usize);
    let (sign, rest) = match s.s.chars().next() {
        Some(c @ ('+' | '-')) => (c.to_string(), &s.s[1..]),
        _ => (String::new(), &s.s[..]),
    };
    s_ret(format!("{sign}{fill}{rest}"))
}
fn str_expandtabs(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let tab = match a.args.get(1) {
        Some(v) => to_int_arg(vm, v)?,
        None => 8,
    };
    let mut out = String::new();
    let mut col = 0i64;
    for c in s.s.chars() {
        match c {
            '\t' => {
                if tab > 0 {
                    let n = tab - col % tab;
                    out.push_str(&" ".repeat(n as usize));
                    col += n;
                }
            }
            '\n' | '\r' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    s_ret(out)
}

/// Resolves optional start/end character arguments to a byte range of `s`.
fn char_range(vm: &mut Vm, s: &PyStr, a: &Args, from: usize) -> PyResult<(usize, usize, usize)> {
    let len = s.nchars as i64;
    let get = |vm: &mut Vm, i: usize| -> PyResult<Option<i64>> {
        match a.args.get(i) {
            None | Some(Value::None) => Ok(None),
            Some(v) => vm.index_of(v).map(Some),
        }
    };
    let norm = |v: i64| -> i64 {
        if v < 0 {
            (v + len).max(0)
        } else {
            v.min(len)
        }
    };
    let start = get(vm, from)?.map(norm).unwrap_or(0);
    let end = get(vm, from + 1)?.map(norm).unwrap_or(len);
    let bs = s.byte_index(start as usize);
    let be = s.byte_index(end.max(start) as usize);
    Ok((bs, be, start as usize))
}
fn char_pos(s: &str, byte: usize) -> usize {
    if s.is_ascii() {
        byte
    } else {
        s[..byte].chars().count()
    }
}
fn find_impl(vm: &mut Vm, a: &Args, rev: bool, name: &str) -> PyResult<i64> {
    nargs(a, name, 1, 3)?;
    let s = as_str(vm, &a.args[0])?;
    let sub = str_arg(vm, &a.args[1], name, 1)
        .map_err(|_| type_err(format!("must be str, not {}", vm.type_name(&a.args[1]))))?;
    let (bs, be, start_char) = char_range(vm, &s, a, 2)?;
    if be < bs {
        return Ok(-1);
    }
    let hay = &s.s[bs..be];
    let found = if rev {
        hay.rfind(&*sub.s)
    } else {
        hay.find(&*sub.s)
    };
    Ok(match found {
        Some(p) => (start_char + char_pos(hay, p)) as i64,
        None => {
            // An empty needle at the very end of a clipped range.
            -1
        }
    })
}
fn str_find(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Int(find_impl(vm, &a, false, "find")?))
}
fn str_rfind(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Int(find_impl(vm, &a, true, "rfind")?))
}
fn str_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let r = find_impl(vm, &a, false, "index")?;
    if r < 0 {
        return Err(value_err("substring not found"));
    }
    Ok(Value::Int(r))
}
fn str_rindex(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let r = find_impl(vm, &a, true, "rindex")?;
    if r < 0 {
        return Err(value_err("substring not found"));
    }
    Ok(Value::Int(r))
}
fn str_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "count", 1, 3)?;
    let s = as_str(vm, &a.args[0])?;
    let sub = str_arg(vm, &a.args[1], "count", 1)?;
    let (bs, be, _) = char_range(vm, &s, &a, 2)?;
    if be < bs {
        return Ok(Value::Int(0));
    }
    let hay = &s.s[bs..be];
    if sub.s.is_empty() {
        return Ok(Value::Int(hay.chars().count() as i64 + 1));
    }
    Ok(Value::Int(hay.matches(&*sub.s).count() as i64))
}
fn affix_impl(vm: &mut Vm, a: &Args, ends: bool) -> PyResult<Value> {
    let name = if ends { "endswith" } else { "startswith" };
    nargs(a, name, 1, 3)?;
    let s = as_str(vm, &a.args[0])?;
    let (bs, be, _) = char_range(vm, &s, a, 2)?;
    let hay = if be >= bs { &s.s[bs..be] } else { "" };
    let check = |p: &str| {
        if ends {
            hay.ends_with(p)
        } else {
            hay.starts_with(p)
        }
    };
    match vm.base_value(&a.args[1]) {
        Value::Str(p) => Ok(Value::Bool(check(&p.s))),
        Value::Tuple(t) => {
            for p in t.iter() {
                match p {
                    Value::Str(p) => {
                        if check(&p.s) {
                            return Ok(Value::Bool(true));
                        }
                    }
                    other => {
                        return Err(type_err(format!(
                            "tuple for {name} must only contain str, not {}",
                            vm.type_name(other)
                        )))
                    }
                }
            }
            Ok(Value::Bool(false))
        }
        other => Err(type_err(format!(
            "{name} first arg must be str or a tuple of str, not {}",
            vm.type_name(&other)
        ))),
    }
}
fn str_startswith(vm: &mut Vm, a: Args) -> PyResult<Value> {
    affix_impl(vm, &a, false)
}
fn str_endswith(vm: &mut Vm, a: Args) -> PyResult<Value> {
    affix_impl(vm, &a, true)
}
fn strip_chars(vm: &Vm, a: &Args, name: &str) -> PyResult<Option<Vec<char>>> {
    match a.args.get(1) {
        None | Some(Value::None) => Ok(None),
        Some(v) => match vm.base_value(v) {
            Value::Str(s) => Ok(Some(s.s.chars().collect())),
            other => Err(type_err(format!(
                "{name} arg must be None or str, not {}",
                vm.type_name(&other)
            ))),
        },
    }
}
fn str_strip(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "strip", 0, 1)?;
    let s = as_str(vm, &a.args[0])?;
    s_ret(match strip_chars(vm, &a, "strip")? {
        None => s.s.trim().to_string(),
        Some(cs) => s.s.trim_matches(|c| cs.contains(&c)).to_string(),
    })
}
fn str_lstrip(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "lstrip", 0, 1)?;
    let s = as_str(vm, &a.args[0])?;
    s_ret(match strip_chars(vm, &a, "lstrip")? {
        None => s.s.trim_start().to_string(),
        Some(cs) => s.s.trim_start_matches(|c| cs.contains(&c)).to_string(),
    })
}
fn str_rstrip(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "rstrip", 0, 1)?;
    let s = as_str(vm, &a.args[0])?;
    s_ret(match strip_chars(vm, &a, "rstrip")? {
        None => s.s.trim_end().to_string(),
        Some(cs) => s.s.trim_end_matches(|c| cs.contains(&c)).to_string(),
    })
}
fn split_args(vm: &mut Vm, mut a: Args, name: &str) -> PyResult<(Rc<PyStr>, Option<String>, i64)> {
    let s = as_str(vm, &a.args[0])?;
    a.args.remove(0);
    let p = take_params(&mut a, name, &["sep", "maxsplit"], 0)?;
    let sep = match &p[0] {
        None | Some(Value::None) => None,
        Some(v) => match vm.base_value(v) {
            Value::Str(s) => {
                if s.s.is_empty() {
                    return Err(value_err("empty separator"));
                }
                Some(s.s.clone())
            }
            other => {
                return Err(type_err(format!(
                    "must be str or None, not {}",
                    vm.type_name(&other)
                )))
            }
        },
    };
    let max = match &p[1] {
        Some(v) => to_int_arg(vm, v)?,
        None => -1,
    };
    Ok((s, sep, max))
}
fn str_split(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let (s, sep, max) = split_args(vm, a, "split")?;
    let parts: Vec<String> = match sep {
        Some(sep) => {
            if max < 0 {
                s.s.split(&*sep).map(String::from).collect()
            } else {
                s.s.splitn(max as usize + 1, &*sep)
                    .map(String::from)
                    .collect()
            }
        }
        None => {
            let mut out = vec![];
            let mut rest = s.s.trim_start();
            let mut n = 0;
            while !rest.is_empty() {
                if max >= 0 && n >= max {
                    out.push(rest.to_string());
                    break;
                }
                match rest.find(char::is_whitespace) {
                    Some(p) => {
                        out.push(rest[..p].to_string());
                        rest = rest[p..].trim_start();
                    }
                    None => {
                        out.push(rest.to_string());
                        break;
                    }
                }
                n += 1;
            }
            out
        }
    };
    Ok(Value::list(parts.into_iter().map(Value::string).collect()))
}
fn str_rsplit(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let (s, sep, max) = split_args(vm, a, "rsplit")?;
    let mut parts: Vec<String> = match sep {
        Some(sep) => {
            if max < 0 {
                s.s.rsplit(&*sep).map(String::from).collect()
            } else {
                s.s.rsplitn(max as usize + 1, &*sep)
                    .map(String::from)
                    .collect()
            }
        }
        None => {
            let mut out = vec![];
            let mut rest = s.s.trim_end();
            let mut n = 0;
            while !rest.is_empty() {
                if max >= 0 && n >= max {
                    out.push(rest.to_string());
                    break;
                }
                match rest.rfind(char::is_whitespace) {
                    Some(p) => {
                        let w = rest[p..].chars().next().unwrap().len_utf8();
                        out.push(rest[p + w..].to_string());
                        rest = rest[..p].trim_end();
                    }
                    None => {
                        out.push(rest.to_string());
                        break;
                    }
                }
                n += 1;
            }
            out
        }
    };
    parts.reverse();
    Ok(Value::list(parts.into_iter().map(Value::string).collect()))
}
fn str_splitlines(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    a.args.remove(0);
    let p = take_params(&mut a, "splitlines", &["keepends"], 0)?;
    let keep = match &p[0] {
        Some(v) => vm.truthy(v)?,
        None => false,
    };
    let mut out = vec![];
    let chars: Vec<char> = s.s.chars().collect();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if is_break {
            let mut end = i + 1;
            if c == '\r' && chars.get(i + 1) == Some(&'\n') {
                end += 1;
            }
            let line: String = if keep {
                chars[start..end].iter().collect()
            } else {
                chars[start..i].iter().collect()
            };
            out.push(Value::string(line));
            start = end;
            i = end;
        } else {
            i += 1;
        }
    }
    if start < chars.len() {
        out.push(Value::string(chars[start..].iter().collect()));
    }
    Ok(Value::list(out))
}
fn str_join(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "str.join", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let items = vm.iterate(&a.args[1]).map_err(|e| {
        if vm.err_matches(&e, "TypeError") {
            type_err("can only join an iterable")
        } else {
            e
        }
    })?;
    let mut out = String::new();
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(&s.s);
        }
        match vm.base_value(it) {
            Value::Str(p) => out.push_str(&p.s),
            other => {
                return Err(type_err(format!(
                    "sequence item {i}: expected str instance, {} found",
                    vm.type_name(&other)
                )))
            }
        }
    }
    s_ret(out)
}
fn str_replace(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "replace", 2, 3)?;
    let s = as_str(vm, &a.args[0])?;
    let old = str_arg(vm, &a.args[1], "replace", 1)?;
    let new = str_arg(vm, &a.args[2], "replace", 2)?;
    let count = match a.args.get(3) {
        Some(v) => to_int_arg(vm, v)?,
        None => -1,
    };
    if old.s.is_empty() {
        // Insert `new` between characters (and at both ends).
        let mut out = String::new();
        let mut n = 0;
        if count != 0 {
            out.push_str(&new.s);
            n += 1;
        }
        for c in s.s.chars() {
            out.push(c);
            if count < 0 || n < count {
                out.push_str(&new.s);
                n += 1;
            }
        }
        if count >= 0 && n > count {
            // Trim trailing insert beyond count (not reachable).
        }
        return s_ret(out);
    }
    s_ret(if count < 0 {
        s.s.replace(&*old.s, &new.s)
    } else {
        s.s.replacen(&*old.s, &new.s, count as usize)
    })
}
fn str_partition(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "partition", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let sep = str_arg(vm, &a.args[1], "partition", 1)?;
    if sep.s.is_empty() {
        return Err(value_err("empty separator"));
    }
    Ok(match s.s.find(&*sep.s) {
        Some(p) => Value::tuple(vec![
            Value::str(&s.s[..p]),
            Value::Str(sep.clone()),
            Value::str(&s.s[p + sep.s.len()..]),
        ]),
        None => Value::tuple(vec![Value::Str(s.clone()), Value::str(""), Value::str("")]),
    })
}
fn str_rpartition(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "rpartition", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let sep = str_arg(vm, &a.args[1], "rpartition", 1)?;
    if sep.s.is_empty() {
        return Err(value_err("empty separator"));
    }
    Ok(match s.s.rfind(&*sep.s) {
        Some(p) => Value::tuple(vec![
            Value::str(&s.s[..p]),
            Value::Str(sep.clone()),
            Value::str(&s.s[p + sep.s.len()..]),
        ]),
        None => Value::tuple(vec![Value::str(""), Value::str(""), Value::Str(s.clone())]),
    })
}
fn str_removeprefix(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "removeprefix", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let p = str_arg(vm, &a.args[1], "removeprefix", 1)?;
    s_ret(s.s.strip_prefix(&*p.s).unwrap_or(&s.s).to_string())
}
fn str_removesuffix(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "removesuffix", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let p = str_arg(vm, &a.args[1], "removesuffix", 1)?;
    if p.s.is_empty() {
        return Ok(Value::Str(s));
    }
    s_ret(s.s.strip_suffix(&*p.s).unwrap_or(&s.s).to_string())
}
fn pred(vm: &mut Vm, a: &Args, f: impl Fn(char) -> bool, empty: bool) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    if s.s.is_empty() {
        return Ok(Value::Bool(empty));
    }
    Ok(Value::Bool(s.s.chars().all(f)))
}
fn str_isalnum(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(vm, &a, |c| c.is_alphanumeric(), false)
}
fn str_isalpha(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(vm, &a, |c| c.is_alphabetic(), false)
}
fn str_isascii(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(vm, &a, |c| c.is_ascii(), true)
}
fn str_isdecimal(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(
        vm,
        &a,
        |c| c.is_ascii_digit() || (c.is_numeric() && c.to_digit(10).is_some()),
        false,
    )
}
fn str_isdigit(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(
        vm,
        &a,
        |c| c.is_ascii_digit() || matches!(c, '²' | '³' | '¹' | '⁰'..='⁹' | '₀'..='₉' | '①'..='⑨'),
        false,
    )
}
fn str_isnumeric(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(vm, &a, |c| c.is_numeric(), false)
}
fn str_isspace(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pred(
        vm,
        &a,
        |c| c.is_whitespace() || matches!(c, '\x1c'..='\x1f'),
        false,
    )
}
fn str_isprintable(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    Ok(Value::Bool(
        crate::format::str_repr(&s.s).len() == s.s.len() + 2
            || !s.s.chars().any(|c| c.is_control()),
    ))
}
fn str_islower(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let cased = s.s.chars().any(|c| c.is_lowercase() || c.is_uppercase());
    Ok(Value::Bool(cased && !s.s.chars().any(|c| c.is_uppercase())))
}
fn str_isupper(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let cased = s.s.chars().any(|c| c.is_lowercase() || c.is_uppercase());
    Ok(Value::Bool(cased && !s.s.chars().any(|c| c.is_lowercase())))
}
fn str_isidentifier(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    let mut chars = s.s.chars();
    Ok(Value::Bool(match chars.next() {
        Some(c) => {
            (c == '_' || c.is_alphabetic()) && chars.all(|c| c == '_' || c.is_alphanumeric())
        }
        None => false,
    }))
}
fn str_encode(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    a.args.remove(0);
    let p = take_params(&mut a, "encode", &["encoding", "errors"], 0)?;
    let enc = match &p[0] {
        Some(Value::Str(e)) => e.s.clone(),
        _ => "utf-8".into(),
    };
    let errors = match &p[1] {
        Some(Value::Str(e)) => e.s.clone(),
        _ => "strict".into(),
    };
    Ok(Value::Bytes(Rc::new(encode_str(&s.s, &enc, &errors)?)))
}
fn str_format_m(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let s = as_str(vm, &a.args[0])?;
    a.args.remove(0);
    s_ret(crate::format::str_format(vm, &s.s, &a.args, &a.kwargs)?)
}
fn str_format_map(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "format_map", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let items = vm.mapping_items(&a.args[1])?;
    let mut kwargs = vec![];
    for (k, v) in items {
        if let Value::Str(k) = k {
            kwargs.push((k.s.as_str().into(), v));
        }
    }
    s_ret(crate::format::str_format(vm, &s.s, &[], &kwargs)?)
}
fn str_maketrans(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let d = new_ref(Dict::new());
    match a.args.len() {
        1 => {
            for (k, v) in vm.mapping_items(&a.args[0])? {
                let key = match &k {
                    Value::Str(s) if s.nchars == 1 => {
                        Value::Int(s.s.chars().next().unwrap() as i64)
                    }
                    Value::Int(_) => k.clone(),
                    _ => {
                        return Err(value_err(
                            "string keys in translate table must be of length 1",
                        ))
                    }
                };
                vm.dict_set(&d, key, v)?;
            }
        }
        2 | 3 => {
            let x = str_arg(vm, &a.args[0], "maketrans", 1)?;
            let y = str_arg(vm, &a.args[1], "maketrans", 2)?;
            if x.nchars != y.nchars {
                return Err(value_err(
                    "the first two maketrans arguments must have equal length",
                ));
            }
            for (c1, c2) in x.s.chars().zip(y.s.chars()) {
                vm.dict_set(&d, Value::Int(c1 as i64), Value::Int(c2 as i64))?;
            }
            if let Some(z) = a.args.get(2) {
                let z = str_arg(vm, z, "maketrans", 3)?;
                for c in z.s.chars() {
                    vm.dict_set(&d, Value::Int(c as i64), Value::None)?;
                }
            }
        }
        _ => return Err(type_err("maketrans() takes at most 3 arguments")),
    }
    Ok(Value::Dict(d))
}
fn str_translate(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "translate", 1, 1)?;
    let s = as_str(vm, &a.args[0])?;
    let table = a.args[1].clone();
    let mut out = String::new();
    for c in s.s.chars() {
        match vm.getitem(&table, &Value::Int(c as i64)) {
            Ok(Value::None) => {}
            Ok(Value::Str(r)) => out.push_str(&r.s),
            Ok(Value::Int(i)) => out.push(char::from_u32(i as u32).unwrap_or(c)),
            Ok(_) => {
                return Err(type_err(
                    "character mapping must return integer, None or str",
                ))
            }
            Err(e) if vm.err_matches(&e, "LookupError") => out.push(c),
            Err(e) => return Err(e),
        }
    }
    s_ret(out)
}

// ---------------------------------------------------------------------------
// bytes
// ---------------------------------------------------------------------------

fn bytes_of(vm: &Vm, v: &Value) -> PyResult<Vec<u8>> {
    match vm.base_value(v) {
        Value::Bytes(b) => Ok((*b).clone()),
        Value::ByteArray(b) => Ok(b.borrow().clone()),
        other => Err(type_err(format!(
            "a bytes-like object is required, not '{}'",
            vm.type_name(&other)
        ))),
    }
}
fn same_bytes_kind(v: &Value, data: Vec<u8>) -> Value {
    match v {
        Value::ByteArray(_) => Value::ByteArray(new_ref(data)),
        _ => Value::Bytes(Rc::new(data)),
    }
}
fn bytes_decode(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    a.args.remove(0);
    let p = take_params(&mut a, "decode", &["encoding", "errors"], 0)?;
    let enc = match &p[0] {
        Some(Value::Str(e)) => e.s.clone(),
        _ => "utf-8".into(),
    };
    let errors = match &p[1] {
        Some(Value::Str(e)) => e.s.clone(),
        _ => "strict".into(),
    };
    Ok(Value::string(decode_bytes(&b, &enc, &errors)?))
}
fn bytes_hex(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let sep = match a.args.get(1) {
        Some(Value::Str(s)) => s.s.clone(),
        _ => String::new(),
    };
    let parts: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    s_ret(parts.join(&sep))
}
fn bytes_fromhex(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = str_arg(vm, &a.args[1], "fromhex", 1)?;
    let clean: String = s.s.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        return Err(value_err("non-hexadecimal number found in fromhex() arg"));
    }
    let mut out = vec![];
    for i in (0..clean.len()).step_by(2) {
        out.push(
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|_| value_err("non-hexadecimal number found in fromhex() arg"))?,
        );
    }
    Ok(Value::Bytes(Rc::new(out)))
}
fn bytes_startswith(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let p = bytes_of(vm, &a.args[1])?;
    Ok(Value::Bool(b.starts_with(&p)))
}
fn bytes_endswith(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let p = bytes_of(vm, &a.args[1])?;
    Ok(Value::Bool(b.ends_with(&p)))
}
fn bfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}
fn bytes_find(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let p = match &a.args[1] {
        Value::Int(i) => vec![*i as u8],
        other => bytes_of(vm, other)?,
    };
    Ok(Value::Int(bfind(&b, &p).map(|x| x as i64).unwrap_or(-1)))
}
fn bytes_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let r = bytes_find(vm, a)?;
    if matches!(r, Value::Int(-1)) {
        return Err(value_err("subsection not found"));
    }
    Ok(r)
}
fn bytes_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let p = bytes_of(vm, &a.args[1])?;
    if p.is_empty() {
        return Ok(Value::Int(b.len() as i64 + 1));
    }
    let mut n = 0;
    let mut i = 0;
    while i + p.len() <= b.len() {
        if b[i..i + p.len()] == p[..] {
            n += 1;
            i += p.len();
        } else {
            i += 1;
        }
    }
    Ok(Value::Int(n))
}
fn bytes_join(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let sep = bytes_of(vm, &a.args[0])?;
    let items = vm.iterate(&a.args[1])?;
    let mut out = vec![];
    for (i, it) in items.iter().enumerate() {
        if i > 0 {
            out.extend(&sep);
        }
        out.extend(bytes_of(vm, it)?);
    }
    Ok(same_bytes_kind(&a.args[0], out))
}
fn bytes_split(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let parts: Vec<Vec<u8>> = match a.args.get(1) {
        None | Some(Value::None) => b
            .split(|c| c.is_ascii_whitespace())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_vec())
            .collect(),
        Some(sep) => {
            let sep = bytes_of(vm, sep)?;
            if sep.is_empty() {
                return Err(value_err("empty separator"));
            }
            let mut out = vec![];
            let mut rest = &b[..];
            while let Some(p) = bfind(rest, &sep) {
                out.push(rest[..p].to_vec());
                rest = &rest[p + sep.len()..];
            }
            out.push(rest.to_vec());
            out
        }
    };
    Ok(Value::list(
        parts
            .into_iter()
            .map(|p| same_bytes_kind(&a.args[0], p))
            .collect(),
    ))
}
fn bytes_strip(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let chars = match a.args.get(1) {
        Some(v) if !v.is_none() => bytes_of(vm, v)?,
        _ => b" \t\n\r\x0b\x0c".to_vec(),
    };
    let start = b.iter().position(|c| !chars.contains(c)).unwrap_or(b.len());
    let end = b
        .iter()
        .rposition(|c| !chars.contains(c))
        .map(|p| p + 1)
        .unwrap_or(start);
    Ok(same_bytes_kind(
        &a.args[0],
        b[start..end.max(start)].to_vec(),
    ))
}
fn bytes_replace(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    let old = bytes_of(vm, &a.args[1])?;
    let new = bytes_of(vm, &a.args[2])?;
    if old.is_empty() {
        return Ok(same_bytes_kind(&a.args[0], b));
    }
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(&old) {
            out.extend(&new);
            i += old.len();
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Ok(same_bytes_kind(&a.args[0], out))
}
fn bytes_upper(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    Ok(same_bytes_kind(&a.args[0], b.to_ascii_uppercase()))
}
fn bytes_lower(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let b = bytes_of(vm, &a.args[0])?;
    Ok(same_bytes_kind(&a.args[0], b.to_ascii_lowercase()))
}
fn bytearray_append(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::ByteArray(b) = this(vm, &a) else {
        return Err(type_err("expected bytearray"));
    };
    let i = to_int_arg(vm, &a.args[1])?;
    if !(0..256).contains(&i) {
        return Err(value_err("byte must be in range(0, 256)"));
    }
    b.borrow_mut().push(i as u8);
    Ok(Value::None)
}
fn bytearray_extend(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::ByteArray(b) = this(vm, &a) else {
        return Err(type_err("expected bytearray"));
    };
    let data = bytes_from(vm, Some(&a.args[1]), None, None)?;
    b.borrow_mut().extend(data);
    Ok(Value::None)
}
fn bytearray_pop(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::ByteArray(b) = this(vm, &a) else {
        return Err(type_err("expected bytearray"));
    };
    let v = b.borrow_mut().pop();
    v.map(|x| Value::Int(x as i64))
        .ok_or_else(|| err("IndexError", "pop from empty bytearray"))
}

// ---------------------------------------------------------------------------
// list / tuple
// ---------------------------------------------------------------------------

fn list_init(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() > 2 {
        return Err(type_err(format!(
            "list expected at most 1 argument, got {}",
            a.args.len() - 1
        )));
    }
    let l = as_list(vm, &a.args[0])?;
    let items = match a.args.get(1) {
        Some(v) => vm.iterate(v)?,
        None => vec![],
    };
    *l.borrow_mut() = items;
    Ok(Value::None)
}
fn list_iadd(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let l = as_list(vm, &a.args[0])?;
    let items = vm.iterate(&a.args[1])?;
    l.borrow_mut().extend(items);
    Ok(a.args[0].clone())
}
fn list_reversed(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    let n = vm.len(&v)?;
    Ok(Value::Iter(new_ref(IterObj::Reversed {
        seq: v,
        idx: n as isize - 1,
    })))
}
fn list_append(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "list.append", 1, 1)?;
    let l = as_list(vm, &a.args[0])?;
    l.borrow_mut().push(a.args[1].clone());
    Ok(Value::None)
}
fn list_extend(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "list.extend", 1, 1)?;
    let l = as_list(vm, &a.args[0])?;
    let items = vm.iterate(&a.args[1])?;
    l.borrow_mut().extend(items);
    Ok(Value::None)
}
fn list_insert(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() != 3 {
        return Err(type_err(format!(
            "insert expected 2 arguments, got {}",
            a.args.len() - 1
        )));
    }
    let l = as_list(vm, &a.args[0])?;
    let i = to_int_arg(vm, &a.args[1])?;
    let mut list = l.borrow_mut();
    let n = list.len() as i64;
    let pos = if i < 0 { (i + n).max(0) } else { i.min(n) };
    list.insert(pos as usize, a.args[2].clone());
    Ok(Value::None)
}
fn list_remove(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "list.remove", 1, 1)?;
    let l = as_list(vm, &a.args[0])?;
    let items = l.borrow().clone();
    for (i, x) in items.iter().enumerate() {
        if x.is(&a.args[1]) || vm.eq(x, &a.args[1])? {
            l.borrow_mut().remove(i);
            return Ok(Value::None);
        }
    }
    Err(value_err("list.remove(x): x not in list"))
}
fn list_pop(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "pop", 0, 1)?;
    let l = as_list(vm, &a.args[0])?;
    let len = l.borrow().len();
    if len == 0 {
        return Err(err("IndexError", "pop from empty list"));
    }
    let i = match a.args.get(1) {
        Some(v) => to_int_arg(vm, v)?,
        None => -1,
    };
    let j = if i < 0 { i + len as i64 } else { i };
    if j < 0 || j >= len as i64 {
        return Err(err("IndexError", "pop index out of range"));
    }
    let v = l.borrow_mut().remove(j as usize);
    Ok(v)
}
fn list_clear(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let l = as_list(vm, &a.args[0])?;
    l.borrow_mut().clear();
    Ok(Value::None)
}
fn seq_index(vm: &mut Vm, items: &[Value], a: &Args) -> PyResult<Option<usize>> {
    let n = items.len() as i64;
    let norm = |v: i64| if v < 0 { (v + n).max(0) } else { v.min(n) };
    let start = match a.args.get(2) {
        Some(v) => norm(to_int_arg(vm, v)?),
        None => 0,
    };
    let end = match a.args.get(3) {
        Some(v) => norm(to_int_arg(vm, v)?),
        None => n,
    };
    for i in start..end {
        let x = &items[i as usize];
        if x.is(&a.args[1]) || vm.eq(x, &a.args[1])? {
            return Ok(Some(i as usize));
        }
    }
    Ok(None)
}
fn list_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "index", 1, 3)?;
    let l = as_list(vm, &a.args[0])?;
    let items = l.borrow().clone();
    match seq_index(vm, &items, &a)? {
        Some(i) => Ok(Value::Int(i as i64)),
        None => {
            let r = vm.repr(&a.args[1])?;
            Err(value_err(format!("{r} is not in list")))
        }
    }
}
fn list_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "count", 1, 1)?;
    let l = as_list(vm, &a.args[0])?;
    let items = l.borrow().clone();
    let mut n = 0;
    for x in &items {
        if x.is(&a.args[1]) || vm.eq(x, &a.args[1])? {
            n += 1;
        }
    }
    Ok(Value::Int(n))
}
fn list_sort(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let l = as_list(vm, &a.args[0])?;
    if a.args.len() > 1 {
        return Err(type_err("sort() takes no positional arguments"));
    }
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
    let items = std::mem::take(&mut *l.borrow_mut());
    let sorted = sort_values(vm, items.clone(), key.as_ref(), reverse);
    let modified = !l.borrow().is_empty();
    match sorted {
        Ok(v) => {
            *l.borrow_mut() = v;
            if modified {
                return Err(value_err("list modified during sort"));
            }
            Ok(Value::None)
        }
        Err(e) => {
            *l.borrow_mut() = items;
            Err(e)
        }
    }
}
fn list_reverse(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let l = as_list(vm, &a.args[0])?;
    l.borrow_mut().reverse();
    Ok(Value::None)
}
fn list_copy(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let l = as_list(vm, &a.args[0])?;
    let items = l.borrow().clone();
    Ok(Value::list(items))
}
fn tuple_items(vm: &Vm, v: &Value) -> PyResult<Rc<Vec<Value>>> {
    match vm.base_value(v) {
        Value::Tuple(t) => Ok(t),
        _ => Err(type_err("expected tuple")),
    }
}
fn tuple_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "index", 1, 3)?;
    let t = tuple_items(vm, &a.args[0])?;
    match seq_index(vm, &t, &a)? {
        Some(i) => Ok(Value::Int(i as i64)),
        None => Err(value_err("tuple.index(x): x not in tuple")),
    }
}
fn tuple_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "count", 1, 1)?;
    let t = tuple_items(vm, &a.args[0])?;
    let mut n = 0;
    for x in t.iter() {
        if x.is(&a.args[1]) || vm.eq(x, &a.args[1])? {
            n += 1;
        }
    }
    Ok(Value::Int(n))
}

// ---------------------------------------------------------------------------
// dict
// ---------------------------------------------------------------------------

fn dict_init(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    if a.args.len() > 2 {
        return Err(type_err(format!(
            "dict expected at most 1 argument, got {}",
            a.args.len() - 1
        )));
    }
    let kwargs = std::mem::take(&mut a.kwargs);
    dict_update(vm, &d, a.args.get(1), kwargs)?;
    Ok(Value::None)
}
fn dict_ior(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    dict_update(vm, &d, a.args.get(1), vec![])?;
    Ok(a.args[0].clone())
}
fn dict_reversed(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    let mut keys = d.borrow().keys();
    keys.reverse();
    Ok(Value::Iter(new_ref(IterObj::List {
        items: keys,
        idx: 0,
    })))
}
fn dictview_reversed(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let mut items = vm.iterate(&a.args[0])?;
    items.reverse();
    Ok(Value::Iter(new_ref(IterObj::List { items, idx: 0 })))
}
fn dict_keys(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "dict.keys", 0, 0)?;
    Ok(Value::DictView(Rc::new((
        as_dict(vm, &a.args[0])?,
        ViewKind::Keys,
    ))))
}
fn dict_values(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "dict.values", 0, 0)?;
    Ok(Value::DictView(Rc::new((
        as_dict(vm, &a.args[0])?,
        ViewKind::Values,
    ))))
}
fn dict_items(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "dict.items", 0, 0)?;
    Ok(Value::DictView(Rc::new((
        as_dict(vm, &a.args[0])?,
        ViewKind::Items,
    ))))
}
fn dict_get(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() < 2 || a.args.len() > 3 {
        return Err(type_err(format!(
            "get expected at least 1 argument, got {}",
            a.args.len() - 1
        )));
    }
    let d = as_dict(vm, &a.args[0])?;
    Ok(vm
        .dict_get(&d, &a.args[1])?
        .unwrap_or_else(|| a.args.get(2).cloned().unwrap_or(Value::None)))
}
fn dict_setdefault(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    if let Some(v) = vm.dict_get(&d, &a.args[1])? {
        return Ok(v);
    }
    let v = a.args.get(2).cloned().unwrap_or(Value::None);
    vm.dict_set(&d, a.args[1].clone(), v.clone())?;
    Ok(v)
}
fn dict_pop(vm: &mut Vm, a: Args) -> PyResult<Value> {
    if a.args.len() < 2 {
        return Err(type_err("pop expected at least 1 argument, got 0"));
    }
    let d = as_dict(vm, &a.args[0])?;
    match vm.dict_del(&d, &a.args[1])? {
        Some(v) => Ok(v),
        None => match a.args.get(2) {
            Some(dflt) => Ok(dflt.clone()),
            None => Err(err_args("KeyError", vec![a.args[1].clone()])),
        },
    }
}
fn dict_popitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    let r = d.borrow_mut().pop_last();
    match r {
        Some((k, v)) => Ok(Value::tuple(vec![k, v])),
        None => Err(err("KeyError", "popitem(): dictionary is empty")),
    }
}
fn dict_update_m(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    if a.args.len() > 2 {
        return Err(type_err(format!(
            "update expected at most 1 argument, got {}",
            a.args.len() - 1
        )));
    }
    let kwargs = std::mem::take(&mut a.kwargs);
    dict_update(vm, &d, a.args.get(1), kwargs)?;
    Ok(Value::None)
}
fn dict_clear(vm: &mut Vm, a: Args) -> PyResult<Value> {
    as_dict(vm, &a.args[0])?.borrow_mut().clear();
    Ok(Value::None)
}
fn dict_copy(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let d = as_dict(vm, &a.args[0])?;
    let c = d.borrow().clone();
    Ok(Value::dict(c))
}
fn dict_fromkeys(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let cls = a.args[0].clone();
    let keys = vm.iterate(&a.args[1])?;
    let v = a.args.get(2).cloned().unwrap_or(Value::None);
    let target = vm.call(&cls, vec![])?;
    for k in keys {
        vm.setitem(&target, &k, v.clone())?;
    }
    Ok(target)
}

// ---------------------------------------------------------------------------
// set
// ---------------------------------------------------------------------------

fn as_set(vm: &Vm, v: &Value) -> PyResult<Ref<SetData>> {
    match vm.base_value(v) {
        Value::Set(s) => Ok(s),
        other => Err(type_err(format!(
            "descriptor requires a 'set' object but received a '{}'",
            vm.type_name(&other)
        ))),
    }
}
fn set_init(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_set(vm, &a.args[0])?;
    let data = match a.args.get(1) {
        Some(v) => build_set(vm, v)?,
        None => SetData::new(),
    };
    *s.borrow_mut() = data;
    Ok(Value::None)
}
fn set_add_m(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "set.add", 1, 1)?;
    let s = as_set(vm, &a.args[0])?;
    vm.set_add(&s, a.args[1].clone())?;
    Ok(Value::None)
}
fn set_remove(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "set.remove", 1, 1)?;
    let s = as_set(vm, &a.args[0])?;
    if !vm.set_discard(&s, &a.args[1])? {
        return Err(err_args("KeyError", vec![a.args[1].clone()]));
    }
    Ok(Value::None)
}
fn set_discard_m(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "set.discard", 1, 1)?;
    let s = as_set(vm, &a.args[0])?;
    vm.set_discard(&s, &a.args[1])?;
    Ok(Value::None)
}
fn set_pop(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_set(vm, &a.args[0])?;
    let mut finger = vm.set_finger;
    let r = s.borrow_mut().pop_any(&mut finger);
    vm.set_finger = finger;
    r.ok_or_else(|| err("KeyError", "pop from an empty set"))
}
fn set_clear(vm: &mut Vm, a: Args) -> PyResult<Value> {
    as_set(vm, &a.args[0])?.borrow_mut().clear();
    Ok(Value::None)
}
fn set_update(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = as_set(vm, &a.args[0])?;
    for other in &a.args[1..] {
        for x in vm.iterate(other)? {
            vm.set_add(&s, x)?;
        }
    }
    Ok(Value::None)
}
fn to_set_value(vm: &mut Vm, v: &Value) -> PyResult<Value> {
    Ok(match v {
        Value::Set(_) | Value::FrozenSet(_) => v.clone(),
        _ => Value::Set(new_ref(build_set(vm, v)?)),
    })
}
fn set_multi(vm: &mut Vm, a: Args, op: BinOp) -> PyResult<Value> {
    let mut acc = this(vm, &a);
    if a.args.len() == 1 {
        let data = crate::builtins::set_data_of(&acc).unwrap_or_default();
        return Ok(match acc {
            Value::FrozenSet(_) => acc,
            _ => Value::Set(new_ref(SetData::copy_from(&data))),
        });
    }
    for other in &a.args[1..] {
        let o = to_set_value(vm, other)?;
        acc = crate::builtins::set_binop(vm, &acc, &o, op)?;
    }
    Ok(acc)
}
fn set_union(vm: &mut Vm, a: Args) -> PyResult<Value> {
    set_multi(vm, a, BinOp::BitOr)
}
fn set_intersection(vm: &mut Vm, a: Args) -> PyResult<Value> {
    set_multi(vm, a, BinOp::BitAnd)
}
fn set_difference(vm: &mut Vm, a: Args) -> PyResult<Value> {
    set_multi(vm, a, BinOp::Sub)
}
fn set_symmetric_difference(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "symmetric_difference", 1, 1)?;
    set_multi(vm, a, BinOp::BitXor)
}
fn set_inplace(vm: &mut Vm, a: Args, op: BinOp) -> PyResult<Value> {
    let s = as_set(vm, &a.args[0])?;
    let r = set_multi(vm, a, op)?;
    if let Some(d) = crate::builtins::set_data_of(&r) {
        *s.borrow_mut() = d;
    }
    Ok(Value::None)
}
fn set_intersection_update(vm: &mut Vm, a: Args) -> PyResult<Value> {
    set_inplace(vm, a, BinOp::BitAnd)
}
fn set_difference_update(vm: &mut Vm, a: Args) -> PyResult<Value> {
    set_inplace(vm, a, BinOp::Sub)
}
fn set_symmetric_difference_update(vm: &mut Vm, a: Args) -> PyResult<Value> {
    set_inplace(vm, a, BinOp::BitXor)
}
fn set_issubset(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let x = this(vm, &a);
    let y = to_set_value(vm, &a.args[1])?;
    Ok(Value::Bool(
        crate::builtins::set_compare(vm, &x, &y, CmpOp::LtE)?.unwrap_or(false),
    ))
}
fn set_issuperset(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let x = this(vm, &a);
    let y = to_set_value(vm, &a.args[1])?;
    Ok(Value::Bool(
        crate::builtins::set_compare(vm, &x, &y, CmpOp::GtE)?.unwrap_or(false),
    ))
}
fn set_isdisjoint(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let x = this(vm, &a);
    for it in vm.iterate(&a.args[1])? {
        if vm.contains(&x, &it)? {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}
fn set_copy(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    let d = crate::builtins::set_data_of(&v).unwrap_or_default();
    Ok(match v {
        Value::FrozenSet(_) => v,
        _ => Value::Set(new_ref(SetData::copy_from(&d))),
    })
}

// ---------------------------------------------------------------------------
// range
// ---------------------------------------------------------------------------

fn range_index(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Range(r) = this(vm, &a) else {
        return Err(type_err("expected range"));
    };
    let v = this(vm, &a);
    if vm.contains(&v, &a.args[1])? {
        if let Value::Int(i) = a.args[1] {
            return Ok(Value::Int((i - r.start) / r.step));
        }
    }
    let rep = vm.repr(&a.args[1])?;
    Err(value_err(format!("{rep} is not in range")))
}
fn range_count(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = this(vm, &a);
    Ok(Value::Int(vm.contains(&v, &a.args[1])? as i64))
}
fn range_reversed(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Range(r) = this(vm, &a) else {
        return Err(type_err("expected range"));
    };
    let n = r.len();
    Ok(Value::Iter(new_ref(IterObj::Range {
        cur: r.start + (n - 1) * r.step,
        step: -r.step,
        remaining: n,
    })))
}

// ---------------------------------------------------------------------------
// iterators and generators
// ---------------------------------------------------------------------------

fn iter_next(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let it = a.args[0].clone();
    if let Value::Gen(g) = &it {
        return match vm.gen_resume(g, Value::None, None)? {
            GenResult::Yielded(v) => Ok(v),
            GenResult::Returned(r) => Err(err_args(
                "StopIteration",
                if r.is_none() { vec![] } else { vec![r] },
            )),
        };
    }
    match vm.next(&it)? {
        Some(v) => Ok(v),
        None => Err(err_args("StopIteration", vec![])),
    }
}
fn iter_self(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(a.args[0].clone())
}
fn gen_send(vm: &mut Vm, a: Args) -> PyResult<Value> {
    nargs(&a, "send", 1, 1)?;
    let Value::Gen(g) = &a.args[0] else {
        return Err(type_err("expected generator"));
    };
    match vm.gen_resume(g, a.args[1].clone(), None)? {
        GenResult::Yielded(v) => Ok(v),
        GenResult::Returned(r) => Err(err_args(
            "StopIteration",
            if r.is_none() { vec![] } else { vec![r] },
        )),
    }
}
fn gen_throw(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Gen(g) = &a.args[0] else {
        return Err(type_err("expected generator"));
    };
    let exc = match a.args.get(1) {
        Some(v) => vm.make_exception(v)?,
        None => return Err(type_err("throw expected at least 1 argument, got 0")),
    };
    let e = PyErr::from_exc(exc, false);
    match vm.gen_resume(g, Value::None, Some(e))? {
        GenResult::Yielded(v) => Ok(v),
        GenResult::Returned(r) => Err(err_args(
            "StopIteration",
            if r.is_none() { vec![] } else { vec![r] },
        )),
    }
}
fn gen_close(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let Value::Gen(g) = &a.args[0] else {
        return Err(type_err("expected generator"));
    };
    vm.gen_close(g)?;
    Ok(Value::None)
}

// ---------------------------------------------------------------------------
// property, functions, exceptions
// ---------------------------------------------------------------------------

fn prop_of(v: &Value) -> PyResult<Rc<Property>> {
    match v {
        Value::Property(p) => Ok(p.clone()),
        _ => Err(type_err("expected property")),
    }
}
fn property_getter(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let p = prop_of(&a.args[0])?;
    Ok(Value::Property(Rc::new(Property {
        fget: a.args[1].clone(),
        fset: p.fset.clone(),
        fdel: p.fdel.clone(),
        doc: p.doc.clone(),
    })))
}
fn property_setter(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let p = prop_of(&a.args[0])?;
    Ok(Value::Property(Rc::new(Property {
        fget: p.fget.clone(),
        fset: a.args[1].clone(),
        fdel: p.fdel.clone(),
        doc: p.doc.clone(),
    })))
}
fn property_deleter(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let p = prop_of(&a.args[0])?;
    Ok(Value::Property(Rc::new(Property {
        fget: p.fget.clone(),
        fset: p.fset.clone(),
        fdel: a.args[1].clone(),
        doc: p.doc.clone(),
    })))
}
fn property_get(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let p = prop_of(&a.args[0])?;
    let obj = a.args.get(1).cloned().unwrap_or(Value::None);
    if obj.is_none() {
        return Ok(a.args[0].clone());
    }
    let fget = p.fget.clone();
    vm.call(&fget, vec![obj])
}
fn property_set(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let p = prop_of(&a.args[0])?;
    let fset = p.fset.clone();
    if fset.is_none() {
        return Err(err("AttributeError", "property has no setter"));
    }
    vm.call(&fset, vec![a.args[1].clone(), a.args[2].clone()])?;
    Ok(Value::None)
}
fn function_get(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    let obj = a.args.get(1).cloned().unwrap_or(Value::None);
    if obj.is_none() {
        return Ok(a.args[0].clone());
    }
    Ok(Value::Method(Rc::new((obj, a.args[0].clone()))))
}
fn staticmethod_get(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    match &a.args[0] {
        Value::StaticMethod(f) => Ok((**f).clone()),
        other => Ok(other.clone()),
    }
}
fn classmethod_get(vm: &mut Vm, a: Args) -> PyResult<Value> {
    match &a.args[0] {
        Value::ClassMethod(f) => {
            let owner = match a.args.get(2) {
                Some(c @ Value::Class(_)) => c.clone(),
                _ => Value::Class(vm.type_of(a.args.get(1).unwrap_or(&Value::None))),
            };
            Ok(Value::Method(Rc::new((owner, (**f).clone()))))
        }
        other => Ok(other.clone()),
    }
}
fn callable_call(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let f = a.args.remove(0);
    vm.call_kw(&f, a.args, a.kwargs)
}
fn exc_init(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let obj = a.args[0].clone();
    let args = a.args[1..].to_vec();
    vm.exc_data(&obj, |d| d.args = Value::tuple(args));
    Ok(Value::None)
}
fn exc_str_m(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::string(exc_str(vm, &a.args[0])?))
}
fn exc_repr(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let v = &a.args[0];
    let args = exc_args(vm, v);
    let name = vm.type_of(v).name();
    let inner = if args.len() == 1 {
        vm.repr(&args[0])?
    } else {
        let t = vm.repr(&Value::tuple(args.clone()))?;
        if args.is_empty() {
            String::new()
        } else {
            t[1..t.len() - 1].to_string()
        }
    };
    Ok(Value::string(format!("{name}({inner})")))
}
fn exc_with_traceback(_vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(a.args[0].clone())
}
fn exc_add_note(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let obj = a.args[0].clone();
    let note = a.args.get(1).cloned().unwrap_or(Value::None);
    let notes = match vm.getattr_opt(&obj, "__notes__")? {
        Some(n) => n,
        None => {
            let l = Value::list(vec![]);
            if let Value::Instance(i) = &obj {
                i.dict.borrow_mut().set_str("__notes__", l.clone());
            }
            l
        }
    };
    if let Value::List(l) = notes {
        l.borrow_mut().push(note);
    }
    Ok(Value::None)
}
fn none_bool(_vm: &mut Vm, _a: Args) -> PyResult<Value> {
    Ok(Value::Bool(false))
}
