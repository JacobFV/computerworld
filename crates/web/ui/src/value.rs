//! Runtime values: JavaScript's value model for the compiled subset.
//!
//! Arrays and objects are shared and mutable (`Rc<RefCell<..>>`), as in JavaScript;
//! identity is `Rc` pointer identity, which is what `Object.is` and React's
//! dependency comparisons use. Closures copy their captured values when created.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cw_web::dom::NodeId;
use cw_web::script::FetchResponse;

pub type Str = Rc<str>;
pub type Obj = Rc<RefCell<Vec<(Str, Value)>>>;
pub type Arr = Rc<RefCell<Vec<Value>>>;

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(Str),
    Array(Arr),
    Object(Obj),
    Func(Rc<Closure>),
    /// A `useState` setter: instance, hook index.
    Setter(u32, u32),
    /// A `useReducer` dispatch: instance, hook index.
    Dispatch(u32, u32),
    /// A `useRef` object (`.current`).
    Ref(Rc<RefCell<Value>>),
    Elem(Rc<Elem>),
    /// A DOM element reached through a ref or an event.
    Node(NodeId),
    Event(Rc<EventObj>),
    Promise(Rc<RefCell<Promise>>),
    Response(Rc<FetchResponse>),
    /// A context object (`createContext`): its id.
    Context(u32),
    Regex(Rc<RegexObj>),
    /// An `Error` (or `TypeError`, …): name and message.
    Error(Rc<ErrorObj>),
    /// A boxed variable's cell (see `ir::Function::boxed`); never seen by programs,
    /// which read through it.
    Cell(Rc<RefCell<Value>>),
    /// A function the runtime provides: a promise's resolve/reject, a
    /// `Promise.all` slot, an async function's continuation.
    Native(Rc<crate::runtime::NativeFn>),
    /// A `Set`: its members in insertion order.
    Set(Arr),
    /// A `Map`: its entries in insertion order.
    Map(Rc<RefCell<Vec<(Value, Value)>>>),
    /// A `Date`: its time value (milliseconds since the epoch, or NaN).
    Date(Rc<Cell<f64>>),
    /// A value of the app's island (the JS VM running the code outside the
    /// compiled subset): its handle, one per VM object (see `crate::island`).
    Foreign(Rc<Foreign>),
}

/// The handle of a VM value.
#[derive(Debug)]
pub struct Foreign {
    pub id: u32,
    /// A function (for `typeof`).
    pub callable: bool,
    /// An array (for `Array.isArray` and rendering).
    pub array: bool,
    /// Where a dropped handle's id is queued for the island to free.
    pub(crate) drops: Option<Rc<RefCell<Vec<u32>>>>,
}

impl Drop for Foreign {
    fn drop(&mut self) {
        if let Some(d) = &self.drops {
            d.borrow_mut().push(self.id);
        }
    }
}

/// What a component element renders: a compiled function or an island's.
#[derive(Clone, Debug)]
pub enum ComponentFn {
    Compiled(Rc<Closure>),
    Foreign(Rc<Foreign>),
}

impl ComponentFn {
    /// The component a value is (`None` for anything else).
    pub fn of(v: Value) -> Option<ComponentFn> {
        match v {
            Value::Func(c) => Some(ComponentFn::Compiled(c)),
            Value::Foreign(f) if f.callable => Some(ComponentFn::Foreign(f)),
            _ => None,
        }
    }
    /// The function as a value.
    pub fn value(&self) -> Value {
        match self {
            ComponentFn::Compiled(c) => Value::Func(c.clone()),
            ComponentFn::Foreign(f) => Value::Foreign(f.clone()),
        }
    }
    /// The same function (as React compares element types).
    pub fn same(&self, o: &ComponentFn) -> bool {
        match (self, o) {
            (ComponentFn::Compiled(a), ComponentFn::Compiled(b)) => Rc::ptr_eq(a, b),
            (ComponentFn::Foreign(a), ComponentFn::Foreign(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct ErrorObj {
    pub name: Str,
    pub message: Str,
}

/// A `RegExp` object.
#[derive(Debug)]
pub struct RegexObj {
    pub source: Str,
    pub flags: Str,
    pub re: Rc<cw_regex::Regex>,
    /// `lastIndex`, in UTF-16 units (used by `g` and `y` regexes).
    pub last_index: Cell<usize>,
}

impl RegexObj {
    pub fn global(&self) -> bool {
        self.flags.contains('g')
    }
    pub fn sticky(&self) -> bool {
        self.flags.contains('y')
    }
}

#[derive(Debug)]
pub struct Closure {
    pub func: u32,
    pub captures: Vec<Value>,
}

/// A React element.
#[derive(Debug)]
pub enum Elem {
    Template {
        tid: u32,
        holes: Vec<Value>,
        key: Option<Str>,
    },
    Component {
        func: ComponentFn,
        props: Value,
        key: Option<Str>,
    },
    Fragment {
        children: Vec<Value>,
        key: Option<Str>,
    },
    Provider {
        ctx: u32,
        value: Value,
        children: Vec<Value>,
        key: Option<Str>,
    },
    /// `createPortal(children, container)`: children rendered into another node,
    /// in React's tree where the portal is.
    Portal {
        children: Vec<Value>,
        container: NodeId,
        key: Option<Str>,
    },
}

impl Elem {
    pub fn key(&self) -> Option<&Str> {
        match self {
            Elem::Template { key, .. }
            | Elem::Component { key, .. }
            | Elem::Fragment { key, .. }
            | Elem::Provider { key, .. }
            | Elem::Portal { key, .. } => key.as_ref(),
        }
    }
}

/// A synthetic event as a handler sees it.
#[derive(Debug)]
pub struct EventObj {
    pub ty: Str,
    pub target: NodeId,
    pub current_target: Cell<NodeId>,
    pub key: Str,
    pub code: Str,
    pub mods: cw_web::script::Modifiers,
    pub client_x: f64,
    pub client_y: f64,
    pub button: f64,
    pub detail: f64,
    pub delta_x: f64,
    pub delta_y: f64,
    pub repeat: bool,
    pub prevented: Cell<bool>,
    pub stopped: Cell<bool>,
    /// What an event of its type carries besides (`state` of a `popstate`,
    /// `oldURL`/`newURL` of a `hashchange`).
    pub extra: Vec<(Str, Value)>,
}

#[derive(Debug)]
pub enum PromiseState {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

#[derive(Debug)]
pub struct Promise {
    pub state: PromiseState,
    pub reactions: Vec<Reaction>,
}

#[derive(Clone, Debug)]
pub enum ReactionKind {
    Then,
    Catch,
    Finally,
}

#[derive(Clone, Debug)]
pub struct Reaction {
    pub kind: ReactionKind,
    pub on_fulfilled: Option<Value>,
    pub on_rejected: Option<Value>,
    pub result: Rc<RefCell<Promise>>,
}

impl Value {
    pub fn error(name: &str, message: &str) -> Value {
        Value::Error(Rc::new(ErrorObj {
            name: Rc::from(name),
            message: Rc::from(message),
        }))
    }

    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }
    pub fn array(v: Vec<Value>) -> Value {
        Value::Array(Rc::new(RefCell::new(v)))
    }
    pub fn object(v: Vec<(Str, Value)>) -> Value {
        Value::Object(Rc::new(RefCell::new(v)))
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            _ => true,
        }
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Func(_) | Value::Setter(..) | Value::Dispatch(..) | Value::Native(_) => {
                "function"
            }
            Value::Foreign(f) if f.callable => "function",
            _ => "object",
        }
    }

    /// JavaScript `ToNumber`.
    pub fn to_number(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Num(n) => *n,
            Value::Date(t) => t.get(),
            Value::Str(s) => string_to_number(s),
            Value::Array(a) => {
                let a = a.borrow();
                match a.len() {
                    0 => 0.0,
                    1 => a[0].to_number(),
                    _ => f64::NAN,
                }
            }
            _ => f64::NAN,
        }
    }

    /// JavaScript `ToString`.
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Undefined => "undefined".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => number_to_string(*n),
            Value::Str(s) => s.to_string(),
            Value::Array(a) => a
                .borrow()
                .iter()
                .map(|v| {
                    if v.is_nullish() {
                        String::new()
                    } else {
                        v.to_js_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(","),
            Value::Func(_) | Value::Setter(..) | Value::Dispatch(..) => {
                "function () { [native code] }".into()
            }
            Value::Promise(_) => "[object Promise]".into(),
            Value::Regex(r) => format!("/{}/{}", r.source, r.flags),
            Value::Error(e) => {
                if e.message.is_empty() {
                    e.name.to_string()
                } else {
                    format!("{}: {}", e.name, e.message)
                }
            }
            Value::Cell(c) => c.borrow().to_js_string(),
            Value::Native(_) => "function () { [native code] }".into(),
            Value::Set(_) => "[object Set]".into(),
            Value::Date(t) => cw_jsvm::builtins::date::date_to_string(t.get()),
            Value::Map(_) => "[object Map]".into(),
            Value::Response(_) => "[object Response]".into(),
            _ => "[object Object]".into(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// `Object.is` (`SameValue`).
pub fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else {
                x == y && (x.to_bits() == y.to_bits() || *x != 0.0)
            }
        }
        _ => strict_equals_ref(a, b),
    }
}

/// `===`.
pub fn strict_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x == y,
        _ => strict_equals_ref(a, b),
    }
}

fn strict_equals_ref(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => Rc::ptr_eq(x, y),
        (Value::Object(x), Value::Object(y)) => Rc::ptr_eq(x, y),
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        (Value::Setter(a, b), Value::Setter(c, d)) => a == c && b == d,
        (Value::Dispatch(a, b), Value::Dispatch(c, d)) => a == c && b == d,
        (Value::Ref(x), Value::Ref(y)) => Rc::ptr_eq(x, y),
        (Value::Elem(x), Value::Elem(y)) => Rc::ptr_eq(x, y),
        (Value::Node(x), Value::Node(y)) => x == y,
        (Value::Event(x), Value::Event(y)) => Rc::ptr_eq(x, y),
        (Value::Promise(x), Value::Promise(y)) => Rc::ptr_eq(x, y),
        (Value::Response(x), Value::Response(y)) => Rc::ptr_eq(x, y),
        (Value::Context(x), Value::Context(y)) => x == y,
        (Value::Regex(x), Value::Regex(y)) => Rc::ptr_eq(x, y),
        (Value::Error(x), Value::Error(y)) => Rc::ptr_eq(x, y),
        (Value::Native(x), Value::Native(y)) => Rc::ptr_eq(x, y),
        (Value::Set(x), Value::Set(y)) => Rc::ptr_eq(x, y),
        (Value::Map(x), Value::Map(y)) => Rc::ptr_eq(x, y),
        (Value::Date(x), Value::Date(y)) => Rc::ptr_eq(x, y),
        (Value::Foreign(x), Value::Foreign(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// `==`.
pub fn loose_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
        (Value::Undefined | Value::Null, _) | (_, Value::Undefined | Value::Null) => false,
        (Value::Num(_), Value::Str(_)) | (Value::Str(_), Value::Num(_)) => {
            a.to_number() == b.to_number()
        }
        (Value::Bool(_), _) => loose_equals(&Value::Num(a.to_number()), b),
        (_, Value::Bool(_)) => loose_equals(a, &Value::Num(b.to_number())),
        (Value::Array(_) | Value::Object(_) | Value::Date(_), Value::Num(_) | Value::Str(_)) => {
            loose_equals(&Value::str(&a.to_js_string()), b)
        }
        (Value::Num(_) | Value::Str(_), Value::Array(_) | Value::Object(_) | Value::Date(_)) => {
            loose_equals(a, &Value::str(&b.to_js_string()))
        }
        _ => strict_equals(a, b),
    }
}

/// Whether two dependency values are unchanged for skipping work: `Object.is`, and
/// for closures of the same function, the same captured values (a closure recreated
/// from unchanged inputs behaves identically).
pub fn same_dep(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Func(x), Value::Func(y)) => {
            Rc::ptr_eq(x, y)
                || (x.func == y.func
                    && x.captures.len() == y.captures.len()
                    && x.captures
                        .iter()
                        .zip(&y.captures)
                        .all(|(p, q)| same_dep(p, q)))
        }
        _ => same_value(a, b),
    }
}

/// JavaScript `StringToNumber`.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(h, 16)
            .map(|v| v as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(b) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return u64::from_str_radix(b, 2)
            .map(|v| v as f64)
            .unwrap_or(f64::NAN);
    }
    if let Some(o) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return u64::from_str_radix(o, 8)
            .map(|v| v as f64)
            .unwrap_or(f64::NAN);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if t.chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && !t.contains("inf")
    {
        t.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

/// JavaScript `Number::toString(10)`: the shortest round-tripping digits, in
/// exponent form outside `[1e-7, 1e21)`.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let neg = n < 0.0;
    let a = n.abs();
    // Shortest digits and exponent from Rust's `{:e}` (also shortest round-trip).
    let e = format!("{a:e}");
    let (mantissa, exp) = e.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n_pos = exp + 1; // position of the decimal point relative to the digits
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if k <= n_pos && n_pos <= 21 {
        out.push_str(&digits);
        for _ in 0..(n_pos - k) {
            out.push('0');
        }
    } else if 0 < n_pos && n_pos <= 21 {
        out.push_str(&digits[..n_pos as usize]);
        out.push('.');
        out.push_str(&digits[n_pos as usize..]);
    } else if -6 < n_pos && n_pos <= 0 {
        out.push_str("0.");
        for _ in 0..(-n_pos) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let e = n_pos - 1;
        out.push(if e >= 0 { '+' } else { '-' });
        out.push_str(&e.abs().to_string());
    }
    out
}

/// `Number.prototype.toFixed(digits)`: exact decimal expansion rounded half up.
pub fn to_fixed(n: f64, digits: usize) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.abs() >= 1e21 {
        return number_to_string(n);
    }
    let digits = digits.min(100);
    // The exact expansion: every finite double has at most 1074 fraction digits.
    let exact = format!("{:.1100}", n.abs());
    let (int, frac) = exact.split_once('.').expect("fraction");
    let mut kept: Vec<u8> = int.bytes().chain(frac.bytes().take(digits)).collect();
    let next = frac.as_bytes().get(digits).copied().unwrap_or(b'0');
    if next >= b'5' {
        // Round half up (the spec picks the larger n on a tie).
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let int_len = kept.len() - digits;
    let mut out = String::new();
    if n < 0.0 && kept.iter().any(|b| *b != b'0') {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&kept[..int_len]).expect("digits"));
    if digits > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(&kept[int_len..]).expect("digits"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (n, s) in [
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (123456789.125, "123456789.125"),
            (f64::NAN, "NaN"),
            (-0.0, "0"),
            (2.5e-10, "2.5e-10"),
        ] {
            assert_eq!(number_to_string(n), s, "{n}");
        }
    }

    #[test]
    fn to_fixed_rounds_half_up_on_the_exact_value() {
        assert_eq!(to_fixed(1.005, 2), "1.00"); // 1.00499999999999989...
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(-1.5, 0), "-2");
        assert_eq!(to_fixed(12.3456, 1), "12.3");
        assert_eq!(to_fixed(0.0, 2), "0.00");
        assert_eq!(to_fixed(99.99, 1), "100.0");
    }

    #[test]
    fn string_to_number_follows_javascript() {
        assert_eq!(string_to_number(" 42 "), 42.0);
        assert_eq!(string_to_number(""), 0.0);
        assert!(string_to_number("4x").is_nan());
        assert_eq!(string_to_number("0x10"), 16.0);
        assert_eq!(string_to_number("1e3"), 1000.0);
    }
}
