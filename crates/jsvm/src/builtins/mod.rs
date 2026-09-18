//! Standard built-in objects.

pub mod array;
pub mod date;
pub mod error;
pub mod function;
pub mod global;
pub mod iter;
pub mod json;
pub mod mapset;
pub mod math;
pub mod number;
pub mod object;
pub mod reflect;
pub mod string;
pub mod symbol;
pub mod typed;

use crate::value::*;
use crate::vm::Vm;

/// A callback argument, or `TypeError: x is not a function`.
pub fn callback(vm: &mut Vm, v: &Value) -> JsResult<Value> {
    if v.is_callable() {
        return Ok(v.clone());
    }
    let d = vm.describe_for_error(v);
    Err(vm.type_error(format!("{d} is not a function")))
}

pub fn obj_arg(vm: &mut Vm, v: &Value, what: &str) -> JsResult<Obj> {
    match v {
        Value::Obj(o) => Ok(o.clone()),
        _ => Err(vm.type_error(format!("{what} called on non-object"))),
    }
}

pub fn num_arg(vm: &mut Vm, a: &Args, i: usize) -> JsResult<f64> {
    let v = a.arg(i);
    vm.to_number(&v)
}

pub fn str_arg(vm: &mut Vm, a: &Args, i: usize) -> JsResult<JsStr> {
    let v = a.arg(i);
    vm.to_string(&v)
}

pub fn new_obj_from(vm: &mut Vm, pairs: Vec<(&str, Value)>) -> Obj {
    let o = vm.new_object();
    for (k, v) in pairs {
        o.set_prop(k, v, ALL);
    }
    o
}
