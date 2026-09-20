//! `JSON.parse` / `JSON.stringify` with V8's error messages.

use crate::numconv::number_to_string;
use crate::value::*;
use crate::vm::Vm;

struct P<'a> {
    s: &'a [u16],
    src: &'a str,
    i: usize,
}

enum Tok {
    Eos,
    Number,
    Str,
    Other,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], 0x20 | 0x09 | 0x0a | 0x0d) {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u16> {
        self.s.get(self.i).copied()
    }
    fn tok_at(&self) -> Tok {
        match self.peek() {
            None => Tok::Eos,
            Some(c) if c == b'-' as u16 || (c >= b'0' as u16 && c <= b'9' as u16) => Tok::Number,
            Some(c) if c == b'"' as u16 => Tok::Str,
            _ => Tok::Other,
        }
    }
    fn pos_suffix(&self, pos: usize) -> String {
        let before = &self.s[..pos.min(self.s.len())];
        let line = before.iter().filter(|&&c| c == b'\n' as u16).count() + 1;
        let col = match before.iter().rposition(|&c| c == b'\n' as u16) {
            Some(nl) => pos - nl,
            None => pos + 1,
        };
        format!("in JSON at position {pos} (line {line} column {col})")
    }
    fn unexpected(&self, msg: Option<&str>) -> String {
        let pos = self.i;
        match self.tok_at() {
            Tok::Eos => return "Unexpected end of JSON input".to_string(),
            _ if msg.is_some() => {
                let m = msg.unwrap();
                let suffix = self.pos_suffix(pos);
                // "... after JSON at position N" (no second "in JSON").
                if m.ends_with("after JSON") {
                    return format!("{m} {}", suffix.trim_start_matches("in JSON "));
                }
                return format!("{m} {suffix}");
            }
            Tok::Number => return format!("Unexpected number {}", self.pos_suffix(pos)),
            Tok::Str => return format!("Unexpected string {}", self.pos_suffix(pos)),
            Tok::Other => {}
        }
        if matches!(
            self.src,
            "NaN" | "Infinity" | "undefined" | "[object Object]"
        ) {
            return format!("\"{}\" is not valid JSON", self.src);
        }
        let c = String::from_utf16_lossy(&[self.s[pos]]);
        let len = self.s.len();
        const K: usize = 10;
        if len < K * 2 + 1 {
            return format!("Unexpected token '{c}', \"{}\" is not valid JSON", self.src);
        }
        let (start, end, pre, post) = if pos < K {
            (0, pos + K, "", "...")
        } else if pos < len - K {
            (pos - K, pos + K, "...", "...")
        } else {
            (pos - K, len, "...", "")
        };
        let sub = String::from_utf16_lossy(&self.s[start..end.min(len)]);
        format!("Unexpected token '{c}', {pre}\"{sub}\"{post} is not valid JSON")
    }

    fn value(&mut self, vm: &mut Vm, depth: usize) -> Result<Value, String> {
        if depth > 5000 {
            return Err("Maximum call stack size exceeded".into());
        }
        self.ws();
        let Some(c) = self.peek() else {
            return Err(self.unexpected(None));
        };
        match c as u8 {
            b'{' if c < 128 => {
                self.i += 1;
                let o = vm.new_object();
                self.ws();
                if self.peek() == Some(b'}' as u16) {
                    self.i += 1;
                    return Ok(Value::Obj(o));
                }
                let mut first = true;
                loop {
                    self.ws();
                    if self.peek() != Some(b'"' as u16) {
                        return Err(self.unexpected(Some(if first {
                            "Expected property name or '}'"
                        } else {
                            "Expected double-quoted property name"
                        })));
                    }
                    first = false;
                    let k = self.string()?;
                    self.ws();
                    if self.peek() != Some(b':' as u16) {
                        return Err(self.unexpected(Some("Expected ':' after property name")));
                    }
                    self.i += 1;
                    let v = self.value(vm, depth + 1)?;
                    let key = Key::Str(JsStr::new(k));
                    let _ = vm.create_data_property(&o, key, v);
                    self.ws();
                    match self.peek() {
                        Some(x) if x == b',' as u16 => {
                            self.i += 1;
                        }
                        Some(x) if x == b'}' as u16 => {
                            self.i += 1;
                            return Ok(Value::Obj(o));
                        }
                        _ => {
                            return Err(
                                self.unexpected(Some("Expected ',' or '}' after property value"))
                            )
                        }
                    }
                }
            }
            b'[' if c < 128 => {
                self.i += 1;
                let mut items = vec![];
                self.ws();
                if self.peek() == Some(b']' as u16) {
                    self.i += 1;
                    return Ok(vm.arr(items));
                }
                loop {
                    items.push(self.value(vm, depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(x) if x == b',' as u16 => {
                            self.i += 1;
                        }
                        Some(x) if x == b']' as u16 => {
                            self.i += 1;
                            return Ok(vm.arr(items));
                        }
                        _ => {
                            return Err(
                                self.unexpected(Some("Expected ',' or ']' after array element"))
                            )
                        }
                    }
                }
            }
            b'"' if c < 128 => Ok(Value::string(self.string()?)),
            b't' | b'f' | b'n' if c < 128 => {
                for (lit, v) in [
                    ("true", Value::Bool(true)),
                    ("false", Value::Bool(false)),
                    ("null", Value::Null),
                ] {
                    if lit.as_bytes()[0] as u16 == c {
                        for (j, b) in lit.bytes().enumerate() {
                            match self.s.get(self.i + j) {
                                None => {
                                    self.i += j;
                                    return Err(self.unexpected(None));
                                }
                                Some(&x) if x == b as u16 => {}
                                Some(_) => {
                                    self.i += j;
                                    return Err(self.unexpected(None));
                                }
                            }
                        }
                        self.i += lit.len();
                        return Ok(v);
                    }
                }
                Err(self.unexpected(None))
            }
            b'-' | b'0'..=b'9' if c < 128 => self.number(),
            _ => Err(self.unexpected(None)),
        }
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        let digit =
            |c: Option<u16>| matches!(c, Some(x) if (b'0' as u16..=b'9' as u16).contains(&x));
        if self.peek() == Some(b'-' as u16) {
            self.i += 1;
            if !digit(self.peek()) {
                return Err(format!(
                    "No number after minus sign {}",
                    self.pos_suffix(self.i)
                ));
            }
        }
        if self.peek() == Some(b'0' as u16) {
            self.i += 1;
            if digit(self.peek()) {
                return Err(format!("Unexpected number {}", self.pos_suffix(self.i)));
            }
        } else {
            while digit(self.peek()) {
                self.i += 1;
            }
        }
        if self.peek() == Some(b'.' as u16) {
            self.i += 1;
            if !digit(self.peek()) {
                return Err(format!(
                    "Unterminated fractional number {}",
                    self.pos_suffix(self.i)
                ));
            }
            while digit(self.peek()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(x) if x == b'e' as u16 || x == b'E' as u16) {
            self.i += 1;
            if matches!(self.peek(), Some(x) if x == b'+' as u16 || x == b'-' as u16) {
                self.i += 1;
            }
            if !digit(self.peek()) {
                return Err(format!(
                    "Exponent part is missing a number {}",
                    self.pos_suffix(self.i)
                ));
            }
            while digit(self.peek()) {
                self.i += 1;
            }
        }
        let txt = String::from_utf16_lossy(&self.s[start..self.i]);
        Ok(Value::Num(txt.parse().unwrap_or(f64::NAN)))
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out: Vec<u16> = vec![];
        loop {
            let Some(c) = self.peek() else {
                return Err(format!("Unterminated string {}", self.pos_suffix(self.i)));
            };
            if c == b'"' as u16 {
                self.i += 1;
                return Ok(String::from_utf16_lossy(&out));
            }
            if c < 0x20 {
                return Err(format!(
                    "Bad control character in string literal {}",
                    self.pos_suffix(self.i)
                ));
            }
            if c == b'\\' as u16 {
                self.i += 1;
                let Some(e) = self.peek() else {
                    return Err(format!("Unterminated string {}", self.pos_suffix(self.i)));
                };
                let ch = match e as u8 {
                    b'"' => '"' as u16,
                    b'\\' => '\\' as u16,
                    b'/' => '/' as u16,
                    b'b' => 8,
                    b'f' => 12,
                    b'n' => 10,
                    b'r' => 13,
                    b't' => 9,
                    b'u' => {
                        let h = self
                            .s
                            .get(self.i + 1..self.i + 5)
                            .map(String::from_utf16_lossy)
                            .unwrap_or_default();
                        match u16::from_str_radix(&h, 16) {
                            Ok(v) if h.len() == 4 => {
                                self.i += 4;
                                v
                            }
                            _ => {
                                // Position of the first bad hex digit.
                                let mut j = self.i + 1;
                                while j < self.s.len()
                                    && j < self.i + 5
                                    && (self.s[j] as u8 as char).is_ascii_hexdigit()
                                    && self.s[j] < 128
                                {
                                    j += 1;
                                }
                                if j >= self.s.len() {
                                    return Err(format!(
                                        "Unterminated string {}",
                                        self.pos_suffix(j)
                                    ));
                                }
                                return Err(format!("Bad Unicode escape {}", self.pos_suffix(j)));
                            }
                        }
                    }
                    _ => return Err(format!("Bad escaped character {}", self.pos_suffix(self.i))),
                };
                out.push(ch);
                self.i += 1;
                continue;
            }
            out.push(c);
            self.i += 1;
        }
    }
}

fn internalize(vm: &mut Vm, holder: &Value, key: Key, reviver: &Value) -> JsResult<Value> {
    let val = vm.get(holder, &key)?;
    if let Value::Obj(o) = &val {
        if o.is_array() {
            let n = vm.length_of(&val)?;
            for i in 0..n {
                let k = Vm::index_key(i);
                let nv = internalize(vm, &val, k.clone(), reviver)?;
                if nv.is_undefined() {
                    vm.delete(o, &k)?;
                } else {
                    vm.create_data_property(o, k, nv)?;
                }
            }
        } else {
            let keys = vm.own_enum_keys(o)?;
            for k in keys {
                let kk = Key::Str(k);
                let nv = internalize(vm, &val, kk.clone(), reviver)?;
                if nv.is_undefined() {
                    vm.delete(o, &kk)?;
                } else {
                    vm.create_data_property(o, kk, nv)?;
                }
            }
        }
    }
    vm.call(reviver, holder.clone(), vec![key.to_value(), val])
}

pub fn parse_json(vm: &mut Vm, text: &str) -> JsResult<Value> {
    let u: Vec<u16> = text.encode_utf16().collect();
    let mut p = P {
        s: &u,
        src: text,
        i: 0,
    };
    let r = p.value(vm, 0).and_then(|v| {
        p.ws();
        if p.i < u.len() {
            Err(p.unexpected(Some("Unexpected non-whitespace character after JSON")))
        } else {
            Ok(v)
        }
    });
    match r {
        Ok(v) => Ok(v),
        Err(msg) => {
            let err = vm.syntax_error(&msg);
            // Node shows the JSON text as the "source" of the error.
            if let Ctl::Throw(Value::Obj(o)) = &err {
                let arrow = if msg.starts_with("Unexpected end") {
                    let first = text.split('\n').next().unwrap_or("");
                    format!("<anonymous_script>:1\n{first}\n")
                } else {
                    let pos = p.i.min(u.len());
                    let before = &u[..pos];
                    let line = before.iter().filter(|&&c| c == b'\n' as u16).count() + 1;
                    let line_start = before
                        .iter()
                        .rposition(|&c| c == b'\n' as u16)
                        .map(|x| x + 1)
                        .unwrap_or(0);
                    let line_end = u[line_start..]
                        .iter()
                        .position(|&c| c == b'\n' as u16)
                        .map(|x| x + line_start)
                        .unwrap_or(u.len());
                    let src_line = String::from_utf16_lossy(&u[line_start..line_end]);
                    format!(
                        "<anonymous_script>:{line}\n{src_line}\n{}^\n",
                        " ".repeat(pos - line_start)
                    )
                };
                if let Kind::Error(ed) = &mut o.borrow_mut().kind {
                    ed.arrow = Some(arrow);
                }
            }
            Err(err)
        }
    }
}

fn parse(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let text = vm.to_string(&a.arg(0))?;
    let v = parse_json(vm, &text)?;
    let reviver = a.arg(1);
    if reviver.is_callable() {
        let root = vm.new_object();
        root.set_prop("", v, ALL);
        return internalize(vm, &Value::Obj(root), Key::str(""), &reviver);
    }
    Ok(v)
}

pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            '\u{fffd}' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Ser {
    replacer: Option<Value>,
    allow: Option<Vec<JsStr>>,
    gap: String,
    stack: Vec<(Obj, String)>,
}

fn circular_error(vm: &mut Vm, ser: &Ser, o: &Obj, key: &str) -> Ctl {
    let start = ser.stack.iter().position(|(x, _)| x.ptr_eq(o)).unwrap_or(0);
    let ctor = |vm: &Vm, x: &Obj| vm.constructor_name(x).unwrap_or_else(|| "Object".into());
    let mut msg = format!(
        "Converting circular structure to JSON\n    --> starting at object with constructor '{}'",
        ctor(vm, &ser.stack[start].0)
    );
    for (x, k) in ser.stack.iter().skip(start + 1) {
        msg.push_str(&format!(
            "\n    |     {} -> object with constructor '{}'",
            key_desc(k),
            ctor(vm, x)
        ));
    }
    msg.push_str(&format!("\n    --- {} closes the circle", key_desc(key)));
    vm.type_error(msg)
}

fn key_desc(k: &str) -> String {
    if !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()) {
        format!("index {k}")
    } else {
        format!("property '{k}'")
    }
}

fn ser_value(
    vm: &mut Vm,
    ser: &mut Ser,
    holder: &Value,
    key: &str,
    indent: &str,
) -> JsResult<Option<String>> {
    let mut v = vm.get(holder, &Key::str(key))?;
    if matches!(v, Value::Obj(_) | Value::BigInt(_)) {
        let tj = vm.get_str(&v, "toJSON")?;
        if tj.is_callable() {
            v = vm.call(&tj, v.clone(), vec![Value::str(key)])?;
        }
    }
    if let Some(r) = ser.replacer.clone() {
        v = vm.call(&r, holder.clone(), vec![Value::str(key), v])?;
    }
    // Unwrap boxed primitives.
    if let Value::Obj(o) = &v {
        let prim = match &o.borrow().kind {
            Kind::Number(_) => 1,
            Kind::String(_) => 2,
            Kind::Boolean(b) => {
                if *b {
                    3
                } else {
                    4
                }
            }
            Kind::BigInt(b) => {
                let _ = b;
                5
            }
            _ => 0,
        };
        match prim {
            1 => v = Value::Num(vm.to_number(&v)?),
            2 => v = Value::Str(vm.to_string(&v)?),
            3 => v = Value::Bool(true),
            4 => v = Value::Bool(false),
            5 => return Err(vm.type_error("Do not know how to serialize a BigInt")),
            _ => {}
        }
    }
    Ok(match &v {
        Value::Null => Some("null".into()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Str(s) => Some(quote(s)),
        Value::Num(n) => Some(if n.is_finite() {
            number_to_string(*n)
        } else {
            "null".into()
        }),
        Value::BigInt(_) => return Err(vm.type_error("Do not know how to serialize a BigInt")),
        Value::Obj(o) if !o.is_callable() => {
            if ser.stack.iter().any(|(x, _)| x.ptr_eq(o)) {
                return Err(circular_error(vm, ser, o, key));
            }
            if ser.stack.len() > 5000 {
                return Err(vm.range_error("Maximum call stack size exceeded"));
            }
            ser.stack.push((o.clone(), key.to_string()));
            let inner = format!("{indent}{}", ser.gap);
            let r = if o.is_array_or_proxy() {
                let n = vm.length_of(&v)?;
                let mut parts = vec![];
                for i in 0..n {
                    let p = ser_value(vm, ser, &v, &i.to_string(), &inner)?;
                    parts.push(p.unwrap_or_else(|| "null".into()));
                }
                if parts.is_empty() {
                    "[]".to_string()
                } else if ser.gap.is_empty() {
                    format!("[{}]", parts.join(","))
                } else {
                    format!(
                        "[\n{inner}{}\n{indent}]",
                        parts.join(&format!(",\n{inner}"))
                    )
                }
            } else {
                let keys = match &ser.allow {
                    Some(k) => k.clone(),
                    None => vm.own_enum_keys(o)?,
                };
                let mut parts = vec![];
                for k in keys {
                    if let Some(p) = ser_value(vm, ser, &v, &k, &inner)? {
                        let sep = if ser.gap.is_empty() { ":" } else { ": " };
                        parts.push(format!("{}{sep}{p}", quote(&k)));
                    }
                }
                if parts.is_empty() {
                    "{}".to_string()
                } else if ser.gap.is_empty() {
                    format!("{{{}}}", parts.join(","))
                } else {
                    format!(
                        "{{\n{inner}{}\n{indent}}}",
                        parts.join(&format!(",\n{inner}"))
                    )
                }
            };
            ser.stack.pop();
            Some(r)
        }
        _ => None,
    })
}

pub fn stringify_value(vm: &mut Vm, v: Value, replacer: Value, space: Value) -> JsResult<Value> {
    let mut ser = Ser {
        replacer: None,
        allow: None,
        gap: String::new(),
        stack: vec![],
    };
    if replacer.is_callable() {
        ser.replacer = Some(replacer);
    } else if let Value::Obj(r) = &replacer {
        if r.is_array() {
            let items = vm.iterable_to_vec(&replacer)?;
            let mut keys: Vec<JsStr> = vec![];
            for it in items {
                let k = match &it {
                    Value::Str(s) => Some(s.clone()),
                    Value::Num(_) => Some(vm.to_string(&it)?),
                    Value::Obj(o)
                        if matches!(o.borrow().kind, Kind::String(_) | Kind::Number(_)) =>
                    {
                        Some(vm.to_string(&it)?)
                    }
                    _ => None,
                };
                if let Some(k) = k {
                    if !keys.contains(&k) {
                        keys.push(k);
                    }
                }
            }
            ser.allow = Some(keys);
        }
    }
    let space = match &space {
        Value::Obj(o) if matches!(o.borrow().kind, Kind::Number(_)) => {
            Value::Num(vm.to_number(&space)?)
        }
        Value::Obj(o) if matches!(o.borrow().kind, Kind::String(_)) => {
            Value::Str(vm.to_string(&space)?)
        }
        s => s.clone(),
    };
    ser.gap = match &space {
        Value::Num(n) => " ".repeat(n.clamp(0.0, 10.0) as usize),
        Value::Str(s) => s.chars().take(10).collect(),
        _ => String::new(),
    };
    let root = vm.new_object();
    root.set_prop("", v, ALL);
    match ser_value(vm, &mut ser, &Value::Obj(root), "", "")? {
        Some(s) => Ok(Value::string(s)),
        None => Ok(Value::Undefined),
    }
}

fn stringify(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    stringify_value(vm, a.arg(0), a.arg(1), a.arg(2))
}

pub fn install(vm: &mut Vm) {
    let j = vm.new_object();
    let tag = vm.syms.to_string_tag.clone();
    j.set_sym(&tag, Value::str("JSON"), CONFIGURABLE);
    vm.method(&j, "parse", 2, parse);
    vm.method(&j, "stringify", 3, stringify);
    vm.set_global("JSON", Value::Obj(j));
}
