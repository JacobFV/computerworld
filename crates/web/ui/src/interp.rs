//! The expression and statement interpreter: JavaScript semantics for the compiled
//! subset, over frames whose variables are slots.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use cw_web::dom::NodeId;
use cw_web::script::{FetchRequest, LogLevel};

use crate::ir::*;
use crate::runtime::*;
use crate::value::*;

#[derive(Debug)]
pub(crate) struct Frame {
    pub locals: Vec<Value>,
    pub closure: Rc<Closure>,
    /// Set for a component's own render frame (not its closures).
    pub inst: Option<u32>,
    /// Occurrences of element expressions this frame evaluated, for cache keys.
    pub occ: BTreeMap<usize, u32>,
    /// Which slots are boxed (live in a shared cell).
    pub boxed: Rc<[bool]>,
}

impl Frame {
    /// A frame for module initialisers and other code outside any function.
    pub fn bare() -> Frame {
        Frame {
            locals: Vec::new(),
            closure: Rc::new(Closure {
                func: 0,
                captures: Vec::new(),
            }),
            inst: None,
            occ: BTreeMap::new(),
            boxed: Rc::from(Vec::new()),
        }
    }

    /// A slot's value, read through its cell when it is boxed.
    pub fn slot_value(&self, c: &Capture) -> Value {
        match self.slot(c) {
            Value::Cell(cell) => cell.borrow().clone(),
            v => v.clone(),
        }
    }

    pub fn slot(&self, c: &Capture) -> &Value {
        match c {
            Capture::Local(n) => &self.locals[*n as usize],
            Capture::Capture(n) => &self.closure.captures[*n as usize],
        }
    }
}

pub(crate) enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

fn throw<T>(v: Value) -> R<T> {
    Err(Throw::Value(v))
}

/// A value for `console.log` and error reports.
pub fn inspect(v: &Value) -> String {
    match v {
        Value::Str(s) => s.to_string(),
        other => inspect_nested(other, 0),
    }
}

fn inspect_nested(v: &Value, depth: usize) -> String {
    match v {
        Value::Str(s) => {
            if depth == 0 {
                s.to_string()
            } else {
                format!("'{s}'")
            }
        }
        Value::Array(a) => {
            if depth > 2 {
                return "[Array]".into();
            }
            let items: Vec<String> = a
                .borrow()
                .iter()
                .map(|x| inspect_nested(x, depth + 1))
                .collect();
            if items.is_empty() {
                "[]".into()
            } else {
                format!("[ {} ]", items.join(", "))
            }
        }
        Value::Object(o) => {
            if depth > 2 {
                return "[Object]".into();
            }
            let items: Vec<String> = o
                .borrow()
                .iter()
                .map(|(k, x)| format!("{k}: {}", inspect_nested(x, depth + 1)))
                .collect();
            if items.is_empty() {
                "{}".into()
            } else {
                format!("{{ {} }}", items.join(", "))
            }
        }
        Value::Func(_) | Value::Setter(..) | Value::Dispatch(..) => "[Function]".into(),
        Value::Undefined | Value::Null | Value::Bool(_) | Value::Num(_) => v.to_js_string(),
        Value::Ref(r) => format!("{{ current: {} }}", inspect_nested(&r.borrow(), depth + 1)),
        _ => v.to_js_string(),
    }
}

/// UTF-16 view of a string (JavaScript indexes strings in code units).
fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn from_utf16(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

/// Relative index resolution for `slice` and friends.
fn rel_index(n: f64, len: usize) -> usize {
    if n.is_nan() {
        return 0;
    }
    let n = n.trunc();
    if n < 0.0 {
        (len as f64 + n).max(0.0) as usize
    } else {
        (n as usize).min(len)
    }
}

fn arg(args: &[Value], i: usize) -> Value {
    args.get(i).cloned().unwrap_or(Value::Undefined)
}

/// A part of an http(s) URL as `location` (WHATWG `URL`) reports it.
pub(crate) fn location_part(url: &str, part: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let (before_hash, hash) = match rest.find('#') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (before_query, search) = match before_hash.find('?') {
        Some(i) => (&before_hash[..i], &before_hash[i..]),
        None => (before_hash, ""),
    };
    let (host, path) = match before_query.find('/') {
        Some(i) => (&before_query[..i], &before_query[i..]),
        None => (before_query, "/"),
    };
    let (hostname, port) = match host.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, p),
        _ => (host, ""),
    };
    let default_port = matches!((scheme, port), ("http", "80") | ("https", "443"));
    let port = if default_port { "" } else { port };
    let host = if port.is_empty() {
        hostname.to_owned()
    } else {
        format!("{hostname}:{port}")
    };
    let search = if search == "?" { "" } else { search };
    let hash = if hash == "#" { "" } else { hash };
    match part {
        "protocol" => format!("{scheme}:"),
        "host" => host,
        "hostname" => hostname.to_owned(),
        "port" => port.to_owned(),
        "pathname" => path.to_owned(),
        "search" => search.to_owned(),
        "hash" => hash.to_owned(),
        "origin" => format!("{scheme}://{host}"),
        _ => format!("{scheme}://{host}{path}{search}{hash}"),
    }
}

/// Whether `v` has a built-in method `name` (as the runtime implements it).
pub(crate) fn has_builtin_method(v: &Value, name: &str) -> bool {
    Runtime::method_kind(v).is_some_and(|k| method_by_name(k, name).is_some())
}

/// A time value from local fields (`new Date(y, m, …)`, `Date.UTC`): years 0–99
/// are 1900–1999, as JavaScript has them.
fn date_fields(args: &[Value]) -> f64 {
    let mut n = [f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
    for (i, v) in args.iter().take(7).enumerate() {
        n[i] = v.to_number();
    }
    let y = if n[0].is_finite() && (0.0..=99.0).contains(&n[0].trunc()) {
        1900.0 + n[0].trunc()
    } else {
        n[0]
    };
    cw_jsvm::builtins::date::make_time(y, n[1], n[2], n[3], n[4], n[5], n[6])
}

/// A `Date` method, as the JS VM's `Date.prototype` does it (the local time zone
/// is UTC); `None` when `m` is no `Date` method.
fn date_method(t: &Rc<std::cell::Cell<f64>>, m: Method, args: &[Value]) -> R<Option<Value>> {
    use cw_jsvm::builtins::date as d;
    use Method as M;
    let time = t.get();
    let field = |f: fn(&d::Parts) -> i64| {
        if time.is_nan() {
            Value::Num(f64::NAN)
        } else {
            Value::Num(f(&d::parts(time)) as f64)
        }
    };
    // Setters: the field index they start at and how many arguments they read.
    let set = |first: usize, max: usize| -> f64 {
        let base = if time.is_nan() && first == 0 {
            0.0
        } else {
            time
        };
        if base.is_nan() {
            return f64::NAN;
        }
        let p = d::parts(base);
        let mut f = [
            p.year as f64,
            p.month as f64,
            p.day as f64,
            p.hour as f64,
            p.minute as f64,
            p.second as f64,
            p.ms as f64,
        ];
        for i in 0..max.min(args.len().max(1)) {
            f[first + i] = arg(args, i).to_number();
        }
        d::make_time(f[0], f[1], f[2], f[3], f[4], f[5], f[6])
    };
    let invalid = || Value::str("Invalid Date");
    Ok(Some(match m {
        M::DateGetFullYear => field(|p| p.year),
        M::DateGetMonth => field(|p| p.month),
        M::DateGetDate => field(|p| p.day),
        M::DateGetDay => field(|p| p.weekday),
        M::DateGetHours => field(|p| p.hour),
        M::DateGetMinutes => field(|p| p.minute),
        M::DateGetSeconds => field(|p| p.second),
        M::DateGetMilliseconds => field(|p| p.ms),
        M::DateGetYear => field(|p| p.year - 1900),
        M::DateGetTime => Value::Num(time),
        M::DateGetTimezoneOffset => Value::Num(if time.is_nan() { f64::NAN } else { 0.0 }),
        M::DateSetFullYear
        | M::DateSetMonth
        | M::DateSetDate
        | M::DateSetHours
        | M::DateSetMinutes
        | M::DateSetSeconds
        | M::DateSetMilliseconds
        | M::DateSetTime => {
            let nt = match m {
                M::DateSetFullYear => set(0, 3),
                M::DateSetMonth => set(1, 2),
                M::DateSetDate => set(2, 1),
                M::DateSetHours => set(3, 4),
                M::DateSetMinutes => set(4, 3),
                M::DateSetSeconds => set(5, 2),
                M::DateSetMilliseconds => set(6, 1),
                _ => d::time_clip(arg(args, 0).to_number()),
            };
            t.set(nt);
            Value::Num(nt)
        }
        M::DateToISOString => {
            if time.is_nan() {
                return js_error("RangeError", "Invalid time value");
            }
            Value::str(&d::iso_string(time))
        }
        M::DateToJSON => {
            if time.is_finite() {
                Value::str(&d::iso_string(time))
            } else {
                Value::Null
            }
        }
        M::DateToString | M::ToString => Value::str(&d::date_to_string(time)),
        M::DateToDateString | M::DateToTimeString | M::DateToUTCString => {
            if time.is_nan() {
                invalid()
            } else {
                let p = d::parts(time);
                let yr = if p.year < 0 {
                    format!("-{:06}", -p.year)
                } else {
                    format!("{:04}", p.year)
                };
                Value::str(&match m {
                    M::DateToDateString => format!(
                        "{} {} {:02} {yr}",
                        d::DAYS[p.weekday as usize],
                        d::MONTHS[p.month as usize],
                        p.day
                    ),
                    M::DateToTimeString => format!(
                        "{:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
                        p.hour, p.minute, p.second
                    ),
                    _ => format!(
                        "{}, {:02} {} {yr} {:02}:{:02}:{:02} GMT",
                        d::DAYS[p.weekday as usize],
                        p.day,
                        d::MONTHS[p.month as usize],
                        p.hour,
                        p.minute,
                        p.second
                    ),
                })
            }
        }
        _ => return Ok(None),
    }))
}

/// `scrollTo(x, y)` / `scrollTo({ left, top })`: each axis, when given.
fn scroll_args(args: &[Value]) -> (Option<f64>, Option<f64>) {
    let num = |v: Option<Value>| match v {
        None | Some(Value::Undefined) => None,
        Some(v) => Some(v.to_number()),
    };
    match arg(args, 0) {
        Value::Object(o) => {
            let o = o.borrow();
            (num(obj_get(&o, "left")), num(obj_get(&o, "top")))
        }
        _ => (num(args.first().cloned()), num(args.get(1).cloned())),
    }
}

fn parse_selector(sel: &str) -> R<cw_web::css::selector::SelectorList> {
    cw_web::css::selector::parse_selector_list(sel)
        .map_err(|_| ())
        .or_else(|_| js_error("SyntaxError", format!("'{sel}' is not a valid selector")))
}

/// Object keys in JavaScript's order: integer-like keys ascending, then the rest in
/// insertion order.
pub(crate) fn object_keys(o: &[(Str, Value)]) -> Vec<Str> {
    let mut ints: Vec<(u32, Str)> = Vec::new();
    let mut rest = Vec::new();
    for (k, _) in o {
        match k.parse::<u32>() {
            Ok(i) if i.to_string() == **k && i != u32::MAX => ints.push((i, k.clone())),
            _ => rest.push(k.clone()),
        }
    }
    ints.sort_by_key(|(i, _)| *i);
    ints.into_iter().map(|(_, k)| k).chain(rest).collect()
}

pub(crate) fn obj_get(o: &[(Str, Value)], k: &str) -> Option<Value> {
    o.iter().find(|(n, _)| &**n == k).map(|(_, v)| v.clone())
}

pub(crate) fn obj_set(o: &mut Vec<(Str, Value)>, k: Str, v: Value) {
    match o.iter_mut().find(|(n, _)| *n == k) {
        Some(e) => e.1 = v,
        None => o.push((k, v)),
    }
}

/// A property key from a value (`obj[k]`).
pub(crate) fn key_string(v: &Value) -> Str {
    match v {
        Value::Str(s) => s.clone(),
        other => Rc::from(other.to_js_string().as_str()),
    }
}

fn compare_strings(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// JavaScript's `Math.round`: halves round toward +∞.
fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}

pub(crate) fn to_int32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    let m = n.trunc().rem_euclid(4294967296.0);
    (m as u32) as i32
}

fn parse_int(s: &str, radix: f64) -> f64 {
    let t = s.trim_start();
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let mut radix = if radix.is_nan() || radix == 0.0 {
        0
    } else {
        radix as u32
    };
    let mut t = t;
    if radix == 0 || radix == 16 {
        if let Some(r) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            t = r;
            radix = 16;
        }
    }
    if radix == 0 {
        radix = 10;
    }
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    let mut v = 0f64;
    let mut any = false;
    for c in t.chars() {
        match c.to_digit(radix) {
            Some(d) => {
                v = v * radix as f64 + d as f64;
                any = true;
            }
            None => break,
        }
    }
    if !any {
        return f64::NAN;
    }
    if neg {
        -v
    } else {
        v
    }
}

fn parse_float(s: &str) -> f64 {
    let t = s.trim_start();
    if t.starts_with("Infinity") || t.starts_with("+Infinity") {
        return f64::INFINITY;
    }
    if t.starts_with("-Infinity") {
        return f64::NEG_INFINITY;
    }
    let b = t.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_e = false;
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() {
            seen_digit = true;
            end = i + 1;
        } else if c == b'.' && !seen_dot && !seen_e {
            seen_dot = true;
        } else if (c == b'e' || c == b'E') && seen_digit && !seen_e {
            seen_e = true;
            if i + 1 < b.len() && (b[i + 1] == b'+' || b[i + 1] == b'-') {
                i += 1;
            }
        } else {
            break;
        }
        i += 1;
    }
    if !seen_digit {
        return f64::NAN;
    }
    t[..end].parse::<f64>().unwrap_or(f64::NAN)
}

impl Runtime {
    // ------------------------------------------------------------------ calls

    pub(crate) fn call_value(&mut self, f: &Value, args: Vec<Value>) -> R<Value> {
        match f {
            Value::Func(c) => {
                let c = c.clone();
                self.call_closure(&c, args, None)
            }
            Value::Setter(inst, hook) => {
                self.set_state(*inst, *hook, arg(&args, 0))?;
                Ok(Value::Undefined)
            }
            Value::Dispatch(inst, hook) => {
                self.dispatch_action(*inst, *hook, arg(&args, 0))?;
                Ok(Value::Undefined)
            }
            Value::Native(n) => {
                let n = n.clone();
                self.call_native(&n, args)
            }
            Value::Foreign(f) if f.callable => {
                let f = f.clone();
                self.foreign_call(&f, args)
            }
            other => type_error(format!("{} is not a function", inspect(other))),
        }
    }

    /// Calls a closure; `inst` marks a component's render frame.
    pub(crate) fn call_closure(
        &mut self,
        c: &Rc<Closure>,
        args: Vec<Value>,
        inst: Option<u32>,
    ) -> R<Value> {
        let program = self.program.clone();
        program.call(self, c, args, inst)
    }

    /// Runs function `f` (the IR of `c.func`) on the interpreter.
    pub(crate) fn interpret_call(
        &mut self,
        f: &Function,
        c: &Rc<Closure>,
        args: Vec<Value>,
        inst: Option<u32>,
    ) -> R<Value> {
        let mut frame = Frame {
            locals: vec![Value::Undefined; f.n_locals as usize],
            closure: c.clone(),
            inst,
            occ: BTreeMap::new(),
            boxed: self.boxed_flags(c.func),
        };
        let mut args = args.into_iter();
        for p in &f.params {
            let v = args.next().unwrap_or(Value::Undefined);
            self.bind(&mut frame, p, v)?;
        }
        if let Some(r) = &f.rest {
            self.bind(&mut frame, r, Value::array(args.collect()))?;
        }
        if f.is_async {
            let program = self.program.clone();
            return Ok(crate::asyncfn::start(self, program, c.func, frame));
        }
        match self.exec_block(&mut frame, &f.body)? {
            Flow::Return(v) => Ok(v),
            _ => Ok(Value::Undefined),
        }
    }

    /// Per slot of function `f`: whether it is boxed.
    pub(crate) fn boxed_flags(&mut self, f: u32) -> Rc<[bool]> {
        if let Some(Some(b)) = self.boxed_cache.get(f as usize) {
            return b.clone();
        }
        let n_locals = self
            .program
            .function_ir(f)
            .map(|func| func.n_locals as usize)
            .unwrap_or(0);
        let mut flags = vec![false; n_locals];
        for b in self.program.boxed(f) {
            if let Some(x) = flags.get_mut(*b as usize) {
                *x = true;
            }
        }
        let flags: Rc<[bool]> = Rc::from(flags);
        if self.boxed_cache.len() <= f as usize {
            self.boxed_cache.resize(f as usize + 1, None);
        }
        self.boxed_cache[f as usize] = Some(flags.clone());
        flags
    }

    /// How many arguments a callback reads (so `map` does not build unused ones).
    fn arity(&self, f: &Value) -> usize {
        match f {
            Value::Func(c) => self.program.arity(c.func),
            // A built-in passed as a callback sees every argument (`map(parseInt)`).
            Value::Native(n) if matches!(**n, NativeFn::Builtin(_)) => 3,
            _ => 1,
        }
    }

    fn callback(&mut self, f: &Value, arity: usize, a: Value, i: usize, arr: &Value) -> R<Value> {
        let args = match arity {
            0 => vec![],
            1 => vec![a],
            2 => vec![a, Value::Num(i as f64)],
            _ => vec![a, Value::Num(i as f64), arr.clone()],
        };
        self.call_value(f, args)
    }

    // ------------------------------------------------------------------ statements

    pub(crate) fn exec_block(&mut self, frame: &mut Frame, stmts: &[Stmt]) -> R<Flow> {
        for s in stmts {
            match self.exec(frame, s)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, frame: &mut Frame, s: &Stmt) -> R<Flow> {
        match s {
            Stmt::Let(p, init) => {
                let v = match init {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                self.bind(frame, p, v)?;
            }
            Stmt::Expr(e) => {
                self.eval(frame, e)?;
            }
            Stmt::If(c, a, b) => {
                let t = self.eval(frame, c)?.truthy();
                return self.exec_block(frame, if t { a } else { b });
            }
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(frame, e)?,
                    None => Value::Undefined,
                };
                return Ok(Flow::Return(v));
            }
            Stmt::Block(b) => return self.exec_block(frame, b),
            Stmt::ForOf(p, it, body) => {
                let v = self.eval(frame, it)?;
                let items = self.iterate(&v)?;
                for v in items {
                    self.bind(frame, p, v)?;
                    match self.exec_block(frame, body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                }
            }
            Stmt::For {
                init,
                test,
                update,
                body,
            } => {
                for s in init {
                    self.exec(frame, s)?;
                }
                let mut guard = 0u64;
                loop {
                    if let Some(t) = test {
                        if !self.eval(frame, t)?.truthy() {
                            break;
                        }
                    }
                    match self.exec_block(frame, body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if let Some(u) = update {
                        self.eval(frame, u)?;
                    }
                    guard += 1;
                    if guard > 50_000_000 {
                        return js_error("RangeError", "loop did not terminate");
                    }
                }
            }
            Stmt::Switch(d, cases) => {
                let v = self.eval(frame, d)?;
                let mut matched = None;
                for (i, (t, _)) in cases.iter().enumerate() {
                    if let Some(t) = t {
                        if strict_equals(&v, &self.eval(frame, t)?) {
                            matched = Some(i);
                            break;
                        }
                    }
                }
                let start = matched.or_else(|| cases.iter().position(|(t, _)| t.is_none()));
                if let Some(start) = start {
                    for (_, body) in &cases[start..] {
                        match self.exec_block(frame, body)? {
                            Flow::Normal => {}
                            Flow::Break => return Ok(Flow::Normal),
                            other => return Ok(other),
                        }
                    }
                }
            }
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Continue => return Ok(Flow::Continue),
            Stmt::Throw(e) => {
                let v = self.eval(frame, e)?;
                return throw(v);
            }
            Stmt::Try {
                block,
                param,
                handler,
                finalizer,
            } => {
                let mut r = self.exec_block(frame, block);
                if let (Err(Throw::Value(v)), Some(h)) = (&r, handler) {
                    let v = v.clone();
                    r = match param {
                        Some(p) => self
                            .bind(frame, p, v)
                            .and_then(|_| self.exec_block(frame, h)),
                        None => self.exec_block(frame, h),
                    };
                }
                if let Some(f) = finalizer {
                    match self.exec_block(frame, f)? {
                        Flow::Normal => {}
                        other => return Ok(other),
                    }
                }
                return r;
            }
        }
        Ok(Flow::Normal)
    }

    /// One statement (for the async walk).
    pub(crate) fn exec_stmt(&mut self, frame: &mut Frame, s: &Stmt) -> R<Flow> {
        self.exec(frame, s)
    }

    pub(crate) fn iterate(&mut self, v: &Value) -> R<Vec<Value>> {
        match v {
            Value::Array(a) => Ok(a.borrow().clone()),
            Value::Str(s) => Ok(s.chars().map(|c| Value::str(&c.to_string())).collect()),
            Value::Set(a) => Ok(a.borrow().clone()),
            Value::Map(m) => Ok(m
                .borrow()
                .iter()
                .map(|(k, v)| Value::array(vec![k.clone(), v.clone()]))
                .collect()),
            Value::Foreign(f) => {
                let f = f.clone();
                self.foreign_items(&f)
            }
            other => type_error(format!("{} is not iterable", inspect(other))),
        }
    }

    pub(crate) fn bind(&mut self, frame: &mut Frame, p: &Pattern, v: Value) -> R<()> {
        match p {
            Pattern::Local(n) => {
                frame.locals[*n as usize] = if frame.boxed.get(*n as usize) == Some(&true) {
                    Value::Cell(Rc::new(RefCell::new(v)))
                } else {
                    v
                };
            }
            Pattern::Ignore => {}
            Pattern::Default(inner, d) => {
                let v = if matches!(v, Value::Undefined) {
                    self.eval(frame, d)?
                } else {
                    v
                };
                self.bind(frame, inner, v)?;
            }
            Pattern::Array { items, rest } => {
                let arr = match &v {
                    Value::Foreign(_) => self.iterate(&v)?,
                    _ => crate::gen::array_items(&v)?,
                };
                for (i, item) in items.iter().enumerate() {
                    if let Some(p) = item {
                        self.bind(frame, p, arr.get(i).cloned().unwrap_or_default())?;
                    }
                }
                if let Some(r) = rest {
                    self.bind(frame, r, crate::gen::array_rest(&arr, items.len()))?;
                }
            }
            Pattern::Object { props, rest } => {
                crate::gen::destructure_check(&v)?;
                for (k, p) in props {
                    let x = self.get_member(&v, k)?;
                    self.bind(frame, p, x)?;
                }
                if let Some(r) = rest {
                    let whole = self.plain_object(&v)?;
                    let rest_v =
                        crate::gen::object_rest(&whole, &mut props.iter().map(|(n, _)| n.as_str()));
                    self.bind(frame, r, rest_v)?;
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ expressions

    fn items(&mut self, frame: &mut Frame, items: &[ArrayItem]) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(items.len());
        for i in items {
            match i {
                ArrayItem::Item(e) => out.push(self.eval(frame, e)?),
                ArrayItem::Spread(e) => {
                    let v = self.eval(frame, e)?;
                    out.extend(self.iterate(&v)?);
                }
            }
        }
        Ok(out)
    }

    pub(crate) fn eval(&mut self, frame: &mut Frame, e: &Expr) -> R<Value> {
        Ok(match e {
            Expr::Undefined => Value::Undefined,
            Expr::Null => Value::Null,
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::str(s),
            Expr::Local(n) => match &frame.locals[*n as usize] {
                Value::Cell(c) => c.borrow().clone(),
                v => v.clone(),
            },
            Expr::Capture(n) => match &frame.closure.captures[*n as usize] {
                Value::Cell(c) => c.borrow().clone(),
                v => v.clone(),
            },
            Expr::Global(n) => self.globals[*n as usize].clone(),
            Expr::Template(quasis, exprs) => {
                let mut s = String::new();
                for (i, q) in quasis.iter().enumerate() {
                    s.push_str(q);
                    if let Some(e) = exprs.get(i) {
                        let v = self.eval(frame, e)?;
                        s.push_str(&self.string_of(&v));
                    }
                }
                Value::str(&s)
            }
            Expr::Array(items) => Value::array(self.items(frame, items)?),
            Expr::Object(props) => {
                let mut out: Vec<(Str, Value)> = Vec::with_capacity(props.len());
                for p in props {
                    match p {
                        Prop::KeyValue(k, v) => {
                            let v = self.eval(frame, v)?;
                            obj_set(&mut out, Rc::from(k.as_str()), v);
                        }
                        Prop::Computed(k, v) => {
                            let k = key_string(&self.eval(frame, k)?);
                            let v = self.eval(frame, v)?;
                            obj_set(&mut out, k, v);
                        }
                        Prop::Spread(v) => {
                            let v = self.eval(frame, v)?;
                            let v = self.plain_object(&v)?;
                            crate::gen::obj_spread(&mut out, &v);
                        }
                    }
                }
                Value::object(out)
            }
            Expr::Member(o, name, optional) => {
                let o = self.eval(frame, o)?;
                if *optional && o.is_nullish() {
                    return Err(Throw::Short);
                }
                self.get_member(&o, name)?
            }
            Expr::Index(o, k, optional) => {
                let o = self.eval(frame, o)?;
                if *optional && o.is_nullish() {
                    return Err(Throw::Short);
                }
                let k = self.eval(frame, k)?;
                self.get_index(&o, &k)?
            }
            Expr::Call(f, args, optional) => {
                let f = self.eval(frame, f)?;
                if *optional && f.is_nullish() {
                    return Err(Throw::Short);
                }
                let args = self.items(frame, args)?;
                self.call_value(&f, args)?
            }
            Expr::Method {
                recv,
                method,
                args,
                optional,
            } => {
                let r = self.eval(frame, recv)?;
                if *optional && r.is_nullish() {
                    return Err(Throw::Short);
                }
                let args = self.items(frame, args)?;
                self.method(&r, *method, args)?
            }
            Expr::Builtin(b, args) => {
                let args = self.items(frame, args)?;
                self.builtin(*b, args)?
            }
            Expr::Unary(op, x) => {
                let v = self.eval(frame, x)?;
                match op {
                    UnaryOp::Not => Value::Bool(!v.truthy()),
                    UnaryOp::Neg => Value::Num(-v.to_number()),
                    UnaryOp::Plus => Value::Num(v.to_number()),
                    UnaryOp::BitNot => Value::Num(!to_int32(v.to_number()) as f64),
                    UnaryOp::Void => Value::Undefined,
                }
            }
            Expr::TypeOf(x) => {
                let v = self.eval(frame, x)?;
                Value::str(v.type_of())
            }
            Expr::Binary(op, a, b) => {
                let a = self.eval(frame, a)?;
                let b = self.eval(frame, b)?;
                self.binary(*op, &a, &b)?
            }
            Expr::Logical(op, a, b) => {
                let a = self.eval(frame, a)?;
                match op {
                    LogicalOp::And => {
                        if a.truthy() {
                            self.eval(frame, b)?
                        } else {
                            a
                        }
                    }
                    LogicalOp::Or => {
                        if a.truthy() {
                            a
                        } else {
                            self.eval(frame, b)?
                        }
                    }
                    LogicalOp::Nullish => {
                        if a.is_nullish() {
                            self.eval(frame, b)?
                        } else {
                            a
                        }
                    }
                }
            }
            Expr::Cond(t, a, b) => {
                if self.eval(frame, t)?.truthy() {
                    self.eval(frame, a)?
                } else {
                    self.eval(frame, b)?
                }
            }
            Expr::Assign(lv, op, v) => {
                let v = match op {
                    None => self.eval(frame, v)?,
                    Some(op) => {
                        let cur = self.read_lvalue(frame, lv)?;
                        let rhs = self.eval(frame, v)?;
                        self.binary(*op, &cur, &rhs)?
                    }
                };
                self.write_lvalue(frame, lv, v.clone())?;
                v
            }
            Expr::Update(lv, prefix, delta) => {
                let cur = self.read_lvalue(frame, lv)?.to_number();
                let new = cur + delta;
                self.write_lvalue(frame, lv, Value::Num(new))?;
                Value::Num(if *prefix { new } else { cur })
            }
            Expr::Closure(f) => {
                let captures = self
                    .program
                    .captures(*f)
                    .iter()
                    .map(|c| frame.slot(c).clone())
                    .collect();
                Value::Func(Rc::new(Closure { func: *f, captures }))
            }
            Expr::Hook(h, args) => self.hook(frame, *h, args)?,
            Expr::Element(el) => self.element(frame, el)?,
            Expr::Seq(xs) => {
                let mut last = Value::Undefined;
                for x in xs {
                    last = self.eval(frame, x)?;
                }
                last
            }
            Expr::Regex(pattern, flags) => self.new_regex(pattern, flags)?,
            Expr::BuiltinFn(b) => Value::Native(Rc::new(NativeFn::Builtin(*b))),
            Expr::Invoke {
                recv,
                name,
                args,
                optional,
            } => {
                let r = self.eval(frame, recv)?;
                if *optional && r.is_nullish() {
                    return Err(Throw::Short);
                }
                let args = self.items(frame, args)?;
                self.invoke_by_name(&r, name, args)?
            }
            Expr::Await(_) => {
                return js_error("SyntaxError", "await is only valid in an async function")
            }
            Expr::Chain(x) => match self.eval(frame, x) {
                Err(Throw::Short) => Value::Undefined,
                other => other?,
            },
        })
    }

    fn read_lvalue(&mut self, frame: &mut Frame, lv: &LValue) -> R<Value> {
        match lv {
            LValue::Local(n) => Ok(match &frame.locals[*n as usize] {
                Value::Cell(c) => c.borrow().clone(),
                v => v.clone(),
            }),
            LValue::Capture(n) => Ok(match &frame.closure.captures[*n as usize] {
                Value::Cell(c) => c.borrow().clone(),
                v => v.clone(),
            }),
            LValue::Global(n) => Ok(self.globals[*n as usize].clone()),
            LValue::Member(o, k) => {
                let o = self.eval(frame, o)?;
                self.get_member(&o, k)
            }
            LValue::Index(o, k) => {
                let o = self.eval(frame, o)?;
                let k = self.eval(frame, k)?;
                self.get_index(&o, &k)
            }
        }
    }

    pub(crate) fn write_lvalue(&mut self, frame: &mut Frame, lv: &LValue, v: Value) -> R<()> {
        match lv {
            LValue::Local(n) => match &frame.locals[*n as usize] {
                Value::Cell(c) => *c.borrow_mut() = v,
                _ => frame.locals[*n as usize] = v,
            },
            LValue::Capture(n) => {
                crate::gen::write_capture(&frame.closure.captures[*n as usize], v)?
            }
            LValue::Global(n) => self.globals[*n as usize] = v,
            LValue::Member(o, k) => {
                let o = self.eval(frame, o)?;
                self.set_member(&o, k, v)?;
            }
            LValue::Index(o, k) => {
                let o = self.eval(frame, o)?;
                let k = self.eval(frame, k)?;
                self.set_index(&o, &k, v)?;
            }
        }
        Ok(())
    }

    pub(crate) fn binary(&mut self, op: BinaryOp, a: &Value, b: &Value) -> R<Value> {
        use BinaryOp as B;
        if matches!(a, Value::Foreign(_)) || matches!(b, Value::Foreign(_)) {
            if let Some(v) = self.foreign_binary(op, a, b)? {
                return Ok(v);
            }
        }
        Ok(match op {
            B::Add => crate::gen::add(a, b),
            B::Sub => Value::Num(a.to_number() - b.to_number()),
            B::Mul => Value::Num(a.to_number() * b.to_number()),
            B::Div => Value::Num(a.to_number() / b.to_number()),
            B::Rem => Value::Num(a.to_number() % b.to_number()),
            B::Exp => Value::Num(a.to_number().powf(b.to_number())),
            B::StrictEq => Value::Bool(strict_equals(a, b)),
            B::StrictNotEq => Value::Bool(!strict_equals(a, b)),
            B::Eq => Value::Bool(loose_equals(a, b)),
            B::NotEq => Value::Bool(!loose_equals(a, b)),
            B::Lt | B::LtEq | B::Gt | B::GtEq => Value::Bool(crate::gen::compare(op, a, b)),
            B::BitAnd => Value::Num((to_int32(a.to_number()) & to_int32(b.to_number())) as f64),
            B::BitOr => Value::Num((to_int32(a.to_number()) | to_int32(b.to_number())) as f64),
            B::BitXor => Value::Num((to_int32(a.to_number()) ^ to_int32(b.to_number())) as f64),
            B::Shl => Value::Num(
                to_int32(a.to_number()).wrapping_shl(to_int32(b.to_number()) as u32 & 31) as f64,
            ),
            B::Shr => Value::Num(
                (to_int32(a.to_number()) >> (to_int32(b.to_number()) as u32 & 31)) as f64,
            ),
            B::UShr => Value::Num(
                ((to_int32(a.to_number()) as u32) >> (to_int32(b.to_number()) as u32 & 31)) as f64,
            ),
            B::In => {
                let k = key_string(a);
                Value::Bool(match b {
                    Value::Object(o) => o.borrow().iter().any(|(n, _)| *n == k),
                    Value::Array(arr) => {
                        &*k == "length" || k.parse::<usize>().is_ok_and(|i| i < arr.borrow().len())
                    }
                    other => {
                        return type_error(format!(
                            "Cannot use 'in' operator to search for '{k}' in {}",
                            inspect(other)
                        ))
                    }
                })
            }
        })
    }

    // ------------------------------------------------------------------ members

    pub(crate) fn get_member(&mut self, o: &Value, name: &str) -> R<Value> {
        if let Value::Foreign(f) = o {
            let f = f.clone();
            return self.foreign_get(&f, name);
        }
        Ok(match o {
            Value::Object(obj) => obj_get(&obj.borrow(), name).unwrap_or_default(),
            Value::Array(a) => match name {
                "length" => Value::Num(a.borrow().len() as f64),
                _ => match name.parse::<usize>() {
                    Ok(i) => a.borrow().get(i).cloned().unwrap_or_default(),
                    Err(_) => Value::Undefined,
                },
            },
            Value::Str(s) => match name {
                "length" => Value::Num(s.encode_utf16().count() as f64),
                _ => Value::Undefined,
            },
            Value::Ref(r) => match name {
                "current" => r.borrow().clone(),
                _ => Value::Undefined,
            },
            Value::Node(n) => self.node_prop(*n, name),
            Value::Event(e) if e.extra.iter().any(|(k, _)| &**k == name) => e
                .extra
                .iter()
                .find(|(k, _)| &**k == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            Value::Event(e) => match name {
                "target" => Value::Node(e.target),
                "currentTarget" => Value::Node(e.current_target.get()),
                "type" => Value::Str(e.ty.clone()),
                "key" => Value::Str(e.key.clone()),
                "code" => Value::Str(e.code.clone()),
                "shiftKey" => Value::Bool(e.mods.shift),
                "ctrlKey" => Value::Bool(e.mods.ctrl),
                "altKey" => Value::Bool(e.mods.alt),
                "metaKey" => Value::Bool(e.mods.meta),
                "repeat" => Value::Bool(e.repeat),
                "defaultPrevented" => Value::Bool(e.prevented.get()),
                "clientX" => Value::Num(e.client_x),
                "clientY" => Value::Num(e.client_y),
                "button" => Value::Num(e.button),
                "detail" => Value::Num(e.detail),
                "deltaX" => Value::Num(e.delta_x),
                "deltaY" => Value::Num(e.delta_y),
                _ => Value::Undefined,
            },
            Value::Error(e) => match name {
                "name" => Value::Str(e.name.clone()),
                "message" => Value::Str(e.message.clone()),
                "stack" => Value::str(&o.to_js_string()),
                _ => Value::Undefined,
            },
            Value::Cell(c) => {
                let inner = c.borrow().clone();
                return self.get_member(&inner, name);
            }
            Value::Set(a) => match name {
                "size" => Value::Num(a.borrow().len() as f64),
                _ => Value::Undefined,
            },
            Value::Map(m) => match name {
                "size" => Value::Num(m.borrow().len() as f64),
                _ => Value::Undefined,
            },
            Value::Regex(r) => match name {
                "source" => Value::Str(r.source.clone()),
                "flags" => Value::Str(r.flags.clone()),
                "global" => Value::Bool(r.global()),
                "lastIndex" => Value::Num(r.last_index.get() as f64),
                _ => Value::Undefined,
            },
            Value::Response(r) => match name {
                "ok" => Value::Bool((200..300).contains(&r.status)),
                "status" => Value::Num(r.status as f64),
                "statusText" => Value::str(&r.status_text),
                "url" => Value::str(&r.url),
                "headers" => o.clone(),
                // Only a `cw.fetch` reply's `body` is in the subset: its text.
                "body" => Value::str(&String::from_utf8_lossy(&r.body)),
                _ => Value::Undefined,
            },
            Value::Undefined | Value::Null => {
                return type_error(format!(
                    "Cannot read properties of {} (reading '{name}')",
                    o.to_js_string()
                ))
            }
            _ => Value::Undefined,
        })
    }

    pub(crate) fn get_index(&mut self, o: &Value, k: &Value) -> R<Value> {
        match (o, k) {
            (Value::Array(a), Value::Num(i)) => {
                if *i >= 0.0 && i.fract() == 0.0 {
                    return Ok(a.borrow().get(*i as usize).cloned().unwrap_or_default());
                }
                Ok(Value::Undefined)
            }
            (Value::Str(s), Value::Num(i)) => {
                let u = utf16(s);
                Ok(
                    if *i >= 0.0 && i.fract() == 0.0 && (*i as usize) < u.len() {
                        Value::str(&from_utf16(&u[*i as usize..*i as usize + 1]))
                    } else {
                        Value::Undefined
                    },
                )
            }
            _ => {
                let k = key_string(k);
                self.get_member(o, &k)
            }
        }
    }

    pub(crate) fn set_member(&mut self, o: &Value, name: &str, v: Value) -> R<()> {
        match o {
            Value::Foreign(f) => {
                let f = f.clone();
                return self.foreign_set(&f, name, v);
            }
            Value::Object(obj) => obj_set(&mut obj.borrow_mut(), Rc::from(name), v),
            Value::Ref(r) if name == "current" => *r.borrow_mut() = v,
            Value::Regex(r) if name == "lastIndex" => {
                r.last_index.set(v.to_number().max(0.0) as usize)
            }
            Value::Array(a) if name == "length" => {
                let n = v.to_number().max(0.0) as usize;
                a.borrow_mut().resize(n, Value::Undefined);
            }
            Value::Node(n) => self.set_node_prop(*n, name, &v),
            Value::Undefined | Value::Null => {
                return type_error(format!(
                    "Cannot set properties of {} (setting '{name}')",
                    o.to_js_string()
                ))
            }
            _ => {}
        }
        Ok(())
    }

    /// A DOM property read through a ref or an event target.
    fn node_prop(&mut self, n: NodeId, name: &str) -> Value {
        let i = &mut self.inner;
        match name {
            "value" => Value::str(&i.control_value(n)),
            "checked" => Value::Bool(i.is_checked(n)),
            "disabled" => Value::Bool(i.is_disabled(n)),
            "id" => Value::str(i.doc.attr(n, "id").unwrap_or("")),
            "name" => Value::str(i.doc.attr(n, "name").unwrap_or("")),
            "type" => Value::str(i.doc.attr(n, "type").unwrap_or("text")),
            "className" => Value::str(i.doc.attr(n, "class").unwrap_or("")),
            "tagName" => Value::str(&i.doc.tag(n).unwrap_or("").to_ascii_uppercase()),
            "textContent" => Value::str(&i.doc.text_content(n)),
            "valueAsNumber" => Value::Num(string_to_number(&i.control_value(n))),
            "selectionStart" | "selectionEnd" => {
                let len = i.control_value(n).encode_utf16().count();
                let (s, e) = i.form.selection.get(&n).copied().unwrap_or((len, len));
                Value::Num(if name == "selectionStart" { s } else { e } as f64)
            }
            // Geometry, as the Realm computes it (`crate::geometry`).
            "offsetLeft" | "offsetTop" | "offsetWidth" | "offsetHeight" | "clientLeft"
            | "clientTop" | "clientWidth" | "clientHeight" | "scrollWidth" | "scrollHeight" => {
                let m = crate::geometry::metrics(i, n);
                let k = [
                    "offsetLeft",
                    "offsetTop",
                    "offsetWidth",
                    "offsetHeight",
                    "clientLeft",
                    "clientTop",
                    "clientWidth",
                    "clientHeight",
                    "scrollWidth",
                    "scrollHeight",
                ]
                .iter()
                .position(|k| *k == name)
                .unwrap_or(0);
                Value::Num(m[k])
            }
            "scrollTop" | "scrollLeft" => {
                let (x, y) = crate::geometry::scroll_of(i, n);
                Value::Num(if name == "scrollTop" { y } else { x })
            }
            "offsetParent" => {
                i.ensure_styles();
                crate::geometry::offset_parent(i, n)
                    .map(Value::Node)
                    .unwrap_or(Value::Null)
            }
            "parentElement" => i
                .doc
                .parent(n)
                .filter(|p| i.doc.is_element(*p))
                .map(Value::Node)
                .unwrap_or(Value::Null),
            "firstElementChild" | "lastElementChild" | "children" | "childElementCount" => {
                let kids: Vec<NodeId> =
                    i.doc.children(n).filter(|c| i.doc.is_element(*c)).collect();
                match name {
                    "firstElementChild" => kids
                        .first()
                        .copied()
                        .map(Value::Node)
                        .unwrap_or(Value::Null),
                    "lastElementChild" => {
                        kids.last().copied().map(Value::Node).unwrap_or(Value::Null)
                    }
                    "childElementCount" => Value::Num(kids.len() as f64),
                    _ => Value::array(kids.into_iter().map(Value::Node).collect()),
                }
            }
            "nextElementSibling" | "previousElementSibling" => {
                let sibs: Vec<NodeId> = match i.doc.parent(n) {
                    Some(p) => i.doc.children(p).filter(|c| i.doc.is_element(*c)).collect(),
                    None => Vec::new(),
                };
                let at = sibs.iter().position(|c| *c == n);
                let pick = match (at, name) {
                    (Some(k), "nextElementSibling") => sibs.get(k + 1).copied(),
                    (Some(k), _) if k > 0 => sibs.get(k - 1).copied(),
                    _ => None,
                };
                pick.map(Value::Node).unwrap_or(Value::Null)
            }
            "isConnected" => Value::Bool(
                n == cw_web::dom::Document::ROOT
                    || i.doc.ancestors(n).any(|a| a == cw_web::dom::Document::ROOT),
            ),
            "nodeName" => Value::str(&i.doc.tag(n).unwrap_or("#text").to_ascii_uppercase()),
            _ => Value::Undefined,
        }
    }

    /// `o[k] = v`.
    pub(crate) fn set_index(&mut self, o: &Value, k: &Value, v: Value) -> R<()> {
        match (o, k) {
            (Value::Array(a), Value::Num(i)) if *i >= 0.0 && i.fract() == 0.0 => {
                let i = *i as usize;
                let mut a = a.borrow_mut();
                if i >= a.len() {
                    a.resize(i + 1, Value::Undefined);
                }
                a[i] = v;
                Ok(())
            }
            _ => {
                let k = key_string(k);
                self.set_member(o, &k, v)
            }
        }
    }

    fn set_node_prop(&mut self, n: NodeId, name: &str, v: &Value) {
        match name {
            "value" => {
                let s = v.to_js_string();
                self.inner.set_value(n, &s);
            }
            "checked" => self.inner.set_checked(n, v.truthy()),
            "scrollTop" => {
                crate::geometry::scroll_to(&mut self.inner, n, None, Some(v.to_number()))
            }
            "scrollLeft" => {
                crate::geometry::scroll_to(&mut self.inner, n, Some(v.to_number()), None)
            }
            "textContent" => {
                let kids: Vec<NodeId> = self.inner.doc.children(n).collect();
                for k in kids {
                    self.detach(k);
                }
                let s = v.to_js_string();
                if !s.is_empty() {
                    let t = self.inner.doc.create_text(&s);
                    self.inner.doc.append(n, t);
                }
                self.inner.touch();
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------ builtins

    pub(crate) fn builtin(&mut self, b: Builtin, args: Vec<Value>) -> R<Value> {
        use Builtin as B;
        let num = |i: usize| arg(&args, i).to_number();
        Ok(match b {
            B::MathMax => Value::Num(args.iter().map(|a| a.to_number()).fold(
                f64::NEG_INFINITY,
                |m, x| {
                    if m.is_nan() || x.is_nan() {
                        f64::NAN
                    } else if x > m || (x == 0.0 && m == 0.0 && m.is_sign_negative()) {
                        x
                    } else {
                        m
                    }
                },
            )),
            B::MathMin => Value::Num(args.iter().map(|a| a.to_number()).fold(
                f64::INFINITY,
                |m, x| {
                    if m.is_nan() || x.is_nan() {
                        f64::NAN
                    } else if x < m || (x == 0.0 && m == 0.0 && x.is_sign_negative()) {
                        x
                    } else {
                        m
                    }
                },
            )),
            B::MathRound => Value::Num(js_round(num(0))),
            B::MathFloor => Value::Num(num(0).floor()),
            B::MathCeil => Value::Num(num(0).ceil()),
            B::MathAbs => Value::Num(num(0).abs()),
            B::MathTrunc => Value::Num(num(0).trunc()),
            B::MathSign => {
                let x = num(0);
                Value::Num(if x.is_nan() || x == 0.0 {
                    x
                } else {
                    x.signum()
                })
            }
            B::MathSqrt => Value::Num(num(0).sqrt()),
            B::MathPow => Value::Num(num(0).powf(num(1))),
            B::MathRandom => {
                let r = self.inner.host_random_u64();
                Value::Num((r >> 11) as f64 / (1u64 << 53) as f64)
            }
            B::MathPi => Value::Num(std::f64::consts::PI),
            B::Infinity => Value::Num(f64::INFINITY),
            B::NaN => Value::Num(f64::NAN),
            B::Number => match args.first() {
                Some(Value::Foreign(f)) => {
                    let f = f.clone();
                    Value::Num(self.foreign_number(&f))
                }
                _ => Value::Num(if args.is_empty() { 0.0 } else { num(0) }),
            },
            B::NumberIsNaN => Value::Bool(matches!(arg(&args, 0), Value::Num(n) if n.is_nan())),
            B::NumberIsInteger => Value::Bool(
                matches!(arg(&args, 0), Value::Num(n) if n.is_finite() && n.fract() == 0.0),
            ),
            B::NumberIsFinite => {
                Value::Bool(matches!(arg(&args, 0), Value::Num(n) if n.is_finite()))
            }
            B::IsNaN => Value::Bool(num(0).is_nan()),
            B::ParseInt => Value::Num(parse_int(&arg(&args, 0).to_js_string(), num(1))),
            B::ParseFloat => Value::Num(parse_float(&arg(&args, 0).to_js_string())),
            B::String => Value::str(&if args.is_empty() {
                String::new()
            } else {
                self.string_of(&arg(&args, 0))
            }),
            B::Boolean => Value::Bool(arg(&args, 0).truthy()),
            B::ArrayIsArray => Value::Bool(match arg(&args, 0) {
                Value::Array(_) => true,
                Value::Foreign(f) => f.array,
                _ => false,
            }),
            B::ArrayFrom => {
                let src = arg(&args, 0);
                let items = match &src {
                    Value::Object(o) => {
                        let len = obj_get(&o.borrow(), "length")
                            .map(|l| l.to_number())
                            .unwrap_or(0.0);
                        let len = if len.is_finite() && len > 0.0 {
                            len as usize
                        } else {
                            0
                        };
                        (0..len)
                            .map(|i| obj_get(&o.borrow(), &i.to_string()).unwrap_or_default())
                            .collect()
                    }
                    other => self.iterate(other)?,
                };
                let f = arg(&args, 1);
                if f.is_nullish() {
                    Value::array(items)
                } else {
                    let arity = self.arity(&f);
                    let mut out = Vec::with_capacity(items.len());
                    let arr = Value::Undefined;
                    for (i, v) in items.into_iter().enumerate() {
                        out.push(self.callback(&f, arity.min(2), v, i, &arr)?);
                    }
                    Value::array(out)
                }
            }
            B::ArrayOf => Value::array(args),
            B::ObjectKeys | B::ObjectValues | B::ObjectEntries => {
                let src = self.plain_object(&arg(&args, 0))?;
                let pairs: Vec<(Str, Value)> = match &src {
                    Value::Object(o) => {
                        let o = o.borrow();
                        object_keys(&o)
                            .into_iter()
                            .map(|k| {
                                let v = obj_get(&o, &k).unwrap_or_default();
                                (k, v)
                            })
                            .collect()
                    }
                    Value::Array(a) => a
                        .borrow()
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (Rc::from(i.to_string().as_str()), v.clone()))
                        .collect(),
                    Value::Str(s) => s
                        .chars()
                        .enumerate()
                        .map(|(i, c)| {
                            (Rc::from(i.to_string().as_str()), Value::str(&c.to_string()))
                        })
                        .collect(),
                    Value::Undefined | Value::Null => {
                        return type_error("Cannot convert undefined or null to object")
                    }
                    _ => Vec::new(),
                };
                Value::array(match b {
                    B::ObjectKeys => pairs.into_iter().map(|(k, _)| Value::Str(k)).collect(),
                    B::ObjectValues => pairs.into_iter().map(|(_, v)| v).collect(),
                    _ => pairs
                        .into_iter()
                        .map(|(k, v)| Value::array(vec![Value::Str(k), v]))
                        .collect(),
                })
            }
            B::ObjectAssign => {
                let target = arg(&args, 0);
                if let Value::Object(t) = &target {
                    for src in &args[1..] {
                        if let Value::Object(s) = src {
                            let pairs = s.borrow().clone();
                            let mut t = t.borrow_mut();
                            for (k, v) in pairs {
                                obj_set(&mut t, k, v);
                            }
                        }
                    }
                }
                target
            }
            B::ObjectFromEntries => {
                let mut out = Vec::new();
                for e in self.iterate(&arg(&args, 0))? {
                    let k = self.get_index(&e, &Value::Num(0.0))?;
                    let v = self.get_index(&e, &Value::Num(1.0))?;
                    obj_set(&mut out, key_string(&k), v);
                }
                Value::object(out)
            }
            B::JsonStringify if matches!(args.first(), Some(Value::Foreign(_))) => {
                let Some(Value::Foreign(f)) = args.first().cloned() else {
                    unreachable!()
                };
                match self.foreign_json(&f) {
                    Some(s) => Value::str(&s),
                    None => Value::Undefined,
                }
            }
            B::JsonStringify => match crate::json::stringify(&arg(&args, 0), &arg(&args, 2)) {
                Some(s) => Value::str(&s),
                None => Value::Undefined,
            },
            B::JsonParse => match crate::json::parse(&arg(&args, 0).to_js_string()) {
                Ok(v) => v,
                Err(e) => return js_error("SyntaxError", e),
            },
            B::DateNow => Value::Num(self.now_ms().floor()),
            B::ConsoleLog | B::ConsoleWarn | B::ConsoleError => {
                let text: Vec<String> = args.iter().map(inspect).collect();
                let level = match b {
                    B::ConsoleWarn => LogLevel::Warn,
                    B::ConsoleError => LogLevel::Error,
                    _ => LogLevel::Log,
                };
                self.log(level, &text.join(" "));
                Value::Undefined
            }
            B::SetTimeout | B::SetInterval => {
                let id = self.next_timer;
                self.next_timer += 1;
                let delay = num(1);
                let delay = if delay.is_nan() || delay < 0.0 {
                    0.0
                } else {
                    delay
                };
                self.timers.push(Timer {
                    id,
                    due: self.clock_ms + delay,
                    interval: if b == B::SetInterval {
                        Some(delay.max(1.0))
                    } else {
                        None
                    },
                    callback: arg(&args, 0),
                    args: args.get(2..).map(|a| a.to_vec()).unwrap_or_default(),
                });
                Value::Num(id as f64)
            }
            B::ClearTimeout | B::ClearInterval => {
                let id = num(0);
                self.timers.retain(|t| t.id as f64 != id);
                Value::Undefined
            }
            B::Fetch => self.fetch(&args)?,
            B::PromiseResolve => {
                let p = new_promise();
                self.resolve_promise(&p, arg(&args, 0));
                Value::Promise(p)
            }
            B::DocumentTitle => Value::str(&self.inner.title()),
            B::Error | B::TypeError => {
                let m = match arg(&args, 0) {
                    Value::Undefined => String::new(),
                    v => v.to_js_string(),
                };
                Value::error(if b == B::Error { "Error" } else { "TypeError" }, &m)
            }
            B::NewPromise => {
                let p = new_promise();
                let resolve = Value::Native(Rc::new(NativeFn::Resolver {
                    promise: p.clone(),
                    reject: false,
                }));
                let reject = Value::Native(Rc::new(NativeFn::Resolver {
                    promise: p.clone(),
                    reject: true,
                }));
                if let Err(Throw::Value(e)) = self.call_value(&arg(&args, 0), vec![resolve, reject])
                {
                    self.reject_promise(&p, e);
                }
                Value::Promise(p)
            }
            B::ObjectIs => Value::Bool(same_value(&arg(&args, 0), &arg(&args, 1))),
            B::IsError => Value::Bool(match arg(&args, 0) {
                Value::Error(e) => {
                    let want = arg(&args, 1).to_js_string();
                    want == "Error" || *e.name == *want
                }
                _ => false,
            }),
            B::WindowAddListener
            | B::DocumentAddListener
            | B::WindowRemoveListener
            | B::DocumentRemoveListener => {
                let window = matches!(b, B::WindowAddListener | B::WindowRemoveListener);
                let ty: Str = Rc::from(arg(&args, 0).to_js_string().as_str());
                let f = arg(&args, 1);
                let capture = match arg(&args, 2) {
                    Value::Bool(c) => c,
                    Value::Object(o) => obj_get(&o.borrow(), "capture").is_some_and(|c| c.truthy()),
                    _ => false,
                };
                let same = |l: &GlobalListener| {
                    l.window == window
                        && l.ty == ty
                        && l.capture == capture
                        && strict_equals(&l.f, &f)
                };
                if matches!(b, B::WindowAddListener | B::DocumentAddListener) {
                    if !f.is_nullish() && !self.global_listeners.iter().any(same) {
                        self.global_listeners.push(GlobalListener {
                            window,
                            ty,
                            f,
                            capture,
                        });
                    }
                } else {
                    self.global_listeners.retain(|l| !same(l));
                }
                Value::Undefined
            }
            B::GetElementById => {
                let id = arg(&args, 0).to_js_string();
                let doc = &self.inner.doc;
                doc.by_id(&id)
                    .iter()
                    .copied()
                    .find(|n| doc.ancestors(*n).any(|a| a == cw_web::dom::Document::ROOT))
                    .map(Value::Node)
                    .unwrap_or(Value::Null)
            }
            B::QuerySelector => self
                .select(
                    cw_web::dom::Document::ROOT,
                    &arg(&args, 0).to_js_string(),
                    true,
                )?
                .first()
                .copied()
                .map(Value::Node)
                .unwrap_or(Value::Null),
            B::QuerySelectorAll => Value::array(
                self.select(
                    cw_web::dom::Document::ROOT,
                    &arg(&args, 0).to_js_string(),
                    false,
                )?
                .into_iter()
                .map(Value::Node)
                .collect(),
            ),
            B::DocumentElement => self
                .inner
                .doc
                .document_element()
                .map(Value::Node)
                .unwrap_or(Value::Null),
            B::ScrollX | B::ScrollY => {
                let (x, y) =
                    crate::geometry::scroll_of(&mut self.inner, cw_web::dom::Document::ROOT);
                Value::Num(if b == B::ScrollX { x } else { y })
            }
            B::WindowScrollTo | B::WindowScrollBy => {
                let (x, y) = scroll_args(&args);
                let root = cw_web::dom::Document::ROOT;
                if b == B::WindowScrollBy {
                    let (cx, cy) = crate::geometry::scroll_of(&mut self.inner, root);
                    crate::geometry::scroll_to(
                        &mut self.inner,
                        root,
                        Some(cx + x.unwrap_or(0.0)),
                        Some(cy + y.unwrap_or(0.0)),
                    );
                } else {
                    crate::geometry::scroll_to(&mut self.inner, root, x, y);
                }
                Value::Undefined
            }
            B::ActiveElement => match self.inner.focused.or_else(|| self.inner.doc.body()) {
                Some(n) => Value::Node(n),
                None => Value::Null,
            },
            B::DocumentBody => self
                .inner
                .doc
                .body()
                .map(Value::Node)
                .unwrap_or(Value::Null),
            B::InnerWidth => Value::Num(self.inner.viewport.width as f64),
            B::InnerHeight => Value::Num(self.inner.viewport.height as f64),
            B::CwKind
            | B::CwArgument
            | B::CwEnv
            | B::CwOnEnv
            | B::CwNow
            | B::CwStateGet
            | B::CwStateSet
            | B::CwReadFile
            | B::CwWriteFile
            | B::CwList
            | B::CwMkdir
            | B::CwFetch
            | B::CwLaunch
            | B::CwEmit
            | B::CwRefuse
            | B::CwWindowSet => self.cw_builtin(b, &args),
            B::PromiseReject => {
                let p = new_promise();
                self.reject_promise(&p, arg(&args, 0));
                Value::Promise(p)
            }
            B::PromiseAll => {
                let items = self.iterate(&arg(&args, 0))?;
                let result = new_promise();
                let state = Rc::new(RefCell::new(AllState {
                    values: vec![Value::Undefined; items.len()],
                    remaining: items.len(),
                    result: result.clone(),
                    done: false,
                }));
                if items.is_empty() {
                    self.resolve_promise(&result, Value::array(vec![]));
                }
                for (i, item) in items.into_iter().enumerate() {
                    let p = match item {
                        Value::Promise(p) => p,
                        v => {
                            let p = new_promise();
                            self.resolve_promise(&p, v);
                            p
                        }
                    };
                    let ok = Value::Native(Rc::new(NativeFn::AllSlot {
                        state: state.clone(),
                        index: i,
                    }));
                    let bad = Value::Native(Rc::new(NativeFn::AllReject(state.clone())));
                    self.promise_then(&p, ReactionKind::Then, ok, bad);
                }
                Value::Promise(result)
            }
            B::NewSet => {
                let mut out: Vec<Value> = Vec::new();
                let src = arg(&args, 0);
                if !src.is_nullish() {
                    for v in self.iterate(&src)? {
                        if !out.iter().any(|x| same_value_zero(x, &v)) {
                            out.push(v);
                        }
                    }
                }
                Value::Set(Rc::new(RefCell::new(out)))
            }
            B::NewDate => {
                let t = match args.as_slice() {
                    [] => self.now_ms().floor(),
                    [v] => match v {
                        Value::Date(t) => t.get(),
                        Value::Str(s) => cw_jsvm::builtins::date::parse_date(s),
                        Value::Array(_) | Value::Object(_) => {
                            cw_jsvm::builtins::date::parse_date(&v.to_js_string())
                        }
                        v => cw_jsvm::builtins::date::time_clip(v.to_number()),
                    },
                    many => date_fields(many),
                };
                Value::Date(Rc::new(std::cell::Cell::new(t)))
            }
            B::DateUTC => Value::Num(date_fields(&args)),
            B::DateParse => Value::Num(cw_jsvm::builtins::date::parse_date(
                &arg(&args, 0).to_js_string(),
            )),
            B::Construct => {
                let f = arg(&args, 0);
                self.construct(&f, args.into_iter().skip(1).collect())?
            }
            B::IsInstance => {
                let v = arg(&args, 0);
                if let Value::Foreign(f) = &v {
                    let f = f.clone();
                    let name = arg(&args, 1).to_js_string();
                    return Ok(Value::Bool(self.foreign_instance(&f, &name)));
                }
                Value::Bool(match arg(&args, 1).to_js_string().as_str() {
                    "Date" => matches!(v, Value::Date(_)),
                    "Array" => matches!(v, Value::Array(_)),
                    "Map" => matches!(v, Value::Map(_)),
                    "Set" => matches!(v, Value::Set(_)),
                    "RegExp" => matches!(v, Value::Regex(_)),
                    "Promise" => matches!(v, Value::Promise(_)),
                    "Function" => v.type_of() == "function",
                    "Object" => !matches!(
                        v,
                        Value::Undefined
                            | Value::Null
                            | Value::Bool(_)
                            | Value::Num(_)
                            | Value::Str(_)
                    ),
                    _ => false,
                })
            }
            B::StartTransition => {
                self.call_value(&arg(&args, 0), vec![])?;
                Value::Undefined
            }
            B::Delete => {
                let key = key_string(&arg(&args, 1));
                match arg(&args, 0) {
                    Value::Object(o) => o.borrow_mut().retain(|(k, _)| *k != key),
                    Value::Array(a) => {
                        if let Ok(i) = key.parse::<usize>() {
                            if let Some(slot) = a.borrow_mut().get_mut(i) {
                                *slot = Value::Undefined;
                            }
                        }
                    }
                    Value::Undefined | Value::Null => {
                        return type_error("Cannot convert undefined or null to object")
                    }
                    _ => {}
                }
                Value::Bool(true)
            }
            B::StorageGet
            | B::StorageSet
            | B::StorageRemove
            | B::StorageClear
            | B::StorageKey
            | B::StorageLength => {
                let area = if num(0) == 1.0 {
                    cw_web::script::StorageArea::Session
                } else {
                    cw_web::script::StorageArea::Local
                };
                let key = arg(&args, 1).to_js_string();
                match b {
                    B::StorageGet => match self.inner.host_storage_get(area, &key) {
                        Some(v) => Value::str(&v),
                        None => Value::Null,
                    },
                    B::StorageSet => {
                        let v = arg(&args, 2).to_js_string();
                        self.inner.host_storage_set(area, &key, &v);
                        Value::Undefined
                    }
                    B::StorageRemove => {
                        self.inner.host_storage_remove(area, &key);
                        Value::Undefined
                    }
                    B::StorageClear => {
                        for k in self.inner.host_storage_keys(area) {
                            self.inner.host_storage_remove(area, &k);
                        }
                        Value::Undefined
                    }
                    B::StorageKey => {
                        let i = num(1);
                        let keys = self.inner.host_storage_keys(area);
                        if i >= 0.0 && i.fract() == 0.0 {
                            keys.get(i as usize)
                                .map(|k| Value::str(k))
                                .unwrap_or(Value::Null)
                        } else {
                            Value::Null
                        }
                    }
                    _ => Value::Num(self.inner.host_storage_keys(area).len() as f64),
                }
            }
            B::RequestAnimationFrame => {
                self.next_raf += 1;
                let id = self.next_raf;
                self.raf.push((id, arg(&args, 0)));
                Value::Num(id as f64)
            }
            B::CancelAnimationFrame => {
                let id = arg(&args, 0).to_number();
                self.raf.retain(|(i, _)| *i as f64 != id);
                Value::Undefined
            }
            B::PerformanceNow => Value::Num(self.performance_now()),
            B::MatchMedia => self.match_media(&arg(&args, 0).to_js_string()),
            B::CreatePortal => {
                let children = match arg(&args, 0) {
                    Value::Array(a) => a.borrow().clone(),
                    Value::Foreign(f) if f.array => {
                        let f = f.clone();
                        self.foreign_items(&f)?
                    }
                    v => vec![v],
                };
                let key = match arg(&args, 2) {
                    k if k.is_nullish() => None,
                    k => Some(Rc::from(k.to_js_string().as_str())),
                };
                match arg(&args, 1) {
                    Value::Node(container) => Value::Elem(Rc::new(Elem::Portal {
                        children,
                        container,
                        key,
                    })),
                    _ => return type_error("Target container is not a DOM element."),
                }
            }
            B::HistoryPush
            | B::HistoryReplace
            | B::HistoryGo
            | B::HistoryTraverse
            | B::HistoryLength
            | B::HistoryState
            | B::LocationSet
            | B::LocationAssign
            | B::LocationReplace
            | B::LocationReload => self.history_builtin(b, &args)?,
            B::LocationPart => {
                let url = self.inner.url.clone();
                Value::str(&location_part(&url, &arg(&args, 0).to_js_string()))
            }
            B::Alert => {
                let kind = arg(&args, 0).to_js_string();
                let text = if args.len() > 1 {
                    arg(&args, 1).to_js_string()
                } else {
                    String::new()
                };
                self.inner.alerts.push(format!("{kind}: {text}"));
                match kind.as_str() {
                    "confirm" => Value::Bool(true),
                    "prompt" => Value::Null,
                    _ => Value::Undefined,
                }
            }
            B::ForInKeys => match arg(&args, 0) {
                Value::Undefined | Value::Null => Value::array(Vec::new()),
                v @ (Value::Object(_) | Value::Array(_) | Value::Str(_)) => {
                    self.builtin(B::ObjectKeys, vec![v])?
                }
                _ => Value::array(Vec::new()),
            },
            B::NewArray => match args.as_slice() {
                [Value::Num(n)] => {
                    if n.fract() != 0.0 || *n < 0.0 || *n > 4294967295.0 {
                        return js_error("RangeError", "Invalid array length");
                    }
                    Value::array(vec![Value::Undefined; *n as usize])
                }
                _ => Value::array(args),
            },
            B::NewMap => {
                let mut out: Vec<(Value, Value)> = Vec::new();
                let src = arg(&args, 0);
                if !src.is_nullish() {
                    for e in self.iterate(&src)? {
                        let k = self.get_index(&e, &Value::Num(0.0))?;
                        let v = self.get_index(&e, &Value::Num(1.0))?;
                        match out.iter_mut().find(|(x, _)| same_value_zero(x, &k)) {
                            Some(slot) => slot.1 = v,
                            None => out.push((k, v)),
                        }
                    }
                }
                Value::Map(Rc::new(RefCell::new(out)))
            }
        })
    }

    /// The elements under `root` (not `root` itself) matching `sel`, in document
    /// order; only the first when `first`.
    pub(crate) fn select(&mut self, root: NodeId, sel: &str, first: bool) -> R<Vec<NodeId>> {
        let list = parse_selector(sel)?;
        let ctx = cw_web::css::MatchContext::new();
        let doc = &self.inner.doc;
        let mut out = Vec::new();
        for n in doc.descendants(root) {
            if n != root
                && doc.is_element(n)
                && cw_web::css::matching::matches_list(doc, n, &list, &ctx)
            {
                out.push(n);
                if first {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn fetch(&mut self, args: &[Value]) -> R<Value> {
        let url = self.inner.resolve_url(&arg(args, 0).to_js_string());
        let init = arg(args, 1);
        let (mut method, mut headers, mut body) = ("GET".to_owned(), Vec::new(), None);
        if let Value::Object(o) = &init {
            let o = o.borrow();
            if let Some(m) = obj_get(&o, "method") {
                method = m.to_js_string().to_ascii_uppercase();
            }
            if let Some(Value::Object(h)) = obj_get(&o, "headers") {
                for (k, v) in h.borrow().iter() {
                    headers.push((k.to_string(), v.to_js_string()));
                }
            }
            if let Some(b) = obj_get(&o, "body") {
                if !b.is_nullish() {
                    body = Some(b.to_js_string().into_bytes());
                }
            }
        }
        let r = self.inner.host_fetch(&FetchRequest {
            url,
            method,
            headers,
            body,
        });
        let p = new_promise();
        // The transport answers at once; the page sees it in a task, as the
        // Realm's fetch does (a zero-delay timer settles the promise).
        let (value, reject) = match r {
            Ok(resp) => (Value::Response(Rc::new(resp)), false),
            Err(e) => (
                Value::error("TypeError", &format!("Failed to fetch ({e})")),
                true,
            ),
        };
        let id = self.next_timer;
        self.next_timer += 1;
        self.timers.push(Timer {
            id,
            due: self.clock_ms,
            interval: None,
            callback: Value::Native(Rc::new(NativeFn::Resolver {
                promise: p.clone(),
                reject,
            })),
            args: vec![value],
        });
        Ok(Value::Promise(p))
    }

    // ------------------------------------------------------------------ methods

    pub(crate) fn method(&mut self, r: &Value, m: Method, args: Vec<Value>) -> R<Value> {
        use Method as M;
        if let Value::Foreign(f) = r {
            // A value typed as a built-in that an island made: its own method.
            let f = f.clone();
            return self.foreign_invoke(&f, crate::ir::method_js_name(m), args);
        }
        match r {
            Value::Array(a) => return self.array_method(a, r, m, args),
            Value::Str(s) => return self.string_method(s, m, args),
            Value::Set(set) => return self.set_method(set, r, m, args),
            Value::Map(map) => return self.map_method(map, r, m, args),
            Value::Regex(re) => {
                let re = re.clone();
                return self.regex_method(&re, m, args);
            }
            Value::Date(t) => {
                if let Some(v) = date_method(t, m, &args)? {
                    return Ok(v);
                }
            }
            _ => {}
        }
        Ok(match (m, r) {
            (M::NumToFixed, Value::Num(n)) => {
                let d = arg(&args, 0).to_number();
                let d = if d.is_nan() { 0 } else { d as usize };
                Value::str(&to_fixed(*n, d))
            }
            (M::NumToString, Value::Num(n)) => {
                let radix = arg(&args, 0);
                if radix.is_nullish() || radix.to_number() == 10.0 {
                    Value::str(&number_to_string(*n))
                } else {
                    Value::str(&radix_string(*n, radix.to_number() as u32))
                }
            }
            (M::ToString, v) => Value::str(&v.to_js_string()),
            (M::PromiseThen, Value::Promise(p)) => Value::Promise(self.promise_then(
                p,
                ReactionKind::Then,
                arg(&args, 0),
                arg(&args, 1),
            )),
            (M::PromiseCatch, Value::Promise(p)) => Value::Promise(self.promise_then(
                p,
                ReactionKind::Catch,
                Value::Undefined,
                arg(&args, 0),
            )),
            (M::PromiseFinally, Value::Promise(p)) => {
                let f = arg(&args, 0);
                Value::Promise(self.promise_then(p, ReactionKind::Finally, f.clone(), f))
            }
            (M::ResponseJson, Value::Response(resp)) => {
                let p = new_promise();
                match crate::json::parse(&String::from_utf8_lossy(&resp.body)) {
                    Ok(v) => self.resolve_promise(&p, v),
                    Err(e) => self.reject_promise(&p, Value::error("SyntaxError", &e)),
                }
                Value::Promise(p)
            }
            (M::HeadersGet | M::HeadersHas, Value::Response(resp)) => {
                let name = arg(&args, 0).to_js_string().to_ascii_lowercase();
                let values: Vec<&str> = resp
                    .headers
                    .iter()
                    .filter(|(k, _)| k.eq_ignore_ascii_case(&name))
                    .map(|(_, v)| v.as_str())
                    .collect();
                if m == M::HeadersHas {
                    Value::Bool(!values.is_empty())
                } else if values.is_empty() {
                    Value::Null
                } else {
                    Value::str(&values.join(", "))
                }
            }
            (M::ResponseText, Value::Response(resp)) => {
                let p = new_promise();
                self.resolve_promise(&p, Value::str(&String::from_utf8_lossy(&resp.body)));
                Value::Promise(p)
            }
            (M::NodeFocus, Value::Node(n)) => {
                let n = *n;
                if self.inner.is_focusable(n) {
                    self.set_focus(Some(n), false);
                }
                Value::Undefined
            }
            (M::NodeBlur, Value::Node(n)) => {
                if self.inner.focused == Some(*n) {
                    self.set_focus(None, false);
                }
                Value::Undefined
            }
            (M::NodeSelect, Value::Node(n)) => {
                let len = self.inner.control_value(*n).encode_utf16().count();
                self.inner.form.selection.insert(*n, (0, len));
                Value::Undefined
            }
            (M::NodeSetSelectionRange, Value::Node(n)) => {
                let len = self.inner.control_value(*n).encode_utf16().count();
                let clamp = |v: &Value| {
                    let x = v.to_number();
                    if x.is_nan() || x < 0.0 {
                        0
                    } else {
                        (x as usize).min(len)
                    }
                };
                let end = clamp(&arg(&args, 1));
                let start = clamp(&arg(&args, 0)).min(end);
                self.inner.form.selection.insert(*n, (start, end));
                Value::Undefined
            }
            (M::NodeGetBoundingClientRect, Value::Node(n)) => {
                let r = crate::geometry::client_rect(&mut self.inner, *n);
                crate::geometry::rect_value(r)
            }
            (M::NodeGetClientRects, Value::Node(n)) => Value::array(
                crate::geometry::client_rects(&mut self.inner, *n)
                    .into_iter()
                    .map(|r| crate::geometry::rect_value(Some(r)))
                    .collect(),
            ),
            (M::NodeScrollIntoView, Value::Node(n)) => {
                let a = arg(&args, 0);
                let block = match &a {
                    Value::Object(o) => obj_get(&o.borrow(), "block").map(|b| b.to_js_string()),
                    _ => None,
                };
                let top = !(matches!(a, Value::Bool(false)) || block.as_deref() == Some("end"));
                let center = block.as_deref() == Some("center");
                crate::geometry::scroll_into_view(&mut self.inner, *n, top, center);
                Value::Undefined
            }
            (M::NodeScrollTo | M::NodeScrollBy, Value::Node(n)) => {
                let (x, y) = scroll_args(&args);
                let n = *n;
                if m == M::NodeScrollBy {
                    let (cx, cy) = crate::geometry::scroll_of(&mut self.inner, n);
                    crate::geometry::scroll_to(
                        &mut self.inner,
                        n,
                        Some(cx + x.unwrap_or(0.0)),
                        Some(cy + y.unwrap_or(0.0)),
                    );
                } else {
                    crate::geometry::scroll_to(&mut self.inner, n, x, y);
                }
                Value::Undefined
            }
            (M::NodeContains, Value::Node(n)) => Value::Bool(match arg(&args, 0) {
                Value::Node(o) => o == *n || self.inner.doc.ancestors(o).any(|a| a == *n),
                _ => false,
            }),
            (M::NodeClosest | M::NodeMatches, Value::Node(n)) => {
                let list = parse_selector(&arg(&args, 0).to_js_string())?;
                let ctx = cw_web::css::MatchContext::new();
                let doc = &self.inner.doc;
                let hit = |x: NodeId| {
                    doc.is_element(x) && cw_web::css::matching::matches_list(doc, x, &list, &ctx)
                };
                if m == M::NodeMatches {
                    Value::Bool(hit(*n))
                } else {
                    std::iter::once(*n)
                        .chain(doc.ancestors(*n))
                        .find(|x| hit(*x))
                        .map(Value::Node)
                        .unwrap_or(Value::Null)
                }
            }
            (M::NodeGetAttribute, Value::Node(n)) => {
                match self.inner.doc.attr(*n, &arg(&args, 0).to_js_string()) {
                    Some(v) => Value::str(v),
                    None => Value::Null,
                }
            }
            (M::NodeHasAttribute, Value::Node(n)) => Value::Bool(
                self.inner
                    .doc
                    .attr(*n, &arg(&args, 0).to_js_string())
                    .is_some(),
            ),
            (M::NodeQuerySelector | M::NodeQuerySelectorAll, Value::Node(n)) => {
                let found =
                    self.select(*n, &arg(&args, 0).to_js_string(), m == M::NodeQuerySelector)?;
                if m == M::NodeQuerySelector {
                    found
                        .first()
                        .copied()
                        .map(Value::Node)
                        .unwrap_or(Value::Null)
                } else {
                    Value::array(found.into_iter().map(Value::Node).collect())
                }
            }
            (M::EventPreventDefault, Value::Event(e)) => {
                e.prevented.set(true);
                Value::Undefined
            }
            (M::EventStopPropagation, Value::Event(e)) => {
                e.stopped.set(true);
                Value::Undefined
            }
            (m, v) => {
                if v.is_nullish() {
                    return type_error(format!(
                        "Cannot read properties of {} (reading '{m:?}')",
                        v.to_js_string()
                    ));
                }
                return type_error(format!("{m:?} is not a function on {}", inspect(v)));
            }
        })
    }

    /// The kind of receiver built-in methods are looked up on.
    pub(crate) fn method_kind(v: &Value) -> Option<MethodKind> {
        use MethodKind as K;
        Some(match v {
            Value::Array(_) => K::Array,
            Value::Str(_) => K::String,
            Value::Num(_) => K::Number,
            Value::Bool(_) => K::Boolean,
            Value::Promise(_) => K::Promise,
            Value::Response(_) => K::Response,
            Value::Regex(_) => K::Regex,
            Value::Set(_) => K::Set,
            Value::Map(_) => K::Map,
            Value::Node(_) => K::Node,
            Value::Event(_) => K::Event,
            Value::Date(_) => K::Date,
            _ => return None,
        })
    }

    /// `recv.name(args)` looked up when it runs (see `ir::Expr::Invoke`): a built-in
    /// method of the receiver's kind, or a function-valued property.
    pub(crate) fn invoke_by_name(&mut self, r: &Value, name: &str, args: Vec<Value>) -> R<Value> {
        use MethodKind as K;
        if let Value::Foreign(f) = r {
            let f = f.clone();
            return self.foreign_invoke(&f, name, args);
        }
        let kind = match r {
            Value::Array(_) => Some(K::Array),
            Value::Str(_) => Some(K::String),
            Value::Num(_) => Some(K::Number),
            Value::Bool(_) => Some(K::Boolean),
            Value::Promise(_) => Some(K::Promise),
            Value::Response(_) => Some(K::Response),
            Value::Regex(_) => Some(K::Regex),
            Value::Set(_) => Some(K::Set),
            Value::Map(_) => Some(K::Map),
            Value::Node(_) => Some(K::Node),
            Value::Event(_) => Some(K::Event),
            Value::Date(_) => Some(K::Date),
            Value::Cell(c) => {
                let inner = c.borrow().clone();
                return self.invoke_by_name(&inner, name, args);
            }
            Value::Undefined | Value::Null => {
                return type_error(format!(
                    "Cannot read properties of {} (reading '{name}')",
                    r.to_js_string()
                ))
            }
            _ => None,
        };
        if let Some(k) = kind {
            if let Some(m) = method_by_name(k, name) {
                return self.method(r, m, args);
            }
        }
        let f = self.get_member(r, name)?;
        match (&f, name) {
            (Value::Func(_) | Value::Setter(..) | Value::Dispatch(..) | Value::Native(_), _) => {
                self.call_value(&f, args)
            }
            // `Object.prototype`'s methods, when the object has no own function.
            (Value::Undefined, "toString") => Ok(Value::str(&r.to_js_string())),
            (Value::Undefined, "valueOf") => Ok(r.clone()),
            (Value::Undefined, "hasOwnProperty") => {
                let k = key_string(&arg(&args, 0));
                Ok(Value::Bool(match r {
                    Value::Object(o) => o.borrow().iter().any(|(n, _)| *n == k),
                    Value::Array(a) => k.parse::<usize>().is_ok_and(|i| i < a.borrow().len()),
                    _ => false,
                }))
            }
            // `Function.prototype.call` / `apply` (functions here have no `this`).
            (Value::Undefined, "call") if r.type_of() == "function" => {
                self.call_value(r, args.into_iter().skip(1).collect())
            }
            (Value::Undefined, "apply") if r.type_of() == "function" => {
                let list = match arg(&args, 1) {
                    Value::Undefined | Value::Null => Vec::new(),
                    v => self.iterate(&v)?,
                };
                self.call_value(r, list)
            }
            _ => type_error(format!("{}.{name} is not a function", inspect(r))),
        }
    }

    fn array_method(&mut self, a: &Arr, whole: &Value, m: Method, args: Vec<Value>) -> R<Value> {
        use Method as M;
        let f = arg(&args, 0);
        Ok(match m {
            M::ArrayMap | M::ArrayFilter | M::ArrayForEach | M::ArrayFlatMap => {
                let items = a.borrow().clone();
                let arity = self.arity(&f);
                let mut out = Vec::with_capacity(items.len());
                for (i, v) in items.into_iter().enumerate() {
                    let r = self.callback(&f, arity, v.clone(), i, whole)?;
                    match m {
                        M::ArrayMap => out.push(r),
                        M::ArrayFilter => {
                            if r.truthy() {
                                out.push(v)
                            }
                        }
                        M::ArrayFlatMap => match r {
                            Value::Array(inner) => out.extend(inner.borrow().iter().cloned()),
                            other => out.push(other),
                        },
                        _ => {}
                    }
                }
                if m == M::ArrayForEach {
                    Value::Undefined
                } else {
                    Value::array(out)
                }
            }
            M::ArrayFind | M::ArrayFindIndex | M::ArraySome | M::ArrayEvery | M::ArrayFindLast => {
                let items = a.borrow().clone();
                let arity = self.arity(&f);
                let order: Vec<usize> = if m == M::ArrayFindLast {
                    (0..items.len()).rev().collect()
                } else {
                    (0..items.len()).collect()
                };
                for i in order {
                    let v = items[i].clone();
                    let hit = self.callback(&f, arity, v.clone(), i, whole)?.truthy();
                    match m {
                        M::ArrayFind | M::ArrayFindLast if hit => return Ok(v),
                        M::ArrayFindIndex if hit => return Ok(Value::Num(i as f64)),
                        M::ArraySome if hit => return Ok(Value::Bool(true)),
                        M::ArrayEvery if !hit => return Ok(Value::Bool(false)),
                        _ => {}
                    }
                }
                match m {
                    M::ArrayFindIndex => Value::Num(-1.0),
                    M::ArraySome => Value::Bool(false),
                    M::ArrayEvery => Value::Bool(true),
                    _ => Value::Undefined,
                }
            }
            M::ArrayReduce => {
                let items = a.borrow().clone();
                let mut it = items.into_iter().enumerate();
                let mut acc = if args.len() > 1 {
                    args[1].clone()
                } else {
                    match it.next() {
                        Some((_, v)) => v,
                        None => return type_error("Reduce of empty array with no initial value"),
                    }
                };
                let arity = self.arity(&f);
                for (i, v) in it {
                    let call_args = match arity {
                        0 | 1 => vec![acc],
                        2 => vec![acc, v],
                        3 => vec![acc, v, Value::Num(i as f64)],
                        _ => vec![acc, v, Value::Num(i as f64), whole.clone()],
                    };
                    acc = self.call_value(&f, call_args)?;
                }
                acc
            }
            M::ArraySlice => {
                let v = a.borrow();
                let len = v.len();
                let s = rel_index(arg(&args, 0).to_number(), len);
                let e = match arg(&args, 1) {
                    Value::Undefined => len,
                    x => rel_index(x.to_number(), len),
                };
                Value::array(if s < e { v[s..e].to_vec() } else { Vec::new() })
            }
            M::ArrayConcat => {
                let mut out = a.borrow().clone();
                for x in args {
                    match x {
                        Value::Array(b) => out.extend(b.borrow().iter().cloned()),
                        other => out.push(other),
                    }
                }
                Value::array(out)
            }
            M::ArrayIncludes => {
                let x = arg(&args, 0);
                Value::Bool(a.borrow().iter().any(|v| match (v, &x) {
                    (Value::Num(p), Value::Num(q)) if p.is_nan() && q.is_nan() => true,
                    _ => strict_equals(v, &x),
                }))
            }
            M::ArrayIndexOf => {
                let x = arg(&args, 0);
                Value::Num(
                    a.borrow()
                        .iter()
                        .position(|v| strict_equals(v, &x))
                        .map(|i| i as f64)
                        .unwrap_or(-1.0),
                )
            }
            M::ArrayJoin => {
                let sep = match arg(&args, 0) {
                    Value::Undefined => ",".to_owned(),
                    s => s.to_js_string(),
                };
                let parts: Vec<String> = a
                    .borrow()
                    .iter()
                    .map(|v| {
                        if v.is_nullish() {
                            String::new()
                        } else {
                            v.to_js_string()
                        }
                    })
                    .collect();
                Value::str(&parts.join(&sep))
            }
            M::ArraySort | M::ArrayToSorted => {
                let mut items = a.borrow().clone();
                self.sort_values(&mut items, &f)?;
                if m == M::ArraySort {
                    *a.borrow_mut() = items;
                    whole.clone()
                } else {
                    Value::array(items)
                }
            }
            M::ArrayReverse => {
                a.borrow_mut().reverse();
                whole.clone()
            }
            M::ArrayToReversed => {
                let mut v = a.borrow().clone();
                v.reverse();
                Value::array(v)
            }
            M::ArrayPush => {
                let mut v = a.borrow_mut();
                v.extend(args);
                Value::Num(v.len() as f64)
            }
            M::ArrayUnshift => {
                let mut v = a.borrow_mut();
                for (i, x) in args.into_iter().enumerate() {
                    v.insert(i, x);
                }
                Value::Num(v.len() as f64)
            }
            M::ArrayPop => a.borrow_mut().pop().unwrap_or_default(),
            M::ArrayShift => {
                let mut v = a.borrow_mut();
                if v.is_empty() {
                    Value::Undefined
                } else {
                    v.remove(0)
                }
            }
            M::ArraySplice => {
                let mut v = a.borrow_mut();
                let len = v.len();
                let s = rel_index(arg(&args, 0).to_number(), len);
                let count = if args.len() < 2 {
                    len - s
                } else {
                    let c = arg(&args, 1).to_number();
                    if c.is_nan() {
                        0
                    } else {
                        (c.max(0.0) as usize).min(len - s)
                    }
                };
                let removed: Vec<Value> =
                    v.splice(s..s + count, args.into_iter().skip(2)).collect();
                Value::array(removed)
            }
            M::ArrayFlat => {
                let mut out = Vec::new();
                for v in a.borrow().iter() {
                    match v {
                        Value::Array(inner) => out.extend(inner.borrow().iter().cloned()),
                        other => out.push(other.clone()),
                    }
                }
                Value::array(out)
            }
            M::ArrayFill => {
                let x = arg(&args, 0);
                for v in a.borrow_mut().iter_mut() {
                    *v = x.clone();
                }
                whole.clone()
            }
            M::ArrayAt => {
                let v = a.borrow();
                let i = arg(&args, 0).to_number().trunc();
                let i = if i < 0.0 { v.len() as f64 + i } else { i };
                if i >= 0.0 {
                    v.get(i as usize).cloned().unwrap_or_default()
                } else {
                    Value::Undefined
                }
            }
            M::ArrayKeys => Value::array(
                (0..a.borrow().len())
                    .map(|i| Value::Num(i as f64))
                    .collect(),
            ),
            M::ArrayEntries => Value::array(
                a.borrow()
                    .iter()
                    .enumerate()
                    .map(|(i, v)| Value::array(vec![Value::Num(i as f64), v.clone()]))
                    .collect(),
            ),
            M::ArrayWith => {
                let mut v = a.borrow().clone();
                let i = arg(&args, 0).to_number().trunc();
                let i = if i < 0.0 { v.len() as f64 + i } else { i };
                if i < 0.0 || i as usize >= v.len() {
                    return js_error("RangeError", "Invalid index");
                }
                v[i as usize] = arg(&args, 1);
                Value::array(v)
            }
            M::ToString => Value::str(&whole.to_js_string()),
            other => return type_error(format!("{other:?} is not an array method")),
        })
    }

    fn sort_values(&mut self, items: &mut [Value], f: &Value) -> R<()> {
        let mut err = None;
        if f.is_nullish() {
            let keys: Vec<Option<String>> = items
                .iter()
                .map(|v| {
                    if matches!(v, Value::Undefined) {
                        None
                    } else {
                        Some(v.to_js_string())
                    }
                })
                .collect();
            let mut idx: Vec<usize> = (0..items.len()).collect();
            idx.sort_by(|a, b| match (&keys[*a], &keys[*b]) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, _) => std::cmp::Ordering::Greater,
                (_, None) => std::cmp::Ordering::Less,
                (Some(x), Some(y)) => compare_strings(x, y),
            });
            let sorted: Vec<Value> = idx.iter().map(|i| items[*i].clone()).collect();
            items.clone_from_slice(&sorted);
            return Ok(());
        }
        let mut v = items.to_vec();
        v.sort_by(|a, b| {
            if err.is_some() {
                return std::cmp::Ordering::Equal;
            }
            match (a, b) {
                (Value::Undefined, Value::Undefined) => return std::cmp::Ordering::Equal,
                (Value::Undefined, _) => return std::cmp::Ordering::Greater,
                (_, Value::Undefined) => return std::cmp::Ordering::Less,
                _ => {}
            }
            match self.call_value(f, vec![a.clone(), b.clone()]) {
                Ok(r) => {
                    let n = r.to_number();
                    if n < 0.0 {
                        std::cmp::Ordering::Less
                    } else if n > 0.0 {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                }
                Err(e) => {
                    err = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        });
        if let Some(e) = err {
            return Err(e);
        }
        items.clone_from_slice(&v);
        Ok(())
    }

    fn string_method(&mut self, s: &Str, m: Method, args: Vec<Value>) -> R<Value> {
        use Method as M;
        let sarg = |i: usize| arg(&args, i).to_js_string();
        Ok(match m {
            M::StrTrim => Value::str(s.trim_matches(is_js_space)),
            M::StrTrimStart => Value::str(s.trim_start_matches(is_js_space)),
            M::StrTrimEnd => Value::str(s.trim_end_matches(is_js_space)),
            M::StrToUpperCase => Value::str(&s.to_uppercase()),
            M::StrToLowerCase => Value::str(&s.to_lowercase()),
            M::StrIncludes => Value::Bool(s.contains(sarg(0).as_str())),
            M::StrStartsWith => Value::Bool(s.starts_with(sarg(0).as_str())),
            M::StrEndsWith => Value::Bool(s.ends_with(sarg(0).as_str())),
            M::StrIndexOf | M::StrLastIndexOf => {
                let hay = utf16(s);
                let needle = utf16(&sarg(0));
                let pos = if needle.is_empty() {
                    Some(if m == M::StrIndexOf { 0 } else { hay.len() })
                } else if m == M::StrIndexOf {
                    hay.windows(needle.len())
                        .position(|w| w == needle.as_slice())
                } else {
                    hay.windows(needle.len())
                        .rposition(|w| w == needle.as_slice())
                };
                Value::Num(pos.map(|p| p as f64).unwrap_or(-1.0))
            }
            M::StrSlice | M::StrSubstring => {
                let u = utf16(s);
                let len = u.len();
                let (st, en) = if m == M::StrSlice {
                    let st = rel_index(arg(&args, 0).to_number(), len);
                    let en = match arg(&args, 1) {
                        Value::Undefined => len,
                        x => rel_index(x.to_number(), len),
                    };
                    (st, en.max(st))
                } else {
                    let clamp = |x: f64| {
                        if x.is_nan() {
                            0
                        } else {
                            (x.max(0.0) as usize).min(len)
                        }
                    };
                    let a = clamp(arg(&args, 0).to_number());
                    let b = match arg(&args, 1) {
                        Value::Undefined => len,
                        x => clamp(x.to_number()),
                    };
                    (a.min(b), a.max(b))
                };
                Value::str(&from_utf16(&u[st..en]))
            }
            M::StrSplit if matches!(arg(&args, 0), Value::Regex(_)) => {
                let Value::Regex(re) = arg(&args, 0) else {
                    unreachable!()
                };
                self.regex_split(&re, s)?
            }
            M::StrSplit => {
                let sep = arg(&args, 0);
                let parts: Vec<Value> = match sep {
                    Value::Undefined => vec![Value::Str(s.clone())],
                    sep => {
                        let sep = sep.to_js_string();
                        if sep.is_empty() {
                            utf16(s)
                                .chunks(1)
                                .map(|c| Value::str(&from_utf16(c)))
                                .collect()
                        } else {
                            s.split(sep.as_str()).map(Value::str).collect()
                        }
                    }
                };
                Value::array(parts)
            }
            M::StrReplace | M::StrReplaceAll if matches!(arg(&args, 0), Value::Regex(_)) => {
                let Value::Regex(re) = arg(&args, 0) else {
                    unreachable!()
                };
                self.regex_replace(&re, s, &arg(&args, 1))?
            }
            M::StrMatch => match arg(&args, 0) {
                Value::Regex(re) => self.regex_match(&re, s)?,
                other => {
                    let re = self.new_regex_obj(&escape_regex(&other.to_js_string()), "")?;
                    self.regex_match(&re, s)?
                }
            },
            M::StrSearch => match arg(&args, 0) {
                Value::Regex(re) => {
                    let text: Vec<char> = s.chars().collect();
                    match self.exec_at(&re, &text, 0, false)? {
                        Some(slots) => Value::Num(utf16_len(&text[..slots[0].unwrap().0]) as f64),
                        None => Value::Num(-1.0),
                    }
                }
                other => Value::Num(
                    s.find(other.to_js_string().as_str())
                        .map(|b| s[..b].encode_utf16().count() as f64)
                        .unwrap_or(-1.0),
                ),
            },
            M::StrReplace | M::StrReplaceAll => {
                let pat = sarg(0);
                let rep = arg(&args, 1);
                let mut out = String::new();
                let mut rest: &str = s;
                let mut offset = 0usize;
                while let Some(i) = rest.find(pat.as_str()) {
                    out.push_str(&rest[..i]);
                    let matched = &rest[i..i + pat.len()];
                    let r = if rep.type_of() == "function" {
                        self.call_value(
                            &rep,
                            vec![
                                Value::str(matched),
                                Value::Num((offset + i) as f64),
                                Value::Str(s.clone()),
                            ],
                        )?
                        .to_js_string()
                    } else {
                        expand_replacement(&rep.to_js_string(), matched)
                    };
                    out.push_str(&r);
                    let adv = i + pat.len();
                    offset += adv;
                    rest = &rest[adv..];
                    if m == M::StrReplace {
                        break;
                    }
                    if pat.is_empty() {
                        // An empty pattern matches between every character.
                        match rest.chars().next() {
                            Some(c) => {
                                out.push(c);
                                rest = &rest[c.len_utf8()..];
                                offset += c.len_utf8();
                            }
                            None => break,
                        }
                    }
                }
                out.push_str(rest);
                Value::str(&out)
            }
            M::StrRepeat => {
                let n = arg(&args, 0).to_number();
                if n < 0.0 || n.is_infinite() {
                    return js_error("RangeError", "Invalid count value");
                }
                Value::str(&s.repeat(n as usize))
            }
            M::StrPadStart | M::StrPadEnd => {
                let target = arg(&args, 0).to_number().max(0.0) as usize;
                let fill = match arg(&args, 1) {
                    Value::Undefined => " ".to_owned(),
                    f => f.to_js_string(),
                };
                let u = utf16(s);
                if target <= u.len() || fill.is_empty() {
                    Value::Str(s.clone())
                } else {
                    let f = utf16(&fill);
                    let pad: Vec<u16> = f.iter().cycle().take(target - u.len()).copied().collect();
                    let mut out = Vec::new();
                    if m == M::StrPadStart {
                        out.extend(pad);
                        out.extend(u);
                    } else {
                        out.extend(u);
                        out.extend(pad);
                    }
                    Value::str(&from_utf16(&out))
                }
            }
            M::StrCharAt | M::StrAt => {
                let u = utf16(s);
                let mut i = arg(&args, 0).to_number();
                if i.is_nan() {
                    i = 0.0;
                }
                let i = i.trunc();
                let i = if m == M::StrAt && i < 0.0 {
                    u.len() as f64 + i
                } else {
                    i
                };
                if i >= 0.0 && (i as usize) < u.len() {
                    Value::str(&from_utf16(&u[i as usize..i as usize + 1]))
                } else if m == M::StrAt {
                    Value::Undefined
                } else {
                    Value::str("")
                }
            }
            M::StrCharCodeAt => {
                let u = utf16(s);
                let i = arg(&args, 0).to_number();
                let i = if i.is_nan() { 0.0 } else { i.trunc() };
                if i >= 0.0 && (i as usize) < u.len() {
                    Value::Num(u[i as usize] as f64)
                } else {
                    Value::Num(f64::NAN)
                }
            }
            M::StrLocaleCompare => {
                let o = sarg(0);
                Value::Num(match locale_compare(s, &o) {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                })
            }
            M::StrCodePointAt => {
                let u = utf16(s);
                let i = arg(&args, 0).to_number();
                let i = if i.is_nan() { 0 } else { i.trunc() as i64 };
                if i < 0 || i as usize >= u.len() {
                    Value::Undefined
                } else {
                    let i = i as usize;
                    let hi = u[i] as u32;
                    if (0xD800..0xDC00).contains(&hi)
                        && i + 1 < u.len()
                        && (0xDC00..0xE000).contains(&(u[i + 1] as u32))
                    {
                        Value::Num(
                            (((hi - 0xD800) << 10) + (u[i + 1] as u32 - 0xDC00) + 0x10000) as f64,
                        )
                    } else {
                        Value::Num(hi as f64)
                    }
                }
            }
            M::StrConcat => {
                let mut out = s.to_string();
                for a in &args {
                    out.push_str(&a.to_js_string());
                }
                Value::str(&out)
            }
            M::ToString => Value::Str(s.clone()),
            other => return type_error(format!("{other:?} is not a string method")),
        })
    }
}

/// A regex match: `(start, end)` in chars per group, group 0 the whole match.
type Slots = Vec<Option<(usize, usize)>>;

/// `SameValueZero`: what `Set` and `Map` compare keys with.
fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => strict_equals(a, b),
    }
}

fn utf16_len(chars: &[char]) -> usize {
    chars.iter().map(|c| c.len_utf16()).sum()
}

/// The char index at a UTF-16 index.
fn char_index(chars: &[char], utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in chars.iter().enumerate() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
    }
    chars.len()
}

fn escape_regex(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\^$.*+?()[]{}|/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl Runtime {
    fn new_regex_obj(&mut self, pattern: &str, flags: &str) -> R<Rc<RegexObj>> {
        let key = (pattern.to_owned(), flags.to_owned());
        let re = match self.regex_cache.get(&key) {
            Some(r) => r.clone(),
            None => {
                let mut fl = cw_regex::Flags::default();
                for c in flags.chars() {
                    match c {
                        'i' => fl.ignore_case = true,
                        'm' => fl.multiline = true,
                        's' => fl.dot_all = true,
                        'u' | 'v' => fl.unicode = true,
                        'g' | 'y' | 'd' => {}
                        _ => {
                            return js_error(
                                "SyntaxError",
                                format!("Invalid regular expression flags '{flags}'"),
                            )
                        }
                    }
                }
                let re = match cw_regex::Regex::new(pattern, cw_regex::Flavor::JavaScript, fl) {
                    Ok(r) => Rc::new(r),
                    Err(e) => return js_error("SyntaxError", e.message),
                };
                self.regex_cache.insert(key, re.clone());
                re
            }
        };
        Ok(Rc::new(RegexObj {
            source: Rc::from(pattern),
            flags: Rc::from(flags),
            re,
            last_index: std::cell::Cell::new(0),
        }))
    }

    pub(crate) fn new_regex(&mut self, pattern: &str, flags: &str) -> R<Value> {
        Ok(Value::Regex(self.new_regex_obj(pattern, flags)?))
    }

    /// One match at or after char index `start` (at exactly `start` when `sticky`).
    fn exec_at(
        &mut self,
        re: &RegexObj,
        text: &[char],
        start: usize,
        sticky: bool,
    ) -> R<Option<Slots>> {
        match re.re.exec(text, start, sticky, false) {
            Ok(m) => Ok(m),
            Err(_) => js_error("RangeError", "regular expression too complex"),
        }
    }

    /// `exec` honouring `lastIndex` for `g`/`y` regexes.
    fn exec_stateful(&mut self, re: &RegexObj, text: &[char]) -> R<Option<Slots>> {
        let stateful = re.global() || re.sticky();
        let start = if stateful {
            char_index(text, re.last_index.get())
        } else {
            0
        };
        if start > text.len() {
            re.last_index.set(0);
            return Ok(None);
        }
        let m = self.exec_at(re, text, start, re.sticky())?;
        if stateful {
            match &m {
                Some(slots) => re.last_index.set(utf16_len(&text[..slots[0].unwrap().1])),
                None => re.last_index.set(0),
            }
        }
        Ok(m)
    }

    fn groups(text: &[char], slots: &[Option<(usize, usize)>]) -> Vec<Value> {
        slots
            .iter()
            .map(|g| match g {
                Some((a, b)) => Value::str(&text[*a..*b].iter().collect::<String>()),
                None => Value::Undefined,
            })
            .collect()
    }

    fn regex_method(&mut self, re: &Rc<RegexObj>, m: Method, args: Vec<Value>) -> R<Value> {
        let s = arg(&args, 0).to_js_string();
        let text: Vec<char> = s.chars().collect();
        Ok(match m {
            Method::RegexTest => Value::Bool(self.exec_stateful(re, &text)?.is_some()),
            Method::RegexExec => match self.exec_stateful(re, &text)? {
                Some(slots) => Value::array(Self::groups(&text, &slots)),
                None => Value::Null,
            },
            Method::ToString => Value::str(&format!("/{}/{}", re.source, re.flags)),
            other => return type_error(format!("{other:?} is not a RegExp method")),
        })
    }

    fn regex_match(&mut self, re: &Rc<RegexObj>, s: &str) -> R<Value> {
        let text: Vec<char> = s.chars().collect();
        if !re.global() {
            return Ok(match self.exec_stateful(re, &text)? {
                Some(slots) => Value::array(Self::groups(&text, &slots)),
                None => Value::Null,
            });
        }
        let mut out = Vec::new();
        let mut pos = 0;
        while pos <= text.len() {
            let Some(slots) = self.exec_at(re, &text, pos, false)? else {
                break;
            };
            let (a, b) = slots[0].unwrap();
            out.push(Value::str(&text[a..b].iter().collect::<String>()));
            pos = if b == a { b + 1 } else { b };
        }
        re.last_index.set(0);
        Ok(if out.is_empty() {
            Value::Null
        } else {
            Value::array(out)
        })
    }

    fn regex_replace(&mut self, re: &Rc<RegexObj>, s: &str, rep: &Value) -> R<Value> {
        let text: Vec<char> = s.chars().collect();
        let mut out = String::new();
        let mut last = 0;
        let mut pos = if re.sticky() {
            char_index(&text, re.last_index.get())
        } else {
            0
        };
        while pos <= text.len() {
            let Some(slots) = self.exec_at(re, &text, pos, re.sticky())? else {
                break;
            };
            let (a, b) = slots[0].unwrap();
            out.extend(&text[last..a]);
            let matched: String = text[a..b].iter().collect();
            let groups: Vec<Option<String>> = slots[1..]
                .iter()
                .map(|g| g.map(|(x, y)| text[x..y].iter().collect()))
                .collect();
            let r = if rep.type_of() == "function" {
                let mut call = vec![Value::str(&matched)];
                call.extend(
                    groups
                        .iter()
                        .map(|g| g.as_deref().map(Value::str).unwrap_or_default()),
                );
                call.push(Value::Num(utf16_len(&text[..a]) as f64));
                call.push(Value::str(s));
                self.call_value(rep, call)?.to_js_string()
            } else {
                let before: String = text[..a].iter().collect();
                let after: String = text[b..].iter().collect();
                cw_regex::expand_js_replacement(
                    &rep.to_js_string(),
                    &matched,
                    &before,
                    &after,
                    &groups,
                    re.re.group_names(),
                )
            };
            out.push_str(&r);
            last = b;
            if !re.global() {
                break;
            }
            pos = if b == a { b + 1 } else { b };
        }
        if last < text.len() {
            out.extend(&text[last..]);
        }
        if re.global() {
            re.last_index.set(0);
        }
        Ok(Value::str(&out))
    }

    fn regex_split(&mut self, re: &Rc<RegexObj>, s: &str) -> R<Value> {
        let text: Vec<char> = s.chars().collect();
        let piece = |a: usize, b: usize| Value::str(&text[a..b].iter().collect::<String>());
        if text.is_empty() {
            return Ok(if self.exec_at(re, &text, 0, true)?.is_some() {
                Value::array(vec![])
            } else {
                Value::array(vec![Value::str("")])
            });
        }
        let mut out = Vec::new();
        let (mut p, mut q) = (0usize, 0usize);
        while q < text.len() {
            let Some(slots) = self.exec_at(re, &text, q, false)? else {
                break;
            };
            let (ms, me) = slots[0].unwrap();
            if ms >= text.len() {
                break;
            }
            if me == p || (me == ms && ms == p) {
                q = ms + 1;
                continue;
            }
            out.push(piece(p, ms));
            out.extend(Self::groups(&text, &slots[1..]));
            p = me;
            q = if me == ms { me + 1 } else { me };
        }
        out.push(piece(p, text.len()));
        Ok(Value::array(out))
    }

    fn set_method(&mut self, set: &Arr, whole: &Value, m: Method, args: Vec<Value>) -> R<Value> {
        let x = arg(&args, 0);
        Ok(match m {
            Method::SetHas => Value::Bool(set.borrow().iter().any(|v| same_value_zero(v, &x))),
            Method::SetAdd => {
                let present = set.borrow().iter().any(|v| same_value_zero(v, &x));
                if !present {
                    set.borrow_mut().push(x);
                }
                whole.clone()
            }
            Method::SetDelete => {
                let mut v = set.borrow_mut();
                let before = v.len();
                v.retain(|y| !same_value_zero(y, &x));
                Value::Bool(v.len() != before)
            }
            Method::SetClear => {
                set.borrow_mut().clear();
                Value::Undefined
            }
            Method::CollectionForEach => {
                let items = set.borrow().clone();
                for v in items {
                    self.call_value(&x, vec![v.clone(), v, whole.clone()])?;
                }
                Value::Undefined
            }
            Method::CollectionKeys | Method::CollectionValues => Value::array(set.borrow().clone()),
            Method::CollectionEntries => Value::array(
                set.borrow()
                    .iter()
                    .map(|v| Value::array(vec![v.clone(), v.clone()]))
                    .collect(),
            ),
            other => return type_error(format!("{other:?} is not a Set method")),
        })
    }

    fn map_method(
        &mut self,
        map: &Rc<RefCell<Vec<(Value, Value)>>>,
        whole: &Value,
        m: Method,
        args: Vec<Value>,
    ) -> R<Value> {
        let k = arg(&args, 0);
        Ok(match m {
            Method::MapGet => map
                .borrow()
                .iter()
                .find(|(x, _)| same_value_zero(x, &k))
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
            Method::SetHas => Value::Bool(map.borrow().iter().any(|(x, _)| same_value_zero(x, &k))),
            Method::MapSet => {
                let v = arg(&args, 1);
                let mut e = map.borrow_mut();
                match e.iter_mut().find(|(x, _)| same_value_zero(x, &k)) {
                    Some(slot) => slot.1 = v,
                    None => e.push((k, v)),
                }
                whole.clone()
            }
            Method::SetDelete => {
                let mut e = map.borrow_mut();
                let before = e.len();
                e.retain(|(x, _)| !same_value_zero(x, &k));
                Value::Bool(e.len() != before)
            }
            Method::SetClear => {
                map.borrow_mut().clear();
                Value::Undefined
            }
            Method::CollectionForEach => {
                let items = map.borrow().clone();
                for (key, v) in items {
                    self.call_value(&k, vec![v, key, whole.clone()])?;
                }
                Value::Undefined
            }
            Method::CollectionKeys => {
                Value::array(map.borrow().iter().map(|(k, _)| k.clone()).collect())
            }
            Method::CollectionValues => {
                Value::array(map.borrow().iter().map(|(_, v)| v.clone()).collect())
            }
            Method::CollectionEntries => Value::array(
                map.borrow()
                    .iter()
                    .map(|(k, v)| Value::array(vec![k.clone(), v.clone()]))
                    .collect(),
            ),
            other => return type_error(format!("{other:?} is not a Map method")),
        })
    }
}

fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// `$&`, `$$` in a replacement string.
fn expand_replacement(rep: &str, matched: &str) -> String {
    if !rep.contains('$') {
        return rep.to_owned();
    }
    let mut out = String::new();
    let mut chars = rep.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            match chars.peek() {
                Some('$') => {
                    out.push('$');
                    chars.next();
                }
                Some('&') => {
                    out.push_str(matched);
                    chars.next();
                }
                _ => out.push('$'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A root-locale collation approximation: case-insensitive first, lower case before
/// upper case on ties, then code units.
fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let fold = |s: &str| -> String { s.to_lowercase() };
    fold(a).cmp(&fold(b)).then_with(|| b.cmp(a))
}

fn radix_string(n: f64, radix: u32) -> String {
    if !(2..=36).contains(&radix) || !n.is_finite() || n.fract() != 0.0 {
        return number_to_string(n);
    }
    let neg = n < 0.0;
    let mut v = n.abs() as u128;
    if v == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while v > 0 {
        digits.push(std::char::from_digit((v % radix as u128) as u32, radix).unwrap());
        v /= radix as u128;
    }
    if neg {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

impl Runtime {
    fn call_native(&mut self, n: &NativeFn, args: Vec<Value>) -> R<Value> {
        match n {
            NativeFn::Builtin(b) => return self.builtin(*b, args),
            NativeFn::ImperativeSet { r, create } => {
                let v = self.call_value(create, vec![])?;
                self.set_ref_value(r, v);
                return Ok(Value::Native(Rc::new(NativeFn::ImperativeClear(r.clone()))));
            }
            NativeFn::ImperativeClear(r) => {
                self.set_ref_value(r, Value::Null);
                return Ok(Value::Undefined);
            }
            NativeFn::BoundMethod { recv, name } => {
                let (recv, name) = (recv.clone(), name.clone());
                return self.invoke_by_name(&recv, &name, args);
            }
            NativeFn::MediaListen { list, add } => {
                // `addEventListener('change', f)` or the legacy `addListener(f)`.
                let f = match args.first() {
                    Some(Value::Str(_)) => args.get(1).cloned().unwrap_or_default(),
                    _ => args.first().cloned().unwrap_or_default(),
                };
                if let Some(l) = self.media_lists.get_mut(*list as usize) {
                    if *add {
                        if !f.is_nullish() && !l.2.iter().any(|g| strict_equals(g, &f)) {
                            l.2.push(f);
                        }
                    } else {
                        l.2.retain(|g| !strict_equals(g, &f));
                    }
                }
                return Ok(Value::Undefined);
            }
            _ => {}
        }
        let v = args.into_iter().next().unwrap_or_default();
        match n {
            NativeFn::Resolver { promise, reject } => {
                if *reject {
                    self.reject_promise(promise, v);
                } else {
                    self.resolve_promise(promise, v);
                }
            }
            NativeFn::AllSlot { state, index } => {
                let done = {
                    let mut st = state.borrow_mut();
                    if st.done {
                        return Ok(Value::Undefined);
                    }
                    st.values[*index] = v;
                    st.remaining -= 1;
                    st.remaining == 0
                };
                if done {
                    let (result, values) = {
                        let mut st = state.borrow_mut();
                        st.done = true;
                        (st.result.clone(), std::mem::take(&mut st.values))
                    };
                    self.resolve_promise(&result, Value::array(values));
                }
            }
            NativeFn::AllReject(state) => {
                let result = {
                    let mut st = state.borrow_mut();
                    if st.done {
                        return Ok(Value::Undefined);
                    }
                    st.done = true;
                    st.result.clone()
                };
                self.reject_promise(&result, v);
            }
            NativeFn::Resume { task, throw } => {
                crate::asyncfn::resume(self, task, v, *throw);
            }
            NativeFn::StoreChanged { inst, hook } => self.store_changed(*inst, *hook)?,
            NativeFn::CwOffEnv(id) => self.cw_off_env(*id),
            NativeFn::Builtin(_)
            | NativeFn::ImperativeSet { .. }
            | NativeFn::ImperativeClear(_)
            | NativeFn::MediaListen { .. }
            | NativeFn::BoundMethod { .. } => unreachable!("called above"),
        }
        Ok(Value::Undefined)
    }
}

pub(crate) fn new_promise() -> Rc<RefCell<Promise>> {
    Rc::new(RefCell::new(Promise {
        state: PromiseState::Pending,
        reactions: Vec::new(),
    }))
}

impl Runtime {
    pub(crate) fn resolve_promise(&mut self, p: &Rc<RefCell<Promise>>, v: Value) {
        if let Value::Foreign(f) = &v {
            // A thenable of the island: adopt it through its `then`.
            let f = f.clone();
            if self.foreign_thenable(&f) {
                let ok = Value::Native(Rc::new(NativeFn::Resolver {
                    promise: p.clone(),
                    reject: false,
                }));
                let bad = Value::Native(Rc::new(NativeFn::Resolver {
                    promise: p.clone(),
                    reject: true,
                }));
                if let Err(Throw::Value(e)) = self.foreign_then(&f, ok, bad) {
                    self.reject_promise(p, e);
                }
                return;
            }
        }
        if let Value::Promise(inner) = &v {
            // Adopt the other promise's eventual state.
            let reaction = Reaction {
                kind: ReactionKind::Then,
                on_fulfilled: None,
                on_rejected: None,
                result: p.clone(),
            };
            self.add_reaction(inner, reaction);
            return;
        }
        self.settle_promise(p, PromiseState::Fulfilled(v));
    }

    pub(crate) fn reject_promise(&mut self, p: &Rc<RefCell<Promise>>, v: Value) {
        self.settle_promise(p, PromiseState::Rejected(v));
    }

    fn settle_promise(&mut self, p: &Rc<RefCell<Promise>>, state: PromiseState) {
        let reactions = {
            let mut pb = p.borrow_mut();
            if !matches!(pb.state, PromiseState::Pending) {
                return;
            }
            pb.state = clone_state(&state);
            std::mem::take(&mut pb.reactions)
        };
        for r in reactions {
            self.microtasks
                .push_back(Microtask::Reaction(r, clone_state(&state)));
        }
    }

    fn add_reaction(&mut self, p: &Rc<RefCell<Promise>>, r: Reaction) {
        let state = {
            let pb = p.borrow();
            match &pb.state {
                PromiseState::Pending => None,
                s => Some(clone_state(s)),
            }
        };
        match state {
            Some(s) => self.microtasks.push_back(Microtask::Reaction(r, s)),
            None => p.borrow_mut().reactions.push(r),
        }
    }

    pub(crate) fn promise_then(
        &mut self,
        p: &Rc<RefCell<Promise>>,
        kind: ReactionKind,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Rc<RefCell<Promise>> {
        let result = new_promise();
        let r = Reaction {
            kind,
            on_fulfilled: (!on_fulfilled.is_nullish()).then_some(on_fulfilled),
            on_rejected: (!on_rejected.is_nullish()).then_some(on_rejected),
            result: result.clone(),
        };
        self.add_reaction(p, r);
        result
    }

    /// Runs one microtask.
    pub(crate) fn run_microtask(&mut self, m: Microtask) {
        let Microtask::Reaction(r, state) = m;
        {
            let (handler, value, fulfilled) = match &state {
                PromiseState::Fulfilled(v) => (r.on_fulfilled.clone(), v.clone(), true),
                PromiseState::Rejected(v) => (r.on_rejected.clone(), v.clone(), false),
                PromiseState::Pending => return,
            };
            match (r.kind, handler) {
                (ReactionKind::Finally, Some(h)) => match self.call_value(&h, vec![]) {
                    Ok(_) => self.settle_promise(&r.result, state),
                    Err(Throw::Value(e)) => self.reject_promise(&r.result, e),
                    Err(Throw::Short) => self.settle_promise(&r.result, state),
                },
                (_, Some(h)) => match self.call_value(&h, vec![value]) {
                    Ok(v) => self.resolve_promise(&r.result, v),
                    Err(Throw::Value(e)) => self.reject_promise(&r.result, e),
                    Err(Throw::Short) => self.resolve_promise(&r.result, Value::Undefined),
                },
                (_, None) => {
                    if fulfilled {
                        self.resolve_promise(&r.result, value)
                    } else {
                        self.reject_promise(&r.result, value)
                    }
                }
            }
        }
    }
}

fn clone_state(s: &PromiseState) -> PromiseState {
    match s {
        PromiseState::Pending => PromiseState::Pending,
        PromiseState::Fulfilled(v) => PromiseState::Fulfilled(v.clone()),
        PromiseState::Rejected(v) => PromiseState::Rejected(v.clone()),
    }
}
