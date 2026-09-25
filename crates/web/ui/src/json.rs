//! `JSON.parse` and `JSON.stringify` over runtime values, keeping object keys in
//! insertion order as JavaScript does.

use std::rc::Rc;

use crate::interp::{obj_get, obj_set, object_keys};
use crate::value::{number_to_string, Value};

pub fn parse(s: &str) -> Result<Value, String> {
    let mut p = P {
        b: s.as_bytes(),
        i: 0,
        s,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("Unexpected token in JSON at position {}", p.i));
    }
    Ok(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
    s: &'a str,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn err<T>(&self) -> Result<T, String> {
        if self.i >= self.b.len() {
            Err("Unexpected end of JSON input".into())
        } else {
            Err(format!("Unexpected token in JSON at position {}", self.i))
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > 512 {
            return Err("JSON nested too deeply".into());
        }
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::object(out));
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return self.err();
                    }
                    let k = self.string()?;
                    self.ws();
                    if self.b.get(self.i) != Some(&b':') {
                        return self.err();
                    }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    obj_set(&mut out, Rc::from(k.as_str()), v);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::object(out));
                        }
                        _ => return self.err(),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::array(out));
                }
                loop {
                    self.ws();
                    out.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::array(out));
                        }
                        _ => return self.err(),
                    }
                }
            }
            Some(b'"') => Ok(Value::str(&self.string()?)),
            Some(b't') if self.s[self.i..].starts_with("true") => {
                self.i += 4;
                Ok(Value::Bool(true))
            }
            Some(b'f') if self.s[self.i..].starts_with("false") => {
                self.i += 5;
                Ok(Value::Bool(false))
            }
            Some(b'n') if self.s[self.i..].starts_with("null") => {
                self.i += 4;
                Ok(Value::Null)
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'
                    )
                {
                    self.i += 1;
                }
                self.s[start..self.i]
                    .parse::<f64>()
                    .map(Value::Num)
                    .map_err(|_| format!("Unexpected number in JSON at position {start}"))
            }
            _ => self.err(),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let Some(&c) = self.b.get(self.i) else {
                return Err("Unterminated string in JSON".into());
            };
            match c {
                b'"' => {
                    self.i += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.i += 1;
                    let e = self.b.get(self.i).copied();
                    self.i += 1;
                    match e {
                        Some(b'"') => out.push('"'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'/') => out.push('/'),
                        Some(b'b') => out.push('\u{8}'),
                        Some(b'f') => out.push('\u{c}'),
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'u') => {
                            let hex = self.s.get(self.i..self.i + 4).ok_or("Bad unicode escape")?;
                            let cp =
                                u16::from_str_radix(hex, 16).map_err(|_| "Bad unicode escape")?;
                            self.i += 4;
                            let mut units = vec![cp];
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with("\\u")
                            {
                                if let Some(lo) = self.s.get(self.i + 2..self.i + 6) {
                                    if let Ok(lo) = u16::from_str_radix(lo, 16) {
                                        if (0xDC00..0xE000).contains(&lo) {
                                            units.push(lo);
                                            self.i += 6;
                                        }
                                    }
                                }
                            }
                            out.push_str(&String::from_utf16_lossy(&units));
                        }
                        _ => return self.err(),
                    }
                }
                _ => {
                    let start = self.i;
                    while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\'
                    {
                        self.i += 1;
                    }
                    out.push_str(&self.s[start..self.i]);
                }
            }
        }
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `JSON.stringify(v, null, indent)`; `None` when the result is `undefined`.
pub fn stringify(v: &Value, indent: &Value) -> Option<String> {
    let gap = match indent {
        Value::Num(n) => " ".repeat((n.max(0.0) as usize).min(10)),
        Value::Str(s) => s.chars().take(10).collect(),
        _ => String::new(),
    };
    let mut out = String::new();
    if write(v, &gap, 0, &mut out) {
        Some(out)
    } else {
        None
    }
}

fn write(v: &Value, gap: &str, depth: usize, out: &mut String) -> bool {
    let nl = |out: &mut String, d: usize| {
        if !gap.is_empty() {
            out.push('\n');
            for _ in 0..d {
                out.push_str(gap);
            }
        }
    };
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => {
            if n.is_finite() {
                out.push_str(&number_to_string(*n))
            } else {
                out.push_str("null")
            }
        }
        Value::Str(s) => quote(s, out),
        Value::Array(a) => {
            let a = a.borrow();
            if a.is_empty() {
                out.push_str("[]");
                return true;
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                nl(out, depth + 1);
                if !write(x, gap, depth + 1, out) {
                    out.push_str("null");
                }
            }
            nl(out, depth);
            out.push(']');
        }
        Value::Object(o) => {
            let o = o.borrow();
            let keys = object_keys(&o);
            let mut first = true;
            out.push('{');
            for k in keys {
                let x = obj_get(&o, &k).unwrap_or_default();
                if matches!(
                    x,
                    Value::Undefined | Value::Func(_) | Value::Setter(..) | Value::Dispatch(..)
                ) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                nl(out, depth + 1);
                quote(&k, out);
                out.push(':');
                if !gap.is_empty() {
                    out.push(' ');
                }
                write(&x, gap, depth + 1, out);
            }
            if !first {
                nl(out, depth);
            }
            out.push('}');
        }
        // `Date.prototype.toJSON`: its ISO string, or `null` when invalid.
        Value::Date(t) => {
            if t.get().is_finite() {
                quote(&cw_jsvm::builtins::date::iso_string(t.get()), out)
            } else {
                out.push_str("null")
            }
        }
        Value::Ref(r) => {
            // A ref is `{ current }`; an `undefined` member is left out.
            let obj = Value::object(vec![(std::rc::Rc::from("current"), r.borrow().clone())]);
            return write(&obj, gap, depth, out);
        }
        Value::Undefined | Value::Func(_) | Value::Setter(..) | Value::Dispatch(..) => {
            return false
        }
        _ => out.push_str("{}"),
    }
    true
}
