//! A port of Node's `util.inspect` and `util.format` (no colors).

use crate::numconv::number_to_string;
use crate::value::*;
use crate::vm::Vm;

#[derive(Clone)]
pub struct Opts {
    /// None = unlimited.
    pub depth: Option<f64>,
    pub break_length: f64,
    /// 0 = false (always multi-line), usize::MAX = true.
    pub compact: usize,
    pub show_hidden: bool,
    pub max_array_length: usize,
    pub max_string_length: usize,
    pub sorted: bool,
    pub custom_inspect: bool,
    pub getters: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            depth: Some(2.0),
            break_length: 80.0,
            compact: 3,
            show_hidden: false,
            max_array_length: 100,
            max_string_length: 10000,
            sorted: false,
            custom_inspect: true,
            getters: false,
        }
    }
}

const COMPACT_TRUE: usize = usize::MAX;

struct Ctx {
    o: Opts,
    indent: usize,
    current_depth: usize,
    seen: Vec<Obj>,
    circular: Vec<(usize, usize)>,
}

fn len16(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.encode_utf16().count()
    }
}

fn meta(c: u32) -> Option<String> {
    Some(match c {
        8 => "\\b".into(),
        9 => "\\t".into(),
        10 => "\\n".into(),
        12 => "\\f".into(),
        13 => "\\r".into(),
        0x27 => "\\'".into(),
        0x5c => "\\\\".into(),
        c if c < 32 || (0x7f..0xa0).contains(&c) => format!("\\x{c:02X}"),
        _ => return None,
    })
}

pub fn str_escape(s: &str) -> String {
    let mut quote = '\'';
    if s.contains('\'') {
        if !s.contains('"') {
            quote = '"';
        } else if !s.contains('`') && !s.contains("${") {
            quote = '`';
        }
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        let cp = c as u32;
        if cp == 0x27 && quote != '\'' {
            out.push(c);
            continue;
        }
        match meta(cp) {
            Some(m) => out.push_str(&m),
            None => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn is_ident_key(k: &str) -> bool {
    let mut cs = k.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn format_number(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    number_to_string(n)
}

fn format_primitive(ctx: &Ctx, v: &Value) -> String {
    match v {
        Value::Str(s) => {
            let mut value: String = s.to_string();
            let mut trailer = String::new();
            let l = s.len16();
            if l > ctx.o.max_string_length {
                let remaining = l - ctx.o.max_string_length;
                value = s.slice16(0, ctx.o.max_string_length).to_string();
                trailer = format!(
                    "... {remaining} more character{}",
                    if remaining > 1 { "s" } else { "" }
                );
            }
            let vl = len16(&value) as f64;
            if ctx.o.compact != COMPACT_TRUE
                && vl > 16.0
                && vl > ctx.o.break_length - ctx.indent as f64 - 4.0
            {
                let mut lines: Vec<String> = vec![];
                let mut cur = String::new();
                for c in value.chars() {
                    cur.push(c);
                    if c == '\n' {
                        lines.push(std::mem::take(&mut cur));
                    }
                }
                if !cur.is_empty() {
                    lines.push(cur);
                }
                let sep = format!(" +\n{}", " ".repeat(ctx.indent + 2));
                return lines
                    .iter()
                    .map(|l| str_escape(l))
                    .collect::<Vec<_>>()
                    .join(&sep)
                    + &trailer;
            }
            str_escape(&value) + &trailer
        }
        Value::Num(n) => format_number(*n),
        Value::BigInt(b) => format!("{}n", b.to_str_radix(10)),
        Value::Bool(b) => b.to_string(),
        Value::Undefined | Value::Empty => "undefined".into(),
        Value::Null => "null".into(),
        Value::Sym(s) => crate::builtins::symbol::symbol_descriptive(s),
        Value::Obj(_) => String::new(),
    }
}

/// getConstructorName: None for null-prototype chains.
fn constructor_name(vm: &mut Vm, o: &Obj) -> Option<String> {
    let mut cur = Some(o.clone());
    let mut hops = 0;
    while let Some(c) = cur {
        let ctor = match c.borrow().props.get_str("constructor") {
            Some(Prop {
                slot: Slot::Data(Value::Obj(f)),
                ..
            }) => Some(f.clone()),
            _ => None,
        };
        if let Some(f) = ctor {
            if f.is_callable() {
                let n = Vm::func_name(&f);
                if !n.is_empty() {
                    return Some(n);
                }
            }
        }
        cur = c.proto();
        hops += 1;
        if hops > 1000 {
            break;
        }
    }
    let _ = vm;
    None
}

fn get_prefix(constructor: &Option<String>, tag: &str, fallback: &str, size: &str) -> String {
    match constructor {
        None => {
            if !tag.is_empty() && fallback != tag {
                format!("[{fallback}{size}: null prototype] [{tag}] ")
            } else {
                format!("[{fallback}{size}: null prototype] ")
            }
        }
        Some(c) => {
            if !tag.is_empty() && c != tag {
                format!("{c}{size} [{tag}] ")
            } else {
                format!("{c}{size} ")
            }
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Extras {
    Object,
    Array,
}

enum Fmt {
    Empty,
    Array,
    Typed,
    Set,
    Map,
    MapIter(bool),
    Promise,
    Weak,
    Arguments,
}

/// Own enumerable keys (strings then symbols); `non_index` skips indices.
fn get_keys(vm: &mut Vm, o: &Obj, non_index: bool, show_hidden: bool) -> JsResult<Vec<Key>> {
    let keys = vm.own_keys(o)?;
    let mut out = vec![];
    for k in keys {
        if non_index && k.array_index().is_some() {
            continue;
        }
        if let Key::Str(s) = &k {
            if s.as_str() == "length"
                && (o.is_array()
                    || matches!(o.borrow().kind, Kind::TypedArray { .. } | Kind::String(_)))
                && !(show_hidden && o.is_array())
            {
                continue;
            }
        }
        let Some(p) = vm.get_own(o, &k)? else {
            continue;
        };
        if p.enumerable() || show_hidden {
            out.push(k);
        }
    }
    Ok(out)
}

impl<'h> Vm<'h> {
    pub fn inspect(&mut self, v: &Value, o: &Opts) -> JsResult<String> {
        let mut ctx = Ctx {
            o: o.clone(),
            indent: 0,
            current_depth: 0,
            seen: vec![],
            circular: vec![],
        };
        format_value(self, &mut ctx, v, 0, false)
    }

    pub fn inspect_default(&mut self, v: &Value) -> JsResult<String> {
        self.inspect(v, &Opts::default())
    }
}

fn format_value(
    vm: &mut Vm,
    ctx: &mut Ctx,
    v: &Value,
    recurse: usize,
    typed: bool,
) -> JsResult<String> {
    let Value::Obj(o) = v else {
        return Ok(format_primitive(ctx, v));
    };
    // Proxies are shown as their target.
    let target = match &o.borrow().kind {
        Kind::Proxy { target, .. } => Some(target.clone()),
        _ => None,
    };
    if let Some(t) = target {
        return format_value(vm, ctx, &Value::Obj(t), recurse, typed);
    }
    if ctx.o.custom_inspect {
        let sym = vm.syms.inspect_custom.clone();
        let f = vm.get(v, &Key::Sym(sym))?;
        if f.is_callable() {
            // Skip when inspecting a prototype object holding the method.
            let is_proto = match vm.get_str(v, "constructor")? {
                Value::Obj(c) => {
                    matches!(c.own_value("prototype"), Some(Value::Obj(p)) if p.ptr_eq(o))
                }
                _ => false,
            };
            let is_inspect_fn = matches!(&f, Value::Obj(fo) if Vm::func_name(fo) == "inspect" && matches!(&fo.borrow().kind, Kind::Function(fd) if matches!(fd.imp, FuncImpl::Native{..})));
            if !is_proto && !is_inspect_fn {
                let depth = match ctx.o.depth {
                    Some(d) => Value::Num(d - recurse as f64),
                    None => Value::Null,
                };
                let opts = opts_object(vm, &ctx.o, recurse);
                let insp = vm.get_str(&Value::Obj(vm.global.clone()), "%inspect")?;
                let ret = vm.call(&f, v.clone(), vec![depth, opts, insp])?;
                let same = matches!(&ret, Value::Obj(r) if r.ptr_eq(o));
                if !same {
                    if let Value::Str(s) = &ret {
                        return Ok(s.replace('\n', &format!("\n{}", " ".repeat(ctx.indent))));
                    }
                    return format_value(vm, ctx, &ret, recurse, false);
                }
            }
        }
    }
    if ctx.seen.iter().any(|x| x.ptr_eq(o)) {
        let addr = o.addr();
        let idx = match ctx.circular.iter().find(|(a, _)| *a == addr) {
            Some((_, i)) => *i,
            None => {
                let i = ctx.circular.len() + 1;
                ctx.circular.push((addr, i));
                i
            }
        };
        return Ok(format!("[Circular *{idx}]"));
    }
    format_raw(vm, ctx, o, recurse, typed)
}

fn opts_object(vm: &mut Vm, o: &Opts, recurse: usize) -> Value {
    let obj = vm.new_object();
    obj.set_prop(
        "depth",
        o.depth
            .map(|d| Value::Num(d - recurse as f64))
            .unwrap_or(Value::Null),
        ALL,
    );
    obj.set_prop("breakLength", Value::Num(o.break_length), ALL);
    obj.set_prop(
        "compact",
        if o.compact == COMPACT_TRUE {
            Value::Bool(true)
        } else if o.compact == 0 {
            Value::Bool(false)
        } else {
            Value::Num(o.compact as f64)
        },
        ALL,
    );
    obj.set_prop("showHidden", Value::Bool(o.show_hidden), ALL);
    obj.set_prop("colors", Value::Bool(false), ALL);
    obj.set_prop("customInspect", Value::Bool(o.custom_inspect), ALL);
    let stylize = vm.native_fn("stylize", 2, |_vm, a| Ok(a.arg(0)));
    obj.set_prop("stylize", Value::Obj(stylize), ALL);
    Value::Obj(obj)
}

fn function_base(vm: &mut Vm, f: &Obj, constructor: &Option<String>, tag: &str) -> String {
    let src = crate::builtins::function::func_to_string(vm, f);
    let (is_class, is_async, is_gen) = match &f.borrow().kind {
        Kind::Function(fd) => match &fd.imp {
            FuncImpl::Closure { code, .. } => (fd.class_ctor, code.is_async, code.is_generator),
            _ => (false, false, false),
        },
        _ => (false, false, false),
    };
    if is_class || (src.starts_with("class") && src.ends_with('}')) {
        let name = match f.own_value("name") {
            Some(Value::Str(s)) if !s.is_empty() => s.to_string(),
            _ => "(anonymous)".to_string(),
        };
        let mut base = format!("class {name}");
        if let Some(c) = constructor {
            if c != "Function" {
                base.push_str(&format!(" [{c}]"));
            }
        }
        if !tag.is_empty() && constructor.as_deref() != Some(tag) {
            base.push_str(&format!(" [{tag}]"));
        }
        match constructor {
            Some(_) => {
                if let Some(p) = f.proto() {
                    let sn = Vm::func_name(&p);
                    if !sn.is_empty() {
                        base.push_str(&format!(" extends {sn}"));
                    }
                }
            }
            None => base.push_str(" extends [null prototype]"),
        }
        return format!("[{base}]");
    }
    let mut ty = "Function".to_string();
    if is_gen {
        ty = format!("Generator{ty}");
    }
    if is_async {
        ty = format!("Async{ty}");
    }
    let mut base = format!("[{ty}");
    if constructor.is_none() {
        base.push_str(" (null prototype)");
    }
    let name = Vm::func_name(f);
    if name.is_empty() {
        base.push_str(" (anonymous)");
    } else {
        base.push_str(&format!(": {name}"));
    }
    base.push(']');
    if let Some(c) = constructor {
        if *c != ty && c != "Function" {
            base.push_str(&format!(" {c}"));
        }
    }
    if !tag.is_empty() && constructor.as_deref() != Some(tag) {
        base.push_str(&format!(" [{tag}]"));
    }
    base
}

fn error_stack_string(vm: &mut Vm, e: &Value) -> JsResult<String> {
    let st = vm.get_str(e, "stack")?;
    if let Value::Str(s) = &st {
        if !s.is_empty() {
            return Ok(s.to_string());
        }
    }
    match e {
        Value::Obj(o) => vm.error_header(o),
        _ => Ok(String::new()),
    }
}

fn identical_sequence_range(a: &[String], b: &[String]) -> (usize, usize) {
    if a.len() > 3 {
        for i in 0..a.len() - 3 {
            if let Some(pos) = b.iter().position(|x| *x == a[i]) {
                let rest = b.len() - pos;
                if rest > 3 {
                    let mut len = 1;
                    let max_len = (a.len() - i).min(rest);
                    while max_len > len && a[i + len] == b[pos + len] {
                        len += 1;
                    }
                    if len > 3 {
                        return (len, i);
                    }
                }
            }
        }
    }
    (0, 0)
}

fn format_error(
    vm: &mut Vm,
    ctx: &Ctx,
    e: &Obj,
    constructor: &Option<String>,
    tag: &str,
    keys: &mut Vec<Key>,
) -> JsResult<String> {
    let ev = Value::Obj(e.clone());
    let name_v = vm.get_str(&ev, "name")?;
    let name = if name_v.is_nullish() {
        "Error".to_string()
    } else {
        vm.to_str(&name_v)?
    };
    let mut stack = error_stack_string(vm, &ev)?;
    // removeDuplicateErrorKeys
    for k in ["name", "message", "stack"] {
        if let Some(i) = keys.iter().position(|x| x.as_str() == Some(k)) {
            let val = vm.get_str(&ev, k)?;
            let vs = if let Value::Str(s) = &val {
                s.to_string()
            } else {
                vm.to_str(&val).unwrap_or_default()
            };
            if stack.contains(&vs) {
                keys.remove(i);
            }
        }
    }
    if vm.has_property(e, &Key::str("cause"))? && !keys.iter().any(|k| k.as_str() == Some("cause"))
    {
        keys.push(Key::str("cause"));
    }
    let errs = vm.get_str(&ev, "errors")?;
    if matches!(&errs, Value::Obj(o) if o.is_array())
        && !keys.iter().any(|k| k.as_str() == Some("errors"))
    {
        keys.push(Key::str("errors"));
    }
    // improveStack
    let len = name.len();
    if constructor.is_none()
        || (name.ends_with("Error")
            && stack.starts_with(&name)
            && (stack.len() == len
                || stack[len..].starts_with(':')
                || stack[len..].starts_with('\n')))
    {
        let fallback = "Error";
        let prefix = get_prefix(constructor, tag, fallback, "");
        let prefix = &prefix[..prefix.len() - 1];
        if name != prefix {
            if prefix.contains(&name) {
                if len == 0 {
                    stack = format!("{prefix}: {stack}");
                } else {
                    stack = format!("{prefix}{}", &stack[len..]);
                }
            } else {
                stack = format!("{prefix} [{name}]{}", &stack[len..]);
            }
        }
    }
    let msg = vm.get_str(&ev, "message")?;
    let msg_s = if let Value::Str(s) = &msg {
        s.to_string()
    } else {
        String::new()
    };
    let pos = if msg_s.is_empty() {
        None
    } else {
        stack.find(&msg_s).map(|p| p + msg_s.len())
    };
    let search_from = pos.unwrap_or(0);
    let stack_start = stack[search_from.min(stack.len())..]
        .find("\n    at")
        .map(|x| x + search_from);
    match stack_start {
        None => {
            stack = format!("[{stack}]");
        }
        Some(ss) => {
            let head = stack[..ss].to_string();
            let mut frames: Vec<String> =
                stack[ss + 1..].split('\n').map(|s| s.to_string()).collect();
            // Frames identical to the cause's.
            let cause = vm.get_str(&ev, "cause")?;
            if let Value::Obj(co) = &cause {
                if matches!(co.borrow().kind, Kind::Error(_)) {
                    let cs = error_stack_string(vm, &cause)?;
                    if let Some(cst) = cs.find("\n    at") {
                        let cframes: Vec<String> =
                            cs[cst + 1..].split('\n').map(|s| s.to_string()).collect();
                        let (l, off) = identical_sequence_range(&frames, &cframes);
                        if l > 0 {
                            let skipped = l - 2;
                            let m =
                                format!("    ... {skipped} lines matching cause stack trace ...");
                            frames.splice(off + 1..off + 1 + skipped, [m]);
                        }
                    }
                }
            }
            stack = format!("{head}\n{}", frames.join("\n"));
        }
    }
    if ctx.indent != 0 {
        stack = stack.replace('\n', &format!("\n{}", " ".repeat(ctx.indent)));
    }
    Ok(stack)
}

fn format_raw(
    vm: &mut Vm,
    ctx: &mut Ctx,
    o: &Obj,
    recurse: usize,
    _typed: bool,
) -> JsResult<String> {
    let ov = Value::Obj(o.clone());
    let constructor = constructor_name(vm, o);
    let tag_v = vm.get(&ov, &Key::Sym(vm.syms.to_string_tag.clone()))?;
    let mut tag = match &tag_v {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    };
    if !tag.is_empty() {
        // Only list non-enumerable / inherited tags.
        let own = vm.get_own(o, &Key::Sym(vm.syms.to_string_tag.clone()))?;
        if let Some(p) = own {
            if p.enumerable() || ctx.o.show_hidden {
                tag = String::new();
            }
        }
    }
    let mut base = String::new();
    let mut braces = ["{".to_string(), "}".to_string()];
    let mut extras = Extras::Object;
    let mut fmt = Fmt::Empty;
    let mut keys: Vec<Key>;
    enum K {
        Array(usize),
        Typed(usize, &'static str),
        Set(usize),
        Map(usize),
        MapIter(bool, bool),
        Func,
        Error,
        RegExp,
        Date(f64),
        Promise,
        WeakSet,
        WeakMap,
        Arguments,
        Boxed(Value),
        ArrayBuffer(usize),
        Generator,
        Other,
    }
    let k = {
        let d = o.borrow();
        match &d.kind {
            Kind::Array(v) => K::Array(v.len()),
            Kind::TypedArray { kind, len, .. } => {
                K::Typed(*len, crate::builtins::typed::kind_name(*kind))
            }
            Kind::Set(m) => K::Set(m.live),
            Kind::Map(m) => K::Map(m.live),
            Kind::MapIter { target, kind, .. } => K::MapIter(
                matches!(target.borrow().kind, Kind::Set(_)),
                *kind == IterKind::Entries,
            ),
            Kind::Function(_) => K::Func,
            Kind::Error(_) => K::Error,
            Kind::RegExp(_) => K::RegExp,
            Kind::Date(t) => K::Date(*t),
            Kind::Promise(_) => K::Promise,
            Kind::WeakSet(_) => K::WeakSet,
            Kind::WeakMap(_) => K::WeakMap,
            Kind::Arguments => K::Arguments,
            Kind::Number(n) => K::Boxed(Value::Num(*n)),
            Kind::String(s) => K::Boxed(Value::Str(s.clone())),
            Kind::Boolean(b) => K::Boxed(Value::Bool(*b)),
            Kind::Symbol(s) => K::Boxed(Value::Sym(s.clone())),
            Kind::BigInt(b) => K::Boxed(Value::BigInt(b.clone())),
            Kind::ArrayBuffer(b) => K::ArrayBuffer(b.borrow().len()),
            Kind::Generator(_) => K::Generator,
            _ => K::Other,
        }
    };
    match k {
        K::Array(len) => {
            let prefix = if constructor.as_deref() != Some("Array") || !tag.is_empty() {
                get_prefix(&constructor, &tag, "Array", &format!("({len})"))
            } else {
                String::new()
            };
            keys = get_keys(vm, o, true, ctx.o.show_hidden)?;
            braces = [format!("{prefix}["), "]".into()];
            if len == 0 && keys.is_empty() {
                return Ok(format!("{}]", braces[0]));
            }
            extras = Extras::Array;
            fmt = Fmt::Array;
        }
        K::Typed(len, fallback) => {
            keys = get_keys(vm, o, true, false)?;
            let prefix = get_prefix(&constructor, &tag, fallback, &format!("({len})"));
            braces = [format!("{prefix}["), "]".into()];
            if len == 0 && keys.is_empty() {
                return Ok(format!("{}]", braces[0]));
            }
            // Buffers print as <Buffer ..>
            if constructor.as_deref() == Some("Buffer") {
                let bytes = vm.typed_bytes(o).unwrap_or_default();
                let mut s = String::from("<Buffer");
                for b in bytes.iter().take(50) {
                    s.push_str(&format!(" {b:02x}"));
                }
                if bytes.len() > 50 {
                    s.push_str(&format!(" ... {} more bytes", bytes.len() - 50));
                }
                s.push('>');
                return Ok(s);
            }
            extras = Extras::Array;
            fmt = Fmt::Typed;
        }
        K::Set(size) => {
            keys = get_keys(vm, o, false, ctx.o.show_hidden)?;
            let prefix = get_prefix(&constructor, &tag, "Set", &format!("({size})"));
            if size == 0 && keys.is_empty() {
                return Ok(format!("{prefix}{{}}"));
            }
            braces = [format!("{prefix}{{"), "}".into()];
            fmt = Fmt::Set;
        }
        K::Map(size) => {
            keys = get_keys(vm, o, false, ctx.o.show_hidden)?;
            let prefix = get_prefix(&constructor, &tag, "Map", &format!("({size})"));
            if size == 0 && keys.is_empty() {
                return Ok(format!("{prefix}{{}}"));
            }
            braces = [format!("{prefix}{{"), "}".into()];
            fmt = Fmt::Map;
        }
        K::MapIter(is_set, entries) => {
            keys = get_keys(vm, o, false, ctx.o.show_hidden)?;
            let t = if is_set { "Set" } else { "Map" };
            let t2 = if entries { "Entries" } else { "Iterator" };
            let tg = if tag.is_empty() {
                format!("{t} {t2}")
            } else {
                tag.clone()
            };
            let tg = if entries { format!("{t} Entries") } else { tg };
            braces = [format!("[{tg}] {{"), "}".into()];
            fmt = Fmt::MapIter(entries);
        }
        _ => {
            keys = get_keys(vm, o, false, ctx.o.show_hidden)?;
            match k {
                K::Func => {
                    base = function_base(vm, o, &constructor, &tag);
                    if keys.is_empty() {
                        return Ok(base);
                    }
                }
                K::Arguments => {
                    braces[0] = "[Arguments] {".into();
                    if keys.is_empty() {
                        return Ok("[Arguments] {}".into());
                    }
                    fmt = Fmt::Arguments;
                }
                K::RegExp => {
                    let f = vm.get_str(&ov, "toString")?;
                    let s = vm.call(&f, ov.clone(), vec![])?;
                    base = vm.to_str(&s)?;
                    let prefix = get_prefix(&constructor, &tag, "RegExp", "");
                    if prefix != "RegExp " {
                        base = format!("{prefix}{base}");
                    }
                    let too_deep = matches!(ctx.o.depth, Some(d) if recurse as f64 > d);
                    if keys.is_empty() || too_deep {
                        return Ok(base);
                    }
                }
                K::Date(t) => {
                    base = if t.is_nan() {
                        "Invalid Date".into()
                    } else {
                        crate::builtins::date::iso_string(t)
                    };
                    let prefix = get_prefix(&constructor, &tag, "Date", "");
                    if prefix != "Date " {
                        base = format!("{prefix}{base}");
                    }
                    if keys.is_empty() {
                        return Ok(base);
                    }
                }
                K::Error => {
                    base = format_error(vm, ctx, o, &constructor, &tag, &mut keys)?;
                    if keys.is_empty() {
                        return Ok(base);
                    }
                }
                K::Promise => {
                    braces[0] = format!("{}{{", get_prefix(&constructor, &tag, "Promise", ""));
                    fmt = Fmt::Promise;
                }
                K::WeakSet => {
                    braces[0] = format!("{}{{", get_prefix(&constructor, &tag, "WeakSet", ""));
                    fmt = Fmt::Weak;
                }
                K::WeakMap => {
                    braces[0] = format!("{}{{", get_prefix(&constructor, &tag, "WeakMap", ""));
                    fmt = Fmt::Weak;
                }
                K::Boxed(p) => {
                    let (ty, inner) = match &p {
                        Value::Num(n) => ("Number", format_number(*n)),
                        Value::Str(s) => ("String", str_escape(s)),
                        Value::Bool(b) => ("Boolean", b.to_string()),
                        Value::Sym(s) => ("Symbol", crate::builtins::symbol::symbol_descriptive(s)),
                        Value::BigInt(b) => ("BigInt", format!("{}n", b.to_str_radix(10))),
                        _ => ("Object", String::new()),
                    };
                    if let Value::Str(s) = &p {
                        // Index keys of String objects are not listed.
                        let n = s.len16();
                        keys.retain(|k| k.array_index().map(|i| i as usize >= n).unwrap_or(true));
                    }
                    base = format!("[{ty}");
                    if constructor.as_deref() != Some(ty) {
                        match &constructor {
                            None => base.push_str(" (null prototype)"),
                            Some(c) => base.push_str(&format!(" ({c})")),
                        }
                    }
                    base.push_str(&format!(": {inner}]"));
                    if !tag.is_empty() && constructor.as_deref() != Some(tag.as_str()) {
                        base.push_str(&format!(" [{tag}]"));
                    }
                    if keys.is_empty() {
                        return Ok(base);
                    }
                }
                K::ArrayBuffer(n) => {
                    let bytes = vm.typed_bytes(o).unwrap_or_default();
                    let hex: Vec<String> =
                        bytes.iter().take(50).map(|b| format!("{b:02x}")).collect();
                    let mut contents = format!("<{}", hex.join(" "));
                    if bytes.len() > 50 {
                        contents.push_str(&format!(" ... {} more bytes", bytes.len() - 50));
                    }
                    contents.push('>');
                    let prefix = get_prefix(&constructor, &tag, "ArrayBuffer", "");
                    return Ok(format!(
                        "{prefix}{{ [Uint8Contents]: {contents}, [byteLength]: {n} }}"
                    ));
                }
                K::Generator => {
                    braces[0] = format!("{}{{", get_prefix(&constructor, &tag, "Object", ""));
                    if keys.is_empty() {
                        return Ok(format!("{}}}", braces[0]));
                    }
                }
                _ => {
                    if constructor.as_deref() == Some("Object") {
                        if !tag.is_empty() {
                            braces[0] =
                                format!("{}{{", get_prefix(&constructor, &tag, "Object", ""));
                        }
                        if keys.is_empty() {
                            return Ok(format!("{}}}", braces[0]));
                        }
                    } else {
                        if keys.is_empty() {
                            return Ok(format!(
                                "{}{{}}",
                                get_prefix(&constructor, &tag, "Object", "")
                            ));
                        }
                        braces[0] = format!("{}{{", get_prefix(&constructor, &tag, "Object", ""));
                    }
                }
            }
        }
    }
    if let Some(d) = ctx.o.depth {
        if recurse as f64 > d {
            let p = get_prefix(
                &constructor,
                &tag,
                if matches!(fmt, Fmt::Array) {
                    "Array"
                } else {
                    "Object"
                },
                "",
            );
            let name = &p[..p.len() - 1];
            return Ok(if constructor.is_some() {
                format!("[{name}]")
            } else {
                name.to_string()
            });
        }
    }
    let recurse = recurse + 1;
    ctx.seen.push(o.clone());
    ctx.current_depth = recurse;
    let mut output: Vec<String> = vec![];
    match fmt {
        Fmt::Array => format_array(vm, ctx, o, recurse, &mut output)?,
        Fmt::Typed => {
            let n = match &o.borrow().kind {
                Kind::TypedArray { len, .. } => *len,
                _ => 0,
            };
            let max = ctx.o.max_array_length.min(n);
            for i in 0..max {
                let v = vm.typed_get(o, i).unwrap_or(Value::Undefined);
                output.push(format_primitive(ctx, &v));
            }
            if n > max {
                output.push(remaining_text(n - max));
            }
        }
        Fmt::Set => {
            let items: Vec<Value> = match &o.borrow().kind {
                Kind::Set(m) => m.entries.iter().flatten().map(|(k, _)| k.clone()).collect(),
                _ => vec![],
            };
            ctx.indent += 2;
            let max = ctx.o.max_array_length.min(items.len());
            for it in items.iter().take(max) {
                output.push(format_value(vm, ctx, it, recurse, false)?);
            }
            if items.len() > max {
                output.push(remaining_text(items.len() - max));
            }
            ctx.indent -= 2;
        }
        Fmt::Map => {
            let items: Vec<(Value, Value)> = match &o.borrow().kind {
                Kind::Map(m) => m.entries.iter().flatten().cloned().collect(),
                _ => vec![],
            };
            ctx.indent += 2;
            let max = ctx.o.max_array_length.min(items.len());
            for (k, v) in items.iter().take(max) {
                let ks = format_value(vm, ctx, k, recurse, false)?;
                let vs = format_value(vm, ctx, v, recurse, false)?;
                output.push(format!("{ks} => {vs}"));
            }
            if items.len() > max {
                output.push(remaining_text(items.len() - max));
            }
            ctx.indent -= 2;
        }
        Fmt::MapIter(entries) => {
            let (target, index, kind) = match &o.borrow().kind {
                Kind::MapIter {
                    target,
                    index,
                    kind,
                    ..
                } => (target.clone(), *index, *kind),
                _ => unreachable!(),
            };
            let items: Vec<(Value, Value)> = match &target.borrow().kind {
                Kind::Map(m) | Kind::Set(m) => {
                    m.entries.iter().skip(index).flatten().cloned().collect()
                }
                _ => vec![],
            };
            let is_set = matches!(target.borrow().kind, Kind::Set(_));
            ctx.indent += 2;
            for (k, v) in items {
                let v = if is_set { k.clone() } else { v };
                let s = if entries {
                    let a = vm.arr(vec![k, v]);
                    format_value(vm, ctx, &a, recurse, false)?
                } else if kind == IterKind::Keys {
                    format_value(vm, ctx, &k, recurse, false)?
                } else {
                    format_value(vm, ctx, &v, recurse, false)?
                };
                output.push(s);
            }
            ctx.indent -= 2;
        }
        Fmt::Promise => {
            let (st, val) = vm.promise_state(o).unwrap();
            match st {
                PromiseState::Pending => output.push("<pending>".into()),
                _ => {
                    ctx.indent += 2;
                    let s = format_value(vm, ctx, &val, recurse, false)?;
                    ctx.indent -= 2;
                    output.push(if st == PromiseState::Rejected {
                        format!("<rejected> {s}")
                    } else {
                        s
                    });
                }
            }
        }
        Fmt::Weak => output.push("<items unknown>".into()),
        Fmt::Arguments | Fmt::Empty => {}
    }
    for key in &keys {
        let s = format_property(vm, ctx, o, recurse, key, extras)?;
        output.push(s);
    }
    if let Some((_, idx)) = ctx.circular.iter().find(|(a, _)| *a == o.addr()) {
        let reference = format!("<ref *{idx}>");
        if ctx.o.compact != COMPACT_TRUE {
            base = if base.is_empty() {
                reference
            } else {
                format!("{reference} {base}")
            };
        } else {
            braces[0] = format!("{reference} {}", braces[0]);
        }
    }
    ctx.seen.pop();
    if ctx.o.sorted && extras == Extras::Object {
        output.sort();
    }
    Ok(reduce_to_single_string(
        ctx,
        output,
        &base,
        &braces,
        extras,
        recurse,
        Some(o),
        vm,
    ))
}

fn remaining_text(n: usize) -> String {
    format!("... {n} more item{}", if n > 1 { "s" } else { "" })
}

fn format_array(
    vm: &mut Vm,
    ctx: &mut Ctx,
    o: &Obj,
    recurse: usize,
    output: &mut Vec<String>,
) -> JsResult<()> {
    let items: Vec<Value> = match &o.borrow().kind {
        Kind::Array(v) => v.clone(),
        _ => vec![],
    };
    let len = items.len();
    let max = ctx.o.max_array_length.min(len);
    let mut i = 0;
    let mut shown = 0;
    while i < len && shown < max {
        if let Value::Empty = items[i] {
            let mut j = i;
            while j < len && matches!(items[j], Value::Empty) {
                j += 1;
            }
            let n = j - i;
            output.push(format!("<{n} empty item{}>", if n > 1 { "s" } else { "" }));
            i = j;
            shown += 1;
            continue;
        }
        ctx.indent += 2;
        let s = format_value(vm, ctx, &items[i], recurse, false)?;
        ctx.indent -= 2;
        output.push(s);
        i += 1;
        shown += 1;
    }
    if i < len {
        output.push(remaining_text(len - i));
    }
    Ok(())
}

fn format_property(
    vm: &mut Vm,
    ctx: &mut Ctx,
    o: &Obj,
    recurse: usize,
    key: &Key,
    extras: Extras,
) -> JsResult<String> {
    let desc = vm.get_own(o, key)?;
    let (str_v, enumerable) = match desc {
        Some(Prop {
            slot: Slot::Data(v),
            flags,
        }) => {
            ctx.indent += 2;
            let s = format_value(vm, ctx, &v, recurse, false)?;
            ctx.indent -= 2;
            (s, flags & ENUMERABLE != 0)
        }
        Some(Prop {
            slot: Slot::Accessor(g, s),
            flags,
        }) => {
            let label = match (&g, &s) {
                (Some(_), Some(_)) => "[Getter/Setter]",
                (Some(_), None) => "[Getter]",
                (None, Some(_)) => "[Setter]",
                _ => "undefined",
            };
            (label.to_string(), flags & ENUMERABLE != 0)
        }
        None => {
            // Inherited (e.g. error cause via prototype).
            let v = vm.get(&Value::Obj(o.clone()), key)?;
            ctx.indent += 2;
            let s = format_value(vm, ctx, &v, recurse, false)?;
            ctx.indent -= 2;
            (s, true)
        }
    };
    if extras == Extras::Array && key.array_index().is_some() {
        return Ok(str_v);
    }
    let name = match key {
        Key::Sym(s) => crate::builtins::symbol::symbol_descriptive(s),
        Key::Str(k) => {
            if k.as_str() == "__proto__" {
                "['__proto__']".to_string()
            } else if !enumerable {
                format!("[{}]", k.as_str())
            } else if is_ident_key(k) {
                k.to_string()
            } else {
                str_escape(k)
            }
        }
    };
    Ok(format!("{name}: {str_v}"))
}

fn is_below_break_length(ctx: &Ctx, output: &[String], start: usize, base: &str) -> bool {
    let mut total = output.len() + start;
    if (total + output.len()) as f64 > ctx.o.break_length {
        return false;
    }
    for s in output {
        total += len16(s);
        if total as f64 > ctx.o.break_length {
            return false;
        }
    }
    base.is_empty() || !base.contains('\n')
}

#[allow(clippy::too_many_arguments)]
fn reduce_to_single_string(
    ctx: &mut Ctx,
    mut output: Vec<String>,
    base: &str,
    braces: &[String; 2],
    extras: Extras,
    recurse: usize,
    value: Option<&Obj>,
    vm: &mut Vm,
) -> String {
    if ctx.o.compact != COMPACT_TRUE {
        if ctx.o.compact >= 1 {
            let entries = output.len();
            if extras == Extras::Array && entries > 6 {
                output = group_array_elements(ctx, output, value, vm);
            }
            if ctx.current_depth - recurse < ctx.o.compact && entries == output.len() {
                let start = output.len() + ctx.indent + braces[0].len() + base.len() + 10;
                if is_below_break_length(ctx, &output, start, base) {
                    let joined = output.join(", ");
                    if !joined.contains('\n') {
                        let b = if base.is_empty() {
                            String::new()
                        } else {
                            format!("{base} ")
                        };
                        return format!("{b}{} {joined} {}", braces[0], braces[1]);
                    }
                }
            }
        }
        let indentation = format!("\n{}", " ".repeat(ctx.indent));
        let b = if base.is_empty() {
            String::new()
        } else {
            format!("{base} ")
        };
        return format!(
            "{b}{}{indentation}  {}{indentation}{}",
            braces[0],
            output.join(&format!(",{indentation}  ")),
            braces[1]
        );
    }
    // compact === true
    let start = output.len() + ctx.indent + braces[0].len() + base.len() + 10;
    if is_below_break_length(ctx, &output, start, base) {
        let b = if base.is_empty() {
            String::new()
        } else {
            format!("{base} ")
        };
        return format!("{b}{} {} {}", braces[0], output.join(", "), braces[1]);
    }
    let indentation = format!("\n{}", " ".repeat(ctx.indent));
    let ln = if base.is_empty() && len16(&braces[0]) == 1 {
        " ".to_string()
    } else {
        format!("{base}{indentation}  ")
    };
    format!(
        "{}{ln}{} {}",
        braces[0],
        output.join(&format!(",{indentation}  ")),
        braces[1]
    )
}

fn group_array_elements(
    ctx: &Ctx,
    output: Vec<String>,
    value: Option<&Obj>,
    vm: &mut Vm,
) -> Vec<String> {
    let mut total_length = 0usize;
    let mut max_length = 0usize;
    let mut output_length = output.len();
    if let Some(o) = value {
        let n = match &o.borrow().kind {
            Kind::Array(v) => v.len(),
            Kind::TypedArray { len, .. } => *len,
            _ => 0,
        };
        if ctx.o.max_array_length < n && output_length > 0 {
            output_length -= 1;
        }
    }
    let separator_space = 2;
    let mut data_len = vec![0usize; output_length];
    for i in 0..output_length {
        let len = len16(&output[i]);
        data_len[i] = len;
        total_length += len + separator_space;
        if max_length < len {
            max_length = len;
        }
    }
    let actual_max = max_length + separator_space;
    if (actual_max * 3 + ctx.indent) as f64 <= ctx.o.break_length - 1.0 + 1.0
        && ((actual_max * 3 + ctx.indent) as f64) < ctx.o.break_length
        && (total_length as f64 / actual_max as f64 > 5.0 || max_length <= 6)
    {
        let approx_char_heights = 2.5;
        let average_bias = (actual_max as f64 - total_length as f64 / output.len() as f64).sqrt();
        let biased_max = (actual_max as f64 - 3.0 - average_bias).max(1.0);
        let columns = ((approx_char_heights * biased_max * output_length as f64).sqrt()
            / biased_max)
            .round()
            .min(((ctx.o.break_length - ctx.indent as f64) / actual_max as f64).floor())
            .min((ctx.o.compact * 4) as f64)
            .min(15.0);
        if columns <= 1.0 {
            return output;
        }
        let columns = columns as usize;
        let mut tmp = vec![];
        let mut max_line_length = vec![];
        for i in 0..columns {
            let mut line_length = 0;
            let mut j = i;
            while j < output.len() {
                if j < data_len.len() && data_len[j] > line_length {
                    line_length = data_len[j];
                }
                j += columns;
            }
            max_line_length.push(line_length + separator_space);
        }
        // Numbers are right-aligned.
        let mut pad_start = true;
        if let Some(o) = value {
            let items: Vec<Value> = match &o.borrow().kind {
                Kind::Array(v) => v.clone(),
                _ => vec![],
            };
            let is_typed = matches!(o.borrow().kind, Kind::TypedArray { .. });
            if !is_typed {
                for i in 0..output.len() {
                    match items.get(i) {
                        Some(Value::Num(_)) | Some(Value::BigInt(_)) => {}
                        _ => {
                            pad_start = false;
                            break;
                        }
                    }
                }
            }
        }
        let _ = vm;
        let mut i = 0;
        while i < output_length {
            let max = (i + columns).min(output_length);
            let mut s = String::new();
            let mut j = i;
            while j < max - 1 {
                let cell = format!("{}, ", output[j]);
                let padding = max_line_length[j - i];
                s.push_str(&pad(&cell, padding, pad_start));
                j += 1;
            }
            if pad_start {
                let padding = max_line_length[j - i] - separator_space;
                s.push_str(&pad(&output[j], padding, true));
            } else {
                s.push_str(&output[j]);
            }
            tmp.push(s);
            i += columns;
        }
        if output_length < output.len() {
            tmp.push(output[output_length].clone());
        }
        return tmp;
    }
    output
}

fn pad(s: &str, width: usize, start: bool) -> String {
    let l = len16(s);
    if l >= width {
        return s.to_string();
    }
    let fill = " ".repeat(width - l);
    if start {
        format!("{fill}{s}")
    } else {
        format!("{s}{fill}")
    }
}

/// Does `v` use a built-in toString (for `%s`)?
fn has_builtin_to_string(vm: &mut Vm, o: &Obj) -> bool {
    // An own or class-defined toString counts as user-defined.
    let mut cur = Some(o.clone());
    while let Some(c) = cur {
        if c.ptr_eq(&vm.intr.object_proto) {
            return true;
        }
        if let Some(Prop {
            slot: Slot::Data(Value::Obj(f)),
            ..
        }) = c.borrow().props.get_str("toString").cloned()
        {
            let native = matches!(&f.borrow().kind, Kind::Function(fd) if matches!(fd.imp, FuncImpl::Native{..}));
            return native;
        }
        cur = c.proto();
    }
    true
}

impl<'h> Vm<'h> {
    /// util.format / console.log formatting.
    pub fn format_args(&mut self, args: &[Value]) -> JsResult<String> {
        let opts = Opts::default();
        let mut s = String::new();
        let mut a = 0;
        let mut join = "";
        if let Some(Value::Str(first)) = args.first() {
            if args.len() == 1 {
                return Ok(first.to_string());
            }
            let f: Vec<char> = first.chars().collect();
            let mut last = 0usize;
            let mut i = 0;
            let slice = |from: usize, to: usize| -> String { f[from..to].iter().collect() };
            while i + 1 < f.len() {
                if f[i] == '%' {
                    i += 1;
                    let next = f[i];
                    if a + 1 != args.len() {
                        let temp: Option<String> = match next {
                            's' => {
                                a += 1;
                                let v = args[a].clone();
                                Some(match &v {
                                    Value::Num(n) => format_number(*n),
                                    Value::BigInt(b) => format!("{}n", b.to_str_radix(10)),
                                    Value::Obj(o) if has_builtin_to_string(self, o) => {
                                        let o2 = Opts {
                                            depth: Some(0.0),
                                            ..opts.clone()
                                        };
                                        self.inspect(&v, &o2)?
                                    }
                                    Value::Sym(sy) => {
                                        crate::builtins::symbol::symbol_descriptive(sy)
                                    }
                                    _ => self.to_str(&v)?,
                                })
                            }
                            'j' => {
                                a += 1;
                                let v = args[a].clone();
                                Some(
                                    match crate::builtins::json::stringify_value(
                                        self,
                                        v,
                                        Value::Undefined,
                                        Value::Undefined,
                                    ) {
                                        Ok(Value::Str(s)) => s.to_string(),
                                        Ok(_) => "undefined".into(),
                                        Err(Ctl::Throw(e)) => {
                                            let m = self
                                                .get_str(&e, "message")
                                                .unwrap_or(Value::Undefined);
                                            let ms = self.to_str(&m).unwrap_or_default();
                                            if ms.contains("circular") {
                                                "[Circular]".into()
                                            } else {
                                                return Err(Ctl::Throw(e));
                                            }
                                        }
                                        Err(o) => return Err(o),
                                    },
                                )
                            }
                            'd' | 'i' | 'f' => {
                                a += 1;
                                let v = args[a].clone();
                                Some(match &v {
                                    Value::BigInt(b) if next != 'f' => {
                                        format!("{}n", b.to_str_radix(10))
                                    }
                                    Value::Sym(_) => "NaN".into(),
                                    Value::Obj(_) if next == 'd' => {
                                        let n = self.to_number(&v).unwrap_or(f64::NAN);
                                        format_number(n)
                                    }
                                    _ => {
                                        let n = if next == 'd' {
                                            self.to_number(&v)?
                                        } else {
                                            let st = self.to_str(&v)?;
                                            if next == 'i' {
                                                crate::numconv::parse_int(&st, 0)
                                            } else {
                                                crate::numconv::parse_float(&st)
                                            }
                                        };
                                        format_number(n)
                                    }
                                })
                            }
                            'O' => {
                                a += 1;
                                let v = args[a].clone();
                                Some(self.inspect(&v, &opts)?)
                            }
                            'o' => {
                                a += 1;
                                let v = args[a].clone();
                                let o2 = Opts {
                                    show_hidden: true,
                                    depth: Some(4.0),
                                    ..opts.clone()
                                };
                                Some(self.inspect(&v, &o2)?)
                            }
                            'c' => {
                                a += 1;
                                Some(String::new())
                            }
                            '%' => {
                                s.push_str(&slice(last, i));
                                last = i + 1;
                                i += 1;
                                continue;
                            }
                            _ => {
                                i += 1;
                                continue;
                            }
                        };
                        if let Some(t) = temp {
                            if last != i - 1 {
                                s.push_str(&slice(last, i - 1));
                            }
                            s.push_str(&t);
                            last = i + 1;
                        }
                    } else if next == '%' {
                        s.push_str(&slice(last, i));
                        last = i + 1;
                    }
                }
                i += 1;
            }
            if last != 0 {
                a += 1;
                join = " ";
                if last < f.len() {
                    s.push_str(&slice(last, f.len()));
                }
            }
        }
        while a < args.len() {
            let v = args[a].clone();
            s.push_str(join);
            match &v {
                Value::Str(x) => s.push_str(x),
                _ => s.push_str(&self.inspect(&v, &opts)?),
            }
            join = " ";
            a += 1;
        }
        Ok(s)
    }

    /// Parses an options object passed to util.inspect.
    pub fn inspect_opts_from(&mut self, v: &Value) -> JsResult<Opts> {
        let mut o = Opts::default();
        if let Value::Obj(_) = v {
            let d = self.get_str(v, "depth")?;
            match d {
                Value::Undefined => {}
                Value::Null => o.depth = None,
                other => {
                    let n = self.to_number(&other)?;
                    o.depth = if n.is_infinite() { None } else { Some(n) };
                }
            }
            let b = self.get_str(v, "breakLength")?;
            if !b.is_undefined() {
                o.break_length = self.to_number(&b)?;
            }
            let c = self.get_str(v, "compact")?;
            match c {
                Value::Bool(true) => o.compact = COMPACT_TRUE,
                Value::Bool(false) => o.compact = 0,
                Value::Num(n) => o.compact = n.max(0.0) as usize,
                _ => {}
            }
            let sh = self.get_str(v, "showHidden")?;
            if !sh.is_undefined() {
                o.show_hidden = sh.truthy();
            }
            let m = self.get_str(v, "maxArrayLength")?;
            match m {
                Value::Undefined => {}
                Value::Null => o.max_array_length = usize::MAX,
                other => {
                    let n = self.to_number(&other)?;
                    o.max_array_length = if n.is_infinite() {
                        usize::MAX
                    } else {
                        n.max(0.0) as usize
                    };
                }
            }
            let ms = self.get_str(v, "maxStringLength")?;
            match ms {
                Value::Undefined => {}
                Value::Null => o.max_string_length = usize::MAX,
                other => {
                    let n = self.to_number(&other)?;
                    o.max_string_length = if n.is_infinite() {
                        usize::MAX
                    } else {
                        n.max(0.0) as usize
                    };
                }
            }
            let so = self.get_str(v, "sorted")?;
            if !so.is_undefined() {
                o.sorted = so.truthy();
            }
            let ci = self.get_str(v, "customInspect")?;
            if !ci.is_undefined() {
                o.custom_inspect = ci.truthy();
            }
        }
        Ok(o)
    }
}
