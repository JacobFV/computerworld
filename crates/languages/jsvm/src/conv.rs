//! Type conversions (ToPrimitive, ToNumber, ToString, ...) and operators.

use crate::bigint::BigInt;
use crate::numconv::{number_to_string, string_to_number, to_int32, to_uint32};
use crate::value::*;
use crate::vm::Vm;
use std::cmp::Ordering;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    Default,
    Number,
    String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arith {
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    Shl,
    Shr,
    UShr,
    BitAnd,
    BitOr,
    BitXor,
}

pub fn js_mod(a: f64, b: f64) -> f64 {
    if b.is_infinite() && a.is_finite() {
        return a;
    }
    let r = a % b;
    if r == 0.0 {
        // Keep the sign of the dividend.
        if a.is_sign_negative() {
            -0.0
        } else {
            0.0
        }
    } else {
        r
    }
}

pub fn js_pow(a: f64, b: f64) -> f64 {
    if b.is_nan() {
        return f64::NAN;
    }
    if b == 0.0 {
        return 1.0;
    }
    if (a == 1.0 || a == -1.0) && b.is_infinite() {
        return f64::NAN;
    }
    a.powf(b)
}

impl<'h> Vm<'h> {
    pub fn to_primitive(&mut self, v: &Value, hint: Hint) -> JsResult<Value> {
        let Value::Obj(o) = v else {
            return Ok(v.clone());
        };
        let tp = self.get_from(o, &Key::Sym(self.syms.to_primitive.clone()), v)?;
        if !tp.is_nullish() {
            if !tp.is_callable() {
                return Err(self.type_error("Symbol.toPrimitive is not a function"));
            }
            let h = match hint {
                Hint::Default => "default",
                Hint::Number => "number",
                Hint::String => "string",
            };
            let r = self.call(&tp, v.clone(), vec![Value::str(h)])?;
            if let Value::Obj(_) = r {
                return Err(self.type_error("Cannot convert object to primitive value"));
            }
            return Ok(r);
        }
        let order = if hint == Hint::String {
            ["toString", "valueOf"]
        } else {
            ["valueOf", "toString"]
        };
        for m in order {
            let f = self.get_from(o, &Key::str(m), v)?;
            if f.is_callable() {
                let r = self.call(&f, v.clone(), vec![])?;
                if !matches!(r, Value::Obj(_)) {
                    return Ok(r);
                }
            }
        }
        Err(self.type_error("Cannot convert object to primitive value"))
    }

    #[inline]
    pub fn to_number(&mut self, v: &Value) -> JsResult<f64> {
        if let Value::Num(n) = v {
            return Ok(*n);
        }
        self.convert_number_slow(v)
    }

    fn convert_number_slow(&mut self, v: &Value) -> JsResult<f64> {
        Ok(match v {
            Value::Undefined | Value::Empty => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Num(n) => *n,
            Value::Str(s) => string_to_number(s),
            Value::Sym(_) => {
                return Err(self.type_error("Cannot convert a Symbol value to a number"))
            }
            Value::BigInt(_) => {
                return Err(self.type_error("Cannot convert a BigInt value to a number"))
            }
            Value::Obj(_) => {
                let p = self.to_primitive(v, Hint::Number)?;
                return self.to_number(&p);
            }
        })
    }

    pub fn to_numeric(&mut self, v: &Value) -> JsResult<Value> {
        match v {
            Value::Num(_) | Value::BigInt(_) => Ok(v.clone()),
            Value::Obj(_) => {
                let p = self.to_primitive(v, Hint::Number)?;
                self.to_numeric(&p)
            }
            _ => Ok(Value::Num(self.to_number(v)?)),
        }
    }

    #[inline]
    pub fn to_string(&mut self, v: &Value) -> JsResult<JsStr> {
        if let Value::Str(s) = v {
            return Ok(s.clone());
        }
        self.convert_string_slow(v)
    }

    fn convert_string_slow(&mut self, v: &Value) -> JsResult<JsStr> {
        Ok(match v {
            Value::Str(s) => s.clone(),
            Value::Num(n) => JsStr::new(number_to_string(*n)),
            Value::Bool(b) => JsStr::new(if *b { "true" } else { "false" }),
            Value::Undefined | Value::Empty => JsStr::new("undefined"),
            Value::Null => JsStr::new("null"),
            Value::BigInt(b) => JsStr::new(b.to_str_radix(10)),
            Value::Sym(_) => {
                return Err(self.type_error("Cannot convert a Symbol value to a string"))
            }
            Value::Obj(_) => {
                let p = self.to_primitive(v, Hint::String)?;
                return self.to_string(&p);
            }
        })
    }

    pub fn to_str(&mut self, v: &Value) -> JsResult<String> {
        Ok(self.to_string(v)?.to_string())
    }

    pub fn to_object(&mut self, v: &Value) -> JsResult<Obj> {
        Ok(match v {
            Value::Obj(o) => o.clone(),
            Value::Undefined | Value::Null | Value::Empty => {
                return Err(self.type_error("Cannot convert undefined or null to object"))
            }
            Value::Bool(b) => {
                self.obj_with(Some(self.intr.boolean_proto.clone()), Kind::Boolean(*b))
            }
            Value::Num(n) => self.obj_with(Some(self.intr.number_proto.clone()), Kind::Number(*n)),
            Value::Str(s) => self.obj_with(
                Some(self.intr.string_proto.clone()),
                Kind::String(s.clone()),
            ),
            Value::Sym(s) => self.obj_with(
                Some(self.intr.symbol_proto.clone()),
                Kind::Symbol(s.clone()),
            ),
            Value::BigInt(b) => self.obj_with(
                Some(self.intr.bigint_proto.clone()),
                Kind::BigInt(b.clone()),
            ),
        })
    }

    pub fn to_key(&mut self, v: &Value) -> JsResult<Key> {
        Ok(match v {
            Value::Str(s) => Key::Str(s.clone()),
            Value::Sym(s) => Key::Sym(s.clone()),
            Value::Num(n) => {
                if *n >= 0.0 && *n < 4294967295.0 && n.fract() == 0.0 {
                    Key::Str(JsStr::new((*n as u64).to_string()))
                } else {
                    Key::Str(JsStr::new(number_to_string(*n)))
                }
            }
            Value::Obj(_) => {
                let p = self.to_primitive(v, Hint::String)?;
                return self.to_key(&p);
            }
            _ => Key::Str(self.to_string(v)?),
        })
    }

    #[inline]
    pub fn to_integer(&mut self, v: &Value) -> JsResult<f64> {
        if let Value::Num(n) = v {
            if n.is_finite() {
                return Ok(n.trunc() + 0.0);
            }
        }
        self.convert_integer_slow(v)
    }

    fn convert_integer_slow(&mut self, v: &Value) -> JsResult<f64> {
        let n = self.to_number(v)?;
        Ok(if n.is_nan() {
            0.0
        } else if n.is_infinite() {
            n
        } else {
            n.trunc() + 0.0
        })
    }

    pub fn to_length(&mut self, v: &Value) -> JsResult<usize> {
        let n = self.to_integer(v)?;
        Ok(if n <= 0.0 {
            0
        } else {
            n.min(9007199254740991.0) as usize
        })
    }

    pub fn to_i32(&mut self, v: &Value) -> JsResult<i32> {
        if let Value::Num(n) = v {
            return Ok(to_int32(*n));
        }
        let n = self.to_number(v)?;
        Ok(to_int32(n))
    }

    pub fn to_u32(&mut self, v: &Value) -> JsResult<u32> {
        let n = self.to_number(v)?;
        Ok(to_uint32(n))
    }

    /// Relative index argument (`slice`, `at`, ...) clamped to [0, len].
    pub fn rel_index(&mut self, v: &Value, len: usize, default: usize) -> JsResult<usize> {
        if v.is_undefined() {
            return Ok(default);
        }
        let n = self.to_integer(v)?;
        Ok(if n < 0.0 {
            (len as f64 + n).max(0.0) as usize
        } else {
            n.min(len as f64) as usize
        })
    }

    // ------------------------------------------------------------ operators
    pub fn op_add(&mut self, a: &Value, b: &Value) -> JsResult<Value> {
        match (a, b) {
            (Value::Num(x), Value::Num(y)) => return Ok(Value::Num(x + y)),
            (Value::Str(x), Value::Str(y)) => {
                let mut s = String::with_capacity(x.len() + y.len());
                s.push_str(x);
                s.push_str(y);
                return Ok(Value::string(s));
            }
            _ => {}
        }
        let pa = self.to_primitive(a, Hint::Default)?;
        let pb = self.to_primitive(b, Hint::Default)?;
        if matches!(pa, Value::Str(_)) || matches!(pb, Value::Str(_)) {
            let sa = self.to_string(&pa)?;
            let sb = self.to_string(&pb)?;
            let mut s = String::with_capacity(sa.len() + sb.len());
            s.push_str(&sa);
            s.push_str(&sb);
            return Ok(Value::string(s));
        }
        let na = self.to_numeric(&pa)?;
        let nb = self.to_numeric(&pb)?;
        match (&na, &nb) {
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::BigInt(Rc::new(x.add(y)))),
            (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x + y)),
            _ => {
                Err(self.type_error("Cannot mix BigInt and other types, use explicit conversions"))
            }
        }
    }

    pub fn arith(&mut self, op: Arith, a: &Value, b: &Value) -> JsResult<Value> {
        if let (Value::Num(x), Value::Num(y)) = (a, b) {
            return Ok(Value::Num(num_arith(op, *x, *y)));
        }
        let na = self.to_numeric(a)?;
        let nb = self.to_numeric(b)?;
        match (&na, &nb) {
            (Value::Num(x), Value::Num(y)) => Ok(Value::Num(num_arith(op, *x, *y))),
            (Value::BigInt(x), Value::BigInt(y)) => self.big_arith(op, x, y),
            _ => {
                Err(self.type_error("Cannot mix BigInt and other types, use explicit conversions"))
            }
        }
    }

    fn big_arith(&mut self, op: Arith, x: &BigInt, y: &BigInt) -> JsResult<Value> {
        let r = match op {
            Arith::Sub => x.sub(y),
            Arith::Mul => x.mul(y),
            Arith::Div => {
                if y.is_zero() {
                    return Err(self.range_error("Division by zero"));
                }
                x.divrem_trunc(y).0
            }
            Arith::Mod => {
                if y.is_zero() {
                    return Err(self.range_error("Division by zero"));
                }
                x.divrem_trunc(y).1
            }
            Arith::Exp => {
                if y.is_negative() {
                    return Err(self.range_error("Exponent must be non-negative"));
                }
                let e = y.to_i64().unwrap_or(i64::MAX);
                if e > 1_000_000 && !(x.is_zero() || x.to_i64() == Some(1)) {
                    return Err(self.range_error("Maximum BigInt size exceeded"));
                }
                x.pow(e as u64)
            }
            Arith::Shl | Arith::Shr => {
                let s = y.to_i64().unwrap_or(0);
                let left = (op == Arith::Shl) == (s >= 0);
                let n = s.unsigned_abs();
                if left {
                    if n > 10_000_000 {
                        return Err(self.range_error("Maximum BigInt size exceeded"));
                    }
                    x.shl(n)
                } else {
                    x.shr(n)
                }
            }
            Arith::UShr => {
                return Err(self.type_error("BigInts have no unsigned right shift, use >> instead"))
            }
            Arith::BitAnd => x.bitop(y, '&'),
            Arith::BitOr => x.bitop(y, '|'),
            Arith::BitXor => x.bitop(y, '^'),
        };
        Ok(Value::BigInt(Rc::new(r)))
    }

    pub fn loose_eq(&mut self, a: &Value, b: &Value) -> JsResult<bool> {
        Ok(match (a, b) {
            (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
            (Value::Undefined | Value::Null, _) | (_, Value::Undefined | Value::Null) => false,
            (Value::Num(x), Value::Num(y)) => x == y,
            (Value::Str(x), Value::Str(y)) => x == y,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Sym(_), Value::Sym(_)) | (Value::Obj(_), Value::Obj(_)) => strict_equals(a, b),
            (Value::BigInt(x), Value::BigInt(y)) => x == y,
            (Value::Num(x), Value::Str(s)) | (Value::Str(s), Value::Num(x)) => {
                *x == string_to_number(s)
            }
            (Value::BigInt(x), Value::Str(s)) | (Value::Str(s), Value::BigInt(x)) => {
                match parse_bigint_str(s) {
                    Some(y) => **x == y,
                    None => false,
                }
            }
            (Value::BigInt(x), Value::Num(n)) | (Value::Num(n), Value::BigInt(x)) => {
                big_num_cmp(x, *n) == Some(Ordering::Equal)
            }
            (Value::Bool(bv), _) => {
                let n = Value::Num(if *bv { 1.0 } else { 0.0 });
                return self.loose_eq(&n, b);
            }
            (_, Value::Bool(bv)) => {
                let n = Value::Num(if *bv { 1.0 } else { 0.0 });
                return self.loose_eq(a, &n);
            }
            (Value::Obj(_), _) => {
                let p = self.to_primitive(a, Hint::Default)?;
                return self.loose_eq(&p, b);
            }
            (_, Value::Obj(_)) => {
                let p = self.to_primitive(b, Hint::Default)?;
                return self.loose_eq(a, &p);
            }
            _ => false,
        })
    }

    /// Abstract relational comparison a < b. None means undefined (NaN).
    pub fn less_than(&mut self, a: &Value, b: &Value, left_first: bool) -> JsResult<Option<bool>> {
        if let (Value::Num(x), Value::Num(y)) = (a, b) {
            if x.is_nan() || y.is_nan() {
                return Ok(None);
            }
            return Ok(Some(x < y));
        }
        let (pa, pb) = if left_first {
            let pa = self.to_primitive(a, Hint::Number)?;
            let pb = self.to_primitive(b, Hint::Number)?;
            (pa, pb)
        } else {
            let pb = self.to_primitive(b, Hint::Number)?;
            let pa = self.to_primitive(a, Hint::Number)?;
            (pa, pb)
        };
        if let (Value::Str(x), Value::Str(y)) = (&pa, &pb) {
            return Ok(Some(cmp_utf16(x, y) == Ordering::Less));
        }
        match (&pa, &pb) {
            (Value::BigInt(x), Value::Str(s)) => {
                return Ok(parse_bigint_str(s).map(|y| x.cmp(&y) == Ordering::Less))
            }
            (Value::Str(s), Value::BigInt(y)) => {
                return Ok(parse_bigint_str(s).map(|x| x.cmp(y) == Ordering::Less))
            }
            _ => {}
        }
        let na = self.to_numeric(&pa)?;
        let nb = self.to_numeric(&pb)?;
        Ok(match (&na, &nb) {
            (Value::Num(x), Value::Num(y)) => {
                if x.is_nan() || y.is_nan() {
                    None
                } else {
                    Some(x < y)
                }
            }
            (Value::BigInt(x), Value::BigInt(y)) => Some(x.cmp(y) == Ordering::Less),
            (Value::BigInt(x), Value::Num(y)) => big_num_cmp(x, *y).map(|o| o == Ordering::Less),
            (Value::Num(x), Value::BigInt(y)) => big_num_cmp(y, *x).map(|o| o == Ordering::Greater),
            _ => None,
        })
    }

    pub fn instance_of(&mut self, v: &Value, target: &Value) -> JsResult<bool> {
        let Value::Obj(t) = target else {
            return Err(self.type_error("Right-hand side of 'instanceof' is not an object"));
        };
        let hi = self.get_from(t, &Key::Sym(self.syms.has_instance.clone()), target)?;
        if !hi.is_nullish() && !self.is_default_has_instance(&hi) {
            let r = self.call(&hi, target.clone(), vec![v.clone()])?;
            return Ok(r.truthy());
        }
        if !t.is_callable() {
            return Err(self.type_error("Right-hand side of 'instanceof' is not callable"));
        }
        self.ordinary_has_instance(t, v)
    }

    fn is_default_has_instance(&self, f: &Value) -> bool {
        match f {
            Value::Obj(o) => {
                matches!(o.own_value("name"), Some(Value::Str(s)) if s.as_str() == "[Symbol.hasInstance]")
            }
            _ => false,
        }
    }

    pub fn ordinary_has_instance(&mut self, t: &Obj, v: &Value) -> JsResult<bool> {
        // Bound functions delegate to their target.
        let bound = match &t.borrow().kind {
            Kind::Function(fd) => match &fd.imp {
                FuncImpl::Bound { target, .. } => Some(target.clone()),
                _ => None,
            },
            _ => None,
        };
        if let Some(b) = bound {
            return self.instance_of(v, &Value::Obj(b));
        }
        let Value::Obj(o) = v else { return Ok(false) };
        let proto = self.get_from(t, &Key::str("prototype"), &Value::Obj(t.clone()))?;
        let Value::Obj(proto) = proto else {
            return Err(self
                .type_error("Function has non-object prototype 'undefined' in instanceof check"));
        };
        let mut cur = o.proto();
        while let Some(c) = cur {
            if c.ptr_eq(&proto) {
                return Ok(true);
            }
            cur = c.proto();
        }
        Ok(false)
    }

    pub fn op_in(&mut self, key: &Value, obj: &Value) -> JsResult<bool> {
        let Value::Obj(o) = obj else {
            let ks = match key {
                Value::Sym(_) => "Symbol()".to_string(),
                _ => self.to_str(key)?,
            };
            let d = self.display_primitive(obj);
            return Err(self.type_error(format!(
                "Cannot use 'in' operator to search for '{ks}' in {d}"
            )));
        };
        let k = self.to_key(key)?;
        self.has_property(o, &k)
    }

    pub fn display_primitive(&mut self, v: &Value) -> String {
        match v {
            Value::Str(s) => s.to_string(),
            Value::Sym(s) => format!(
                "Symbol({})",
                s.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
            ),
            _ => self.to_str(v).unwrap_or_default(),
        }
    }

    pub fn neg(&mut self, v: &Value) -> JsResult<Value> {
        match self.to_numeric(v)? {
            Value::BigInt(b) => Ok(Value::BigInt(Rc::new(b.neg()))),
            Value::Num(n) => Ok(Value::Num(-n)),
            _ => unreachable!(),
        }
    }

    pub fn bitnot(&mut self, v: &Value) -> JsResult<Value> {
        match self.to_numeric(v)? {
            Value::BigInt(b) => Ok(Value::BigInt(Rc::new(b.neg().sub(&BigInt::from_i64(1))))),
            Value::Num(n) => Ok(Value::Num(!to_int32(n) as f64)),
            _ => unreachable!(),
        }
    }

    pub fn inc_dec(&mut self, v: &Value, inc: bool) -> JsResult<Value> {
        if let Value::Num(n) = v {
            return Ok(Value::Num(if inc { n + 1.0 } else { n - 1.0 }));
        }
        match self.to_numeric(v)? {
            Value::BigInt(b) => {
                let one = BigInt::from_i64(1);
                Ok(Value::BigInt(Rc::new(if inc {
                    b.add(&one)
                } else {
                    b.sub(&one)
                })))
            }
            Value::Num(n) => Ok(Value::Num(if inc { n + 1.0 } else { n - 1.0 })),
            _ => unreachable!(),
        }
    }
}

pub fn num_arith(op: Arith, x: f64, y: f64) -> f64 {
    match op {
        Arith::Sub => x - y,
        Arith::Mul => x * y,
        Arith::Div => x / y,
        Arith::Mod => js_mod(x, y),
        Arith::Exp => js_pow(x, y),
        Arith::Shl => (to_int32(x).wrapping_shl(to_uint32(y) & 31)) as f64,
        Arith::Shr => (to_int32(x) >> (to_uint32(y) & 31)) as f64,
        Arith::UShr => (to_uint32(x) >> (to_uint32(y) & 31)) as f64,
        Arith::BitAnd => (to_int32(x) & to_int32(y)) as f64,
        Arith::BitOr => (to_int32(x) | to_int32(y)) as f64,
        Arith::BitXor => (to_int32(x) ^ to_int32(y)) as f64,
    }
}

pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    a.encode_utf16().cmp(b.encode_utf16())
}

pub fn parse_bigint_str(s: &str) -> Option<BigInt> {
    let t = s.trim_matches(crate::numconv::is_js_whitespace);
    if t.is_empty() {
        return Some(BigInt::zero());
    }
    let lower = t.to_ascii_lowercase();
    for (p, r) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = lower.strip_prefix(p) {
            return BigInt::parse_digits(rest, r);
        }
    }
    let (neg, digits) = if let Some(r) = t.strip_prefix('-') {
        (true, r)
    } else if let Some(r) = t.strip_prefix('+') {
        (false, r)
    } else {
        (false, t)
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v = BigInt::parse_digits(digits, 10)?;
    Some(if neg { v.neg() } else { v })
}

pub fn big_num_cmp(x: &BigInt, n: f64) -> Option<Ordering> {
    if n.is_nan() {
        return None;
    }
    if n == f64::INFINITY {
        return Some(Ordering::Less);
    }
    if n == f64::NEG_INFINITY {
        return Some(Ordering::Greater);
    }
    let fl = n.floor();
    let b = BigInt::from_f64(fl);
    match x.cmp(&b) {
        Ordering::Equal => {
            if n > fl {
                Some(Ordering::Less)
            } else {
                Some(Ordering::Equal)
            }
        }
        o => Some(o),
    }
}
