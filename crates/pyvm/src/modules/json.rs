//! The `json` module: CPython-compatible encoder and decoder.
use super::{new_module, set_fn, set_val, sys::exec_snippet};
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

struct Enc {
    indent: Option<String>,
    item_sep: String,
    key_sep: String,
    sort_keys: bool,
    ensure_ascii: bool,
    skipkeys: bool,
    allow_nan: bool,
    default: Value,
    stack: Vec<usize>,
}

fn encode_str(s: &str, ascii: bool, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ascii && (c as u32) > 0x7e => {
                let u = c as u32;
                if u > 0xffff {
                    let v = u - 0x10000;
                    out.push_str(&format!(
                        "\\u{:04x}\\u{:04x}",
                        0xd800 + (v >> 10),
                        0xdc00 + (v & 0x3ff)
                    ));
                } else {
                    out.push_str(&format!("\\u{u:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn float_str(f: f64, allow_nan: bool) -> PyResult<String> {
    if f.is_nan() || f.is_infinite() {
        if !allow_nan {
            return Err(value_err(format!(
                "Out of range float values are not JSON compliant: {}",
                crate::format::float_repr(f)
            )));
        }
        return Ok(if f.is_nan() {
            "NaN".into()
        } else if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        });
    }
    Ok(crate::format::float_repr(f))
}

impl Enc {
    fn key(&self, vm: &mut Vm, k: &Value) -> PyResult<Option<String>> {
        Ok(Some(match vm.base_value(k) {
            Value::Str(s) => s.s.clone(),
            Value::Int(i) => i.to_string(),
            Value::Big(b) => b.to_str_radix(10),
            Value::Float(f) => float_str(f, self.allow_nan)?,
            Value::Bool(b) => if b { "true" } else { "false" }.into(),
            Value::None => "null".into(),
            other => {
                if self.skipkeys {
                    return Ok(None);
                }
                return Err(type_err(format!(
                    "keys must be str, int, float, bool or None, not {}",
                    vm.type_name(&other)
                )));
            }
        }))
    }
    fn enc(&mut self, vm: &mut Vm, v: &Value, depth: usize, out: &mut String) -> PyResult<()> {
        if depth > 500 {
            return Err(err(
                "RecursionError",
                "maximum recursion depth exceeded while encoding a JSON object",
            ));
        }
        match v {
            Value::None => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Big(b) => out.push_str(&b.to_str_radix(10)),
            Value::Float(f) => out.push_str(&float_str(*f, self.allow_nan)?),
            Value::Str(s) => encode_str(&s.s, self.ensure_ascii, out),
            Value::List(_) | Value::Tuple(_) => {
                let items = vm.iterate(v)?;
                self.enter(v)?;
                if items.is_empty() {
                    out.push_str("[]");
                } else {
                    out.push('[');
                    for (i, it) in items.iter().enumerate() {
                        if i > 0 {
                            out.push_str(&self.item_sep);
                        }
                        self.newline(out, depth + 1);
                        self.enc(vm, it, depth + 1, out)?;
                    }
                    self.newline(out, depth);
                    out.push(']');
                }
                self.stack.pop();
            }
            Value::Dict(d) => {
                let mut items = d.borrow().items();
                self.enter(v)?;
                if items.is_empty() {
                    out.push_str("{}");
                } else {
                    if self.sort_keys {
                        let keys: Vec<Value> = items.iter().map(|(k, _)| k.clone()).collect();
                        let sorted = crate::bfuncs::sort_values(vm, keys, None, false)?;
                        let mut reordered = vec![];
                        for k in sorted {
                            let pos = items.iter().position(|(kk, _)| kk.is(&k)).unwrap();
                            reordered.push(items.remove(pos));
                        }
                        items = reordered;
                    }
                    out.push('{');
                    let mut first = true;
                    for (k, val) in items.iter() {
                        let Some(ks) = self.key(vm, k)? else {
                            continue;
                        };
                        if !first {
                            out.push_str(&self.item_sep);
                        }
                        first = false;
                        self.newline(out, depth + 1);
                        encode_str(&ks, self.ensure_ascii, out);
                        out.push_str(&self.key_sep);
                        self.enc(vm, val, depth + 1, out)?;
                    }
                    self.newline(out, depth);
                    out.push('}');
                }
                self.stack.pop();
            }
            Value::Instance(i) => {
                let base = match &*i.native.borrow() {
                    NativeData::Base(b) => Some(b.clone()),
                    _ => None,
                };
                match base {
                    Some(
                        b @ (Value::Dict(_)
                        | Value::List(_)
                        | Value::Tuple(_)
                        | Value::Str(_)
                        | Value::Int(_)
                        | Value::Float(_)),
                    ) => return self.enc(vm, &b, depth, out),
                    _ => return self.fallback(vm, v, depth, out),
                }
            }
            _ => return self.fallback(vm, v, depth, out),
        }
        Ok(())
    }
    fn fallback(&mut self, vm: &mut Vm, v: &Value, depth: usize, out: &mut String) -> PyResult<()> {
        if self.default.is_none() {
            return Err(type_err(format!(
                "Object of type {} is not JSON serializable",
                vm.type_name(v)
            )));
        }
        self.enter(v)?;
        let d = self.default.clone();
        let r = vm.call(&d, vec![v.clone()])?;
        let res = self.enc(vm, &r, depth, out);
        self.stack.pop();
        res
    }
    fn enter(&mut self, v: &Value) -> PyResult<()> {
        let id = v.id();
        if self.stack.contains(&id) {
            return Err(value_err("Circular reference detected"));
        }
        self.stack.push(id);
        Ok(())
    }
    fn newline(&self, out: &mut String, depth: usize) {
        if let Some(ind) = &self.indent {
            out.push('\n');
            for _ in 0..depth {
                out.push_str(ind);
            }
        }
    }
}

fn dumps(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(
        &mut a,
        "dumps",
        &[
            "obj",
            "skipkeys",
            "ensure_ascii",
            "check_circular",
            "allow_nan",
            "cls",
            "indent",
            "separators",
            "default",
            "sort_keys",
        ],
        1,
    )?;
    let flag = |vm: &mut Vm, v: &Option<Value>, d: bool| -> PyResult<bool> {
        match v {
            Some(x) => vm.truthy(x),
            None => Ok(d),
        }
    };
    let indent = match &p[6] {
        None | Some(Value::None) => None,
        Some(Value::Int(n)) => Some(" ".repeat((*n).max(0) as usize)),
        Some(Value::Str(s)) => Some(s.s.clone()),
        Some(other) => Some(vm.str_of(other)?),
    };
    let (item_sep, key_sep) = match &p[7] {
        Some(Value::Tuple(t)) if t.len() == 2 => (vm.str_of(&t[0])?, vm.str_of(&t[1])?),
        Some(Value::List(l)) if l.borrow().len() == 2 => {
            let l = l.borrow().clone();
            (vm.str_of(&l[0])?, vm.str_of(&l[1])?)
        }
        _ => {
            if indent.is_some() {
                (",".into(), ": ".into())
            } else {
                (", ".into(), ": ".into())
            }
        }
    };
    let mut enc = Enc {
        indent,
        item_sep,
        key_sep,
        sort_keys: flag(vm, &p[9], false)?,
        ensure_ascii: flag(vm, &p[2], true)?,
        skipkeys: flag(vm, &p[1], false)?,
        allow_nan: flag(vm, &p[4], true)?,
        default: p[8].clone().unwrap_or(Value::None),
        stack: vec![],
    };
    let mut out = String::new();
    enc.enc(vm, p[0].as_ref().unwrap(), 0, &mut out)?;
    Ok(Value::string(out))
}

struct Dec<'a> {
    s: &'a [char],
    pos: usize,
    object_hook: Value,
    object_pairs_hook: Value,
    parse_float: Value,
    parse_int: Value,
}

fn line_col(s: &[char], pos: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for c in &s[..pos.min(s.len())] {
        if *c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

impl<'a> Dec<'a> {
    fn fail(&self, msg: &str, pos: usize) -> Box<PyErr> {
        let (line, col) = line_col(self.s, pos);
        let doc: String = self.s.iter().collect();
        err_args(
            "json.JSONDecodeError",
            vec![
                Value::string(format!("{msg}: line {line} column {col} (char {pos})")),
                Value::string(doc),
                Value::Int(pos as i64),
                Value::str(msg),
            ],
        )
    }
    fn ws(&mut self) {
        while self.pos < self.s.len() && matches!(self.s[self.pos], ' ' | '\t' | '\n' | '\r') {
            self.pos += 1;
        }
    }
    fn value(&mut self, vm: &mut Vm, depth: usize) -> PyResult<Value> {
        if depth > 900 {
            return Err(err(
                "RecursionError",
                "maximum recursion depth exceeded while decoding a JSON document",
            ));
        }
        self.ws();
        let Some(&c) = self.s.get(self.pos) else {
            return Err(self.fail("Expecting value", self.pos));
        };
        match c {
            '{' => self.object(vm, depth),
            '[' => self.array(vm, depth),
            '"' => {
                self.pos += 1;
                Ok(Value::string(self.string()?))
            }
            't' if self.lit("true") => Ok(Value::Bool(true)),
            'f' if self.lit("false") => Ok(Value::Bool(false)),
            'n' if self.lit("null") => Ok(Value::None),
            'N' if self.lit("NaN") => Ok(Value::Float(f64::NAN)),
            'I' if self.lit("Infinity") => Ok(Value::Float(f64::INFINITY)),
            '-' if self.s[self.pos..].starts_with(&['-', 'I']) && {
                self.pos += 1;
                self.lit("Infinity") || {
                    self.pos -= 1;
                    false
                }
            } =>
            {
                Ok(Value::Float(f64::NEG_INFINITY))
            }
            '-' | '0'..='9' => self.number(vm),
            _ => Err(self.fail("Expecting value", self.pos)),
        }
    }
    fn lit(&mut self, w: &str) -> bool {
        let n = w.chars().count();
        if self.pos + n <= self.s.len()
            && self.s[self.pos..self.pos + n].iter().copied().eq(w.chars())
        {
            self.pos += n;
            true
        } else {
            false
        }
    }
    fn number(&mut self, vm: &mut Vm) -> PyResult<Value> {
        let start = self.pos;
        if self.s[self.pos] == '-' {
            self.pos += 1;
        }
        if self.pos >= self.s.len() || !self.s[self.pos].is_ascii_digit() {
            return Err(self.fail("Expecting value", start));
        }
        if self.s[self.pos] == '0' {
            self.pos += 1;
        } else {
            while self.pos < self.s.len() && self.s[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let mut is_float = false;
        if self.pos + 1 < self.s.len()
            && self.s[self.pos] == '.'
            && self.s[self.pos + 1].is_ascii_digit()
        {
            is_float = true;
            self.pos += 1;
            while self.pos < self.s.len() && self.s[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.s.len() && (self.s[self.pos] == 'e' || self.s[self.pos] == 'E') {
            let save = self.pos;
            self.pos += 1;
            if self.pos < self.s.len() && (self.s[self.pos] == '+' || self.s[self.pos] == '-') {
                self.pos += 1;
            }
            if self.pos < self.s.len() && self.s[self.pos].is_ascii_digit() {
                is_float = true;
                while self.pos < self.s.len() && self.s[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            } else {
                self.pos = save;
            }
        }
        let text: String = self.s[start..self.pos].iter().collect();
        if is_float {
            if !self.parse_float.is_none() {
                let pf = self.parse_float.clone();
                return vm.call(&pf, vec![Value::string(text)]);
            }
            Ok(Value::Float(text.parse().unwrap_or(0.0)))
        } else {
            if !self.parse_int.is_none() {
                let pi = self.parse_int.clone();
                return vm.call(&pi, vec![Value::string(text)]);
            }
            Ok(crate::bfuncs::parse_int_str(vm, &text, 10).unwrap_or(Value::Int(0)))
        }
    }
    fn string(&mut self) -> PyResult<String> {
        let start = self.pos - 1;
        let mut out = String::new();
        loop {
            let Some(&c) = self.s.get(self.pos) else {
                return Err(self.fail("Unterminated string starting at", start));
            };
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(&e) = self.s.get(self.pos) else {
                        return Err(self.fail("Unterminated string starting at", start));
                    };
                    self.pos += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\x08'),
                        'f' => out.push('\x0c'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let cp = self.hex4()?;
                            if (0xd800..0xdc00).contains(&cp)
                                && self.s.get(self.pos) == Some(&'\\')
                                && self.s.get(self.pos + 1) == Some(&'u')
                            {
                                let save = self.pos;
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if (0xdc00..0xe000).contains(&lo) {
                                    let c = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
                                    out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                                    continue;
                                }
                                self.pos = save;
                            }
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err(self.fail("Invalid \\escape", self.pos - 2)),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(self.fail("Invalid control character at", self.pos - 1))
                }
                c => out.push(c),
            }
        }
    }
    fn hex4(&mut self) -> PyResult<u32> {
        if self.pos + 4 > self.s.len() {
            return Err(self.fail("Invalid \\uXXXX escape", self.pos - 1));
        }
        let h: String = self.s[self.pos..self.pos + 4].iter().collect();
        let v = u32::from_str_radix(&h, 16)
            .map_err(|_| self.fail("Invalid \\uXXXX escape", self.pos - 1))?;
        self.pos += 4;
        Ok(v)
    }
    fn array(&mut self, vm: &mut Vm, depth: usize) -> PyResult<Value> {
        self.pos += 1;
        let mut items = vec![];
        self.ws();
        if self.s.get(self.pos) == Some(&']') {
            self.pos += 1;
            return Ok(Value::list(items));
        }
        loop {
            items.push(self.value(vm, depth + 1)?);
            self.ws();
            match self.s.get(self.pos) {
                Some(',') => {
                    self.pos += 1;
                    self.ws();
                    if self.s.get(self.pos) == Some(&']') {
                        return Err(self.fail("Expecting value", self.pos));
                    }
                }
                Some(']') => {
                    self.pos += 1;
                    return Ok(Value::list(items));
                }
                _ => return Err(self.fail("Expecting ',' delimiter", self.pos)),
            }
        }
    }
    fn object(&mut self, vm: &mut Vm, depth: usize) -> PyResult<Value> {
        self.pos += 1;
        let mut pairs: Vec<(Value, Value)> = vec![];
        self.ws();
        if self.s.get(self.pos) == Some(&'}') {
            self.pos += 1;
            return self.finish_object(vm, pairs);
        }
        loop {
            self.ws();
            if self.s.get(self.pos) != Some(&'"') {
                return Err(self.fail(
                    "Expecting property name enclosed in double quotes",
                    self.pos,
                ));
            }
            self.pos += 1;
            let k = self.string()?;
            self.ws();
            if self.s.get(self.pos) != Some(&':') {
                return Err(self.fail("Expecting ':' delimiter", self.pos));
            }
            self.pos += 1;
            let v = self.value(vm, depth + 1)?;
            pairs.push((Value::string(k), v));
            self.ws();
            match self.s.get(self.pos) {
                Some(',') => {
                    self.pos += 1;
                    self.ws();
                    if self.s.get(self.pos) == Some(&'}') {
                        return Err(self.fail(
                            "Expecting property name enclosed in double quotes",
                            self.pos,
                        ));
                    }
                }
                Some('}') => {
                    self.pos += 1;
                    return self.finish_object(vm, pairs);
                }
                _ => return Err(self.fail("Expecting ',' delimiter", self.pos)),
            }
        }
    }
    fn finish_object(&mut self, vm: &mut Vm, pairs: Vec<(Value, Value)>) -> PyResult<Value> {
        if !self.object_pairs_hook.is_none() {
            let h = self.object_pairs_hook.clone();
            let list = Value::list(
                pairs
                    .into_iter()
                    .map(|(k, v)| Value::tuple(vec![k, v]))
                    .collect(),
            );
            return vm.call(&h, vec![list]);
        }
        let d = new_ref(Dict::new());
        for (k, v) in pairs {
            vm.dict_set(&d, k, v)?;
        }
        let d = Value::Dict(d);
        if !self.object_hook.is_none() {
            let h = self.object_hook.clone();
            return vm.call(&h, vec![d]);
        }
        Ok(d)
    }
}

fn loads(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let object_hook = a.kw("object_hook").unwrap_or(Value::None);
    let object_pairs_hook = a.kw("object_pairs_hook").unwrap_or(Value::None);
    let parse_float = a.kw("parse_float").unwrap_or(Value::None);
    let parse_int = a.kw("parse_int").unwrap_or(Value::None);
    let _ = a.kw("strict");
    let _ = a.kw("cls");
    let s = match a.args.first() {
        Some(Value::Str(s)) => s.s.clone(),
        Some(Value::Bytes(b)) => String::from_utf8_lossy(b).into_owned(),
        Some(Value::ByteArray(b)) => String::from_utf8_lossy(&b.borrow()).into_owned(),
        Some(other) => {
            return Err(type_err(format!(
                "the JSON object must be str, bytes or bytearray, not {}",
                vm.type_name(other)
            )))
        }
        None => {
            return Err(type_err(
                "loads() missing 1 required positional argument: 's'",
            ))
        }
    };
    let chars: Vec<char> = s.chars().collect();
    let mut d = Dec {
        s: &chars,
        pos: 0,
        object_hook,
        object_pairs_hook,
        parse_float,
        parse_int,
    };
    if chars.first() == Some(&'\u{feff}') {
        return Err(d.fail("Unexpected UTF-8 BOM (decode using utf-8-sig)", 0));
    }
    let v = d.value(vm, 0)?;
    d.ws();
    if d.pos < chars.len() {
        return Err(d.fail("Extra data", d.pos));
    }
    Ok(v)
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("json");
    set_fn(&m, "dumps", dumps);
    set_fn(&m, "loads", loads);
    // JSONDecodeError: a ValueError subclass with doc/pos/lineno/colno.
    let jde = new_class(
        "JSONDecodeError",
        vec![vm.t.exc("ValueError")],
        Kind::Exception,
        false,
    );
    jde.dict
        .borrow_mut()
        .set_str("__module__", Value::str("json.decoder"));
    vm.t.exceptions.insert("json.JSONDecodeError", jde.clone());
    set_val(&m, "JSONDecodeError", Value::Class(jde));
    exec_snippet(
        vm,
        &m,
        r#"
def _jde_init(self, msg, doc=None, pos=None, raw=None):
    ValueError.__init__(self, msg)
    self.doc = doc
    self.pos = pos
    self.msg = raw if raw is not None else msg
    if doc is not None and pos is not None:
        self.lineno = doc.count('\n', 0, pos) + 1
        self.colno = pos - doc.rfind('\n', 0, pos)
JSONDecodeError.__init__ = _jde_init
del _jde_init
def dump(obj, fp, **kw):
    fp.write(dumps(obj, **kw))
def load(fp, **kw):
    return loads(fp.read(), **kw)
class JSONEncoder:
    def __init__(self, *, skipkeys=False, ensure_ascii=True, check_circular=True, allow_nan=True, sort_keys=False, indent=None, separators=None, default=None):
        self.skipkeys = skipkeys
        self.ensure_ascii = ensure_ascii
        self.allow_nan = allow_nan
        self.sort_keys = sort_keys
        self.indent = indent
        self.separators = separators
        if default is not None:
            self.default = default
    def default(self, o):
        raise TypeError(f'Object of type {o.__class__.__name__} is not JSON serializable')
    def encode(self, o):
        return dumps(o, skipkeys=self.skipkeys, ensure_ascii=self.ensure_ascii, allow_nan=self.allow_nan, sort_keys=self.sort_keys, indent=self.indent, separators=self.separators, default=self.default)
class JSONDecoder:
    def __init__(self, *, object_hook=None, parse_float=None, parse_int=None, parse_constant=None, strict=True, object_pairs_hook=None):
        self.object_hook = object_hook
        self.object_pairs_hook = object_pairs_hook
    def decode(self, s):
        return loads(s, object_hook=self.object_hook, object_pairs_hook=self.object_pairs_hook)
_dumps = dumps
def dumps(obj, *, cls=None, **kw):
    if cls is not None:
        return cls(**kw).encode(obj)
    return _dumps(obj, **kw)
_loads = loads
def loads(s, *, cls=None, **kw):
    if cls is not None:
        return cls(**kw).decode(s)
    return _loads(s, **kw)
"#,
    );
    let _ = Rc::new(0);
    Value::Module(m)
}
