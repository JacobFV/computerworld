//! `String` and `String.prototype` (UTF-16 semantics).

use super::*;
use crate::numconv::is_js_whitespace;
use crate::value::*;
use crate::vm::Vm;

fn this_str(vm: &mut Vm, a: &Args, method: &str) -> JsResult<JsStr> {
    match &a.this {
        Value::Str(s) => Ok(s.clone()),
        Value::Undefined | Value::Null => Err(vm.type_error(format!(
            "String.prototype.{method} called on null or undefined"
        ))),
        v => vm.to_string(v),
    }
}

fn string_ctor(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = if a.args.is_empty() {
        JsStr::new("")
    } else {
        match (&a.args[0], &a.new_target) {
            (Value::Sym(sym), None) => JsStr::new(format!(
                "Symbol({})",
                sym.desc.as_ref().map(|d| d.to_string()).unwrap_or_default()
            )),
            (v, _) => vm.to_string(v)?,
        }
    };
    match &a.new_target {
        None => Ok(Value::Str(s)),
        Some(nt) => {
            let sp = vm.intr.string_proto.clone();
            let proto = vm.proto_from_ctor(nt, &sp)?;
            Ok(Value::Obj(vm.obj_with(Some(proto), Kind::String(s))))
        }
    }
}

fn from_char_code(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut u = vec![];
    for v in &a.args {
        u.push(vm.to_u32(v)? as u16);
    }
    Ok(Value::Str(JsStr::from_utf16(&u)))
}

fn from_code_point(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let mut s = String::new();
    for v in &a.args {
        let n = vm.to_number(v)?;
        if n.fract() != 0.0 || !(0.0..=1114111.0).contains(&n) {
            let d = vm.describe_for_error(v);
            return Err(vm.range_error(format!("Invalid code point {d}")));
        }
        s.push(char::from_u32(n as u32).unwrap_or('\u{fffd}'));
    }
    Ok(Value::string(s))
}

fn raw(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let strs = a.arg(0);
    let raw = vm.get_str(&strs, "raw")?;
    let n = vm.length_of(&raw)?;
    let mut s = String::new();
    for i in 0..n {
        let part = vm.get_index(&raw, i)?;
        s.push_str(&vm.to_string(&part)?);
        if i + 1 < n && i + 1 < a.args.len() {
            let sub = a.args[i + 1].clone();
            s.push_str(&vm.to_string(&sub)?);
        }
    }
    Ok(Value::string(s))
}

pub fn index_of_16(hay: &JsStr, needle: &JsStr, from: usize) -> Option<usize> {
    if hay.is_ascii() && needle.is_ascii() {
        if from > hay.len() {
            return if needle.is_empty() {
                Some(hay.len())
            } else {
                None
            };
        }
        return hay[from..].find(needle.as_str()).map(|i| i + from);
    }
    let h = hay.utf16();
    let n = needle.utf16();
    if n.is_empty() {
        return Some(from.min(h.len()));
    }
    if n.len() > h.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| h[i..i + n.len()] == n[..])
}

fn last_index_of_16(hay: &JsStr, needle: &JsStr, from: usize) -> Option<usize> {
    let h = hay.utf16();
    let n = needle.utf16();
    if n.len() > h.len() {
        return None;
    }
    let start = from.min(h.len() - n.len());
    (0..=start).rev().find(|&i| h[i..i + n.len()] == n[..])
}

fn char_at(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "charAt")?;
    let i = vm.to_integer(&a.arg(0))?;
    if i < 0.0 {
        return Ok(Value::str(""));
    }
    Ok(match s.code_unit(i as usize) {
        Some(u) => Value::Str(JsStr::from_utf16(&[u])),
        None => Value::str(""),
    })
}

fn char_code_at(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "charCodeAt")?;
    let i = vm.to_integer(&a.arg(0))?;
    if i < 0.0 {
        return Ok(Value::Num(f64::NAN));
    }
    Ok(match s.code_unit(i as usize) {
        Some(u) => Value::Num(u as f64),
        None => Value::Num(f64::NAN),
    })
}

fn code_point_at(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "codePointAt")?;
    let i = vm.to_integer(&a.arg(0))?;
    if i < 0.0 || i as usize >= s.len16() {
        return Ok(Value::Undefined);
    }
    let u = s.utf16();
    let i = i as usize;
    let c = u[i] as u32;
    if (0xd800..0xdc00).contains(&c) && i + 1 < u.len() {
        let d = u[i + 1] as u32;
        if (0xdc00..0xe000).contains(&d) {
            return Ok(Value::Num(
                (0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00)) as f64,
            ));
        }
    }
    Ok(Value::Num(c as f64))
}

fn at(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "at")?;
    let len = s.len16() as f64;
    let i = vm.to_integer(&a.arg(0))?;
    let k = if i < 0.0 { len + i } else { i };
    if k < 0.0 || k >= len {
        return Ok(Value::Undefined);
    }
    Ok(Value::Str(JsStr::from_utf16(&[s
        .code_unit(k as usize)
        .unwrap()])))
}

fn index_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "indexOf")?;
    let n = str_arg(vm, a, 0)?;
    let from = vm.to_integer(&a.arg(1))?.max(0.0) as usize;
    Ok(Value::Num(
        index_of_16(&s, &n, from.min(s.len16()))
            .map(|i| i as f64)
            .unwrap_or(-1.0),
    ))
}

fn last_index_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "lastIndexOf")?;
    let n = str_arg(vm, a, 0)?;
    let p = vm.to_number(&a.arg(1))?;
    let from = if p.is_nan() {
        usize::MAX
    } else {
        p.max(0.0) as usize
    };
    Ok(Value::Num(
        last_index_of_16(&s, &n, from)
            .map(|i| i as f64)
            .unwrap_or(-1.0),
    ))
}

fn is_regexp(v: &Value) -> bool {
    matches!(v, Value::Obj(o) if matches!(o.borrow().kind, Kind::RegExp(_)))
}

fn includes(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "includes")?;
    if is_regexp(&a.arg(0)) {
        return Err(vm.type_error(
            "First argument to String.prototype.includes must not be a regular expression",
        ));
    }
    let n = str_arg(vm, a, 0)?;
    let from = vm.to_integer(&a.arg(1))?.max(0.0) as usize;
    Ok(Value::Bool(
        index_of_16(&s, &n, from.min(s.len16())).is_some(),
    ))
}

fn starts_with(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "startsWith")?;
    if is_regexp(&a.arg(0)) {
        return Err(vm.type_error(
            "First argument to String.prototype.startsWith must not be a regular expression",
        ));
    }
    let n = str_arg(vm, a, 0)?;
    let from = vm.to_integer(&a.arg(1))?.max(0.0) as usize;
    if s.is_ascii() && n.is_ascii() {
        return Ok(Value::Bool(
            from <= s.len() && s[from..].starts_with(n.as_str()),
        ));
    }
    let h = s.utf16();
    let nn = n.utf16();
    Ok(Value::Bool(from <= h.len() && h[from..].starts_with(&nn)))
}

fn ends_with(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "endsWith")?;
    if is_regexp(&a.arg(0)) {
        return Err(vm.type_error(
            "First argument to String.prototype.endsWith must not be a regular expression",
        ));
    }
    let n = str_arg(vm, a, 0)?;
    let len = s.len16();
    let end = if a.arg(1).is_undefined() {
        len
    } else {
        (vm.to_integer(&a.arg(1))?.max(0.0) as usize).min(len)
    };
    let h = s.utf16();
    let nn = n.utf16();
    Ok(Value::Bool(h[..end].ends_with(&nn)))
}

fn slice(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "slice")?;
    let len = s.len16();
    let st = vm.rel_index(&a.arg(0), len, 0)?;
    let en = vm.rel_index(&a.arg(1), len, len)?;
    Ok(Value::Str(if st < en {
        s.slice16(st, en)
    } else {
        JsStr::new("")
    }))
}

fn substring(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "substring")?;
    let len = s.len16() as f64;
    let st = vm.to_integer(&a.arg(0))?.clamp(0.0, len) as usize;
    let en = if a.arg(1).is_undefined() {
        len as usize
    } else {
        vm.to_integer(&a.arg(1))?.clamp(0.0, len) as usize
    };
    let (x, y) = if st <= en { (st, en) } else { (en, st) };
    Ok(Value::Str(s.slice16(x, y)))
}

fn substr(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "substr")?;
    let len = s.len16();
    let st = vm.rel_index(&a.arg(0), len, 0)?;
    let n = if a.arg(1).is_undefined() {
        (len - st) as f64
    } else {
        vm.to_integer(&a.arg(1))?
    };
    let n = n.clamp(0.0, (len - st) as f64) as usize;
    Ok(Value::Str(s.slice16(st, st + n)))
}

fn to_upper(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "toUpperCase")?;
    Ok(Value::string(s.to_uppercase()))
}

fn to_lower(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "toLowerCase")?;
    Ok(Value::string(s.to_lowercase()))
}

fn trim(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "trim")?;
    Ok(Value::str(s.trim_matches(is_js_whitespace)))
}
fn trim_start(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "trimStart")?;
    Ok(Value::str(s.trim_start_matches(is_js_whitespace)))
}
fn trim_end(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "trimEnd")?;
    Ok(Value::str(s.trim_end_matches(is_js_whitespace)))
}

fn pad(vm: &mut Vm, a: &mut Args, start: bool) -> JsResult<Value> {
    let s = this_str(vm, a, if start { "padStart" } else { "padEnd" })?;
    let target = vm.to_length(&a.arg(0))?;
    let fill = if a.arg(1).is_undefined() {
        JsStr::new(" ")
    } else {
        str_arg(vm, a, 1)?
    };
    let len = s.len16();
    if target <= len || fill.is_empty() {
        return Ok(Value::Str(s));
    }
    if target > (1 << 29) {
        return Err(vm.range_error("Invalid string length"));
    }
    let need = target - len;
    let fu = fill.utf16();
    let mut padv: Vec<u16> = Vec::with_capacity(need);
    while padv.len() < need {
        padv.extend_from_slice(&fu);
    }
    padv.truncate(need);
    let pad = String::from_utf16_lossy(&padv);
    Ok(Value::string(if start {
        format!("{pad}{}", s.as_str())
    } else {
        format!("{}{pad}", s.as_str())
    }))
}

fn pad_start(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    pad(vm, a, true)
}
fn pad_end(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    pad(vm, a, false)
}

fn repeat(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "repeat")?;
    let n = vm.to_integer(&a.arg(0))?;
    if n < 0.0 || n.is_infinite() {
        let d = crate::numconv::number_to_string(n);
        return Err(vm.range_error(format!("Invalid count value: {d}")));
    }
    if s.len() as f64 * n > (1u64 << 29) as f64 {
        return Err(vm.range_error("Invalid string length"));
    }
    Ok(Value::string(s.repeat(n as usize)))
}

fn concat(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "concat")?;
    let mut out = s.to_string();
    for v in &a.args {
        out.push_str(&vm.to_string(v)?);
    }
    Ok(Value::string(out))
}

fn split(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let sep = a.arg(0);
    if let Value::Obj(_) = &sep {
        let f = vm.get(&sep, &Key::Sym(vm.syms.split.clone()))?;
        if f.is_callable() {
            let this = a.this.clone();
            return vm.call(&f, sep, vec![this, a.arg(1)]);
        }
    }
    let s = this_str(vm, a, "split")?;
    let limit = if a.arg(1).is_undefined() {
        u32::MAX
    } else {
        vm.to_u32(&a.arg(1))?
    } as usize;
    if limit == 0 {
        return Ok(vm.arr(vec![]));
    }
    if sep.is_undefined() {
        return Ok(vm.arr(vec![Value::Str(s)]));
    }
    let sep = vm.to_string(&sep)?;
    let mut out = vec![];
    if sep.is_empty() {
        for u in s.utf16() {
            if out.len() >= limit {
                break;
            }
            out.push(Value::Str(JsStr::from_utf16(&[u])));
        }
        return Ok(vm.arr(out));
    }
    for part in s.split(sep.as_str()) {
        if out.len() >= limit {
            break;
        }
        out.push(Value::str(part));
    }
    Ok(vm.arr(out))
}

pub fn expand_replacement(tpl: &str, matched: &str, pos16: usize, s: &JsStr) -> String {
    let before = s.slice16(0, pos16);
    let after = s.slice16(pos16 + matched.encode_utf16().count(), s.len16());
    cw_regex::expand_js_replacement(
        tpl,
        matched,
        &before,
        &after,
        &[Some(matched.to_string())],
        &[],
    )
}

fn replace_impl(vm: &mut Vm, a: &mut Args, all: bool) -> JsResult<Value> {
    let pat = a.arg(0);
    if let Value::Obj(_) = &pat {
        if all && is_regexp(&pat) {
            let flags = vm.get_str(&pat, "flags")?;
            let fs = vm.to_str(&flags)?;
            if !fs.contains('g') {
                return Err(vm.type_error("replaceAll must be called with a global RegExp"));
            }
        }
        let f = vm.get(&pat, &Key::Sym(vm.syms.replace.clone()))?;
        if f.is_callable() {
            let this = a.this.clone();
            return vm.call(&f, pat, vec![this, a.arg(1)]);
        }
    }
    let s = this_str(vm, a, if all { "replaceAll" } else { "replace" })?;
    let p = vm.to_string(&pat)?;
    let rep = a.arg(1);
    let func = rep.is_callable();
    let tpl = if func {
        JsStr::new("")
    } else {
        vm.to_string(&rep)?
    };
    let mut positions = vec![];
    let plen = p.len16();
    let mut from = 0;
    while let Some(i) = index_of_16(&s, &p, from) {
        positions.push(i);
        if !all {
            break;
        }
        from = i + plen.max(1);
        if from > s.len16() {
            break;
        }
    }
    if positions.is_empty() {
        return Ok(Value::Str(s));
    }
    let mut out = String::new();
    let mut last = 0;
    for pos in positions {
        out.push_str(&s.slice16(last, pos));
        let r = if func {
            let v = vm.call(
                &rep,
                Value::Undefined,
                vec![
                    Value::Str(p.clone()),
                    Value::Num(pos as f64),
                    Value::Str(s.clone()),
                ],
            )?;
            vm.to_str(&v)?
        } else {
            expand_replacement(&tpl, &p, pos, &s)
        };
        out.push_str(&r);
        last = pos + plen;
    }
    out.push_str(&s.slice16(last, s.len16()));
    Ok(Value::string(out))
}

fn replace(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    replace_impl(vm, a, false)
}
fn replace_all(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    replace_impl(vm, a, true)
}

fn regexp_delegate(
    vm: &mut Vm,
    a: &mut Args,
    sym: std::rc::Rc<Symbol>,
    flags: &str,
    method: &str,
) -> JsResult<Value> {
    let pat = a.arg(0);
    if !pat.is_nullish() {
        let f = vm.get(&pat, &Key::Sym(sym.clone()))?;
        if f.is_callable() {
            if method == "matchAll" && is_regexp(&pat) {
                let fl = vm.get_str(&pat, "flags")?;
                let fs = vm.to_str(&fl)?;
                if !fs.contains('g') {
                    return Err(vm.type_error(
                        "String.prototype.matchAll called with a non-global RegExp argument",
                    ));
                }
            }
            let this = a.this.clone();
            return vm.call(&f, pat, vec![this]);
        }
    }
    let s = this_str(vm, a, method)?;
    let src = if pat.is_undefined() {
        JsStr::new("")
    } else {
        vm.to_string(&pat)?
    };
    let re = vm.regexp_create(&src, &JsStr::new(flags), None)?;
    let f = vm.get(&Value::Obj(re.clone()), &Key::Sym(sym))?;
    vm.call(&f, Value::Obj(re), vec![Value::Str(s)])
}

fn match_(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let sym = vm.syms.match_.clone();
    regexp_delegate(vm, a, sym, "", "match")
}
fn match_all(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let sym = vm.syms.match_all.clone();
    regexp_delegate(vm, a, sym, "g", "matchAll")
}
fn search(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let sym = vm.syms.search.clone();
    regexp_delegate(vm, a, sym, "", "search")
}

/// Simplified ICU-like collation key for `localeCompare`.
fn collate_key(c: char) -> (u32, u32, u32) {
    // (primary, secondary accent, tertiary case)
    let (base, accent) = strip_accent(c);
    let lower = base.to_lowercase().next().unwrap_or(base);
    let case = if base.is_uppercase() { 1 } else { 0 };
    let primary = if lower.is_whitespace() {
        1
    } else if lower.is_ascii_punctuation() || (!lower.is_alphanumeric() && (lower as u32) < 0x2000)
    {
        100 + lower as u32
    } else if lower.is_ascii_digit() {
        10_000 + lower as u32
    } else if lower.is_alphabetic() {
        20_000 + lower as u32
    } else {
        200_000 + lower as u32
    };
    (primary, accent, case)
}

fn strip_accent(c: char) -> (char, u32) {
    const MAP: &[(&str, char)] = &[
        ("àáâãäåā", 'a'),
        ("ÀÁÂÃÄÅĀ", 'A'),
        ("çćč", 'c'),
        ("ÇĆČ", 'C'),
        ("èéêëēė", 'e'),
        ("ÈÉÊËĒĖ", 'E'),
        ("ìíîïī", 'i'),
        ("ÌÍÎÏĪ", 'I'),
        ("ñń", 'n'),
        ("ÑŃ", 'N'),
        ("òóôõöøō", 'o'),
        ("ÒÓÔÕÖØŌ", 'O'),
        ("ùúûüū", 'u'),
        ("ÙÚÛÜŪ", 'U'),
        ("ýÿ", 'y'),
        ("ÝŸ", 'Y'),
        ("šś", 's'),
        ("ŠŚ", 'S'),
        ("žźż", 'z'),
        ("ŽŹŻ", 'Z'),
    ];
    for (set, base) in MAP {
        if let Some(i) = set.chars().position(|x| x == c) {
            return (*base, i as u32 + 1);
        }
    }
    (c, 0)
}

pub fn locale_compare_str(a: &str, b: &str) -> i32 {
    let ka: Vec<(u32, u32, u32)> = a.chars().map(collate_key).collect();
    let kb: Vec<(u32, u32, u32)> = b.chars().map(collate_key).collect();
    for level in 0..3 {
        let pa: Vec<u32> = ka.iter().map(|k| [k.0, k.1, k.2][level]).collect();
        let pb: Vec<u32> = kb.iter().map(|k| [k.0, k.1, k.2][level]).collect();
        match pa.cmp(&pb) {
            std::cmp::Ordering::Less => return -1,
            std::cmp::Ordering::Greater => return 1,
            _ => {}
        }
    }
    0
}

/// Collation with `numeric` (digit runs compare by value) and
/// `sensitivity: 'base' | 'accent'` options.
pub fn locale_compare_opts(a: &str, b: &str, numeric: bool, sensitivity: &str) -> i32 {
    let fold = |s: &str| -> String {
        match sensitivity {
            "base" => s
                .chars()
                .map(|c| strip_accent(c).0.to_lowercase().next().unwrap_or(c))
                .collect(),
            "accent" => s.to_lowercase(),
            _ => s.to_string(),
        }
    };
    let (a, b) = (fold(a), fold(b));
    if !numeric {
        return locale_compare_str(&a, &b);
    }
    let runs = |s: &str| -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = vec![];
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match out.last_mut() {
                Some((ld, r)) if *ld == d => r.push(c),
                _ => out.push((d, c.to_string())),
            }
        }
        out
    };
    let (ra, rb) = (runs(&a), runs(&b));
    for (x, y) in ra.iter().zip(rb.iter()) {
        if x.0 && y.0 {
            let (nx, ny) = (x.1.trim_start_matches('0'), y.1.trim_start_matches('0'));
            let c = nx.len().cmp(&ny.len()).then(nx.cmp(ny));
            match c {
                std::cmp::Ordering::Less => return -1,
                std::cmp::Ordering::Greater => return 1,
                _ => {}
            }
        } else {
            let c = locale_compare_str(&x.1, &y.1);
            if c != 0 {
                return c;
            }
        }
    }
    match ra.len().cmp(&rb.len()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        _ => locale_compare_str(&a, &b),
    }
}

fn locale_compare(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "localeCompare")?;
    let o = str_arg(vm, a, 0)?;
    let opts = a.arg(2);
    let (mut numeric, mut sens) = (false, String::from("variant"));
    if let Value::Obj(_) = &opts {
        numeric = vm.get_str(&opts, "numeric")?.truthy();
        let sv = vm.get_str(&opts, "sensitivity")?;
        if let Value::Str(x) = sv {
            sens = x.to_string();
        }
    }
    Ok(Value::Num(
        locale_compare_opts(&s, &o, numeric, &sens) as f64
    ))
}

fn normalize(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "normalize")?;
    if !a.arg(0).is_undefined() {
        let f = str_arg(vm, a, 0)?;
        if !matches!(f.as_str(), "NFC" | "NFD" | "NFKC" | "NFKD") {
            return Err(
                vm.range_error("The normalization form should be one of NFC, NFD, NFKC, NFKD.")
            );
        }
    }
    Ok(Value::Str(s))
}

fn to_string(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    match &a.this {
        Value::Str(s) => Ok(Value::Str(s.clone())),
        Value::Obj(o) => match &o.borrow().kind {
            Kind::String(s) => Ok(Value::Str(s.clone())),
            _ => Err(vm.type_error("String.prototype.toString requires that 'this' be a String")),
        },
        _ => Err(vm.type_error("String.prototype.toString requires that 'this' be a String")),
    }
}

fn is_well_formed(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let _ = this_str(vm, a, "isWellFormed")?;
    Ok(Value::Bool(true))
}

fn string_iterator(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = this_str(vm, a, "[Symbol.iterator]")?;
    Ok(Value::Obj(vm.obj_with(
        Some(vm.intr.string_iter_proto.clone()),
        Kind::StringIter {
            s,
            pos: 0,
            done: false,
        },
    )))
}

fn string_iter_next(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = &a.this else {
        return Err(vm.type_error("next method called on incompatible receiver"));
    };
    let r = {
        let mut d = o.borrow_mut();
        match &mut d.kind {
            Kind::StringIter { s, pos, done } => {
                if *done {
                    None
                } else {
                    let b = s.byte_offset(*pos);
                    match s.as_str()[b..].chars().next() {
                        Some(c) => {
                            *pos += c.len_utf16();
                            Some(c.to_string())
                        }
                        None => {
                            *done = true;
                            None
                        }
                    }
                }
            }
            _ => None,
        }
    };
    Ok(match r {
        Some(c) => vm.iter_result(Value::string(c), false),
        None => vm.iter_result(Value::Undefined, true),
    })
}

pub fn install(vm: &mut Vm) {
    let proto = vm.intr.string_proto.clone();
    let ctor = vm.make_ctor("String", 1, string_ctor, &proto);
    vm.set_global("String", Value::Obj(ctor.clone()));
    vm.method(&ctor, "fromCharCode", 1, from_char_code);
    vm.method(&ctor, "fromCodePoint", 1, from_code_point);
    vm.method(&ctor, "raw", 1, raw);
    let m: &[(&str, u32, NativeFn)] = &[
        ("at", 1, at),
        ("charAt", 1, char_at),
        ("charCodeAt", 1, char_code_at),
        ("codePointAt", 1, code_point_at),
        ("concat", 1, concat),
        ("endsWith", 1, ends_with),
        ("includes", 1, includes),
        ("indexOf", 1, index_of),
        ("isWellFormed", 0, is_well_formed),
        ("lastIndexOf", 1, last_index_of),
        ("localeCompare", 1, locale_compare),
        ("match", 1, match_),
        ("matchAll", 1, match_all),
        ("normalize", 0, normalize),
        ("padEnd", 1, pad_end),
        ("padStart", 1, pad_start),
        ("repeat", 1, repeat),
        ("replace", 2, replace),
        ("replaceAll", 2, replace_all),
        ("search", 1, search),
        ("slice", 2, slice),
        ("split", 2, split),
        ("startsWith", 1, starts_with),
        ("substr", 2, substr),
        ("substring", 2, substring),
        ("toLocaleLowerCase", 0, to_lower),
        ("toLocaleUpperCase", 0, to_upper),
        ("toLowerCase", 0, to_lower),
        ("toString", 0, to_string),
        ("toUpperCase", 0, to_upper),
        ("trim", 0, trim),
        ("trimEnd", 0, trim_end),
        ("trimStart", 0, trim_start),
        ("valueOf", 0, to_string),
    ];
    for (n, l, f) in m {
        vm.method(&proto, n, *l, *f);
    }
    // Legacy aliases share the same function objects in V8.
    let te = proto.own_value("trimEnd").unwrap();
    proto.set_hidden("trimRight", te);
    let ts = proto.own_value("trimStart").unwrap();
    proto.set_hidden("trimLeft", ts);
    let it = vm.syms.iterator.clone();
    vm.method_sym(&proto, &it, "[Symbol.iterator]", 0, string_iterator);
    let sip = vm.intr.string_iter_proto.clone();
    vm.method(&sip, "next", 0, string_iter_next);
    let tag = vm.syms.to_string_tag.clone();
    sip.set_sym(&tag, Value::str("String Iterator"), CONFIGURABLE);
}
