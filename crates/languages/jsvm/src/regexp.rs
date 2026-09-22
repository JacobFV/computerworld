//! `RegExp` on top of cw-regex (JavaScript flavour), with UTF-16 indices.

use crate::builtins::str_arg;
use crate::value::*;
use crate::vm::Vm;
use cw_regex::{Flags, Flavor, Regex};
use std::rc::Rc;

/// Capture group spans (start, end) in UTF-16 units; `None` for unmatched.
type Captures = Vec<Option<(usize, usize)>>;

fn flags_of(f: &str) -> Result<Flags, String> {
    let mut fl = Flags::default();
    for (i, c) in f.chars().enumerate() {
        if !"dgimsuyv".contains(c) || f.chars().take(i).any(|g| g == c) {
            return Err(format!(
                "Invalid flags supplied to RegExp constructor '{f}'"
            ));
        }
        match c {
            'i' => fl.ignore_case = true,
            'm' => fl.multiline = true,
            's' => fl.dot_all = true,
            'u' | 'v' => fl.unicode = true,
            _ => {}
        }
    }
    Ok(fl)
}

fn compile(pattern: &str, flags: &str) -> Result<Regex, String> {
    let fl = flags_of(flags)?;
    Regex::new(pattern, Flavor::JavaScript, fl).map_err(|e| {
        let prefix = format!("Invalid regular expression: /{pattern}/: ");
        match e.message.strip_prefix(&prefix) {
            Some(rest) => format!("Invalid regular expression: /{pattern}/{flags}: {rest}"),
            None => e.message,
        }
    })
}

/// Parse-time validation of a regex literal.
pub fn validate(pattern: &str, flags: &str) -> Result<(), String> {
    for (i, c) in flags.chars().enumerate() {
        if !"dgimsuyv".contains(c) || flags.chars().take(i).any(|g| g == c) {
            return Err("Invalid regular expression flags".into());
        }
    }
    compile(pattern, flags).map(|_| ())
}

/// Canonical flags string order: "dgimsuvy".
fn canonical_flags(f: &str) -> String {
    "dgimsuvy".chars().filter(|c| f.contains(*c)).collect()
}

impl<'h> Vm<'h> {
    pub fn regexp_create(
        &mut self,
        pattern: &JsStr,
        flags: &JsStr,
        proto: Option<Obj>,
    ) -> JsResult<Obj> {
        let re = match compile(pattern, flags) {
            Ok(r) => r,
            Err(m) => return Err(self.syntax_error(m)),
        };
        let data = RegExpData {
            source: pattern.clone(),
            flags: JsStr::new(canonical_flags(flags)),
            re: Rc::new(re),
            global: flags.contains('g'),
            sticky: flags.contains('y'),
            unicode: flags.contains('u') || flags.contains('v'),
            has_indices: flags.contains('d'),
        };
        let proto = proto.unwrap_or_else(|| self.intr.regexp_proto.clone());
        let o = self.obj_with(Some(proto), Kind::RegExp(Box::new(data)));
        o.borrow_mut()
            .props
            .insert(Key::str("lastIndex"), Prop::data(Value::Num(0.0), WRITABLE));
        Ok(o)
    }

    fn re_data(&mut self, v: &Value, method: &str) -> JsResult<(Obj, Rc<Regex>, bool, bool, bool)> {
        if let Value::Obj(o) = v {
            if let Kind::RegExp(d) = &o.borrow().kind {
                return Ok((o.clone(), d.re.clone(), d.global, d.sticky, d.has_indices));
            }
        }
        let d = self.describe_for_error(v);
        Err(self.type_error(format!(
            "Method RegExp.prototype.{method} called on incompatible receiver {d}"
        )))
    }

    /// Runs the regex honouring lastIndex / g / y. Returns capture spans in
    /// UTF-16 indices.
    pub fn regexp_exec_raw(&mut self, rv: &Value, s: &JsStr) -> JsResult<Option<Captures>> {
        let (o, re, global, sticky, _) = self.re_data(rv, "exec")?;
        let li = if global || sticky {
            let l = self.get_str(rv, "lastIndex")?;
            self.to_length(&l)?
        } else {
            0
        };
        let len16 = s.len16();
        if li > len16 {
            if global || sticky {
                self.set_str(rv, "lastIndex", Value::Num(0.0))?;
            }
            return Ok(None);
        }
        let chars: Vec<char> = s.chars().collect();
        // UTF-16 offset of each char index.
        let ascii = s.is_ascii();
        let offs: Vec<usize> = if ascii {
            vec![]
        } else {
            let mut v = Vec::with_capacity(chars.len() + 1);
            let mut n = 0;
            for c in &chars {
                v.push(n);
                n += c.len_utf16();
            }
            v.push(n);
            v
        };
        let to16 = |ci: usize| if ascii { ci } else { offs[ci] };
        let start_ci = if ascii {
            li
        } else {
            offs.iter().position(|&x| x >= li).unwrap_or(chars.len())
        };
        let r = match re.exec(&chars, start_ci, sticky, false) {
            Ok(r) => r,
            Err(_) => return Err(self.range_error("Maximum call stack size exceeded")),
        };
        let _ = o;
        match r {
            None => {
                if global || sticky {
                    self.set_str(rv, "lastIndex", Value::Num(0.0))?;
                }
                Ok(None)
            }
            Some(slots) => {
                let spans: Vec<Option<(usize, usize)>> = slots
                    .iter()
                    .map(|x| x.map(|(a, b)| (to16(a), to16(b))))
                    .collect();
                if global || sticky {
                    let end = spans[0].map(|x| x.1).unwrap_or(0);
                    self.set_str(rv, "lastIndex", Value::Num(end as f64))?;
                }
                Ok(Some(spans))
            }
        }
    }

    pub fn regexp_group_names(&mut self, rv: &Value) -> Vec<(String, usize)> {
        if let Value::Obj(o) = rv {
            if let Kind::RegExp(d) = &o.borrow().kind {
                return d.re.group_names().to_vec();
            }
        }
        vec![]
    }

    /// Builds the exec() result array.
    pub fn regexp_result(
        &mut self,
        rv: &Value,
        s: &JsStr,
        spans: &[Option<(usize, usize)>],
    ) -> JsResult<Value> {
        let items: Vec<Value> = spans
            .iter()
            .map(|sp| match sp {
                Some((a, b)) => Value::Str(s.slice16(*a, *b)),
                None => Value::Undefined,
            })
            .collect();
        let arr = self.new_array(items.clone());
        let idx = spans[0].map(|x| x.0).unwrap_or(0);
        arr.set_prop("index", Value::Num(idx as f64), ALL);
        arr.set_prop("input", Value::Str(s.clone()), ALL);
        let names = self.regexp_group_names(rv);
        let groups = if names.is_empty() {
            Value::Undefined
        } else {
            let g = self.obj_with(None, Kind::Ordinary);
            for (n, i) in &names {
                g.set_prop(n, items.get(*i).cloned().unwrap_or(Value::Undefined), ALL);
            }
            Value::Obj(g)
        };
        arr.set_prop("groups", groups, ALL);
        let has_indices = matches!(rv, Value::Obj(o) if matches!(&o.borrow().kind, Kind::RegExp(d) if d.has_indices));
        if has_indices {
            let ind: Vec<Value> = spans
                .iter()
                .map(|sp| match sp {
                    Some((a, b)) => self.arr(vec![Value::Num(*a as f64), Value::Num(*b as f64)]),
                    None => Value::Undefined,
                })
                .collect();
            let ia = self.new_array(ind);
            ia.set_prop("groups", Value::Undefined, ALL);
            arr.set_prop("indices", Value::Obj(ia), ALL);
        }
        Ok(Value::Obj(arr))
    }

    fn advance(s: &JsStr, i: usize, unicode: bool) -> usize {
        if !unicode || s.is_ascii() {
            return i + 1;
        }
        match (s.code_unit(i), s.code_unit(i + 1)) {
            (Some(a), Some(b))
                if (0xd800..0xdc00).contains(&a) && (0xdc00..0xe000).contains(&b) =>
            {
                i + 2
            }
            _ => i + 1,
        }
    }

    fn is_unicode(v: &Value) -> bool {
        matches!(v, Value::Obj(o) if matches!(&o.borrow().kind, Kind::RegExp(d) if d.unicode))
    }
}

fn regexp_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let pat = a.arg(0);
    let flags = a.arg(1);
    let (src, fl) = match &pat {
        Value::Obj(o) if matches!(o.borrow().kind, Kind::RegExp(_)) => {
            let (s, f) = match &o.borrow().kind {
                Kind::RegExp(d) => (d.source.clone(), d.flags.clone()),
                _ => unreachable!(),
            };
            if a.new_target.is_none() && flags.is_undefined() {
                return Ok(pat.clone());
            }
            (
                s,
                if flags.is_undefined() {
                    f
                } else {
                    vm.to_string(&flags)?
                },
            )
        }
        _ => {
            let s = if pat.is_undefined() {
                JsStr::new("(?:)")
            } else {
                vm.to_string(&pat)?
            };
            let f = if flags.is_undefined() {
                JsStr::new("")
            } else {
                vm.to_string(&flags)?
            };
            (s, f)
        }
    };
    let proto = match &a.new_target {
        Some(nt) => {
            let rp = vm.intr.regexp_proto.clone();
            Some(vm.proto_from_ctor(nt, &rp)?)
        }
        None => None,
    };
    let src = if src.is_empty() {
        JsStr::new("(?:)")
    } else {
        src
    };
    Ok(Value::Obj(vm.regexp_create(&src, &fl, proto)?))
}

fn exec(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let this = a.this.clone();
    match vm.regexp_exec_raw(&this, &s)? {
        None => Ok(Value::Null),
        Some(spans) => vm.regexp_result(&this, &s, &spans),
    }
}

/// RegExpExec: honours an overridden `exec`.
fn regexp_exec(vm: &mut Vm, rx: &Value, s: &JsStr) -> JsResult<Option<Value>> {
    let ex = vm.get_str(rx, "exec")?;
    if let Value::Obj(f) = &ex {
        let is_builtin = matches!(f.own_value("name"), Some(Value::Str(ref n)) if n.as_str() == "exec")
            && matches!(&f.borrow().kind, Kind::Function(fd) if matches!(fd.imp, FuncImpl::Native{..}));
        if !is_builtin && ex.is_callable() {
            let r = vm.call(&ex, rx.clone(), vec![Value::Str(s.clone())])?;
            return Ok(if r.is_nullish() { None } else { Some(r) });
        }
    }
    match vm.regexp_exec_raw(rx, s)? {
        None => Ok(None),
        Some(spans) => Ok(Some(vm.regexp_result(rx, s, &spans)?)),
    }
}

fn test(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let this = a.this.clone();
    Ok(Value::Bool(regexp_exec(vm, &this, &s)?.is_some()))
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let src = vm.get_str(&a.this, "source")?;
    let fl = vm.get_str(&a.this, "flags")?;
    let s = vm.to_str(&src)?;
    let f = vm.to_str(&fl)?;
    Ok(Value::string(format!("/{s}/{f}")))
}

fn source(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        if let Kind::RegExp(d) = &o.borrow().kind {
            // Escape "/" and line terminators like V8.
            let mut out = String::new();
            let mut in_class = false;
            let mut esc = false;
            for c in d.source.chars() {
                if esc {
                    out.push(c);
                    esc = false;
                    continue;
                }
                match c {
                    '\\' => {
                        out.push(c);
                        esc = true;
                    }
                    '[' => {
                        in_class = true;
                        out.push(c);
                    }
                    ']' => {
                        in_class = false;
                        out.push(c);
                    }
                    '/' if !in_class => out.push_str("\\/"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    c => out.push(c),
                }
            }
            return Ok(Value::string(out));
        }
        if o.ptr_eq(&vm.intr.regexp_proto) {
            return Ok(Value::str("(?:)"));
        }
    }
    Err(vm.type_error("RegExp.prototype.source getter called on non-RegExp object"))
}

fn flags(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    if let Value::Obj(o) = &a.this {
        if let Kind::RegExp(d) = &o.borrow().kind {
            return Ok(Value::Str(d.flags.clone()));
        }
        if o.ptr_eq(&vm.intr.regexp_proto) {
            return Ok(Value::str(""));
        }
    }
    Err(vm.type_error("RegExp.prototype.flags getter called on non-object"))
}

macro_rules! flag_getter {
    ($name:ident, $c:expr) => {
        fn $name(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
            if let Value::Obj(o) = &a.this {
                if let Kind::RegExp(d) = &o.borrow().kind {
                    return Ok(Value::Bool(d.flags.contains($c)));
                }
                if o.ptr_eq(&vm.intr.regexp_proto) {
                    return Ok(Value::Undefined);
                }
            }
            Err(vm.type_error("RegExp flag getter called on non-RegExp object"))
        }
    };
}
flag_getter!(global, 'g');
flag_getter!(ignore_case, 'i');
flag_getter!(multiline, 'm');
flag_getter!(dot_all, 's');
flag_getter!(unicode, 'u');
flag_getter!(unicode_sets, 'v');
flag_getter!(sticky, 'y');
flag_getter!(has_indices, 'd');

fn sym_match(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let rx = a.this.clone();
    let s = str_arg(vm, a, 0)?;
    let fl = vm.get_str(&rx, "flags")?;
    let fs = vm.to_str(&fl)?;
    if !fs.contains('g') {
        return Ok(regexp_exec(vm, &rx, &s)?.unwrap_or(Value::Null));
    }
    vm.set_str(&rx, "lastIndex", Value::Num(0.0))?;
    let mut out = vec![];
    let unicode = Vm::is_unicode(&rx);
    while let Some(r) = regexp_exec(vm, &rx, &s)? {
        let m = vm.get_index(&r, 0)?;
        let ms = vm.to_string(&m)?;
        if ms.is_empty() {
            let li = vm.get_str(&rx, "lastIndex")?;
            let l = vm.to_length(&li)?;
            vm.set_str(
                &rx,
                "lastIndex",
                Value::Num(Vm::advance(&s, l, unicode) as f64),
            )?;
        }
        out.push(Value::Str(ms));
    }
    if out.is_empty() {
        return Ok(Value::Null);
    }
    Ok(vm.arr(out))
}

fn sym_match_all(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let rx = a.this.clone();
    let s = str_arg(vm, a, 0)?;
    let fl = vm.get_str(&rx, "flags")?;
    let fs = vm.to_str(&fl)?;
    let src = vm.get_str(&rx, "source")?;
    let srcs = vm.to_string(&src)?;
    // Clone the regexp (source escaping undone by using the raw data).
    let raw_src = match &rx {
        Value::Obj(o) => match &o.borrow().kind {
            Kind::RegExp(d) => d.source.clone(),
            _ => srcs,
        },
        _ => srcs,
    };
    let clone = vm.regexp_create(&raw_src, &JsStr::new(fs.clone()), None)?;
    let li = vm.get_str(&rx, "lastIndex")?;
    let l = vm.to_length(&li)?;
    clone.set_prop("lastIndex", Value::Num(l as f64), WRITABLE);
    let it = vm.obj_with(
        Some(vm.intr.regexp_str_iter_proto.clone()),
        Kind::RegExpStringIter {
            re: clone,
            s,
            global: fs.contains('g'),
            unicode: fs.contains('u') || fs.contains('v'),
            done: false,
        },
    );
    Ok(Value::Obj(it))
}

fn str_iter_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.this.clone() else {
        return Err(vm.type_error("next called on incompatible receiver"));
    };
    let (re, s, global, unicode, done) = match &o.borrow().kind {
        Kind::RegExpStringIter {
            re,
            s,
            global,
            unicode,
            done,
        } => (re.clone(), s.clone(), *global, *unicode, *done),
        _ => return Err(vm.type_error("next called on incompatible receiver")),
    };
    if done {
        return Ok(vm.iter_result(Value::Undefined, true));
    }
    let rv = Value::Obj(re);
    let set_done = |o: &Obj| {
        if let Kind::RegExpStringIter { done, .. } = &mut o.borrow_mut().kind {
            *done = true;
        }
    };
    match regexp_exec(vm, &rv, &s)? {
        None => {
            set_done(&o);
            Ok(vm.iter_result(Value::Undefined, true))
        }
        Some(r) => {
            if !global {
                set_done(&o);
                return Ok(vm.iter_result(r, false));
            }
            let m = vm.get_index(&r, 0)?;
            let ms = vm.to_string(&m)?;
            if ms.is_empty() {
                let li = vm.get_str(&rv, "lastIndex")?;
                let l = vm.to_length(&li)?;
                vm.set_str(
                    &rv,
                    "lastIndex",
                    Value::Num(Vm::advance(&s, l, unicode) as f64),
                )?;
            }
            Ok(vm.iter_result(r, false))
        }
    }
}

fn sym_search(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let rx = a.this.clone();
    let s = str_arg(vm, a, 0)?;
    let prev = vm.get_str(&rx, "lastIndex")?;
    vm.set_str(&rx, "lastIndex", Value::Num(0.0))?;
    let r = regexp_exec(vm, &rx, &s)?;
    vm.set_str(&rx, "lastIndex", prev)?;
    match r {
        None => Ok(Value::Num(-1.0)),
        Some(r) => vm.get_str(&r, "index"),
    }
}

fn sym_replace(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let rx = a.this.clone();
    let s = str_arg(vm, a, 0)?;
    let rep = a.arg(1);
    let func = rep.is_callable();
    let tpl = if func {
        JsStr::new("")
    } else {
        vm.to_string(&rep)?
    };
    let fl = vm.get_str(&rx, "flags")?;
    let fs = vm.to_str(&fl)?;
    let global = fs.contains('g');
    let unicode = fs.contains('u') || fs.contains('v');
    if global {
        vm.set_str(&rx, "lastIndex", Value::Num(0.0))?;
    }
    let mut results = vec![];
    while let Some(r) = regexp_exec(vm, &rx, &s)? {
        results.push(r.clone());
        if !global {
            break;
        }
        let m = vm.get_index(&r, 0)?;
        let ms = vm.to_string(&m)?;
        if ms.is_empty() {
            let li = vm.get_str(&rx, "lastIndex")?;
            let l = vm.to_length(&li)?;
            vm.set_str(
                &rx,
                "lastIndex",
                Value::Num(Vm::advance(&s, l, unicode) as f64),
            )?;
        }
    }
    let names = vm.regexp_group_names(&rx);
    let mut out = String::new();
    let mut next_pos = 0usize;
    for r in results {
        let n = vm.length_of(&r)?;
        let m = vm.get_index(&r, 0)?;
        let matched = vm.to_string(&m)?;
        let pos_v = vm.get_str(&r, "index")?;
        let pos = (vm.to_integer(&pos_v)?.max(0.0) as usize).min(s.len16());
        let mut caps: Vec<Value> = vec![];
        for i in 1..n {
            let c = vm.get_index(&r, i)?;
            caps.push(if c.is_undefined() {
                c
            } else {
                Value::Str(vm.to_string(&c)?)
            });
        }
        let groups = vm.get_str(&r, "groups")?;
        let replacement = if func {
            let mut args = vec![Value::Str(matched.clone())];
            args.extend(caps.iter().cloned());
            args.push(Value::Num(pos as f64));
            args.push(Value::Str(s.clone()));
            if !groups.is_undefined() {
                args.push(groups.clone());
            }
            let v = vm.call(&rep, Value::Undefined, args)?;
            vm.to_str(&v)?
        } else {
            let mut gs: Vec<Option<String>> = vec![Some(matched.to_string())];
            gs.extend(caps.iter().map(|c| match c {
                Value::Str(x) => Some(x.to_string()),
                _ => None,
            }));
            let before = s.slice16(0, pos);
            let after = s.slice16((pos + matched.len16()).min(s.len16()), s.len16());
            cw_regex::expand_js_replacement(&tpl, &matched, &before, &after, &gs, &names)
        };
        if pos >= next_pos {
            out.push_str(&s.slice16(next_pos, pos));
            out.push_str(&replacement);
            next_pos = pos + matched.len16();
        }
    }
    if next_pos < s.len16() {
        out.push_str(&s.slice16(next_pos, s.len16()));
    }
    Ok(Value::string(out))
}

fn sym_split(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let rx = a.this.clone();
    let s = str_arg(vm, a, 0)?;
    let limit = if a.arg(1).is_undefined() {
        u32::MAX as usize
    } else {
        vm.to_u32(&a.arg(1))? as usize
    };
    let (o, re, _, _, _) = vm.re_data(&rx, "split")?;
    let _ = o;
    let unicode = Vm::is_unicode(&rx);
    let mut out: Vec<Value> = vec![];
    if limit == 0 {
        return Ok(vm.arr(out));
    }
    let chars: Vec<char> = s.chars().collect();
    let ascii = s.is_ascii();
    let offs: Vec<usize> = {
        let mut v = Vec::with_capacity(chars.len() + 1);
        let mut n = 0;
        for c in &chars {
            v.push(n);
            n += c.len_utf16();
        }
        v.push(n);
        v
    };
    let to16 = |ci: usize| if ascii { ci } else { offs[ci] };
    let size = chars.len();
    if size == 0 {
        let m = re
            .exec(&chars, 0, true, false)
            .map_err(|_| vm.range_error("Maximum call stack size exceeded"))?;
        if m.is_none() {
            out.push(Value::Str(s.clone()));
        }
        return Ok(vm.arr(out));
    }
    let mut p = 0usize;
    let mut q = 0usize;
    while q < size {
        let m = match re.exec(&chars, q, true, false) {
            Ok(m) => m,
            Err(_) => return Err(vm.range_error("Maximum call stack size exceeded")),
        };
        let Some(slots) = m else {
            q += 1;
            continue;
        };
        let e = slots[0].map(|x| x.1).unwrap_or(q).min(size);
        if e == p {
            q += 1;
            let _ = unicode;
            continue;
        }
        out.push(Value::Str(s.slice16(to16(p), to16(q))));
        if out.len() >= limit {
            return Ok(vm.arr(out));
        }
        for g in slots.iter().skip(1) {
            out.push(match g {
                Some((x, y)) => Value::Str(s.slice16(to16(*x), to16(*y))),
                None => Value::Undefined,
            });
            if out.len() >= limit {
                return Ok(vm.arr(out));
            }
        }
        p = e;
        q = p;
    }
    out.push(Value::Str(s.slice16(to16(p), to16(size))));
    Ok(vm.arr(out))
}

fn compile_m(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let src = str_arg(vm, a, 0)?;
    let fl = if a.arg(1).is_undefined() {
        JsStr::new("")
    } else {
        str_arg(vm, a, 1)?
    };
    let n = vm.regexp_create(&src, &fl, None)?;
    if let Value::Obj(o) = &a.this {
        let data = std::mem::replace(&mut n.borrow_mut().kind, Kind::Ordinary);
        o.borrow_mut().kind = data;
        o.set_prop("lastIndex", Value::Num(0.0), WRITABLE);
    }
    Ok(a.this.clone())
}

fn escape(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = str_arg(vm, a, 0)?;
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i == 0 && c.is_ascii_alphanumeric() {
            out.push_str(&format!("\\x{:02x}", c as u32));
        } else if "^$\\.*+?()[]{}|/".contains(c) {
            out.push('\\');
            out.push(c);
        } else if ",-=<>#&!%:;@~'`\"".contains(c) {
            out.push_str(&format!("\\x{:02x}", c as u32));
        } else if c.is_whitespace() {
            match c {
                '\t' => out.push_str("\\t"),
                '\n' => out.push_str("\\n"),
                ' ' => out.push_str("\\x20"),
                _ => out.push_str(&format!("\\u{:04x}", c as u32)),
            }
        } else {
            out.push(c);
        }
    }
    Ok(Value::string(out))
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.regexp_proto.clone();
    let ctor = vm.make_ctor("RegExp", 2, regexp_ctor, &proto);
    vm.method(&ctor, "escape", 1, escape);
    vm.set_global("RegExp", Value::Obj(ctor));
    vm.method(&proto, "exec", 1, exec);
    vm.method(&proto, "test", 1, test);
    vm.method(&proto, "toString", 0, to_string);
    vm.method(&proto, "compile", 2, compile_m);
    vm.getter(&proto, "source", source);
    vm.getter(&proto, "flags", flags);
    vm.getter(&proto, "global", global);
    vm.getter(&proto, "ignoreCase", ignore_case);
    vm.getter(&proto, "multiline", multiline);
    vm.getter(&proto, "dotAll", dot_all);
    vm.getter(&proto, "unicode", unicode);
    vm.getter(&proto, "unicodeSets", unicode_sets);
    vm.getter(&proto, "sticky", sticky);
    vm.getter(&proto, "hasIndices", has_indices);
    let syms = [
        (
            vm.syms.match_.clone(),
            "[Symbol.match]",
            sym_match as NativeFn,
        ),
        (
            vm.syms.match_all.clone(),
            "[Symbol.matchAll]",
            sym_match_all,
        ),
        (vm.syms.replace.clone(), "[Symbol.replace]", sym_replace),
        (vm.syms.search.clone(), "[Symbol.search]", sym_search),
        (vm.syms.split.clone(), "[Symbol.split]", sym_split),
    ];
    for (s, n, f) in syms {
        vm.method_sym(&proto, &s, n, 2, f);
    }
    let rsip = vm.intr.regexp_str_iter_proto.clone();
    vm.method(&rsip, "next", 0, str_iter_next);
    let tag = vm.syms.to_string_tag.clone();
    rsip.set_sym(&tag, Value::str("RegExp String Iterator"), CONFIGURABLE);
}
