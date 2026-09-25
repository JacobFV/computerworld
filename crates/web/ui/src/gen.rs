//! What generated programs call.
//!
//! `cw-tsx build --emit rust` translates each IR function into a Rust `fn` that does
//! what the interpreter does for its statements and expressions, in the same order,
//! through these entry points: the same member access, operators, built-ins,
//! methods, hooks and element construction the interpreter uses, so the two cannot
//! drift apart. Only the walk over the IR is gone: control flow is Rust's, locals
//! are Rust variables (numbers and booleans unboxed where the generator proves the
//! type), template holes check their dependencies with the slots named in the
//! code, and calls to module functions are direct calls.
//!
//! The interface is for generated code: it is public so a generated module can live
//! in another crate, and not meant for anything else.

use std::cell::RefCell;
use std::rc::Rc;

pub use crate::interp::inspect;
pub use crate::ir::{BinaryOp, Builtin, Capture, Hook, Method};
pub use crate::program::{GenFn, GenFunc, GenProgram, STAttr, STNode, STemplate};
pub use crate::render::{
    component_callee, component_elem, key_of, props_spread, provider_context, HookArg, TplBuilder,
};
pub use crate::runtime::{Runtime, Throw, R};
pub use crate::value::{loose_equals, same_value, strict_equals, Closure, Elem, Str, Value};

use crate::runtime::type_error;

/// A slot's value, read through its cell when it is boxed.
#[inline]
pub fn read(v: &Value) -> Value {
    match v {
        Value::Cell(c) => c.borrow().clone(),
        v => v.clone(),
    }
}

/// Assigns a local slot (through its cell when it is boxed).
#[inline]
pub fn write_slot(slot: &mut Value, v: Value) {
    match slot {
        Value::Cell(c) => *c.borrow_mut() = v,
        _ => *slot = v,
    }
}

/// Assigns a captured variable (always boxed in the function that owns it).
pub fn write_capture(slot: &Value, v: Value) -> R<()> {
    match slot {
        Value::Cell(c) => {
            *c.borrow_mut() = v;
            Ok(())
        }
        _ => Err(Throw::Value(Value::error(
            "TypeError",
            "Assignment to a captured constant",
        ))),
    }
}

/// A new cell for a boxed slot's binding.
#[inline]
pub fn new_cell(v: Value) -> Value {
    Value::Cell(Rc::new(RefCell::new(v)))
}

/// A closure of function `f`.
#[inline]
pub fn closure(f: u32, captures: Vec<Value>) -> Value {
    Value::Func(Rc::new(Closure { func: f, captures }))
}

#[inline]
pub fn str(s: &str) -> Value {
    Value::str(s)
}

/// `{ k: v }` in an object literal.
pub fn obj_put(out: &mut Vec<(Str, Value)>, k: &str, v: Value) {
    crate::interp::obj_set(out, Rc::from(k), v);
}

/// `{ k: v }` with the key already made.
#[inline]
pub fn obj_put_str(out: &mut Vec<(Str, Value)>, k: Str, v: Value) {
    crate::interp::obj_set(out, k, v);
}

/// `{ [k]: v }`.
pub fn obj_put_key(out: &mut Vec<(Str, Value)>, k: &Value, v: Value) {
    crate::interp::obj_set(out, crate::interp::key_string(k), v);
}

/// `{ ...v }` in an object literal.
pub fn obj_spread(out: &mut Vec<(Str, Value)>, v: &Value) {
    match v {
        Value::Object(o) => {
            for (k, v) in o.borrow().iter() {
                crate::interp::obj_set(out, k.clone(), v.clone());
            }
        }
        Value::Array(a) => {
            for (i, v) in a.borrow().iter().enumerate() {
                crate::interp::obj_set(out, Rc::from(i.to_string().as_str()), v.clone());
            }
        }
        _ => {}
    }
}

/// The items an array pattern destructures.
pub fn array_items(v: &Value) -> R<Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a.borrow().clone()),
        Value::Str(s) => Ok(s.chars().map(|c| Value::str(&c.to_string())).collect()),
        other => type_error(format!("{} is not iterable", inspect(other))),
    }
}

/// An array pattern's `...rest`.
pub fn array_rest(items: &[Value], from: usize) -> Value {
    Value::array(items.get(from..).map(|t| t.to_vec()).unwrap_or_default())
}

/// An object pattern's check: `null` and `undefined` cannot be destructured.
pub fn destructure_check(v: &Value) -> R<()> {
    if v.is_nullish() {
        return type_error(format!(
            "Cannot destructure '{}' as it is {}.",
            v.to_js_string(),
            v.to_js_string()
        ));
    }
    Ok(())
}

/// An object pattern's `...rest`: the properties not named.
pub fn object_rest(v: &Value, names: &mut dyn Iterator<Item = &str>) -> Value {
    let names: Vec<&str> = names.collect();
    let out = match v {
        Value::Object(o) => o
            .borrow()
            .iter()
            .filter(|(k, _)| !names.iter().any(|n| *n == &**k))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    Value::object(out)
}

/// JavaScript's `ToInt32`.
#[inline]
pub fn to_int32(n: f64) -> i32 {
    crate::interp::to_int32(n)
}

/// `throw v`.
pub fn throw<T>(v: Value) -> R<T> {
    Err(Throw::Value(v))
}

/// A loop that ran the interpreter's iteration limit.
pub fn loop_limit<T>() -> R<T> {
    crate::runtime::js_error("RangeError", "loop did not terminate")
}

/// `<>…</>`.
pub fn fragment(children: Vec<Value>, key: Option<Str>) -> Value {
    Value::Elem(Rc::new(Elem::Fragment { children, key }))
}

/// `<Ctx.Provider value>…</Ctx.Provider>`.
pub fn provider(ctx: u32, value: Value, children: Vec<Value>, key: Option<Str>) -> Value {
    Value::Elem(Rc::new(Elem::Provider {
        ctx,
        value,
        children,
        key,
    }))
}

/// `a + b` on values (numbers add, anything with a string concatenates).
pub fn add(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Value::Num(x + y),
        _ => {
            let prim = |v: &Value| match v {
                Value::Array(_) | Value::Object(_) | Value::Func(_) => {
                    Value::str(&v.to_js_string())
                }
                other => other.clone(),
            };
            let (a, b) = (prim(a), prim(b));
            match (&a, &b) {
                (Value::Str(x), _) => {
                    let mut s = String::with_capacity(x.len() + 8);
                    s.push_str(x);
                    s.push_str(&b.to_js_string());
                    Value::str(&s)
                }
                (_, Value::Str(y)) => {
                    let mut s = a.to_js_string();
                    s.push_str(y);
                    Value::str(&s)
                }
                _ => Value::Num(a.to_number() + b.to_number()),
            }
        }
    }
}

/// `v === "s"`.
#[inline]
pub fn eq_str(v: &Value, s: &str) -> bool {
    matches!(v, Value::Str(x) if &**x == s)
}

/// `"s" + v`.
pub fn add_str_left(s: &str, v: &Value) -> Value {
    let b = v.to_js_string();
    let mut out = String::with_capacity(s.len() + b.len());
    out.push_str(s);
    out.push_str(&b);
    Value::str(&out)
}

/// `v + "s"`.
pub fn add_str_right(v: &Value, s: &str) -> Value {
    let mut out = v.to_js_string();
    out.push_str(s);
    Value::str(&out)
}

/// `a < b` and friends: strings compare by UTF-16 code units, the rest as numbers.
pub fn compare(op: BinaryOp, a: &Value, b: &Value) -> bool {
    let ord = match (a, b) {
        (Value::Str(x), Value::Str(y)) => Some(x.encode_utf16().cmp(y.encode_utf16())),
        _ => a.to_number().partial_cmp(&b.to_number()),
    };
    match (op, ord) {
        (_, None) => false,
        (BinaryOp::Lt, Some(o)) => o.is_lt(),
        (BinaryOp::LtEq, Some(o)) => o.is_le(),
        (BinaryOp::Gt, Some(o)) => o.is_gt(),
        (_, Some(o)) => o.is_ge(),
    }
}

impl Runtime {
    /// `f(...args)`.
    #[inline]
    pub fn call(&mut self, f: &Value, args: Vec<Value>) -> R<Value> {
        self.call_value(f, args)
    }

    /// `o.name`.
    #[inline]
    pub fn member(&mut self, o: &Value, name: &str) -> R<Value> {
        self.get_member(o, name)
    }

    /// `o[k]`.
    #[inline]
    pub fn index(&mut self, o: &Value, k: &Value) -> R<Value> {
        self.get_index(o, k)
    }

    /// `o.name = v`.
    #[inline]
    pub fn put_member(&mut self, o: &Value, name: &str, v: Value) -> R<()> {
        self.set_member(o, name, v)
    }

    /// `o[k] = v`.
    #[inline]
    pub fn put_index(&mut self, o: &Value, k: &Value, v: Value) -> R<()> {
        self.set_index(o, k, v)
    }

    /// A built-in method on a receiver.
    #[inline]
    pub fn call_method(&mut self, r: &Value, m: Method, args: Vec<Value>) -> R<Value> {
        self.method(r, m, args)
    }

    /// `recv.name(args)` resolved when it runs (`ir::Expr::Invoke`).
    pub fn invoke(&mut self, r: &Value, name: &str, args: Vec<Value>) -> R<Value> {
        self.invoke_by_name(r, name, args)
    }

    /// A built-in function or constant.
    #[inline]
    pub fn call_builtin(&mut self, b: Builtin, args: Vec<Value>) -> R<Value> {
        self.builtin(b, args)
    }

    /// A binary operator on values.
    #[inline]
    pub fn op(&mut self, op: BinaryOp, a: &Value, b: &Value) -> R<Value> {
        self.binary(op, a, b)
    }

    /// `...v` in an array literal or an argument list.
    pub fn spread(&mut self, out: &mut Vec<Value>, v: &Value) -> R<()> {
        out.extend(self.iterate(v)?);
        Ok(())
    }

    /// What `for...of` walks.
    #[inline]
    pub fn items_of(&mut self, v: &Value) -> R<Vec<Value>> {
        self.iterate(v)
    }

    /// A regular expression literal (a new object each evaluation).
    #[inline]
    pub fn regex(&mut self, pattern: &str, flags: &str) -> R<Value> {
        self.new_regex(pattern, flags)
    }

    #[inline]
    pub fn global(&self, i: usize) -> Value {
        self.globals[i].clone()
    }

    #[inline]
    pub fn set_global(&mut self, i: usize, v: Value) {
        self.globals[i] = v;
    }

    /// `createContext(default)` for the context global `i`.
    pub fn set_context_default(&mut self, i: u32, v: Value) {
        self.ctx_defaults.insert(i, v);
    }

    /// String literal `i` of this program: made the first time, shared after (a
    /// string's identity is its content, so sharing one is unobservable).
    #[inline]
    pub fn lit(&mut self, i: usize, s: &'static str) -> Value {
        if let Some(Some(v)) = self.lits.get(i) {
            return Value::Str(v.clone());
        }
        if self.lits.len() <= i {
            self.lits.resize(i + 1, None);
        }
        let v: Str = Rc::from(s);
        self.lits[i] = Some(v.clone());
        Value::Str(v)
    }

    /// String literal `i` as a property key.
    #[inline]
    pub fn lit_key(&mut self, i: usize, s: &'static str) -> Str {
        match self.lit(i, s) {
            Value::Str(k) => k,
            _ => unreachable!(),
        }
    }

    /// Whether hole skipping is sound for this program (with `inst`, a template
    /// element of a component's own render caches its holes).
    #[inline]
    pub fn is_pure_render(&self) -> bool {
        self.pure_render
    }
}

/// How a `try` block, its handler or its finaliser ended, in generated code (a
/// `return`, `break` or `continue` leaving it is carried out after the finaliser).
/// `Break` and `Continue` name the loop or `switch` by the generator's label number.
#[derive(Debug)]
pub enum Flow {
    Normal,
    Return(Value),
    Break(u32),
    Continue(u32),
}
